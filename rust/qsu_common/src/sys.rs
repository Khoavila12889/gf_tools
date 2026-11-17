//! Thông tin hệ thống cho giai đoạn kiểm tra tiền đề (đặc tả §2):
//! quyền admin, nguồn điện, phiên bản OS, boot mode, kiểu phân vùng,
//! danh sách ổ đĩa, key OEM trong BIOS. Tất cả qua Win32 API gốc.

use std::path::PathBuf;
use std::ptr;

use winapi::shared::minwindef::{DWORD, FALSE, HKEY, TRUE};
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::fileapi::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
};
use winapi::um::handleapi::CloseHandle;
use winapi::um::ioapiset::DeviceIoControl;
use winapi::um::processthreadsapi::{GetCurrentProcess, OpenProcessToken};
use winapi::um::shellapi::ShellExecuteW;
use winapi::um::winbase::{
    GetFirmwareEnvironmentVariableW, GetSystemPowerStatus, DRIVE_FIXED, SYSTEM_POWER_STATUS,
};
use winapi::um::winioctl::IOCTL_DISK_GET_DRIVE_LAYOUT_EX;
use winapi::um::winnt::{
    TokenElevation, HANDLE, KEY_READ, KEY_WRITE, REG_SZ, TOKEN_ELEVATION, TOKEN_QUERY,
};
use winapi::um::winreg::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW, HKEY_LOCAL_MACHINE,
};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

// ---------------------------------------------------------------------------
// Quyền quản trị / nâng quyền
// ---------------------------------------------------------------------------
pub fn is_admin() -> bool {
    unsafe {
        let mut token: HANDLE = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elev: TOKEN_ELEVATION = std::mem::zeroed();
        let mut ret: DWORD = 0;
        let ok = winapi::um::securitybaseapi::GetTokenInformation(
            token,
            TokenElevation,
            &mut elev as *mut TOKEN_ELEVATION as *mut winapi::ctypes::c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as DWORD,
            &mut ret,
        );
        CloseHandle(token);
        ok != 0 && elev.TokenIsElevated != 0
    }
}

/// Chạy lại chính tiến trình hiện tại bằng verb "runas". Trả true nếu đã
/// phát lệnh chạy lại (tiến trình hiện tại nên thoát ngay sau đó).
pub fn relaunch_as_admin() -> bool {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg_str = args
        .iter()
        .map(|a| format!("\"{}\"", a.replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(" ");
    let op = wide("runas");
    let file = wide(&exe.to_string_lossy());
    let params = wide(&arg_str);
    let dir = wide(&std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default());
    let rc = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            if arg_str.is_empty() { ptr::null() } else { params.as_ptr() },
            dir.as_ptr(),
            winapi::um::winuser::SW_SHOWNORMAL,
        ) as isize
    };
    rc > 32
}

// ---------------------------------------------------------------------------
// Registry (chỉ đọc/ghi các khóa của chính bộ công cụ + đọc thông tin OS)
// ---------------------------------------------------------------------------
pub fn reg_get_sz(root: HKEY, subkey: &str, value: &str) -> Option<String> {
    unsafe {
        let sk = wide(subkey);
        let mut hkey: HKEY = ptr::null_mut();
        if RegOpenKeyExW(root, sk.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return None;
        }
        let name = wide(value);
        let mut buf = vec![0u16; 1024];
        let mut len: DWORD = (buf.len() * 2) as DWORD;
        let mut typ: DWORD = 0;
        let rc = RegQueryValueExW(
            hkey,
            name.as_ptr(),
            ptr::null_mut(),
            &mut typ,
            buf.as_mut_ptr() as *mut u8,
            &mut len,
        );
        RegCloseKey(hkey);
        if rc != 0 {
            return None;
        }
        let n = (len as usize / 2).min(buf.len());
        Some(from_wide(&buf[..n]).trim().to_string())
    }
}

