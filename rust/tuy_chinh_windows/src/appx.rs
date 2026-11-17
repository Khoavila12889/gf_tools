//! Gỡ ứng dụng rác (Appx) — danh sách tuyển chọn các gói Store an toàn
//! để gỡ cho NGƯỜI DÙNG HIỆN TẠI.
//!
//! Nguyên tắc an toàn:
//! - Chỉ liệt kê các gói nằm trong danh sách tuyển chọn dưới đây VÀ
//!   đang thật sự cài trên máy; không đụng gói framework/hệ thống.
//! - Chỉ gỡ bản cài của người dùng hiện tại bằng `Remove-AppxPackage`.
//!   CHỦ ĐÍCH không gỡ gói provisioned (Get-AppxProvisionedPackage) —
//!   gói provisioned sẽ tự cài lại cho người dùng mới; muốn chặn hẳn
//!   phải là quyết định riêng của kỹ thuật viên, ngoài phạm vi v1.
//! - Gỡ rồi vẫn cài lại được từ Microsoft Store.
//!
//! Ngoại lệ PowerShell (có ghi chép): Windows không có Win32 API công
//! khai để liệt kê/gỡ gói Appx của người dùng hiện tại (API chính thức
//! là WinRT Windows.Management.Deployment hoặc cmdlet Appx). Module
//! dùng `powershell.exe` với lệnh ghép sẵn (không có dữ liệu người
//! dùng trong dòng lệnh ngoài tên gói đã kiểm chứng theo danh sách).

use std::sync::Arc;

use qsu_common::cmd;
use qsu_common::log::Logger;

use crate::tweaks::Risk;

#[derive(Clone)]
pub struct AppxInfo {
    pub name: String,        // tên gói, ví dụ Microsoft.BingNews
    pub display: String,     // tên hiển thị tiếng Việt
    pub version: String,
    pub full_name: String,   // PackageFullName dùng khi gỡ
    pub risk: Risk,
}

/// Danh sách tuyển chọn: (tên gói, tên hiển thị, mức rủi ro).
/// "Cần cân nhắc" = ứng dụng có thể có người đang dùng thật
/// (Điện thoại, Xbox, Mail...) — mặc định không tick.
pub const CURATED: &[(&str, &str, Risk)] = &[
    ("Microsoft.BingNews", "Tin tức Bing (Microsoft News)", Risk::Safe),
    ("Microsoft.BingWeather", "Thời tiết Bing", Risk::Safe),
    ("Microsoft.GetHelp", "Nhận trợ giúp (Get Help)", Risk::Safe),
    ("Microsoft.Getstarted", "Bắt đầu (Tips/Get Started)", Risk::Safe),
    ("Microsoft.MicrosoftSolitaireCollection", "Bộ bài Solitaire", Risk::Safe),
    ("Microsoft.People", "Danh bạ (People)", Risk::Safe),
    ("Microsoft.Todos", "Microsoft To Do", Risk::Safe),
    ("Microsoft.WindowsFeedbackHub", "Trung tâm phản hồi (Feedback Hub)", Risk::Safe),
    ("Microsoft.ZuneMusic", "Groove Music / Media Player (ZuneMusic)", Risk::Safe),
    ("Microsoft.ZuneVideo", "Phim & TV (Movies & TV)", Risk::Safe),
    ("Microsoft.WindowsMaps", "Bản đồ (Maps)", Risk::Safe),
    ("Microsoft.MicrosoftOfficeHub", "Ứng dụng Office (hub)", Risk::Safe),
    ("Microsoft.SkypeApp", "Skype (bản Store)", Risk::Safe),
    ("Microsoft.MixedReality.Portal", "Cổng Mixed Reality", Risk::Safe),
    ("Microsoft.OneConnect", "Gói dữ liệu di động (Mobile Plans)", Risk::Safe),
    ("Microsoft.Print3D", "Print 3D", Risk::Safe),
    ("Microsoft.Wallet", "Ví Microsoft (Wallet)", Risk::Safe),
    ("Microsoft.WindowsSoundRecorder", "Ghi âm (Voice Recorder)", Risk::Safe),
    ("Clipchamp.Clipchamp", "Clipchamp (dựng video)", Risk::Safe),
    ("Microsoft.YourPhone", "Liên kết điện thoại (Phone Link)", Risk::Caution),
    ("MicrosoftTeams", "Microsoft Teams (cá nhân)", Risk::Caution),
    ("Microsoft.WindowsCommunicationsApps", "Thư & Lịch (Mail and Calendar)", Risk::Caution),
    ("Microsoft.XboxApp", "Ứng dụng Xbox", Risk::Caution),
    ("Microsoft.XboxGamingOverlay", "Xbox Game Bar (lớp phủ)", Risk::Caution),
    ("Microsoft.XboxGameOverlay", "Xbox Game Overlay", Risk::Caution),
    ("Microsoft.GamingApp", "Gaming App (Xbox)", Risk::Caution),
    ("Microsoft.Office.OneNote", "OneNote (bản Store)", Risk::Caution),
];

