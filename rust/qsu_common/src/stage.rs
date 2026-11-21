//! Stage config (stage.json) — cầu nối giữa wizard (trên Windows đang
//! chạy) và winpe_install (trong WinPE). Wizard ghi file này vào
//! <Data>:\<stagingFolder>\stage.json; winpe_install tìm ổ Data bằng
//! marker + đọc file.
//!
//! LƯU Ý BẢO MẬT: cấu trúc này VẪN giữ các trường `admin_password`,
//! `ultraviewer_password` và `apps` vì lý do tương thích lược đồ,
//! nhưng wizard hiện tại KHÔNG BAO GIỜ ghi chúng ra stage.json: nó gán
//! rỗng rồi loại hẳn các khoá `apps` / `ultraviewerPassword` /
//! `adminPassword` khỏi JSON trước khi ghi (bản rebuild không có agent
//! hậu cài). Mật khẩu admin chỉ nằm trong autounattend.xml (bắt buộc
//! cho AutoLogon) và không bao giờ được ghi vào log.
//!
//! Ngoại lệ CÓ CHỦ ĐÍCH duy nhất (v0.7.27): `notify_once_token` —
//! token DÙNG MỘT LẦN do relay Zalo cấp cho wizard lúc staging
//! (hết hạn ≤ 12 giờ, tối đa 3 tin) để engine trong WinPE gửi tin
//! CÀI XONG / LỖI mà không cần giữ bí mật dài hạn nào. Đây KHÔNG
//! PHẢI secret dài hạn: secret dài hạn của relay và Bot Token
//! Telegram chỉ nằm trong settings.json cạnh wizard, TUYỆT ĐỐI
//! không vào stage.json.

use std::path::Path;

use crate::json::{self, Json};
use crate::settings::AppEntry;

/// Một phân vùng hệ thống được chọn XOÁ trong chế độ phân vùng
/// `"rebuild"` (v0.7.13): wizard chụp lại từ liệt kê đĩa lúc staging
/// (loại + kích thước + định danh vật lý), engine WinPE đối chiếu lại
/// với đĩa thật ngay trước khi xoá — lệch bất kỳ điểm nào là dừng
/// cứng (exit 13), không đụng vào đĩa.
#[derive(Debug, Clone, Default)]
pub struct RebuildPart {
    /// "EFI" | "MSR" | "Windows" | "Recovery" | "System".
    pub kind: String,
    pub size_bytes: u64,
    /// GUID duy nhất của phân vùng trên đĩa GPT (PartitionId);
    /// rỗng với đĩa MBR (đối chiếu bằng offset + size).
    pub gpt_guid: String,
    /// Offset tuyệt đối từ đầu đĩa (byte).
    pub offset_bytes: u64,
}

impl RebuildPart {
    pub fn to_json(&self) -> Json {
        json::obj(vec![
            ("kind", json::s(&self.kind)),
            ("sizeBytes", json::n(self.size_bytes)),
            ("gptGuid", json::s(&self.gpt_guid)),
            ("offsetBytes", json::n(self.offset_bytes)),
        ])
    }

