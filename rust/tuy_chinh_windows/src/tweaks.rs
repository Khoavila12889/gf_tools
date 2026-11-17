//! Danh mục tinh chỉnh (catalogue) dạng DỮ LIỆU — nguồn nội dung biên
//! soạn theo catalogue của **Sophia Script for Windows** (farag2, MIT —
//! xem ghi công trong cửa sổ Giới thiệu) và đối chiếu chéo với winutil
//! (ChrisTitusTech, MIT). Mô tả tiếng Việt do dự án tự viết.
//!
//! Chuẩn chất lượng áp dụng (theo Sophia Script): mỗi tweak có thao tác
//! ÁP DỤNG và thao tác HOÀN TÁC về mặc định Windows, kèm bộ dò trạng
//! thái (Probe) đọc trạng thái thật của hệ thống để giao diện hiện
//! "Đã áp dụng / Chưa áp dụng / Không rõ".

use crate::reg::Root;
use qsu_common::svc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Category {
    Privacy,
    Ui,
    Perf,
    System,
    Features,
    Appx,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Privacy => "Quyền riêng tư & Telemetry",
            Category::Ui => "Giao diện & Explorer",
            Category::Perf => "Hiệu năng",
            Category::System => "Hệ thống",
            Category::Features => "Menu ngữ cảnh & Tính năng",
            Category::Appx => "Gỡ ứng dụng rác (Appx)",
        }
    }
    /// Các nhóm có tweak tĩnh (Appx là danh sách động, xử lý riêng).
    pub fn tweak_categories() -> [Category; 5] {
        [Category::Privacy, Category::Ui, Category::Perf, Category::System, Category::Features]
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Risk {
    Safe,
    Caution,
}

impl Risk {
    pub fn label(self) -> &'static str {
        match self {
            Risk::Safe => "An toàn",
            Risk::Caution => "Cần cân nhắc",
        }
    }
}

