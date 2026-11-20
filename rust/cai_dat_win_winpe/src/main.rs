#![windows_subsystem = "windows"]
//! CaiDatWinWinPE — module "Cài Đặt Win=WinPE" (Rust).
//!
//! Trình cài Windows TƯƠNG TÁC chạy bên trong WinPE: kỹ thuật viên boot
//! WinPE từ USB, chạy file exe này từ ổ dữ liệu, chọn nguồn cài đặt
//! (ISO hoặc install.wim/.esd/.swm), chọn phân vùng đích, rồi cài ngay
//! tại máy. Đây là bản mặt trước tương tác của cùng một engine mà
//! wizard Cài Đặt Win Tự Động dùng ở luồng staging tự động
//! (`WinPEInstall.exe` + stage.json) — module này KHÔNG đọc stage.json,
//! mọi lựa chọn do kỹ thuật viên quyết tại chỗ.
//!
//! Hai chế độ theo môi trường:
//! - Trong WinPE (nhận diện qua khoá registry MiniNT / giá trị
//!   PEBootType): chế độ cài đặt đầy đủ.
//! - Trong Windows thường: chỉ hiện giải thích + nút "Chuẩn bị" chép
//!   công cụ ra USB/ổ dữ liệu để sẵn sàng dùng trong WinPE.
//!
//! Kỷ luật an toàn (kế thừa winpe_install, chặn ở tầng code):
//! - Liệt kê phân vùng bằng Win32 API gốc (GetVolumeInformationW...),
//!   KHÔNG dùng WMI (không đảm bảo có trong WinPE); đối chiếu chéo
//!   bằng `diskpart list volume` (ký tự/dung lượng) và lệnh `vol`
//!   (serial dạng XXXX-XXXX — mẫu hex không phụ thuộc ngôn ngữ).
//! - Ổ chứa chính file exe và ổ chứa nguồn cài đặt bị LOẠI TRỪ khỏi
//!   đích cài theo mặc định; muốn chọn phải bật ô ghi đè riêng kèm
//!   cảnh báo, và tầng thực thi kiểm tra lại lần nữa trước khi ghi.
//! - Danh sách đích còn có tuỳ chọn TOÀN BỘ Ổ ĐĨA (xoá & chia lại từ
//!   đầu, §6b): đích là cả đĩa vật lý nhận diện bằng serial phần
//!   cứng; đĩa chứa ổ của công cụ hoặc ổ chứa nguồn bị TỪ CHỐI TUYỆT
//!   ĐỐI (không có ghi đè); phải nhập đúng serial của ĐĨA để xác
//!   nhận; thực thi đối chiếu lại serial + số đĩa chứa các ổ cấm
//!   ngay trước khi chạy diskpart chia lại (EFI/MSR/Windows/Recovery
//!   theo chuẩn, copy WinRE vào phân vùng Recovery mới, bcdboot trỏ
//!   vào phân vùng boot mới).
//! - Format chỉ khi kỹ thuật viên nhập đúng serial của phân vùng đích.
//! - Trước khi format/apply, thực thi đọc lại serial của ổ đích và
//!   đối chiếu với serial đã chọn — lệch là dừng (ký tự ổ có thể đã
//!   thay đổi do cắm/rút thiết bị).
//! - Chuỗi thao tác: [format] → DISM /Apply-Image (log theo dòng) →
//!   inject driver offline → ghi autounattend vào Windows\Panther →
//!   copy thư mục `custom` (nếu có cạnh exe) → bcdboot. Lỗi ở bước
//!   format/apply/unattend/bcdboot thì DỪNG và báo đúng bước lỗi;
//!   lỗi inject driver / copy custom chỉ cảnh báo (giống engine) vì
//!   bản Windows vẫn hoàn chỉnh và sửa được sau. Nút Khởi động lại
//!   chỉ bật sau khi cài thành công toàn bộ.
//! - Mật khẩu admin là TUỲ CHỌN (v0.7.2): để trống = tài khoản
//!   không mật khẩu (unattend bỏ hẳn phần tử <Password>); có đặt
//!   thì mật khẩu chỉ nằm trong autounattend.xml, KHÔNG bao giờ
//!   ghi vào log.
//!
//! v0.7.19 (sửa ba lỗi thực địa 2026-11-18 + một lỗi hậu boot):
//! - CHỐT BOOT: đường cài theo phân vùng dùng chung
//!   `disk::finalize_boot()` của qsu_common — tìm phân vùng boot
//!   TRÊN CÙNG ĐĨA với phân vùng Windows (GPT: EFI System theo
//!   GUID; MBR: System Reserved active), tự gán ký tự nếu thiếu,
//!   bcdboot + kiểm chứng file boot thật, lỗi kèm output của
//!   bcdboot. Thay hẳn `find_efi_letter()` cũ (quét thư mục \EFI
//!   theo ký tự ổ + mountvol /s) — ca thực địa: bung ảnh 100%
//!   nhưng bcdboot thất bại vì EFI của SSD không có ký tự ổ, kỹ
//!   thuật viên phải chạy bcdboot tay.
//! - NGUỒN ISO: mount bằng VirtDisk API gốc qua
//!   `qsu_common::iso` — KHÔNG còn phụ thuộc PowerShell/cmdlet
//!   Storage (WinPE BHT/Anhdv không có, bản cũ báo ngõ cụt).
//!   Nút "Chọn file..."/"Duyệt..." mở TRÌNH DUYỆT FILE TÍCH
//!   HỢP của module (hộp thoại file của Windows không mở được
//!   trong các WinPE này); ô gõ đường dẫn trực tiếp giữ nguyên.
//! - NÚT BẤM: luồng nền nạp nguồn/liệt kê được bọc bắt lỗi toàn
//!   diện — busy LUÔN được nhả và giao diện LUÔN quay lại trạng
//!   thái bấm được sau mọi kết quả nạp (bản cũ từng kẹt nút
//!   "Nạp nguồn" sau một lần nạp lỗi, phải mở lại module); hộp
//!   thoại trong refresh() chỉ hiện SAU khi đã cập nhật trạng
//!   thái nút.
//! - AUTOUNATTEND: thêm hai lệnh FirstLogonCommands dọn cờ
//!   "phải đổi mật khẩu ở lần đăng nhập đầu" của tài khoản
//!   admin (net user /logonpasswordchg:no + PasswordNeverExpires)
//!   — ca thực địa: AutoLogon bị chặn ở màn hình đổi mật khẩu
//!   thay vì vào thẳng desktop. Áp dụng cả khi mật khẩu trống.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::ptr;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use native_windows_gui as nwg;
use qsu_common::cmd;
use qsu_common::disk::{self, PhysicalDisk};
use qsu_common::log::Logger;
use qsu_common::settings::Settings;
use qsu_common::sys;
use winapi::shared::minwindef::{DWORD, HKEY};
use winapi::shared::windef::{HWND, RECT};
use winapi::um::fileapi::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
};
use winapi::um::winbase::{DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE};
use winapi::um::winnt::KEY_READ;
use winapi::um::winreg::{RegCloseKey, RegOpenKeyExW};
use winapi::um::winuser::{
    GetClientRect, GetWindowLongPtrW, InvalidateRect, SetWindowLongPtrW, ES_PASSWORD,
    ES_READONLY, GWL_STYLE,
};

const GB: u64 = 1_073_741_824;

// ---------------------------------------------------------------------------
// Nhận diện môi trường WinPE
// ---------------------------------------------------------------------------
fn reg_key_exists(subkey: &str) -> bool {
    unsafe {
        let sk = sys::wide(subkey);
        let mut hkey: HKEY = ptr::null_mut();
        let rc = RegOpenKeyExW(sys::hklm(), sk.as_ptr(), 0, KEY_READ, &mut hkey);
        if rc == 0 {
            RegCloseKey(hkey);
            true
        } else {
            false
        }
    }
}

