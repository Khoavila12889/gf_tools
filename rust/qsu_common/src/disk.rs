//! ĐĨA VẬT LÝ + chế độ XOÁ TOÀN BỘ Ổ ĐĨA ("Cài mới hoàn toàn").
//!
//! Bổ sung cho `sys.rs` (vốn chỉ liệt kê VOLUME có ký tự ổ): module này
//! liệt kê từng ĐĨA VẬT LÝ (PhysicalDriveN) kèm serial phần cứng, model,
//! dung lượng và danh sách phân vùng ĐẦY ĐỦ (kể cả phân vùng không ký
//! tự như EFI/MSR/Recovery) — toàn bộ bằng IOCTL gốc, không WMI, không
//! phân tích output diskpart (bị bản địa hoá):
//!
//! - Serial/model: IOCTL_STORAGE_QUERY_PROPERTY (STORAGE_DEVICE_DESCRIPTOR
//!   được phân tích theo offset tài liệu vì winapi 0.3 không định nghĩa
//!   struct này).
//! - Dung lượng đĩa: IOCTL_DISK_GET_LENGTH_INFO.
//! - Phân vùng + loại (EFI/MSR/Recovery): IOCTL_DISK_GET_DRIVE_LAYOUT_EX.
//! - Ký tự ổ/nhãn/serial volume: liệt kê mọi volume bằng
//!   FindFirstVolumeW + IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS để gắn
//!   volume vào đúng (đĩa, offset) của phân vùng trong layout.
//!
//! Liệt kê (`enumerate_disks`) có chẩn đoán + dự phòng: tiêu chí tồn
//! tại của đĩa là mở được handle (không còn phụ thuộc một IOCTL
//! dung lượng duy nhất như bản đầu), và khi đường IOCTL thiếu đĩa
//! hoặc thiếu serial thì hợp nhất thêm PowerShell Get-Disk (JSON)
//! và diskpart (list/detail — phân tích theo mẫu, không theo nhãn
//! bản địa hoá). Mọi lỗi đều được ghi vào `DiskEnumeration::diag`.
//!
//! Phần WIPE sinh script diskpart chia lại đĩa từ đầu (GPT: EFI 260MB /
//! MSR 16MB / Windows / Recovery 600MB chuẩn Microsoft; MBR: System
//! Reserved 100MB / Windows / Recovery 600MB), chạy script, rồi xác
//! minh lại bằng liệt kê thật. Đĩa đích luôn được nhận diện bằng SERIAL
//! PHẦN CỨNG — không bao giờ chỉ bằng số thứ tự hay ký tự ổ.

use std::ptr;
use std::sync::Arc;

use winapi::shared::guiddef::GUID;
use winapi::shared::minwindef::DWORD;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::fileapi::{
    CreateFileW, FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, GetFileAttributesW,
    GetLogicalDrives, GetVolumeInformationW, GetVolumePathNamesForVolumeNameW,
    INVALID_FILE_ATTRIBUTES, OPEN_EXISTING,
};
use winapi::um::winbase::SetVolumeMountPointW;
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::ioapiset::DeviceIoControl;
use winapi::um::winioctl::{
    DRIVE_LAYOUT_INFORMATION_EX, GET_LENGTH_INFORMATION, IOCTL_DISK_GET_DRIVE_LAYOUT_EX,
    IOCTL_DISK_GET_LENGTH_INFO, IOCTL_STORAGE_QUERY_PROPERTY, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
    PARTITION_INFORMATION_EX, VOLUME_DISK_EXTENTS,
};
use winapi::um::winnt::{FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ, HANDLE};

use crate::cmd;
use crate::log::Logger;
use crate::sys::{self, PartitionStyle};

const GB: u64 = 1_073_741_824;
const MB: u64 = 1_048_576;

/// Dung lượng tối thiểu của đĩa đích wipe (đặc tả §6b): dưới mức này
/// phân vùng Windows sau khi chia sẽ quá chật, từ chối ngay từ đầu.
pub const MIN_WIPE_DISK_BYTES: u64 = 25 * GB;

// GUID loại phân vùng GPT (dạng chuỗi chuẩn hoá để so sánh)
const GPT_EFI: &str = "C12A7328-F81F-11D2-BA4B-00A0C93EC93B";
const GPT_MSR: &str = "E3C9E316-0B5C-4DB8-817D-F92DF00215AE";
const GPT_RECOVERY: &str = "DE94BBA4-06D1-4D40-A16A-BFD50179D6AC";

// ---------------------------------------------------------------------------
// Kiểu dữ liệu
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PartKind {
    Normal,
    Efi,
    Msr,
    Recovery,
    System, // System Reserved (MBR, có cờ boot)
    Unknown,
}

impl PartKind {
    pub fn label_vi(&self) -> &'static str {
        match self {
            PartKind::Normal => "Phân vùng thường",
            PartKind::Efi => "EFI System",
            PartKind::Msr => "MSR (đệm Microsoft)",
            PartKind::Recovery => "Recovery",
            PartKind::System => "System Reserved",
            PartKind::Unknown => "Không rõ loại",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiskPartition {
    pub number: u32,
    pub offset: u64,
    pub size: u64,
    pub kind: PartKind,
    /// Ký tự ổ nếu phân vùng đang được gán (EFI/MSR/Recovery thường None)
    pub letter: Option<char>,
    pub label: String,
    pub fs: String,
    /// Serial của volume (0 nếu không đọc được — MSR không có volume)
    pub volume_serial: u32,
    /// GUID DUY NHẤT của phân vùng trên đĩa GPT (PartitionId) — định
    /// danh vật lý ổn định dùng cho chế độ "rebuild" (v0.7.13) để
    /// đối chiếu phân vùng cần xoá mà không phụ thuộc ký tự ổ/nhãn.
    /// Rỗng với đĩa MBR và với phân vùng đến từ đường liệt kê dự
    /// phòng (diskpart không cho thông tin này).
    pub part_guid: String,
}

impl DiskPartition {
    /// Một dòng mô tả phân vùng cho danh sách cảnh báo xoá dữ liệu.
    pub fn describe(&self) -> String {
        format!(
            "{} — {} — {} — {}",
            self.letter.map(|c| format!("{}:", c)).unwrap_or_else(|| "(không ký tự ổ)".to_string()),
            if self.label.is_empty() { "(không nhãn)".to_string() } else { self.label.clone() },
            sys::format_gb(self.size),
            self.kind.label_vi()
        )
    }
}

#[derive(Debug, Clone)]
pub struct PhysicalDisk {
    pub number: u32,
    /// Serial phần cứng của đĩa (đã cắt khoảng trắng đầu/cuối)
    pub serial: String,
    pub model: String,
    pub size: u64,
    pub style: PartitionStyle,
    pub partitions: Vec<DiskPartition>,
    /// true khi định danh của đĩa đến từ đường DỰ PHÒNG (PowerShell
    /// Get-Disk / diskpart) vì đường IOCTL không đọc được — danh sách
    /// phân vùng khi đó có thể không đầy đủ (chỉ gồm các volume có ký
    /// tự ổ đọc được qua diskpart) nên phía gọi phải thận trọng khi
    /// suy luận "đĩa không chứa ổ X" từ danh sách phân vùng rỗng.
    pub from_fallback: bool,
}

impl PhysicalDisk {
    pub fn contains_letter(&self, letter: char) -> bool {
        self.partitions.iter().any(|p| p.letter == Some(letter))
    }
    /// Danh sách mọi phân vùng trên đĩa (mỗi phân vùng một dòng) — dùng
    /// cho cảnh báo "toàn bộ dữ liệu trên các phân vùng này sẽ bị xoá".
    pub fn describe_partitions(&self) -> String {
        if self.partitions.is_empty() {
            return "(đĩa chưa có phân vùng nào)".to_string();
        }
        self.partitions
            .iter()
            .map(|p| format!("  - {}", p.describe()))
            .collect::<Vec<_>>()
            .join("\n")
    }
    /// Dòng tóm tắt một đĩa cho danh sách chọn (combo/list).
    pub fn summary(&self) -> String {
        format!(
            "Đĩa {} — {} — {} — serial {} — {} ({} phân vùng){}",
            self.number,
            if self.model.is_empty() { "(không rõ model)" } else { self.model.as_str() },
            sys::format_gb(self.size),
            if self.serial.is_empty() { "(không đọc được)" } else { self.serial.as_str() },
            match self.style {
                PartitionStyle::Gpt => "GPT",
                PartitionStyle::Mbr => "MBR",
                PartitionStyle::Unknown => "chưa rõ kiểu",
            },
            self.partitions.len(),
            if self.from_fallback { " — (liệt kê dự phòng)" } else { "" }
        )
    }
}

/// Chuẩn hoá serial đĩa để so sánh (serial người dùng gõ tay, serial
/// trong stage.json, serial đọc trực tiếp): chỉ giữ ký tự chữ/số và
/// viết hoa — bỏ qua khác biệt khoảng trắng/dấu gạch do cách hiển thị.
pub fn normalize_disk_serial(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase()
}

/// Tìm đĩa theo serial (so sánh đã chuẩn hoá). Chỉ trả về khi khớp
/// ĐÚNG MỘT đĩa — khớp nhiều đĩa (serial trùng bất thường) coi như
/// không xác định được, thà dừng còn hơn wipe nhầm.
pub fn find_disk_by_serial<'a>(disks: &'a [PhysicalDisk], serial: &str) -> Option<&'a PhysicalDisk> {
    let want = normalize_disk_serial(serial);
    if want.is_empty() {
        return None;
    }
    let mut hits = disks
        .iter()
        .filter(|d| normalize_disk_serial(&d.serial) == want);
    let first = hits.next()?;
    if hits.next().is_some() {
        return None;
    }
    Some(first)
}