fn curated(name: &str) -> Option<&'static (&'static str, &'static str, Risk)> {
    CURATED.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name))
}

/// Liệt kê các gói trong danh sách tuyển chọn đang cài cho người dùng
/// hiện tại. Output của PowerShell được ép dạng "Name|FullName|Version"
/// mỗi dòng một gói để phân tích không phụ thuộc ngôn ngữ.
pub fn list_installed(logger: &Arc<Logger>) -> Result<Vec<AppxInfo>, String> {
    let script = "Get-AppxPackage | ForEach-Object { \"$($_.Name)|$($_.PackageFullName)|$($_.Version)\" }";
    let out = cmd::run_capture(
        "powershell.exe",
        &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script],
    )
    .map_err(|e| format!("không chạy được powershell: {}", e))?;
    if out.code != Some(0) {
        return Err(format!("Get-AppxPackage trả mã {:?}", out.code));
    }
    let mut found = Vec::new();
    for line in out.stdout.lines() {
        let parts: Vec<&str> = line.trim().split('|').collect();
        if parts.len() != 3 {
            continue;
        }
        if let Some(entry) = curated(parts[0]) {
            found.push(AppxInfo {
                name: parts[0].to_string(),
                display: entry.1.to_string(),
                version: parts[2].to_string(),
                full_name: parts[1].to_string(),
                risk: entry.2,
            });
        }
    }
    // Sắp xếp: an toàn trước, rồi theo tên hiển thị
    found.sort_by(|a, b| {
        let ra = if a.risk == Risk::Safe { 0 } else { 1 };
        let rb = if b.risk == Risk::Safe { 0 } else { 1 };
        ra.cmp(&rb).then_with(|| a.display.cmp(&b.display))
    });
    logger.line(&format!("Tìm thấy {} gói Appx trong danh sách gỡ an toàn", found.len()));
    Ok(found)
}

/// Gỡ một gói cho người dùng hiện tại.
pub fn remove(pkg: &AppxInfo, logger: &Arc<Logger>) -> Result<(), String> {
    // Chỉ chấp nhận PackageFullName thuộc danh sách đã liệt kê (chữ/số/
    // dấu chấm/gạch dưới/gạch ngang) — không nội suy dữ liệu lạ vào lệnh.
    if !pkg.full_name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
        return Err(format!("PackageFullName không hợp lệ: {}", pkg.full_name));
    }
    let script = format!("Remove-AppxPackage -Package \"{}\" -ErrorAction Stop", pkg.full_name);
    logger.line(&format!("Gỡ gói {} ({})...", pkg.display, pkg.name));
    let out = cmd::run_capture(
        "powershell.exe",
        &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script],
    )
    .map_err(|e| format!("không chạy được powershell: {}", e))?;
    for l in out.stderr.lines() {
        if !l.trim().is_empty() {
            logger.line(&format!("  ! {}", l));
        }
    }
    if out.code == Some(0) {
        logger.line(&format!("Đã gỡ {}", pkg.display));
        Ok(())
    } else {
        Err(format!("gỡ {} thất bại (mã {:?})", pkg.name, out.code))
    }
}