/// Một thao tác nguyên tử khi áp dụng/hoàn tác một tweak.
#[derive(Debug, Clone, Copy)]
pub enum Op {
    Dword { root: Root, path: &'static str, name: &'static str, value: u32 },
    Sz { root: Root, path: &'static str, name: &'static str, value: &'static str },
    DelValue { root: Root, path: &'static str, name: &'static str },
    /// Đặt giá trị (Default) của khoá.
    DefaultSz { root: Root, path: &'static str, value: &'static str },
    DelTree { root: Root, path: &'static str },
    ServiceStart { name: &'static str, start: u32, stop_now: bool, start_now: bool },
    /// Bật/tắt tác vụ theo lịch bằng schtasks (công cụ chuẩn của Windows;
    /// Task Scheduler COM không có binding sẵn trong winapi 0.3 đủ dùng).
    TaskEnabled { path: &'static str, enable: bool },
    /// true = kích hoạt gói nguồn Ultimate Performance; false = về Balanced.
    UltimatePower(bool),
}

/// Cách đọc trạng thái hiện tại của một tweak.
#[derive(Debug, Clone, Copy)]
pub enum Probe {
    /// Đã áp dụng khi DWORD bằng `value`. `missing_applied` = true khi
    /// giá trị không tồn tại cũng đồng nghĩa đã áp dụng (tweak mà trạng
    /// thái mặc định của Windows trùng với trạng thái áp dụng).
    Dword { root: Root, path: &'static str, name: &'static str, value: u32, missing_applied: bool },
    Sz { root: Root, path: &'static str, name: &'static str, value: &'static str },
    KeyExists { root: Root, path: &'static str },
    ServiceStart { name: &'static str, start: u32 },
    TaskDisabled { path: &'static str },
    UltimatePower,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TweakState {
    Applied,
    NotApplied,
    Unknown,
}

impl TweakState {
    pub fn label(self) -> &'static str {
        match self {
            TweakState::Applied => "Đã áp dụng",
            TweakState::NotApplied => "Chưa áp dụng",
            TweakState::Unknown => "Không rõ",
        }
    }
}

pub struct Tweak {
    pub id: &'static str,
    pub name: &'static str,
    pub category: Category,
    pub desc: &'static str,
    pub risk: Risk,
    pub apply: &'static [Op],
    pub undo: &'static [Op],
    pub probe: Probe,
}

// Đường dẫn dùng lặp lại
const ADV: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced";
const PERS: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
const CDM: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\ContentDeliveryManager";
const DESK: &str = "Control Panel\\Desktop";
const AU_POLICY: &str = "SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsUpdate\\AU";

use Category::*;
use Op::*;
use Root::*;

pub const TWEAKS: &[Tweak] = &[
    // =====================================================================
    // QUYỀN RIÊNG TƯ & TELEMETRY (9)
    // =====================================================================
    Tweak {
        id: "prv_diagtrack",
        name: "Tắt dịch vụ theo dõi DiagTrack",
        category: Privacy,
        desc: "Vô hiệu hoá dịch vụ Diagnostics Tracking Service (DiagTrack) — dịch vụ thu thập dữ liệu chẩn đoán/telemetry chính của Windows. Hoàn tác: đưa về Tự động và chạy lại.",
        risk: Risk::Safe,
        apply: &[ServiceStart { name: "DiagTrack", start: svc::START_DISABLED, stop_now: true, start_now: false }],
        undo: &[ServiceStart { name: "DiagTrack", start: svc::START_AUTO, stop_now: false, start_now: true }],
        probe: Probe::ServiceStart { name: "DiagTrack", start: svc::START_DISABLED },
    },
    Tweak {
        id: "prv_adid",
        name: "Tắt mã quảng cáo (Advertising ID)",
        category: Privacy,
        desc: "Không cho ứng dụng dùng mã quảng cáo cá nhân để theo dõi và hiển thị quảng cáo theo hành vi (Cài đặt > Quyền riêng tư > Chung).",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\AdvertisingInfo", name: "Enabled", value: 0 }],
        undo: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\AdvertisingInfo", name: "Enabled", value: 1 }],
        probe: Probe::Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\AdvertisingInfo", name: "Enabled", value: 0, missing_applied: false },
    },
    Tweak {
        id: "prv_activity",
        name: "Tắt lịch sử hoạt động (Activity History)",
        category: Privacy,
        desc: "Tắt ghi lịch sử hoạt động của người dùng (ứng dụng đã mở, tài liệu đã dùng) và không tải lịch sử lên tài khoản Microsoft — chính sách hệ thống EnableActivityFeed/PublishUserActivities/UploadUserActivities = 0.",
        risk: Risk::Safe,
        apply: &[
            Dword { root: Hklm, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\System", name: "EnableActivityFeed", value: 0 },
            Dword { root: Hklm, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\System", name: "PublishUserActivities", value: 0 },
            Dword { root: Hklm, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\System", name: "UploadUserActivities", value: 0 },
        ],
        undo: &[
            DelValue { root: Hklm, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\System", name: "EnableActivityFeed" },
            DelValue { root: Hklm, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\System", name: "PublishUserActivities" },
            DelValue { root: Hklm, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\System", name: "UploadUserActivities" },
        ],
        probe: Probe::Dword { root: Hklm, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\System", name: "EnableActivityFeed", value: 0, missing_applied: false },
    },
    Tweak {
        id: "prv_tailored",
        name: "Tắt trải nghiệm tuỳ biến theo dữ liệu chẩn đoán",
        category: Privacy,
        desc: "Không cho Windows dùng dữ liệu chẩn đoán để hiển thị gợi ý/mẹo tuỳ biến (Tailored experiences) trong Cài đặt và màn hình chào.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\CloudContent", name: "DisableTailoredExperiencesWithDiagnosticData", value: 1 }],
        undo: &[DelValue { root: Hkcu, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\CloudContent", name: "DisableTailoredExperiencesWithDiagnosticData" }],
        probe: Probe::Dword { root: Hkcu, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\CloudContent", name: "DisableTailoredExperiencesWithDiagnosticData", value: 1, missing_applied: false },
    },
    Tweak {
        id: "prv_suggest",
        name: "Tắt gợi ý ứng dụng & nội dung quảng cáo",
        category: Privacy,
        desc: "Tắt các gợi ý ứng dụng trong Start, ứng dụng được cài âm thầm theo gợi ý, mẹo ở màn hình khoá và nội dung đề xuất của Content Delivery Manager.",
        risk: Risk::Safe,
        apply: &[
            Dword { root: Hkcu, path: CDM, name: "ContentDeliveryAllowed", value: 0 },
            Dword { root: Hkcu, path: CDM, name: "SilentInstalledAppsEnabled", value: 0 },
            Dword { root: Hkcu, path: CDM, name: "SystemPaneSuggestionsEnabled", value: 0 },
            Dword { root: Hkcu, path: CDM, name: "SoftLandingEnabled", value: 0 },
            Dword { root: Hkcu, path: CDM, name: "PreInstalledAppsEnabled", value: 0 },
            Dword { root: Hkcu, path: CDM, name: "OemPreInstalledAppsEnabled", value: 0 },
        ],
        undo: &[
            Dword { root: Hkcu, path: CDM, name: "ContentDeliveryAllowed", value: 1 },
            Dword { root: Hkcu, path: CDM, name: "SilentInstalledAppsEnabled", value: 1 },
            Dword { root: Hkcu, path: CDM, name: "SystemPaneSuggestionsEnabled", value: 1 },
            Dword { root: Hkcu, path: CDM, name: "SoftLandingEnabled", value: 1 },
            Dword { root: Hkcu, path: CDM, name: "PreInstalledAppsEnabled", value: 1 },
            Dword { root: Hkcu, path: CDM, name: "OemPreInstalledAppsEnabled", value: 1 },
        ],
        probe: Probe::Dword { root: Hkcu, path: CDM, name: "ContentDeliveryAllowed", value: 0, missing_applied: false },
    },
    Tweak {
        id: "prv_trackprogs",
        name: "Tắt theo dõi ứng dụng đã mở",
        category: Privacy,
        desc: "Không cho Windows ghi lại ứng dụng người dùng đã khởi chạy để 'cải thiện Start và kết quả tìm kiếm' (Start_TrackProgs = 0).",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: ADV, name: "Start_TrackProgs", value: 0 }],
        undo: &[Dword { root: Hkcu, path: ADV, name: "Start_TrackProgs", value: 1 }],
        probe: Probe::Dword { root: Hkcu, path: ADV, name: "Start_TrackProgs", value: 0, missing_applied: false },
    },
    Tweak {
        id: "prv_ink",
        name: "Tắt thu thập chữ viết & gõ phím cá nhân hoá",
        category: Privacy,
        desc: "Không cho Windows thu thập mẫu chữ viết tay và từ ngữ gõ phím để cá nhân hoá nhận dạng (Input Personalization), kể cả danh bạ đã thu thập.",
        risk: Risk::Safe,
        apply: &[
            Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\InputPersonalization", name: "RestrictImplicitInkCollection", value: 1 },
            Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\InputPersonalization", name: "RestrictImplicitTextCollection", value: 1 },
            Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\InputPersonalization\\TrainedDataStore", name: "HarvestContacts", value: 0 },
        ],
        undo: &[
            Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\InputPersonalization", name: "RestrictImplicitInkCollection", value: 0 },
            Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\InputPersonalization", name: "RestrictImplicitTextCollection", value: 0 },
            Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\InputPersonalization\\TrainedDataStore", name: "HarvestContacts", value: 1 },
        ],
        probe: Probe::Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\InputPersonalization", name: "RestrictImplicitTextCollection", value: 1, missing_applied: false },
    },
    Tweak {
        id: "prv_ceip_tasks",
        name: "Tắt tác vụ CEIP (cải thiện trải nghiệm khách hàng)",
        category: Privacy,
        desc: "Vô hiệu hoá hai tác vụ theo lịch của chương trình CEIP: Consolidator và UsbCeip — các tác vụ định kỳ gửi dữ liệu sử dụng về Microsoft.",
        risk: Risk::Safe,
        apply: &[
            TaskEnabled { path: "\\Microsoft\\Windows\\Customer Experience Improvement Program\\Consolidator", enable: false },
            TaskEnabled { path: "\\Microsoft\\Windows\\Customer Experience Improvement Program\\UsbCeip", enable: false },
        ],
        undo: &[
            TaskEnabled { path: "\\Microsoft\\Windows\\Customer Experience Improvement Program\\Consolidator", enable: true },
            TaskEnabled { path: "\\Microsoft\\Windows\\Customer Experience Improvement Program\\UsbCeip", enable: true },
        ],
        probe: Probe::TaskDisabled { path: "\\Microsoft\\Windows\\Customer Experience Improvement Program\\Consolidator" },
    },
    Tweak {
        id: "prv_appraiser",
        name: "Tắt tác vụ thu thập tương thích ứng dụng",
        category: Privacy,
        desc: "Vô hiệu hoá Microsoft Compatibility Appraiser và ProgramDataUpdater — tác vụ quét ứng dụng đã cài để gửi dữ liệu tương thích về Microsoft (chạy nền định kỳ, tốn CPU/ổ đĩa).",
        risk: Risk::Safe,
        apply: &[
            TaskEnabled { path: "\\Microsoft\\Windows\\Application Experience\\Microsoft Compatibility Appraiser", enable: false },
            TaskEnabled { path: "\\Microsoft\\Windows\\Application Experience\\ProgramDataUpdater", enable: false },
        ],
        undo: &[
            TaskEnabled { path: "\\Microsoft\\Windows\\Application Experience\\Microsoft Compatibility Appraiser", enable: true },
            TaskEnabled { path: "\\Microsoft\\Windows\\Application Experience\\ProgramDataUpdater", enable: true },
        ],
        probe: Probe::TaskDisabled { path: "\\Microsoft\\Windows\\Application Experience\\Microsoft Compatibility Appraiser" },
    },
    // =====================================================================
    // GIAO DIỆN & EXPLORER (11)
    // =====================================================================
    Tweak {
        id: "ui_ext",
        name: "Hiện đuôi file trong Explorer",
        category: Ui,
        desc: "Luôn hiển thị phần mở rộng của file (HideFileExt = 0) — thói quen cần có cho kỹ thuật viên và giúp tránh file giả mạo kiểu 'tailieu.pdf.exe'.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: ADV, name: "HideFileExt", value: 0 }],
        undo: &[Dword { root: Hkcu, path: ADV, name: "HideFileExt", value: 1 }],
        probe: Probe::Dword { root: Hkcu, path: ADV, name: "HideFileExt", value: 0, missing_applied: false },
    },
    Tweak {
        id: "ui_hidden",
        name: "Hiện file & thư mục ẩn",
        category: Ui,
        desc: "Hiển thị các file/thư mục có thuộc tính ẩn (Hidden = 1) trong File Explorer.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: ADV, name: "Hidden", value: 1 }],
        undo: &[Dword { root: Hkcu, path: ADV, name: "Hidden", value: 2 }],
        probe: Probe::Dword { root: Hkcu, path: ADV, name: "Hidden", value: 1, missing_applied: false },
    },
    Tweak {
        id: "ui_superhidden",
        name: "Hiện file hệ thống được bảo vệ",
        category: Ui,
        desc: "Hiển thị cả các file hệ thống được bảo vệ (ShowSuperHidden = 1). Chỉ nên bật khi xử lý sự cố — người dùng phổ thông có thể xoá nhầm file hệ thống.",
        risk: Risk::Caution,
        apply: &[Dword { root: Hkcu, path: ADV, name: "ShowSuperHidden", value: 1 }],
        undo: &[Dword { root: Hkcu, path: ADV, name: "ShowSuperHidden", value: 0 }],
        probe: Probe::Dword { root: Hkcu, path: ADV, name: "ShowSuperHidden", value: 1, missing_applied: false },
    },
    Tweak {
        id: "ui_launchto",
        name: "Mở File Explorer vào This PC",
        category: Ui,
        desc: "Cửa sổ Explorer mới mở thẳng vào This PC (LaunchTo = 1) thay vì Quick Access/Home.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: ADV, name: "LaunchTo", value: 1 }],
        undo: &[Dword { root: Hkcu, path: ADV, name: "LaunchTo", value: 2 }],
        probe: Probe::Dword { root: Hkcu, path: ADV, name: "LaunchTo", value: 1, missing_applied: false },
    },
    Tweak {
        id: "ui_dark",
        name: "Bật chế độ tối (ứng dụng + hệ thống)",
        category: Ui,
        desc: "Chuyển giao diện ứng dụng và hệ thống sang tối (AppsUseLightTheme / SystemUsesLightTheme = 0). Hoàn tác: về chế độ sáng.",
        risk: Risk::Safe,
        apply: &[
            Dword { root: Hkcu, path: PERS, name: "AppsUseLightTheme", value: 0 },
            Dword { root: Hkcu, path: PERS, name: "SystemUsesLightTheme", value: 0 },
        ],
        undo: &[
            Dword { root: Hkcu, path: PERS, name: "AppsUseLightTheme", value: 1 },
            Dword { root: Hkcu, path: PERS, name: "SystemUsesLightTheme", value: 1 },
        ],
        probe: Probe::Dword { root: Hkcu, path: PERS, name: "AppsUseLightTheme", value: 0, missing_applied: false },
    },
    Tweak {
        id: "ui_widgets_off",
        name: "Tắt Widgets trên thanh tác vụ (Windows 11)",
        category: Ui,
        desc: "Ẩn nút/bảng Widgets (tin tức, thời tiết) khỏi thanh tác vụ (TaskbarDa = 0). Chỉ có tác dụng trên Windows 11.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: ADV, name: "TaskbarDa", value: 0 }],
        undo: &[Dword { root: Hkcu, path: ADV, name: "TaskbarDa", value: 1 }],
        probe: Probe::Dword { root: Hkcu, path: ADV, name: "TaskbarDa", value: 0, missing_applied: false },
    },
    Tweak {
        id: "ui_taskbar_left",
        name: "Căn trái thanh tác vụ (Windows 11)",
        category: Ui,
        desc: "Đưa nút Start và biểu tượng taskbar về phía trái như Windows 10 (TaskbarAl = 0). Chỉ có tác dụng trên Windows 11.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: ADV, name: "TaskbarAl", value: 0 }],
        undo: &[Dword { root: Hkcu, path: ADV, name: "TaskbarAl", value: 1 }],
        probe: Probe::Dword { root: Hkcu, path: ADV, name: "TaskbarAl", value: 0, missing_applied: false },
    },
    Tweak {
        id: "ui_search_icon",
        name: "Thu gọn ô tìm kiếm thành biểu tượng",
        category: Ui,
        desc: "Thanh tác vụ chỉ hiện biểu tượng kính lúp thay vì ô tìm kiếm dài (SearchboxTaskbarMode = 1). Hoàn tác: hiện ô tìm kiếm đầy đủ.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Search", name: "SearchboxTaskbarMode", value: 1 }],
        undo: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Search", name: "SearchboxTaskbarMode", value: 2 }],
        probe: Probe::Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Search", name: "SearchboxTaskbarMode", value: 1, missing_applied: false },
    },
    Tweak {
        id: "ui_transparency_off",
        name: "Tắt hiệu ứng trong suốt",
        category: Ui,
        desc: "Tắt độ trong suốt của Start/taskbar/cửa sổ (EnableTransparency = 0) — giao diện phẳng hơn, nhẹ GPU hơn trên máy yếu.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: PERS, name: "EnableTransparency", value: 0 }],
        undo: &[Dword { root: Hkcu, path: PERS, name: "EnableTransparency", value: 1 }],
        probe: Probe::Dword { root: Hkcu, path: PERS, name: "EnableTransparency", value: 0, missing_applied: false },
    },
    Tweak {
        id: "ui_checkboxes",
        name: "Hiện ô tick chọn file trong Explorer",
        category: Ui,
        desc: "Hiển thị ô checkbox cạnh mỗi file/thư mục để chọn nhiều mục bằng chuột (AutoCheckSelect = 1).",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: ADV, name: "AutoCheckSelect", value: 1 }],
        undo: &[Dword { root: Hkcu, path: ADV, name: "AutoCheckSelect", value: 0 }],
        probe: Probe::Dword { root: Hkcu, path: ADV, name: "AutoCheckSelect", value: 1, missing_applied: false },
    },
    Tweak {
        id: "ui_fullpath",
        name: "Hiện đường dẫn đầy đủ trên thanh tiêu đề Explorer",
        category: Ui,
        desc: "Thanh tiêu đề cửa sổ Explorer hiện đường dẫn đầy đủ thay vì chỉ tên thư mục (CabinetState FullPath = 1).",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\CabinetState", name: "FullPath", value: 1 }],
        undo: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\CabinetState", name: "FullPath", value: 0 }],
        probe: Probe::Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\CabinetState", name: "FullPath", value: 1, missing_applied: false },
    },
    // =====================================================================
    // HIỆU NĂNG (9)
    // =====================================================================
    Tweak {
        id: "perf_gamemode",
        name: "Bật Game Mode",
        category: Perf,
        desc: "Bật chế độ Game Mode: Windows ưu tiên tài nguyên cho game và hạn chế tác vụ nền khi chơi (AutoGameModeEnabled = 1). Mặc định Windows đã bật — trạng thái 'Đã áp dụng' khi chưa từng tắt.",
        risk: Risk::Safe,
        apply: &[
            Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\GameBar", name: "AutoGameModeEnabled", value: 1 },
            Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\GameBar", name: "AllowAutoGameMode", value: 1 },
        ],
        undo: &[
            Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\GameBar", name: "AutoGameModeEnabled", value: 0 },
            Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\GameBar", name: "AllowAutoGameMode", value: 0 },
        ],
        probe: Probe::Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\GameBar", name: "AutoGameModeEnabled", value: 1, missing_applied: true },
    },
    Tweak {
        id: "perf_gamedvr_off",
        name: "Tắt ghi hình nền Xbox Game Bar (Game DVR)",
        category: Perf,
        desc: "Tắt tính năng ghi lại gameplay ở nền của Game Bar (GameDVR_Enabled / AppCaptureEnabled = 0) — giảm giật lag và giải phóng CPU/GPU khi chơi game.",
        risk: Risk::Safe,
        apply: &[
            Dword { root: Hkcu, path: "System\\GameConfigStore", name: "GameDVR_Enabled", value: 0 },
            Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\GameDVR", name: "AppCaptureEnabled", value: 0 },
        ],
        undo: &[
            Dword { root: Hkcu, path: "System\\GameConfigStore", name: "GameDVR_Enabled", value: 1 },
            Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\GameDVR", name: "AppCaptureEnabled", value: 1 },
        ],
        probe: Probe::Dword { root: Hkcu, path: "System\\GameConfigStore", name: "GameDVR_Enabled", value: 0, missing_applied: false },
    },
    Tweak {
        id: "perf_ultimate",
        name: "Kích hoạt gói nguồn Ultimate Performance",
        category: Perf,
        desc: "Nhân bản và kích hoạt gói nguồn Ultimate Performance (ẩn sẵn trong Windows cho máy trạm): CPU luôn sẵn sàng ở mức cao, bỏ các độ trễ tiết kiệm điện. Chỉ phù hợp máy bàn cắm điện; hoàn tác: về gói Balanced.",
        risk: Risk::Caution,
        apply: &[UltimatePower(true)],
        undo: &[UltimatePower(false)],
        probe: Probe::UltimatePower,
    },
    Tweak {
        id: "perf_startupdelay",
        name: "Bỏ độ trễ 10 giây của ứng dụng khởi động cùng Windows",
        category: Perf,
        desc: "Mặc định Windows chờ ~10 giây sau khi đăng nhập mới chạy các ứng dụng trong thư mục Startup. Đặt StartupDelayInMSec = 0 để chạy ngay.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Serialize", name: "StartupDelayInMSec", value: 0 }],
        undo: &[DelValue { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Serialize", name: "StartupDelayInMSec" }],
        probe: Probe::Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Serialize", name: "StartupDelayInMSec", value: 0, missing_applied: false },
    },
    Tweak {
        id: "perf_visualfx",
        name: "Ưu tiên hiệu năng cho hiệu ứng hình ảnh",
        category: Perf,
        desc: "Đặt hiệu ứng hình ảnh ở mức 'Ưu tiên hiệu năng' (VisualFXSetting = 2): tắt phần lớn hoạt ảnh/bóng đổ. Giao diện sẽ kém mượt mà hơn — cân nhắc trên máy người dùng phổ thông.",
        risk: Risk::Caution,
        apply: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VisualEffects", name: "VisualFXSetting", value: 2 }],
        undo: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VisualEffects", name: "VisualFXSetting", value: 0 }],
        probe: Probe::Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VisualEffects", name: "VisualFXSetting", value: 2, missing_applied: false },
    },
    Tweak {
        id: "perf_menudelay",
        name: "Mở menu tức thì (MenuShowDelay = 0)",
        category: Perf,
        desc: "Bỏ độ trễ 400ms mặc định khi di chuột mở menu con (Start, menu ngữ cảnh) — cảm giác phản hồi nhanh hơn. Hoàn tác: 400.",
        risk: Risk::Safe,
        apply: &[Sz { root: Hkcu, path: DESK, name: "MenuShowDelay", value: "0" }],
        undo: &[Sz { root: Hkcu, path: DESK, name: "MenuShowDelay", value: "400" }],
        probe: Probe::Sz { root: Hkcu, path: DESK, name: "MenuShowDelay", value: "0" },
    },
    Tweak {
        id: "perf_throttling_off",
        name: "Tắt tiết kiệm điện nền (Power Throttling)",
        category: Perf,
        desc: "Tắt cơ chế hạ xung các tiến trình nền để tiết kiệm điện (PowerThrottlingOff = 1). Hữu ích cho máy bàn/máy xử lý tác vụ nền nặng; laptop sẽ tốn pin hơn.",
        risk: Risk::Caution,
        apply: &[Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\Power\\PowerThrottling", name: "PowerThrottlingOff", value: 1 }],
        undo: &[DelValue { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\Power\\PowerThrottling", name: "PowerThrottlingOff" }],
        probe: Probe::Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\Power\\PowerThrottling", name: "PowerThrottlingOff", value: 1, missing_applied: false },
    },
    Tweak {
        id: "perf_hags",
        name: "Bật tăng tốc GPU theo lịch phần cứng (HAGS)",
        category: Perf,
        desc: "Bật Hardware-Accelerated GPU Scheduling (HwSchMode = 2): giảm độ trễ và cải thiện hiệu năng game trên GPU hỗ trợ (GTX 10xx+/RX 5600+). CẦN khởi động lại máy mới có hiệu lực.",
        risk: Risk::Caution,
        apply: &[Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers", name: "HwSchMode", value: 2 }],
        undo: &[Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers", name: "HwSchMode", value: 1 }],
        probe: Probe::Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers", name: "HwSchMode", value: 2, missing_applied: false },
    },
    Tweak {
        id: "perf_mouse",
        name: "Tắt tăng tốc chuột (Enhance pointer precision)",
        category: Perf,
        desc: "Tắt gia tốc con trỏ chuột (MouseSpeed/MouseThreshold1/MouseThreshold2 = 0) — di chuột tuyến tính 1:1, chuẩn cho game FPS và đồ hoạ. Hoàn tác: bật lại (1/6/10).",
        risk: Risk::Safe,
        apply: &[
            Sz { root: Hkcu, path: "Control Panel\\Mouse", name: "MouseSpeed", value: "0" },
            Sz { root: Hkcu, path: "Control Panel\\Mouse", name: "MouseThreshold1", value: "0" },
            Sz { root: Hkcu, path: "Control Panel\\Mouse", name: "MouseThreshold2", value: "0" },
        ],
        undo: &[
            Sz { root: Hkcu, path: "Control Panel\\Mouse", name: "MouseSpeed", value: "1" },
            Sz { root: Hkcu, path: "Control Panel\\Mouse", name: "MouseThreshold1", value: "6" },
            Sz { root: Hkcu, path: "Control Panel\\Mouse", name: "MouseThreshold2", value: "10" },
        ],
        probe: Probe::Sz { root: Hkcu, path: "Control Panel\\Mouse", name: "MouseSpeed", value: "0" },
    },
    // =====================================================================
    // HỆ THỐNG (11)
    // =====================================================================
    Tweak {
        id: "sys_longpaths",
        name: "Bật đường dẫn dài (hơn 260 ký tự)",
        category: System,
        desc: "Cho phép đường dẫn file dài vượt giới hạn MAX_PATH 260 ký tự (LongPathsEnabled = 1) — cần cho cây thư mục sâu, dự án lập trình, sao lưu.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\FileSystem", name: "LongPathsEnabled", value: 1 }],
        undo: &[Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\FileSystem", name: "LongPathsEnabled", value: 0 }],
        probe: Probe::Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\FileSystem", name: "LongPathsEnabled", value: 1, missing_applied: false },
    },
    Tweak {
        id: "sys_numlock",
        name: "Bật NumLock khi khởi động",
        category: System,
        desc: "Tự bật phím NumLock ở màn hình đăng nhập và sau khi đăng nhập (InitialKeyboardIndicators = 2 cho người dùng hiện tại và hồ sơ mặc định).",
        risk: Risk::Safe,
        apply: &[
            Sz { root: Hkcu, path: "Control Panel\\Keyboard", name: "InitialKeyboardIndicators", value: "2" },
            Sz { root: Hku, path: ".DEFAULT\\Control Panel\\Keyboard", name: "InitialKeyboardIndicators", value: "2" },
        ],
        undo: &[
            Sz { root: Hkcu, path: "Control Panel\\Keyboard", name: "InitialKeyboardIndicators", value: "0" },
            Sz { root: Hku, path: ".DEFAULT\\Control Panel\\Keyboard", name: "InitialKeyboardIndicators", value: "0" },
        ],
        probe: Probe::Sz { root: Hkcu, path: "Control Panel\\Keyboard", name: "InitialKeyboardIndicators", value: "2" },
    },
    Tweak {
        id: "sys_verbose",
        name: "Hiện thông báo trạng thái chi tiết khi đăng nhập/tắt máy",
        category: System,
        desc: "Màn hình đăng nhập/tắt máy hiện rõ đang làm gì ('Đang áp dụng chính sách...', 'Đang chờ dịch vụ...') thay vì dòng chữ chung chung — rất hữu ích khi chẩn đoán máy chậm ở khâu logon (VerboseStatus = 1).",
        risk: Risk::Safe,
        apply: &[Dword { root: Hklm, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\System", name: "VerboseStatus", value: 1 }],
        undo: &[DelValue { root: Hklm, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\System", name: "VerboseStatus" }],
        probe: Probe::Dword { root: Hklm, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\System", name: "VerboseStatus", value: 1, missing_applied: false },
    },
    Tweak {
        id: "sys_bsod",
        name: "Hiện thông số chi tiết trên màn hình xanh (BSOD)",
        category: System,
        desc: "Khi máy gặp màn hình xanh, hiển thị thêm mã lỗi và thông số kỹ thuật (DisplayParameters = 1) thay vì chỉ mã QR — phục vụ tra cứu nguyên nhân.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\CrashControl", name: "DisplayParameters", value: 1 }],
        undo: &[Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\CrashControl", name: "DisplayParameters", value: 0 }],
        probe: Probe::Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\CrashControl", name: "DisplayParameters", value: 1, missing_applied: false },
    },
    Tweak {
        id: "sys_noreboot",
        name: "Không tự khởi động lại sau cập nhật khi đang có người dùng",
        category: System,
        desc: "Chính sách NoAutoRebootWithLoggedOnUsers = 1: Windows Update không tự khởi động lại máy khi đang có người dùng đăng nhập — tránh mất việc đang làm dở.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hklm, path: AU_POLICY, name: "NoAutoRebootWithLoggedOnUsers", value: 1 }],
        undo: &[DelValue { root: Hklm, path: AU_POLICY, name: "NoAutoRebootWithLoggedOnUsers" }],
        probe: Probe::Dword { root: Hklm, path: AU_POLICY, name: "NoAutoRebootWithLoggedOnUsers", value: 1, missing_applied: false },
    },
    Tweak {
        id: "sys_desktopver",
        name: "Hiện phiên bản Windows trên Desktop",
        category: System,
        desc: "Hiển thị tên bản dựng Windows ở góc dưới phải màn hình nền (PaintDesktopVersion = 1) — tiện cho kỹ thuật viên nhận diện máy từ xa qua ảnh chụp.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: DESK, name: "PaintDesktopVersion", value: 1 }],
        undo: &[Dword { root: Hkcu, path: DESK, name: "PaintDesktopVersion", value: 0 }],
        probe: Probe::Dword { root: Hkcu, path: DESK, name: "PaintDesktopVersion", value: 1, missing_applied: false },
    },
    Tweak {
        id: "sys_autorun_off",
        name: "Tắt Tự động chạy (AutoRun) cho mọi loại ổ",
        category: System,
        desc: "Vô hiệu hoá AutoRun trên mọi loại ổ đĩa (NoDriveTypeAutoRun = 255) — chặn đường lây mã độc qua USB/ổ mạng kiểu autorun.inf.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hklm, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer", name: "NoDriveTypeAutoRun", value: 255 }],
        undo: &[DelValue { root: Hklm, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer", name: "NoDriveTypeAutoRun" }],
        probe: Probe::Dword { root: Hklm, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer", name: "NoDriveTypeAutoRun", value: 255, missing_applied: false },
    },
    Tweak {
        id: "sys_remoteassist_off",
        name: "Tắt Trợ giúp từ xa (Remote Assistance)",
        category: System,
        desc: "Không cho máy nhận kết nối Trợ giúp từ xa (fAllowToGetHelp = 0) — giảm bề mặt tấn công; không ảnh hưởng Remote Desktop hay UltraViewer.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\Remote Assistance", name: "fAllowToGetHelp", value: 0 }],
        undo: &[Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\Remote Assistance", name: "fAllowToGetHelp", value: 1 }],
        probe: Probe::Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\Remote Assistance", name: "fAllowToGetHelp", value: 0, missing_applied: false },
    },
    Tweak {
        id: "sys_restartapps_off",
        name: "Không tự mở lại ứng dụng sau khi khởi động lại",
        category: System,
        desc: "Sau khi máy khởi động lại (kể cả do cập nhật), Windows không tự mở lại các ứng dụng đang chạy trước đó (RestartApps = 0 trong Winlogon).",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon", name: "RestartApps", value: 0 }],
        undo: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon", name: "RestartApps", value: 1 }],
        probe: Probe::Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon", name: "RestartApps", value: 0, missing_applied: false },
    },
    Tweak {
        id: "sys_shutdown_timeout",
        name: "Rút ngắn thời gian chờ khi tắt máy",
        category: System,
        desc: "Giảm thời gian chờ ứng dụng/dịch vụ tự đóng khi tắt máy (HungAppTimeout 1000ms, WaitToKillAppTimeout 2000ms, tự kết thúc ứng dụng treo) — máy tắt nhanh hơn; ứng dụng đang bận ghi dữ liệu có ít thời gian hơn để tự lưu.",
        risk: Risk::Caution,
        apply: &[
            Sz { root: Hkcu, path: DESK, name: "HungAppTimeout", value: "1000" },
            Sz { root: Hkcu, path: DESK, name: "WaitToKillAppTimeout", value: "2000" },
            Sz { root: Hkcu, path: DESK, name: "AutoEndTasks", value: "1" },
        ],
        undo: &[
            Sz { root: Hkcu, path: DESK, name: "HungAppTimeout", value: "5000" },
            Sz { root: Hkcu, path: DESK, name: "WaitToKillAppTimeout", value: "20000" },
            Sz { root: Hkcu, path: DESK, name: "AutoEndTasks", value: "0" },
        ],
        probe: Probe::Sz { root: Hkcu, path: DESK, name: "HungAppTimeout", value: "1000" },
    },
    Tweak {
        id: "sys_faststartup_off",
        name: "Tắt Khởi động nhanh (Fast Startup)",
        category: System,
        desc: "Tắt tính năng khởi động nhanh (HiberbootEnabled = 0): tắt máy là tắt hẳn, tránh lỗi treo driver/dual-boot và trạng thái 'uptime không reset'. Máy sẽ khởi động chậm hơn một chút.",
        risk: Risk::Caution,
        apply: &[Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Power", name: "HiberbootEnabled", value: 0 }],
        undo: &[Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Power", name: "HiberbootEnabled", value: 1 }],
        probe: Probe::Dword { root: Hklm, path: "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Power", name: "HiberbootEnabled", value: 0, missing_applied: false },
    },
    // =====================================================================
    // MENU NGỮ CẢNH & TÍNH NĂNG (9)
    // =====================================================================
    Tweak {
        id: "feat_cortana_off",
        name: "Tắt Cortana",
        category: Features,
        desc: "Vô hiệu hoá trợ lý Cortana bằng chính sách hệ thống (AllowCortana = 0). Cortana đã bị Microsoft khai tử ở nhiều khu vực nhưng chính sách này vẫn dọn sạch phần còn lại.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hklm, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\Windows Search", name: "AllowCortana", value: 0 }],
        undo: &[DelValue { root: Hklm, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\Windows Search", name: "AllowCortana" }],
        probe: Probe::Dword { root: Hklm, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\Windows Search", name: "AllowCortana", value: 0, missing_applied: false },
    },
    Tweak {
        id: "feat_bing_off",
        name: "Tắt tìm kiếm Bing/web trong Start",
        category: Features,
        desc: "Ô tìm kiếm Start chỉ tìm trên máy, không gửi truy vấn lên Bing và không hiện kết quả web (DisableSearchBoxSuggestions = 1) — tìm nhanh hơn và riêng tư hơn.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\Explorer", name: "DisableSearchBoxSuggestions", value: 1 }],
        undo: &[DelValue { root: Hkcu, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\Explorer", name: "DisableSearchBoxSuggestions" }],
        probe: Probe::Dword { root: Hkcu, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\Explorer", name: "DisableSearchBoxSuggestions", value: 1, missing_applied: false },
    },
    Tweak {
        id: "feat_copilot_off",
        name: "Tắt Windows Copilot",
        category: Features,
        desc: "Tắt trợ lý Copilot bằng chính sách (TurnOffWindowsCopilot = 1) và ẩn nút Copilot khỏi thanh tác vụ.",
        risk: Risk::Safe,
        apply: &[
            Dword { root: Hkcu, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsCopilot", name: "TurnOffWindowsCopilot", value: 1 },
            Dword { root: Hkcu, path: ADV, name: "ShowCopilotButton", value: 0 },
        ],
        undo: &[
            DelValue { root: Hkcu, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsCopilot", name: "TurnOffWindowsCopilot" },
            Dword { root: Hkcu, path: ADV, name: "ShowCopilotButton", value: 1 },
        ],
        probe: Probe::Dword { root: Hkcu, path: "SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsCopilot", name: "TurnOffWindowsCopilot", value: 1, missing_applied: false },
    },
    Tweak {
        id: "feat_classic_menu",
        name: "Dùng menu chuột phải đầy đủ kiểu Windows 10 (Windows 11)",
        category: Features,
        desc: "Bỏ menu chuột phải rút gọn của Windows 11, hiện luôn menu đầy đủ kiểu cũ (đăng ký CLSID InprocServer32 rỗng cho người dùng hiện tại). Cần khởi động lại Explorer/đăng xuất mới thấy. Hoàn tác: xoá khoá đã tạo.",
        risk: Risk::Safe,
        apply: &[DefaultSz { root: Hkcu, path: "SOFTWARE\\Classes\\CLSID\\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\\InprocServer32", value: "" }],
        undo: &[DelTree { root: Hkcu, path: "SOFTWARE\\Classes\\CLSID\\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}" }],
        probe: Probe::KeyExists { root: Hkcu, path: "SOFTWARE\\Classes\\CLSID\\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\\InprocServer32" },
    },
    Tweak {
        id: "feat_meetnow_off",
        name: "Ẩn biểu tượng Meet Now (Skype)",
        category: Features,
        desc: "Ẩn nút 'Meet Now' của Skype khỏi khu vực thông báo (HideSCAMeetNow = 1).",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer", name: "HideSCAMeetNow", value: 1 }],
        undo: &[DelValue { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer", name: "HideSCAMeetNow" }],
        probe: Probe::Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer", name: "HideSCAMeetNow", value: 1, missing_applied: false },
    },
    Tweak {
        id: "feat_news_off",
        name: "Tắt Tin tức & thời tiết trên thanh tác vụ (Windows 10)",
        category: Features,
        desc: "Ẩn tiện ích News and Interests khỏi thanh tác vụ Windows 10 (ShellFeedsTaskbarViewMode = 2). Hoàn tác: hiện biểu tượng + văn bản.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Feeds", name: "ShellFeedsTaskbarViewMode", value: 2 }],
        undo: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Feeds", name: "ShellFeedsTaskbarViewMode", value: 0 }],
        probe: Probe::Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Feeds", name: "ShellFeedsTaskbarViewMode", value: 2, missing_applied: false },
    },
    Tweak {
        id: "feat_highlights_off",
        name: "Tắt điểm nhấn tìm kiếm (Search Highlights)",
        category: Features,
        desc: "Tắt hình ảnh/nội dung 'điểm nhấn' thay đổi mỗi ngày trong ô tìm kiếm (IsDynamicSearchBoxEnabled = 0).",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\SearchSettings", name: "IsDynamicSearchBoxEnabled", value: 0 }],
        undo: &[Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\SearchSettings", name: "IsDynamicSearchBoxEnabled", value: 1 }],
        probe: Probe::Dword { root: Hkcu, path: "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\SearchSettings", name: "IsDynamicSearchBoxEnabled", value: 0, missing_applied: false },
    },
    Tweak {
        id: "feat_chat_off",
        name: "Ẩn biểu tượng Chat (Teams cá nhân)",
        category: Features,
        desc: "Ẩn nút Chat của Microsoft Teams bản cá nhân khỏi thanh tác vụ Windows 11 (TaskbarMn = 0). Không gỡ ứng dụng, chỉ ẩn nút.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: ADV, name: "TaskbarMn", value: 0 }],
        undo: &[Dword { root: Hkcu, path: ADV, name: "TaskbarMn", value: 1 }],
        probe: Probe::Dword { root: Hkcu, path: ADV, name: "TaskbarMn", value: 0, missing_applied: false },
    },
    Tweak {
        id: "feat_taskview_off",
        name: "Ẩn nút Task View",
        category: Features,
        desc: "Ẩn nút Task View (màn hình ảo) khỏi thanh tác vụ (ShowTaskViewButton = 0). Tổ hợp phím Win+Tab vẫn dùng bình thường.",
        risk: Risk::Safe,
        apply: &[Dword { root: Hkcu, path: ADV, name: "ShowTaskViewButton", value: 0 }],
        undo: &[Dword { root: Hkcu, path: ADV, name: "ShowTaskViewButton", value: 1 }],
        probe: Probe::Dword { root: Hkcu, path: ADV, name: "ShowTaskViewButton", value: 0, missing_applied: false },
    },
];