// ---------------------------------------------------------------------------
// Truy vấn IOCTL mức thấp
// ---------------------------------------------------------------------------
/// Mở handle và giữ lại GetLastError khi thất bại — liệt kê đĩa cần
/// mã lỗi thật để chẩn đoán (bản cũ nuốt hết lỗi nên đĩa "biến mất"
/// khỏi danh sách mà không để lại dấu vết nào).
fn open_path_err(path: &str, access: u32) -> Result<HANDLE, DWORD> {
    unsafe {
        let w = sys::wide(path);
        let h = CreateFileW(
            w.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null_mut(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            Err(GetLastError())
        } else {
            Ok(h)
        }
    }
}

fn open_path(path: &str, access: u32) -> Option<HANDLE> {
    open_path_err(path, access).ok()
}

/// Như `ioctl` nhưng trả GetLastError (đọc NGAY sau DeviceIoControl,
/// trước khi CloseHandle kịp ghi đè).
fn ioctl_err(handle: HANDLE, code: u32, input: &[u8], output: &mut [u8]) -> Result<usize, DWORD> {
    unsafe {
        let mut returned: DWORD = 0;
        let ok = DeviceIoControl(
            handle,
            code,
            input.as_ptr() as *mut winapi::ctypes::c_void,
            input.len() as DWORD,
            output.as_mut_ptr() as *mut winapi::ctypes::c_void,
            output.len() as DWORD,
            &mut returned,
            ptr::null_mut(),
        );
        if ok == 0 {
            Err(GetLastError())
        } else {
            Ok(returned as usize)
        }
    }
}

fn ioctl(handle: HANDLE, code: u32, input: &[u8], output: &mut [u8]) -> Option<usize> {
    ioctl_err(handle, code, input, output).ok()
}

fn read_c_string(buf: &[u8], offset: usize) -> String {
    if offset == 0 || offset >= buf.len() {
        return String::new();
    }
    let end = buf[offset..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| offset + p)
        .unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[offset..end]).trim().to_string()
}

fn read_u32(buf: &[u8], offset: usize) -> u32 {
    if offset + 4 > buf.len() {
        return 0;
    }
    u32::from_le_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]])
}

/// Serial phần cứng + model của PhysicalDriveN qua
/// IOCTL_STORAGE_QUERY_PROPERTY. STORAGE_DEVICE_DESCRIPTOR (offset
/// theo tài liệu Microsoft): VendorIdOffset @12, ProductIdOffset @16,
/// ProductRevisionOffset @20, SerialNumberOffset @24.
/// Như `query_serial_model` nhưng kèm ghi chú chẩn đoán khi không
/// đọc được (mở handle lỗi mã bao nhiêu / IOCTL lỗi mã bao nhiêu /
/// descriptor có offset rỗng) — phục vụ log liệt kê đĩa.
fn query_serial_model_full(number: u32) -> (String, String, Option<String>) {
    let path = format!("\\\\.\\PhysicalDrive{}", number);
    // IOCTL này cần quyền đọc; dự phòng mở không quyền nếu bị chặn.
    let handle = match open_path_err(&path, GENERIC_READ).or_else(|_| open_path_err(&path, 0)) {
        Ok(h) => h,
        Err(e) => {
            return (
                String::new(),
                String::new(),
                Some(format!("mở {} để đọc serial/model thất bại (GetLastError={})", path, e)),
            )
        }
    };
    // STORAGE_PROPERTY_QUERY: PropertyId=0 (StorageDeviceProperty),
    // QueryType=0 (PropertyStandardQuery)
    let query = [0u8; 12];
    let mut buf = vec![0u8; 4096];
    let res = ioctl_err(handle, IOCTL_STORAGE_QUERY_PROPERTY, &query, &mut buf);
    unsafe { CloseHandle(handle) };
    if let Err(e) = res {
        return (
            String::new(),
            String::new(),
            Some(format!("IOCTL_STORAGE_QUERY_PROPERTY lỗi (GetLastError={})", e)),
        );
    }
    let model_off = read_u32(&buf, 16) as usize;
    let serial_off = read_u32(&buf, 24) as usize;
    let model = read_c_string(&buf, model_off);
    let serial = read_c_string(&buf, serial_off);
    let note = if serial.is_empty() && model.is_empty() {
        Some(format!(
            "descriptor không chứa chuỗi serial/model (offset serial={}, model={}) — driver lưu trữ không cung cấp",
            serial_off, model_off
        ))
    } else if serial.is_empty() {
        Some(format!("descriptor có model nhưng offset serial={} (rỗng)", serial_off))
    } else {
        None
    };
    (serial, model, note)
}

/// Dung lượng đĩa + mã lỗi khi không đọc được (None = đọc thành công).
fn query_disk_length(number: u32) -> (u64, Option<DWORD>) {
    let path = format!("\\\\.\\PhysicalDrive{}", number);
    let handle = match open_path_err(&path, 0) {
        Ok(h) => h,
        Err(e) => return (0, Some(e)),
    };
    let mut buf = [0u8; 8];
    let res = ioctl_err(handle, IOCTL_DISK_GET_LENGTH_INFO, &[], &mut buf);
    unsafe { CloseHandle(handle) };
    match res {
        Ok(_) => {
            let info = buf.as_ptr() as *const GET_LENGTH_INFORMATION;
            (unsafe { *(*info).Length.QuadPart() as u64 }, None)
        }
        Err(e) => (0, Some(e)),
    }
}

fn guid_string(g: &GUID) -> String {
    format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        g.Data1, g.Data2, g.Data3, g.Data4[0], g.Data4[1], g.Data4[2], g.Data4[3], g.Data4[4],
        g.Data4[5], g.Data4[6], g.Data4[7]
    )
}

struct RawPart {
    offset: u64,
    size: u64,
    number: u32,
    kind: PartKind,
    /// GUID duy nhất của phân vùng (chỉ GPT; MBR để rỗng)
    part_guid: String,
}

/// Layout phân vùng của PhysicalDriveN qua IOCTL_DISK_GET_DRIVE_LAYOUT_EX
/// (buffer lớn như sys::disk_partition_style — IOCTL này cần buffer đủ
/// cho toàn bộ layout).
fn query_layout(number: u32) -> (PartitionStyle, Vec<RawPart>, Option<DWORD>) {
    let path = format!("\\\\.\\PhysicalDrive{}", number);
    let handle = match open_path_err(&path, 0) {
        Ok(h) => h,
        Err(e) => return (PartitionStyle::Unknown, Vec::new(), Some(e)),
    };
    let mut buf = vec![0u8; 65536];
    let res = ioctl_err(handle, IOCTL_DISK_GET_DRIVE_LAYOUT_EX, &[], &mut buf);
    unsafe { CloseHandle(handle) };
    if let Err(e) = res {
        return (PartitionStyle::Unknown, Vec::new(), Some(e));
    }
    unsafe {
        let layout = buf.as_ptr() as *const DRIVE_LAYOUT_INFORMATION_EX;
        let style = match (*layout).PartitionStyle {
            1 => PartitionStyle::Gpt,
            0 => PartitionStyle::Mbr,
            _ => PartitionStyle::Unknown,
        };
        let count = ((*layout).PartitionCount as usize).min(128);
        let base = &(*layout).PartitionEntry[0] as *const PARTITION_INFORMATION_EX;
        let mut parts = Vec::new();
        for i in 0..count {
            let e = &*base.add(i);
            let (kind, part_guid) = match e.PartitionStyle {
                1 => {
                    // GPT: loại theo GUID loại phân vùng; đồng thời
                    // giữ GUID DUY NHẤT của phân vùng (PartitionId)
                    // cho chế độ rebuild đối chiếu khi xoá.
                    let k = match guid_string(&e.u.Gpt().PartitionType).as_str() {
                        GPT_EFI => PartKind::Efi,
                        GPT_MSR => PartKind::Msr,
                        GPT_RECOVERY => PartKind::Recovery,
                        _ => PartKind::Normal,
                    };
                    (k, guid_string(&e.u.Gpt().PartitionId))
                }
                0 => {
                    // MBR: loại theo partition type + cờ boot
                    let m = e.u.Mbr();
                    let k = if m.PartitionType == 0x27 {
                        PartKind::Recovery
                    } else if m.PartitionType == 0xEF {
                        PartKind::Efi
                    } else if m.BootIndicator != 0 {
                        PartKind::System
                    } else {
                        PartKind::Normal
                    };
                    (k, String::new())
                }
                _ => (PartKind::Unknown, String::new()),
            };
            parts.push(RawPart {
                offset: *e.StartingOffset.QuadPart() as u64,
                size: *e.PartitionLength.QuadPart() as u64,
                number: e.PartitionNumber,
                kind,
                part_guid,
            });
        }
        (style, parts, None)
    }
}

// ---------------------------------------------------------------------------
// Gắn volume (ký tự ổ/nhãn/serial) vào phân vùng theo (đĩa, offset)
// ---------------------------------------------------------------------------
struct VolumeMatch {
    disk: u32,
    offset: u64,
    size: u64,
    letter: Option<char>,
    label: String,
    fs: String,
    serial: u32,
}

/// Extent đầu tiên của một volume path (đĩa + offset + độ dài).
fn volume_extent(volume_path: &str) -> Option<(u32, u64, u64)> {
    let trimmed = volume_path.trim_end_matches('\\');
    let handle = open_path(trimmed, 0)?;
    let mut buf = vec![0u8; 1024];
    let res = ioctl(handle, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, &[], &mut buf);
    unsafe { CloseHandle(handle) };
    res?;
    unsafe {
        let ext = buf.as_ptr() as *const VOLUME_DISK_EXTENTS;
        if (*ext).NumberOfDiskExtents == 0 {
            return None;
        }
        let e = &(*ext).Extents[0];
        Some((
            e.DiskNumber,
            *e.StartingOffset.QuadPart() as u64,
            *e.ExtentLength.QuadPart() as u64,
        ))
    }
}

/// Số thứ tự đĩa vật lý chứa một volume theo ký tự ổ (None nếu volume
/// không nằm trên đĩa vật lý đơn lẻ — dynamic/striped...).
pub fn volume_disk_number(letter: char) -> Option<u32> {
    let path = format!("\\\\.\\{}:", letter.to_ascii_uppercase());
    let handle = open_path(&path, 0)?;
    let mut buf = vec![0u8; 1024];
    let res = ioctl(handle, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, &[], &mut buf);
    unsafe { CloseHandle(handle) };
    res?;
    unsafe {
        let ext = buf.as_ptr() as *const VOLUME_DISK_EXTENTS;
        if (*ext).NumberOfDiskExtents == 0 {
            None
        } else {
            Some((*ext).Extents[0].DiskNumber)
        }
    }
}

/// Extent vật lý (số đĩa + offset + độ dài) của volume theo ký tự ổ.
/// Chế độ "rebuild" dùng để chứng minh phân vùng chứa staging KHÔNG
/// nằm trong danh sách xoá khi đĩa đích trùng đĩa staging.
pub fn volume_disk_extent(letter: char) -> Option<(u32, u64, u64)> {
    let path = format!("\\\\.\\{}:", letter.to_ascii_uppercase());
    volume_extent(&path)
}

