//! Engine thực thi catalogue: chạy các Op (registry gốc qua `reg.rs`,
//! dịch vụ qua `qsu_common::svc`, tác vụ theo lịch qua `schtasks`,
//! gói nguồn qua `powercfg`) và đánh giá Probe để biết trạng thái thật.
//!
//! Hai ngoại lệ dùng công cụ ngoài — đều là công cụ chuẩn của Windows
//! và được ghi rõ lý do:
//! - `schtasks`: Task Scheduler COM không có binding đủ dùng trong
//!   winapi 0.3; dò trạng thái bằng bản XML (`/Query /XML`) vì tên phần
//!   tử XML không bị bản địa hoá, khác với output dạng bảng.
//! - `powercfg`: không có Win32 API công khai để nhân bản/kích hoạt
//!   gói nguồn; GUID được trích bằng mẫu hex (không phụ thuộc ngôn ngữ
//!   của output). GUID của gói Ultimate Performance do chính module
//!   tạo được lưu ở file trạng thái cạnh exe để dò/hoàn tác chính xác.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use qsu_common::cmd;
use qsu_common::log::Logger;
use qsu_common::svc;

use crate::reg;
use crate::tweaks::{Op, Probe, Tweak, TweakState};

pub const ULTIMATE_TEMPLATE_GUID: &str = "e9a42b02-d5df-448d-aa00-03f14749eb61";
pub const BALANCED_GUID: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";

pub fn state_file(exe_dir: &Path) -> PathBuf {
    exe_dir.join("TuyChinhWindows_state.txt")
}

fn read_state(exe_dir: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(state_file(exe_dir)).ok()?;
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

fn write_state(exe_dir: &Path, key: &str, value: &str) {
    let path = state_file(exe_dir);
    let mut lines: Vec<String> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with(&format!("{}=", key)))
        .map(|l| l.to_string())
        .collect();
    lines.push(format!("{}={}", key, value));
    let _ = std::fs::write(&path, lines.join("\r\n") + "\r\n");
}

/// Tìm GUID đầu tiên trong văn bản (mẫu hex 8-4-4-4-12) — không phụ
/// thuộc ngôn ngữ của output powercfg.
fn find_guid(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let is_hex = |b: u8| b.is_ascii_hexdigit();
    let mut i = 0usize;
    while i + 36 <= bytes.len() {
        let w = &bytes[i..i + 36];
        if is_hex(w[0]) && w[8] == b'-' && w[13] == b'-' && w[18] == b'-' && w[23] == b'-' {
            let ok = w.iter().enumerate().all(|(j, &b)| matches!(j, 8 | 13 | 18 | 23) || is_hex(b));
            if ok {
                return Some(String::from_utf8_lossy(w).to_lowercase());
            }
        }
        i += 1;
    }
    None
}

fn active_scheme_guid() -> Option<String> {
    let out = cmd::run_capture("powercfg", &["/getactivescheme"]).ok()?;
    find_guid(&out.stdout)
}

fn ultimate_guid(exe_dir: &Path) -> Option<String> {
    if let Some(g) = read_state(exe_dir, "ultimate_guid") {
        return Some(g.to_lowercase());
    }
    // Trường hợp gói được tạo với đúng GUID mẫu (thường gặp khi nhân bản
    // gói ẩn của Windows): dò trong danh sách gói nguồn.
    let out = cmd::run_capture("powercfg", &["/list"]).ok()?;
    if out.stdout.to_lowercase().contains(ULTIMATE_TEMPLATE_GUID) {
        return Some(ULTIMATE_TEMPLATE_GUID.to_string());
    }
    None
}

fn op_ultimate(on: bool, exe_dir: &Path, logger: &Logger) -> Result<(), String> {
    if on {
        let guid = match ultimate_guid(exe_dir) {
            Some(g) => {
                logger.line(&format!("Gói Ultimate Performance đã tồn tại: {}", g));
                g
            }
            None => {
                logger.line("Nhân bản gói Ultimate Performance bằng powercfg /duplicatescheme...");
                let out = cmd::run_capture("powercfg", &["/duplicatescheme", ULTIMATE_TEMPLATE_GUID])
                    .map_err(|e| format!("không chạy được powercfg: {}", e))?;
                let g = find_guid(&out.stdout)
                    .ok_or_else(|| "không đọc được GUID gói nguồn mới từ output powercfg".to_string())?;
                logger.line(&format!("Đã tạo gói nguồn: {}", g));
                write_state(exe_dir, "ultimate_guid", &g);
                g
            }
        };
        let out = cmd::run_capture("powercfg", &["/setactive", &guid])
            .map_err(|e| format!("không chạy được powercfg: {}", e))?;
        if out.code == Some(0) {
            logger.line(&format!("Đã kích hoạt gói nguồn {}", guid));
            Ok(())
        } else {
            Err(format!("powercfg /setactive trả mã {:?}", out.code))
        }
    } else {
        let out = cmd::run_capture("powercfg", &["/setactive", BALANCED_GUID])
            .map_err(|e| format!("không chạy được powercfg: {}", e))?;
        if out.code == Some(0) {
            logger.line("Đã chuyển gói nguồn về Balanced (mặc định Windows)");
            Ok(())
        } else {
            Err(format!("powercfg /setactive Balanced trả mã {:?}", out.code))
        }
    }
}

