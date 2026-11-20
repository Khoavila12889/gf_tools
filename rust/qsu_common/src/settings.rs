//! Cấu hình module (settings.json đặt cạnh file .exe của module).
//! Mọi giá trị có mặc định hợp lý; người dùng ghi đè bằng settings.json
//! trên hạ tầng của mình — không hard-code server của bất kỳ ai.
//!
//! Phần `postInstallApps` là danh sách ứng dụng cài im lặng ở giai đoạn
//! hậu cài (mặc định: UltraViewer + Google Chrome). Mật khẩu KHÔNG nằm
//! trong settings mẫu; mật khẩu UltraViewer/AutoLogon lấy từ ô nhập của
//! wizard (hoặc khóa `ultraviewerPassword` trong settings.json thật của
//! người dùng) và không bao giờ được ghi vào log.

use std::path::{Path, PathBuf};

use crate::json::{self, Json};

#[derive(Debug, Clone)]
pub struct AppEntry {
    pub name: String,
    /// Tên file khi lưu xuống đĩa (ví dụ UltraViewer_setup.exe)
    pub file_name: String,
    pub url: String,
    /// SHA-256 kỳ vọng (hex). Rỗng = bỏ qua kiểm hash (có cảnh báo trong log).
    pub sha256: String,
    /// "exe" chạy trực tiếp với args; "msi" chạy qua msiexec /i
    pub kind: String,
    pub args: Vec<String>,
    /// Dịch vụ Windows cần đặt Automatic + khởi động sau cài (nếu có)
    pub service_name: String,
    pub selected_by_default: bool,
}

impl AppEntry {
    pub fn to_json(&self) -> Json {
        json::obj(vec![
            ("name", json::s(&self.name)),
            ("fileName", json::s(&self.file_name)),
            ("url", json::s(&self.url)),
            ("sha256", json::s(&self.sha256)),
            ("kind", json::s(&self.kind)),
            ("args", Json::Arr(self.args.iter().map(|a| json::s(a)).collect())),
            ("serviceName", json::s(&self.service_name)),
            ("selectedByDefault", json::b(self.selected_by_default)),
        ])
    }
    pub fn from_json(v: &Json) -> Option<AppEntry> {
        let args = v
            .get("args")
            .and_then(|a| a.as_arr())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        Some(AppEntry {
            name: v.str_or("name", ""),
            file_name: v.str_or("fileName", ""),
            url: v.str_or("url", ""),
            sha256: v.str_or("sha256", ""),
            kind: v.str_or("kind", "exe"),
            args,
            service_name: v.str_or("serviceName", ""),
            selected_by_default: v.bool_or("selectedByDefault", true),
        })
    }
}

/// Hai ứng dụng mặc định của giai đoạn hậu cài. URL là địa chỉ tải chính
/// thức của nhà phát hành; sha256 để trống trong bản mẫu — người dùng điền
/// hash của đúng phiên bản mình phê duyệt vào settings.json (wizard/post
/// install sẽ cảnh báo nếu để trống).
pub fn default_apps() -> Vec<AppEntry> {
    vec![
        AppEntry {
            name: "UltraViewer".to_string(),
            file_name: "UltraViewer_setup.exe".to_string(),
            url: "https://download.ultraviewer.net/UltraViewer_setup_6.6_en.exe".to_string(),
            sha256: String::new(),
            kind: "exe".to_string(),
            // Trình cài UltraViewer (Inno Setup): cài im lặng hoàn toàn
            args: vec![
                "/VERYSILENT".to_string(),
                "/SUPPRESSMSGBOXES".to_string(),
                "/NORESTART".to_string(),
            ],
            service_name: "UltraViewer_Service".to_string(),
            selected_by_default: true,
        },
        AppEntry {
            name: "Google Chrome".to_string(),
            file_name: "GoogleChromeStandaloneEnterprise64.msi".to_string(),
            url: "https://dl.google.com/dl/chrome/install/googlechromestandaloneenterprise64.msi"
                .to_string(),
            sha256: String::new(),
            kind: "msi".to_string(),
            args: vec!["/quiet".to_string(), "/norestart".to_string()],
            service_name: String::new(),
            selected_by_default: true,
        },
    ]
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub catalogue_url: String,
    pub locale: String,
    pub ui_language: String,
    pub input_locale: String,
    pub timezone: String,
    pub download_folder: String,
    pub logs_folder: String,
    pub staging_folder: String,
    pub min_free_gb_inplace: u64,
    pub min_free_gb_clean: u64,
    pub winpe_wim_path: String,
    pub storage_drivers_path: String,
    pub default_apply_tweaks: bool,
    /// Mặc định hồ sơ CORE cho ảnh rút gọn Tiny11 Core (v0.7.24):
    /// wizard khởi tạo nút "Ảnh rút gọn (Tiny11 Core)" từ khoá này
    /// và TỰ GHI LẠI mỗi khi kỹ thuật viên bật/tắt nút — hai ca
    /// test v0.7.21/v0.7.22 đã chạy nhầm hồ sơ STANDARD vì nút
    /// mặc định TẮT và không được bật trong ca chạy.
    pub default_core_profile: bool,
    /// Mật khẩu unattended của UltraViewer đặt sẵn trong settings (wizard
    /// có ô nhập riêng, giá trị wizard nhập thắng). Không ghi log.
    pub ultraviewer_password: String,
    pub post_install_apps: Vec<AppEntry>,
    /// Đường dẫn file settings.json đã nạp (nếu có)
    pub loaded_from: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            catalogue_url: String::new(),
            locale: "vi-VN".to_string(),
            ui_language: "vi-VN".to_string(),
            input_locale: "vi-VN".to_string(),
            timezone: "SE Asia Standard Time".to_string(),
            download_folder: "QSU-Downloads".to_string(),
            logs_folder: "QSU-Logs".to_string(),
            staging_folder: "QSU-Stage".to_string(),
            min_free_gb_inplace: 25,
            min_free_gb_clean: 12,
            winpe_wim_path: String::new(),
            storage_drivers_path: String::new(),
            default_apply_tweaks: false,
            default_core_profile: false,
            ultraviewer_password: String::new(),
            post_install_apps: default_apps(),
            loaded_from: None,
        }
    }
}