pub fn reg_set_sz(root: HKEY, subkey: &str, value: &str, data: &str) -> bool {
    unsafe {
        let sk = wide(subkey);
        let mut hkey: HKEY = ptr::null_mut();
        let mut disp: DWORD = 0;
        if RegCreateKeyExW(
            root,
            sk.as_ptr(),
            0,
            ptr::null_mut(),
            0,
            KEY_WRITE,
            ptr::null_mut(),
            &mut hkey,
            &mut disp,
        ) != 0
        {
            return false;
        }
        let name = wide(value);
        let val = wide(data);
        let rc = RegSetValueExW(
            hkey,
            name.as_ptr(),
            0,
            REG_SZ,
            val.as_ptr() as *const u8,
            (val.len() * 2) as DWORD,
        );
        RegCloseKey(hkey);
        rc == 0
    }
}

pub fn reg_delete_value(root: HKEY, subkey: &str, value: &str) -> bool {
    unsafe {
        let sk = wide(subkey);
        let mut hkey: HKEY = ptr::null_mut();
        if RegOpenKeyExW(root, sk.as_ptr(), 0, KEY_WRITE, &mut hkey) != 0 {
            return false;
        }
        let name = wide(value);
        let rc = RegDeleteValueW(hkey, name.as_ptr());
        RegCloseKey(hkey);
        rc == 0
    }
}

pub fn hklm() -> HKEY {
    HKEY_LOCAL_MACHINE
}

// ---------------------------------------------------------------------------
// Thông tin OS hiện tại
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Default)]
pub struct OsInfo {
    pub product_name: String,
    pub edition_id: String,
    pub build: u32,
    pub display_version: String,
}

pub fn os_info() -> OsInfo {
    let key = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion";
    let mut info = OsInfo {
        product_name: reg_get_sz(hklm(), key, "ProductName").unwrap_or_default(),
        edition_id: reg_get_sz(hklm(), key, "EditionID").unwrap_or_default(),
        build: reg_get_sz(hklm(), key, "CurrentBuildNumber")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        display_version: reg_get_sz(hklm(), key, "DisplayVersion").unwrap_or_default(),
    };
    if info.build >= 22000 && info.product_name.starts_with("Windows 10") {
        info.product_name = info.product_name.replacen("Windows 10", "Windows 11", 1);
    }
    info
}

pub fn computer_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Boot mode + kiểu phân vùng đĩa hệ thống
// ---------------------------------------------------------------------------
/// UEFI nếu firmware cho gọi GetFirmwareEnvironmentVariableW; trên máy
/// BIOS thuần tuý API này trả ERROR_INVALID_FUNCTION.
pub fn firmware_is_uefi() -> bool {
    unsafe {
        let name = wide("");
        let guid = wide("{00000000-0000-0000-0000-000000000000}");
        let mut buf = [0u8; 1];
        let rc = GetFirmwareEnvironmentVariableW(
            name.as_ptr(),
            guid.as_ptr(),
            buf.as_mut_ptr() as *mut winapi::ctypes::c_void,
            0,
        );
        !(rc == 0 && GetLastError() == 1) // 1 = ERROR_INVALID_FUNCTION -> Legacy BIOS
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PartitionStyle {
    Gpt,
    Mbr,
    Unknown,
}

/// Kiểu phân vùng của PhysicalDriveN (0 = đĩa hệ thống thông thường).
pub fn disk_partition_style(drive_number: u32) -> PartitionStyle {
    unsafe {
        let path = wide(&format!("\\\\.\\PhysicalDrive{}", drive_number));
        let handle = winapi::um::fileapi::CreateFileW(
            path.as_ptr(),
            0, // chỉ cần metadata
            3, // FILE_SHARE_READ | FILE_SHARE_WRITE
            ptr::null_mut(),
            winapi::um::fileapi::OPEN_EXISTING,
            0,
            ptr::null_mut(),
        );
        if handle == winapi::um::handleapi::INVALID_HANDLE_VALUE {
            return PartitionStyle::Unknown;
        }
        // DRIVE_LAYOUT_INFORMATION_EX bắt đầu bằng PartitionStyle (DWORD).
        // IOCTL này đòi buffer đủ chứa TOÀN BỘ layout (mọi phân vùng),
        // nên phải cấp buffer lớn — buffer nhỏ khiến lệnh thất bại trên
        // hầu hết đĩa thật (nhiều phân vùng) và trả về Unknown oan.
        let mut buf = vec![0u8; 65536];
        let mut returned: DWORD = 0;
        let ok = DeviceIoControl(
            handle,
            IOCTL_DISK_GET_DRIVE_LAYOUT_EX,
            ptr::null_mut(),
            0,
            buf.as_mut_ptr() as *mut winapi::ctypes::c_void,
            buf.len() as DWORD,
            &mut returned,
            ptr::null_mut(),
        );
        CloseHandle(handle);
        if ok == 0 || returned < 4 {
            return PartitionStyle::Unknown;
        }
        match u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) {
            1 => PartitionStyle::Gpt,
            0 => PartitionStyle::Mbr,
            _ => PartitionStyle::Unknown,
        }
    }
}

// ---------------------------------------------------------------------------
// Nguồn điện
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Copy)]
pub struct PowerStatus {
    pub ac_online: bool,
    pub has_battery: bool,
    pub percent: u8, // 255 = không rõ
}