fn op_task(path: &str, enable: bool, logger: &Logger) -> Result<(), String> {
    let flag = if enable { "/ENABLE" } else { "/DISABLE" };
    let out = cmd::run_capture("schtasks", &["/Change", "/TN", path, flag])
        .map_err(|e| format!("không chạy được schtasks: {}", e))?;
    if out.code == Some(0) {
        logger.line(&format!("Tác vụ {} -> {}", path, if enable { "Đã bật" } else { "Đã tắt" }));
        Ok(())
    } else {
        let detail = out.stderr.trim().to_string();
        Err(format!("schtasks {} {} lỗi (mã {:?}) {}", flag, path, out.code, detail))
    }
}

fn task_is_disabled(path: &str) -> Option<bool> {
    let out = cmd::run_capture("schtasks", &["/Query", "/TN", path, "/XML"]).ok()?;
    if out.code != Some(0) {
        return None; // tác vụ không tồn tại trên bản Windows này
    }
    // XML định nghĩa tác vụ: <Enabled>false</Enabled> chỉ xuất hiện khi
    // tác vụ bị tắt; bật thì phần tử bị lược bỏ hoặc là true.
    Some(out.stdout.contains("<Enabled>false</Enabled>"))
}

/// Thực thi một Op; ghi log chi tiết; Err = mô tả lỗi.
pub fn run_op(op: &Op, exe_dir: &Path, logger: &Logger) -> Result<(), String> {
    match op {
        Op::Dword { root, path, name, value } => {
            reg::set_dword(*root, path, name, *value)?;
            logger.line(&format!("Đặt {} {}\\{} = {} (DWORD)", root.label(), path, name, value));
            Ok(())
        }
        Op::Sz { root, path, name, value } => {
            reg::set_sz(*root, path, name, value)?;
            logger.line(&format!("Đặt {} {}\\{} = \"{}\" (Chuỗi)", root.label(), path, name, value));
            Ok(())
        }
        Op::DelValue { root, path, name } => {
            reg::delete_value(*root, path, name)?;
            logger.line(&format!("Xoá giá trị {} {}\\{}", root.label(), path, name));
            Ok(())
        }
        Op::DefaultSz { root, path, value } => {
            reg::set_default_sz(*root, path, value)?;
            logger.line(&format!("Đặt (Default) của {} {} = \"{}\"", root.label(), path, value));
            Ok(())
        }
        Op::DelTree { root, path } => {
            reg::delete_tree(*root, path)?;
            logger.line(&format!("Xoá cây khoá {} {}", root.label(), path));
            Ok(())
        }
        Op::ServiceStart { name, start, stop_now, start_now } => {
            svc::set_start_type(name, *start)
                .map_err(|e| format!("đổi kiểu khởi động {}: {}", name, e))?;
            logger.line(&format!("Dịch vụ {}: kiểu khởi động = {}", name, svc::start_type_label(*start)));
            if *stop_now {
                match svc::stop(name) {
                    Ok(()) => logger.line(&format!("Dịch vụ {}: đã dừng", name)),
                    Err(e) => logger.line(&format!("Dịch vụ {}: {} (bỏ qua)", name, e)),
                }
            }
            if *start_now {
                match svc::start(name) {
                    Ok(()) => logger.line(&format!("Dịch vụ {}: đã chạy", name)),
                    Err(e) => logger.line(&format!("Dịch vụ {}: {} (bỏ qua)", name, e)),
                }
            }
            Ok(())
        }
        Op::TaskEnabled { path, enable } => op_task(path, *enable, logger),
        Op::UltimatePower(on) => op_ultimate(*on, exe_dir, logger),
    }
}