/// WinPE luôn có khoá `...\Control\MiniNT` và giá trị `PEBootType`
/// trong `...\Control`; Windows thường không có cả hai.
fn detect_winpe() -> bool {
    if reg_key_exists("SYSTEM\\CurrentControlSet\\Control\\MiniNT") {
        return true;
    }
    if let Some(v) = sys::reg_get_sz(sys::hklm(), "SYSTEM\\CurrentControlSet\\Control", "PEBootType") {
        if !v.trim().is_empty() {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Phân vùng (volume) — liệt kê bằng Win32 API, không WMI
// ---------------------------------------------------------------------------
#[derive(Clone)]
struct VolumeInfo {
    letter: char,
    label: String,
    fs: String,
    serial: u32,
    total: u64,
    free: u64,
    drive_type: u32,
    has_windows: bool,
    has_users: bool,
}

impl VolumeInfo {
    fn type_vi(&self) -> &'static str {
        match self.drive_type {
            DRIVE_FIXED => "Ổ cố định",
            DRIVE_REMOVABLE => "USB/di động",
            DRIVE_RAMDISK => "Ổ RAM (WinPE)",
            _ => "Khác",
        }
    }
    /// Mô tả nội dung hiện có trên phân vùng (để kỹ thuật viên cân nhắc)
    fn contents_vi(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.has_windows {
            parts.push("có Windows cũ".to_string());
        }
        if self.has_users {
            parts.push("có thư mục Users (dữ liệu người dùng)".to_string());
        }
        if parts.is_empty() {
            "không thấy Windows/dữ liệu người dùng".to_string()
        } else {
            parts.join("; ")
        }
    }
}

/// Serial hiển thị dạng XXXX-XXXX như lệnh `vol` của Windows.
fn format_serial(serial: u32) -> String {
    format!("{:04X}-{:04X}", (serial >> 16) & 0xFFFF, serial & 0xFFFF)
}

/// Một dòng trong danh sách đích cài: hoặc một PHÂN VÙNG (đường giữ
/// nguyên cấu trúc đĩa) hoặc TOÀN BỘ MỘT Ổ ĐĨA VẬT LÝ (đường wipe —
/// xoá & chia lại từ đầu, §6b).
#[derive(Clone)]
enum TargetEntry {
    Volume(VolumeInfo),
    Disk(PhysicalDisk),
}

/// Lý do một ĐĨA bị cấm làm đích wipe (None = được chọn). Khác với
/// phân vùng: đĩa chứa ổ của công cụ / ổ chứa nguồn bị cấm TUYỆT ĐỐI,
/// không có cơ chế ghi đè — wipe đĩa đó là tự phá nguồn đang chạy.
fn disk_block_reason(m: &Model, d: &PhysicalDisk) -> Option<String> {
    if let Some(l) = m.exe_letter {
        if d.contains_letter(l) {
            return Some(format!("đĩa chứa chính công cụ này (ổ {}:)", l));
        }
    }
    if let Some(src) = &m.source_file {
        if let Some(l) = path_letter(src) {
            if d.contains_letter(l) {
                return Some(format!("đĩa chứa nguồn cài đặt (ổ {}:)", l));
            }
        }
    }
    None
}

/// Đọc serial hiện tại của một ký tự ổ (dùng để đối chiếu lại ngay
/// trước khi ghi — ký tự ổ có thể đã đổi chủ sau khi cắm/rút thiết bị).
fn volume_serial_of(letter: char) -> Option<u32> {
    unsafe {
        let root = sys::wide(&format!("{}:\\", letter));
        let mut label_buf = vec![0u16; 256];
        let mut serial: DWORD = 0;
        let mut fs_buf = vec![0u16; 64];
        let ok = GetVolumeInformationW(
            root.as_ptr(),
            label_buf.as_mut_ptr(),
            label_buf.len() as DWORD,
            &mut serial,
            ptr::null_mut(),
            ptr::null_mut(),
            fs_buf.as_mut_ptr(),
            fs_buf.len() as DWORD,
        );
        if ok == 0 {
            None
        } else {
            Some(serial)
        }
    }
}

/// Liệt kê mọi volume có ký tự ổ: cố định + USB + RAM disk (bỏ qua
/// CD-ROM/mạng). Serial/nhãn/hệ file từ GetVolumeInformationW —
/// không phụ thuộc ngôn ngữ hiển thị.
fn enumerate_volumes() -> Vec<VolumeInfo> {
    let mut out = Vec::new();
    unsafe {
        let mut buf = vec![0u16; 512];
        let n = GetLogicalDriveStringsW(buf.len() as DWORD, buf.as_mut_ptr());
        if n == 0 {
            return out;
        }
        let mut i = 0usize;
        while i < buf.len() && buf[i] != 0 {
            let start = i;
            while i < buf.len() && buf[i] != 0 {
                i += 1;
            }
            let root = sys::from_wide(&buf[start..i]);
            i += 1;
            if root.len() < 2 {
                continue;
            }
            let root_w = sys::wide(&root);
            let dtype = GetDriveTypeW(root_w.as_ptr());
            if dtype != DRIVE_FIXED && dtype != DRIVE_REMOVABLE && dtype != DRIVE_RAMDISK {
                continue;
            }
            let letter = root.chars().next().unwrap_or('?').to_ascii_uppercase();
            let mut label_buf = vec![0u16; 256];
            let mut serial: DWORD = 0;
            let mut fs_buf = vec![0u16; 64];
            let ok = GetVolumeInformationW(
                root_w.as_ptr(),
                label_buf.as_mut_ptr(),
                label_buf.len() as DWORD,
                &mut serial,
                ptr::null_mut(),
                ptr::null_mut(),
                fs_buf.as_mut_ptr(),
                fs_buf.len() as DWORD,
            );
            if ok == 0 {
                continue; // ổ chưa format / chưa sẵn sàng
            }
            let mut free: u64 = 0;
            let mut total: u64 = 0;
            GetDiskFreeSpaceExW(
                root_w.as_ptr(),
                &mut free as *mut u64 as *mut _,
                &mut total as *mut u64 as *mut _,
                ptr::null_mut(),
            );
            out.push(VolumeInfo {
                letter,
                label: sys::from_wide(&label_buf),
                fs: sys::from_wide(&fs_buf),
                serial,
                total,
                free,
                drive_type: dtype,
                has_windows: Path::new(&format!("{}Windows", root)).is_dir(),
                has_users: Path::new(&format!("{}Users", root)).is_dir(),
            });
        }
    }
    out.sort_by_key(|v| v.letter);
    out
}

/// Đối chiếu chéo bằng diskpart: phân tích `list volume` lấy ký tự ổ +
/// dung lượng gần đúng. Output diskpart bị bản địa hoá nên việc phân
/// tích chỉ dựa trên MẪU không phụ thuộc ngôn ngữ: một dòng dữ liệu có
/// cặp <số> <đơn vị KB/MB/GB/TB>, và ký tự ổ là token 1 ký tự chữ cái
/// đứng ngay sau số thứ tự volume. Không phân tích được thì bỏ qua —
/// số liệu Win32 vẫn là chuẩn.
fn diskpart_list_volume() -> Vec<(char, u64)> {
    let script_path = std::env::temp_dir().join("qsu_dp_list.txt");
    if std::fs::write(&script_path, "list volume\r\n").is_err() {
        return Vec::new();
    }
    let sp = script_path.to_string_lossy().into_owned();
    let out = match cmd::run_capture("diskpart", &["/s", &sp]) {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    let unit_mult = |u: &str| -> Option<f64> {
        match u.to_ascii_uppercase().as_str() {
            "KB" => Some(1024.0),
            "MB" => Some(1024.0 * 1024.0),
            "GB" => Some(1024.0 * 1024.0 * 1024.0),
            "TB" => Some(1024.0 * 1024.0 * 1024.0 * 1024.0),
            _ => None,
        }
    };
    let mut res = Vec::new();
    for line in out.stdout.lines() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        // Tìm cặp <số> <đơn vị> trong dòng
        let mut size: Option<u64> = None;
        for w in toks.windows(2) {
            if let (Ok(num), Some(m)) = (
                w[0].replace(',', ".").parse::<f64>(),
                unit_mult(w[1]),
            ) {
                size = Some((num * m) as u64);
                break;
            }
        }
        let size = match size {
            Some(s) => s,
            None => continue,
        };
        // Ký tự ổ: token 1 ký tự chữ cái, token ngay trước nó là số thứ tự
        for i in 1..toks.len() {
            let t = toks[i];
            if t.chars().count() == 1
                && t.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
                && toks[i - 1].parse::<u32>().is_ok()
            {
                res.push((t.chars().next().unwrap().to_ascii_uppercase(), size));
                break;
            }
        }
    }
    res
}

/// Đối chiếu serial bằng lệnh `vol X:` — mẫu hex XXXX-XXXX trong output
/// không phụ thuộc ngôn ngữ hiển thị.
fn vol_serial_of(letter: char) -> Option<String> {
    let arg = format!("{}:", letter);
    let out = cmd::run_capture("vol", &[&arg]).ok()?;
    for tok in out.stdout.split_whitespace() {
        let t = tok.trim();
        let bytes = t.as_bytes();
        if bytes.len() == 9
            && bytes[4] == b'-'
            && bytes.iter().enumerate().all(|(i, b)| i == 4 || b.is_ascii_hexdigit())
        {
            return Some(t.to_ascii_uppercase());
        }
    }
    None
}

/// Chạy đối chiếu chéo và trả dòng tóm tắt (ghi log + hiện trạng thái).
fn cross_check(volumes: &[VolumeInfo]) -> String {
    let mut notes: Vec<String> = Vec::new();
    // 1) diskpart: ký tự + dung lượng
    let dp = diskpart_list_volume();
    if dp.is_empty() {
        notes.push("diskpart: không phân tích được output (bỏ qua, dùng số liệu Win32)".to_string());
    } else {
        let mut matched = 0usize;
        for (letter, approx) in &dp {
            if let Some(v) = volumes.iter().find(|v| v.letter == *letter) {
                let diff = (v.total as i64 - *approx as i64).abs();
                if v.total == 0 || diff <= (v.total / 33) as i64 + GB as i64 {
                    matched += 1;
                } else {
                    notes.push(format!(
                        "diskpart: ổ {} lệch dung lượng (Win32 {} / diskpart ~{})",
                        letter,
                        sys::format_gb(v.total),
                        sys::format_gb(*approx)
                    ));
                }
            }
        }
        notes.push(format!("diskpart: khớp {}/{} ổ về ký tự + dung lượng", matched, dp.len()));
    }
    // 2) vol: serial
    let mut ok = 0usize;
    let mut total = 0usize;
    for v in volumes {
        total += 1;
        match vol_serial_of(v.letter) {
            Some(s) if s == format_serial(v.serial) => ok += 1,
            Some(s) => notes.push(format!(
                "vol: ổ {} lệch serial (Win32 {} / vol {})",
                v.letter,
                format_serial(v.serial),
                s
            )),
            None => {}
        }
    }
    notes.push(format!("vol: serial khớp {}/{} ổ", ok, total));
    notes.join(" · ")
}

// ---------------------------------------------------------------------------
// DISM + đọc phiên bản (cùng cách wizard Cài Đặt Win Tự Động)
// ---------------------------------------------------------------------------
#[derive(Clone)]
struct EditionInfo {
    index: u32,
    name: String,
    edition: String,
    build: u32,
    arch: String,
    size_bytes: u64,
}

/// Đường dẫn dism.exe: ưu tiên bản trong SystemRoot (trong WinPE là
/// X:\Windows\System32\dism.exe) như engine winpe_install.
fn dism_exe() -> String {
    let sr = std::env::var("SystemRoot").unwrap_or_else(|_| "X:\\Windows".to_string());
    let p = Path::new(&sr).join("System32").join("dism.exe");
    if p.exists() {
        p.to_string_lossy().into_owned()
    } else {
        "dism".to_string()
    }
}

fn dism_wim_info(wim: &Path, index: Option<u32>) -> Result<String, String> {
    let wim_arg = format!("/WimFile:{}", wim.to_string_lossy());
    let mut args: Vec<String> = vec!["/English".to_string(), "/Get-WimInfo".to_string(), wim_arg];
    if let Some(i) = index {
        args.push(format!("/Index:{}", i));
    }
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = cmd::run_capture(&dism_exe(), &arg_refs)
        .map_err(|e| format!("không chạy được DISM: {}", e))?;
    if out.code != Some(0) {
        return Err(format!(
            "DISM /Get-WimInfo thất bại (mã {:?}): {}",
            out.code,
            out.stderr.trim()
        ));
    }
    Ok(out.stdout)
}

/// Phân tích các dòng "Key : Value" trong output DISM
fn parse_kv(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(pos) = line.find(':') {
            let k = line[..pos].trim().to_string();
            let v = line[pos + 1..].trim().to_string();
            if !k.is_empty() {
                out.push((k, v));
            }
        }
    }
    out
}

fn kv_get(kv: &[(String, String)], key: &str) -> String {
    kv.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

fn parse_build(version: &str) -> u32 {
    version.split('.').nth(2).and_then(|p| p.trim().parse().ok()).unwrap_or(0)
}

fn load_editions(wim: &Path) -> Result<Vec<EditionInfo>, String> {
    let summary = dism_wim_info(wim, None)?;
    let indexes: Vec<u32> = parse_kv(&summary)
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("Index"))
        .filter_map(|(_, v)| v.parse().ok())
        .collect();
    if indexes.is_empty() {
        return Err("DISM không liệt kê được index nào trong file image.".to_string());
    }
    let mut out = Vec::new();
    for idx in indexes {
        let detail = dism_wim_info(wim, Some(idx))?;
        let kv = parse_kv(&detail);
        let version = kv_get(&kv, "Version");
        let size_bytes = kv_get(&kv, "Size")
            .replace(',', "")
            .replace("bytes", "")
            .trim()
            .parse()
            .unwrap_or(0);
        out.push(EditionInfo {
            index: idx,
            name: kv_get(&kv, "Name"),
            edition: kv_get(&kv, "Edition"),
            build: parse_build(&version),
            arch: kv_get(&kv, "Architecture"),
            size_bytes,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Mount ISO — VirtDisk API gốc qua qsu_common::iso (v0.7.19)
// ---------------------------------------------------------------------------
// Các bản trước mount bằng PowerShell Mount-DiskImage và khi
// WinPE không có PowerShell (BHT/Anhdv đều vậy) thì báo ngõ cụt
// "WinPE này không có PowerShell/cmdlet Storage nên không mount
// được ISO" — thực địa 2026-11-18. Từ v0.7.19 mount/dismount đi
// thẳng xuống VirtDisk API trong thư viện dùng chung (sổ đăng
// ký mount + gỡ đối xứng bằng DetachVirtualDisk); lỗi attach
// mang sẵn hướng dẫn dự phòng (mount sẵn bằng công cụ của
// WinPE rồi chọn/gõ thẳng sources\install.*).
use qsu_common::iso::{dismount_iso, mount_iso};

/// Tìm file image trong ISO đã mount: install.wim / .esd / .swm
fn find_image_file(mounted_letter: char) -> Option<PathBuf> {
    let sources = PathBuf::from(format!("{}:\\sources", mounted_letter));
    for name in ["install.wim", "install.esd", "install.swm"] {
        let p = sources.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Mẫu SWM đi kèm khi image là file .swm chia nhỏ: cùng thư mục, cùng
/// tiền tố (bỏ số thứ tự ở cuối tên) + "*.swm".
fn swm_pattern_for(image: &Path) -> Option<String> {
    let name = image.file_name()?.to_string_lossy().into_owned();
    if !name.to_ascii_lowercase().ends_with(".swm") {
        return None;
    }
    let stem = &name[..name.len() - 4];
    let stem = stem.trim_end_matches(|c: char| c.is_ascii_digit());
    let dir = image.parent()?;
    Some(format!("{}\\{}*.swm", dir.to_string_lossy(), stem))
}

// ---------------------------------------------------------------------------
// autounattend.xml — cùng bản sinh của wizard (đặc tả §7 của luồng cài
// Win): vi-VN, bỏ qua OOBE, tạo tài khoản admin + AutoLogon 1 lần.
// Mật khẩu là TUỲ CHỌN (v0.7.2, cùng logic wizard): để trống → tài
// khoản KHÔNG mật khẩu và unattend BỎ HẲN phần tử <Password> ở cả
// LocalAccount lẫn AutoLogon (cấu trúc Schneegans — AutoLogon không
// mật khẩu được Setup hỗ trợ; KHÔNG sinh <Value> rỗng). Khi có mật
// khẩu, mật khẩu chỉ xuất hiện tại đây và không vào log.
// FirstLogonCommands (v0.7.19) chỉ chứa hai lệnh DỌN CỜ MẬT KHẨU
// cho chính tài khoản admin vừa tạo (không phải lệnh cài ứng
// dụng): `net user <tên> /logonpasswordchg:no` dọn cờ "phải đổi
// mật khẩu ở lần đăng nhập đầu" (pwdLastSet = 0) mà Setup gắn
// cho tài khoản tạo từ unattend — cờ này chặn AutoLogon ở màn
// hình đổi mật khẩu (thực địa 2026-11-18); lệnh PowerShell đặt
// PasswordNeverExpires cho tài khoản.
// ---------------------------------------------------------------------------
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// Hồ sơ CORE (v0.7.21, tham số `core_profile`): cho ảnh Windows
// rút gọn kiểu Tiny11 Core — các ảnh này thiếu nhiều component mà
// một unattend đầy đủ chạm tới, và thực địa cho thấy chúng boot
// vòng lỗi 0xEF khi cài bằng pipeline chuẩn nhưng lên ngon khi
// cài tay tối thiểu kiểu WinNTSetup. Khác biệt của hồ sơ CORE:
// unattend KHÔNG mang <FirstLogonCommands> (không lệnh dọn cờ
// mật khẩu, không lệnh hậu cài); cờ "phải đổi mật khẩu ở lần
// đăng nhập đầu" của tài khoản admin được dọn bằng
// SetupComplete.cmd tối thiểu do module ghi vào
// Windows\Setup\Scripts của bản Windows mới (chạy bằng quyền
// SYSTEM ở cuối pha setup, TRƯỚC lần đăng nhập đầu — nên
// AutoLogon vẫn vào thẳng desktop). Phần còn lại của unattend
// (locale, tên máy, OOBE rút gọn, LocalAccount, AutoLogon) giữ
// nguyên như hồ sơ standard.
fn build_unattend(st: &Settings, computer_name: &str, admin_user: &str, admin_password: &str, core_profile: bool) -> String {
    let comp = xml_escape(computer_name);
    let user = xml_escape(admin_user);
    let pass = xml_escape(admin_password);
    // Khối <Password> cho LocalAccount (thụt 12) và AutoLogon (thụt 8);
    // chuỗi rỗng khi không đặt mật khẩu — xem ghi chú đầu hàm.
    let account_pw = if admin_password.is_empty() {
        String::new()
    } else {
        format!(
            "\n            <Password>\n              <Value>{}</Value>\n              <PlainText>true</PlainText>\n            </Password>",
            pass
        )
    };
    let autologon_pw = if admin_password.is_empty() {
        String::new()
    } else {
        format!(
            "\n        <Password>\n          <Value>{}</Value>\n          <PlainText>true</PlainText>\n        </Password>",
            pass
        )
    };
    // Lệnh chạy ở lần đăng nhập đầu (v0.7.19, cùng bản sửa với
    // wizard): tài khoản Setup tạo từ unattend mang cờ "phải
    // đổi mật khẩu ở lần đăng nhập đầu tiên" (pwdLastSet = 0)
    // — thực địa 2026-11-18: AutoLogon bị chặn ở màn hình đổi
    // mật khẩu thay vì vào thẳng desktop. Hai lệnh này dọn cờ
    // đó + đặt mật khẩu không hết hạn, áp dụng CẢ khi mật khẩu
    // để trống lẫn khi có đặt.
    let fl_commands = format!(
        r#"        <SynchronousCommand wcm:action="add">
          <Order>1</Order>
          <CommandLine>net user {user} /logonpasswordchg:no</CommandLine>
          <Description>Không bắt đổi mật khẩu ở lần đăng nhập đầu tiên</Description>
          <RequiresUserInput>false</RequiresUserInput>
        </SynchronousCommand>
        <SynchronousCommand wcm:action="add">
          <Order>2</Order>
          <CommandLine>powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "Set-LocalUser -Name '{user}' -PasswordNeverExpires $true"</CommandLine>
          <Description>Mật khẩu tài khoản admin không hết hạn</Description>
          <RequiresUserInput>false</RequiresUserInput>
        </SynchronousCommand>"#,
        user = user
    );
    // Khối <FirstLogonCommands> hoàn chỉnh của hồ sơ standard
    // (hai lệnh dọn cờ mật khẩu + lệnh hậu cài). Hồ sơ CORE: khối
    // này VẮNG HẲN trong unattend — xem ghi chú đầu hàm.
    let fl_block = if core_profile {
        String::new()
    } else {
        format!(
            "      <FirstLogonCommands>\n{flc}\n        <SynchronousCommand wcm:action=\"add\">\n          <Order>3</Order>\n          <CommandLine>{post}</CommandLine>\n          <Description>Tự cài UltraViewer, Chrome và 7-Zip sau khi vào Windows mới</Description>\n          <RequiresUserInput>false</RequiresUserInput>\n        </SynchronousCommand>\n      </FirstLogonCommands>\n",
            flc = fl_commands,
            post = xml_escape(qsu_common::post_install::FIRST_LOGON_COMMAND),
        )
    };
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend">
  <settings pass="specialize">
    <component name="Microsoft-Windows-International-Core" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <InputLocale>{input}</InputLocale>
      <SystemLocale>{locale}</SystemLocale>
      <UILanguage>{ui}</UILanguage>
      <UserLocale>{locale}</UserLocale>
    </component>
    <component name="Microsoft-Windows-Shell-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <ComputerName>{comp}</ComputerName>
      <TimeZone>{tz}</TimeZone>
    </component>
  </settings>
  <settings pass="oobeSystem">
    <component name="Microsoft-Windows-International-Core" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <InputLocale>{input}</InputLocale>
      <SystemLocale>{locale}</SystemLocale>
      <UILanguage>{ui}</UILanguage>
      <UserLocale>{locale}</UserLocale>
    </component>
    <component name="Microsoft-Windows-Shell-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <OOBE>
        <HideEULAPage>true</HideEULAPage>
        <HideOEMRegistrationScreen>true</HideOEMRegistrationScreen>
        <HideOnlineAccountScreens>true</HideOnlineAccountScreens>
        <HideWirelessSetupInOOBE>true</HideWirelessSetupInOOBE>
        <ProtectYourPC>3</ProtectYourPC>
      </OOBE>
      <UserAccounts>
        <LocalAccounts>
          <LocalAccount wcm:action="add">
            <Name>{user}</Name>
            <Group>Administrators</Group>{account_pw}
          </LocalAccount>
        </LocalAccounts>
      </UserAccounts>
      <AutoLogon>
        <Enabled>true</Enabled>
        <Username>{user}</Username>{autologon_pw}
        <LogonCount>1</LogonCount>
      </AutoLogon>
{fl_block}
    </component>
  </settings>
</unattend>
"#,
        input = xml_escape(&st.input_locale),
        locale = xml_escape(&st.locale),
        ui = xml_escape(&st.ui_language),
        tz = xml_escape(&st.timezone),
        comp = comp,
        user = user,
        fl_block = fl_block,
    )
}

// ---------------------------------------------------------------------------
// Thao tác đĩa ở tầng thực thi (kế thừa winpe_install)
// ---------------------------------------------------------------------------
/// Format một phân vùng: format.com trước, diskpart dự phòng (như engine).
fn format_partition(log: &Arc<Logger>, letter: char) -> bool {
    let target = format!("{}:", letter);
    match cmd::run_logged(log, "format", &[&target, "/fs:NTFS", "/q", "/y", "/v:Windows"]) {
        Ok(out) if out.code == Some(0) => return true,
        Ok(out) => log.line(&format!("format.com trả mã {:?} — thử diskpart", out.code)),
        Err(e) => log.line(&format!("không chạy được format.com: {} — thử diskpart", e)),
    }
    let script = format!("select volume {}\nformat fs=ntfs quick label=\"Windows\"\n", letter);
    let script_path = std::env::temp_dir().join("qsu_diskpart_fmt.txt");
    if std::fs::write(&script_path, &script).is_err() {
        return false;
    }
    let sp = script_path.to_string_lossy().into_owned();
    match cmd::run_logged(log, "diskpart", &["/s", &sp]) {
        Ok(out) => out.code == Some(0),
        Err(_) => false,
    }
}

/// (Đã gỡ ở v0.7.19) Hàm find_efi_letter() cũ — quét các ổ ĐÃ CÓ
/// ký tự để tìm thư mục \EFI ở gốc, dự phòng mountvol /s — không
/// thấy được phân vùng EFI chưa có ký tự ổ và có thể trỏ nhầm
/// phân vùng EFI của đĩa khác (rủi ro R1; ca thực địa
/// 2026-11-18: bung ảnh 100% nhưng bcdboot thất bại, phải chạy
/// bcdboot tay). Bước chốt boot của đường cài theo phân vùng nay
/// dùng chung `disk::finalize_boot()` trong qsu_common.

/// Copy đệ quy một thư mục (dùng cho payload `custom`). Nội dung chỉ
/// được copy nguyên trạng — không bao giờ được diễn giải hay thực thi.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<u64> {
    std::fs::create_dir_all(dst)?;
    let mut count = 0u64;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let ft = entry.file_type()?;
        if ft.is_dir() {
            count += copy_dir_recursive(&from, &to)?;
        } else if ft.is_file() {
            std::fs::copy(&from, &to)?;
            count += 1;
        }
    }
    Ok(count)
}

fn valid_computer_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 15
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !name.chars().all(|c| c.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// Trạng thái dùng chung luồng nền -> giao diện (qua NWG Notice)
// ---------------------------------------------------------------------------
struct SourceLoaded {
    mounted_letter: Option<char>,
    image_path: PathBuf,
    swm_pattern: Option<String>,
    editions: Vec<EditionInfo>,
}

/// Chế độ của trình duyệt file TÍCH HỢP (v0.7.19): chọn file
/// nguồn cài đặt, hoặc chọn thư mục driver. Trình duyệt này
/// thay hẳn hộp thoại file của Windows (NWG FileDialog /
/// GetOpenFileName) — hộp thoại đó KHÔNG mở được trong WinPE
/// BHT/Anhdv (thực địa 2026-11-18: bấm "Chọn file..." không có
/// cửa sổ nào hiện), trong khi cửa sổ Win32 thường của module
/// thì chạy bình thường.
#[derive(Clone, Copy, PartialEq)]
enum BrowserMode {
    SourceFile,
    DriverFolder,
}

#[derive(Default)]
struct Shared {
    log_lines: VecDeque<String>,
    volumes: Option<(Vec<VolumeInfo>, Vec<PhysicalDisk>, String)>,
    source: Option<Result<SourceLoaded, String>>,
    progress: Option<(u32, String)>,
    status: Option<String>,
    install_done: Option<Result<String, String>>,
    busy: bool,
}

type SharedRef = Arc<Mutex<Shared>>;

fn push_log(shared: &SharedRef, sender: &nwg::NoticeSender, msg: &str) {
    {
        let mut s = shared.lock().unwrap();
        s.log_lines.push_back(msg.to_string());
        while s.log_lines.len() > 400 {
            s.log_lines.pop_front();
        }
    }
    sender.notice();
}

fn set_status(shared: &SharedRef, sender: &nwg::NoticeSender, msg: &str) {
    {
        let mut s = shared.lock().unwrap();
        s.status = Some(msg.to_string());
    }
    sender.notice();
}

fn set_progress(shared: &SharedRef, sender: &nwg::NoticeSender, pct: u32, label: &str) {
    {
        let mut s = shared.lock().unwrap();
        s.progress = Some((pct, label.to_string()));
    }
    sender.notice();
}

// ---------------------------------------------------------------------------
// Hộp thoại
// ---------------------------------------------------------------------------
fn info_box(title: &str, content: &str) {
    let p = nwg::MessageParams { title, content, buttons: nwg::MessageButtons::Ok, icons: nwg::MessageIcons::Info };
    nwg::message(&p);
}

fn error_box(title: &str, content: &str) {
    let p = nwg::MessageParams { title, content, buttons: nwg::MessageButtons::Ok, icons: nwg::MessageIcons::Error };
    nwg::message(&p);
}

fn confirm_box(title: &str, content: &str, warning: bool) -> bool {
    let p = nwg::MessageParams {
        title,
        content,
        buttons: nwg::MessageButtons::YesNo,
        icons: if warning { nwg::MessageIcons::Warning } else { nwg::MessageIcons::Question },
    };
    nwg::message(&p) == nwg::MessageChoice::Yes
}

fn set_edit_style(ctrl_hwnd: Option<HWND>, add_style: u32) {
    if let Some(hwnd) = ctrl_hwnd {
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            SetWindowLongPtrW(hwnd, GWL_STYLE, style | add_style as isize);
            InvalidateRect(hwnd, std::ptr::null(), 1);
        }
    }
}

fn lv_insert_row(lv: &nwg::ListView, row: i32, cols: &[String]) {
    for (c, t) in cols.iter().enumerate() {
        lv.insert_item(nwg::InsertListViewItem {
            index: Some(row),
            column_index: c as i32,
            text: Some(t.clone()),
            ..Default::default()
        });
    }
}

// ---------------------------------------------------------------------------
// Luồng nền: liệt kê phân vùng + đối chiếu chéo
// ---------------------------------------------------------------------------
fn spawn_enumerate(shared: SharedRef, sender: nwg::NoticeSender) {
    {
        shared.lock().unwrap().busy = true;
    }
    set_status(&shared, &sender, "Đang liệt kê phân vùng (Win32 + đối chiếu diskpart/vol)...");
    std::thread::spawn(move || {
        // Bọc bắt lỗi toàn diện như luồng nạp nguồn (v0.7.19):
        // panic trong liệt kê cũng phải nhả `busy`, nếu không
        // mọi nút bấm kẹt cứng tới khi mở lại module.
        let work = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let vols = enumerate_volumes();
            let summary = cross_check(&vols);
            // Đĩa vật lý cho tuỳ chọn đích "Toàn bộ ổ đĩa" (§6b)
            let disks = disk::list_physical_disks();
            (vols, disks, summary)
        }));
        match work {
            Ok((vols, disks, summary)) => {
                push_log(&shared, &sender, &format!("Đối chiếu chéo: {}", summary));
                push_log(&shared, &sender, &format!("Đã liệt kê {} đĩa vật lý.", disks.len()));
                let mut s = shared.lock().unwrap();
                s.volumes = Some((vols, disks, summary));
                s.busy = false;
                drop(s);
            }
            Err(_) => {
                push_log(&shared, &sender, "LỖI nội bộ khi liệt kê ổ đĩa (luồng nền bị sập) — bấm \"Quét lại ổ đĩa\" để thử lại.");
                let mut s = shared.lock().unwrap();
                s.busy = false;
                s.status = Some("Liệt kê ổ đĩa lỗi nội bộ — bấm \"Quét lại ổ đĩa\" để thử lại.".to_string());
                drop(s);
            }
        }
        sender.notice();
    });
}

// ---------------------------------------------------------------------------
// Luồng nền: nạp nguồn (mount ISO nếu cần + đọc danh sách phiên bản)
// ---------------------------------------------------------------------------
fn spawn_load_source(
    shared: SharedRef,
    sender: nwg::NoticeSender,
    path: PathBuf,
    prev_mounted: Option<(PathBuf, char)>,
) {
    {
        shared.lock().unwrap().busy = true;
    }
    set_status(&shared, &sender, &format!("Đang nạp nguồn {}...", path.display()));
    std::thread::spawn(move || {
        // Bọc bắt lỗi TOÀN DIỆN (v0.7.19): bất kỳ panic nào trong
        // luồng nền cũng thành một kết quả Err bình thường —
        // `busy` LUÔN được nhả và notice LUÔN được gửi, giao
        // diện không bao giờ kẹt ở trạng thái khoá nút như ca
        // thực địa "Không cho bấm nạp" (phải mở lại module).
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<SourceLoaded, String> {
            // Gỡ mount ISO cũ nếu đổi nguồn
            if let Some((prev_iso, _)) = &prev_mounted {
                if prev_iso != &path {
                    dismount_iso(prev_iso);
                }
            }
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if ext == "iso" {
                if let Some((prev_iso, prev_letter)) = &prev_mounted {
                    if prev_iso == &path {
                        // ISO cũ vẫn đang mount — dùng lại
                        let image = find_image_file(*prev_letter)
                            .ok_or_else(|| "ISO đã mount nhưng thiếu sources\\install.*".to_string())?;
                        let editions = load_editions(&image)?;
                        return Ok(SourceLoaded {
                            mounted_letter: Some(*prev_letter),
                            swm_pattern: swm_pattern_for(&image),
                            image_path: image,
                            editions,
                        });
                    }
                }
                // Mount bằng VirtDisk API gốc (qsu_common::iso) —
                // KHÔNG qua PowerShell (v0.7.19). Lỗi attach đã
                // mang sẵn hướng dẫn dự phòng trong thông điệp.
                let letter = mount_iso(&path)?;
                match find_image_file(letter) {
                    Some(image) => match load_editions(&image) {
                        Ok(editions) => Ok(SourceLoaded {
                            mounted_letter: Some(letter),
                            swm_pattern: swm_pattern_for(&image),
                            image_path: image,
                            editions,
                        }),
                        Err(e) => {
                            dismount_iso(&path);
                            Err(e)
                        }
                    },
                    None => {
                        dismount_iso(&path);
                        Err("Trong ISO không có sources\\install.wim / .esd / .swm.".to_string())
                    }
                }
            } else if ["wim", "esd", "swm"].contains(&ext.as_str()) {
                let editions = load_editions(&path)?;
                Ok(SourceLoaded {
                    mounted_letter: None,
                    swm_pattern: swm_pattern_for(&path),
                    image_path: path.clone(),
                    editions,
                })
            } else {
                Err("File nguồn phải là .iso hoặc install.wim / .esd / .swm.".to_string())
            }
        }))
        .unwrap_or_else(|_| {
            Err("Lỗi nội bộ khi nạp nguồn (luồng nền bị sập) — hãy thử nạp lại; nếu lặp lại, ghi lại đường dẫn file nguồn và báo người hỗ trợ.".to_string())
        });
        let mut s = shared.lock().unwrap();
        s.source = Some(result);
        s.busy = false;
        drop(s);
        sender.notice();
    });
}

