//! Engine Registry gốc (Win32 API, không qua `reg.exe`): đọc/ghi DWORD
//! và chuỗi (REG_SZ), đặt giá trị mặc định của khoá, xoá giá trị, kiểm
//! tra khoá tồn tại, xoá cả cây khoá (dùng cho hoàn tác menu ngữ cảnh
//! cổ điển). Mọi hàm trả `Result` kèm mã lỗi Win32 để ghi log.

use std::ptr;

use winapi::shared::minwindef::{DWORD, HKEY};
use winapi::um::winnt::{KEY_READ, KEY_WRITE, REG_DWORD, REG_SZ};
use winapi::um::winreg::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS,
};

use qsu_common::sys;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Root {
    Hkcu,
    Hklm,
    Hku,
}

impl Root {
    pub fn hkey(self) -> HKEY {
        match self {
            Root::Hkcu => HKEY_CURRENT_USER,
            Root::Hklm => HKEY_LOCAL_MACHINE,
            Root::Hku => HKEY_USERS,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Root::Hkcu => "HKCU",
            Root::Hklm => "HKLM",
            Root::Hku => "HKU",
        }
    }
}

fn open(root: Root, path: &str, access: u32) -> Result<HKEY, String> {
    unsafe {
        let sk = sys::wide(path);
        let mut hkey: HKEY = ptr::null_mut();
        let rc = RegOpenKeyExW(root.hkey(), sk.as_ptr(), 0, access, &mut hkey);
        if rc != 0 || hkey.is_null() {
            return Err(format!("mở khoá {} {} lỗi {}", root.label(), path, rc));
        }
        Ok(hkey)
    }
}

fn create(root: Root, path: &str) -> Result<HKEY, String> {
    unsafe {
        let sk = sys::wide(path);
        let mut hkey: HKEY = ptr::null_mut();
        let rc = RegCreateKeyExW(
            root.hkey(),
            sk.as_ptr(),
            0,
            ptr::null_mut(),
            0,
            KEY_WRITE,
            ptr::null_mut(),
            &mut hkey,
            ptr::null_mut(),
        );
        if rc != 0 || hkey.is_null() {
            return Err(format!("tạo/mở khoá {} {} lỗi {}", root.label(), path, rc));
        }
        Ok(hkey)
    }
}

pub fn get_dword(root: Root, path: &str, name: &str) -> Option<u32> {
    unsafe {
        let hkey = open(root, path, KEY_READ).ok()?;
        let vn = sys::wide(name);
        let mut data: u32 = 0;
        let mut len: DWORD = 4;
        let mut typ: DWORD = 0;
        let rc = RegQueryValueExW(
            hkey,
            vn.as_ptr(),
            ptr::null_mut(),
            &mut typ,
            &mut data as *mut u32 as *mut u8,
            &mut len,
        );
        RegCloseKey(hkey);
        if rc == 0 && typ == REG_DWORD {
            Some(data)
        } else {
            None
        }
    }
}

pub fn get_sz(root: Root, path: &str, name: &str) -> Option<String> {
    unsafe {
        let hkey = open(root, path, KEY_READ).ok()?;
        let vn = sys::wide(name);
        let mut buf = vec![0u16; 1024];
        let mut len: DWORD = (buf.len() * 2) as DWORD;
        let mut typ: DWORD = 0;
        let rc = RegQueryValueExW(
            hkey,
            vn.as_ptr(),
            ptr::null_mut(),
            &mut typ,
            buf.as_mut_ptr() as *mut u8,
            &mut len,
        );
        RegCloseKey(hkey);
        if rc != 0 || typ != REG_SZ {
            return None;
        }
        let n = (len as usize / 2).min(buf.len());
        Some(sys::from_wide(&buf[..n]).trim().to_string())
    }
}

pub fn set_dword(root: Root, path: &str, name: &str, value: u32) -> Result<(), String> {
    unsafe {
        let hkey = create(root, path)?;
        let vn = sys::wide(name);
        let rc = RegSetValueExW(hkey, vn.as_ptr(), 0, REG_DWORD, &value as *const u32 as *const u8, 4);
        RegCloseKey(hkey);
        if rc == 0 {
            Ok(())
        } else {
            Err(format!("ghi {} {}\\{} lỗi {}", root.label(), path, name, rc))
        }
    }
}

fn set_sz_impl(root: Root, path: &str, name: Option<&str>, value: &str) -> Result<(), String> {
    unsafe {
        let hkey = create(root, path)?;
        let val = sys::wide(value);
        let name_wide = name.map(sys::wide);
        let name_ptr = match &name_wide {
            Some(v) => v.as_ptr(),
            None => ptr::null(), // NULL = giá trị (Default) của khoá
        };
        let rc = RegSetValueExW(
            hkey,
            name_ptr,
            0,
            REG_SZ,
            val.as_ptr() as *const u8,
            (val.len() * 2) as DWORD,
        );
        RegCloseKey(hkey);
        if rc == 0 {
            Ok(())
        } else {
            Err(format!("ghi chuỗi {} {}\\{} lỗi {}", root.label(), path, name.unwrap_or("(Default)"), rc))
        }
    }
}

pub fn set_sz(root: Root, path: &str, name: &str, value: &str) -> Result<(), String> {
    set_sz_impl(root, path, Some(name), value)
}

/// Đặt giá trị mặc định (Default) của một khoá — dùng cho tweak menu
/// chuột phải cổ điển của Windows 11 (khoá InprocServer32 với (Default)="").
pub fn set_default_sz(root: Root, path: &str, value: &str) -> Result<(), String> {
    set_sz_impl(root, path, None, value)
}

pub fn delete_value(root: Root, path: &str, name: &str) -> Result<(), String> {
    unsafe {
        // Khoá không tồn tại => coi như đã xoá
        let hkey = match open(root, path, KEY_WRITE) {
            Ok(h) => h,
            Err(_) => return Ok(()),
        };
        let vn = sys::wide(name);
        let rc = RegDeleteValueW(hkey, vn.as_ptr());
        RegCloseKey(hkey);
        // 2 = ERROR_FILE_NOT_FOUND: giá trị vốn không có, coi như xong
        if rc == 0 || rc == 2 {
            Ok(())
        } else {
            Err(format!("xoá {} {}\\{} lỗi {}", root.label(), path, name, rc))
        }
    }
}

pub fn key_exists(root: Root, path: &str) -> bool {
    match open(root, path, KEY_READ) {
        Ok(h) => {
            unsafe { RegCloseKey(h) };
            true
        }
        Err(_) => false,
    }
}

pub fn delete_tree(root: Root, path: &str) -> Result<(), String> {
    unsafe {
        let sk = sys::wide(path);
        let rc = RegDeleteTreeW(root.hkey(), sk.as_ptr());
        // 2 = không tồn tại: coi như đã xoá
        if rc == 0 || rc == 2 {
            Ok(())
        } else {
            Err(format!("xoá cây khoá {} {} lỗi {}", root.label(), path, rc))
        }
    }
}