/// Chạy một danh sách Op (áp dụng hoặc hoàn tác của một tweak).
/// Trả (số op thành công, số op lỗi, lỗi cuối nếu có).
pub fn run_ops(ops: &[Op], exe_dir: &Path, logger: &Logger) -> (usize, usize, Option<String>) {
    let mut ok = 0usize;
    let mut bad = 0usize;
    let mut last_err: Option<String> = None;
    for op in ops {
        match run_op(op, exe_dir, logger) {
            Ok(()) => ok += 1,
            Err(e) => {
                bad += 1;
                logger.line(&format!("  LỖI: {}", e));
                last_err = Some(e);
            }
        }
    }
    (ok, bad, last_err)
}

/// Đánh giá trạng thái hiện tại của một tweak từ Probe.
pub fn probe(tweak: &Tweak, exe_dir: &Path) -> TweakState {
    match &tweak.probe {
        Probe::Dword { root, path, name, value, missing_applied } => match reg::get_dword(*root, path, name) {
            Some(v) => {
                if v == *value {
                    TweakState::Applied
                } else {
                    TweakState::NotApplied
                }
            }
            None => {
                if *missing_applied {
                    TweakState::Applied
                } else {
                    TweakState::NotApplied
                }
            }
        },
        Probe::Sz { root, path, name, value } => match reg::get_sz(*root, path, name) {
            Some(v) => {
                if v == *value {
                    TweakState::Applied
                } else {
                    TweakState::NotApplied
                }
            }
            None => TweakState::NotApplied,
        },
        Probe::KeyExists { root, path } => {
            if reg::key_exists(*root, path) {
                TweakState::Applied
            } else {
                TweakState::NotApplied
            }
        }
        Probe::ServiceStart { name, start } => {
            let info = svc::query(name);
            if !info.exists {
                TweakState::Unknown
            } else if info.start_type == *start {
                TweakState::Applied
            } else {
                TweakState::NotApplied
            }
        }
        Probe::TaskDisabled { path } => match task_is_disabled(path) {
            Some(true) => TweakState::Applied,
            Some(false) => TweakState::NotApplied,
            None => TweakState::Unknown,
        },
        Probe::UltimatePower => match (active_scheme_guid(), ultimate_guid(exe_dir)) {
            (Some(active), Some(ult)) => {
                if active == ult {
                    TweakState::Applied
                } else {
                    TweakState::NotApplied
                }
            }
            (Some(active), None) => {
                if active == ULTIMATE_TEMPLATE_GUID {
                    TweakState::Applied
                } else {
                    TweakState::NotApplied
                }
            }
            _ => TweakState::Unknown,
        },
    }
}

/// Tạo điểm khôi phục hệ thống (System Restore).
///
/// Không có Win32 API công khai cho việc này — đường hỗ trợ chính thức
/// là PowerShell `Checkpoint-Computer` (cùng ngoại lệ có ghi chép như
/// module Tắt BitLocker dùng Get-BitLockerVolume). Trước đó đặt
/// SystemRestorePointCreationFrequency = 0 bằng Registry API gốc để
/// không bị giới hạn 24 giờ giữa hai điểm khôi phục.
pub fn create_restore_point(logger: &Arc<Logger>) -> Result<(), String> {
    reg::set_dword(
        reg::Root::Hklm,
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\SystemRestore",
        "SystemRestorePointCreationFrequency",
        0,
    )
    .map_err(|e| format!("đặt tần suất điểm khôi phục: {}", e))?;
    logger.line("Đã cho phép tạo điểm khôi phục không giới hạn 24 giờ (SystemRestorePointCreationFrequency = 0)");

    let script = "Enable-ComputerRestore -Drive \"$env:SystemDrive\\\" -ErrorAction SilentlyContinue; \
                  Checkpoint-Computer -Description \"Tuy Chinh Windows - truoc khi ap dung tweak\" -RestorePointType \"MODIFY_SETTINGS\"";
    logger.line("Đang tạo điểm khôi phục (Checkpoint-Computer)...");
    let out = cmd::run_capture(
        "powershell.exe",
        &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script],
    )
    .map_err(|e| format!("không chạy được powershell: {}", e))?;
    for l in out.stdout.lines().chain(out.stderr.lines()) {
        if !l.trim().is_empty() {
            logger.line(&format!("  | {}", l));
        }
    }
    if out.code == Some(0) {
        logger.line("Đã tạo điểm khôi phục hệ thống");
        Ok(())
    } else {
        Err(format!(
            "Checkpoint-Computer trả mã {:?} — kiểm tra System Restore đã bật cho ổ hệ thống chưa",
            out.code
        ))
    }
}