// ---------------------------------------------------------------------------
// Kế hoạch cài đặt (đã kiểm tra ở UI, thực thi kiểm tra lại lần nữa)
// ---------------------------------------------------------------------------
/// Đích wipe toàn bộ ổ đĩa (§6b) khi người dùng chọn một dòng ĐĨA.
#[derive(Clone)]
struct WipeTarget {
    disk_number: u32,
    disk_serial: String,
    /// Danh sách phân vùng hiện có trên đĩa (cho hộp xác nhận + log)
    disk_parts: String,
}

struct InstallPlan {
    image_path: PathBuf,
    swm_pattern: Option<String>,
    index: u32,
    edition_name: String,
    target_letter: char,
    target_serial: u32,
    format: bool,
    driver_dir: Option<PathBuf>,
    unattend_xml: Option<String>,
    /// Tên tài khoản admin trong autounattend (rỗng khi không sinh
    /// autounattend) + cờ "không đặt mật khẩu" — chỉ để hộp xác nhận
    /// và log nói rõ trạng thái; mật khẩu KHÔNG nằm trong plan.
    admin_user: String,
    admin_passwordless: bool,
    /// Hồ sơ CORE cho ảnh rút gọn (v0.7.21): bước thực thi bỏ qua
    /// agent hậu cài, ghi SetupComplete.cmd tối thiểu thay thế.
    core_profile: bool,
    custom_dir: Option<PathBuf>,
    log_dir: PathBuf,
    uefi: bool,
    /// Các ký tự ổ bị cấm làm đích (ổ chứa exe + ổ chứa nguồn) — tầng
    /// thực thi đối chiếu lại, không tin tưởng riêng tầng giao diện.
    forbidden_letters: Vec<char>,
    /// Some(...) khi đích là TOÀN BỘ Ổ ĐĨA (xoá & chia lại). Khi đó
    /// target_letter/target_serial chỉ là trị giữ chỗ — ký tự ổ thật
    /// của phân vùng Windows mới được xác định sau khi diskpart chạy.
    wipe: Option<WipeTarget>,
    /// Serial kỳ vọng đã chuẩn hoá cho ô xác nhận gõ tay (volume:
    /// dạng hex 8 ký tự; đĩa: serial phần cứng đã chuẩn hoá).
    serial_expect_norm: String,
    /// Serial hiển thị cho người dùng nhìn để gõ lại.
    confirm_display: String,
    /// Mô tả đích (cho hộp xác nhận cuối): nhãn phân vùng hoặc tóm
    /// tắt đĩa.
    target_desc: String,
}

