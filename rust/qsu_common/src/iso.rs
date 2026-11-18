//! MOUNT ISO BẰNG VIRT DISK API GỐC — dùng chung (v0.7.19).
//!
//! Sinh ra cho module CaiDatWinWinPE: trong các WinPE phổ biến
//! (BHT/Anhdv...) KHÔNG có PowerShell/cmdlet Storage, nên đường
//! mount bằng `Mount-DiskImage` của các bản trước là ngõ cụt —
//! thực địa 2026-11-18 module báo thẳng "WinPE này không có
//! PowerShell/cmdlet Storage nên không mount được ISO" và kỹ
//! thuật viên phải mount ISO bằng công cụ ngoài rồi gõ tay
//! đường dẫn `F:\sources\install.wim`. Từ v0.7.19 đường mount
//! của module WinPE đi thẳng xuống VirtDisk API như LỚP 3 của
//! wizard (v0.7.17): OpenVirtualDisk (storage-type ISO) +
//! AttachVirtualDisk đọc-chỉ-đọc + dò ký tự ổ DVD mới xuất
//! hiện. Wizard giữ nguyên bộ ba lớp của nó; module này chỉ
//! dùng VirtDisk — trong WinPE không có lớp PowerShell nào để
//! xếp dưới.
//!
//! Gỡ mount ĐỐI XỨNG: sổ đăng ký trong tiến trình giữ HANDLE
//! của từng ISO đã attach; `dismount_iso` DetachVirtualDisk
//! bằng chính handle đó rồi đóng handle. Mount do tiến trình
//! KHÁC thực hiện (Explorer/công cụ của WinPE) không nằm trong
//! sổ và không bị module đụng vào — khi VirtDisk attach không
//! thực hiện được, lỗi trả về hướng dẫn kỹ thuật viên mount
//! sẵn bằng công cụ của WinPE rồi chọn/gõ thẳng file
//! sources\install.* (đúng đường đã cứu ca thực địa).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// FFI tối thiểu tới VirtDisk API (virtdisk.dll) — winapi 0.3
/// không có module virtdisk nên khai báo tay đúng nguyên mẫu
/// của Windows SDK (bản sao của lớp 3 trong wizard, v0.7.17).
mod virtdisk {
    #![allow(dead_code)]
    use std::ffi::c_void;
    use winapi::shared::guiddef::GUID;
    use winapi::shared::minwindef::{DWORD, ULONG};
    use winapi::um::winnt::{HANDLE, PCWSTR};