    pub fn from_json(v: &Json) -> RebuildPart {
        RebuildPart {
            kind: v.str_or("kind", ""),
            size_bytes: v.u64_or("sizeBytes", 0),
            gpt_guid: v.str_or("gptGuid", ""),
            offset_bytes: v.u64_or("offsetBytes", 0),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct StageConfig {
    pub version: u64,
    /// "clean" (cài mới qua WinPE) — chế độ cài đè không cần stage config
    pub mode: String,
    /// Đường dẫn tương đối (so với gốc staging) tới file image
    pub image_file: String,
    /// Mẫu file SWM đi kèm (nếu image là .swm), ví dụ "sources\\install*.swm"
    pub image_swm: String,
    pub image_index: u64,
    pub image_name: String,
    pub boot_mode: String, // "UEFI" | "Legacy"
    /// Chế độ phân vùng khi cài mới (§6b): "preserve" = giữ cấu trúc
    /// phân vùng hiện có, chỉ format phân vùng OS (mặc định, tương
    /// thích stage.json cũ không có trường này); "wipeDisk" = xoá
    /// TOÀN BỘ ổ đĩa đích, chia lại từ đầu — đích khi đó là cả đĩa vật
    /// lý nhận diện bằng `target_disk_serial`; "rebuild" (v0.7.13) =
    /// chỉ xoá các phân vùng hệ thống liệt kê trong
    /// `rebuild_partitions` trên đĩa đích (đĩa chứa staging ĐƯỢC
    /// phép làm đích), chia lại layout mới trong đúng vùng trống
    /// đó, giữ nguyên mọi phân vùng data trên cùng đĩa.
    pub partition_mode: String,
    /// Serial PHẦN CỨNG của đĩa vật lý đích (dùng khi
    /// partition_mode == "wipeDisk" hoặc "rebuild"). Engine đối
    /// chiếu với serial thật ngay trước khi chạy diskpart; lệch là
    /// dừng, không đụng đĩa.
    pub target_disk_serial: String,
    /// Danh sách phân vùng hệ thống sẽ XOÁ trong chế độ "rebuild"
    /// (v0.7.13): chỉ các phân vùng này bị xoá và chia lại trong
    /// vùng trống của chúng; mọi phân vùng khác trên cùng đĩa
    /// (data) giữ nguyên. Rỗng với hai chế độ còn lại và với
    /// stage.json cũ.
    pub rebuild_partitions: Vec<RebuildPart>,
    /// Số serial của phân vùng OS tại thời điểm wizard chạy — WinPE đối
    /// chiếu theo serial, không theo ký tự ổ đĩa
    pub os_volume_serial: u64,
    pub os_volume_label: String,
    pub data_volume_serial: u64,
    /// GUID entry BCD tạm của WinPE (để post_install dọn)
    pub bcd_guid: String,
    pub computer_name: String,
    pub admin_user: String,
    /// Mật khẩu tài khoản admin (cho AutoLogon ở unattend) — KHÔNG log
    pub admin_password: String,
    /// Mật khẩu unattended UltraViewer — KHÔNG log
    pub ultraviewer_password: String,
    pub apply_tweaks: bool,
    /// Hồ sơ ảnh cài đặt (v0.7.21; định nghĩa CORE thay đổi ở
    /// v0.7.22 và v0.7.23): "standard" (mặc định; stage.json cũ
    /// không có trường này vẫn hiểu là standard) hoặc "core" —
    /// ảnh Windows rút gọn kiểu Tiny11 Core: engine cài theo
    /// công thức WinNTSetup (không inject driver, không chép
    /// WinRE, không agent hậu cài/SetupComplete) cộng
    /// autounattend CHỈ chứa pha oobeSystem để ca cài vẫn tự
    /// động (xem docs/co_che_v0.7.23.md).
    pub image_profile: String,
    /// Danh sách ứng dụng hậu cài đầy đủ (url/hash/args) — nhúng tại đây
    /// vì trên Windows mới không còn settings.json của wizard
    pub apps: Vec<AppEntry>,
    /// Thư mục backup driver (tương đối so với gốc ổ Data)
    pub driver_backup_rel: String,
    pub logs_folder: String,
    pub staging_folder: String,
    /// Thông báo trạng thái qua relay Zalo (v0.7.27): địa chỉ
    /// relay + TOKEN DÙNG MỘT LẦN wizard xin từ relay ngay lúc
    /// staging (xem ghi chú bảo mật đầu file). Engine dùng cặp
    /// này để gửi đúng tin kết thúc (CÀI XONG / LỖI) qua
    /// /notify-once. Rỗng ở một trong hai = tắt (mặc định; mọi
    /// stage.json cũ đều rỗng hai trường này).
    pub notify_relay_url: String,
    pub notify_once_token: String,
}

impl StageConfig {
    pub fn to_json(&self) -> Json {
        json::obj(vec![
            ("version", json::n(self.version)),
            ("mode", json::s(&self.mode)),
            ("imageFile", json::s(&self.image_file)),
            ("imageSwm", json::s(&self.image_swm)),
            ("imageIndex", json::n(self.image_index)),
            ("imageName", json::s(&self.image_name)),
            ("bootMode", json::s(&self.boot_mode)),
            ("partitionMode", json::s(&self.partition_mode)),
            ("targetDiskSerial", json::s(&self.target_disk_serial)),
            (
                "rebuildPartitions",
                Json::Arr(self.rebuild_partitions.iter().map(|p| p.to_json()).collect()),
            ),
            ("osVolumeSerial", json::n(self.os_volume_serial)),
            ("osVolumeLabel", json::s(&self.os_volume_label)),
            ("dataVolumeSerial", json::n(self.data_volume_serial)),
            ("bcdGuid", json::s(&self.bcd_guid)),
            ("computerName", json::s(&self.computer_name)),
            ("adminUser", json::s(&self.admin_user)),
            ("adminPassword", json::s(&self.admin_password)),
            ("ultraviewerPassword", json::s(&self.ultraviewer_password)),
            ("applyTweaks", json::b(self.apply_tweaks)),
            ("imageProfile", json::s(&self.image_profile)),
            (
                "apps",
                Json::Arr(self.apps.iter().map(|a| a.to_json()).collect()),
            ),
            ("driverBackupRel", json::s(&self.driver_backup_rel)),
            ("logsFolder", json::s(&self.logs_folder)),
            ("stagingFolder", json::s(&self.staging_folder)),
            ("notifyRelayUrl", json::s(&self.notify_relay_url)),
            ("notifyOnceToken", json::s(&self.notify_once_token)),
        ])
    }

    pub fn from_json(v: &Json) -> StageConfig {
        let apps = v
            .get("apps")
            .and_then(|a| a.as_arr())
            .map(|a| a.iter().filter_map(AppEntry::from_json).collect())
            .unwrap_or_default();
        // stage.json cũ không có "rebuildPartitions" → vec rỗng,
        // hành vi giữ nguyên như trước v0.7.13.
        let rebuild_partitions = v
            .get("rebuildPartitions")
            .and_then(|a| a.as_arr())
            .map(|a| a.iter().map(RebuildPart::from_json).collect())
            .unwrap_or_default();
        StageConfig {
            version: v.u64_or("version", 1),
            mode: v.str_or("mode", "clean"),
            image_file: v.str_or("imageFile", ""),
            image_swm: v.str_or("imageSwm", ""),
            image_index: v.u64_or("imageIndex", 1),
            image_name: v.str_or("imageName", ""),
            boot_mode: v.str_or("bootMode", "UEFI"),
            // Mặc định "preserve": stage.json cũ (trước §6b) không có
            // trường này vẫn chạy đúng hành vi giữ phân vùng.
            partition_mode: v.str_or("partitionMode", "preserve"),
            target_disk_serial: v.str_or("targetDiskSerial", ""),
            rebuild_partitions,
            os_volume_serial: v.u64_or("osVolumeSerial", 0),
            os_volume_label: v.str_or("osVolumeLabel", ""),
            data_volume_serial: v.u64_or("dataVolumeSerial", 0),
            bcd_guid: v.str_or("bcdGuid", ""),
            computer_name: v.str_or("computerName", ""),
            admin_user: v.str_or("adminUser", ""),
            admin_password: v.str_or("adminPassword", ""),
            ultraviewer_password: v.str_or("ultraviewerPassword", ""),
            apply_tweaks: v.bool_or("applyTweaks", false),
            // stage.json cũ (trước v0.7.21) không có trường này →
            // "standard", hành vi giữ nguyên.
            image_profile: v.str_or("imageProfile", "standard"),
            apps,
            driver_backup_rel: v.str_or("driverBackupRel", "QSU-DriverBackup"),
            logs_folder: v.str_or("logsFolder", "QSU-Logs"),
            staging_folder: v.str_or("stagingFolder", "QSU-Stage"),
            // stage.json cũ (trước v0.7.27) không có hai khoá
            // này → rỗng = engine không gửi tin kết thúc.
            notify_relay_url: v.str_or("notifyRelayUrl", ""),
            notify_once_token: v.str_or("notifyOnceToken", ""),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json::stringify(&self.to_json()))
    }

    pub fn load(path: &Path) -> Result<StageConfig, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("không đọc được {}: {}", path.display(), e))?;
        let v = json::parse(&text)?;
        Ok(StageConfig::from_json(&v))
    }
}