// ---------------------------------------------------------------------------
// Luồng nền: thực thi cài đặt (kỷ luật winpe_install)
// ---------------------------------------------------------------------------
fn spawn_install(shared: SharedRef, sender: nwg::NoticeSender, plan: InstallPlan) {
    {
        shared.lock().unwrap().busy = true;
    }
    std::thread::spawn(move || {
        let done = |shared: &SharedRef, sender: &nwg::NoticeSender, r: Result<String, String>| {
            let mut s = shared.lock().unwrap();
            s.install_done = Some(r);
            s.busy = false;
            drop(s);
            sender.notice();
        };
        let log_path = plan
            .log_dir
            .join(format!("winpe-manual-{}.log", qsu_common::log::log_stamp()));
        let sh = shared.clone();
        let snd = sender;
        let sink = Box::new(move |line: &str| {
            push_log(&sh, &snd, line);
        });
        let log = Arc::new(Logger::new(log_path, Some(sink)));
        let fail = |step: &str, why: String| -> Result<String, String> {
            log.line(&format!("LỖI ở bước {}: {}", step, why));
            Err(format!("Bước \"{}\" thất bại: {}", step, why))
        };

        log.line("=== CaiDatWinWinPE bắt đầu cài đặt ===");
        log.line(&format!(
            "Image: {} (index {} — {})",
            plan.image_path.display(),
            plan.index,
            plan.edition_name
        ));
        // Chỉ ghi trạng thái không-mật-khẩu; mật khẩu không vào log.
        if plan.admin_passwordless {
            log.line("Tài khoản admin: không đặt mật khẩu (theo lựa chọn)");
        }
        let mut eff_letter = plan.target_letter;
        let mut wipe_boot: Option<char> = None;
        let mut wipe_recovery: Option<char> = None;
        if let Some(w) = &plan.wipe {
            log.line(&format!(
                "Đích: TOÀN BỘ ĐĨA {} — serial phần cứng {} — XOÁ & CHIA LẠI TỪ ĐẦU",
                w.disk_number, w.disk_serial
            ));
            log.line(&format!("Các phân vùng trên đĩa đích sắp bị XOÁ TOÀN BỘ:\n{}", w.disk_parts));
        } else {
            log.line(&format!(
                "Đích: {}: serial kỳ vọng {} — format: {}",
                plan.target_letter,
                format_serial(plan.target_serial),
                if plan.format { "CÓ" } else { "không" }
            ));
        }
        set_progress(&shared, &sender, 2, "Đang kiểm tra lại đích cài...");

        // --- Kiểm tra lại ở tầng thực thi (không tin riêng giao diện) ---
        if plan.wipe.is_none() {
            if plan.forbidden_letters.contains(&plan.target_letter) {
                done(&shared, &sender, fail("Kiểm tra an toàn", "đích cài trùng ổ chứa công cụ/nguồn cài đặt".to_string()));
                return;
            }
            match volume_serial_of(plan.target_letter) {
                Some(s) if s == plan.target_serial => {}
                Some(s) => {
                    done(
                        &shared,
                        &sender,
                        fail(
                            "Kiểm tra an toàn",
                            format!(
                                "serial ổ {}: đã thay đổi (đã chọn {}, hiện tại {}) — có thể do cắm/rút thiết bị. Hãy quét lại danh sách ổ và chọn lại.",
                                plan.target_letter,
                                format_serial(plan.target_serial),
                                format_serial(s)
                            ),
                        ),
                    );
                    return;
                }
                None => {
                    done(
                        &shared,
                        &sender,
                        fail("Kiểm tra an toàn", format!("không đọc được ổ {}: nữa", plan.target_letter)),
                    );
                    return;
                }
            }
        }
        if !plan.image_path.exists() {
            done(&shared, &sender, fail("Kiểm tra nguồn", format!("không thấy file image {}", plan.image_path.display())));
            return;
        }

        // --- Bước 0 (chỉ wipeDisk): xoá & chia lại toàn bộ ổ đĩa ---
        if let Some(w) = &plan.wipe {
            // Đối chiếu lại tại thời điểm thực thi: tìm đĩa theo serial
            // phần cứng trong liệt kê THẬT (số đĩa có thể đã đổi do
            // cắm/rút thiết bị — serial mới là căn cứ).
            let disks = disk::list_physical_disks();
            let live = match disk::find_disk_by_serial(&disks, &w.disk_serial) {
                Some(d) => d.clone(),
                None => {
                    done(
                        &shared,
                        &sender,
                        fail(
                            "Kiểm tra an toàn (wipe)",
                            format!("không tìm thấy đĩa vật lý có serial '{}' trong liệt kê hiện tại. Dừng, không đụng đĩa nào.", w.disk_serial),
                        ),
                    );
                    return;
                }
            };
            // Đĩa đích KHÔNG được chứa bất kỳ ổ cấm nào (ổ chứa công
            // cụ / ổ chứa nguồn) — kiểm tra theo số đĩa vật lý thật.
            for l in &plan.forbidden_letters {
                match disk::volume_disk_number(*l) {
                    Some(n) if n == live.number => {
                        done(
                            &shared,
                            &sender,
                            fail(
                                "Kiểm tra an toàn (wipe)",
                                format!("đĩa đích (đĩa {}) đang chứa ổ {}: bị cấm (ổ chứa công cụ/nguồn). Từ chối wipe.", live.number, l),
                            ),
                        );
                        return;
                    }
                    _ => {}
                }
            }
            set_progress(&shared, &sender, 4, &format!("Đang xoá & chia lại đĩa {} (diskpart)...", live.number));
            match disk::execute_wipe(&log, live.number, &live.serial, live.size, plan.uefi) {
                Ok(r) => {
                    eff_letter = r.windows_letter;
                    wipe_boot = Some(r.boot_letter);
                    wipe_recovery = r.recovery_letter;
                    log.line(&format!(
                        "Wipe xong: phân vùng Windows mới = {}:, boot = {}:, recovery = {}",
                        r.windows_letter,
                        r.boot_letter,
                        r.recovery_letter.map(|c| format!("{}:", c)).unwrap_or_else(|| "(không ký tự)".to_string())
                    ));
                }
                Err(e) => {
                    done(&shared, &sender, fail("Xoá & chia lại ổ đĩa (diskpart)", e));
                    return;
                }
            }
        }

        let target_root = format!("{}:\\", eff_letter);
        let mut warnings: Vec<String> = Vec::new();

        // --- Bước 1: format (nếu chọn; wipeDisk đã format trong diskpart) ---
        if plan.wipe.is_some() {
            log.line("Chế độ wipeDisk: phân vùng Windows đã được format trong script diskpart — bỏ qua bước format riêng.");
        } else if plan.format {
            set_progress(&shared, &sender, 5, &format!("Đang format phân vùng {}: (NTFS)...", plan.target_letter));
            log.line(&format!("Đang format phân vùng {}: (NTFS, quick)...", plan.target_letter));
            if !format_partition(&log, plan.target_letter) {
                done(&shared, &sender, fail("Format phân vùng đích", "format.com và diskpart đều thất bại".to_string()));
                return;
            }
            log.line("Format xong.");
        }

        // --- Bước 2: DISM /Apply-Image ---
        let dism = dism_exe();
        let image_arg = format!("/ImageFile:{}", plan.image_path.to_string_lossy());
        let index_arg = format!("/Index:{}", plan.index);
        let apply_arg = format!("/ApplyDir:{}", target_root);
        let mut args: Vec<String> = vec!["/Apply-Image".to_string(), image_arg, index_arg, apply_arg];
        if let Some(swm) = &plan.swm_pattern {
            args.push(format!("/SWMFile:{}", swm));
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        log.line(&format!("$ {} {}", dism, arg_refs.join(" ")));
        set_progress(&shared, &sender, 10, "Đang apply image (DISM)...");
        let sh2 = shared.clone();
        let snd2 = sender;
        let apply_res = cmd::run_streaming(&dism, &arg_refs, |line| {
            let t = line.trim();
            if t.is_empty() {
                return;
            }
            log.line(&format!("  | {}", t));
            if let Some(pos) = t.rfind('%') {
                let num: String = t[..pos].chars().rev().take_while(|c| c.is_ascii_digit() || *c == '.').collect::<String>().chars().rev().collect();
                if let Ok(pct) = num.parse::<f32>() {
                    let mapped = 10 + (pct * 0.7) as u32;
                    set_progress(&sh2, &snd2, mapped.min(80), &format!("Đang apply image: {:.0}%", pct));
                }
            }
        });
        match apply_res {
            Ok(Some(0)) => log.line("Apply-Image hoàn tất."),
            other => {
                done(
                    &shared,
                    &sender,
                    fail("DISM /Apply-Image", format!("mã thoát {:?} — xem chi tiết trong log", other.ok().flatten())),
                );
                return;
            }
        }

        // --- Bước 3: inject driver offline (lỗi = cảnh báo, như engine) ---
        if let Some(drv) = &plan.driver_dir {
            set_progress(&shared, &sender, 85, "Đang inject driver offline...");
            let img_arg = format!("/Image:{}", target_root);
            let drv_arg = format!("/Driver:{}", drv.to_string_lossy());
            log.line(&format!("Đang inject driver từ {}...", drv.display()));
            match cmd::run_logged(&log, &dism, &[&img_arg, "/Add-Driver", &drv_arg, "/Recurse"]) {
                Ok(out) if out.code == Some(0) => log.line("Inject driver xong."),
                Ok(out) => {
                    let w = format!("inject driver trả mã {:?} — Windows vẫn dùng được, kiểm tra lại driver sau khi vào Windows", out.code);
                    log.line(&format!("CẢNH BÁO: {}", w));
                    warnings.push(w);
                }
                Err(e) => {
                    let w = format!("không chạy được DISM inject driver: {}", e);
                    log.line(&format!("CẢNH BÁO: {}", w));
                    warnings.push(w);
                }
            }
        }

        // --- Bước 4: autounattend vào Windows\Panther ---
        if let Some(xml) = &plan.unattend_xml {
            if plan.core_profile {
                // Hồ sơ CORE (v0.7.21): bỏ qua agent hậu cài; ghi
                // SetupComplete.cmd tối thiểu — một lệnh net user
                // dọn cờ "phải đổi mật khẩu ở lần đăng nhập đầu",
                // chạy bằng quyền SYSTEM ở cuối pha setup, trước
                // lần đăng nhập đầu tiên.
                let scripts = Path::new(&target_root).join("Windows").join("Setup").join("Scripts");
                let sc_line = format!("net user {} /logonpasswordchg:no", plan.admin_user);
                let sc_res = std::fs::create_dir_all(&scripts).and_then(|_| {
                    let sc = scripts.join("SetupComplete.cmd");
                    if sc.exists() {
                        // Ảnh đã có sẵn SetupComplete.cmd (từ bộ
                        // tuỳ biến): nối thêm dòng của mình, KHÔNG
                        // ghi đè nội dung sẵn có của ảnh.
                        let mut cur = std::fs::read_to_string(&sc).unwrap_or_default();
                        if !cur.ends_with('\n') {
                            cur.push_str("\r\n");
                        }
                        cur.push_str(&sc_line);
                        cur.push_str("\r\n");
                        std::fs::write(&sc, cur)
                    } else {
                        std::fs::write(&sc, format!("@echo off\r\n{}\r\n", sc_line))
                    }
                });
                if let Err(e) = sc_res {
                    done(&shared, &sender, fail("Ghi SetupComplete.cmd", e.to_string()));
                    return;
                }
                log.line("Hồ sơ CORE: đã ghi SetupComplete.cmd tối thiểu (dọn cờ đổi mật khẩu), bỏ qua agent hậu cài.");
            } else {
                if let Err(e) = qsu_common::post_install::write_to_windows(Path::new(&target_root)) {
                    done(&shared, &sender, fail("Chuẩn bị tự cài ứng dụng", e.to_string()));
                    return;
                }
                log.line("Đã chuẩn bị tự cài UltraViewer, Chrome và 7-Zip ở lần đăng nhập đầu.");
                let notify_dir = Path::new(&target_root).join("ProgramData").join("QSU");
                if let Err(e) = qsu_common::post_install::copy_host_telegram_config(&notify_dir) {
                    done(&shared, &sender, fail("Cấu hình Telegram", e.to_string()));
                    return;
                }
            }
            set_progress(&shared, &sender, 90, "Đang ghi autounattend.xml...");
            let panther = Path::new(&target_root).join("Windows").join("Panther");
            let write_res = std::fs::create_dir_all(&panther).and_then(|_| {
                std::fs::write(panther.join("autounattend.xml"), xml)?;
                std::fs::write(panther.join("unattend.xml"), xml)
            });
            match write_res {
                Ok(_) => {
                    log.line("Đã ghi autounattend.xml + unattend.xml vào Windows\\Panther.");
                    if plan.core_profile {
                        log.line("[Cơ chế] Hồ sơ CORE (v0.7.21): autounattend.xml KHÔNG mang FirstLogonCommands; cờ 'phải đổi mật khẩu ở lần đăng nhập đầu' của tài khoản admin được dọn bằng SetupComplete.cmd (net user <tên> /logonpasswordchg:no) chạy bằng quyền SYSTEM ở cuối pha setup, trước lần đăng nhập đầu — AutoLogon vẫn vào thẳng desktop. Hồ sơ này dành cho ảnh rút gọn kiểu Tiny11 Core.");
                    } else {
                    // [Cơ chế] (v0.7.19): dọn cờ "phải đổi mật khẩu ở lần đăng nhập đầu" của tài khoản admin.
                    log.line("[Cơ chế] autounattend.xml mang hai lệnh FirstLogonCommands cho tài khoản admin — `net user <tên> /logonpasswordchg:no` và đặt PasswordNeverExpires: tài khoản Setup tạo từ unattend mang cờ phải đổi mật khẩu ở lần đăng nhập đầu (pwdLastSet=0), không dọn cờ thì AutoLogon bị chặn ở màn hình đổi mật khẩu thay vì vào thẳng desktop (thực địa 2026-11-18). Áp dụng cả khi mật khẩu để trống lẫn khi có đặt.");
                    }
                }
                Err(e) => {
                    done(&shared, &sender, fail("Ghi autounattend.xml", e.to_string()));
                    return;
                }
            }
        }

        // --- Bước 5: thư mục custom (copy nguyên trạng, lỗi = cảnh báo) ---
        if let Some(custom) = &plan.custom_dir {
            let dst = Path::new(&target_root).join("ProgramData").join("QSU").join("custom");
            match copy_dir_recursive(custom, &dst) {
                Ok(n) => log.line(&format!("Đã copy thư mục custom ({} file) vào ProgramData\\QSU\\custom.", n)),
                Err(e) => {
                    let w = format!("copy thư mục custom thất bại: {}", e);
                    log.line(&format!("CẢNH BÁO: {}", w));
                    warnings.push(w);
                }
            }
        }

        // --- Bước 5b: WinRE vào phân vùng Recovery mới (chỉ wipeDisk) ---
        if plan.wipe.is_some() {
            match wipe_recovery {
                Some(r) => {
                    disk::copy_winre(&log, eff_letter, r);
                }
                None => log.line(
                    "CẢNH BÁO: chế độ wipeDisk nhưng phân vùng Recovery không có ký tự ổ — bỏ qua bước WinRE.",
                ),
            }
        }

        // --- Bước 6: chốt boot (bcdboot) ---
        set_progress(&shared, &sender, 95, "Đang tạo boot (bcdboot)...");
        let boot_res: Result<char, String> = if let Some(bl) = wipe_boot {
            // wipeDisk: trỏ vào phân vùng boot MỚI vừa chia (EFI mới /
            // System Reserved mới), không dùng phân vùng EFI cũ.
            // (Giữ nguyên đường đã chạy đúng ở thực địa.)
            let bcdboot_path = Path::new(&target_root).join("Windows").join("System32").join("bcdboot.exe");
            let bcdboot_s = if bcdboot_path.exists() {
                bcdboot_path.to_string_lossy().into_owned()
            } else {
                "bcdboot".to_string()
            };
            let win_arg = format!("{}\\Windows", target_root);
            let s_arg = format!("{}:", bl);
            let fw = if plan.uefi { "UEFI" } else { "BIOS" };
            match cmd::run_logged(&log, &bcdboot_s, &[&win_arg, "/s", &s_arg, "/f", fw]) {
                Ok(o) if o.code == Some(0) => Ok(bl),
                Ok(o) => Err(format!(
                    "bcdboot vào phân vùng boot mới {}: trả mã {:?} — output của bcdboot đã ghi trong log phía trên; Windows mới chưa khởi động được.",
                    bl, o.code
                )),
                Err(e) => Err(format!("không chạy được bcdboot: {}", e)),
            }
        } else {
            // Cài theo phân vùng (v0.7.19): helper DÙNG CHUNG
            // disk::finalize_boot — tìm phân vùng boot TRÊN CÙNG
            // ĐĨA với phân vùng đích theo loại phân vùng, tự gán
            // ký tự nếu thiếu, bcdboot + kiểm chứng file boot
            // thật; Err mang theo output của bcdboot. Thay hẳn
            // find_efi_letter() cũ (ca thực địa 2026-11-18: bung
            // ảnh 100% nhưng bước này thất bại vì phân vùng EFI
            // của SSD không có ký tự ổ).
            disk::finalize_boot(&log, eff_letter, plan.uefi).map(|f| f.boot_letter)
        };
        if let Err(e) = boot_res {
            done(&shared, &sender, fail("bcdboot", e));
            return;
        }

        set_progress(&shared, &sender, 100, "Cài đặt hoàn tất.");
        log.line("=== CaiDatWinWinPE hoàn tất — sẵn sàng khởi động lại vào Windows mới ===");
        let mut msg = format!(
            "Đã cài xong Windows ({}) vào ổ {}:.\nMáy đã sẵn sàng khởi động vào Windows mới.",
            plan.edition_name, eff_letter
        );
        if !warnings.is_empty() {
            msg.push_str(&format!("\n\nCảnh báo trong quá trình cài:\n- {}", warnings.join("\n- ")));
        }
        msg.push_str(&format!("\n\nLog đầy đủ: {}", log.path().display()));
        done(&shared, &sender, Ok(msg));
    });
}

// ---------------------------------------------------------------------------
// Ứng dụng
// ---------------------------------------------------------------------------
struct Model {
    in_winpe: bool,
    uefi: bool,
    exe_dir: PathBuf,
    exe_letter: Option<char>,
    settings: Settings,
    volumes: Vec<VolumeInfo>,
    targets: Vec<TargetEntry>,
    source_file: Option<PathBuf>,
    mounted_iso: Option<PathBuf>,
    mounted_letter: Option<char>,
    image_path: Option<PathBuf>,
    swm_pattern: Option<String>,
    editions: Vec<EditionInfo>,
    pending_plan: Option<InstallPlan>,
    running: bool,
    succeeded: bool,
    log_rendered: usize,
    // --- Trình duyệt file tích hợp (v0.7.19) ---
    browser_mode: BrowserMode,
    browser_dir: PathBuf,
    /// Các mục đang hiện, song song với lst_bentries:
    /// (đường dẫn đầy đủ, là thư mục?)
    browser_entries: Vec<(PathBuf, bool)>,
    browser_drives: Vec<char>,
}

struct App {
    notice: nwg::Notice,
    // --- Cụm cài đặt (chỉ hiện trong WinPE) ---
    inp_source: nwg::TextInput,
    btn_browse_source: nwg::Button,
    btn_load_source: nwg::Button,
    lbl_source_info: nwg::Label,
    lst_editions: nwg::ListBox<String>,
    lbl_edition_info: nwg::Label,
    lst_targets: nwg::ListView,
    chk_override: nwg::CheckBox,
    btn_refresh_vols: nwg::Button,
    lbl_target_info: nwg::Label,
    chk_format: nwg::CheckBox,
    inp_driver: nwg::TextInput,
    btn_browse_driver: nwg::Button,
    chk_unattend: nwg::CheckBox,
    chk_core: nwg::CheckBox,
    inp_computer: nwg::TextInput,
    inp_user: nwg::TextInput,
    inp_pw1: nwg::TextInput,
    inp_pw2: nwg::TextInput,
    lbl_serial: nwg::Label,
    inp_serial: nwg::TextInput,
    btn_confirm_format: nwg::Button,
    bar: nwg::ProgressBar,
    lbl_progress: nwg::Label,
    btn_start: nwg::Button,
    btn_reboot: nwg::Button,
    // --- Cụm chuẩn bị (chỉ hiện trong Windows thường) ---
    cmb_prep_drive: nwg::ComboBox<String>,
    btn_prep_refresh: nwg::Button,
    btn_prepare: nwg::Button,
    lbl_prep_result: nwg::Label,
    // --- Chung ---
    txt_log: nwg::TextBox,
    lbl_status: nwg::Label,
    btn_close: nwg::Button,
    // --- Trình duyệt file tích hợp (v0.7.19) ---
    win_browser: nwg::Window,
    lbl_btitle: nwg::Label,
    lst_bdrives: nwg::ListBox<String>,
    lbl_bpath: nwg::Label,
    lst_bentries: nwg::ListBox<String>,
    btn_bup: nwg::Button,
    btn_bchoose: nwg::Button,
    btn_bcancel: nwg::Button,
    model: RefCell<Model>,
    shared: SharedRef,
}

fn set_ctrl_visible(handle: &nwg::ControlHandle, visible: bool) {
    if let Some(hwnd) = handle.hwnd() {
        unsafe {
            winapi::um::winuser::ShowWindow(
                hwnd,
                if visible { winapi::um::winuser::SW_SHOW } else { winapi::um::winuser::SW_HIDE },
            );
        }
    }
}

fn path_letter(p: &Path) -> Option<char> {
    p.to_string_lossy().chars().next().map(|c| c.to_ascii_uppercase())
}

/// Lý do một phân vùng bị loại trừ khỏi đích cài (None = được chọn).
fn exclusion_reason(m: &Model, v: &VolumeInfo) -> Option<String> {
    if Some(v.letter) == m.exe_letter {
        return Some("ổ chứa chính công cụ này".to_string());
    }
    if let Some(src) = &m.source_file {
        if path_letter(src) == Some(v.letter) {
            return Some("ổ chứa nguồn cài đặt".to_string());
        }
    }
    if m.mounted_letter == Some(v.letter) {
        return Some("ổ ảo của ISO đã mount".to_string());
    }
    None
}

fn build_ui(exe_dir: PathBuf, in_winpe: bool) -> (nwg::Window, Rc<App>) {
    // --- Fonts chuẩn dùng chung (qsu_common::ui) ---
    let f_section = qsu_common::ui::font_section();
    let f_normal = qsu_common::ui::font_normal();
    let f_small = qsu_common::ui::font_small();
    let f_button = qsu_common::ui::font_button();
    let f_mono = qsu_common::ui::font_mono();

    let mut window = Default::default();
    nwg::Window::builder()
        .size((1060, 940))
        .position((110, 40))
        .title("Cài Đặt Win = WinPE — Utility Rebuild")
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE | nwg::WindowFlags::MINIMIZE_BOX)
        .build(&mut window)
        .expect("window");
    qsu_common::ui::set_window_icon(&window);
    let (client_w, client_h) = {
        let mut rc: RECT = unsafe { std::mem::zeroed() };
        unsafe { GetClientRect(window.handle.hwnd().unwrap(), &mut rc) };
        (rc.right, rc.bottom)
    };
    let _ = (client_w, client_h);

    macro_rules! add_label {
        ($text:expr, $font:expr, $pos:expr, $size:expr) => {{
            let mut lbl = Default::default();
            nwg::Label::builder().text($text).font(Some($font)).parent(&window).build(&mut lbl).expect("label");
            lbl.set_position($pos.0, $pos.1);
            lbl.set_size($size.0, $size.1);
            lbl
        }};
    }
    macro_rules! add_button {
        ($text:expr, $pos:expr, $size:expr) => {{
            let mut b = Default::default();
            nwg::Button::builder().text($text).font(Some(f_button)).parent(&window).build(&mut b).expect("button");
            b.set_position($pos.0, $pos.1);
            b.set_size($size.0, $size.1);
            b
        }};
    }
    macro_rules! add_input {
        ($pos:expr, $size:expr) => {{
            let mut t = Default::default();
            nwg::TextInput::builder().font(Some(f_normal)).parent(&window).build(&mut t).expect("input");
            t.set_position($pos.0, $pos.1);
            t.set_size($size.0, $size.1);
            t
        }};
    }

    // --- Header ---
    let subtitle = if in_winpe {
        "Trình cài Windows tương tác trong WinPE: chọn nguồn, chọn phân vùng đích, cài ngay tại máy"
    } else {
        "Module này chỉ cài đặt khi chạy trong WinPE — ở Windows thường, dùng mục Chuẩn bị bên dưới"
    };
    let (_t1, _t2) = qsu_common::ui::add_header(&window, "Cài Đặt Win = WinPE", subtitle);
    let uefi = sys::firmware_is_uefi();
    let mode_text = if in_winpe {
        format!(
            "Môi trường: WinPE — chế độ CÀI ĐẶT (firmware: {})",
            if uefi { "UEFI" } else { "Legacy BIOS" }
        )
    } else {
        "Môi trường: Windows thường — chế độ CHUẨN BỊ".to_string()
    };
    let _mode = add_label!(&mode_text, f_normal, (16, 64), (1000, 22));

    // ================= Cụm CÀI ĐẶT (WinPE) =================
    let lbl_s1 = add_label!("1 — NGUỒN CÀI ĐẶT", f_section, (16, 90), (460, 24));
    let inp_source = add_input!((16, 118), (660, 26));
    let btn_browse_source = add_button!("Chọn file...", (688, 114), (150, 34));
    let btn_load_source = add_button!("Nạp nguồn", (846, 114), (162, 34));
    let lbl_source_info = add_label!(
        "Chọn file .iso hoặc file sources\\install.wim / .esd / .swm trên bất kỳ ổ nào, rồi bấm Nạp nguồn.",
        f_small, (16, 152), (1008, 20)
    );
    let mut lst_editions = Default::default();
    nwg::ListBox::builder().parent(&window).build(&mut lst_editions).expect("listbox");
    lst_editions.set_position(16, 178);
    lst_editions.set_size(660, 78);
    let lbl_edition_info = add_label!(
        "Danh sách phiên bản trong image sẽ hiện ở đây sau khi nạp nguồn.",
        f_small, (688, 178), (320, 78)
    );

    let lbl_s2 = add_label!("2 — ĐÍCH CÀI: PHÂN VÙNG, HOẶC TOÀN BỘ Ổ ĐĨA (XOÁ & CHIA LẠI)", f_section, (16, 264), (700, 24));
    let mut lst_targets = Default::default();
    nwg::ListView::builder()
        .parent(&window)
        .list_style(nwg::ListViewStyle::Detailed)
        .flags(nwg::ListViewFlags::VISIBLE | nwg::ListViewFlags::TAB_STOP | nwg::ListViewFlags::ALWAYS_SHOW_SELECTION)
        .ex_flags(nwg::ListViewExFlags::GRID | nwg::ListViewExFlags::FULL_ROW_SELECT)
        .build(&mut lst_targets)
        .expect("listview");
    lst_targets.set_headers_enabled(true);
    lst_targets.set_position(16, 292);
    lst_targets.set_size(992, 148);
    for (i, (name, w)) in [
        ("Ổ", 44),
        ("Nhãn", 110),
        ("FS", 52),
        ("Tổng", 80),
        ("Trống", 80),
        ("Serial", 90),
        ("Loại", 84),
        ("Nội dung / Ghi chú", 424),
    ]
    .iter()
    .enumerate()
    {
        lst_targets.insert_column(nwg::InsertListViewColumn {
            index: Some(i as i32),
            text: Some(name.to_string()),
            width: Some(*w),
            ..Default::default()
        });
    }
    let mut chk_override = Default::default();
    nwg::CheckBox::builder()
        .text("Cho phép chọn cả ổ BỊ LOẠI TRỪ (ổ chứa công cụ / ổ chứa nguồn) — NGUY HIỂM, chỉ bật khi thật sự cần")
        .font(Some(f_normal))
        .parent(&window)
        .build(&mut chk_override)
        .expect("checkbox");
    chk_override.set_position(16, 448);
    chk_override.set_size(810, 24);
    let btn_refresh_vols = add_button!("Quét lại ổ đĩa", (846, 444), (162, 32));
    let lbl_target_info = add_label!(
        "Chưa chọn đích cài. Chọn một phân vùng, hoặc một dòng TOÀN BỘ Ổ ĐĨA để xoá sạch & chia lại từ đầu. Ổ RAM của WinPE (X:) không được liệt kê làm đích.",
        f_small, (16, 480), (1008, 36)
    );

    let lbl_s3 = add_label!("3 — TUỲ CHỌN", f_section, (16, 524), (460, 24));
    let mut chk_format = Default::default();
    nwg::CheckBox::builder()
        .text("Format phân vùng đích trước khi cài (NTFS, nhanh) — xoá sạch dữ liệu trên ổ đích")
        .font(Some(f_normal))
        .parent(&window)
        .build(&mut chk_format)
        .expect("checkbox");
    chk_format.set_position(16, 552);
    chk_format.set_size(600, 24);
    let custom_dir = exe_dir.join("custom");
    let custom_text = if custom_dir.is_dir() {
        "Thư mục custom: CÓ — sẽ copy vào ProgramData\\QSU\\custom của Windows mới"
    } else {
        "Thư mục custom: không có (bỏ qua)"
    };
    let lbl_custom = add_label!(custom_text, f_small, (628, 554), (396, 22));
    let lbl_driver_cap = add_label!("Thư mục driver (tuỳ chọn):", f_normal, (16, 584), (170, 22));
    let inp_driver = add_input!((190, 582), (540, 26));
    let btn_browse_driver = add_button!("Duyệt...", (742, 580), (150, 32));
    let mut chk_unattend = Default::default();
    nwg::CheckBox::builder()
        .text("Tự thiết lập Windows: tài khoản admin, AutoLogon; tự cài UltraViewer, Chrome, 7-Zip (cần mạng)")
        .font(Some(f_normal))
        .parent(&window)
        .check_state(nwg::CheckBoxState::Checked)
        .build(&mut chk_unattend)
        .expect("checkbox");
    chk_unattend.set_position(16, 618);
    chk_unattend.set_size(810, 24);
    // Ô chọn hồ sơ CORE cho ảnh rút gọn (v0.7.21) — đặt ở dải
    // trống giữa hàng xác nhận serial và thanh tiến trình, không
    // xô lệch bố cục hiện có. Chỉ có nghĩa khi ô autounattend ở
    // trên được tick.
    let mut chk_core = Default::default();
    nwg::CheckBox::builder()
        .text("Ảnh rút gọn (Tiny11 Core): profile tối thiểu — không lệnh chạy thêm ở lần đăng nhập đầu, dọn cờ mật khẩu bằng SetupComplete.cmd")
        .font(Some(f_normal))
        .parent(&window)
        .build(&mut chk_core)
        .expect("checkbox");
    chk_core.set_position(16, 749);
    chk_core.set_size(810, 22);
    let lbl_comp_cap = add_label!("Tên máy tính:", f_normal, (16, 650), (110, 22));
    let inp_computer = add_input!((130, 648), (200, 26));
    inp_computer.set_text("QSU-PC");
    let lbl_user_cap = add_label!("Tài khoản admin:", f_normal, (346, 650), (130, 22));
    let inp_user = add_input!((480, 648), (180, 26));
    inp_user.set_text("Admin");
    let lbl_pw_cap = add_label!("Mật khẩu (để trống = không đặt mật khẩu):", f_normal, (16, 684), (330, 22));
    let inp_pw1 = add_input!((352, 682), (170, 26));
    set_edit_style(inp_pw1.handle.hwnd(), ES_PASSWORD);
    let lbl_pw2_cap = add_label!("Nhập lại mật khẩu:", f_normal, (538, 684), (140, 22));
    let inp_pw2 = add_input!((684, 682), (150, 26));
    set_edit_style(inp_pw2.handle.hwnd(), ES_PASSWORD);
    let lbl_pw_note = add_label!("(Không ghi vào log.)", f_small, (850, 684), (158, 22));

    // Hàng xác nhận serial cho format (ẩn cho tới khi cần)
    let lbl_serial = add_label!("", f_normal, (16, 718), (430, 22));
    let inp_serial = add_input!((450, 716), (170, 26));
    let btn_confirm_format = add_button!("Xác nhận serial & cài", (632, 714), (240, 32));

    let mut bar = Default::default();
    nwg::ProgressBar::builder().parent(&window).build(&mut bar).expect("bar");
    bar.set_position(16, 776);
    bar.set_size(660, 16);
    bar.set_range(0..101);
    bar.set_pos(0);
    let lbl_progress = add_label!("", f_small, (688, 774), (320, 22));
    let btn_start = add_button!("BẮT ĐẦU CÀI ĐẶT", (688, 798), (320, 44));
    qsu_common::ui::make_primary(&btn_start);
    let btn_reboot = add_button!("Khởi động lại", (688, 850), (156, 38));
    let btn_close = add_button!("Đóng", (852, 850), (156, 38));

    // ================= Cụm CHUẨN BỊ (Windows thường) =================
    let lbl_prep_explain = add_label!(
        "Module Cài Đặt Win=WinPE dùng để cài Windows NGAY TRONG WinPE: boot máy bằng USB WinPE, chạy file CaiDatWinWinPE.exe từ ổ dữ liệu/USB, chọn ISO hoặc install.wim, chọn phân vùng đích rồi cài. Trong Windows thường module không cài đặt — chỉ chép sẵn công cụ ra ổ bạn chọn.",
        f_normal, (16, 90), (1008, 66)
    );
    let lbl_prep_steps = add_label!(
        "Cách dùng trong WinPE:\n\
         1) Boot USB WinPE (tạo bằng module Tạo Boot WinPE hoặc bộ WinPE sẵn có).\n\
         2) Mở CaiDatWinWinPE.exe từ ổ chứa công cụ này.\n\
         3) Mục 1: bấm Chọn file... (trình duyệt file tích hợp của module) để chọn file .iso — module tự mount bằng VirtDisk, không cần PowerShell — hoặc chọn trực tiếp sources\\install.wim/.esd/.swm; cũng gõ thẳng đường dẫn vào ô được. Bấm Nạp nguồn, chọn phiên bản.\n\
         4) Mục 2: chọn phân vùng đích — ổ chứa công cụ và ổ chứa nguồn bị loại trừ mặc định; bật Format chỉ khi muốn xoá sạch ổ đích (phải nhập serial xác nhận).\n\
         5) Mục 3: thư mục driver (inject offline), autounattend (tên máy + tài khoản admin). Bấm BẮT ĐẦU CÀI ĐẶT, xong bấm Khởi động lại.",
        f_normal, (16, 164), (1008, 128)
    );
    let lbl_prep_drive_cap = add_label!("Ổ đích để chép công cụ (USB / ổ dữ liệu):", f_normal, (16, 306), (430, 22));
    let mut cmb_prep_drive = Default::default();
    nwg::ComboBox::builder().parent(&window).build(&mut cmb_prep_drive).expect("combo");
    cmb_prep_drive.set_position(16, 332);
    cmb_prep_drive.set_size(430, 26);
    let btn_prep_refresh = add_button!("Quét lại ổ đĩa", (458, 328), (150, 32));
    let btn_prepare = add_button!("Chuẩn bị: chép công cụ ra ổ đã chọn", (16, 372), (430, 42));
    let lbl_prep_result = add_label!("", f_normal, (16, 426), (1008, 66));
    let lbl_prep_note = add_label!(
        "Công cụ sẽ chép CaiDatWinWinPE.exe ra thư mục gốc của ổ đã chọn, kèm WinPEInstall.exe nếu thấy cạnh công cụ này (engine cho luồng wizard tự động — module này không bắt buộc phải có nó).",
        f_small, (16, 500), (1008, 40)
    );

    // ================= Cụm chung =================
    let lbl_status = add_label!("Đang khởi tạo...", f_small, (16, 752), (1008, 20));
    let mut txt_log = Default::default();
    nwg::TextBox::builder().font(Some(f_mono)).parent(&window).build(&mut txt_log).expect("log");
    txt_log.set_position(16, 798);
    txt_log.set_size(660, 92);
    set_edit_style(txt_log.handle.hwnd(), ES_READONLY);

    let mut notice = Default::default();
    nwg::Notice::builder().parent(&window).build(&mut notice).expect("notice");

    // --- Cửa sổ trình duyệt file TÍCH HỢP (v0.7.19) ---
    // Cửa sổ Win32 thường do module tự sở hữu — không phụ thuộc
    // hộp thoại file của Windows (không mở được trong WinPE
    // BHT/Anhdv). Ban đầu ẨN; mở khi bấm "Chọn file..." hoặc
    // "Duyệt..." (xem browser_open).
    let mut win_browser = Default::default();
    nwg::Window::builder()
        .size((720, 520))
        .position((200, 110))
        .title("Chọn file — trình duyệt tích hợp")
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::MINIMIZE_BOX)
        .build(&mut win_browser)
        .expect("browser window");
    qsu_common::ui::set_window_icon(&win_browser);
    let mut lbl_btitle = Default::default();
    nwg::Label::builder()
        .text("Chọn file nguồn cài đặt (.iso / .wim / .esd / .swm)")
        .font(Some(f_section))
        .parent(&win_browser)
        .build(&mut lbl_btitle)
        .expect("browser title");
    lbl_btitle.set_position(16, 10);
    lbl_btitle.set_size(688, 24);
    let mut lst_bdrives = Default::default();
    nwg::ListBox::builder().parent(&win_browser).build(&mut lst_bdrives).expect("browser drives");
    lst_bdrives.set_position(16, 40);
    lst_bdrives.set_size(688, 56);
    let mut lbl_bpath = Default::default();
    nwg::Label::builder().text("").font(Some(f_mono)).parent(&win_browser).build(&mut lbl_bpath).expect("browser path");
    lbl_bpath.set_position(16, 104);
    lbl_bpath.set_size(688, 20);
    let mut lst_bentries = Default::default();
    nwg::ListBox::builder().parent(&win_browser).build(&mut lst_bentries).expect("browser entries");
    lst_bentries.set_position(16, 128);
    lst_bentries.set_size(688, 284);
    let mut btn_bup = Default::default();
    nwg::Button::builder().text("Lên một cấp").font(Some(f_button)).parent(&win_browser).build(&mut btn_bup).expect("browser up");
    btn_bup.set_position(16, 424);
    btn_bup.set_size(150, 38);
    let mut btn_bchoose = Default::default();
    nwg::Button::builder().text("Chọn").font(Some(f_button)).parent(&win_browser).build(&mut btn_bchoose).expect("browser choose");
    btn_bchoose.set_position(372, 424);
    btn_bchoose.set_size(160, 38);
    let mut btn_bcancel = Default::default();
    nwg::Button::builder().text("Huỷ").font(Some(f_button)).parent(&win_browser).build(&mut btn_bcancel).expect("browser cancel");
    btn_bcancel.set_position(548, 424);
    btn_bcancel.set_size(156, 38);

    let (settings, settings_err) = Settings::load(&exe_dir);
    let shared: SharedRef = Arc::new(Mutex::new(Shared::default()));
    if let Some(e) = &settings_err {
        shared.lock().unwrap().log_lines.push_back(format!("CẢNH BÁO: {}", e));
    }
    let exe_letter = std::env::current_exe()
        .ok()
        .and_then(|p| path_letter(&p));

    let app = Rc::new(App {
        notice,
        inp_source,
        btn_browse_source,
        btn_load_source,
        lbl_source_info,
        lst_editions,
        lbl_edition_info,
        lst_targets,
        chk_override,
        btn_refresh_vols,
        lbl_target_info,
        chk_format,
        inp_driver,
        btn_browse_driver,
        chk_unattend,
        chk_core,
        inp_computer,
        inp_user,
        inp_pw1,
        inp_pw2,
        lbl_serial,
        inp_serial,
        btn_confirm_format,
        bar,
        lbl_progress,
        btn_start,
        btn_reboot,
        cmb_prep_drive,
        btn_prep_refresh,
        btn_prepare,
        lbl_prep_result,
        txt_log,
        lbl_status,
        btn_close,
        win_browser,
        lbl_btitle,
        lst_bdrives,
        lbl_bpath,
        lst_bentries,
        btn_bup,
        btn_bchoose,
        btn_bcancel,
        model: RefCell::new(Model {
            in_winpe,
            uefi,
            exe_dir,
            exe_letter,
            settings,
            volumes: Vec::new(),
            targets: Vec::new(),
            source_file: None,
            mounted_iso: None,
            mounted_letter: None,
            image_path: None,
            swm_pattern: None,
            editions: Vec::new(),
            pending_plan: None,
            running: false,
            succeeded: false,
            log_rendered: 0,
            browser_mode: BrowserMode::SourceFile,
            browser_dir: PathBuf::new(),
            browser_entries: Vec::new(),
            browser_drives: Vec::new(),
        }),
        shared,
    });

    // --- Bố cục theo chế độ: ẩn cụm không thuộc chế độ hiện tại ---
    let install_handles: Vec<nwg::ControlHandle> = vec![
        lbl_s1.handle.clone(),
        app.inp_source.handle.clone(),
        app.btn_browse_source.handle.clone(),
        app.btn_load_source.handle.clone(),
        app.lbl_source_info.handle.clone(),
        app.lst_editions.handle.clone(),
        app.lbl_edition_info.handle.clone(),
        lbl_s2.handle.clone(),
        app.lst_targets.handle.clone(),
        app.chk_override.handle.clone(),
        app.btn_refresh_vols.handle.clone(),
        app.lbl_target_info.handle.clone(),
        lbl_s3.handle.clone(),
        app.chk_format.handle.clone(),
        lbl_custom.handle.clone(),
        lbl_driver_cap.handle.clone(),
        app.inp_driver.handle.clone(),
        app.btn_browse_driver.handle.clone(),
        app.chk_unattend.handle.clone(),
        app.chk_core.handle.clone(),
        lbl_comp_cap.handle.clone(),
        app.inp_computer.handle.clone(),
        lbl_user_cap.handle.clone(),
        app.inp_user.handle.clone(),
        lbl_pw_cap.handle.clone(),
        app.inp_pw1.handle.clone(),
        lbl_pw2_cap.handle.clone(),
        app.inp_pw2.handle.clone(),
        lbl_pw_note.handle.clone(),
        app.bar.handle.clone(),
        app.lbl_progress.handle.clone(),
        app.btn_start.handle.clone(),
        app.btn_reboot.handle.clone(),
    ];
    let prep_handles: Vec<nwg::ControlHandle> = vec![
        lbl_prep_explain.handle.clone(),
        lbl_prep_steps.handle.clone(),
        lbl_prep_drive_cap.handle.clone(),
        app.cmb_prep_drive.handle.clone(),
        app.btn_prep_refresh.handle.clone(),
        app.btn_prepare.handle.clone(),
        app.lbl_prep_result.handle.clone(),
        lbl_prep_note.handle.clone(),
    ];
    for h in &install_handles {
        set_ctrl_visible(h, in_winpe);
    }
    for h in &prep_handles {
        set_ctrl_visible(h, !in_winpe);
    }
    // Hàng xác nhận serial chỉ hiện khi đang chờ xác nhận format
    set_ctrl_visible(&app.lbl_serial.handle, false);
    set_ctrl_visible(&app.inp_serial.handle, false);
    set_ctrl_visible(&app.btn_confirm_format.handle, false);
    app.btn_reboot.set_enabled(false);
    if !in_winpe {
        // Chế độ chuẩn bị: đưa vùng log lên gần cụm chuẩn bị cho dễ theo dõi
        app.txt_log.set_position(16, 580);
        app.txt_log.set_size(1008, 160);
    }

    // Trình duyệt file tích hợp: ẩn cho tới khi được mở.
    set_ctrl_visible(&app.win_browser.handle, false);

    let handler = app.clone();
    nwg::full_bind_event_handler(&window.handle, move |evt, data, handle| {
        handle_event(&handler, evt, &data, handle);
    });
    let handler_b = app.clone();
    nwg::full_bind_event_handler(&app.win_browser.handle, move |evt, data, handle| {
        handle_browser_event(&handler_b, evt, &data, handle);
    });

    (window, app)
}