pub fn power_status() -> PowerStatus {
    unsafe {
        let mut st: SYSTEM_POWER_STATUS = std::mem::zeroed();
        if GetSystemPowerStatus(&mut st) == 0 {
            return PowerStatus { ac_online: true, has_battery: false, percent: 255 };
        }
        PowerStatus {
            ac_online: st.ACLineStatus == 1,
            // BatteryFlag 128 = không có pin hệ thống
            has_battery: st.BatteryFlag != 128 && st.BatteryFlag != 255,
            percent: st.BatteryLifePercent,
        }
    }
}

// ---------------------------------------------------------------------------
// Ổ đĩa / phân vùng
// ---------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct DriveInfo {
    pub letter: char,
    pub label: String,
    pub serial: u32,
    pub total: u64,
    pub free: u64,
}

pub fn list_fixed_drives() -> Vec<DriveInfo> {
    unsafe {
        let mut buf = vec![0u16; 512];
        let n = GetLogicalDriveStringsW(buf.len() as DWORD, buf.as_mut_ptr());
        if n == 0 {
            return Vec::new();
        }
        let mut drives = Vec::new();
        let mut i = 0usize;
        while i < buf.len() && buf[i] != 0 {
            let start = i;
            while i < buf.len() && buf[i] != 0 {
                i += 1;
            }
            let root = from_wide(&buf[start..i]);
            i += 1; // qua ký tự NUL ngăn cách
            if root.len() < 2 {
                continue;
            }
            let root_w = wide(&root);
            if GetDriveTypeW(root_w.as_ptr()) != DRIVE_FIXED {
                continue;
            }
            let letter = root.chars().next().unwrap_or('?');
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
                continue;
            }
            let mut free: u64 = 0;
            let mut total: u64 = 0;
            GetDiskFreeSpaceExW(
                root_w.as_ptr(),
                &mut free as *mut u64 as *mut _,
                &mut total as *mut u64 as *mut _,
                ptr::null_mut(),
            );
            drives.push(DriveInfo {
                letter,
                label: from_wide(&label_buf),
                serial,
                total,
                free,
            });
        }
        drives
    }
}

pub fn os_drive_letter() -> char {
    let mut buf = vec![0u16; 260];
    unsafe {
        let n = winapi::um::sysinfoapi::GetWindowsDirectoryW(buf.as_mut_ptr(), buf.len() as DWORD);
        if n > 0 {
            let s = from_wide(&buf);
            if let Some(c) = s.chars().next() {
                return c.to_ascii_uppercase();
            }
        }
    }
    'C'
}

/// Kiểm tra key OEM (bảng MSDM trong ACPI firmware) — chỉ trả Có/Không,
/// không đọc/không ghi lại giá trị key.
pub fn oem_key_present() -> bool {
    fn sig(s: &str) -> u32 {
        let b = s.as_bytes();
        (b[0] as u32) | ((b[1] as u32) << 8) | ((b[2] as u32) << 16) | ((b[3] as u32) << 24)
    }
    unsafe {
        let size = winapi::um::sysinfoapi::GetSystemFirmwareTable(
            sig("ACPI"),
            sig("MSDM"),
            ptr::null_mut(),
            0,
        );
        size > 0
    }
}

pub fn format_gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
}

pub const TRUE_: i32 = TRUE;
pub const FALSE_: i32 = FALSE;