/// Liệt kê mọi volume trong hệ thống (kể cả không ký tự ổ) kèm vị trí
/// vật lý của chúng.
fn enumerate_volume_matches() -> (Vec<VolumeMatch>, Option<String>) {
    let mut out = Vec::new();
    unsafe {
        let mut name_buf = vec![0u16; 1024];
        let find = FindFirstVolumeW(name_buf.as_mut_ptr(), name_buf.len() as DWORD);
        if find == INVALID_HANDLE_VALUE {
            let e = GetLastError();
            return (
                out,
                Some(format!(
                    "FindFirstVolumeW thất bại (GetLastError={}) — không gắn được ký tự ổ vào phân vùng",
                    e
                )),
            );
        }
        loop {
            let volume_path = sys::from_wide(&name_buf);
            // Ký tự ổ (đường dẫn đầu tiên dạng "X:\")
            let mut path_buf = vec![0u16; 1024];
            let mut needed: DWORD = 0;
            let mut letter = None;
            if GetVolumePathNamesForVolumeNameW(
                name_buf.as_ptr(),
                path_buf.as_mut_ptr(),
                path_buf.len() as DWORD,
                &mut needed,
            ) != 0
            {
                let first = sys::from_wide(&path_buf);
                let mut chars = first.chars();
                if let (Some(c), Some(':')) = (chars.next(), chars.next()) {
                    if c.is_ascii_alphabetic() {
                        letter = Some(c.to_ascii_uppercase());
                    }
                }
            }
            // Nhãn / serial / hệ file (volume RAW/chưa format sẽ lỗi — bỏ qua phần này)
            let root_w = sys::wide(&volume_path);
            let mut label_buf = vec![0u16; 256];
            let mut serial: DWORD = 0;
            let mut fs_buf = vec![0u16; 64];
            let (mut label, mut fs, mut vol_serial) = (String::new(), String::new(), 0u32);
            if GetVolumeInformationW(
                root_w.as_ptr(),
                label_buf.as_mut_ptr(),
                label_buf.len() as DWORD,
                &mut serial,
                ptr::null_mut(),
                ptr::null_mut(),
                fs_buf.as_mut_ptr(),
                fs_buf.len() as DWORD,
            ) != 0
            {
                label = sys::from_wide(&label_buf);
                fs = sys::from_wide(&fs_buf);
                vol_serial = serial;
            }
            if let Some((disk, offset, size)) = volume_extent(&volume_path) {
                out.push(VolumeMatch {
                    disk,
                    offset,
                    size,
                    letter,
                    label,
                    fs,
                    serial: vol_serial,
                });
            }
            // Sang volume tiếp theo (xoá buffer để tránh sót ký tự)
            for v in name_buf.iter_mut() {
                *v = 0;
            }
            if FindNextVolumeW(find, name_buf.as_mut_ptr(), name_buf.len() as DWORD) == 0 {
                let _ = GetLastError();
                break;
            }
        }
        FindVolumeClose(find);
    }
    (out, None)
}

/// Kết quả liệt kê đĩa kèm chẩn đoán: mỗi quyết định (đĩa nào được
/// tính là tồn tại, IOCTL nào lỗi mã bao nhiêu, đường dự phòng nào
/// đã chạy) đều có một dòng trong `diag` để phía gọi ghi vào log —
/// bản cũ nuốt toàn bộ lỗi nên đĩa biến mất không dấu vết.
pub struct DiskEnumeration {
    pub disks: Vec<PhysicalDisk>,
    pub diag: Vec<String>,
}

/// Liệt kê toàn bộ đĩa vật lý (tối đa 16 đĩa đầu) kèm phân vùng đầy
/// đủ. Giữ chữ ký cũ cho engine WinPE + module WinPE; phía cần log
/// chẩn đoán dùng `enumerate_disks`.
pub fn list_physical_disks() -> Vec<PhysicalDisk> {
    enumerate_disks().disks
}

/// Liệt kê đĩa HAI đường, hợp nhất kết quả:
/// 1) IOCTL gốc (nhanh, đầy đủ nhất): tiêu chí tồn tại của một đĩa
///    là MỞ ĐƯỢC handle `\\.\PhysicalDriveN` — bản cũ còn đòi thêm
///    IOCTL_DISK_GET_LENGTH_INFO thành công mới tính là có đĩa, nên
///    một driver lưu trữ từ chối IOCTL đó (RAID/RST, card reader,
///    một số controller USB) là cả đĩa biến mất khỏi danh sách.
///    Nay dung lượng đọc không được thì suy từ tổng phân vùng; mọi
///    lỗi đều ghi vào `diag` kèm GetLastError.
/// 2) Dự phòng khi đường 1 thiếu đĩa hoặc thiếu serial/dung lượng:
///    PowerShell Get-Disk (JSON — tên thuộc tính cố định tiếng Anh,
///    không phụ thuộc ngôn ngữ hiển thị) rồi tới diskpart (phân tích
///    theo MẪU số/đơn vị/GUID, không theo nhãn đã bản địa hoá). Đĩa
///    chỉ thấy qua dự phòng có `from_fallback = true` và serial lấy
///    từ Disk ID của diskpart (tiền tố "DP-") khi không có serial
///    phần cứng — wizard và engine dùng chung hàm này nên định danh
///    hai phía khớp nhau.
pub fn enumerate_disks() -> DiskEnumeration {
    let mut diag: Vec<String> = Vec::new();
    let (matches, vol_note) = enumerate_volume_matches();
    if let Some(n) = vol_note {
        diag.push(format!("Liệt kê đĩa (IOCTL): {}", n));
    }
    let mut disks: Vec<PhysicalDisk> = Vec::new();
    for number in 0..16u32 {
        // Mở được handle = đĩa tồn tại. Không còn vòng dừng sớm theo
        // số lần lỗi liên tiếp: quét đủ 16 số (mở handle lỗi rất rẻ)
        // để một dãy thiết bị "ma" (đầu đọc thẻ rỗng...) ở số thấp
        // không che mất đĩa thật ở số cao hơn.
        let handle = match open_path_err(&format!("\\\\.\\PhysicalDrive{}", number), 0) {
            Ok(h) => h,
            Err(e) => {
                // 2 = ERROR_FILE_NOT_FOUND: số đĩa không tồn tại —
                // chỉ ghi log cho 4 số đầu để log gọn; các mã lỗi
                // khác (5 = Access Denied...) luôn ghi vì bất thường.
                if number < 4 || e != 2 {
                    diag.push(format!(
                        "PhysicalDrive{}: không mở được handle (GetLastError={}) — coi như không có đĩa số này.",
                        number, e
                    ));
                }
                continue;
            }
        };
        unsafe { CloseHandle(handle) };
        let (len, len_err) = query_disk_length(number);
        let (serial, model, sm_note) = query_serial_model_full(number);
        let (style, raw_parts, layout_err) = query_layout(number);
        let mut partitions: Vec<DiskPartition> = raw_parts
            .into_iter()
            .map(|rp| {
                // Gắn volume theo (đĩa, offset); dự phòng khớp theo
                // (đĩa, dung lượng) khi offset lệch nhẹ do làm tròn.
                let m = matches
                    .iter()
                    .find(|m| m.disk == number && m.offset == rp.offset)
                    .or_else(|| {
                        matches.iter().find(|m| {
                            m.disk == number
                                && m.size == rp.size
                                && m.offset.abs_diff(rp.offset) <= MB
                        })
                    });
                DiskPartition {
                    number: rp.number,
                    offset: rp.offset,
                    size: rp.size,
                    kind: rp.kind,
                    letter: m.and_then(|m| m.letter),
                    label: m.map(|m| m.label.clone()).unwrap_or_default(),
                    fs: m.map(|m| m.fs.clone()).unwrap_or_default(),
                    volume_serial: m.map(|m| m.serial).unwrap_or(0),
                    part_guid: rp.part_guid.clone(),
                }
            })
            .collect();
        partitions.sort_by_key(|p| p.offset);
        // Dung lượng: ưu tiên GET_LENGTH_INFO; IOCTL đó lỗi thì suy
        // từ tổng dung lượng các phân vùng đọc được.
        let mut size = len;
        if size == 0 {
            size = partitions.iter().map(|p| p.size).sum();
        }
        let mut notes: Vec<String> = Vec::new();
        if let Some(e) = len_err {
            notes.push(format!("IOCTL_DISK_GET_LENGTH_INFO lỗi (GetLastError={})", e));
        }
        if let Some(n) = sm_note {
            notes.push(n);
        }
        if let Some(e) = layout_err {
            notes.push(format!(
                "IOCTL_DISK_GET_DRIVE_LAYOUT_EX lỗi (GetLastError={}) — không đọc được bảng phân vùng",
                e
            ));
        }
        diag.push(format!(
            "Đĩa {} (IOCTL): model '{}' — {} — serial '{}' — {} phân vùng{}",
            number,
            if model.is_empty() { "(trống)" } else { model.as_str() },
            sys::format_gb(size),
            if serial.is_empty() { "(trống)" } else { serial.as_str() },
            partitions.len(),
            if notes.is_empty() {
                String::new()
            } else {
                format!(" [{}]", notes.join("; "))
            }
        ));
        disks.push(PhysicalDisk {
            number,
            serial,
            model,
            size,
            style,
            partitions,
            from_fallback: false,
        });
    }
    // Đường dự phòng: chỉ chạy khi đường IOCTL thiếu đĩa, hoặc có
    // đĩa thiếu serial/dung lượng (định danh wipe bắt buộc có serial).
    let need_fallback = disks.is_empty()
        || disks
            .iter()
            .any(|d| d.serial.trim().is_empty() || d.size == 0);
    if need_fallback {
        diag.push(
            "Liệt kê IOCTL chưa đủ (thiếu đĩa, hoặc có đĩa thiếu serial/dung lượng) — chạy đường dự phòng PowerShell Get-Disk, sau đó diskpart nếu vẫn thiếu."
                .to_string(),
        );
        merge_powershell_disks(&mut disks, &mut diag);
        merge_diskpart_disks(&mut disks, &mut diag);
    }
    disks.sort_by_key(|d| d.number);
    diag.push(format!(
        "Liệt kê đĩa: tổng {} đĩa vật lý ({} qua IOCTL, {} qua dự phòng).",
        disks.len(),
        disks.iter().filter(|d| !d.from_fallback).count(),
        disks.iter().filter(|d| d.from_fallback).count()
    ));
    DiskEnumeration { disks, diag }
}

