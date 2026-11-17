//! Chạy lệnh ngoài (dism, pnputil, bcdedit, manage-bde...) với stdout/stderr
//! được thu lại và ghi vào log. Từ tiến trình GUI dùng CREATE_NO_WINDOW để
//! không nháy cửa sổ console.

use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use crate::log::Logger;

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct CmdOut {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

fn base_cmd(program: &str) -> Command {
    let mut c = Command::new(program);
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

/// Chạy lệnh, chờ kết thúc, trả toàn bộ output. Giải mã output theo
/// trang mã địa phương của Windows (thường là CP1258/CP1252 với tiếng Việt,
/// hoặc UTF-8 khi công cụ hỗ trợ) — dùng from_utf8_lossy để không bao giờ
/// panic vì output lạ.
pub fn run_capture(program: &str, args: &[&str]) -> std::io::Result<CmdOut> {
    let out = base_cmd(program).args(args).output()?;
    Ok(CmdOut {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// Chạy lệnh và ghi vào log: dòng lệnh, từng dòng stdout/stderr (khi chạy
/// xong), và mã thoát. Dùng cho các lệnh ngắn (bcdedit, manage-bde...).
pub fn run_logged(log: &Arc<Logger>, program: &str, args: &[&str]) -> std::io::Result<CmdOut> {
    log.line(&format!("$ {} {}", program, args.join(" ")));
    let out = run_capture(program, args)?;
    for l in out.stdout.lines() {
        if !l.trim().is_empty() {
            log.line(&format!("  | {}", l));
        }
    }
    for l in out.stderr.lines() {
        if !l.trim().is_empty() {
            log.line(&format!("  ! {}", l));
        }
    }
    log.line(&format!("  -> exit {:?}", out.code));
    Ok(out)
}

/// Chạy lệnh dài (DISM apply-image, pnputil export...) và chuyển từng dòng
/// output vào callback NGAY khi nó xuất hiện (phân tích tăng dần, cập nhật
/// tiến độ giao diện). Trả mã thoát khi tiến trình kết thúc.
pub fn run_streaming<F: FnMut(&str)>(program: &str, args: &[&str], mut on_line: F) -> std::io::Result<Option<i32>> {
    let mut child: Child = base_cmd(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // stderr đọc ở luồng riêng để tránh deadlock khi buffer đầy
    let stderr_handle = child.stderr.take().map(|err| {
        std::thread::spawn(move || {
            let reader = BufReader::new(err);
            let mut collected = Vec::new();
            for line in reader.lines() {
                match line {
                    Ok(l) => collected.push(l),
                    Err(_) => break,
                }
            }
            collected
        })
    });
    if let Some(out) = child.stdout.take() {
        let reader = BufReader::new(out);
        for line in reader.lines() {
            match line {
                Ok(l) => on_line(&l),
                Err(_) => break,
            }
        }
    }
    let status = child.wait()?;
    if let Some(h) = stderr_handle {
        if let Ok(lines) = h.join() {
            for l in lines {
                on_line(&l);
            }
        }
    }
    Ok(status.code())
}
