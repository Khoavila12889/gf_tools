//! Điều khiển Windows Service qua Service Control Manager (Win32 gốc,
//! không qua `sc.exe`/`net.exe`): truy vấn trạng thái + kiểu khởi động,
//! start/stop (có chờ tới trạng thái đích), đổi kiểu khởi động.
//!
//! Dùng cho module Quản Lý Update (wuauserv/UsoSvc/BITS...) và module
//! Dọn Rác (dừng wuauserv khi xoá cache Windows Update).

use std::ptr;
use std::time::{Duration, Instant};

use winapi::shared::minwindef::DWORD;
use winapi::um::winsvc::{
    ChangeServiceConfigW, CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW,
    QueryServiceConfigW, QueryServiceStatusEx, StartServiceW, SC_HANDLE, SC_STATUS_PROCESS_INFO,
    SERVICE_STATUS_PROCESS,
};

// Mã trạng thái dịch vụ (SERVICE_* trong winsvc.h)
pub const STATE_STOPPED: DWORD = 1;
pub const STATE_START_PENDING: DWORD = 2;
pub const STATE_STOP_PENDING: DWORD = 3;
pub const STATE_RUNNING: DWORD = 4;
pub const STATE_PAUSED: DWORD = 7;

// Kiểu khởi động (dwStartType)
pub const START_BOOT: DWORD = 0;
pub const START_SYSTEM: DWORD = 1;
pub const START_AUTO: DWORD = 2;
pub const START_DEMAND: DWORD = 3;
pub const START_DISABLED: DWORD = 4;

const SERVICE_NO_CHANGE: DWORD = 0xFFFF_FFFF;

#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub exists: bool,
    pub state: DWORD,
    pub start_type: DWORD,
}

pub fn state_label(state: DWORD) -> String {
    match state {
        STATE_STOPPED => "Đã dừng".to_string(),
        STATE_START_PENDING => "Đang khởi động...".to_string(),
        STATE_STOP_PENDING => "Đang dừng...".to_string(),
        STATE_RUNNING => "Đang chạy".to_string(),
        5 => "Đang tiếp tục...".to_string(),
        6 => "Đang tạm dừng...".to_string(),
        STATE_PAUSED => "Đã tạm dừng".to_string(),
        _ => format!("Không rõ ({})", state),
    }
}

pub fn start_type_label(start_type: DWORD) -> String {
    match start_type {
        START_BOOT => "Boot".to_string(),
        START_SYSTEM => "System".to_string(),
        START_AUTO => "Tự động (Automatic)".to_string(),
        START_DEMAND => "Thủ công (Manual)".to_string(),
        START_DISABLED => "Vô hiệu hoá (Disabled)".to_string(),
        _ => format!("Khác ({})", start_type),
    }
}

fn open_manager() -> Option<SC_HANDLE> {
    unsafe {
        let mgr = OpenSCManagerW(ptr::null(), ptr::null(), 0x0001 /* SC_MANAGER_CONNECT */);
        if mgr.is_null() {
            None
        } else {
            Some(mgr)
        }
    }
}

fn open_service(mgr: SC_HANDLE, name: &str, access: DWORD) -> Option<SC_HANDLE> {
    unsafe {
        let wname = crate::sys::wide(name);
        let svc = OpenServiceW(mgr, wname.as_ptr(), access);
        if svc.is_null() {
            None
        } else {
            Some(svc)
        }
    }
}

fn query_state(svc: SC_HANDLE) -> Option<DWORD> {
    unsafe {
        let mut status: SERVICE_STATUS_PROCESS = std::mem::zeroed();
        let mut needed: DWORD = 0;
        let ok = QueryServiceStatusEx(
            svc,
            SC_STATUS_PROCESS_INFO,
            &mut status as *mut SERVICE_STATUS_PROCESS as *mut u8,
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as DWORD,
            &mut needed,
        );
        if ok == 0 {
            None
        } else {
            Some(status.dwCurrentState)
        }
    }
}

fn query_start_type(svc: SC_HANDLE) -> Option<DWORD> {
    unsafe {
        let mut needed: DWORD = 0;
        // Lần 1: lấy kích thước buffer cần thiết
        QueryServiceConfigW(svc, ptr::null_mut(), 0, &mut needed);
        if needed == 0 {
            return None;
        }
        let mut buf = vec![0u8; needed as usize];
        let cfg = buf.as_mut_ptr() as *mut winapi::um::winsvc::QUERY_SERVICE_CONFIGW;
        let ok = QueryServiceConfigW(svc, cfg, needed, &mut needed);
        if ok == 0 {
            None
        } else {
            Some((*cfg).dwStartType)
        }
    }
}

