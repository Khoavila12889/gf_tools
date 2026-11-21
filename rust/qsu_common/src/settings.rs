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
    /// Thông báo trạng thái cài đặt (v0.7.27): tab "Thông báo"
    /// của wizard đọc/ghi các khoá này. Nội dung tin chỉ gồm
    /// trạng thái + tên máy (xem `notify`); token/secret dưới đây
    /// chỉ nằm trong settings.json cạnh wizard — KHÔNG vào
    /// stage.json, KHÔNG vào log.
    pub notify_enabled: bool,
    /// Bot Token của bot Telegram của chính kỹ thuật viên.
    pub telegram_bot_token: String,
    pub telegram_chat_id: String,
    /// Relay Zalo Golden Farm trên server của đơn vị: địa chỉ
    /// gốc (vd http://<server>:<port>) và khoá bí mật dài hạn.
    pub zalo_relay_url: String,
    pub zalo_relay_secret: String,
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
            notify_enabled: false,
            telegram_bot_token: String::new(),
            telegram_chat_id: String::new(),
            zalo_relay_url: String::new(),
            zalo_relay_secret: String::new(),
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
        st.notify_enabled = root.bool_or("notifyEnabled", st.notify_enabled);
        st.telegram_bot_token = root.str_or("telegramBotToken", "");
        st.telegram_chat_id = root.str_or("telegramChatId", "");
        st.zalo_relay_url = root.str_or("zaloRelayUrl", "");
        st.zalo_relay_secret = root.str_or("zaloRelaySecret", "");
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

/// Độ dài (byte) của token giá trị JSON bắt đầu tại đầu `s` (đã
/// bỏ khoảng trắng dẫn đầu): chuỗi (quét tới dấu `"` đóng, có
/// xét escape), đối tượng/mảng (đếm ngoặc cân bằng, bỏ qua ngoặc
/// nằm trong chuỗi), hoặc literal true/false/null/số (quét tới
/// ký tự phân cách). None nếu không nhận diện được.
fn json_value_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    match b[0] {
        b'"' => {
            let mut i = 1;
            while i < b.len() {
                match b[i] {
                    b'\\' => i += 2,
                    b'"' => return Some(i + 1),
                    _ => i += 1,
                }
            }
            None
        }
        b'{' | b'[' => {
            let mut depth = 0i32;
            let mut i = 0;
            let mut in_str = false;
            while i < b.len() {
                let c = b[i];
                if in_str {
                    match c {
                        b'\\' => i += 1,
                        b'"' => in_str = false,
                        _ => {}
                    }
                } else {
                    match c {
                        b'"' => in_str = true,
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => {
                            depth -= 1;
                            if depth == 0 {
                                return Some(i + 1);
                            }
                        }
                        _ => {}
                    }
                }
                i += 1;
            }
            None
        }
        _ => {
            let mut i = 0;
            while i < b.len()
                && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'-' | b'+' | b'.'))
            {
                i += 1;
            }
            if i > 0 {
                Some(i)
            } else {
                None
            }
        }
    }
}

/// Đặt một khoá trong văn bản settings.json về giá trị mới
/// (`raw_value` là literal JSON đã serialize sẵn) theo phong
/// cách sửa-tại-vị-trí của `persist_core_default` bên wizard:
/// có khoá thì thay đúng token giá trị của nó, chưa có thì chèn
/// ngay sau dấu `{` đầu tiên, giữ nguyên mọi khoá và định dạng
/// còn lại. Không nhận diện được vị trí giá trị cũ thì giữ
/// nguyên văn bản (không phá file).
fn set_json_key(text: &str, key: &str, raw_value: &str) -> String {
    let needle = format!("\"{}\"", key);
    if let Some(kp) = text.find(&needle) {
        let after = &text[kp + needle.len()..];
        if let Some(cp) = after.find(':') {
            let rest = &after[cp + 1..];
            let skip = rest.len() - rest.trim_start().len();
            let value_start = kp + needle.len() + cp + 1 + skip;
            if let Some(old_len) = json_value_len(&text[value_start..]) {
                return format!(
                    "{}{}{}",
                    &text[..value_start],
                    raw_value,
                    &text[value_start + old_len..]
                );
            }
        }
        return text.to_string();
    }
    match text.find('{') {
        Some(bp) => format!(
            "{}{{ \"{}\": {},{}",
            &text[..bp],
            key,
            raw_value,
            &text[bp + 1..]
        ),
        None => format!("{{\n  \"{}\": {}\n}}\n", key, raw_value),
    }
}

/// Ghi lại 5 khoá thông báo (v0.7.27) vào settings.json cạnh
/// file exe của wizard — tab "Thông báo" gọi khi bấm "Lưu cấu
/// hình thông báo". Chỉ sửa đúng 5 khoá này trong văn bản file,
/// giữ nguyên mọi khoá khác (kể cả các khoá phiên bản sau chưa
/// biết); chưa có file thì tạo file chỉ chứa các khoá này
/// (Settings::load tự bù mặc định cho phần còn lại).
pub fn persist_notify_keys(
    exe_dir: &Path,
    notify_enabled: bool,
    telegram_bot_token: &str,
    telegram_chat_id: &str,
    zalo_relay_url: &str,
    zalo_relay_secret: &str,
) -> std::io::Result<()> {
    let path = exe_dir.join("settings.json");
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    text = set_json_key(
        &text,
        "notifyEnabled",
        if notify_enabled { "true" } else { "false" },
    );
    text = set_json_key(
        &text,
        "telegramBotToken",
        &json::stringify(&json::s(telegram_bot_token)),
    );
    text = set_json_key(
        &text,
        "telegramChatId",
        &json::stringify(&json::s(telegram_chat_id)),
    );
    text = set_json_key(
        &text,
        "zaloRelayUrl",
        &json::stringify(&json::s(zalo_relay_url)),
    );
    text = set_json_key(
        &text,
        "zaloRelaySecret",
        &json::stringify(&json::s(zalo_relay_secret)),
    );
    std::fs::write(&path, text)
}