// ---------------------------------------------------------------------------
// Đổ dữ liệu lên giao diện
// ---------------------------------------------------------------------------
fn refill_targets(app: &Rc<App>) {
    app.lst_targets.clear();
    let m = app.model.borrow();
    for (i, t) in m.targets.iter().enumerate() {
        match t {
            TargetEntry::Volume(v) => {
                let note = match exclusion_reason(&m, v) {
                    Some(r) => format!("LOẠI TRỪ — {} · {}", r, v.contents_vi()),
                    None => v.contents_vi(),
                };
                lv_insert_row(
                    &app.lst_targets,
                    i as i32,
                    &[
                        format!("{}:", v.letter),
                        if v.label.is_empty() { "(không nhãn)".to_string() } else { v.label.clone() },
                        v.fs.clone(),
                        sys::format_gb(v.total),
                        sys::format_gb(v.free),
                        format_serial(v.serial),
                        v.type_vi().to_string(),
                        note,
                    ],
                );
            }
            TargetEntry::Disk(d) => {
                let parts_inline = d
                    .partitions
                    .iter()
                    .map(|p| {
                        format!(
                            "{} {}",
                            p.letter.map(|c| format!("{}:", c)).unwrap_or_else(|| "•".to_string()),
                            sys::format_gb(p.size)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let note = match disk_block_reason(&m, d) {
                    Some(r) => format!(
                        "TOÀN BỘ Ổ ĐĨA — BỊ CẤM ({}). Gồm: {}",
                        r, parts_inline
                    ),
                    None => format!(
                        "TOÀN BỘ Ổ ĐĨA — XOÁ & CHIA LẠI TỪ ĐẦU (EFI/MSR/Windows/Recovery). Gồm: {}",
                        parts_inline
                    ),
                };
                lv_insert_row(
                    &app.lst_targets,
                    i as i32,
                    &[
                        format!("ĐĨA {}", d.number),
                        if d.model.is_empty() { "(không rõ model)".to_string() } else { d.model.clone() },
                        match d.style {
                            sys::PartitionStyle::Gpt => "GPT".to_string(),
                            sys::PartitionStyle::Mbr => "MBR".to_string(),
                            sys::PartitionStyle::Unknown => "?".to_string(),
                        },
                        sys::format_gb(d.size),
                        "—".to_string(),
                        if d.serial.is_empty() { "(không đọc được)".to_string() } else { d.serial.clone() },
                        "Toàn bộ ổ đĩa".to_string(),
                        note,
                    ],
                );
            }
        }
    }
}

fn refill_prep_drives(app: &Rc<App>) {
    let m = app.model.borrow();
    let items: Vec<String> = m
        .volumes
        .iter()
        .map(|v| {
            format!(
                "{}: — {} — {} (trống {})",
                v.letter,
                if v.label.is_empty() { "không nhãn".to_string() } else { v.label.clone() },
                sys::format_gb(v.total),
                sys::format_gb(v.free)
            )
        })
        .collect();
    app.cmb_prep_drive.set_collection(items);
    if !m.volumes.is_empty() {
        app.cmb_prep_drive.set_selection(Some(0));
    }
}

fn update_target_info(app: &Rc<App>) {
    let sel = app.lst_targets.selected_item();
    let m = app.model.borrow();
    let text = match sel.and_then(|i| m.targets.get(i)) {
        Some(TargetEntry::Volume(v)) => {
            let mut t = format!(
                "Đích đã chọn: {}: — {} — {} (trống {}) — Serial {} — Nội dung: {}.",
                v.letter,
                if v.label.is_empty() { "không nhãn".to_string() } else { v.label.clone() },
                sys::format_gb(v.total),
                sys::format_gb(v.free),
                format_serial(v.serial),
                v.contents_vi()
            );
            if let Some(r) = exclusion_reason(&m, v) {
                t.push_str(&format!(
                    " PHÂN VÙNG NÀY ĐANG BỊ LOẠI TRỪ ({}) — phải bật ô ghi đè ở trên mới cài được.",
                    r
                ));
            }
            t
        }
        Some(TargetEntry::Disk(d)) => {
            let mut t = format!(
                "Đích đã chọn: TOÀN BỘ ĐĨA {} — {} — {} — serial {}.\nCác phân vùng hiện có:\n{}\nChế độ này XOÁ SẠCH cả đĩa rồi chia lại (EFI/MSR/Windows/Recovery) — phải nhập đúng serial của ĐĨA để xác nhận.",
                d.number,
                if d.model.is_empty() { "không rõ model".to_string() } else { d.model.clone() },
                sys::format_gb(d.size),
                if d.serial.is_empty() { "(không đọc được)".to_string() } else { d.serial.clone() },
                d.describe_partitions()
            );
            if let Some(r) = disk_block_reason(&m, d) {
                t.push_str(&format!(
                    " ĐĨA NÀY BỊ CẤM LÀM ĐÍCH XOÁ ({}) — không có cách ghi đè, hãy chọn đĩa khác.",
                    r
                ));
            }
            t
        }
        None => "Chưa chọn đích cài.".to_string(),
    };
    app.lbl_target_info.set_text(&text);
}

fn update_edition_info(app: &Rc<App>) {
    let sel = app.lst_editions.selection();
    let m = app.model.borrow();
    let text = match sel.and_then(|i| m.editions.get(i)) {
        Some(e) => format!(
            "Index {} — {}\nPhiên bản: {} — build {}\nKiến trúc: {} — dung lượng image: {}",
            e.index,
            e.name,
            if e.edition.is_empty() { "—".to_string() } else { e.edition.clone() },
            e.build,
            if e.arch.is_empty() { "—".to_string() } else { e.arch.clone() },
            sys::format_gb(e.size_bytes)
        ),
        None => "Hãy chọn một phiên bản trong danh sách.".to_string(),
    };
    app.lbl_edition_info.set_text(&text);
}

fn render_log(app: &Rc<App>) {
    let (text, total) = {
        let s = app.shared.lock().unwrap();
        (s.log_lines.iter().cloned().collect::<Vec<_>>().join("\r\n"), s.log_lines.len())
    };
    if total != app.model.borrow().log_rendered {
        app.txt_log.set_text(&text);
        app.model.borrow_mut().log_rendered = total;
    }
}

fn set_busy_ui(app: &Rc<App>) {
    let busy = app.shared.lock().unwrap().busy || app.model.borrow().running;
    for c in [
        &app.btn_browse_source,
        &app.btn_load_source,
        &app.btn_refresh_vols,
        &app.btn_browse_driver,
        &app.btn_start,
        &app.btn_confirm_format,
        &app.btn_prepare,
        &app.btn_prep_refresh,
    ] {
        c.set_enabled(!busy);
    }
    let succeeded = app.model.borrow().succeeded;
    app.btn_reboot.set_enabled(succeeded && !busy);
}

fn refresh(app: &Rc<App>) {
    render_log(app);
    // Hộp thoại bị TRÌ HOÃN tới cuối refresh (v0.7.19): trạng
    // thái + nút bấm được cập nhật (set_busy_ui) TRƯỚC khi bất
    // kỳ hộp thoại modal nào hiện — vòng lặp modal của hộp
    // thoại trong WinPE không còn cơ hội nuốt mất lượt cập nhật
    // mở khoá nút như ca thực địa "Không cho bấm nạp".
    let mut pending_dialog: Option<(bool, String)> = None;
    let (volumes, source, progress, status, done) = {
        let mut s = app.shared.lock().unwrap();
        (s.volumes.take(), s.source.take(), s.progress.take(), s.status.take(), s.install_done.take())
    };
    if let Some(st) = status {
        app.lbl_status.set_text(&st);
    }
    if let Some((vols, disks, summary)) = volumes {
        {
            let mut m = app.model.borrow_mut();
            let mut targets: Vec<TargetEntry> = vols
                .iter()
                .filter(|v| v.drive_type != DRIVE_RAMDISK)
                .cloned()
                .map(TargetEntry::Volume)
                .collect();
            // Cuối danh sách: các dòng TOÀN BỘ Ổ ĐĨA (xoá & chia lại)
            targets.extend(disks.into_iter().map(TargetEntry::Disk));
            m.targets = targets;
            m.volumes = vols;
        }
        refill_targets(app);
        refill_prep_drives(app);
        update_target_info(app);
        app.lbl_status.set_text(&format!("Đã liệt kê {} phân vùng. {}", app.model.borrow().volumes.len(), summary));
    }
    if let Some(res) = source {
        match res {
            Ok(loaded) => {
                let info = match loaded.mounted_letter {
                    Some(l) => format!(
                        "Nguồn: {} (mount từ ISO, ổ {}:) — {} phiên bản. Hãy chọn phiên bản rồi chọn phân vùng đích.",
                        loaded.image_path.display(),
                        l,
                        loaded.editions.len()
                    ),
                    None => format!(
                        "Nguồn: {} — {} phiên bản. Hãy chọn phiên bản rồi chọn phân vùng đích.",
                        loaded.image_path.display(),
                        loaded.editions.len()
                    ),
                };
                {
                    let mut m = app.model.borrow_mut();
                    m.mounted_letter = loaded.mounted_letter;
                    if loaded.mounted_letter.is_some() {
                        m.mounted_iso = m.source_file.clone();
                    } else {
                        m.mounted_iso = None;
                    }
                    m.image_path = Some(loaded.image_path);
                    m.swm_pattern = loaded.swm_pattern;
                    m.editions = loaded.editions;
                }
                let items: Vec<String> = app
                    .model
                    .borrow()
                    .editions
                    .iter()
                    .map(|e| {
                        format!(
                            "{}. {} — {} — build {} — {} — {}",
                            e.index,
                            e.name,
                            e.edition,
                            e.build,
                            e.arch,
                            sys::format_gb(e.size_bytes)
                        )
                    })
                    .collect();
                app.lst_editions.set_collection(items);
                if !app.model.borrow().editions.is_empty() {
                    app.lst_editions.set_selection(Some(0));
                }
                app.lbl_source_info.set_text(&info);
                update_edition_info(app);
                // Nguồn đổi -> tính lại ghi chú loại trừ của danh sách đích
                refill_targets(app);
                update_target_info(app);
            }
            Err(e) => {
                app.lbl_source_info.set_text("Nạp nguồn thất bại — xem thông báo lỗi.");
                pending_dialog = Some((true, e));
            }
        }
    }
    if let Some((pct, label)) = progress {
        app.bar.set_pos(pct);
        app.lbl_progress.set_text(&label);
    }
    if let Some(res) = done {
        app.model.borrow_mut().running = false;
        match res {
            Ok(msg) => {
                app.model.borrow_mut().succeeded = true;
                app.lbl_status.set_text("Cài đặt hoàn tất — bấm Khởi động lại để vào Windows mới.");
                pending_dialog = Some((false, msg));
            }
            Err(e) => {
                app.lbl_status.set_text("Cài đặt thất bại — xem log để biết bước lỗi.");
                pending_dialog = Some((true, e));
            }
        }
    }
    // Mở khoá/cập nhật nút TRƯỚC, hộp thoại SAU (v0.7.19 — xem
    // chú thích đầu hàm). Sau khi hộp thoại đóng, đồng bộ lại
    // trạng thái nút một lần nữa cho chắc.
    set_busy_ui(app);
    if let Some((is_error, text)) = pending_dialog {
        if is_error {
            error_box("Cài Đặt Win = WinPE", &text);
        } else {
            info_box("Cài Đặt Win = WinPE", &text);
        }
        set_busy_ui(app);
    }
}

// ---------------------------------------------------------------------------
// Hành động: nạp nguồn
// ---------------------------------------------------------------------------
fn start_load_source(app: &Rc<App>, path: PathBuf) {
    if !path.exists() {
        error_box("Cài Đặt Win = WinPE", &format!("Không thấy file nguồn:\n{}", path.display()));
        return;
    }
    let prev = {
        let mut m = app.model.borrow_mut();
        m.source_file = Some(path.clone());
        match (&m.mounted_iso, &m.mounted_letter) {
            (Some(iso), Some(l)) => Some((iso.clone(), *l)),
            _ => None,
        }
    };
    app.lbl_source_info.set_text(&format!("Đang nạp nguồn {}...", path.display()));
    spawn_load_source(app.shared.clone(), app.notice.sender(), path, prev);
}

// ---------------------------------------------------------------------------
// Hành động: bắt đầu cài (kiểm tra ở UI; thực thi kiểm tra lại)
// ---------------------------------------------------------------------------
fn on_start(app: &Rc<App>) {
    if !app.model.borrow().in_winpe {
        return; // Nút cài chỉ hiện trong WinPE — chốt chặn thêm ở tầng code
    }
    if app.shared.lock().unwrap().busy || app.model.borrow().running {
        return;
    }
    let plan = match build_plan(app, false) {
        Some(p) => p,
        None => return,
    };
    if plan.format || plan.wipe.is_some() {
        // Cần xác nhận bằng serial trước — hiện hàng xác nhận.
        // Format phân vùng: serial của PHÂN VÙNG; wipe đĩa: serial
        // phần cứng của cả ĐĨA ĐÍCH.
        if let Some(w) = &plan.wipe {
            app.lbl_serial.set_text(&format!(
                "XOÁ TOÀN BỘ ĐĨA {} — nhập đúng serial của ĐĨA ĐÍCH để xác nhận:",
                w.disk_number
            ));
            app.lbl_status.set_text(&format!(
                "Đang chờ xác nhận XOÁ TOÀN BỘ ĐĨA {}: nhập serial {} vào ô xác nhận.",
                w.disk_number, w.disk_serial
            ));
        } else {
            app.lbl_serial.set_text(&format!(
                "FORMAT ổ {}: — nhập đúng serial của ổ đích để xác nhận:",
                plan.target_letter
            ));
            app.lbl_status.set_text(&format!(
                "Đang chờ xác nhận FORMAT: nhập serial {} của ổ {}: vào ô xác nhận.",
                plan.confirm_display, plan.target_letter
            ));
        }
        set_ctrl_visible(&app.lbl_serial.handle, true);
        set_ctrl_visible(&app.inp_serial.handle, true);
        set_ctrl_visible(&app.btn_confirm_format.handle, true);
        app.inp_serial.set_text("");
        app.inp_serial.set_focus();
        app.model.borrow_mut().pending_plan = Some(plan);
        return;
    }
    if confirm_install(&plan) {
        begin_install(app, plan);
    }
}

fn on_confirm_format(app: &Rc<App>) {
    let plan = match app.model.borrow_mut().pending_plan.take() {
        Some(p) => p,
        None => return,
    };
    // Chuẩn hoá cả hai phía: bỏ mọi ký tự không phải chữ/số, viết hoa —
    // chấp nhận serial phân vùng dạng XXXX-XXXX lẫn serial đĩa nguyên văn.
    let typed = disk::normalize_disk_serial(&app.inp_serial.text());
    if typed != plan.serial_expect_norm {
        error_box(
            "Cài Đặt Win = WinPE",
            &format!(
                "Serial vừa nhập không khớp serial kỳ vọng ({}).\nKiểm tra cột Serial trong danh sách đích và nhập lại.",
                plan.confirm_display
            ),
        );
        app.model.borrow_mut().pending_plan = Some(plan);
        return;
    }
    hide_serial_row(app);
    if confirm_install(&plan) {
        begin_install(app, plan);
    }
}

fn hide_serial_row(app: &Rc<App>) {
    set_ctrl_visible(&app.lbl_serial.handle, false);
    set_ctrl_visible(&app.inp_serial.handle, false);
    set_ctrl_visible(&app.btn_confirm_format.handle, false);
}

/// Dựng InstallPlan từ giao diện + mọi kiểm tra an toàn phía UI.
/// `for_format_confirm` không dùng — giữ chữ ký gọn cho hai điểm gọi.
fn build_plan(app: &Rc<App>, _for_format_confirm: bool) -> Option<InstallPlan> {
    let title = "Cài Đặt Win = WinPE";
    let (image_path, swm_pattern, edition) = {
        let m = app.model.borrow();
        let image = match &m.image_path {
            Some(p) => p.clone(),
            None => {
                error_box(title, "Chưa nạp nguồn. Hãy chọn file ISO/WIM ở mục 1 và bấm Nạp nguồn.");
                return None;
            }
        };
        let sel = match app.lst_editions.selection() {
            Some(s) => s,
            None => {
                error_box(title, "Hãy chọn một phiên bản Windows trong danh sách (mục 1).");
                return None;
            }
        };
        let ed = match m.editions.get(sel) {
            Some(e) => e.clone(),
            None => return None,
        };
        (image, m.swm_pattern.clone(), ed)
    };
    let target_entry = {
        let m = app.model.borrow();
        let sel = match app.lst_targets.selected_item() {
            Some(s) => s,
            None => {
                error_box(title, "Hãy chọn một đích cài trong danh sách (mục 2): một phân vùng, hoặc một dòng TOÀN BỘ Ổ ĐĨA.");
                return None;
            }
        };
        match m.targets.get(sel) {
            Some(t) => t.clone(),
            None => return None,
        }
    };
    // Các trị đích dùng chung cho hai nhánh (phân vùng / toàn bộ đĩa)
    let target_letter: char;
    let target_serial: u32;
    let format: bool;
    let wipe: Option<WipeTarget>;
    let serial_expect_norm: String;
    let confirm_display: String;
    let target_desc: String;
    match &target_entry {
        TargetEntry::Volume(target) => {
            // Loại trừ mặc định + ghi đè có cảnh báo
            let override_on = app.chk_override.check_state() == nwg::CheckBoxState::Checked;
            if let Some(reason) = exclusion_reason(&app.model.borrow(), target) {
                if !override_on {
                    error_box(
                        title,
                        &format!(
                            "Phân vùng {}: đang bị loại trừ ({}).\nNếu thật sự muốn cài vào đây, hãy bật ô \"Cho phép chọn cả ổ BỊ LOẠI TRỪ\" ở mục 2 trước.",
                            target.letter, reason
                        ),
                    );
                    return None;
                }
                if !confirm_box(
                    title,
                    &format!(
                        "CẢNH BÁO: bạn đang ghi đè lên phân vùng bị loại trừ — {}: là {}.\n\
                         Cài vào đây có thể phá hỏng chính nguồn cài đặt/công cụ đang chạy.\n\nVẫn tiếp tục?",
                        target.letter, reason
                    ),
                    true,
                ) {
                    return None;
                }
            }
            let fmt = app.chk_format.check_state() == nwg::CheckBoxState::Checked;
            // Kiểm tra dung lượng: format thì tính tổng dung lượng, không format tính phần trống
            let avail = if fmt { target.total } else { target.free };
            if edition.size_bytes > 0 && avail < edition.size_bytes + GB {
                error_box(
                    title,
                    &format!(
                        "Phân vùng {}: không đủ chỗ: cần khoảng {} cho image này, hiện có {}.",
                        target.letter,
                        sys::format_gb(edition.size_bytes + GB),
                        sys::format_gb(avail)
                    ),
                );
                return None;
            }
            if !fmt && (target.has_windows || target.has_users) {
                if !confirm_box(
                    title,
                    &format!(
                        "Phân vùng {}: đang có dữ liệu ({}) và bạn KHÔNG chọn Format.\n\
                         DISM apply đè lên thư mục có sẵn thường thất bại hoặc để lại rác của bản Windows cũ.\n\
                         Khuyến nghị quay lại bật Format. Vẫn tiếp tục không format?",
                        target.letter,
                        target.contents_vi()
                    ),
                    true,
                ) {
                    return None;
                }
            }
            target_letter = target.letter;
            target_serial = target.serial;
            format = fmt;
            wipe = None;
            serial_expect_norm = format!("{:08X}", target.serial);
            confirm_display = format_serial(target.serial);
            target_desc = format!(
                "{}: ({})",
                target.letter,
                if target.label.is_empty() { "không nhãn".to_string() } else { target.label.clone() }
            );
        }
        TargetEntry::Disk(d) => {
            // --- Nhánh TOÀN BỘ Ổ ĐĨA (xoá & chia lại, §6b) ---
            // Cấm tuyệt đối: đĩa chứa ổ của công cụ / ổ chứa nguồn.
            // Ô ghi đè ở mục 2 KHÔNG áp dụng cho đích đĩa.
            if let Some(reason) = disk_block_reason(&app.model.borrow(), d) {
                error_box(
                    title,
                    &format!(
                        "Đĩa {} bị CẤM làm đích xoá: {}.\nWipe đĩa này sẽ phá huỷ chính công cụ/nguồn đang dùng để cài — không có cách ghi đè. Hãy chọn một đĩa khác.",
                        d.number, reason
                    ),
                );
                return None;
            }
            if d.serial.trim().is_empty() {
                error_box(title, "Không đọc được serial phần cứng của đĩa này — không thể xác minh đích wipe an toàn. Hãy chọn đĩa khác.");
                return None;
            }
            if d.size < disk::MIN_WIPE_DISK_BYTES {
                error_box(
                    title,
                    &format!(
                        "Đĩa {} chỉ có {} — quá nhỏ để chia lại (cần tối thiểu {}).",
                        d.number,
                        sys::format_gb(d.size),
                        sys::format_gb(disk::MIN_WIPE_DISK_BYTES)
                    ),
                );
                return None;
            }
            if edition.size_bytes > 0 && d.size < edition.size_bytes + 2 * GB {
                error_box(
                    title,
                    &format!(
                        "Đĩa {} không đủ lớn cho image này: cần khoảng {}, đĩa có {}.",
                        d.number,
                        sys::format_gb(edition.size_bytes + 2 * GB),
                        sys::format_gb(d.size)
                    ),
                );
                return None;
            }
            target_letter = '?'; // ký tự ổ thật có sau khi diskpart chia lại
            target_serial = 0;
            format = false; // diskpart đã format trong script chia lại
            wipe = Some(WipeTarget {
                disk_number: d.number,
                disk_serial: d.serial.clone(),
                disk_parts: d.describe_partitions(),
            });
            serial_expect_norm = disk::normalize_disk_serial(&d.serial);
            confirm_display = d.serial.clone();
            target_desc = d.summary();
        }
    }
    // Thư mục driver
    let drv_text = app.inp_driver.text().trim().to_string();
    let driver_dir = if drv_text.is_empty() {
        None
    } else {
        let p = PathBuf::from(&drv_text);
        if !p.is_dir() {
            error_box(title, &format!("Thư mục driver không tồn tại:\n{}", drv_text));
            return None;
        }
        Some(p)
    };
    // autounattend (kèm tên tài khoản + trạng thái mật khẩu để hộp
    // xác nhận cuối và file log nói rõ — mật khẩu không bao giờ
    // được mang sang InstallPlan, chỉ trạng thái có/không)
    let mut unattend_user = String::new();
    let mut unattend_passwordless = false;
    let mut core_profile = false;
    let unattend_xml = if app.chk_unattend.check_state() == nwg::CheckBoxState::Checked {
        let computer = app.inp_computer.text().trim().to_string();
        let user = app.inp_user.text().trim().to_string();
        let pw1 = app.inp_pw1.text();
        let pw2 = app.inp_pw2.text();
        if !valid_computer_name(&computer) {
            error_box(title, "Tên máy tính không hợp lệ: 1–15 ký tự chữ/số/gạch ngang, không toàn số.");
            return None;
        }
        if user.is_empty() || user.chars().any(|c| c.is_whitespace()) {
            error_box(title, "Tài khoản admin không hợp lệ (không được trống, không chứa khoảng trắng).");
            return None;
        }
        // Mật khẩu TUỲ CHỌN (v0.7.2, cùng logic wizard): hai ô lệch
        // nhau thì chặn; cả hai ô trống = tài khoản KHÔNG mật khẩu
        // (hợp lệ — trạng thái này hiện rõ ở hộp xác nhận cuối và
        // ghi vào log, không cần hộp hỏi riêng như trước).
        if pw1 != pw2 {
            error_box(title, "Hai ô mật khẩu không khớp — để trống cả hai nếu không muốn đặt mật khẩu.");
            return None;
        }
        unattend_user = user.clone();
        unattend_passwordless = pw1.is_empty();
        core_profile = app.chk_core.check_state() == nwg::CheckBoxState::Checked;
        let m = app.model.borrow();
        Some(build_unattend(&m.settings, &computer, &user, &pw1, core_profile))
    } else {
        None
    };
    // Thư mục custom cạnh exe (nếu có)
    let custom_dir = {
        let c = app.model.borrow().exe_dir.join("custom");
        if c.is_dir() {
            Some(c)
        } else {
            None
        }
    };
    // Thư mục log: ưu tiên ổ chứa nguồn, rồi ổ chứa exe — tránh ổ đích
    let log_dir = {
        let m = app.model.borrow();
        let mut candidates: Vec<char> = Vec::new();
        if let Some(src) = &m.source_file {
            if let Some(l) = path_letter(src) {
                candidates.push(l);
            }
        }
        if let Some(l) = m.exe_letter {
            candidates.push(l);
        }
        let mut chosen: Option<PathBuf> = None;
        for l in candidates {
            if l != target_letter {
                chosen = Some(PathBuf::from(format!("{}:\\QSU-Logs", l)));
                break;
            }
        }
        chosen.unwrap_or_else(|| std::env::temp_dir().join("QSU-Logs"))
    };
    let forbidden_letters = {
        let m = app.model.borrow();
        let mut v: Vec<char> = Vec::new();
        if let Some(l) = m.exe_letter {
            v.push(l);
        }
        if let Some(src) = &m.source_file {
            if let Some(l) = path_letter(src) {
                v.push(l);
            }
        }
        if let Some(l) = m.mounted_letter {
            v.push(l);
        }
        v
    };
    let uefi = app.model.borrow().uefi;
    Some(InstallPlan {
        image_path,
        swm_pattern,
        index: edition.index,
        edition_name: if edition.name.is_empty() { format!("Index {}", edition.index) } else { edition.name.clone() },
        target_letter,
        target_serial,
        format,
        driver_dir,
        unattend_xml,
        admin_user: unattend_user,
        admin_passwordless: unattend_passwordless,
        core_profile,
        custom_dir,
        log_dir,
        uefi,
        forbidden_letters,
        wipe,
        serial_expect_norm,
        confirm_display,
        target_desc,
    })
}

fn confirm_install(plan: &InstallPlan) -> bool {
    // Dòng trạng thái tài khoản (chỉ khi có autounattend): không
    // đặt mật khẩu phải hiện rõ trong tóm tắt xác nhận (v0.7.2,
    // cùng câu chữ với wizard).
    let account_line = if plan.unattend_xml.is_some() {
        if plan.admin_passwordless {
            format!(
                "• Tài khoản {}: KHÔNG mật khẩu — người dùng tự đặt mật khẩu sau khi cài\n",
                plan.admin_user
            )
        } else {
            format!("• Tài khoản {}: có đặt mật khẩu (không hiển thị)\n", plan.admin_user)
        }
    } else {
        String::new()
    };
    let account_line = if plan.unattend_xml.is_some() {
        if plan.core_profile {
            format!("{}• Hồ sơ CORE (ảnh rút gọn Tiny11 Core): profile tối thiểu, KHÔNG tự cài ứng dụng sau đăng nhập.\n", account_line)
        } else {
            format!("{}• Tự cài UltraViewer, Chrome, 7-Zip sau đăng nhập (cần Internet).\n", account_line)
        }
    } else { account_line };
    if let Some(w) = &plan.wipe {
        let text = format!(
            "Chuẩn bị cài Windows — CHẾ ĐỘ XOÁ TOÀN BỘ Ổ ĐĨA:\n\n\
             • Phiên bản: {} (index {})\n\
             • Image: {}\n\
             • Đích: TOÀN BỘ ĐĨA {} — serial {}\n\
             • Các phân vùng hiện có trên đĩa đích:\n{}\n\
             • Đĩa sẽ được chia lại: boot (EFI/System Reserved){} + Windows + Recovery 600MB (kèm WinRE)\n\
             • Driver offline: {}\n\
             • autounattend: {}\n\
             {}\
             • Thư mục custom: {}\n\n\
             CẢNH BÁO: TOÀN BỘ DỮ LIỆU TRÊN CÁC PHÂN VÙNG NÀY SẼ BỊ XOÁ VĨNH VIỄN.\n\
             Bấm Yes để bắt đầu. Quá trình apply image mất vài phút và không nên ngắt giữa chừng.",
            plan.edition_name,
            plan.index,
            plan.image_path.display(),
            w.disk_number,
            w.disk_serial,
            w.disk_parts,
            if plan.uefi { " + MSR 16MB" } else { "" },
            plan.driver_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "Không".to_string()),
            if plan.unattend_xml.is_some() { "Có" } else { "Không" },
            account_line,
            if plan.custom_dir.is_some() { "Có" } else { "Không" },
        );
        return confirm_box("Xác nhận cài đặt", &text, true);
    }
    let text = format!(
        "Chuẩn bị cài Windows:\n\n\
         • Phiên bản: {} (index {})\n\
         • Image: {}\n\
         • Đích: {} — serial {}\n\
         • Format ổ đích: {}\n\
         • Driver offline: {}\n\
         • autounattend: {}\n\
         {}\
         • Thư mục custom: {}\n\n\
         {}Bấm Yes để bắt đầu. Quá trình apply image mất vài phút và không nên ngắt giữa chừng.",
        plan.edition_name,
        plan.index,
        plan.image_path.display(),
        plan.target_desc,
        plan.confirm_display,
        if plan.format { "CÓ — xoá sạch dữ liệu trên ổ đích" } else { "Không" },
        plan.driver_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "Không".to_string()),
        if plan.unattend_xml.is_some() { "Có" } else { "Không" },
        account_line,
        if plan.custom_dir.is_some() { "Có" } else { "Không" },
        if plan.format { "CẢNH BÁO: toàn bộ dữ liệu trên ổ đích sẽ bị XOÁ.\n" } else { "" },
    );
    confirm_box("Xác nhận cài đặt", &text, plan.format)
}

fn begin_install(app: &Rc<App>, plan: InstallPlan) {
    hide_serial_row(app);
    app.model.borrow_mut().running = true;
    app.model.borrow_mut().succeeded = false;
    app.bar.set_pos(0);
    app.lbl_progress.set_text("Đang bắt đầu...");
    app.lbl_status.set_text("Đang cài đặt — đừng tắt máy/ngắt nguồn.");
    set_busy_ui(app);
    spawn_install(app.shared.clone(), app.notice.sender(), plan);
}

// ---------------------------------------------------------------------------
// Hành động: Chuẩn bị (Windows thường) — chép công cụ ra ổ đã chọn
// ---------------------------------------------------------------------------
fn do_prepare(app: &Rc<App>) {
    if app.model.borrow().in_winpe {
        return; // Chuẩn bị chỉ dành cho Windows thường
    }
    let title = "Chuẩn bị công cụ WinPE";
    let sel = match app.cmb_prep_drive.selection() {
        Some(s) => s,
        None => {
            info_box(title, "Hãy chọn một ổ đích trong danh sách trước.");
            return;
        }
    };
    let vol = match app.model.borrow().volumes.get(sel).cloned() {
        Some(v) => v,
        None => return,
    };
    let dest_root = PathBuf::from(format!("{}:\\", vol.letter));
    let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("CaiDatWinWinPE.exe"));
    let exe_dir = app.model.borrow().exe_dir.clone();
    let sender = app.notice.sender();
    let mut results: Vec<String> = Vec::new();

    let dest_exe = dest_root.join("CaiDatWinWinPE.exe");
    if exe_path == dest_exe {
        results.push(format!("• CaiDatWinWinPE.exe: đã nằm sẵn ở {} — không cần chép.", dest_root.display()));
        push_log(&app.shared, &sender, "Chuẩn bị: công cụ đã nằm sẵn ở ổ đích.");
    } else {
        match std::fs::copy(&exe_path, &dest_exe) {
            Ok(_) => {
                results.push(format!("• Đã chép CaiDatWinWinPE.exe → {}", dest_exe.display()));
                push_log(&app.shared, &sender, &format!("Chuẩn bị: đã chép {} → {}", exe_path.display(), dest_exe.display()));
            }
            Err(e) => {
                results.push(format!("• LỖI chép CaiDatWinWinPE.exe: {}", e));
                push_log(&app.shared, &sender, &format!("Chuẩn bị: lỗi chép công cụ: {}", e));
            }
        }
    }
    let wp_candidates = [exe_dir.join("install").join("WinPEInstall.exe"), exe_dir.join("WinPEInstall.exe")];
    match wp_candidates.iter().find(|p| p.exists()) {
        Some(p) => {
            let dest = dest_root.join("WinPEInstall.exe");
            match std::fs::copy(p, &dest) {
                Ok(_) => {
                    results.push(format!("• Đã chép WinPEInstall.exe → {}", dest.display()));
                    push_log(&app.shared, &sender, &format!("Chuẩn bị: đã chép {} → {}", p.display(), dest.display()));
                }
                Err(e) => results.push(format!("• LỖI chép WinPEInstall.exe: {}", e)),
            }
        }
        None => {
            results.push(
                "• Không thấy WinPEInstall.exe cạnh công cụ — bỏ qua (engine đó chỉ phục vụ luồng wizard tự động; module này chạy độc lập không cần nó)."
                    .to_string(),
            );
        }
    }
    match qsu_common::post_install::copy_host_telegram_config(&dest_root) {
        Ok(true) => results.push("• Đã chép cấu hình Telegram riêng, giới hạn quyền SYSTEM/Administrators.".to_string()),
        Ok(false) => {},
        Err(e) => results.push(format!("• LỖI chuẩn bị cấu hình Telegram: {}", e)),
    }
    let text = results.join("\n");
    app.lbl_prep_result.set_text(&text);
    app.lbl_status.set_text("Đã chuẩn bị xong — vào WinPE, mở CaiDatWinWinPE.exe từ ổ vừa chép.");
    info_box(title, &text);
}