/// Truy vấn trạng thái + kiểu khởi động của một dịch vụ.
pub fn query(name: &str) -> ServiceInfo {
    let none = ServiceInfo { exists: false, state: 0, start_type: 0 };
    let mgr = match open_manager() {
        Some(m) => m,
        None => return none,
    };
    // SERVICE_QUERY_STATUS (0x4) | SERVICE_QUERY_CONFIG (0x1)
    let svc = match open_service(mgr, name, 0x0004 | 0x0001) {
        Some(s) => s,
        None => {
            unsafe { CloseServiceHandle(mgr) };
            return none;
        }
    };
    let info = ServiceInfo {
        exists: true,
        state: query_state(svc).unwrap_or(0),
        start_type: query_start_type(svc).unwrap_or(0),
    };
    unsafe {
        CloseServiceHandle(svc);
        CloseServiceHandle(mgr);
    }
    info
}

fn wait_for_state(svc: SC_HANDLE, want: DWORD, timeout: Duration) -> bool {
    let start = Instant::now();
    loop {
        match query_state(svc) {
            Some(s) if s == want => return true,
            Some(_) => {}
            None => return false,
        }
        if start.elapsed() > timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// Khởi động dịch vụ và chờ tới RUNNING (tối đa ~30 giây).
/// Dịch vụ đã chạy sẵn được coi là thành công.
pub fn start(name: &str) -> Result<(), String> {
    let mgr = open_manager().ok_or("Không mở được Service Control Manager")?;
    // SERVICE_START (0x10) | SERVICE_QUERY_STATUS (0x4)
    let svc = match open_service(mgr, name, 0x0010 | 0x0004) {
        Some(s) => s,
        None => {
            unsafe { CloseServiceHandle(mgr) };
            return Err(format!("Không mở được dịch vụ {}", name));
        }
    };
    let result = (|| {
        if query_state(svc) == Some(STATE_RUNNING) {
            return Ok(());
        }
        let ok = unsafe { StartServiceW(svc, 0, ptr::null_mut()) };
        if ok == 0 {
            let err = unsafe { winapi::um::errhandlingapi::GetLastError() };
            // 1056 = dịch vụ đã chạy; 1058 = dịch vụ bị vô hiệu hoá
            if err == 1056 {
                return Ok(());
            }
            return Err(format!("StartService {} lỗi {}", name, err));
        }
        if wait_for_state(svc, STATE_RUNNING, Duration::from_secs(30)) {
            Ok(())
        } else {
            Err(format!("Dịch vụ {} không lên được trạng thái Đang chạy", name))
        }
    })();
    unsafe {
        CloseServiceHandle(svc);
        CloseServiceHandle(mgr);
    }
    result
}

/// Dừng dịch vụ và chờ tới STOPPED (tối đa ~30 giây).
/// Dịch vụ đã dừng sẵn được coi là thành công.
pub fn stop(name: &str) -> Result<(), String> {
    let mgr = open_manager().ok_or("Không mở được Service Control Manager")?;
    // SERVICE_STOP (0x20) | SERVICE_QUERY_STATUS (0x4)
    let svc = match open_service(mgr, name, 0x0020 | 0x0004) {
        Some(s) => s,
        None => {
            unsafe { CloseServiceHandle(mgr) };
            return Err(format!("Không mở được dịch vụ {}", name));
        }
    };
    let result = (|| {
        if query_state(svc) == Some(STATE_STOPPED) {
            return Ok(());
        }
        let mut raw_status: winapi::um::winsvc::SERVICE_STATUS = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            ControlService(
                svc,
                1, // SERVICE_CONTROL_STOP
                &mut raw_status,
            )
        };
        if ok == 0 {
            let err = unsafe { winapi::um::errhandlingapi::GetLastError() };
            // 1062 = dịch vụ chưa được khởi động
            if err == 1062 {
                return Ok(());
            }
            return Err(format!("ControlService(stop) {} lỗi {}", name, err));
        }
        if wait_for_state(svc, STATE_STOPPED, Duration::from_secs(30)) {
            Ok(())
        } else {
            Err(format!("Dịch vụ {} không dừng hẳn được", name))
        }
    })();
    unsafe {
        CloseServiceHandle(svc);
        CloseServiceHandle(mgr);
    }
    result
}

/// Đổi kiểu khởi động (START_AUTO / START_DEMAND / START_DISABLED).
pub fn set_start_type(name: &str, start_type: DWORD) -> Result<(), String> {
    let mgr = open_manager().ok_or("Không mở được Service Control Manager")?;
    // SERVICE_CHANGE_CONFIG (0x2)
    let svc = match open_service(mgr, name, 0x0002) {
        Some(s) => s,
        None => {
            unsafe { CloseServiceHandle(mgr) };
            return Err(format!("Không mở được dịch vụ {}", name));
        }
    };
    let ok = unsafe {
        ChangeServiceConfigW(
            svc,
            SERVICE_NO_CHANGE,
            start_type,
            SERVICE_NO_CHANGE,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    let err = unsafe { winapi::um::errhandlingapi::GetLastError() };
    unsafe {
        CloseServiceHandle(svc);
        CloseServiceHandle(mgr);
    }
    if ok == 0 {
        Err(format!("Đổi kiểu khởi động {} lỗi {}", name, err))
    } else {
        Ok(())
    }
}