// ---------------------------------------------------------------------------
// Dự phòng 1 — PowerShell Get-Disk (JSON)
// ---------------------------------------------------------------------------
/// Bổ sung định danh (serial/model/dung lượng/kiểu) từ
/// `Get-Disk | ConvertTo-Json` — tên thuộc tính là tiếng Anh cố định
/// nên không dính bản địa hoá. Chỉ ĐIỀN CHỖ TRỐNG của đường IOCTL và
/// thêm đĩa mà IOCTL không mở được; không ghi đè dữ liệu IOCTL.
fn merge_powershell_disks(disks: &mut Vec<PhysicalDisk>, diag: &mut Vec<String>) {
    let script = "Get-Disk | Select-Object Number,SerialNumber,FriendlyName,Size,PartitionStyle | ConvertTo-Json -Compress";
    let out = match cmd::run_capture("powershell", &["-NoProfile", "-Command", script]) {
        Ok(o) => o,
        Err(e) => {
            diag.push(format!(
                "Dự phòng PowerShell Get-Disk: không chạy được powershell ({}) — bỏ qua đường này.",
                e
            ));
            return;
        }
    };
    if out.code != Some(0) || out.stdout.trim().is_empty() {
        diag.push(format!(
            "Dự phòng PowerShell Get-Disk: thoát mã {:?}, stdout rỗng={} — stderr: {}",
            out.code,
            out.stdout.trim().is_empty(),
            out.stderr.trim().chars().take(200).collect::<String>()
        ));
        return;
    }
    let parsed = match crate::json::parse(out.stdout.trim()) {
        Ok(j) => j,
        Err(e) => {
            diag.push(format!(
                "Dự phòng PowerShell Get-Disk: JSON không phân tích được ({}) — output: {}",
                e,
                out.stdout.trim().chars().take(200).collect::<String>()
            ));
            return;
        }
    };
    let entries: Vec<crate::json::Json> = match &parsed {
        crate::json::Json::Arr(a) => a.clone(),
        other => vec![other.clone()],
    };
    for e in &entries {
        let number = match e.get("Number").and_then(|v| v.as_u64()) {
            Some(n) => n as u32,
            None => continue,
        };
        let serial = e
            .get("SerialNumber")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let model = e
            .get("FriendlyName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let size = e.get("Size").and_then(|v| v.as_u64()).unwrap_or(0);
        let style = match e.get("PartitionStyle").and_then(|v| v.as_str()).unwrap_or("") {
            "GPT" => PartitionStyle::Gpt,
            "MBR" => PartitionStyle::Mbr,
            _ => PartitionStyle::Unknown,
        };
        if let Some(d) = disks.iter_mut().find(|d| d.number == number) {
            let mut filled: Vec<&str> = Vec::new();
            if d.serial.trim().is_empty() && !serial.is_empty() {
                d.serial = serial;
                filled.push("serial");
            }
            if d.model.is_empty() && !model.is_empty() {
                d.model = model;
                filled.push("model");
            }
            if d.size == 0 && size > 0 {
                d.size = size;
                filled.push("dung lượng");
            }
            if d.style == PartitionStyle::Unknown && style != PartitionStyle::Unknown {
                d.style = style;
                filled.push("kiểu phân vùng");
            }
            if !filled.is_empty() {
                diag.push(format!(
                    "Đĩa {}: Get-Disk bổ sung {} còn thiếu từ IOCTL.",
                    number,
                    filled.join(", ")
                ));
            }
        } else {
            diag.push(format!(
                "Đĩa {}: chỉ thấy qua Get-Disk (IOCTL không mở được) — model '{}', serial '{}', {}.",
                number,
                if model.is_empty() { "(trống)" } else { model.as_str() },
                if serial.is_empty() { "(trống)" } else { serial.as_str() },
                sys::format_gb(size)
            ));
            disks.push(PhysicalDisk {
                number,
                serial,
                model,
                size,
                style,
                partitions: Vec::new(),
                from_fallback: true,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Dự phòng 2 — diskpart (list disk + detail disk)
// ---------------------------------------------------------------------------
fn dp_unit_mult(u: &str) -> Option<f64> {
    match u.to_ascii_uppercase().as_str() {
        "KB" => Some(1024.0),
        "MB" => Some(1024.0 * 1024.0),
        "GB" => Some(1024.0 * 1024.0 * 1024.0),
        "TB" => Some(1024.0 * 1024.0 * 1024.0 * 1024.0),
        _ => None,
    }
}

/// Cặp (số, đơn vị) dung lượng đầu tiên trong dòng — mẫu không phụ
/// thuộc ngôn ngữ hiển thị (nhãn cột bị bản địa hoá, số thì không).
fn dp_first_size(toks: &[&str]) -> Option<u64> {
    for w in toks.windows(2) {
        if let (Ok(num), Some(m)) = (w[0].replace(',', ".").parse::<f64>(), dp_unit_mult(w[1])) {
            return Some((num * m) as u64);
        }
    }
    None
}

fn run_diskpart_script(script: &str, file_name: &str) -> Result<cmd::CmdOut, String> {
    let path = std::env::temp_dir().join(file_name);
    std::fs::write(&path, script).map_err(|e| format!("không ghi được file script diskpart: {}", e))?;
    let sp = path.to_string_lossy().into_owned();
    cmd::run_capture("diskpart", &["/s", &sp]).map_err(|e| format!("không chạy được diskpart: {}", e))
}

/// Phân tích `list disk`: mỗi dòng dữ liệu có token[1] là số thứ tự
/// đĩa và một cặp <số> <đơn vị> dung lượng; token "*" đứng riêng =
/// đĩa GPT. Trả (số đĩa, dung lượng, có_dấu_GPT).
fn parse_diskpart_list(out: &str) -> Vec<(u32, u64, bool)> {
    let mut res = Vec::new();
    for line in out.lines() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.len() < 3 {
            continue;
        }
        let number: u32 = match toks[1].parse() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let size = match dp_first_size(&toks) {
            Some(s) if s > 0 => s,
            _ => continue,
        };
        let gpt = toks.iter().any(|t| *t == "*");
        res.push((number, size, gpt));
    }
    res
}

/// Phân tích `detail disk` theo BỐ CỤC (không theo nhãn bản địa hoá):
/// - Disk ID: token dạng GUID {XXXXXXXX-...} hoặc giá trị sau dấu ':'
///   cuối dòng gồm đúng 8 ký tự hex (chữ ký MBR).
/// - Model: dòng không rỗng NGAY TRƯỚC dòng chứa Disk ID (bố cục cố
///   định của detail disk).
/// - Các volume trên đĩa: dòng có cặp dung lượng + một token ký tự ổ
///   đơn độc đứng ngay sau một token số (số thứ tự volume).
fn parse_diskpart_detail(out: &str) -> (String, String, Vec<DiskPartition>) {
    let lines: Vec<&str> = out.lines().collect();
    let mut id = String::new();
    let mut id_line: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        for tok in t.split_whitespace() {
            let core = tok.trim_matches(|c| c == '{' || c == '}');
            if core.len() == 36
                && core.chars().filter(|c| *c == '-').count() == 4
                && core.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
            {
                id = core.to_ascii_uppercase();
                id_line = Some(i);
                break;
            }
        }
        if id.is_empty() {
            if let Some(pos) = t.rfind(':') {
                let v = t[pos + 1..].trim();
                if v.len() == 8 && v.chars().all(|c| c.is_ascii_hexdigit()) {
                    id = v.to_ascii_uppercase();
                    id_line = Some(i);
                }
            }
        }
        if !id.is_empty() {
            break;
        }
    }
    let mut model = String::new();
    if let Some(i) = id_line {
        for line in lines[..i].iter().rev() {
            let t = line.trim();
            if t.is_empty()
                || t.starts_with("Microsoft")
                || t.starts_with("Copyright")
                || t.to_ascii_lowercase().starts_with("diskpart")
            {
                continue;
            }
            model = t.to_string();
            break;
        }
    }
    let mut parts = Vec::new();
    for line in &lines {
        let t = line.trim();
        let toks: Vec<&str> = t.split_whitespace().collect();
        let size = match dp_first_size(&toks) {
            Some(s) if s > 0 => s,
            _ => continue,
        };
        for i in 1..toks.len() {
            let tok = toks[i];
            if tok.chars().count() == 1
                && tok.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
                && toks[i - 1].parse::<u32>().is_ok()
            {
                parts.push(DiskPartition {
                    number: 0,
                    offset: 0,
                    size,
                    kind: PartKind::Unknown,
                    letter: Some(tok.chars().next().unwrap().to_ascii_uppercase()),
                    label: String::new(),
                    fs: String::new(),
                    volume_serial: 0,
                    part_guid: String::new(),
                });
                break;
            }
        }
    }
    (model, id, parts)
}

/// Bổ sung định danh cho các đĩa vẫn thiếu serial sau hai đường
/// trên, bằng diskpart. Serial dự phòng = "DP-" + Disk ID (GUID GPT
/// hoặc chữ ký MBR): không phải serial phần cứng, nhưng wizard và
/// engine dùng CHUNG hàm liệt kê này nên định danh khớp nhau ở cả
/// hai phía; tiền tố DP- giúp người dùng nhận ra khi gõ xác nhận.
fn merge_diskpart_disks(disks: &mut Vec<PhysicalDisk>, diag: &mut Vec<String>) {
    if !disks.iter().any(|d| d.serial.trim().is_empty()) && !disks.is_empty() {
        return;
    }
    let out = match run_diskpart_script("list disk\r\n", "qsu_dp_list.txt") {
        Ok(o) => o,
        Err(e) => {
            diag.push(format!("Dự phòng diskpart: {} — bỏ qua đường này.", e));
            return;
        }
    };
    diag.push("Dự phòng diskpart — output `list disk` (nguyên văn):".to_string());
    for line in out.stdout.lines().take(24) {
        if !line.trim().is_empty() {
            diag.push(format!("  | {}", line.trim_end()));
        }
    }
    let rows = parse_diskpart_list(&out.stdout);
    if rows.is_empty() {
        diag.push("Dự phòng diskpart: không phân tích được dòng đĩa nào từ `list disk`.".to_string());
        return;
    }
    for (number, size, gpt_hint) in rows {
        let needs_identity = match disks.iter().find(|d| d.number == number) {
            Some(d) => d.serial.trim().is_empty(),
            None => true,
        };
        if !needs_identity {
            continue;
        }
        let detail = match run_diskpart_script(
            &format!("select disk {}\r\ndetail disk\r\n", number),
            "qsu_dp_detail.txt",
        ) {
            Ok(o) => o,
            Err(e) => {
                diag.push(format!("Đĩa {}: diskpart detail lỗi ({}) — bỏ qua đĩa này.", number, e));
                continue;
            }
        };
        let (model, id, parts) = parse_diskpart_detail(&detail.stdout);
        if id.is_empty() {
            diag.push(format!(
                "Đĩa {}: không tìm thấy Disk ID trong output `detail disk` — output nguyên văn:",
                number
            ));
            for line in detail.stdout.lines().take(30) {
                if !line.trim().is_empty() {
                    diag.push(format!("  | {}", line.trim_end()));
                }
            }
            continue;
        }
        let serial = format!("DP-{}", id);
        if let Some(d) = disks.iter_mut().find(|d| d.number == number) {
            d.serial = serial.clone();
            if d.model.is_empty() && !model.is_empty() {
                d.model = model.clone();
            }
            if d.size == 0 && size > 0 {
                d.size = size;
            }
            if d.partitions.is_empty() && !parts.is_empty() {
                d.partitions = parts;
            }
            d.from_fallback = true;
            diag.push(format!(
                "Đĩa {}: diskpart bổ sung định danh dự phòng (serial {} — Disk ID, model '{}').",
                number, serial, model
            ));
        } else {
            diag.push(format!(
                "Đĩa {}: chỉ thấy qua diskpart — model '{}', serial {} (Disk ID), {}.",
                number,
                if model.is_empty() { "(trống)" } else { model.as_str() },
                serial,
                sys::format_gb(size)
            ));
            disks.push(PhysicalDisk {
                number,
                serial,
                model,
                size,
                style: if gpt_hint {
                    PartitionStyle::Gpt
                } else {
                    PartitionStyle::Unknown
                },
                partitions: parts,
                from_fallback: true,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// WIPE — xoá toàn bộ ổ đĩa, chia lại từ đầu
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Copy)]
pub struct WipeLayout {
    /// Phân vùng khởi động: EFI (GPT, FAT32) hoặc System Reserved (MBR)
    pub boot_mb: u64,
    pub msr_mb: u64,
    pub windows_mb: u64,
    pub recovery_mb: u64,
}

/// Tính layout chia đĩa theo chuẩn Microsoft. Trả None nếu đĩa quá
/// nhỏ (phân vùng Windows còn lại < 20 GB).
pub fn wipe_layout(disk_size: u64, uefi: bool) -> Option<WipeLayout> {
    if disk_size < MIN_WIPE_DISK_BYTES {
        return None;
    }
    let total_mb = disk_size / MB;
    let (boot_mb, msr_mb) = if uefi { (260, 16) } else { (100, 0) };
    let recovery_mb = 600;
    // Chừa ~4MB cho cấu trúc GPT/MBR + căn lề cuối đĩa
    let windows_mb = total_mb.checked_sub(boot_mb + msr_mb + recovery_mb + 4)?;
    if windows_mb < 20 * 1024 {
        return None;
    }
    Some(WipeLayout {
        boot_mb,
        msr_mb,
        windows_mb,
        recovery_mb,
    })
}

/// Sinh script diskpart cho wipe (ký tự ổ tạm: S = boot, W = Windows,
/// R = Recovery — sau khi chạy sẽ xác minh lại bằng liệt kê thật,
/// không tin tuyệt đối vào các ký tự này).
pub fn wipe_script(disk_number: u32, layout: &WipeLayout, uefi: bool) -> String {
    if uefi {
        format!(
            "select disk {n}\r\n\
             clean\r\n\
             convert gpt\r\n\
             create partition efi size={boot}\r\n\
             format quick fs=fat32 label=\"System\"\r\n\
             assign letter=S\r\n\
             create partition msr size={msr}\r\n\
             create partition primary size={win}\r\n\
             format quick fs=ntfs label=\"Windows\"\r\n\
             assign letter=W\r\n\
             create partition primary size={rec}\r\n\
             format quick fs=ntfs label=\"Recovery\"\r\n\
             assign letter=R\r\n\
             set id=\"de94bba4-06d1-4d40-a16a-bfd50179d6ac\"\r\n\
             gpt attributes=0x8000000000000001\r\n\
             exit\r\n",
            n = disk_number,
            boot = layout.boot_mb,
            msr = layout.msr_mb,
            win = layout.windows_mb,
            rec = layout.recovery_mb
        )
    } else {
        format!(
            "select disk {n}\r\n\
             clean\r\n\
             convert mbr\r\n\
             create partition primary size={boot}\r\n\
             format quick fs=ntfs label=\"System Reserved\"\r\n\
             active\r\n\
             assign letter=S\r\n\
             create partition primary size={win}\r\n\
             format quick fs=ntfs label=\"Windows\"\r\n\
             assign letter=W\r\n\
             create partition primary size={rec}\r\n\
             format quick fs=ntfs label=\"Recovery\"\r\n\
             assign letter=R\r\n\
             set id=27\r\n\
             exit\r\n",
            n = disk_number,
            boot = layout.boot_mb,
            win = layout.windows_mb,
            rec = layout.recovery_mb
        )
    }
}

/// ---------------------------------------------------------------------------
/// Tự gán ký tự ổ cho phân vùng boot sau khi chia lại (v0.7.16)
///
/// Bài học thực địa (winpe-20261117-1811.log, ca wipe trên
/// DESKTOP-704R89L): script diskpart chạy thành công MỌI bước, kể cả
/// dòng "assign letter=S" — DiskPart báo "successfully assigned the
/// drive letter", exit 0. Nhưng liệt kê sau wipe bằng IOCTL cho thấy
/// phân vùng EFI "SYSTEM" KHÔNG có ký tự ổ (hai phân vùng Windows/
/// Recovery thì có). Bước xác minh cũ đòi phân vùng boot phải có ký
/// tự ổ nên huỷ TOÀN BỘ ca cài — ngay sau bước phá huỷ (đĩa đã xoá
/// trắng, chia layout mới) và trước khi bung ảnh: máy mất Windows cũ,
/// không có Windows mới, không boot được gì.
///
/// Nguyên tắc rút ra: một bước kiểm tra nằm SAU thao tác phá huỷ
/// phải TỰ CHỮA được lỗi nó phát hiện, không chỉ báo lỗi rồi dừng.
/// Từ v0.7.16, khi phân vùng boot đã đối chiếu được trong layout
/// mới mà thiếu ký tự ổ (hoặc ký tự liệt kê báo có nhưng probe
/// không truy cập được), engine tự gán ký tự rồi PROBE THẬT đường
/// dẫn gốc "X:\" — chỉ dừng khi phân vùng boot vắng mặt hẳn trong
/// layout, hoặc gán + probe đều thất bại sau khi thử lại.
/// ---------------------------------------------------------------------------

/// Probe THẬT một ký tự ổ: GetFileAttributesW trên đường dẫn gốc
/// "X:\". Đây là tiêu chí thành công duy nhất khi gán ký tự — việc
/// ký tự có/không xuất hiện trong liệt kê volume không chứng minh
/// được volume đã mount truy cập được thật (đúng ca lỗi 1811).
fn probe_drive_root(letter: char) -> bool {
    let root = format!("{}:\\", letter);
    let w = sys::wide(&root);
    unsafe { GetFileAttributesW(w.as_ptr()) != INVALID_FILE_ATTRIBUTES }
}

/// Ký tự ổ có đang bị chiếm không: danh sách `reserved` của lần chạy
/// (ký tự đã chốt cho phân vùng khác), bitmask GetLogicalDrives,
/// và probe thực tế (bitmask trong WinPE có thể trễ — probe là
/// chốt cuối).
fn drive_letter_taken(letter: char, reserved: &[char]) -> bool {
    if reserved.contains(&letter) {
        return true;
    }
    let mask = unsafe { GetLogicalDrives() };
    if mask & (1u32 << (letter as u32 - 'A' as u32)) != 0 {
        return true;
    }
    probe_drive_root(letter)
}

/// Chọn ký tự ổ trống: ưu tiên ký tự script diskpart đã định gán
/// ('S' cho phân vùng boot), sau đó quét T..Z rồi D..R.
fn pick_free_letter(preferred: char, reserved: &[char]) -> Option<char> {
    if !drive_letter_taken(preferred, reserved) {
        return Some(preferred);
    }
    let mut candidates: Vec<char> = ('T'..='Z').collect();
    candidates.extend('D'..='R');
    candidates.into_iter().find(|c| !drive_letter_taken(*c, reserved))
}

/// Cơ chế gán 1: script diskpart phụ (select disk / select
/// partition / assign letter) — cùng mẫu ghi log nguyên văn với
/// các script chia đĩa. `part_ordinal` là số thứ tự phân vùng mà
/// diskpart nhìn thấy (PartitionNumber của layout IOCTL).
fn assign_letter_diskpart(log: &Arc<Logger>, disk_number: u32, part_ordinal: u32, letter: char) -> bool {
    let script = format!(
        "select disk {}\r\nselect partition {}\r\nassign letter={}\r\nexit\r\n",
        disk_number, part_ordinal, letter
    );
    log.line("  --- diskpart script gán ký tự (nguyên văn) ---");
    for line in script.lines() {
        log.line(&format!("  | {}", line));
    }
    log.line("  --- hết script ---");
    let script_path = std::env::temp_dir().join("qsu_assign_letter.txt");
    if let Err(e) = std::fs::write(&script_path, &script) {
        log.line(&format!("  gán ký tự (diskpart): không ghi được file script: {}", e));
        return false;
    }
    let sp = script_path.to_string_lossy().into_owned();
    match cmd::run_logged(log, "diskpart", &["/s", &sp]) {
        Ok(out) => out.code == Some(0),
        Err(e) => {
            log.line(&format!("  gán ký tự (diskpart): không chạy được diskpart: {}", e));
            false
        }
    }
}

/// Tên volume (\\?\Volume{GUID}\) của phân vùng tại (số đĩa,
/// offset) — quét FindFirstVolumeW + extent, cùng cách
/// enumerate_volume_matches gắn volume vào phân vùng. Dùng cho cơ
/// chế gán dự phòng qua SetVolumeMountPointW.
fn find_volume_name(disk_number: u32, offset: u64) -> Option<String> {
    unsafe {
        let mut name_buf = vec![0u16; 1024];
        let find = FindFirstVolumeW(name_buf.as_mut_ptr(), name_buf.len() as DWORD);
        if find == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut found = None;
        loop {
            let volume_path = sys::from_wide(&name_buf);
            if let Some((d, off, _)) = volume_extent(&volume_path) {
                if d == disk_number && off == offset {
                    found = Some(volume_path);
                    break;
                }
            }
            for v in name_buf.iter_mut() {
                *v = 0;
            }
            if FindNextVolumeW(find, name_buf.as_mut_ptr(), name_buf.len() as DWORD) == 0 {
                break;
            }
        }
        FindVolumeClose(find);
        found
    }
}

/// Cơ chế gán 2 (dự phòng): SetVolumeMountPointW trực tiếp trên
/// volume của phân vùng — bỏ qua diskpart khi diskpart báo thành
/// công mà mount không hiệu lực (đúng ca lỗi 1811).
fn assign_letter_mountpoint(log: &Arc<Logger>, disk_number: u32, part: &DiskPartition, letter: char) -> bool {
    let name = match find_volume_name(disk_number, part.offset) {
        Some(n) => n,
        None => {
            log.line(&format!(
                "  gán ký tự (mountpoint): không tìm thấy tên volume của phân vùng tại đĩa {} offset {}",
                disk_number, part.offset
            ));
            return false;
        }
    };
    let mount = sys::wide(&format!("{}:\\", letter));
    let vol = sys::wide(&name);
    let ok = unsafe { SetVolumeMountPointW(mount.as_ptr(), vol.as_ptr()) } != 0;
    if ok {
        log.line(&format!(
            "  gán ký tự (mountpoint): SetVolumeMountPointW {}:\\ ← {} — OK",
            letter, name
        ));
    } else {
        log.line(&format!(
            "  gán ký tự (mountpoint): SetVolumeMountPointW {}:\\ ← {} thất bại (GetLastError={})",
            letter,
            name,
            unsafe { GetLastError() }
        ));
    }
    ok
}

/// Bảo đảm phân vùng boot ĐÃ ĐỐI CHIẾU ĐƯỢC trên đĩa đích có một
/// ký tự ổ DÙNG ĐƯỢC THẬT (v0.7.16 — xem khối chú thích ở trên).
/// - Phân vùng có sẵn ký tự và probe đạt → trả ký tự đó.
/// - Thiếu ký tự (hoặc có mà probe không đạt) → tự gán: ký tự ưu
///   tiên 'S', loại trừ `reserved`; mỗi lần thử chạy cả hai cơ chế
///   (diskpart, rồi SetVolumeMountPointW) và chốt bằng probe thật;
///   thử tối đa 2 lần.
/// - Err chỉ khi không còn ký tự trống, hoặc cả 2 lần thử đều
///   không cho đường dẫn truy cập được.
pub fn ensure_boot_letter(
    log: &Arc<Logger>,
    disk_number: u32,
    part: &DiskPartition,
    part_ordinal: u32,
    preferred: char,
    reserved: &[char],
) -> Result<char, String> {
    log.line(&format!(
        "Phân vùng boot khớp trên đĩa {}: phân vùng #{} (số thứ tự diskpart {}) — {} — {} — ký tự theo liệt kê: {}",
        disk_number,
        part.number,
        part_ordinal,
        part.kind.label_vi(),
        sys::format_gb(part.size),
        part.letter
            .map(|c| format!("{}:", c))
            .unwrap_or_else(|| "(chưa có)".to_string())
    ));
    if let Some(l) = part.letter {
        if probe_drive_root(l) {
            log.line(&format!("  probe {}:\\ — truy cập được, dùng ký tự này.", l));
            return Ok(l);
        }
        log.line(&format!(
            "  CẢNH BÁO: liệt kê báo phân vùng boot có ký tự {}: nhưng probe {}:\\ thất bại — tự gán lại ký tự.",
            l, l
        ));
    }
    let letter = pick_free_letter(preferred, reserved).ok_or_else(|| {
        "không còn ký tự ổ trống nào để gán cho phân vùng boot".to_string()
    })?;
    log.line(&format!(
        "  ký tự sẽ gán cho phân vùng boot: {}: (ưu tiên '{}'; loại trừ: {})",
        letter,
        preferred,
        if reserved.is_empty() {
            "(không có)".to_string()
        } else {
            reserved.iter().map(|c| format!("{}:", c)).collect::<Vec<_>>().join(", ")
        }
    ));
    for attempt in 1..=2u32 {
        log.line(&format!("  tự gán ký tự {}: — lần thử {}/2...", letter, attempt));
        let via_diskpart = assign_letter_diskpart(log, disk_number, part_ordinal, letter);
        let via_mountpoint = if probe_drive_root(letter) {
            false
        } else {
            assign_letter_mountpoint(log, disk_number, part, letter)
        };
        let probe = probe_drive_root(letter);
        log.line(&format!(
            "  lần thử {}: diskpart={}, mountpoint={}, probe {}:\\={}",
            attempt, via_diskpart, via_mountpoint, letter, probe
        ));
        if probe {
            log.line(&format!(
                "  ✓ tự gán ký tự thành công: phân vùng boot = {}: (lần thử {})",
                letter, attempt
            ));
            return Ok(letter);
        }
    }
    Err(format!(
        "phân vùng boot trên đĩa {} (phân vùng #{}) vẫn không có ký tự ổ dùng được sau 2 lần tự gán (ký tự đã thử: {}:)",
        disk_number, part.number, letter
    ))
}

/// Kết quả sau wipe/rebuild: ký tự ổ THẬT của các phân vùng mới
/// trên đĩa đích — phân vùng Windows/Recovery xác minh bằng liệt
/// kê, phân vùng boot chốt bằng probe đường dẫn thật sau khi tự
/// gán ký tự nếu thiếu (v0.7.16, xem ensure_boot_letter).
#[derive(Debug, Clone)]
pub struct WipeResult {
    pub windows_letter: char,
    pub boot_letter: char,
    pub recovery_letter: Option<char>,
}

/// Thực thi wipe một đĩa vật lý: TỰ đối chiếu lại serial phần cứng của
/// đúng số đĩa đó ngay trước khi chạy diskpart (không tin caller),
/// sinh script + ghi log nguyên văn, chạy diskpart, rồi xác minh các
/// phân vùng mới bằng liệt kê thật.
pub fn execute_wipe(
    log: &Arc<Logger>,
    disk_number: u32,
    expected_serial: &str,
    disk_size: u64,
    uefi: bool,
) -> Result<WipeResult, String> {
    // Đối chiếu serial tại thời điểm thực thi — qua CHUNG hàm liệt
    // kê với phía chọn đích (enumerate_disks, có cả đường dự phòng):
    // bản cũ chỉ hỏi IOCTL_STORAGE_QUERY_PROPERTY tại chỗ, đĩa nào
    // được định danh qua dự phòng (serial DP-...) sẽ luôn "không
    // khớp" và wipe bị chặn oan.
    let enumeration = enumerate_disks();
    for line in &enumeration.diag {
        log.line(&format!("  [liệt kê lại trước wipe] {}", line));
    }
    let live_disk = enumeration
        .disks
        .iter()
        .find(|d| d.number == disk_number)
        .ok_or_else(|| {
            format!(
                "không liệt kê lại được đĩa {} tại thời điểm thực thi. Dừng, không đụng vào đĩa.",
                disk_number
            )
        })?;
    let live_serial = live_disk.serial.clone();
    let live_model = live_disk.model.clone();
    if normalize_disk_serial(&live_serial) != normalize_disk_serial(expected_serial) {
        return Err(format!(
            "serial đĩa {} tại thời điểm thực thi là '{}' — KHÔNG khớp serial kỳ vọng '{}'. Dừng, không đụng vào đĩa.",
            disk_number, live_serial, expected_serial
        ));
    }
    let layout = wipe_layout(disk_size, uefi)
        .ok_or_else(|| format!("đĩa {} quá nhỏ để chia lại ({}).", disk_number, sys::format_gb(disk_size)))?;
    log.line(&format!(
        "WIPE đĩa {} ({} — {} — serial {}): layout {} = boot {}MB{} + Windows {}MB + Recovery {}MB",
        disk_number,
        live_model,
        sys::format_gb(disk_size),
        live_serial,
        if uefi { "GPT" } else { "MBR" },
        layout.boot_mb,
        if uefi { format!(" + MSR {}MB", layout.msr_mb) } else { String::new() },
        layout.windows_mb,
        layout.recovery_mb
    ));
    let script = wipe_script(disk_number, &layout, uefi);
    log.line("--- diskpart script (nguyên văn) ---");
    for line in script.lines() {
        log.line(&format!("  | {}", line));
    }
    log.line("--- hết script ---");
    let script_path = std::env::temp_dir().join("qsu_wipe_diskpart.txt");
    std::fs::write(&script_path, &script)
        .map_err(|e| format!("không ghi được file script diskpart: {}", e))?;
    let sp = script_path.to_string_lossy().into_owned();
    let out = cmd::run_logged(log, "diskpart", &["/s", &sp])
        .map_err(|e| format!("không chạy được diskpart: {}", e))?;
    if out.code != Some(0) {
        return Err(format!("diskpart trả mã {:?} — kiểm tra log.", out.code));
    }
    // Xác minh bằng liệt kê thật trên chính đĩa đích
    let disks = list_physical_disks();
    let disk = disks
        .iter()
        .find(|d| d.number == disk_number)
        .ok_or_else(|| format!("sau diskpart không liệt kê lại được đĩa {}", disk_number))?;
    log.line(&format!("Đĩa {} sau wipe:\n{}", disk_number, disk.describe_partitions()));
    let mut windows: Option<char> = None;
    let mut boot_part: Option<(usize, DiskPartition)> = None;
    let mut recovery: Option<char> = None;
    for (idx, p) in disk.partitions.iter().enumerate() {
        if p.label.eq_ignore_ascii_case("Windows") {
            // Đối chiếu thêm dung lượng cho chắc (sai lệch < 10%)
            let expect = layout.windows_mb * MB;
            if p.size.abs_diff(expect) <= expect / 10 {
                windows = p.letter;
            }
        }
        if uefi {
            if p.kind == PartKind::Efi {
                boot_part = Some((idx, p.clone()));
            }
        } else if p.label.eq_ignore_ascii_case("System Reserved") || p.kind == PartKind::System {
            boot_part = Some((idx, p.clone()));
        }
        if p.label.eq_ignore_ascii_case("Recovery") || p.kind == PartKind::Recovery {
            recovery = p.letter;
        }
    }
    let windows_letter = windows.ok_or_else(|| {
        "sau wipe không tìm thấy phân vùng Windows (nhãn 'Windows') có ký tự ổ trên đĩa đích".to_string()
    })?;
    // Phân vùng boot: chỉ vắng mặt hẳn trong layout mới mới là lỗi
    // cứng; có mặt mà thiếu ký tự ổ thì TỰ GÁN (v0.7.16 — ca lỗi
    // winpe-20261117-1811: diskpart báo assign thành công nhưng EFI
    // vẫn không có ký tự, bản cũ dừng ngay sau bước phá huỷ).
    let boot_letter = match boot_part {
        None => {
            return Err(
                "sau wipe không tìm thấy phân vùng boot (EFI/System Reserved) trên đĩa đích".to_string(),
            )
        }
        Some((idx, p)) => {
            let ordinal = if p.number > 0 { p.number } else { idx as u32 + 1 };
            let mut reserved: Vec<char> = vec![windows_letter];
            if let Some(r) = recovery {
                reserved.push(r);
            }
            ensure_boot_letter(log, disk_number, &p, ordinal, 'S', &reserved)
                .map_err(|e| format!("sau wipe: {}", e))?
        }
    };
    log.line(&format!(
        "Xác minh sau wipe: Windows = {}:, Boot = {}:, Recovery = {}",
        windows_letter,
        boot_letter,
        recovery.map(|c| format!("{}:", c)).unwrap_or_else(|| "(không có ký tự)".to_string())
    ));
    Ok(WipeResult {
        windows_letter,
        boot_letter,
        recovery_letter: recovery,
    })
}

// ---------------------------------------------------------------------------
// REBUILD — chỉ xoá các phân vùng hệ thống đã chọn trên đĩa đích, chia
// lại layout mới trong đúng vùng trống của chúng, GIỮ NGUYÊN mọi phân
// vùng data trên cùng đĩa (v0.7.13 — sinh ra cho máy CHỈ CÓ 1 ĐĨA:
// EFI+MSR+C: Windows cũ+D: data → cài lại hệ thống, D: nguyên vẹn).
// Khác wipe ở ba điểm cốt lõi: (1) đĩa đích ĐƯỢC PHÉP trùng đĩa chứa
// staging/data — phân vùng staging được bảo vệ bằng kiểm tra extent;
// (2) không `clean`, không đổi kiểu đĩa — chỉ xoá đúng các phân vùng
// đã đối chiếu được bằng GUID (GPT) hoặc offset+size (MBR); (3) mọi
// kiểm tra (đối chiếu, liên tục, đủ chỗ) đều chạy TRƯỚC khi xoá bất
// cứ thứ gì — lệch là Err, đĩa không bị đụng vào.
// ---------------------------------------------------------------------------

/// Layout cho vùng trống của rebuild. Trả None nếu vùng không đủ:
/// (boot + MSR + Recovery) + phân vùng Windows tối thiểu 10 GB.
pub fn rebuild_layout(region_size: u64, uefi: bool) -> Option<WipeLayout> {
    let total_mb = region_size / MB;
    let (boot_mb, msr_mb) = if uefi { (260, 16) } else { (100, 0) };
    let recovery_mb = 600;
    let windows_mb = total_mb.checked_sub(boot_mb + msr_mb + recovery_mb)?;
    if windows_mb < 10 * 1024 {
        return None;
    }
    Some(WipeLayout {
        boot_mb,
        msr_mb,
        windows_mb,
        recovery_mb,
    })
}

/// Chuẩn hoá GUID phân vùng để so sánh (bỏ ngoặc nhọn, viết hoa).
fn normalize_part_guid(s: &str) -> String {
    s.trim().trim_matches(|c| c == '{' || c == '}').to_ascii_uppercase()
}

/// Script diskpart cho rebuild: xoá đúng các phân vùng đã đối chiếu
/// (theo số thứ tự LIVE, thứ tự offset giảm dần), rồi tạo layout mới
/// GHIM offset vào đúng vùng vừa trống (offset tính bằng KB như
/// diskpart yêu cầu) để không bao giờ lấn sang phân vùng data kề bên.
/// Ký tự ổ tạm: S = boot, W = Windows, R = Recovery (xác minh lại
/// bằng liệt kê sau khi chạy, như wipe).
fn rebuild_script(
    disk_number: u32,
    delete_numbers_desc: &[u32],
    region_start: u64,
    layout: &WipeLayout,
    uefi: bool,
) -> String {
    let mut s = format!("select disk {}\r\n", disk_number);
    for n in delete_numbers_desc {
        s += &format!("select partition {}\r\ndelete partition override\r\n", n);
    }
    let kb = |byte_off: u64| byte_off.div_ceil(1024);
    let mb_kb = |mb: u64| mb * 1024;
    let start_kb = kb(region_start);
    if uefi {
        s += &format!(
            "create partition efi size={boot} offset={o0}\r\n\
             format quick fs=fat32 label=\"System\"\r\n\
             assign letter=S\r\n\
             create partition msr size={msr} offset={o1}\r\n\
             create partition primary size={win} offset={o2}\r\n\
             format quick fs=ntfs label=\"Windows\"\r\n\
             assign letter=W\r\n\
             create partition primary size={rec} offset={o3}\r\n\
             format quick fs=ntfs label=\"Recovery\"\r\n\
             assign letter=R\r\n\
             set id=\"de94bba4-06d1-4d40-a16a-bfd50179d6ac\"\r\n\
             gpt attributes=0x8000000000000001\r\n\
             exit\r\n",
            boot = layout.boot_mb,
            msr = layout.msr_mb,
            win = layout.windows_mb,
            rec = layout.recovery_mb,
            o0 = start_kb,
            o1 = start_kb + mb_kb(layout.boot_mb),
            o2 = start_kb + mb_kb(layout.boot_mb + layout.msr_mb),
            o3 = start_kb + mb_kb(layout.boot_mb + layout.msr_mb + layout.windows_mb),
        );
    } else {
        s += &format!(
            "create partition primary size={boot} offset={o0}\r\n\
             format quick fs=ntfs label=\"System Reserved\"\r\n\
             active\r\n\
             assign letter=S\r\n\
             create partition primary size={win} offset={o2}\r\n\
             format quick fs=ntfs label=\"Windows\"\r\n\
             assign letter=W\r\n\
             create partition primary size={rec} offset={o3}\r\n\
             format quick fs=ntfs label=\"Recovery\"\r\n\
             assign letter=R\r\n\
             set id=27\r\n\
             exit\r\n",
            boot = layout.boot_mb,
            win = layout.windows_mb,
            rec = layout.recovery_mb,
            o0 = start_kb,
            o2 = start_kb + mb_kb(layout.boot_mb),
            o3 = start_kb + mb_kb(layout.boot_mb + layout.windows_mb),
        );
    }
    s
}

/// Thực thi rebuild trên đĩa `disk_number`: tự liệt kê lại đĩa, đối
/// chiếu serial + từng phân vùng trong `spec` (GPT theo GUID duy
/// nhất; MBR theo offset+size ±1MB), kiểm tra phân vùng staging
/// không nằm trong danh sách xoá, kiểm tra các phân vùng cần xoá
/// tạo thành MỘT vùng liên tục và vùng đó đủ lớn — TẤT CẢ trước khi
/// xoá. Chỉ khi mọi kiểm tra đạt mới chạy diskpart (xoá + chia lại),
/// rồi xác minh: phân vùng mới có đủ, phân vùng giữ lại còn nguyên
/// vị trí/kích thước. Bất kỳ lệch nào → Err, phía gọi dừng cứng.
///
/// `staging_extent`: (offset, size) của volume chứa staging NẾU nó
/// nằm trên chính đĩa này; None = staging ở đĩa khác (kiểm tra bảo
/// vệ khi đó hiển nhiên đạt). Phía gọi PHẢI truyền Some khi đĩa
/// staging trùng đĩa đích — không xác định được extent thì dừng ở
/// phía gọi, không gọi hàm này.
pub fn execute_rebuild(
    log: &Arc<Logger>,
    disk_number: u32,
    expected_serial: &str,
    uefi: bool,
    spec: &[crate::stage::RebuildPart],
    staging_extent: Option<(u64, u64)>,
) -> Result<WipeResult, String> {
    if spec.is_empty() {
        return Err("stage.json không có danh sách phân vùng cần xoá (rebuildPartitions rỗng). Dừng, không đụng vào đĩa.".to_string());
    }
    let win_count = spec.iter().filter(|e| e.kind.eq_ignore_ascii_case("Windows")).count();
    if win_count != 1 {
        return Err(format!(
            "danh sách rebuildPartitions phải có đúng 1 mục loại Windows (đang có {}). Dừng, không đụng vào đĩa.",
            win_count
        ));
    }
    let enumeration = enumerate_disks();
    for line in &enumeration.diag {
        log.line(&format!("  [liệt kê lại trước rebuild] {}", line));
    }
    let live_disk = enumeration
        .disks
        .iter()
        .find(|d| d.number == disk_number)
        .ok_or_else(|| {
            format!(
                "không liệt kê lại được đĩa {} tại thời điểm thực thi. Dừng, không đụng vào đĩa.",
                disk_number
            )
        })?;
    let live_serial = live_disk.serial.clone();
    let live_model = live_disk.model.clone();
    if normalize_disk_serial(&live_serial) != normalize_disk_serial(expected_serial) {
        return Err(format!(
            "serial đĩa {} tại thời điểm thực thi là '{}' — KHÔNG khớp serial kỳ vọng '{}'. Dừng, không đụng vào đĩa.",
            disk_number, live_serial, expected_serial
        ));
    }
    let is_gpt = live_disk.style == PartitionStyle::Gpt;
    log.line(&format!(
        "REBUILD đĩa {} ({} — {} — serial {} — {}): đối chiếu {} phân vùng hệ thống cần xoá...",
        disk_number,
        live_model,
        sys::format_gb(live_disk.size),
        live_serial,
        if is_gpt { "GPT" } else { "MBR" },
        spec.len()
    ));

    // --- Đối chiếu từng mục spec với phân vùng LIVE ---
    let mut claimed: Vec<u32> = Vec::new(); // số thứ tự phân vùng đã khớp
    let mut matched: Vec<DiskPartition> = Vec::new();
    for e in spec {
        let hits: Vec<&DiskPartition> = live_disk
            .partitions
            .iter()
            .filter(|p| !claimed.contains(&p.number))
            .filter(|p| {
                if is_gpt && !e.gpt_guid.trim().is_empty() {
                    normalize_part_guid(&p.part_guid) == normalize_part_guid(&e.gpt_guid)
                } else {
                    p.offset.abs_diff(e.offset_bytes) <= MB
                        && p.size.abs_diff(e.size_bytes) <= MB
                }
            })
            .collect();
        if hits.len() != 1 {
            return Err(format!(
                "đối chiếu phân vùng {} (offset {}, {}) trên đĩa {}: tìm thấy {} phân vùng khớp (cần đúng 1). Đĩa có thể đã thay đổi từ lúc wizard chạy. Dừng, KHÔNG xoá gì.",
                e.kind,
                e.offset_bytes,
                sys::format_gb(e.size_bytes),
                disk_number,
                hits.len()
            ));
        }
        let p = hits[0].clone();
        claimed.push(p.number);
        log.line(&format!(
            "  ✓ {} → phân vùng #{} (offset {}, {}, {})",
            e.kind,
            p.number,
            p.offset,
            sys::format_gb(p.size),
            p.describe()
        ));
        matched.push(p);
    }

    // --- Bảo vệ phân vùng staging ---
    if let Some((s_off, s_size)) = staging_extent {
        let s_end = s_off.saturating_add(s_size);
        for p in &matched {
            let p_end = p.offset.saturating_add(p.size);
            if s_off < p_end && p.offset < s_end {
                return Err(format!(
                    "NGUY HIỂM: phân vùng chứa bộ cài/staging (offset {}) TRÙNG với phân vùng #{} nằm trong danh sách xoá. Dừng, KHÔNG xoá gì.",
                    s_off, p.number
                ));
            }
        }
        log.line("  ✓ phân vùng chứa staging không nằm trong danh sách xoá.");
    }

    // --- Liên tục: các phân vùng cần xoá phải là MỘT vùng liền nhau ---
    matched.sort_by_key(|p| p.offset);
    for w in matched.windows(2) {
        let prev_end = w[0].offset.saturating_add(w[0].size);
        let gap = w[1].offset as i128 - prev_end as i128;
        if gap.abs() > MB as i128 {
            return Err(format!(
                "các phân vùng cần xoá KHÔNG tạo thành một vùng liên tục: phân vùng #{} kết thúc tại {} nhưng phân vùng #{} bắt đầu tại {} (lệch {} byte). Dừng, KHÔNG xoá gì.",
                w[0].number, prev_end, w[1].number, w[1].offset, gap
            ));
        }
    }
    let region_start = matched.first().map(|p| p.offset).unwrap_or(0);
    let region_end = matched
        .iter()
        .map(|p| p.offset.saturating_add(p.size))
        .max()
        .unwrap_or(region_start);
    let region_size = region_end.saturating_sub(region_start);

    // --- Đủ chỗ: kiểm tra TRƯỚC khi xoá bất cứ thứ gì ---
    let layout = rebuild_layout(region_size, uefi).ok_or_else(|| {
        format!(
            "vùng hệ thống cũ ({}) quá nhỏ cho layout mới: cần tối thiểu {} (boot {}MB{} + Recovery 600MB + Windows tối thiểu 10GB). Dừng, KHÔNG xoá gì.",
            sys::format_gb(region_size),
            sys::format_gb((if uefi { 876 } else { 700 }) * MB + 10 * GB),
            if uefi { 260 } else { 100 },
            if uefi { " + MSR 16MB" } else { "" },
        )
    })?;
    log.line(&format!(
        "Vùng hệ thống cũ: offset {} → {} ({}). Layout mới {}: boot {}MB{} + Windows {}MB + Recovery {}MB.",
        region_start,
        region_end,
        sys::format_gb(region_size),
        if uefi { "GPT" } else { "MBR" },
        layout.boot_mb,
        if uefi { format!(" + MSR {}MB", layout.msr_mb) } else { String::new() },
        layout.windows_mb,
        layout.recovery_mb
    ));

    // Ảnh chụp các phân vùng GIỮ LẠI để xác minh sau khi chia
    let kept: Vec<(u64, u64)> = live_disk
        .partitions
        .iter()
        .filter(|p| !claimed.contains(&p.number))
        .map(|p| (p.offset, p.size))
        .collect();
    log.line(&format!(
        "Phân vùng giữ nguyên trên đĩa {}: {} phân vùng (sẽ đối chiếu lại sau khi chia).",
        disk_number,
        kept.len()
    ));

    // --- Xoá + chia lại bằng MỘT script diskpart ---
    let mut delete_numbers: Vec<u32> = matched.iter().map(|p| p.number).collect();
    delete_numbers.sort_unstable_by(|a, b| {
        let oa = matched.iter().find(|p| &p.number == a).map(|p| p.offset).unwrap_or(0);
        let ob = matched.iter().find(|p| &p.number == b).map(|p| p.offset).unwrap_or(0);
        ob.cmp(&oa) // offset giảm dần
    });
    let script = rebuild_script(disk_number, &delete_numbers, region_start, &layout, uefi);
    log.line("--- diskpart script rebuild (nguyên văn) ---");
    for line in script.lines() {
        log.line(&format!("  | {}", line));
    }
    log.line("--- hết script ---");
    let script_path = std::env::temp_dir().join("qsu_rebuild_diskpart.txt");
    std::fs::write(&script_path, &script)
        .map_err(|e| format!("không ghi được file script diskpart: {}", e))?;
    let sp = script_path.to_string_lossy().into_owned();
    let out = cmd::run_logged(log, "diskpart", &["/s", &sp])
        .map_err(|e| format!("không chạy được diskpart: {}", e))?;
    if out.code != Some(0) {
        return Err(format!("diskpart trả mã {:?} — kiểm tra log.", out.code));
    }

    // --- Xác minh sau khi chia: phân vùng mới + phân vùng giữ lại ---
    let disks = list_physical_disks();
    let disk = disks
        .iter()
        .find(|d| d.number == disk_number)
        .ok_or_else(|| format!("sau diskpart không liệt kê lại được đĩa {}", disk_number))?;
    log.line(&format!("Đĩa {} sau rebuild:\n{}", disk_number, disk.describe_partitions()));
    for (off, size) in &kept {
        let still = disk
            .partitions
            .iter()
            .any(|p| p.offset.abs_diff(*off) <= MB && p.size.abs_diff(*size) <= MB);
        if !still {
            return Err(format!(
                "SAU KHI CHIA: không còn thấy phân vùng giữ lại tại offset {} ({}) trên đĩa {} — bất thường nghiêm trọng, dừng lại để kiểm tra thủ công.",
                off,
                sys::format_gb(*size),
                disk_number
            ));
        }
    }
    log.line("  ✓ mọi phân vùng giữ lại vẫn nguyên vị trí và kích thước.");
    let in_region = |p: &DiskPartition| {
        p.offset + MB >= region_start && p.offset < region_end.saturating_add(MB)
    };
    let mut windows: Option<char> = None;
    let mut boot_part: Option<(usize, DiskPartition)> = None;
    let mut recovery: Option<char> = None;
    for (idx, p) in disk.partitions.iter().enumerate().filter(|(_, p)| in_region(p)) {
        if p.label.eq_ignore_ascii_case("Windows") {
            let expect = layout.windows_mb * MB;
            if p.size.abs_diff(expect) <= expect / 10 {
                windows = p.letter;
            }
        }
        if uefi {
            if p.kind == PartKind::Efi {
                boot_part = Some((idx, p.clone()));
            }
        } else if p.label.eq_ignore_ascii_case("System Reserved") || p.kind == PartKind::System {
            boot_part = Some((idx, p.clone()));
        }
        if p.label.eq_ignore_ascii_case("Recovery") || p.kind == PartKind::Recovery {
            recovery = p.letter;
        }
    }
    let windows_letter = windows.ok_or_else(|| {
        "sau rebuild không tìm thấy phân vùng Windows mới (nhãn 'Windows') có ký tự ổ trong vùng vừa chia".to_string()
    })?;
    // Phân vùng boot mới: vắng mặt hẳn trong vùng vừa chia mới là
    // lỗi cứng; có mặt mà thiếu ký tự ổ thì TỰ GÁN (v0.7.16 — cùng
    // cơ chế tự chữa như wipe, xem ensure_boot_letter).
    let boot_letter = match boot_part {
        None => {
            return Err(
                "sau rebuild không tìm thấy phân vùng boot mới (EFI/System Reserved) trong vùng vừa chia".to_string(),
            )
        }
        Some((idx, p)) => {
            let ordinal = if p.number > 0 { p.number } else { idx as u32 + 1 };
            let mut reserved: Vec<char> = vec![windows_letter];
            if let Some(r) = recovery {
                reserved.push(r);
            }
            ensure_boot_letter(log, disk_number, &p, ordinal, 'S', &reserved)
                .map_err(|e| format!("sau rebuild: {}", e))?
        }
    };
    log.line(&format!(
        "Xác minh sau rebuild: Windows = {}:, Boot = {}:, Recovery = {}",
        windows_letter,
        boot_letter,
        recovery.map(|c| format!("{}:", c)).unwrap_or_else(|| "(không có ký tự)".to_string())
    ));
    Ok(WipeResult {
        windows_letter,
        boot_letter,
        recovery_letter: recovery,
    })
}

/// Copy WinRE từ bản Windows đã apply vào phân vùng Recovery mới
/// (`<R>:\Recovery\WindowsRE\Winre.wim`) + đăng ký bằng reagentc ở chế
/// độ offline (best-effort — lỗi chỉ ghi log, không chặn cài đặt).
pub fn copy_winre(log: &Arc<Logger>, windows_letter: char, recovery_letter: char) -> bool {
    let src = std::path::PathBuf::from(format!(
        "{}:\\Windows\\System32\\Recovery\\Winre.wim",
        windows_letter
    ));
    if !src.exists() {
        log.line(&format!(
            "CẢNH BÁO: không thấy Winre.wim tại {} — bỏ qua bước WinRE.",
            src.display()
        ));
        return false;
    }
    let dst_dir = std::path::PathBuf::from(format!("{}:\\Recovery\\WindowsRE", recovery_letter));
    if let Err(e) = std::fs::create_dir_all(&dst_dir) {
        log.line(&format!("CẢNH BÁO: không tạo được thư mục {}: {}", dst_dir.display(), e));
        return false;
    }
    if let Err(e) = std::fs::copy(&src, dst_dir.join("Winre.wim")) {
        log.line(&format!("CẢNH BÁO: copy Winre.wim thất bại: {}", e));
        return false;
    }
    log.line(&format!(
        "Đã copy Winre.wim vào {}:\\Recovery\\WindowsRE.",
        recovery_letter
    ));
    // ReAgent.xml đi kèm (nếu ảnh có) — Setup thường sinh file này khi
    // đăng ký WinRE; copy theo để cấu hình đầy đủ hơn.
    let agent_src = std::path::PathBuf::from(format!(
        "{}:\\Windows\\System32\\Recovery\\ReAgent.xml",
        windows_letter
    ));
    if agent_src.exists() {
        let _ = std::fs::copy(&agent_src, dst_dir.join("ReAgent.xml"));
    }
    // Đăng ký WinRE offline (best-effort)
    let path_arg = format!("{}:\\Recovery\\WindowsRE", recovery_letter);
    let target_arg = format!("{}:\\Windows", windows_letter);
    match cmd::run_logged(
        log,
        "reagentc",
        &["/setreimage", "/path", &path_arg, "/target", &target_arg],
    ) {
        Ok(out) if out.code == Some(0) => log.line("reagentc /setreimage: OK."),
        Ok(out) => log.line(&format!(
            "CẢNH BÁO: reagentc /setreimage trả mã {:?} — WinRE đã copy nhưng chưa đăng ký, Windows sẽ tự xử lý ở lần boot đầu hoặc chạy lại reagentc sau.",
            out.code
        )),
        Err(e) => log.line(&format!(
            "CẢNH BÁO: không chạy được reagentc ({} ) — WinRE đã copy nhưng chưa đăng ký.",
            e
        )),
    }
    true
}