// ---------------------------------------------------------------------------
// Trình duyệt file TÍCH HỢP (v0.7.19) — thay hộp thoại file của
// Windows (không mở được trong WinPE BHT/Anhdv). Chỉ dùng Win32
// cơ bản: GetLogicalDriveStringsW / GetDriveTypeW /
// GetVolumeInformationW để liệt kê ổ, read_dir để duyệt thư
// mục. File nguồn được lọc *.iso/*.wim/*.esd/*.swm và hiện TÊN
// ĐẦY ĐỦ; đường dẫn chọn được trả NGUYÊN VĂN vào ô nguồn (đúng
// như khi gõ tay — đường đã cứu ca thực địa 2026-11-18).
// ---------------------------------------------------------------------------
fn drive_type_name(t: u32) -> &'static str {
    match t {
        DRIVE_FIXED => "ổ cố định",
        DRIVE_REMOVABLE => "ổ di động/USB",
        DRIVE_CDROM => "ổ CD/DVD",
        DRIVE_REMOTE => "ổ mạng",
        DRIVE_RAMDISK => "ổ RAM (WinPE)",
        _ => "loại khác",
    }
}

/// Mọi ổ đĩa logic hiện có: (ký tự, dòng hiển thị kèm nhãn + loại).
fn browser_list_drives() -> Vec<(char, String)> {
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetLogicalDriveStringsW(buf.len() as DWORD, buf.as_mut_ptr()) } as usize;
    let mut out = Vec::new();
    if n == 0 {
        return out;
    }
    buf.truncate(n.min(buf.len()));
    for part in buf.split(|c| *c == 0) {
        if part.len() < 2 {
            continue;
        }
        let letter = (part[0] as u8 as char).to_ascii_uppercase();
        if !letter.is_ascii_alphabetic() {
            continue;
        }
        let root = format!("{}:\\", letter);
        let root_w = sys::wide(&root);
        let dtype = unsafe { GetDriveTypeW(root_w.as_ptr()) };
        let mut label_buf = vec![0u16; 261];
        let ok = unsafe {
            GetVolumeInformationW(
                root_w.as_ptr(),
                label_buf.as_mut_ptr(),
                label_buf.len() as DWORD,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
            )
        };
        let label = if ok != 0 {
            sys::from_wide(&label_buf)
        } else {
            String::new()
        };
        out.push((
            letter,
            format!(
                "{}: — {} — {}",
                letter,
                if label.is_empty() { "(không nhãn)".to_string() } else { label },
                drive_type_name(dtype)
            ),
        ));
    }
    out
}