/// Tên thư mục con hợp lệ trên ổ Data: không rỗng, không chứa ký tự
/// phân tách đường dẫn / ổ đĩa, không phải "." hoặc "..". Các trường
/// thư mục trong settings.json (download/logs/staging) bắt buộc qua
/// kiểm tra này — đặc biệt `stagingFolder` được wizard dùng để XOÁ
/// đệ quy thư mục staging cũ: giá trị rỗng/".." sẽ trỏ tới gốc ổ Data
/// hoặc thoát ra ngoài, cực kỳ nguy hiểm.
fn valid_folder_name(v: &str) -> bool {
    let t = v.trim();
    !t.is_empty()
        && t != "."
        && t != ".."
        && !t.contains('\\')
        && !t.contains('/')
        && !t.contains(':')
}

impl Settings {
    /// Nạp settings.json trong thư mục cho trước (thường là cạnh file exe).
    /// Thiếu file hoặc lỗi parse → dùng mặc định (kèm thông báo lỗi nếu có).
    pub fn load(dir: &Path) -> (Settings, Option<String>) {
        let path = dir.join("settings.json");
        let mut st = Settings::default();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return (st, None),
        };
        let root = match json::parse(&text) {
            Ok(v) => v,
            Err(e) => return (st, Some(format!("settings.json lỗi cú pháp: {}", e))),
        };
        st.catalogue_url = root.str_or("catalogueUrl", &st.catalogue_url);
        st.locale = root.str_or("locale", &st.locale);
        st.ui_language = root.str_or("uiLanguage", &st.ui_language);
        st.input_locale = root.str_or("inputLocale", &st.input_locale);
        st.timezone = root.str_or("timezone", &st.timezone);
        st.download_folder = root.str_or("downloadFolder", &st.download_folder);
        st.logs_folder = root.str_or("logsFolder", &st.logs_folder);
        st.staging_folder = root.str_or("stagingFolder", &st.staging_folder);
        // Tên thư mục không hợp lệ (rỗng, chứa đường dẫn...) -> về mặc định
        if !valid_folder_name(&st.download_folder) {
            st.download_folder = Settings::default().download_folder;
        }
        if !valid_folder_name(&st.logs_folder) {
            st.logs_folder = Settings::default().logs_folder;
        }
        if !valid_folder_name(&st.staging_folder) {
            st.staging_folder = Settings::default().staging_folder;
        }
        st.min_free_gb_inplace = root.u64_or("minFreeGbInPlace", st.min_free_gb_inplace);
        st.min_free_gb_clean = root.u64_or("minFreeGbClean", st.min_free_gb_clean);
        st.winpe_wim_path = root.str_or("winpeWimPath", &st.winpe_wim_path);
        st.storage_drivers_path = root.str_or("storageDriversPath", &st.storage_drivers_path);
        st.default_apply_tweaks = root.bool_or("defaultApplyTweaks", st.default_apply_tweaks);
        st.default_core_profile = root.bool_or("defaultCoreProfile", st.default_core_profile);
        st.ultraviewer_password = root.str_or("ultraviewerPassword", "");
        if let Some(apps) = root.get("postInstallApps").and_then(|a| a.as_arr()) {
            let parsed: Vec<AppEntry> = apps.iter().filter_map(AppEntry::from_json).collect();
            if !parsed.is_empty() {
                st.post_install_apps = parsed;
            }
        }
        st.loaded_from = Some(path);
        (st, None)
    }
}
