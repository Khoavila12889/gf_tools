//! Logger dùng chung: ghi file có timestamp + đẩy dòng log qua callback
//! (để giao diện wizard hiển thị trực tiếp). Mọi lệnh ngoài đều được ghi
//! kèm stdout/stderr vào cùng file log này (đặc tả §8).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Logger {
    file: Mutex<Option<std::fs::File>>,
    path: PathBuf,
    sink: Option<Box<dyn Fn(&str) + Send + Sync>>,
}

pub fn now_string() -> String {
    unsafe {
        let mut st: winapi::um::minwinbase::SYSTEMTIME = std::mem::zeroed();
        winapi::um::sysinfoapi::GetLocalTime(&mut st);
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
        )
    }
}

/// Tên file log theo mẫu đặc tả: install-<yyyyMMdd-HHmm>.log
pub fn log_stamp() -> String {
    unsafe {
        let mut st: winapi::um::minwinbase::SYSTEMTIME = std::mem::zeroed();
        winapi::um::sysinfoapi::GetLocalTime(&mut st);
        format!(
            "{:04}{:02}{:02}-{:02}{:02}",
            st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute
        )
    }
}

impl Logger {
    pub fn new(path: PathBuf, sink: Option<Box<dyn Fn(&str) + Send + Sync>>) -> Logger {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = OpenOptions::new().create(true).append(true).open(&path).ok();
        Logger { file: Mutex::new(file), path, sink }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn line(&self, msg: &str) {
        let full = format!("[{}] {}", now_string(), msg);
        if let Ok(mut guard) = self.file.lock() {
            if let Some(f) = guard.as_mut() {
                let _ = writeln!(f, "{}", full);
                let _ = f.flush();
            }
        }
        if let Some(sink) = &self.sink {
            sink(&full);
        }
    }
}