/// Đổ danh sách mục của thư mục đang đứng lên trình duyệt file.
fn browser_fill(app: &Rc<App>) {
    let (mode, dir) = {
        let m = app.model.borrow();
        (m.browser_mode, m.browser_dir.clone())
    };
    let mut entries: Vec<(PathBuf, bool)> = Vec::new();
    let mut items: Vec<String> = Vec::new();
    if let Some(parent) = dir.parent() {
        entries.push((parent.to_path_buf(), true));
        items.push("..  (lên một cấp)".to_string());
    }
    let mut folders: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                folders.push(p);
            } else if mode == BrowserMode::SourceFile {
                let ext = p
                    .extension()
                    .map(|x| x.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                if ["iso", "wim", "esd", "swm"].contains(&ext.as_str()) {
                    files.push(p);
                }
            }
        }
    }
    let name_key = |p: &PathBuf| {
        p.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    folders.sort_by_key(name_key);
    files.sort_by_key(name_key);
    for f in folders {
        items.push(format!(
            "[Thư mục] {}",
            f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        ));
        entries.push((f, true));
    }
    for f in files {
        items.push(
            f.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        entries.push((f, false));
    }
    let empty_note = entries.is_empty() || (entries.len() == 1 && dir.parent().is_some());
    app.lst_bentries.set_collection(items);
    app.lbl_bpath.set_text(&format!(
        "Đang ở: {}{}",
        dir.display(),
        if empty_note && mode == BrowserMode::SourceFile {
            "  —  (không có file .iso/.wim/.esd/.swm nào ở đây)"
        } else {
            ""
        }
    ));
    app.btn_bchoose.set_text(if mode == BrowserMode::DriverFolder {
        "Chọn thư mục này"
    } else {
        "Chọn mục đã đánh dấu"
    });
    app.model.borrow_mut().browser_entries = entries;
}

/// Mở trình duyệt file ở chế độ tương ứng.
fn browser_open(app: &Rc<App>, mode: BrowserMode) {
    let drives = browser_list_drives();
    let items: Vec<String> = drives.iter().map(|(_, d)| d.clone()).collect();
    app.lst_bdrives.set_collection(items);
    app.lbl_btitle.set_text(if mode == BrowserMode::DriverFolder {
        "Chọn thư mục driver để inject offline"
    } else {
        "Chọn file nguồn cài đặt (.iso / .wim / .esd / .swm)"
    });
    app.win_browser.set_text(if mode == BrowserMode::DriverFolder {
        "Chọn thư mục driver — trình duyệt tích hợp"
    } else {
        "Chọn file nguồn — trình duyệt tích hợp"
    });
    // Thư mục bắt đầu: thư mục chứa đường dẫn đang có trong ô
    // tương ứng; không có thì thư mục chứa chính module.
    let start = {
        let m = app.model.borrow();
        let cur = if mode == BrowserMode::SourceFile {
            app.inp_source.text()
        } else {
            app.inp_driver.text()
        };
        let p = PathBuf::from(cur.trim());
        let cand = if p.is_dir() {
            Some(p.clone())
        } else {
            p.parent().map(|x| x.to_path_buf())
        };
        cand.filter(|d| d.is_dir())
            .unwrap_or_else(|| m.exe_dir.clone())
    };
    {
        let mut m = app.model.borrow_mut();
        m.browser_mode = mode;
        m.browser_drives = drives.iter().map(|(c, _)| *c).collect();
        m.browser_dir = start;
    }
    browser_fill(app);
    set_ctrl_visible(&app.win_browser.handle, true);
    app.lst_bentries.set_focus();
}

/// Kích hoạt một mục trong trình duyệt: thư mục thì đi vào,
/// file thì chọn luôn (gọi từ double-click và nút Chọn).
fn browser_activate(app: &Rc<App>, idx: usize) {
    let entry = {
        let m = app.model.borrow();
        m.browser_entries.get(idx).cloned()
    };
    let Some((path, is_dir)) = entry else { return };
    if is_dir {
        app.model.borrow_mut().browser_dir = path;
        browser_fill(app);
    } else {
        set_ctrl_visible(&app.win_browser.handle, false);
        app.inp_source.set_text(&path.to_string_lossy());
        start_load_source(app, path);
    }
}

fn handle_browser_event(app: &Rc<App>, evt: nwg::Event, _data: &nwg::EventData, handle: nwg::ControlHandle) {
    match evt {
        nwg::Event::OnButtonClick => {
            if handle == app.btn_bcancel.handle {
                set_ctrl_visible(&app.win_browser.handle, false);
            } else if handle == app.btn_bup.handle {
                let parent = {
                    let m = app.model.borrow();
                    m.browser_dir.parent().map(|p| p.to_path_buf())
                };
                if let Some(p) = parent {
                    app.model.borrow_mut().browser_dir = p;
                    browser_fill(app);
                }
            } else if handle == app.btn_bchoose.handle {
                let mode = app.model.borrow().browser_mode;
                if mode == BrowserMode::DriverFolder {
                    // Thư mục đang đánh dấu (nếu là thư mục) thắng;
                    // không thì thư mục đang đứng.
                    let sel = app.lst_bentries.selection();
                    let chosen = {
                        let m = app.model.borrow();
                        match sel.and_then(|i| m.browser_entries.get(i).cloned()) {
                            Some((p, true)) => p,
                            _ => m.browser_dir.clone(),
                        }
                    };
                    app.inp_driver.set_text(&chosen.to_string_lossy());
                    set_ctrl_visible(&app.win_browser.handle, false);
                } else if let Some(i) = app.lst_bentries.selection() {
                    browser_activate(app, i);
                }
            }
        }
        nwg::Event::OnListBoxSelect => {
            if handle == app.lst_bdrives.handle {
                if let Some(i) = app.lst_bdrives.selection() {
                    let letter = {
                        let m = app.model.borrow();
                        m.browser_drives.get(i).copied()
                    };
                    if let Some(l) = letter {
                        app.model.borrow_mut().browser_dir = PathBuf::from(format!("{}:\\", l));
                        browser_fill(app);
                    }
                }
            } else if handle == app.lst_bentries.handle {
                if let Some(i) = app.lst_bentries.selection() {
                    let e = {
                        let m = app.model.borrow();
                        m.browser_entries.get(i).cloned()
                    };
                    if let Some((p, _)) = e {
                        app.lbl_bpath.set_text(&format!("Đã đánh dấu: {}", p.display()));
                    }
                }
            }
        }
        nwg::Event::OnListBoxDoubleClick => {
            if handle == app.lst_bentries.handle {
                if let Some(i) = app.lst_bentries.selection() {
                    browser_activate(app, i);
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Sự kiện
// ---------------------------------------------------------------------------
fn handle_event(app: &Rc<App>, evt: nwg::Event, data: &nwg::EventData, handle: nwg::ControlHandle) {
    match evt {
        nwg::Event::OnNotice => {
            if handle == app.notice.handle {
                refresh(app);
            }
        }
        nwg::Event::OnButtonClick => {
            if handle == app.btn_close.handle {
                if !app.model.borrow().running {
                    if let Some(iso) = app.model.borrow().mounted_iso.clone() {
                        dismount_iso(&iso);
                    }
                    // Dọn mọi mount VirtDisk còn lại của tiến trình.
                    qsu_common::iso::dismount_all();
                    nwg::stop_thread_dispatch();
                }
            } else if handle == app.btn_browse_source.handle {
                // Trình duyệt file TÍCH HỢP (v0.7.19) — hộp thoại
                // file của Windows không mở được trong WinPE này.
                browser_open(app, BrowserMode::SourceFile);
            } else if handle == app.btn_load_source.handle {
                let text = app.inp_source.text().trim().to_string();
                if text.is_empty() {
                    error_box("Cài Đặt Win = WinPE", "Hãy nhập đường dẫn hoặc bấm \"Chọn file...\" trước.");
                } else {
                    start_load_source(app, PathBuf::from(text));
                }
            } else if handle == app.btn_refresh_vols.handle || handle == app.btn_prep_refresh.handle {
                spawn_enumerate(app.shared.clone(), app.notice.sender());
            } else if handle == app.btn_browse_driver.handle {
                // Cùng trình duyệt tích hợp, chế độ chọn thư mục.
                browser_open(app, BrowserMode::DriverFolder);
            } else if handle == app.btn_start.handle {
                on_start(app);
            } else if handle == app.btn_confirm_format.handle {
                on_confirm_format(app);
            } else if handle == app.btn_reboot.handle {
                if confirm_box(
                    "Khởi động lại",
                    "Khởi động lại máy ngay để vào Windows mới?",
                    false,
                ) {
                    if cmd::run_capture("wpeutil", &["reboot"]).is_err() {
                        let _ = cmd::run_capture("shutdown", &["/r", "/t", "5"]);
                    }
                }
            } else if handle == app.btn_prepare.handle {
                do_prepare(app);
            }
        }
        nwg::Event::OnListBoxSelect => {
            if handle == app.lst_editions.handle {
                update_edition_info(app);
            }
        }
        nwg::Event::OnListViewItemChanged => {
            if handle == app.lst_targets.handle {
                let (_row, _col, selected) = data.on_list_view_item_changed();
                if selected {
                    update_target_info(app);
                }
            }
        }
        _ => {}
    }
}

fn main() {
    let in_winpe = detect_winpe();
    // Trong WinPE tiến trình luôn chạy với đặc quyền cao nhất (SYSTEM)
    // nên bỏ qua vòng nâng quyền; ở Windows thường thì tự nâng như các
    // module khác (Chuẩn bị chép ra USB thực ra không cần admin, nhưng
    // giữ đồng nhất hành vi khi launcher mở module).
    if !in_winpe && !sys::is_admin() {
        if sys::relaunch_as_admin() {
            return;
        }
        return;
    }
    nwg::init().expect("Không khởi tạo được native-windows-gui");
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let (_window, app) = build_ui(exe_dir, in_winpe);
    push_log(
        &app.shared,
        &app.notice.sender(),
        if in_winpe {
            "--- Khởi động Cài Đặt Win = WinPE (chế độ WinPE) ---"
        } else {
            "--- Khởi động Cài Đặt Win = WinPE (chế độ Chuẩn bị trên Windows thường) ---"
        },
    );
    spawn_enumerate(app.shared.clone(), app.notice.sender());
    nwg::dispatch_thread_events();
}