    /// VIRTUAL_STORAGE_TYPE_DEVICE_ISO.
    pub const STORAGE_DEVICE_ISO: ULONG = 1;
    /// GUID nhà cung cấp Microsoft của VIRTUAL_STORAGE_TYPE
    /// ({EC984AEC-A0F9-47E9-901F-71415A66345B}).
    pub const VENDOR_MICROSOFT: GUID = GUID {
        Data1: 0xEC98_4AEC,
        Data2: 0xA0F9,
        Data3: 0x47E9,
        Data4: [0x90, 0x1F, 0x71, 0x41, 0x5A, 0x66, 0x34, 0x5B],
    };
    pub const OPEN_VERSION_1: DWORD = 1;
    pub const OPEN_RW_DEPTH_DEFAULT: ULONG = 1;
    pub const ATTACH_VERSION_1: DWORD = 1;
    /// VIRTUAL_DISK_ACCESS_ATTACH_RO (0x00010000) |
    /// VIRTUAL_DISK_ACCESS_DETACH (0x00040000).
    pub const ACCESS_ATTACH_RO_DETACH: DWORD = 0x0005_0000;
    /// ATTACH_VIRTUAL_DISK_FLAG_READ_ONLY (0x1) |
    /// ATTACH_VIRTUAL_DISK_FLAG_PERMANENT_LIFETIME (0x4).
    pub const ATTACH_FLAGS_ISO: DWORD = 0x0000_0005;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct VirtualStorageType {
        pub device_id: ULONG,
        pub vendor_id: GUID,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct OpenParameters {
        pub version: DWORD,
        pub rw_depth: ULONG,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct AttachParameters {
        pub version: DWORD,
        pub reserved: ULONG,
    }

    #[link(name = "virtdisk")]
    extern "system" {
        pub fn OpenVirtualDisk(
            storage_type: *const VirtualStorageType,
            path: PCWSTR,
            access_mask: DWORD,
            flags: DWORD,
            parameters: *const OpenParameters,
            handle: *mut HANDLE,
        ) -> DWORD;
        pub fn AttachVirtualDisk(
            handle: HANDLE,
            security_descriptor: *mut c_void,
            flags: DWORD,
            provider_flags: DWORD,
            parameters: *const AttachParameters,
            overlapped: *mut c_void,
        ) -> DWORD;
        pub fn DetachVirtualDisk(handle: HANDLE, flags: DWORD, provider_flags: DWORD) -> DWORD;
    }
}

struct IsoMountRec {
    letter: char,
    /// HANDLE VirtDisk dạng usize (HANDLE thô không Send — sổ
    /// chỉ được chạm qua Mutex nên an toàn).
    vdisk_handle: usize,
}

/// Sổ đăng ký các ISO do CHÍNH tiến trình này attach: khoá là
/// đường dẫn viết thường (Windows không phân biệt hoa thường).
fn iso_mounts() -> &'static Mutex<Vec<(String, IsoMountRec)>> {
    static MOUNTS: OnceLock<Mutex<Vec<(String, IsoMountRec)>>> = OnceLock::new();
    MOUNTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn iso_mount_key(iso: &Path) -> String {
    iso.to_string_lossy().to_lowercase()
}

fn register_iso_mount(iso: &Path, rec: IsoMountRec) {
    let key = iso_mount_key(iso);
    let mut mounts = iso_mounts().lock().unwrap();
    mounts.retain(|(k, _)| *k != key);
    mounts.push((key, rec));
}

fn take_iso_mount(iso: &Path) -> Option<IsoMountRec> {
    let key = iso_mount_key(iso);
    let mut mounts = iso_mounts().lock().unwrap();
    let pos = mounts.iter().position(|(k, _)| *k == key)?;
    Some(mounts.remove(pos).1)
}

/// Mặt nạ ký tự ổ hiện có (bit i = ổ 'A'+i) — chụp trước/sau
/// attach để tìm volume DVD mới của ISO.
fn logical_drives_mask() -> u32 {
    unsafe { winapi::um::fileapi::GetLogicalDrives() }
}

/// Ký tự ổ là ổ CD/DVD (DRIVE_CDROM = 5).
fn is_cdrom_letter(letter: char) -> bool {
    let root: Vec<u16> = format!("{}:\\", letter)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe { winapi::um::fileapi::GetDriveTypeW(root.as_ptr()) == 5 }
}

/// Ổ (đã mount) có chứa file ảnh cài đặt trong sources\ không.
fn cdrom_has_image(letter: char) -> bool {
    let sources = PathBuf::from(format!("{}:\\sources", letter));
    ["install.wim", "install.esd", "install.swm"]
        .iter()
        .any(|n| sources.join(n).exists())
}

/// Thông điệp dự phòng khi VirtDisk không attach được — KHÔNG
/// bao giờ còn ngõ cụt kiểu bản cũ; luôn chỉ rõ đường đi tiếp
/// đã chứng minh được ở thực địa (mount sẵn → file install.*).
fn attach_failure_hint(vd_err: &str) -> String {
    format!(
        "Không mount được ISO bằng VirtDisk API: {}\n\
         Cách xử lý: dùng công cụ của WinPE này mount sẵn file ISO (hoặc giải nén ISO ra ổ dữ liệu), \
         rồi chọn trực tiếp — hoặc gõ vào ô đường dẫn — file sources\\install.wim (hoặc .esd / .swm) \
         trong ISO đã mount, ví dụ F:\\sources\\install.wim.",
        vd_err
    )
}

/// Mount ISO bằng VirtDisk API gốc. Thành công trả ký tự ổ của
/// volume ISO; mount được ghi vào sổ đăng ký để `dismount_iso`
/// gỡ đối xứng. ISO do chính tiến trình này attach trước đó và
/// gốc ổ còn truy cập được thì dùng lại ngay.
pub fn mount_iso(iso: &Path) -> Result<char, String> {
    if let Some(rec) = take_iso_mount(iso) {
        if PathBuf::from(format!("{}:\\", rec.letter)).exists() {
            let letter = rec.letter;
            register_iso_mount(iso, rec);
            return Ok(letter);
        }
        // Mount cũ đã chết (bị tháo ngoài) — gỡ nốt phần cứng
        // còn treo rồi attach lại từ đầu.
        if rec.vdisk_handle != 0 {
            unsafe {
                let h = rec.vdisk_handle as winapi::um::winnt::HANDLE;
                virtdisk::DetachVirtualDisk(h, 0, 0);
                winapi::um::handleapi::CloseHandle(h);
            }
        }
    }
    match mount_iso_virtdisk(iso) {
        Ok((letter, handle)) => {
            register_iso_mount(iso, IsoMountRec { letter, vdisk_handle: handle });
            Ok(letter)
        }
        Err(e) => Err(attach_failure_hint(&e)),
    }
}

/// Attach thật bằng VirtDisk. Thành công trả (ký tự ổ, HANDLE
/// dạng usize) để sổ đăng ký giữ handle cho bước gỡ mount.
fn mount_iso_virtdisk(iso: &Path) -> Result<(char, usize), String> {
    use winapi::um::handleapi::CloseHandle;
    let before = logical_drives_mask();
    let wide: Vec<u16> = iso
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let storage = virtdisk::VirtualStorageType {
        device_id: virtdisk::STORAGE_DEVICE_ISO,
        vendor_id: virtdisk::VENDOR_MICROSOFT,
    };
    let open_params = virtdisk::OpenParameters {
        version: virtdisk::OPEN_VERSION_1,
        rw_depth: virtdisk::OPEN_RW_DEPTH_DEFAULT,
    };
    let mut handle: winapi::um::winnt::HANDLE = std::ptr::null_mut();
    let rc = unsafe {
        virtdisk::OpenVirtualDisk(
            &storage,
            wide.as_ptr(),
            virtdisk::ACCESS_ATTACH_RO_DETACH,
            0,
            &open_params,
            &mut handle,
        )
    };
    if rc != 0 || handle.is_null() {
        return Err(format!(
            "OpenVirtualDisk thất bại (mã lỗi Windows {} — 0x{:08X}). File ISO có thể hỏng/không phải ảnh đĩa hợp lệ, hoặc WinPE này thiếu thành phần Virtual Disk.",
            rc, rc
        ));
    }
    let attach_params = virtdisk::AttachParameters {
        version: virtdisk::ATTACH_VERSION_1,
        reserved: 0,
    };
    let rc = unsafe {
        virtdisk::AttachVirtualDisk(
            handle,
            std::ptr::null_mut(),
            virtdisk::ATTACH_FLAGS_ISO,
            0,
            &attach_params,
            std::ptr::null_mut(),
        )
    };
    if rc != 0 {
        unsafe {
            CloseHandle(handle);
        }
        return Err(format!(
            "AttachVirtualDisk thất bại (mã lỗi Windows {} — 0x{:08X}).",
            rc, rc
        ));
    }
    // Dò ký tự ổ MỚI: volume DVD của ISO hiện ra sau attach ít
    // lâu — thăm dò tối đa ~10 giây. Ưu tiên ký tự có sẵn
    // sources\install.* (chính là ISO cài Windows); không có
    // thì lấy ký tự DVD mới đầu tiên (bước đọc edition sẽ tự
    // báo nếu ISO thiếu file cài đặt).
    let mut fallback: Option<char> = None;
    for _ in 0..40 {
        let now = logical_drives_mask();
        let new_bits = now & !before;
        for i in 0..26u32 {
            if new_bits & (1u32 << i) != 0 {
                let letter = (b'A' + i as u8) as char;
                if is_cdrom_letter(letter) {
                    if cdrom_has_image(letter) {
                        return Ok((letter, handle as usize));
                    }
                    if fallback.is_none() {
                        fallback = Some(letter);
                    }
                }
            }
        }
        if let Some(l) = fallback {
            std::thread::sleep(Duration::from_millis(250));
            if cdrom_has_image(l) {
                return Ok((l, handle as usize));
            }
            return Ok((l, handle as usize));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    unsafe {
        virtdisk::DetachVirtualDisk(handle, 0, 0);
        CloseHandle(handle);
    }
    Err("Đã attach ISO bằng VirtDisk nhưng không thấy ký tự ổ DVD mới nào xuất hiện sau 10 giây.".to_string())
}

/// Gỡ mount một ISO do tiến trình này attach (DetachVirtualDisk
/// + đóng handle). ISO không có trong sổ (mount bởi tiến trình
/// khác) thì không làm gì — không đụng mount của người khác.
pub fn dismount_iso(iso: &Path) {
    if let Some(rec) = take_iso_mount(iso) {
        if rec.vdisk_handle != 0 {
            unsafe {
                let h = rec.vdisk_handle as winapi::um::winnt::HANDLE;
                virtdisk::DetachVirtualDisk(h, 0, 0);
                winapi::um::handleapi::CloseHandle(h);
            }
        }
    }
}

/// Gỡ mount MỌI ISO tiến trình này đã attach (dọn dẹp khi
/// module thoát).
pub fn dismount_all() {
    let all: Vec<(String, IsoMountRec)> = {
        let mut mounts = iso_mounts().lock().unwrap();
        mounts.drain(..).collect()
    };
    for (_, rec) in all {
        if rec.vdisk_handle != 0 {
            unsafe {
                let h = rec.vdisk_handle as winapi::um::winnt::HANDLE;
                virtdisk::DetachVirtualDisk(h, 0, 0);
                winapi::um::handleapi::CloseHandle(h);
            }
        }
    }
}
