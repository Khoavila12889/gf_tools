#![windows_subsystem = "windows"]
//! CaiDatWinTuDong — wizard "Cài Đặt Windows Online / Từ Xa" (Rust).
//!
//! Triển khai theo đặc tả `docs/logic_cai_dat_win_online.md`:
//!   §2 kiểm tra tiền đề · §3 nguồn ISO (local + kho online, tải có
//!   resume, kiểm SHA-256 bắt buộc) · §4 chọn phiên bản trong ISO ·
//!   §5 cài đè bằng setup.exe · §6 cài mới từ xa bằng WinPE staging +
//!   BCD boot một lần · §7 sinh autounattend.xml · §8 log/huỷ an toàn.
//!
//! Quyết định triển khai đáng chú ý (xem "Implementation notes v1.1"
//! trong đặc tả):
//! - Mount ISO BA LỚP (v0.7.17): (1) dùng lại mount có sẵn nếu ISO
//!   đã gắn (Get-DiskImage Attached); (2) PowerShell `Mount-DiskImage`
//!   với lỗi ép thành terminating để hộp thoại luôn thấy lỗi thật —
//!   các bản trước chỉ trông vào một lệnh này và khi nó nuốt lỗi,
//!   hộp thoại hiện "(output: )" rỗng (thực địa trên máy chạy
//!   Windows lite: cmdlet Storage không mount được mà không để lại
//!   chữ nào); (3) dự phòng bằng VirtDisk API gốc (OpenVirtualDisk
//!   storage-type ISO + AttachVirtualDisk đọc-chỉ-đọc, ký tự ổ dò
//!   bằng hiệu tập volume trước/sau attach) — không phụ thuộc
//!   PowerShell/cmdlet nào. Gỡ mount đối xứng theo cơ chế đã mount
//!   (sổ đăng ký mount trong tiến trình): Dismount-DiskImage cho
//!   mount PowerShell, DetachVirtualDisk + đóng handle cho VirtDisk.
//!   Engine WinPE không phụ thuộc cơ chế này.
//! - Trạng thái BitLocker đọc qua `Get-BitLockerVolume` (tên enum
//!   không phụ thuộc ngôn ngữ hiển thị), tránh phân tích output đã
//!   bản địa hoá của manage-bde.
//! - DISM luôn gọi kèm `/English` để output phân tích được trên mọi
//!   ngôn ngữ Windows.
//! - Mọi thao tác dài chạy ở luồng nền; giao diện nhận cập nhật qua
//!   NWG Notice — UI không bao giờ bị treo.
//! - stage.json ghi ra ĐÃ LOẠI BỎ các trường `apps`,
//!   `ultraviewerPassword` và `adminPassword`: bản rebuild không có
//!   agent hậu cài (không cài ứng dụng im lặng, không cấu hình mật
//!   khẩu truy cập từ xa). Mật khẩu admin là TUỲ CHỌN (v0.7.2): để
//!   trống cả hai ô = tài khoản không mật khẩu (unattend bỏ hẳn phần
//!   tử <Password>); có đặt thì mật khẩu chỉ nằm trong
//!   autounattend.xml và không vào log.
//! - Thư mục `custom` cạnh file wizard (nếu có) được copy nguyên
//!   trạng vào staging; winpe_install chép tiếp vào
//!   ProgramData\QSU\custom của bản Windows mới. Bộ công cụ không
//!   diễn giải hay thực thi nội dung thư mục này.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use native_windows_gui as nwg;
use qsu_common::cmd;
use qsu_common::disk::{self, DiskPartition, PartKind, PhysicalDisk};
use qsu_common::http;
use qsu_common::json::{self, Json};
use qsu_common::log::Logger;
use qsu_common::settings::Settings;
use qsu_common::sha256;
use qsu_common::stage::{RebuildPart, StageConfig};
use qsu_common::sys::{self, DriveInfo, OsInfo, PartitionStyle, PowerStatus};
use winapi::shared::minwindef::{LPARAM, WPARAM};
use winapi::shared::windef::{HWND, RECT};
use winapi::um::commctrl::LVITEMW;
use winapi::um::winuser::{
    GetClientRect, GetParent, GetWindowLongPtrW, GetWindowRect, InvalidateRect, SendMessageW,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, SystemParametersInfoW, ES_PASSWORD, ES_READONLY,
    GWL_STYLE, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOW, WS_MAXIMIZEBOX,
    WS_THICKFRAME,
};

// ---------------------------------------------------------------------------
// Hằng số
// ---------------------------------------------------------------------------
const DATA_MARKER: &str = ".qsu_data_drive";
const DRIVER_BACKUP_FOLDER: &str = "QSU-DriverBackup";
const GB: u64 = 1_073_741_824;


// ---------------------------------------------------------------------------
// Kiểu dữ liệu nghiệp vụ
// ---------------------------------------------------------------------------
#[derive(Clone, Copy, PartialEq, Debug)]
enum BitlockerState {
    Decrypted,
    Encrypted,
    InProgress,
    Unknown,
}

impl BitlockerState {
    fn label(&self) -> &'static str {
        match self {
            BitlockerState::Decrypted => "Đã giải mã hoàn toàn (an toàn)",
            BitlockerState::Encrypted => "Đang bật — sẽ tạm treo (suspend) khi cài",
            BitlockerState::InProgress => "Đang mã hoá/giải mã dở dang — CHẶN",
            BitlockerState::Unknown => "Không đọc được (máy có thể không có BitLocker)",
        }
    }
}

#[derive(Clone)]
struct Preflight {
    os_letter: char,
    os: OsInfo,
    uefi: bool,
    part: PartitionStyle,
    bitlocker: BitlockerState,
    power: PowerStatus,
    oem_key: bool,
    drives: Vec<DriveInfo>,
    marker_drives: Vec<char>,
    /// Mọi đĩa vật lý (serial/model/phân vùng đầy đủ) — phục vụ chế độ
    /// wipeDisk (§6b): chọn đĩa đích + đối chiếu ổ Data nằm ở đĩa nào.
    disks: Vec<PhysicalDisk>,
    /// Kết quả dò firmware dạng thô (v0.7.7): giá trị nguyên văn của
    /// cả hai API dò + phán quyết — ghi vào log quét và log cài đặt
    /// để đối chiếu với các phần tử BCD đã đăng ký.
    firmware: FirmwareProbe,
    /// Chẩn đoán liệt kê đĩa (mỗi IOCTL lỗi mã nào, đường dự phòng
    /// nào đã chạy) — đẩy vào log panel lúc quét, ghi ngay ra file
    /// wizard-scan-*.log khi quét xong (v0.7.2) và chép tiếp vào
    /// file log cài đặt khi bắt đầu cài.
    disk_diag: Vec<String>,
}

impl Preflight {
    fn drive(&self, letter: char) -> Option<&DriveInfo> {
        self.drives.iter().find(|d| d.letter == letter)
    }
    fn os_free(&self) -> u64 {
        self.drive(self.os_letter).map(|d| d.free).unwrap_or(0)
    }
    /// Ổ Data mặc định: ổ có marker trước, không có thì ổ phụ lớn nhất.
    /// Marker nằm trên chính ổ OS (do một lần cài đè trên máy một ổ để
    /// lại) KHÔNG được tính — ổ OS không bao giờ là ổ Data.
    fn default_data_letter(&self) -> Option<char> {
        if let Some(l) = self.marker_drives.iter().find(|l| **l != self.os_letter) {
            return Some(*l);
        }
        self.drives
            .iter()
            .filter(|d| d.letter != self.os_letter)
            .max_by_key(|d| d.total)
            .map(|d| d.letter)
    }
    fn non_os_drives(&self) -> Vec<&DriveInfo> {
        self.drives.iter().filter(|d| d.letter != self.os_letter).collect()
    }
}

#[derive(Clone)]
struct IsoFile {
    path: PathBuf,
    size: u64,
}

#[derive(Clone)]
struct CatalogueImage {
    name: String,
    edition: String,
    url: String,
    size: u64,
    sha256: String,
}

#[derive(Clone)]
struct EditionInfo {
    index: u32,
    name: String,
    edition: String,
    build: u32,
    arch: String,
    size_bytes: u64,
}

/// Kết quả một thao tác mạng của tab "Thông báo" (v0.7.27) —
/// luồng nền ghi vào `Shared::notify_done`, `refresh` đọc ra để
/// nhả khoá nút + hiện kết quả. Không biến thể nào mang theo
/// token/secret (kể cả trong thông điệp lỗi — `qsu_common::notify`
/// đã lọc Bot Token khỏi lỗi trước khi trả).
enum NotifyDone {
    /// "Lấy Chat ID": Ok((chat_id, tên người gửi / nhóm))
    ChatId(Result<(String, String), String>),
    /// "Gửi thử ...": kèm tên kênh để hiện đúng dòng trạng thái
    Test(&'static str, Result<(), String>),
}

// ---------------------------------------------------------------------------
// Trạng thái dùng chung luồng nền -> giao diện (qua NWG Notice)
// ---------------------------------------------------------------------------
#[derive(Default)]
struct Shared {
    log_lines: VecDeque<String>,
    preflight: Option<Preflight>,
    iso_list: Option<Vec<IsoFile>>,
    catalogue: Option<Result<Vec<CatalogueImage>, String>>,
    dl_progress: Option<(u64, u64)>,
    dl_done: Option<Result<PathBuf, String>>,
    editions: Option<Result<(char, Vec<EditionInfo>), String>>,
    exec_progress: Option<(u32, String)>,
    exec_msg: Option<String>,
    exec_done: Option<Result<String, String>>,
    notify_done: Option<NotifyDone>,
}

type SharedRef = Arc<Mutex<Shared>>;

/// Khoá Shared chịu được poison: nếu một luồng nền từng panic trong
/// lúc đang giữ khoá, các luồng còn lại vẫn lấy được trạng thái thay
/// vì panic dây chuyền (mỗi `lock().unwrap()` trên mutex đã poison
/// là thêm một panic mới — mà trong app GUI, panic không ai thấy).
/// Dùng trên toàn bộ đường thực thi cài đặt để chốt hoàn tất/lỗi
/// LUÔN ghi được vào Shared, giao diện luôn được mở khoá lại.
fn lock_shared(s: &SharedRef) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// Trích thông điệp từ payload của `catch_unwind` để ghi log/hiện lỗi.
fn panic_payload_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "không rõ nội dung panic".to_string()
    }
}

/// Thư mục log trên ổ Data (vd `E:\QSU-Logs`) — nơi hộp đen panic
/// ghi BẢN SAO THỨ HAI ngay khi ổ Data đã được biết (đặt ở
/// `on_preflight` và lúc tạo logger cài đặt). Bản cạnh file exe có
/// thể bị kỹ thuật viên bỏ sót vì nằm trong thư mục modules; bản
/// trên ổ Data nằm cùng chỗ với các file log họ vẫn gửi chẩn đoán.
static PANIC_LOG_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

fn set_panic_log_dir(dir: PathBuf) {
    if let Ok(mut g) = PANIC_LOG_DIR.lock() {
        *g = Some(dir);
    }
}

/// Hộp đen khi panic: wizard biên dịch dạng GUI subsystem (không có
/// console) nên panic mặc định biến mất không dấu vết — người dùng
/// chỉ thấy cửa sổ "tự nhiên đóng" mà không có gì để gửi chẩn đoán.
/// Hook này ghi MỌI panic (luồng giao diện lẫn luồng nền) kèm vị trí
/// vào `CaiDatWinTuDong-panic.log` cạnh file exe VÀ (khi đã biết ổ
/// Data) một bản sao trong thư mục log trên ổ Data. Hook không được
/// panic theo: mọi thao tác ghi đều nuốt lỗi.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "(không rõ vị trí)".to_string());
        let line = format!(
            "[{}] PANIC tại {}: {}\n",
            qsu_common::log::now_string(),
            loc,
            panic_payload_text(info.payload())
        );
        let mut targets = vec![exe_dir.join("CaiDatWinTuDong-panic.log")];
        if let Ok(g) = PANIC_LOG_DIR.lock() {
            if let Some(dir) = g.as_ref() {
                let _ = std::fs::create_dir_all(dir);
                targets.push(dir.join("CaiDatWinTuDong-panic.log"));
            }
        }
        for path in targets {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                use std::io::Write;
                let _ = f.write_all(line.as_bytes());
            }
        }
    }));
}

fn push_log(shared: &SharedRef, sender: &nwg::NoticeSender, msg: &str) {
    {
        let mut s = lock_shared(shared);
        s.log_lines.push_back(msg.to_string());
        while s.log_lines.len() > 400 {
            s.log_lines.pop_front();
        }
    }
    sender.notice();
}

/// Tạo logger ghi ra ổ Data (thư mục logs theo settings); sink đồng
/// thời đẩy từng dòng vào vùng log của giao diện.
fn make_logger(
    base_dir: &Path,
    logs_folder: &str,
    shared: &SharedRef,
    sender: nwg::NoticeSender,
) -> Arc<Logger> {
    let path = base_dir
        .join(logs_folder)
        .join(format!("install-{}.log", qsu_common::log::log_stamp()));
    let sh = shared.clone();
    let snd = sender;
    let sink = Box::new(move |line: &str| {
        {
            // Khoá chịu poison (lock_shared): một dòng log KHÔNG BAO
            // GIỜ được là nguyên nhân giết tiến trình — sink này chạy
            // ngay trong Logger::line trên mọi luồng, kể cả luồng
            // giao diện đang ở giữa handler.
            let mut s = lock_shared(&sh);
            s.log_lines.push_back(line.to_string());
            while s.log_lines.len() > 400 {
                s.log_lines.pop_front();
            }
        }
        snd.notice();
    });
    Arc::new(Logger::new(path, Some(sink)))
}

// ---------------------------------------------------------------------------
// Hộp thoại
// ---------------------------------------------------------------------------
fn info_box(title: &str, content: &str) {
    let p = nwg::MessageParams {
        title,
        content,
        buttons: nwg::MessageButtons::Ok,
        icons: nwg::MessageIcons::Info,
    };
    nwg::message(&p);
}

fn error_box(title: &str, content: &str) {
    let p = nwg::MessageParams {
        title,
        content,
        buttons: nwg::MessageButtons::Ok,
        icons: nwg::MessageIcons::Error,
    };
    nwg::message(&p);
}

fn confirm_box(title: &str, content: &str) -> bool {
    let p = nwg::MessageParams {
        title,
        content,
        buttons: nwg::MessageButtons::YesNo,
        icons: nwg::MessageIcons::Question,
    };
    nwg::message(&p) == nwg::MessageChoice::Yes
}

// ---------------------------------------------------------------------------
// Tiện ích kiểu cửa sổ (cờ mật khẩu / chỉ-đọc cho ô nhập)
// ---------------------------------------------------------------------------
fn set_edit_style(ctrl_hwnd: Option<HWND>, add_style: u32) {
    if let Some(hwnd) = ctrl_hwnd {
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            SetWindowLongPtrW(hwnd, GWL_STYLE, style | add_style as isize);
            InvalidateRect(hwnd, std::ptr::null(), 1);
        }
    }
}

// ---------------------------------------------------------------------------
// Checkbox cho ListView (Win32 thô, cùng cách module Tuỳ Chỉnh
// Windows/Dọn Rác dùng): NWG 1.0.13 không bọc sẵn LVS_EX_CHECKBOXES
// nên phải gửi message trực tiếp. Dùng cho danh sách phân vùng của
// đĩa đích ở chế độ rebuild (chọn phân vùng hệ thống sẽ xoá).
// ---------------------------------------------------------------------------
const LVM_SETEXTENDEDLISTVIEWSTYLE: u32 = 0x1036;
const LVM_GETITEMSTATE: u32 = 0x102C;
const LVM_SETITEMSTATE: u32 = 0x102B;
/// Giá trị ĐÚNG theo winapi/commctrl (LVS_EX_CHECKBOXES = 0x4).
/// Lưu ý: bốn module Tuỳ Chỉnh Windows / Dọn Rác / Backup-Restore
/// Driver / Sao Lưu Wifi từng dùng 0x400 (đó là LVS_EX_INFOTIP) nên
/// checkbox không hiện — đã sửa cả bốn về 0x4 ở v0.7.14.
const LVS_EX_CHECKBOXES: u32 = 0x0000_0004;
const LVIS_STATEIMAGEMASK: u32 = 0xF000;

/// Bật/tắt checkbox theo hàng cho ListView.
fn lv_set_checkboxes(lv: &nwg::ListView, on: bool) {
    if let Some(hwnd) = lv.handle.hwnd() {
        let v = if on { LVS_EX_CHECKBOXES } else { 0 };
        unsafe {
            SendMessageW(hwnd, LVM_SETEXTENDEDLISTVIEWSTYLE, LVS_EX_CHECKBOXES as WPARAM, v as LPARAM);
        }
    }
}

/// Hàng `row` có đang được tick không.
fn lv_item_checked(lv: &nwg::ListView, row: usize) -> bool {
    match lv.handle.hwnd() {
        Some(hwnd) => {
            let st = unsafe {
                SendMessageW(hwnd, LVM_GETITEMSTATE, row as WPARAM, LVIS_STATEIMAGEMASK as LPARAM)
            };
            (st >> 12) == 2
        }
        None => false,
    }
}

/// Đặt trạng thái tick của hàng `row`.
fn lv_set_checked(lv: &nwg::ListView, row: usize, checked: bool) {
    if let Some(hwnd) = lv.handle.hwnd() {
        let mut item: LVITEMW = unsafe { std::mem::zeroed() };
        item.stateMask = LVIS_STATEIMAGEMASK;
        item.state = if checked { 2 << 12 } else { 1 << 12 };
        unsafe {
            SendMessageW(hwnd, LVM_SETITEMSTATE, row as WPARAM, &item as *const LVITEMW as LPARAM);
        }
    }
}

/// Chèn một hàng vào ListView (mỗi cột một InsertListViewItem với
/// column_index — đúng API NWG 1.0.13 như lv_insert_row của module
/// Tuỳ Chỉnh Windows).
fn lv_insert_row(lv: &nwg::ListView, row: i32, cols: &[String]) {
    for (c, t) in cols.iter().enumerate() {
        lv.insert_item(nwg::InsertListViewItem {
            index: Some(row),
            column_index: c as i32,
            text: Some(t.clone()),
            ..Default::default()
        });
    }
}

// ---------------------------------------------------------------------------
// Giai đoạn 0 — kiểm tra tiền đề (đặc tả §2)
// ---------------------------------------------------------------------------
fn probe_bitlocker(os_letter: char) -> BitlockerState {
    // Get-BitLockerVolume trả tên enum tiếng Anh trên mọi ngôn ngữ
    // hiển thị — phân tích chắc chắn hơn output manage-bde đã bản địa hoá.
    let script = format!(
        "(Get-BitLockerVolume -MountPoint '{}:').VolumeStatus.ToString()",
        os_letter
    );
    if let Ok(out) = cmd::run_capture(
        "powershell",
        &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script],
    ) {
        return match out.stdout.trim() {
            "FullyDecrypted" => BitlockerState::Decrypted,
            "FullyEncrypted" => BitlockerState::Encrypted,
            "EncryptionInProgress" | "DecryptionInProgress" | "EncryptionSuspended"
            | "DecryptionSuspended" => BitlockerState::InProgress,
            _ => BitlockerState::Unknown,
        };
    }
    BitlockerState::Unknown
}

// ---------------------------------------------------------------------------
// Dò firmware (v0.7.7) — phán quyết UEFI/Legacy KHÔNG còn dựa vào một
// trick duy nhất. Bản cũ dùng sys::firmware_is_uefi() của qsu_common:
// chỉ suy luận qua ERROR_INVALID_FUNCTION của
// GetFirmwareEnvironmentVariableW và không ghi lại giá trị thô. Nếu
// nó phán sai trên một dòng máy ảo (thực địa DESKTOP-J8LPB75: wizard
// báo "UEFI (MBR)" trong khi đĩa hiện tên thiết bị kiểu Hyper-V
// Gen-1 "Virtual HD ATA Device") thì cả đường winload lẫn biến thể
// boot.sdi đều sai theo, mà log không có bằng chứng nào để đối
// chiếu. Nay dò bằng HAI API độc lập và giữ NGUYÊN giá trị thô của
// từng API để ghi vào log quét + log cài đặt:
//   A) GetFirmwareType (API có tài liệu, từ Windows 8): ghi ra
//      1=BIOS, 2=UEFI, 0=Unknown. Có kết quả dứt khoát (1/2) thì
//      phán quyết theo A.
//   B) Trick kinh điển GetFirmwareEnvironmentVariableW với tên
//      rỗng + GUID toàn 0: trên máy Legacy lời gọi này thất bại với
//      ERROR_INVALID_FUNCTION (1); trên máy UEFI nó thất bại bằng
//      lỗi khác (hoặc thành công). Chỉ dùng B khi A không kết luận.
// Khai báo extern trực tiếp tại wizard (kernel32) để không phải mở
// thêm feature winapi và không đụng qsu_common — các module khác
// giữ nguyên byte.
// ---------------------------------------------------------------------------
#[link(name = "kernel32")]
extern "system" {
    fn GetFirmwareType(firmware_type: *mut u32) -> i32;
    fn GetFirmwareEnvironmentVariableW(
        lp_name: *const u16,
        lp_guid: *const u16,
        p_buffer: *mut std::ffi::c_void,
        n_size: u32,
    ) -> u32;
    fn GetLastError() -> u32;
}

#[derive(Clone, Debug)]
struct FirmwareProbe {
    /// Giá trị GetFirmwareType ghi ra: 0=Unknown, 1=BIOS, 2=UEFI
    /// (None = lời gọi API thất bại).
    fw_type: Option<u32>,
    /// BOOL trả về của GetFirmwareType (false = API lỗi).
    fw_type_ok: bool,
    /// Return value của GetFirmwareEnvironmentVariableW.
    env_rc: u32,
    /// GetLastError ngay sau lời gọi env (1 = ERROR_INVALID_FUNCTION;
    /// chỉ có nghĩa khi env_rc == 0, các trường hợp còn lại để 0).
    env_err: u32,
    /// Phán quyết cuối cùng — mọi quyết định sau (đường winload,
    /// biến thể boot.sdi, stage.json boot_mode) đều theo giá trị này.
    uefi: bool,
}

fn probe_firmware() -> FirmwareProbe {
    unsafe {
        let mut t: u32 = 0;
        let ok = GetFirmwareType(&mut t);
        let (fw_type, fw_type_ok) = if ok != 0 { (Some(t), true) } else { (None, false) };
        let name: Vec<u16> = vec![0];
        let guid: Vec<u16> = "{00000000-0000-0000-0000-000000000000}"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut buf = [0u8; 1];
        let rc = GetFirmwareEnvironmentVariableW(
            name.as_ptr(),
            guid.as_ptr(),
            buf.as_mut_ptr() as *mut std::ffi::c_void,
            0,
        );
        let err = if rc == 0 { GetLastError() } else { 0 };
        let uefi = match fw_type {
            Some(2) => true,
            Some(1) => false,
            // A không kết luận (Unknown hoặc lỗi API) -> theo B:
            // Legacy chỉ khi lỗi đúng ERROR_INVALID_FUNCTION.
            _ => !(rc == 0 && err == 1),
        };
        FirmwareProbe {
            fw_type,
            fw_type_ok,
            env_rc: rc,
            env_err: err,
            uefi,
        }
    }
}

impl FirmwareProbe {
    /// Một dòng mô tả giá trị thô của cả hai API + phán quyết —
    /// ghi vào log quét (persist_scan_log), log cài đặt (exec_main)
    /// và log tại register_bcd để đối chiếu sau này.
    fn detail(&self) -> String {
        let a = match (self.fw_type_ok, self.fw_type) {
            (true, Some(2)) => "GetFirmwareType=2 (UEFI)".to_string(),
            (true, Some(1)) => "GetFirmwareType=1 (BIOS/Legacy)".to_string(),
            (true, Some(v)) => format!("GetFirmwareType={} (Unknown)", v),
            _ => "GetFirmwareType=lỗi API".to_string(),
        };
        format!(
            "{}; GetFirmwareEnvironmentVariableW rc={} lastError={} (1=ERROR_INVALID_FUNCTION); phán quyết: {}",
            a,
            self.env_rc,
            self.env_err,
            if self.uefi { "UEFI" } else { "Legacy/BIOS" }
        )
    }
}

fn spawn_preflight(shared: SharedRef, sender: nwg::NoticeSender) {
    std::thread::spawn(move || {
        let os_letter = sys::os_drive_letter();
        let drives = sys::list_fixed_drives();
        let marker_drives = drives
            .iter()
            .filter(|d| {
                Path::new(&format!("{}:\\", d.letter)).join(DATA_MARKER).exists()
            })
            .map(|d| d.letter)
            .collect();
        // Liệt kê đĩa KÈM chẩn đoán: đẩy từng dòng vào log wizard
        // ngay lúc quét — đĩa thiếu/hỏng ở đâu, đường dự phòng nào
        // đã chạy, đều phải nhìn thấy được trong log (bản cũ liệt kê
        // im lặng, đĩa biến mất không dấu vết).
        let enumeration = disk::enumerate_disks();
        for line in &enumeration.diag {
            push_log(&shared, &sender, &format!("[Đĩa] {}", line));
        }
        // Dò firmware bằng hai API độc lập và đẩy giá trị thô vào
        // log panel ngay lúc quét (v0.7.7) — phán quyết UEFI/Legacy
        // quyết định đường winload + biến thể boot.sdi về sau nên
        // bằng chứng của nó phải nằm trong mọi file log.
        let fw = probe_firmware();
        push_log(&shared, &sender, &format!("[Firmware] {}", fw.detail()));
        let pf = Preflight {
            os_letter,
            os: sys::os_info(),
            uefi: fw.uefi,
            firmware: fw,
            part: sys::disk_partition_style(0),
            bitlocker: probe_bitlocker(os_letter),
            power: sys::power_status(),
            oem_key: sys::oem_key_present(),
            drives,
            marker_drives,
            disks: enumeration.disks,
            disk_diag: enumeration.diag,
        };
        shared.lock().unwrap().preflight = Some(pf);
        sender.notice();
    });
}

// ---------------------------------------------------------------------------
// Giai đoạn 1 — nguồn ISO (đặc tả §3)
// ---------------------------------------------------------------------------
fn is_iso_file(p: &Path) -> bool {
    p.extension()
        .map(|e| e.eq_ignore_ascii_case("iso"))
        .unwrap_or(false)
}

fn scan_isos_on_drive(letter: char) -> Vec<IsoFile> {
    // Gốc ổ + thư mục con cấp 1 (đúng hành vi bản gốc)
    let root = PathBuf::from(format!("{}:\\", letter));
    let mut out = Vec::new();
    let mut subdirs = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() && is_iso_file(&p) {
                out.push(IsoFile { size: std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0), path: p });
            } else if p.is_dir() {
                subdirs.push(p);
            }
        }
    }
    for d in subdirs {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_file() && is_iso_file(&p) {
                    out.push(IsoFile {
                        size: std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0),
                        path: p,
                    });
                }
            }
        }
    }
    out
}

/// Quét ISO song song: một luồng cho mỗi ổ đĩa.
fn spawn_iso_scan(shared: SharedRef, sender: nwg::NoticeSender, drives: Vec<char>) {
    std::thread::spawn(move || {
        let acc = Arc::new(Mutex::new(Vec::new()));
        let mut handles = Vec::new();
        for letter in drives {
            let acc = acc.clone();
            handles.push(std::thread::spawn(move || {
                let found = scan_isos_on_drive(letter);
                acc.lock().unwrap().extend(found);
            }));
        }
        for h in handles {
            let _ = h.join();
        }
        let mut v = acc.lock().unwrap().clone();
        v.sort_by(|a, b| {
            a.path
                .to_string_lossy()
                .to_lowercase()
                .cmp(&b.path.to_string_lossy().to_lowercase())
        });
        shared.lock().unwrap().iso_list = Some(v);
        sender.notice();
    });
}

fn spawn_catalogue_fetch(shared: SharedRef, sender: nwg::NoticeSender, url: String) {
    std::thread::spawn(move || {
        let result = (|| -> Result<Vec<CatalogueImage>, String> {
            if url.trim().is_empty() {
                return Err(
                    "settings.json chưa có catalogueUrl — hãy cấu hình địa chỉ kho ISO của bạn."
                        .to_string(),
                );
            }
            let bytes = http::get_bytes(&url)?;
            let text = String::from_utf8_lossy(&bytes);
            let root = json::parse(&text)?;
            let arr = root
                .get("images")
                .and_then(|v| v.as_arr())
                .ok_or_else(|| "catalogue thiếu mảng \"images\"".to_string())?;
            let mut out = Vec::new();
            for item in arr {
                out.push(CatalogueImage {
                    name: item.str_or("name", "(không tên)"),
                    edition: item.str_or("edition", ""),
                    url: item.str_or("url", ""),
                    size: item.u64_or("size", 0),
                    sha256: item.str_or("sha256", ""),
                });
            }
            Ok(out)
        })();
        shared.lock().unwrap().catalogue = Some(result);
        sender.notice();
    });
}

/// Luồng nền "Lấy Chat ID" của tab "Thông báo" (v0.7.27): bọc
/// `catch_unwind` và LUÔN ghi kết quả vào `Shared::notify_done`
/// (kể cả khi panic) để `refresh` nhả khoá bốn nút của tab ở mọi
/// lối ra — xem `NotifyDone`.
fn spawn_notify_fetch_chat_id(shared: SharedRef, sender: nwg::NoticeSender, token: String) {
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            qsu_common::notify::telegram_fetch_chat_id(&token)
        }));
        let result = match result {
            Ok(r) => r,
            Err(p) => Err(format!("lỗi nội bộ: {}", panic_payload_text(&*p))),
        };
        lock_shared(&shared).notify_done = Some(NotifyDone::ChatId(result));
        sender.notice();
    });
}

/// Luồng nền "Gửi thử Telegram" (v0.7.27) — cùng kỷ luật khoá
/// nút như `spawn_notify_fetch_chat_id`.
fn spawn_notify_test_telegram(
    shared: SharedRef,
    sender: nwg::NoticeSender,
    token: String,
    chat_id: String,
) {
    std::thread::spawn(move || {
        let text = qsu_common::notify::text_test(&sys::computer_name());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            qsu_common::notify::telegram_send(&token, &chat_id, &text)
        }));
        let result = match result {
            Ok(r) => r,
            Err(p) => Err(format!("lỗi nội bộ: {}", panic_payload_text(&*p))),
        };
        lock_shared(&shared).notify_done = Some(NotifyDone::Test("Telegram", result));
        sender.notice();
    });
}

/// Luồng nền "Gửi thử Zalo" qua relay (v0.7.27) — cùng kỷ luật
/// khoá nút như `spawn_notify_fetch_chat_id`.
fn spawn_notify_test_zalo(
    shared: SharedRef,
    sender: nwg::NoticeSender,
    url: String,
    secret: String,
) {
    std::thread::spawn(move || {
        let text = qsu_common::notify::text_test(&sys::computer_name());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            qsu_common::notify::relay_send(&url, &secret, &text)
        }));
        let result = match result {
            Ok(r) => r,
            Err(p) => Err(format!("lỗi nội bộ: {}", panic_payload_text(&*p))),
        };
        lock_shared(&shared).notify_done = Some(NotifyDone::Test("Zalo", result));
        sender.notice();
    });
}

fn spawn_download(
    shared: SharedRef,
    sender: nwg::NoticeSender,
    img: CatalogueImage,
    dest_dir: PathBuf,
) {
    std::thread::spawn(move || {
        let finish = |shared: &SharedRef,
                      sender: &nwg::NoticeSender,
                      r: Result<PathBuf, String>| {
            shared.lock().unwrap().dl_done = Some(r);
            sender.notice();
        };
        if img.sha256.trim().is_empty() {
            finish(
                &shared,
                &sender,
                Err(format!(
                    "Mục '{}' trong catalogue không có sha256 — từ chối tải (đặc tả §3 yêu cầu kiểm hash bắt buộc).",
                    img.name
                )),
            );
            return;
        }
        let raw_name = img
            .url
            .split('?')
            .next()
            .unwrap_or("")
            .rsplit('/')
            .next()
            .unwrap_or("windows.iso")
            .to_string();
        let file_name = if raw_name.is_empty() { "windows.iso".to_string() } else { raw_name };
        let dest = dest_dir.join(file_name);
        let sh = shared.clone();
        let snd = sender.clone();
        let mut last_pct: u64 = u64::MAX;
        let dl = http::download_file(&img.url, &dest, |done, total| {
            if total > 0 {
                let pct = done.saturating_mul(100) / total;
                if pct != last_pct {
                    last_pct = pct;
                    sh.lock().unwrap().dl_progress = Some((done, total));
                    snd.notice();
                }
            }
        });
        if let Err(e) = dl {
            finish(&shared, &sender, Err(format!("Tải ISO thất bại: {}", e)));
            return;
        }
        // Kiểm SHA-256 streaming (không nạp cả file vào RAM).
        // hash_file nhận &dyn Fn nên dùng Cell cho bộ đếm phần trăm.
        let sh = shared.clone();
        let snd = sender.clone();
        let last_pct = std::cell::Cell::new(u64::MAX);
        let cb = move |done: u64, total: u64| {
            if total > 0 {
                let pct = done.saturating_mul(100) / total;
                if pct != last_pct.get() {
                    last_pct.set(pct);
                    sh.lock().unwrap().dl_progress = Some((done, total));
                    snd.notice();
                }
            }
        };
        match sha256::hash_file(&dest, Some(&cb)) {
            Ok(h) if h.eq_ignore_ascii_case(img.sha256.trim()) => {
                finish(&shared, &sender, Ok(dest));
            }
            Ok(h) => {
                let _ = std::fs::remove_file(&dest);
                // Cắt tiền tố bằng chars (KHÔNG cắt theo byte): sha256 lấy
                // từ catalogue là dữ liệu ngoài, có thể chứa ký tự đa byte
                // — cắt theo byte tại vị trí không đúng biên ký tự sẽ
                // panic và giết luồng tải, khiến giao diện kẹt vĩnh viễn.
                let h_pre: String = h.chars().take(16).collect();
                let c_pre: String = img.sha256.chars().take(16).collect();
                finish(
                    &shared,
                    &sender,
                    Err(format!(
                        "SHA-256 không khớp (tải về: {}…, catalogue: {}…). Đã xoá file tải lỗi.",
                        h_pre, c_pre
                    )),
                );
            }
            Err(e) => finish(&shared, &sender, Err(format!("Không đọc được file đã tải để kiểm hash: {}", e))),
        }
    });
}

// ---------------------------------------------------------------------------
// Giai đoạn 2 — mount ISO + đọc phiên bản (đặc tả §4)
// ---------------------------------------------------------------------------
// Mount ISO BA LỚP (v0.7.17). Bối cảnh thực địa: trên máy test chạy
// Windows lite, bấm "Đọc ISO" với ISO gốc của Microsoft thì hộp
// thoại báo "Mount-DiskImage không trả ký tự ổ đĩa (output: )" —
// output RỖNG cả hai luồng, đọc ISO bị huỷ mà không ai biết lý do:
// lệnh PowerShell duy nhất của các bản trước nuốt mất lỗi thật
// (cmdlet Storage trên bản lite không mount được nhưng lỗi không
// thành terminating nên mã thoát vẫn 0 và không chữ nào lọt ra).
// Từ v0.7.17 đường mount có ba lớp, lớp sau cứu lớp trước:
//   1. DÙNG LẠI: ISO đã gắn sẵn (lần đọc trước, Explorer mount,
//      tool khác...) → lấy ký tự qua Get-DiskImage Attached.
//   2. POWERSHELL: Mount-DiskImage với $ErrorActionPreference =
//      'Stop' — lỗi cmdlet thành terminating, mã thoát khác 0 và
//      stderr mang NGUYÊN VĂN lỗi; hộp thoại hiện đúng lỗi đó.
//   3. VIRT DISK API: OpenVirtualDisk (storage-type ISO) +
//      AttachVirtualDisk đọc-chỉ-đọc gọi thẳng từ Rust, không qua
//      PowerShell — cứu các máy mà tầng cmdlet Storage hỏng/thiếu.
// Ký tự ổ của lớp 3 dò bằng HIỆU TẬP VOLUME: chụp mặt nạ ký tự ổ
// trước attach, ký tự DVD MỚI xuất hiện sau attach là ISO.
// Gỡ mount ĐỐI XỨNG theo cơ chế đã mount, nhờ SỔ ĐĂNG KÝ mount
// của tiến trình: mount PowerShell → Dismount-DiskImage; mount
// VirtDisk → DetachVirtualDisk bằng chính handle đã mở + đóng
// handle (kèm Dismount-DiskImage dọn dẹp vô hại). Mọi điểm trước
// đây gọi dismount_iso giữ nguyên vị trí.

/// Cơ chế đã mount một ISO — quyết định cách gỡ mount đối xứng.
#[derive(Clone, Copy, PartialEq)]
enum IsoMountKind {
    PowerShell,
    VirtDisk,
}

struct IsoMountRec {
    kind: IsoMountKind,
    letter: char,
    /// HANDLE VirtDisk lưu dạng usize (HANDLE thô không Send —
    /// sổ chỉ được chạm qua Mutex nên an toàn); 0 nếu mount bằng
    /// PowerShell.
    vdisk_handle: usize,
}

/// Sổ đăng ký các ISO đang mount bởi tiến trình wizard: khoá là
/// đường dẫn viết thường (Windows không phân biệt hoa thường).
fn iso_mounts() -> &'static Mutex<Vec<(String, IsoMountRec)>> {
    static MOUNTS: OnceLock<Mutex<Vec<(String, IsoMountRec)>>> = OnceLock::new();
    MOUNTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn iso_mount_key(iso: &Path) -> String {
    iso.to_string_lossy().to_lowercase()
}

fn register_iso_mount(iso: &Path, rec: IsoMountRec) {
    let key = iso_mount_key(iso);
    let mut mounts = iso_mounts().lock().unwrap();
    mounts.retain(|(k, _)| *k != key);
    mounts.push((key, rec));
}

fn take_iso_mount(iso: &Path) -> Option<IsoMountRec> {
    let key = iso_mount_key(iso);
    let mut mounts = iso_mounts().lock().unwrap();
    let pos = mounts.iter().position(|(k, _)| *k == key)?;
    Some(mounts.remove(pos).1)
}

/// Chạy một script PowerShell thu CẢ stdout lẫn stderr (mọi lệnh
/// trong đường mount ISO đi qua đây — lỗi thật không bị nuốt).
fn run_powershell(script: &str) -> Result<cmd::CmdOut, String> {
    cmd::run_capture(
        "powershell",
        &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script],
    )
    .map_err(|e| format!("không chạy được PowerShell: {}", e))
}

/// Trích ký tự ổ đĩa từ output Get-Volume: ký tự chữ cái đầu tiên
/// (heuristic cũ đã chứng minh đúng ở thực địa — giữ nguyên).
fn letter_from_output(s: &str) -> Option<char> {
    s.trim()
        .chars()
        .find(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_uppercase())
}

/// Gộp một lần chạy PowerShell thất bại thành đoạn lỗi ĐỌC ĐƯỢC:
/// mã thoát (thập phân + hex) + stderr nguyên văn (nơi PowerShell
/// ghi lỗi cmdlet); stderr rỗng thì dùng stdout; cả hai rỗng thì
/// nói rõ là không có thêm chi tiết — KHÔNG bao giờ còn "(output: )"
/// trống trơn như bản cũ. Chi tiết dài thì giữ phần CUỐI (lỗi nằm
/// ở cuối), cắt theo biên ký tự.
fn ps_failure_text(what: &str, out: &cmd::CmdOut) -> String {
    let mut detail = out.stderr.trim().to_string();
    if detail.is_empty() {
        detail = out.stdout.trim().to_string();
    }
    let chars: Vec<char> = detail.chars().collect();
    if chars.len() > 700 {
        detail = chars[chars.len() - 700..].iter().collect();
    }
    if detail.is_empty() {
        format!(
            "{} thất bại ({}) — PowerShell không in thêm chi tiết nào.",
            what,
            fmt_exit_code(out.code)
        )
    } else {
        format!("{} thất bại ({}): {}", what, fmt_exit_code(out.code), detail)
    }
}

/// Lớp 1 — ISO đã gắn sẵn thì dùng lại ký tự ổ, khỏi mount lại.
fn iso_mounted_letter(iso: &Path) -> Option<char> {
    let path_s = iso.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$img = Get-DiskImage -ImagePath '{}' -ErrorAction SilentlyContinue; if ($img -and $img.Attached) {{ $v = $img | Get-Volume -ErrorAction SilentlyContinue; if ($v) {{ $v.DriveLetter }} }}",
        path_s
    );
    let out = run_powershell(&script).ok()?;
    if out.code != Some(0) {
        return None;
    }
    letter_from_output(&out.stdout)
}

/// Lớp 2 — mount bằng PowerShell Mount-DiskImage, lỗi ép thành
/// terminating để mã thoát + stderr luôn mang lỗi thật.
fn mount_iso_powershell(iso: &Path) -> Result<char, String> {
    let path_s = iso.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference = 'Stop'; $v = Mount-DiskImage -ImagePath '{}' -PassThru | Get-Volume; if ($v -and $v.DriveLetter) {{ [string]$v.DriveLetter }} else {{ throw 'Mount-DiskImage không gán được ký tự ổ đĩa cho ISO.' }}",
        path_s
    );
    let out = run_powershell(&script)?;
    if out.code == Some(0) {
        if let Some(l) = letter_from_output(&out.stdout) {
            return Ok(l);
        }
    }
    Err(ps_failure_text("Mount-DiskImage", &out))
}

/// FFI tối thiểu tới VirtDisk API (virtdisk.dll) — winapi 0.3
/// không có module virtdisk nên khai báo tay đúng nguyên mẫu của
/// Windows SDK. Chỉ phục vụ lớp mount dự phòng (lớp 3).
mod virtdisk {
    #![allow(dead_code)]
    use std::ffi::c_void;
    use winapi::shared::guiddef::GUID;
    use winapi::shared::minwindef::{DWORD, ULONG};
    use winapi::um::winnt::{HANDLE, PCWSTR};

    /// VIRTUAL_STORAGE_TYPE_DEVICE_ISO.
    pub const STORAGE_DEVICE_ISO: ULONG = 1;
    /// GUID nhà cung cấp Microsoft của VIRTUAL_STORAGE_TYPE
    /// ({EC984AEC-A0F9-47E9-901F-71415A66345B}).
    pub const VENDOR_MICROSOFT: GUID = GUID {
        Data1: 0xEC98_4AEC,
        Data2: 0xA0F9,
        Data3: 0x47E9,
        Data4: [0x90, 0x1F, 0x71, 0x41, 0x5A, 0x66, 0x34, 0x5B],
    };
    pub const OPEN_VERSION_1: DWORD = 1;
    pub const OPEN_RW_DEPTH_DEFAULT: ULONG = 1;
    pub const ATTACH_VERSION_1: DWORD = 1;
    /// VIRTUAL_DISK_ACCESS_ATTACH_RO (0x00010000) |
    /// VIRTUAL_DISK_ACCESS_DETACH (0x00040000): handle đủ quyền
    /// gắn đọc-chỉ-đọc bây giờ và tháo bằng chính nó về sau.
    pub const ACCESS_ATTACH_RO_DETACH: DWORD = 0x0005_0000;
    /// ATTACH_VIRTUAL_DISK_FLAG_READ_ONLY (0x1) |
    /// ATTACH_VIRTUAL_DISK_FLAG_PERMANENT_LIFETIME (0x4): ảnh gắn
    /// sống tới khi DetachVirtualDisk tường minh — không tự tháo
    /// nếu handle bị đóng sớm; wizard giữ handle trong sổ đăng ký
    /// và tháo đúng ở các điểm gỡ mount như trước.
    pub const ATTACH_FLAGS_ISO: DWORD = 0x0000_0005;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct VirtualStorageType {
        pub device_id: ULONG,
        pub vendor_id: GUID,
    }
    /// OPEN_VIRTUAL_DISK_PARAMETERS bản Version 1 (Version +
    /// Version1.RWDepth).
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct OpenParameters {
        pub version: DWORD,
        pub rw_depth: ULONG,
    }
    /// ATTACH_VIRTUAL_DISK_PARAMETERS bản Version 1 (Version +
    /// Version1.Reserved).
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct AttachParameters {
        pub version: DWORD,
        pub reserved: ULONG,
    }

    #[link(name = "virtdisk")]
    extern "system" {
        pub fn OpenVirtualDisk(
            storage_type: *const VirtualStorageType,
            path: PCWSTR,
            access_mask: DWORD,
            flags: DWORD,
            parameters: *const OpenParameters,
            handle: *mut HANDLE,
        ) -> DWORD;
        pub fn AttachVirtualDisk(
            handle: HANDLE,
            security_descriptor: *mut c_void,
            flags: DWORD,
            provider_flags: DWORD,
            parameters: *const AttachParameters,
            overlapped: *mut c_void,
        ) -> DWORD;
        pub fn DetachVirtualDisk(handle: HANDLE, flags: DWORD, provider_flags: DWORD) -> DWORD;
    }
}

/// Mặt nạ ký tự ổ hiện có (bit i = ổ 'A'+i) — chụp trước/sau
/// attach để tìm volume DVD mới của ISO.
fn logical_drives_mask() -> u32 {
    unsafe { winapi::um::fileapi::GetLogicalDrives() }
}

/// Ký tự ổ là ổ CD/DVD (DRIVE_CDROM = 5) và gốc ổ truy cập được.
fn is_cdrom_letter(letter: char) -> bool {
    let root: Vec<u16> = format!("{}:\\", letter).encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { winapi::um::fileapi::GetDriveTypeW(root.as_ptr()) == 5 }
}

/// Lớp 3 — mount bằng VirtDisk API gốc, không qua PowerShell.
/// Thành công trả (ký tự ổ, HANDLE dạng usize) để sổ đăng ký giữ
/// handle cho bước gỡ mount đối xứng.
fn mount_iso_virtdisk(iso: &Path) -> Result<(char, usize), String> {
    use winapi::um::handleapi::CloseHandle;
    let before = logical_drives_mask();
    let wide: Vec<u16> = iso
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let storage = virtdisk::VirtualStorageType {
        device_id: virtdisk::STORAGE_DEVICE_ISO,
        vendor_id: virtdisk::VENDOR_MICROSOFT,
    };
    let open_params = virtdisk::OpenParameters {
        version: virtdisk::OPEN_VERSION_1,
        rw_depth: virtdisk::OPEN_RW_DEPTH_DEFAULT,
    };
    let mut handle: winapi::um::winnt::HANDLE = std::ptr::null_mut();
    let rc = unsafe {
        virtdisk::OpenVirtualDisk(
            &storage,
            wide.as_ptr(),
            virtdisk::ACCESS_ATTACH_RO_DETACH,
            0,
            &open_params,
            &mut handle,
        )
    };
    if rc != 0 || handle.is_null() {
        return Err(format!(
            "OpenVirtualDisk thất bại (mã lỗi Windows {} — 0x{:08X}). File ISO có thể hỏng/không phải ảnh đĩa hợp lệ, hoặc máy thiếu thành phần Virtual Disk.",
            rc, rc
        ));
    }
    let attach_params = virtdisk::AttachParameters {
        version: virtdisk::ATTACH_VERSION_1,
        reserved: 0,
    };
    let rc = unsafe {
        virtdisk::AttachVirtualDisk(
            handle,
            std::ptr::null_mut(),
            virtdisk::ATTACH_FLAGS_ISO,
            0,
            &attach_params,
            std::ptr::null_mut(),
        )
    };
    if rc != 0 {
        unsafe {
            CloseHandle(handle);
        }
        return Err(format!(
            "AttachVirtualDisk thất bại (mã lỗi Windows {} — 0x{:08X}).",
            rc, rc
        ));
    }
    // Dò ký tự ổ MỚI: volume DVD của ISO hiện ra sau attach ít
    // lâu — thăm dò tối đa ~10 giây. Ưu tiên ký tự có sẵn
    // sources\install.* (chính là ISO cài Windows); không có thì
    // lấy ký tự DVD mới đầu tiên (bước đọc edition sẽ tự báo nếu
    // ISO thiếu file cài đặt, như trước).
    let mut fallback: Option<char> = None;
    for _ in 0..40 {
        let now = logical_drives_mask();
        let new_bits = now & !before;
        for i in 0..26u32 {
            if new_bits & (1u32 << i) != 0 {
                let letter = (b'A' + i as u8) as char;
                if is_cdrom_letter(letter) {
                    if find_image_file(letter).is_some() {
                        return Ok((letter, handle as usize));
                    }
                    if fallback.is_none() {
                        fallback = Some(letter);
                    }
                }
            }
        }
        if let Some(l) = fallback {
            // Cho thêm vài vòng thăm dò ngắn để sources kịp hiện
            // trước khi chốt ký tự dự phòng.
            std::thread::sleep(Duration::from_millis(250));
            if find_image_file(l).is_some() {
                return Ok((l, handle as usize));
            }
            return Ok((l, handle as usize));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    unsafe {
        virtdisk::DetachVirtualDisk(handle, 0, 0);
        CloseHandle(handle);
    }
    Err("Đã attach ISO bằng VirtDisk nhưng không thấy ký tự ổ DVD mới nào xuất hiện sau 10 giây.".to_string())
}

/// Mount ISO (ba lớp — xem khối chú thích ở đầu mục). Thành công
/// trả ký tự ổ của volume ISO; mount được ghi vào sổ đăng ký để
/// `dismount_iso` gỡ đúng cơ chế.
fn mount_iso(iso: &Path) -> Result<char, String> {
    // Sổ đăng ký còn giữ mount của chính tiến trình này (đọc lại
    // cùng ISO) và gốc ổ còn truy cập được → dùng lại ngay.
    if let Some(rec) = take_iso_mount(iso) {
        if PathBuf::from(format!("{}:\\", rec.letter)).exists() {
            let letter = rec.letter;
            register_iso_mount(iso, rec);
            return Ok(letter);
        }
        // Mount cũ đã chết (ai đó tháo ngoài) — gỡ nốt phần cứng
        // còn treo rồi đi tiếp các lớp dưới.
        if rec.kind == IsoMountKind::VirtDisk && rec.vdisk_handle != 0 {
            unsafe {
                let h = rec.vdisk_handle as winapi::um::winnt::HANDLE;
                virtdisk::DetachVirtualDisk(h, 0, 0);
                winapi::um::handleapi::CloseHandle(h);
            }
        }
    }
    // Lớp 1: đã gắn sẵn ở tầng hệ thống.
    if let Some(letter) = iso_mounted_letter(iso) {
        register_iso_mount(
            iso,
            IsoMountRec { kind: IsoMountKind::PowerShell, letter, vdisk_handle: 0 },
        );
        return Ok(letter);
    }
    // Lớp 2: PowerShell Mount-DiskImage (lỗi thật được giữ lại).
    let ps_err = match mount_iso_powershell(iso) {
        Ok(letter) => {
            register_iso_mount(
                iso,
                IsoMountRec { kind: IsoMountKind::PowerShell, letter, vdisk_handle: 0 },
            );
            return Ok(letter);
        }
        Err(e) => e,
    };
    // Lớp 3: VirtDisk API gốc.
    match mount_iso_virtdisk(iso) {
        Ok((letter, handle)) => {
            register_iso_mount(
                iso,
                IsoMountRec { kind: IsoMountKind::VirtDisk, letter, vdisk_handle: handle },
            );
            Ok(letter)
        }
        Err(vd_err) => Err(format!(
            "Không mount được ISO bằng cả hai cơ chế.\n— PowerShell: {}\n— VirtDisk (API gốc của Windows): {}\nHãy kiểm tra file ISO có hỏng/không, đường dẫn có truy cập được không, rồi thử lại.",
            ps_err, vd_err
        )),
    }
}

/// Gỡ mount ISO — ĐỐI XỨNG với cơ chế đã mount (xem sổ đăng ký):
/// VirtDisk thì DetachVirtualDisk + đóng handle; PowerShell (hoặc
/// không có trong sổ — mount từ tiến trình khác/lần chạy trước)
/// thì Dismount-DiskImage như các bản trước. Lỗi gỡ mount không
/// bao giờ chặn luồng chính (giữ đúng hành vi cũ).
fn dismount_iso(iso: &Path) {
    let rec = take_iso_mount(iso);
    if let Some(rec) = &rec {
        if rec.kind == IsoMountKind::VirtDisk && rec.vdisk_handle != 0 {
            unsafe {
                let h = rec.vdisk_handle as winapi::um::winnt::HANDLE;
                virtdisk::DetachVirtualDisk(h, 0, 0);
                winapi::um::handleapi::CloseHandle(h);
            }
        }
    }
    // Dismount-DiskImage chạy cho mount PowerShell; với mount
    // VirtDisk nó chỉ là dọn dẹp vô hại (ảnh đã detach ở trên thì
    // lệnh này không còn gì để gỡ và được SilentlyContinue bỏ qua).
    let path_s = iso.to_string_lossy().replace('\'', "''");
    let script = format!(
        "Dismount-DiskImage -ImagePath '{}' -ErrorAction SilentlyContinue",
        path_s
    );
    let _ = run_powershell(&script);
}

/// Tìm file image trong ISO đã mount: install.wim / .esd / .swm
fn find_image_file(mounted_letter: char) -> Option<(PathBuf, String)> {
    let sources = PathBuf::from(format!("{}:\\sources", mounted_letter));
    for name in ["install.wim", "install.esd", "install.swm"] {
        let p = sources.join(name);
        if p.exists() {
            return Some((p, name.to_string()));
        }
    }
    None
}

fn dism_wim_info(wim: &Path, index: Option<u32>) -> Result<String, String> {
    let wim_arg = format!("/WimFile:{}", wim.to_string_lossy());
    let mut args: Vec<String> = vec![
        "/English".to_string(),
        "/Get-WimInfo".to_string(),
        wim_arg,
    ];
    if let Some(i) = index {
        args.push(format!("/Index:{}", i));
    }
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = cmd::run_capture("dism", &arg_refs)
        .map_err(|e| format!("không chạy được DISM: {}", e))?;
    if out.code != Some(0) {
        // DISM in lỗi của chính nó vào stdout (stderr thường rỗng) —
        // gộp cả hai nguồn và trích dòng lỗi tiêu biểu, mã thoát in
        // dạng thập phân + hex thay cho debug repr (mã {:?}).
        let combined = format!("{}\n{}", out.stdout, out.stderr);
        return Err(cmd_fail_msg("DISM /Get-WimInfo", out.code, &combined));
    }
    Ok(out.stdout)
}

/// Mã thoát của lệnh ngoài dạng đọc được cho người dùng:
/// "mã -1051655919 (0xC1510111)". KHÔNG bao giờ in debug repr kiểu
/// `Ok(Some(...))` / `Some(...)` ra hộp thoại — thực địa v0.7.5 lỗi
/// mount DISM hiện nguyên repr Rust cho kỹ thuật viên đọc.
fn fmt_exit_code(code: Option<i32>) -> String {
    match code {
        Some(c) => format!("mã {} (0x{:08X})", c, c as u32),
        None => "không có mã thoát (tiến trình bị kết thúc bất thường)".to_string(),
    }
}

/// Dòng lỗi tiêu biểu nhất trong output của một công cụ ngoài. Các
/// công cụ (DISM, bcdedit, manage-bde...) tự in lỗi của chúng vào
/// output — trích dòng đó đưa lên hộp thoại thay vì bắt người dùng
/// đi tìm trong log: ưu tiên dòng cuối chứa mã hex 0x..., rồi dòng
/// chứa "Error"/"Lỗi", cuối cùng là dòng không rỗng cuối cùng.
fn last_error_line(output: &str) -> String {
    let lines: Vec<&str> = output
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    for l in lines.iter().rev() {
        if l.to_ascii_lowercase().contains("0x") {
            return l.to_string();
        }
    }
    for l in lines.iter().rev() {
        if l.contains("Error") || l.contains("error") || l.contains("Lỗi") {
            return l.to_string();
        }
    }
    lines.last().map(|l| l.to_string()).unwrap_or_default()
}

/// Thông điệp lỗi lệnh ngoài hoàn chỉnh cho hộp thoại/log panel:
/// việc gì thất bại + mã thoát (thập phân + hex) + dòng lỗi của
/// chính công cụ. Output đầy đủ vẫn nằm trong log như trước.
fn cmd_fail_msg(what: &str, code: Option<i32>, output: &str) -> String {
    let mut msg = format!("{} thất bại ({})", what, fmt_exit_code(code));
    let line = last_error_line(output);
    if !line.is_empty() {
        msg.push_str(&format!(". Lỗi từ công cụ: {}", line));
    }
    msg
}

/// Xoá thuộc tính read-only của MỘT file, trả true nếu đã phải đổi.
/// ★ Gốc rễ thực địa v0.7.5: file trên ISO/DVD luôn mang thuộc tính
/// read-only và `std::fs::copy` giữ nguyên thuộc tính ở file đích —
/// DISM từ chối mount một WIM read-only để SỬA với lỗi 0xC1510111
/// "You do not have permissions to mount and modify this image.
/// Verify that you have Read/Write permissions..." (winpe.wim chép
/// từ ISO mount F: vào staging bị đúng lỗi này). KHÔNG phải lỗi nén
/// solid như phỏng đoán ban đầu.
fn clear_readonly_file(path: &Path) -> bool {
    if let Ok(meta) = std::fs::metadata(path) {
        let perms = meta.permissions();
        if perms.readonly() {
            let mut p = perms;
            p.set_readonly(false);
            return std::fs::set_permissions(path, p).is_ok();
        }
    }
    false
}

/// Xoá read-only cho một file hoặc cả cây thư mục (đệ quy), trả số
/// file đã đổi thuộc tính. Dùng cho mọi thứ chép từ ISO vào staging
/// (cây sources, boot.wim...) để các bước sau (DISM mount, ghi đè,
/// xoá làm lại) không vấp thuộc tính của đĩa quang.
fn clear_readonly_tree(path: &Path) -> usize {
    let mut n = 0;
    if path.is_dir() {
        if let Ok(rd) = std::fs::read_dir(path) {
            for e in rd.flatten() {
                n += clear_readonly_tree(&e.path());
            }
        }
    } else if clear_readonly_file(path) {
        n = 1;
    }
    n
}

/// Ghi một khối output DISM vào log (từng dòng không rỗng, tiền tố
/// "  | " như run_logged) — dùng cho các output cần chẩn đoán đầy đủ.
fn log_dism_output(log: &Arc<Logger>, text: &str) {
    for l in text.lines() {
        let t = l.trim_end();
        if !t.trim().is_empty() {
            log.line(&format!("  | {}", t));
        }
    }
}

/// Chẩn đoán WinPE WIM TRƯỚC khi mount: chạy DISM /Get-WimInfo và
/// ghi TOÀN BỘ kết quả vào log (danh sách index/tên/dung lượng;
/// chi tiết của index được chọn có thêm các trường DISM báo — gồm
/// cả kiểu nén nếu DISM liệt kê), rồi chọn index WinPE một cách có
/// chủ đích và ghi rõ lý do chọn: ưu tiên "Boot Index" do DISM
/// báo; không có thì chọn theo TÊN ảnh — index có tên chứa
/// "Windows PE" và không chứa "Setup" (v0.7.8: quy ước cũ "index 2
/// là ảnh WinPE" là SAI với boot.wim chuẩn lẫn ISO mod thực địa —
/// index 1 mới là "Microsoft Windows PE", index 2 là "Microsoft
/// Windows Setup"; v0.7.6/0.7.7 đã mount/inject/xuất nhầm ảnh Setup
/// nên cờ /Bootable không bao giờ gắn đúng ảnh WinPE); cuối cùng
/// mới rơi về index 1 nếu có, rồi index đầu tiên liệt kê được.
/// /Get-WimInfo lỗi thì ghi lỗi đọc được và tạm dùng index 1 —
/// bước mount/export phía sau sẽ cho kết luận cuối.
fn probe_winpe_wim(log: &Arc<Logger>, wim: &Path) -> u32 {
    log.line(&format!(
        "--- Chẩn đoán WinPE WIM (DISM /Get-WimInfo): {}",
        wim.display()
    ));
    if let Ok(meta) = std::fs::metadata(wim) {
        log.line(&format!("Kích thước file WIM trên đĩa: {} byte.", meta.len()));
    }
    let info = match dism_wim_info(wim, None) {
        Ok(t) => t,
        Err(e) => {
            log.line(&format!(
                "CẢNH BÁO: {} — tạm chọn index 1, bước mount/export sẽ kết luận.",
                e
            ));
            return 1;
        }
    };
    log_dism_output(log, &info);
    let kv = parse_kv(&info);
    // Ghép từng index với tên ảnh của nó theo thứ tự DISM liệt kê
    // (summary in lần lượt "Index : N" rồi "Name : ..." cho mỗi ảnh).
    let mut named: Vec<(u32, String)> = Vec::new();
    let mut cur: Option<u32> = None;
    for line in info.lines() {
        if let Some(pos) = line.find(':') {
            let k = line[..pos].trim();
            let v = line[pos + 1..].trim();
            if k.eq_ignore_ascii_case("Index") {
                cur = v.parse::<u32>().ok();
            } else if k.eq_ignore_ascii_case("Name") {
                if let Some(i) = cur {
                    named.push((i, v.to_string()));
                }
            }
        }
    }
    let name_of = |idx: u32| -> String {
        named
            .iter()
            .find(|(i, _)| *i == idx)
            .map(|(_, n)| n.clone())
            .unwrap_or_default()
    };
    if let Ok(v) = kv_get(&kv, "Boot Index").parse::<u32>() {
        if v > 0 {
            log.line(&format!(
                "Chọn index {} ({}) — theo 'Boot Index' do DISM báo cho WIM này.",
                v,
                name_of(v)
            ));
            log_wim_index_detail(log, wim, v);
            return v;
        }
    }
    let indexes: Vec<u32> = kv
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("Index"))
        .filter_map(|(_, v)| v.parse().ok())
        .collect();
    // Chọn theo tên: ảnh WinPE thật (không phải ảnh Setup chạy
    // trong WinPE). "Windows Setup" cũng mang Edition WindowsPE
    // nên chỉ lọc theo tên mới phân biệt được.
    let by_name = named.iter().find(|(_, n)| {
        let l = n.to_ascii_lowercase();
        l.contains("windows pe") && !l.contains("setup")
    });
    let (chosen, why) = if let Some((i, n)) = by_name {
        (
            *i,
            format!("tên ảnh '{}' là WinPE thật (không phải Windows Setup)", n),
        )
    } else if indexes.contains(&1) {
        (1, "không có tên phân biệt được — rơi về index 1 (boot.wim chuẩn: index 1 là WinPE)".to_string())
    } else {
        (
            indexes.first().copied().unwrap_or(1),
            "không có tên phân biệt được và không có index 1 — dùng index đầu tiên liệt kê được".to_string(),
        )
    };
    log.line(&format!(
        "WIM không báo 'Boot Index'; liệt kê được {} index [{}] — chọn index {} ({}).",
        indexes.len(),
        indexes
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(", "),
        chosen,
        why
    ));
    log_wim_index_detail(log, wim, chosen);
    chosen
}

/// Ghi chi tiết một index WIM vào log (DISM /Get-WimInfo /Index:N)
/// — một phần của chẩn đoán trước mount; lỗi chỉ ghi cảnh báo.
fn log_wim_index_detail(log: &Arc<Logger>, wim: &Path, index: u32) {
    match dism_wim_info(wim, Some(index)) {
        Ok(t) => {
            log.line(&format!("--- Chi tiết index {}:", index));
            log_dism_output(log, &t);
        }
        Err(e) => log.line(&format!(
            "CẢNH BÁO: đọc chi tiết index {} thất bại: {}",
            index, e
        )),
    }
}

/// Phân tích các dòng "Key : Value" trong output DISM
fn parse_kv(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(pos) = line.find(':') {
            let k = line[..pos].trim().to_string();
            let v = line[pos + 1..].trim().to_string();
            if !k.is_empty() {
                out.push((k, v));
            }
        }
    }
    out
}

fn kv_get(kv: &[(String, String)], key: &str) -> String {
    kv.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

fn parse_build(version: &str) -> u32 {
    version
        .split('.')
        .nth(2)
        .and_then(|p| p.trim().parse().ok())
        .unwrap_or(0)
}

fn load_editions(wim: &Path) -> Result<Vec<EditionInfo>, String> {
    let summary = dism_wim_info(wim, None)?;
    let indexes: Vec<u32> = parse_kv(&summary)
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("Index"))
        .filter_map(|(_, v)| v.parse().ok())
        .collect();
    if indexes.is_empty() {
        return Err("DISM không liệt kê được index nào trong file image.".to_string());
    }
    let mut out = Vec::new();
    for idx in indexes {
        let detail = dism_wim_info(wim, Some(idx))?;
        let kv = parse_kv(&detail);
        let version = kv_get(&kv, "Version");
        let size_bytes = kv_get(&kv, "Size")
            .replace(',', "")
            .replace("bytes", "")
            .trim()
            .parse()
            .unwrap_or(0);
        out.push(EditionInfo {
            index: idx,
            name: kv_get(&kv, "Name"),
            edition: kv_get(&kv, "Edition"),
            build: parse_build(&version),
            arch: kv_get(&kv, "Architecture"),
            size_bytes,
        });
    }
    Ok(out)
}

/// Index mặc định: edition trùng edition đang chạy (giữ digital license)
fn default_edition_index(editions: &[EditionInfo], current_edition: &str) -> usize {
    for (i, e) in editions.iter().enumerate() {
        if !e.edition.is_empty() && e.edition.eq_ignore_ascii_case(current_edition) {
            return i;
        }
    }
    let token = match current_edition {
        "Professional" | "ProfessionalN" => "Pro",
        "Core" | "CoreN" => "Home",
        other => other,
    };
    for (i, e) in editions.iter().enumerate() {
        if e.name.contains(token) {
            return i;
        }
    }
    0
}

fn spawn_load_editions(
    shared: SharedRef,
    sender: nwg::NoticeSender,
    iso: PathBuf,
    prev: Option<(PathBuf, char)>,
) {
    std::thread::spawn(move || {
        let result = (|| -> Result<(char, Vec<EditionInfo>), String> {
            if let Some((prev_iso, _)) = &prev {
                if prev_iso != &iso {
                    dismount_iso(prev_iso);
                } else {
                    // ISO cũ vẫn đang mount — dùng lại ký tự ổ
                    let letter = prev.unwrap().1;
                    let (wim, _) = find_image_file(letter)
                        .ok_or_else(|| "ISO đã mount nhưng thiếu sources\\install.*".to_string())?;
                    return Ok((letter, load_editions(&wim)?));
                }
            }
            let letter = mount_iso(&iso)?;
            match find_image_file(letter) {
                Some((wim, _)) => match load_editions(&wim) {
                    Ok(eds) => Ok((letter, eds)),
                    Err(e) => {
                        dismount_iso(&iso);
                        Err(e)
                    }
                },
                None => {
                    dismount_iso(&iso);
                    Err("Trong ISO không có sources\\install.wim / .esd / .swm.".to_string())
                }
            }
        })();
        shared.lock().unwrap().editions = Some(result);
        sender.notice();
    });
}

// ---------------------------------------------------------------------------
// autounattend.xml (đặc tả §7) — KHÔNG có lệnh cài ứng dụng nào.
// Mật khẩu là TUỲ CHỌN (v0.7.2): kỹ thuật viên cài hàng loạt được để
// trống → tài khoản KHÔNG mật khẩu, người dùng cuối tự đặt sau. Khi
// không có mật khẩu, unattend BỎ HẲN phần tử <Password> ở cả
// LocalAccount lẫn AutoLogon (đúng cấu trúc bản sinh Schneegans —
// AutoLogon không mật khẩu là cấu hình Setup hỗ trợ cho đăng nhập
// nội bộ; KHÔNG sinh <Value> rỗng). Khi có mật khẩu, mật khẩu chỉ
// xuất hiện tại đây và không vào log.
// FirstLogonCommands (v0.7.19) chỉ chứa hai lệnh DỌN CỜ MẬT KHẨU
// cho chính tài khoản admin vừa tạo (không phải lệnh cài ứng
// dụng): tài khoản Setup tạo ra mang cờ "phải đổi mật khẩu ở lần
// đăng nhập đầu" (pwdLastSet = 0) khiến AutoLogon bị chặn ở màn
// hình đổi mật khẩu — thực địa v0.7.18. `net user <tên>
// /logonpasswordchg:no` dọn cờ đó, lệnh PowerShell đặt
// PasswordNeverExpires để mật khẩu (kể cả mật khẩu trống theo
// chính sách mặc định) không bị ép hết hạn về sau.
// ---------------------------------------------------------------------------
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Ghi trạng thái nút hồ sơ CORE vào settings.json cạnh wizard
/// (khoá `defaultCoreProfile`) để lần mở sau wizard nhớ lựa
/// chọn (v0.7.24). Sửa file dạng văn bản tại đúng vị trí khoá —
/// giữ nguyên mọi khoá và định dạng còn lại; chưa có khoá thì
/// chèn ngay sau dấu `{` đầu tiên; chưa có file thì tạo file
/// tối thiểu. Lỗi ghi thì bỏ qua: trạng thái vẫn áp dụng cho
/// phiên hiện tại, chỉ không được nhớ sang lần mở sau.
fn persist_core_default(exe_dir: &Path, on: bool) {
    let path = exe_dir.join("settings.json");
    let val = if on { "true" } else { "false" };
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let new_text = match text.find("\"defaultCoreProfile\"") {
        Some(kp) => {
            let after = &text[kp..];
            let tok = match (after.find("true"), after.find("false")) {
                (Some(t), Some(f)) => {
                    if t < f {
                        Some(("true", t))
                    } else {
                        Some(("false", f))
                    }
                }
                (Some(t), None) => Some(("true", t)),
                (None, Some(f)) => Some(("false", f)),
                (None, None) => None,
            };
            match tok {
                Some((t, off)) => {
                    let start = kp + off;
                    format!("{}{}{}", &text[..start], val, &text[start + t.len()..])
                }
                None => text.clone(),
            }
        }
        None => match text.find('{') {
            Some(bp) => format!(
                "{}{{ \"defaultCoreProfile\": {},{}",
                &text[..bp],
                val,
                &text[bp + 1..]
            ),
            None => format!("{{\n  \"defaultCoreProfile\": {}\n}}\n", val),
        },
    };
    let _ = std::fs::write(&path, new_text);
}

// Hồ sơ CORE (tham số `core_profile`): cho ảnh Windows rút gọn
// kiểu Tiny11 Core — các ảnh này thiếu nhiều component mà một
// unattend đầy đủ chạm tới, và thực địa cho thấy chúng boot
// vòng lỗi 0xEF khi cài bằng pipeline chuẩn nhưng lên ngon khi
// cài tay tối thiểu kiểu WinNTSetup. Diễn tiến hồ sơ CORE:
// v0.7.21 rút gọn unattend (bỏ FirstLogonCommands, dọn cờ mật
// khẩu bằng SetupComplete.cmd) — vẫn 0xEF. v0.7.22 bỏ hẳn
// unattend/driver/WinRE/agent — boot QUA được 0xEF (thực địa
// 2026-11-20) nhưng phải ngồi bấm OOBE và nhập tên thủ công.
// Từ v0.7.23, unattend của hồ sơ CORE CHỈ chứa pha oobeSystem
// (pha specialize vắng hẳn): vẫn tự qua các màn hình thiết
// lập, tự tạo LocalAccount và AutoLogon như hồ sơ standard —
// tiêu chí của tool là cài TỰ ĐỘNG — trong khi mọi phần còn
// lại giữ công thức WinNTSetup (không FirstLogonCommands,
// không SetupComplete, không agent hậu cài; driver/WinRE do
// engine/module bỏ qua ở tầng của chúng). Đánh đổi: không
// đặt được tên máy và múi giờ (chỉ nằm ở pha specialize —
// tên máy để Windows tự sinh, múi giờ theo mặc định của
// ảnh); và không có lệnh dọn cờ "phải đổi mật khẩu ở lần
// đăng nhập đầu" — nếu ảnh đặt cờ đó cho tài khoản tạo từ
// unattend, lần đăng nhập đầu có thể dừng ở màn hình đổi
// mật khẩu (để trống cả ba ô rồi Enter) như ca v0.7.18.
// v0.7.26: engine/module khôi phục SetupComplete.cmd tối
// thiểu của v0.7.21 để dọn cờ đó — chẩn đoán 2026-11-20 đã
// minh oan SetupComplete (0xEF là do entry boot của bcdboot,
// xem mục 17) — nên ca cài CORE từ v0.7.26 vào thẳng desktop,
// không còn dừng ở màn hình đổi mật khẩu.
fn build_unattend(st: &Settings, computer_name: &str, admin_user: &str, admin_password: &str, core_profile: bool) -> String {
    let comp = xml_escape(computer_name);
    let user = xml_escape(admin_user);
    let pass = xml_escape(admin_password);
    // Khối <Password> cho LocalAccount (thụt 12) và AutoLogon (thụt 8);
    // chuỗi rỗng khi không đặt mật khẩu — xem ghi chú đầu hàm.
    let account_pw = if admin_password.is_empty() {
        String::new()
    } else {
        format!(
            "\n            <Password>\n              <Value>{}</Value>\n              <PlainText>true</PlainText>\n            </Password>",
            pass
        )
    };
    let autologon_pw = if admin_password.is_empty() {
        String::new()
    } else {
        format!(
            "\n        <Password>\n          <Value>{}</Value>\n          <PlainText>true</PlainText>\n        </Password>",
            pass
        )
    };
    // Lệnh chạy ở lần đăng nhập đầu (v0.7.19): tài khoản Setup
    // tạo từ unattend mang cờ "phải đổi mật khẩu ở lần đăng
    // nhập đầu tiên" (pwdLastSet = 0) — thực địa v0.7.18 cho
    // thấy AutoLogon bị chặn ở màn hình đổi mật khẩu thay vì
    // vào thẳng desktop. Hai lệnh này dọn cờ đó + đặt mật khẩu
    // không hết hạn, áp dụng CẢ khi mật khẩu để trống lẫn khi
    // có đặt, để ca cài hoàn toàn tự động về được desktop.
    let fl_commands = format!(
        r#"        <SynchronousCommand wcm:action="add">
          <Order>1</Order>
          <CommandLine>net user {user} /logonpasswordchg:no</CommandLine>
          <Description>Không bắt đổi mật khẩu ở lần đăng nhập đầu tiên</Description>
          <RequiresUserInput>false</RequiresUserInput>
        </SynchronousCommand>
        <SynchronousCommand wcm:action="add">
          <Order>2</Order>
          <CommandLine>powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "Set-LocalUser -Name '{user}' -PasswordNeverExpires $true"</CommandLine>
          <Description>Mật khẩu tài khoản admin không hết hạn</Description>
          <RequiresUserInput>false</RequiresUserInput>
        </SynchronousCommand>"#,
        user = user
    );
    // Khối <FirstLogonCommands> hoàn chỉnh của hồ sơ standard
    // (hai lệnh dọn cờ mật khẩu + lệnh hậu cài). Hồ sơ CORE: khối
    // này VẮNG HẲN trong unattend — xem ghi chú đầu hàm.
    let fl_block = if core_profile {
        String::new()
    } else {
        format!(
            "      <FirstLogonCommands>\n{flc}\n        <SynchronousCommand wcm:action=\"add\">\n          <Order>3</Order>\n          <CommandLine>{post}</CommandLine>\n          <Description>Tự cài UltraViewer, Chrome và 7-Zip sau khi vào Windows mới</Description>\n          <RequiresUserInput>false</RequiresUserInput>\n        </SynchronousCommand>\n      </FirstLogonCommands>\n",
            flc = fl_commands,
            post = xml_escape(qsu_common::post_install::FIRST_LOGON_COMMAND),
        )
    };
    // Pha specialize của hồ sơ standard. Hồ sơ CORE: vắng hẳn —
    // unattend CORE chỉ còn pha oobeSystem (xem ghi chú đầu hàm).
    let spec_block = if core_profile {
        String::new()
    } else {
        format!(
            r#"  <settings pass="specialize">
    <component name="Microsoft-Windows-International-Core" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <InputLocale>{input}</InputLocale>
      <SystemLocale>{locale}</SystemLocale>
      <UILanguage>{ui}</UILanguage>
      <UserLocale>{locale}</UserLocale>
    </component>
    <component name="Microsoft-Windows-Shell-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <ComputerName>{comp}</ComputerName>
      <TimeZone>{tz}</TimeZone>
    </component>
  </settings>
"#,
            input = xml_escape(&st.input_locale),
            locale = xml_escape(&st.locale),
            ui = xml_escape(&st.ui_language),
            tz = xml_escape(&st.timezone),
            comp = comp,
        )
    };
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend">
{spec}  <settings pass="oobeSystem">
    <component name="Microsoft-Windows-International-Core" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <InputLocale>{input}</InputLocale>
      <SystemLocale>{locale}</SystemLocale>
      <UILanguage>{ui}</UILanguage>
      <UserLocale>{locale}</UserLocale>
    </component>
    <component name="Microsoft-Windows-Shell-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
      <OOBE>
        <HideEULAPage>true</HideEULAPage>
        <HideOEMRegistrationScreen>true</HideOEMRegistrationScreen>
        <HideOnlineAccountScreens>true</HideOnlineAccountScreens>
        <HideWirelessSetupInOOBE>true</HideWirelessSetupInOOBE>
        <ProtectYourPC>3</ProtectYourPC>
      </OOBE>
      <UserAccounts>
        <LocalAccounts>
          <LocalAccount wcm:action="add">
            <Name>{user}</Name>
            <Group>Administrators</Group>{account_pw}
          </LocalAccount>
        </LocalAccounts>
      </UserAccounts>
      <AutoLogon>
        <Enabled>true</Enabled>
        <Username>{user}</Username>{autologon_pw}
        <LogonCount>1</LogonCount>
      </AutoLogon>
{fl_block}
    </component>
  </settings>
</unattend>
"#,
        spec = spec_block,
        input = xml_escape(&st.input_locale),
        locale = xml_escape(&st.locale),
        ui = xml_escape(&st.ui_language),
        user = user,
        fl_block = fl_block,
    )
}

// ---------------------------------------------------------------------------
// Giai đoạn 3 — thực thi cài đặt (đặc tả §5, §6, §8)
// ---------------------------------------------------------------------------
/// Chế độ phân vùng khi cài mới (stage.json `partitionMode`):
/// - Preserve: giữ cấu trúc phân vùng, chỉ format phân vùng OS.
/// - WipeDisk (§6b): xoá TOÀN BỘ đĩa đích, chia lại từ đầu.
/// - Rebuild (v0.7.13): chỉ xoá các phân vùng hệ thống đã chọn trên
///   đĩa đích (EFI/MSR/Windows cũ/Recovery), chia lại layout mới
///   trong vùng trống của chúng; phân vùng data trên cùng đĩa —
///   kể cả phân vùng chứa staging — được giữ nguyên tuyệt đối.
#[derive(Clone, Copy, PartialEq, Debug)]
enum PartMode {
    Preserve,
    WipeDisk,
    Rebuild,
}

impl PartMode {
    fn stage_str(&self) -> &'static str {
        match self {
            PartMode::Preserve => "preserve",
            PartMode::WipeDisk => "wipeDisk",
            PartMode::Rebuild => "rebuild",
        }
    }
}

#[derive(Clone)]
struct ExecPlan {
    mode_clean: bool,
    iso: PathBuf,
    mounted_letter: char,
    image_name: String, // install.wim / install.esd / install.swm
    edition: EditionInfo,
    os_letter: char,
    data_letter: char,
    os_serial: u64,
    os_label: String,
    data_serial: u64,
    uefi: bool,
    /// Giá trị thô của hai API dò firmware lúc quét tiền đề (v0.7.7)
    /// — ghi vào log cài đặt ngay đầu quá trình và đối chiếu lại tại
    /// register_bcd với tín hiệu thứ ba ({fwbootmgr}).
    firmware_detail: String,
    bitlocker: BitlockerState,
    computer_name: String,
    admin_user: String,
    admin_password: String,
    apply_tweaks: bool,
    /// Hồ sơ CORE (v0.7.21) — ghi vào stage.json `imageProfile`.
    core_profile: bool,
    /// Chế độ phân vùng khi cài mới (chỉ có nghĩa khi mode_clean).
    part_mode: PartMode,
    /// Đĩa vật lý đích của wipeDisk/rebuild (số thứ tự + serial phần
    /// cứng — engine đối chiếu lại trong WinPE ngay trước khi xoá).
    target_disk_number: u32,
    target_disk_serial: String,
    /// Tóm tắt đĩa đích + danh sách phân vùng sẽ bị xoá (cho hộp xác
    /// nhận cuối + ghi log trước khi thực thi).
    target_disk_summary: String,
    target_disk_parts: String,
    /// Rebuild: danh sách phân vùng hệ thống sẽ xoá (ghi vào
    /// stage.json `rebuildPartitions`) + mô tả các phân vùng được
    /// GIỮ NGUYÊN trên cùng đĩa (cho hộp xác nhận + log).
    rebuild_parts: Vec<RebuildPart>,
    rebuild_kept: String,
}

fn set_exec_progress(shared: &SharedRef, sender: &nwg::NoticeSender, pct: u32, label: &str) {
    lock_shared(shared).exec_progress = Some((pct, label.to_string()));
    sender.notice();
}

fn dir_size(p: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                total += dir_size(&path);
            } else if let Ok(m) = e.metadata() {
                total += m.len();
            }
        }
    }
    total
}

fn copy_dir_progress(
    src: &Path,
    dst: &Path,
    total: u64,
    done: &mut u64,
    on_progress: &mut dyn FnMut(u64, u64),
) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)?.flatten() {
        let from = e.path();
        let to = dst.join(e.file_name());
        if from.is_dir() {
            copy_dir_progress(&from, &to, total, done, on_progress)?;
        } else {
            std::fs::copy(&from, &to)?;
            *done += e.metadata().map(|m| m.len()).unwrap_or(0);
            on_progress(*done, total);
        }
    }
    Ok(())
}

fn extract_guid(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = text[i..].find('}') {
                let cand = &text[i..=i + end];
                let inner = &cand[1..cand.len() - 1];
                if inner.len() == 36
                    && inner.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
                {
                    return Some(cand.to_string());
                }
            }
        }
    }
    None
}

fn qsu_start_cmd(staging_folder: &str) -> String {
    // Chạy trong WinPE: tìm staging trên mọi ổ (ký tự ổ thay đổi theo
    // lần boot), chạy WinPEInstall.exe tại đó; không thấy thì chạy bản
    // đã inject sẵn trong WIM. Sau cùng reboot — bootsequence chỉ có
    // hiệu lực một lần nên máy sẽ quay về Windows cũ nếu có lỗi.
    let letters = "C D E F G H I J K L M N O P Q R S T U V W X Y Z";
    format!(
        "@echo off\r\nwpeinit\r\ncall :find\r\ngoto end\r\n:find\r\nfor %%D in ({letters}) do if exist %%D:\\{staging}\\WinPEInstall.exe (%%D:\\{staging}\\WinPEInstall.exe {staging} & exit /b)\r\n%SYSTEMDRIVE%\\Windows\\System32\\WinPEInstall.exe {staging}\r\nexit /b\r\n:end\r\nwpeutil reboot\r\n",
        letters = letters,
        staging = staging_folder,
    )
}

fn tail_panther(path: &Path, shared: &SharedRef, sender: &nwg::NoticeSender, log: &Arc<Logger>) {
    use std::io::{Read, Seek, SeekFrom};
    let mut pos: u64 = 0;
    let mut last_msg = String::new();
    for _ in 0..1800 {
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.len() > pos {
                if let Ok(mut f) = std::fs::File::open(path) {
                    if f.seek(SeekFrom::Start(pos)).is_ok() {
                        let mut buf = Vec::new();
                        if f.read_to_end(&mut buf).is_ok() {
                            pos = meta.len();
                            let text = String::from_utf8_lossy(&buf);
                            for line in text.lines() {
                                let t = line.trim();
                                if t.contains('%') && t != last_msg {
                                    last_msg = t.to_string();
                                    lock_shared(shared).exec_msg =
                                        Some(format!("Setup: {}", t));
                                    sender.notice();
                                }
                            }
                        }
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    log.line("Dừng theo dõi log Panther (hết thời gian chờ).");
}

fn spawn_exec(
    shared: SharedRef,
    sender: nwg::NoticeSender,
    log: Arc<Logger>,
    settings: Settings,
    exe_dir: PathBuf,
    plan: ExecPlan,
) {
    std::thread::spawn(move || {
        // Chốt chặn cuối cùng của luồng cài đặt: BẤT KỲ panic nào
        // trong quá trình staging cũng phải biến thành một lỗi hiển
        // thị rõ ràng (hộp thoại + log panel + file log trên ổ Data)
        // kèm mở khoá giao diện — không bao giờ chết âm thầm khiến
        // cửa sổ đứng im vô thời hạn hoặc biến mất không lời giải
        // thích (báo cáo thực địa v0.7.2: bấm Yes xong "không có
        // hành động gì"). Panic chi tiết còn được hộp đen ghi vào
        // CaiDatWinTuDong-panic.log cạnh file exe (install_panic_hook).
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            exec_main(&shared, &sender, &log, &settings, &exe_dir, &plan);
        }));
        if let Err(payload) = result {
            let msg = format!(
                "Lỗi nội bộ bất ngờ trong quá trình cài đặt ({}). Quá trình đã dừng — chi tiết nằm trong file log trên ổ Data và CaiDatWinTuDong-panic.log cạnh file wizard.",
                panic_payload_text(&*payload)
            );
            log.line(&format!("PANIC trong luồng cài đặt: {}", msg));
            lock_shared(&shared).exec_done = Some(Err(msg));
            sender.notice();
        }
    });
}

/// Thân luồng cài đặt (chạy trên luồng nền do `spawn_exec` tạo, đã
/// được bọc `catch_unwind`). Mọi lỗi nghiệp vụ đều kết thúc qua
/// `fail(...)` → `exec_done = Err` → hộp thoại lỗi trên giao diện;
/// thành công qua `exec_done = Ok` trong exec_clean/exec_inplace.
fn exec_main(
    shared: &SharedRef,
    sender: &nwg::NoticeSender,
    log: &Arc<Logger>,
    settings: &Settings,
    exe_dir: &Path,
    plan: &ExecPlan,
) {
        let fail = |msg: String| {
            log.line(&format!("LỖI: {}", msg));
            lock_shared(shared).exec_done = Some(Err(msg));
            sender.notice();
        };
        // Nhịp tim đầu tiên: báo tiến độ NGAY khi luồng bắt đầu —
        // người dùng thấy phản hồi trong vòng một nhịp notice đầu
        // tiên sau khi bấm Yes, trước mọi thao tác dài (sao lưu
        // driver, copy sources...).
        set_exec_progress(shared, sender, 1, "Đã bắt đầu — đang chuẩn bị staging...");
        log.line("=== Bắt đầu quá trình cài đặt ===");
        log.line(&format!("Firmware: {}", plan.firmware_detail));
        log.line(&format!(
            "Chế độ: {}",
            if plan.mode_clean {
                match plan.part_mode {
                    PartMode::WipeDisk => {
                        "Cài mới hoàn toàn (WinPE staging — XOÁ TOÀN BỘ Ổ ĐĨA ĐÍCH, CHIA LẠI TỪ ĐẦU)"
                    }
                    PartMode::Rebuild => {
                        "Cài mới (WinPE staging — CHỈ XOÁ PHÂN VÙNG HỆ THỐNG TRÊN ĐĨA ĐÍCH, GIỮ PHÂN VÙNG DATA)"
                    }
                    PartMode::Preserve => {
                        "Cài mới từ xa (WinPE staging — giữ cấu trúc phân vùng)"
                    }
                }
            } else {
                "Cài đè (giữ ứng dụng + dữ liệu)"
            }
        ));
        if plan.part_mode == PartMode::WipeDisk {
            log.line(&format!(
                "Đĩa đích wipe: PhysicalDrive{} — serial {}",
                plan.target_disk_number, plan.target_disk_serial
            ));
            log.line(&format!(
                "Các phân vùng trên đĩa đích sẽ bị XOÁ TOÀN BỘ:\n{}",
                plan.target_disk_parts
            ));
        }
        if plan.part_mode == PartMode::Rebuild {
            log.line(&format!(
                "Đĩa đích rebuild: PhysicalDrive{} — serial {}",
                plan.target_disk_number, plan.target_disk_serial
            ));
            log.line(&format!(
                "Các phân vùng HỆ THỐNG trên đĩa đích sẽ bị XOÁ:\n{}",
                plan.target_disk_parts
            ));
            log.line(&format!(
                "Các phân vùng trên đĩa đích được GIỮ NGUYÊN:\n{}",
                plan.rebuild_kept
            ));
        }
        log.line(&format!("ISO: {}", plan.iso.display()));
        log.line(&format!(
            "Phiên bản: {} (index {}, build {})",
            plan.edition.name, plan.edition.index, plan.edition.build
        ));
        // Mật khẩu admin KHÔNG bao giờ được ghi vào log — chỉ ghi
        // trạng thái "không đặt mật khẩu" khi kỹ thuật viên chọn vậy.
        if plan.mode_clean && plan.admin_password.is_empty() {
            log.line("Tài khoản admin: không đặt mật khẩu (theo lựa chọn)");
        }

        let data_root = PathBuf::from(format!("{}:\\", plan.data_letter));
        let os_root = PathBuf::from(format!("{}:\\", plan.os_letter));

        // --- Chuẩn bị chung: marker + thư mục + suspend BitLocker + backup driver
        set_exec_progress(shared, sender, 3, "Chuẩn bị ổ Data...");
        let marker_path = data_root.join(DATA_MARKER);
        if !marker_path.exists() {
            let content = format!(
                "QSU data drive marker - do not delete\nComputer: {}\nCreated: {}\n",
                sys::computer_name(),
                qsu_common::log::now_string()
            );
            if let Err(e) = std::fs::write(&marker_path, content) {
                fail(format!("không tạo được marker ổ Data: {}", e));
                return;
            }
            log.line("Đã tạo marker .qsu_data_drive trên ổ Data.");
        }
        let logs_dir = data_root.join(&settings.logs_folder);
        let _ = std::fs::create_dir_all(&logs_dir);
        let backup_dir = data_root.join(DRIVER_BACKUP_FOLDER);
        let _ = std::fs::create_dir_all(&backup_dir);

        if plan.bitlocker == BitlockerState::Encrypted {
            set_exec_progress(shared, sender, 6, "Tạm treo BitLocker...");
            let mount = format!("{}:", plan.os_letter);
            match cmd::run_logged(log, "manage-bde", &["-protectors", "-disable", &mount]) {
                Ok(out) if out.code == Some(0) => log.line("Đã suspend BitLocker trên ổ OS."),
                Ok(out) => {
                    let combined = format!("{}\n{}", out.stdout, out.stderr);
                    fail(cmd_fail_msg(
                        "Suspend BitLocker (manage-bde -protectors -disable)",
                        out.code,
                        &combined,
                    ));
                    return;
                }
                Err(e) => {
                    fail(format!("không chạy được manage-bde: {}", e));
                    return;
                }
            }
        }

        set_exec_progress(shared, sender, 10, "Sao lưu driver hiện tại...");
        let backup_s = backup_dir.to_string_lossy().into_owned();
        match cmd::run_logged(log, "pnputil", &["/export-driver", "*", &backup_s]) {
            Ok(out) if out.code == Some(0) => log.line("Sao lưu driver hoàn tất."),
            Ok(out) => log.line(&format!(
                "CẢNH BÁO: pnputil /export-driver trả {} — vẫn tiếp tục.",
                fmt_exit_code(out.code)
            )),
            Err(e) => log.line(&format!(
                "CẢNH BÁO: không chạy được pnputil: {} — vẫn tiếp tục.",
                e
            )),
        }

    if plan.mode_clean {
        exec_clean(shared, sender, log, settings, exe_dir, plan, &data_root);
    } else {
        exec_inplace(shared, sender, log, settings, exe_dir, plan, &data_root, &os_root, &logs_dir);
    }
}

fn exec_inplace(
    shared: &SharedRef,
    sender: &nwg::NoticeSender,
    log: &Arc<Logger>,
    _settings: &Settings,
    exe_dir: &Path,
    plan: &ExecPlan,
    _data_root: &Path,
    os_root: &Path,
    logs_dir: &Path,
) {
    let fail = |msg: String| {
        log.line(&format!("LỖI: {}", msg));
        lock_shared(shared).exec_done = Some(Err(msg));
        sender.notice();
    };
    // Thư mục custom của người dùng -> ProgramData\QSU\custom (được giữ
    // lại qua nâng cấp tại chỗ). Copy nguyên trạng, không diễn giải.
    let custom_src = exe_dir.join("custom");
    if custom_src.is_dir() {
        set_exec_progress(shared, sender, 16, "Copy thư mục custom...");
        let dst = os_root.join("ProgramData").join("QSU").join("custom");
        let total = dir_size(&custom_src);
        let mut done = 0u64;
        let mut noop = |_: u64, _: u64| {};
        if let Err(e) = copy_dir_progress(&custom_src, &dst, total, &mut done, &mut noop) {
            log.line(&format!("CẢNH BÁO: copy custom thất bại: {}", e));
        } else {
            log.line("Đã copy thư mục custom vào ProgramData\\QSU\\custom.");
        }
    }

    // File trạng thái (đặc tả §5): wizard mở lại sau reboot đọc được
    set_exec_progress(shared, sender, 24, "Ghi trạng thái cài đặt...");
    let state = json::obj(vec![
        ("mode", json::s("inplace")),
        ("iso", json::s(&plan.iso.to_string_lossy())),
        ("edition", json::s(&plan.edition.name)),
        ("startedAt", json::s(&qsu_common::log::now_string())),
    ]);
    let _ = std::fs::write(logs_dir.join("install-state.json"), json::stringify(&state));
    let _ = sys::reg_set_sz(
        sys::hklm(),
        "SOFTWARE\\QSU",
        "InstallState",
        &format!("inplace {}", qsu_common::log::log_stamp()),
    );

    // Chạy setup.exe của bộ cài (đặc tả §5)
    set_exec_progress(shared, sender, 30, "Khởi chạy Windows Setup...");
    let setup = PathBuf::from(format!("{}:\\setup.exe", plan.mounted_letter));
    if !setup.exists() {
        fail(format!("không thấy setup.exe tại {}", setup.display()));
        return;
    }
    log.line(&format!(
        "$ {} /auto upgrade /quiet /showoobe none /DynamicUpdate disable /Compat IgnoreWarning /MigrateDrivers all /Telemetry disable",
        setup.display()
    ));
    let spawn = std::process::Command::new(&setup)
        .args([
            "/auto", "upgrade", "/quiet", "/showoobe", "none", "/DynamicUpdate",
            "disable", "/Compat", "IgnoreWarning", "/MigrateDrivers", "all",
            "/Telemetry", "disable",
        ])
        .current_dir(format!("{}:\\", plan.mounted_letter))
        .spawn();
    match spawn {
        Ok(_) => log.line("setup.exe đã khởi chạy."),
        Err(e) => {
            fail(format!("không chạy được setup.exe: {}", e));
            return;
        }
    }
    set_exec_progress(shared, sender, 40, "Setup đang chạy — theo dõi tiến độ qua log Panther...");
    lock_shared(shared).exec_done = Some(Ok(
        "Đã khởi chạy Windows Setup. Máy sẽ tự khởi động lại nhiều lần và hoàn tất cài đè; bản Windows cũ nằm trong Windows.old (quay lui được trong 10 ngày).".to_string(),
    ));
    sender.notice();
    // Theo dõi log Panther của setup cho tới khi wizard đóng
    let panther = os_root
        .join("$WINDOWS.~BT")
        .join("Sources")
        .join("Panther")
        .join("setupact.log");
    tail_panther(&panther, shared, sender, log);
}

fn register_bcd(
    log: &Arc<Logger>,
    plan: &ExecPlan,
    settings: &Settings,
) -> Result<String, String> {
    let run = |args: &[&str]| -> Result<String, String> {
        let out = cmd::run_logged(log, "bcdedit", args)
            .map_err(|e| format!("không chạy được bcdedit: {}", e))?;
        if out.code != Some(0) {
            let combined = format!("{}\n{}", out.stdout, out.stderr);
            return Err(cmd_fail_msg(
                &format!("bcdedit {}", args.join(" ")),
                out.code,
                &combined,
            ));
        }
        Ok(out.stdout)
    };
    // Đối chiếu firmware bằng một tín hiệu thứ ba, độc lập với hai
    // API đã dò ở tiền đề: object {fwbootmgr} CHỈ tồn tại trên máy
    // UEFI (bcdedit /enum {fwbootmgr} thành công); trên máy Legacy
    // lệnh này lỗi. Chỉ ghi log đối chiếu — phán quyết vẫn là
    // plan.uefi từ tiền đề (hai API); nếu hai bên mâu thuẫn, dòng
    // CẢNH BÁO này + dòng "Firmware:" ở đầu log là bằng chứng để
    // chẩn đoán (thực địa v0.7.6: máy ảo báo UEFI nhưng đĩa mang
    // tên thiết bị kiểu Gen-1 — cần mọi tín hiệu nằm trong log).
    log.line(&format!("Firmware (tiền đề): {}", plan.firmware_detail));
    match cmd::run_capture("bcdedit", &["/enum", "{fwbootmgr}"]) {
        Ok(out) => {
            let fwobj = out.code == Some(0);
            if fwobj == plan.uefi {
                log.line(&format!(
                    "Đối chiếu firmware: bcdedit /enum {{fwbootmgr}} -> {} — khớp phán quyết {} của tiền đề.",
                    fmt_exit_code(out.code),
                    if plan.uefi { "UEFI" } else { "Legacy/BIOS" }
                ));
            } else {
                log.line(&format!(
                    "CẢNH BÁO đối chiếu firmware: bcdedit /enum {{fwbootmgr}} -> {} ({}), KHÔNG khớp phán quyết {} của tiền đề — vẫn đăng ký theo phán quyết tiền đề; xem dòng 'Firmware:' ở đầu log để chẩn đoán.",
                    fmt_exit_code(out.code),
                    if fwobj {
                        "có {fwbootmgr} — dấu hiệu UEFI"
                    } else {
                        "không có {fwbootmgr} — dấu hiệu Legacy/BIOS"
                    },
                    if plan.uefi { "UEFI" } else { "Legacy/BIOS" }
                ));
            }
        }
        Err(e) => log.line(&format!(
            "Đối chiếu firmware: không chạy được bcdedit /enum {{fwbootmgr}}: {}",
            e
        )),
    }
    let dev_out = run(&["/create", "/d", "QSU WinPE Ramdisk", "/device"])?;
    let dev_guid = extract_guid(&dev_out)
        .ok_or_else(|| "không đọc được GUID của ramdisk entry từ output bcdedit".to_string())?;
    let part_arg = format!("partition={}:", plan.data_letter);
    run(&["/set", &dev_guid, "ramdisksdidevice", &part_arg])?;
    let sdi_arg = format!("\\{}\\boot.sdi", settings.staging_folder);
    run(&["/set", &dev_guid, "ramdisksdipath", &sdi_arg])?;
    let load_out = run(&["/create", "/d", "QSU WinPE", "/application", "osloader"])?;
    let load_guid = extract_guid(&load_out)
        .ok_or_else(|| "không đọc được GUID của loader entry từ output bcdedit".to_string())?;
    let ramdisk_arg = format!(
        "ramdisk=[{}:]\\{}\\winpe.wim,{}",
        plan.data_letter, settings.staging_folder, dev_guid
    );
    run(&["/set", &load_guid, "device", &ramdisk_arg])?;
    run(&["/set", &load_guid, "osdevice", &ramdisk_arg])?;
    let winload = if plan.uefi {
        "\\windows\\system32\\boot\\winload.efi"
    } else {
        "\\windows\\system32\\winload.exe"
    };
    run(&["/set", &load_guid, "path", winload])?;
    run(&["/set", &load_guid, "systemroot", "\\windows"])?;
    run(&["/set", &load_guid, "winpe", "yes"])?;
    run(&["/set", &load_guid, "detecthal", "yes"])?;
    run(&["/bootsequence", &load_guid])?;

    // --- Kiểm chứng sau đăng ký, chế độ CHẨN ĐOÁN log-only (v0.7.7) ---
    // verify_bcd dump `bcdedit /enum all` NGUYÊN VĂN vào log cài
    // đặt và đối chiếu từng phần tử then chốt, ghi kết quả vào
    // log — nhưng KHÔNG chặn và KHÔNG gỡ entry: install log thực
    // địa v0.7.6 (install-20261117-1335.log) đã chứng minh mọi
    // lệnh BCD thành công và các phần tử đúng nguyên văn công
    // thức ramdisk, nên một cổng chặn dựa trên phân tích output
    // (vốn có thể lệch vì bản địa hoá) chỉ thêm rủi ro chặn oan.
    // (Bước chuẩn hoá + cổng chặn cờ bootable của WIM từ v0.7.7
    // đã bị GỠ ở v0.7.11 theo chỉ thị người dùng — cổng đó cũng
    // dựa trên phân tích text DISM và đã chặn oan 4/4 lần chạy
    // thực địa, kể cả ISO Microsoft gốc; nay wizard dùng WinPE
    // nguyên trạng của ISO và đi thẳng tới đăng ký BCD này.)
    verify_bcd(log, &run, plan, settings, &dev_guid, &load_guid, winload);
    log.line(&format!(
        "Đã đăng ký BCD: loader {} (boot một lần).",
        load_guid
    ));
    Ok(load_guid)
}

/// Đối chiếu các phần tử BCD vừa đăng ký bằng cách đọc lại output
/// `bcdedit /enum` (v0.7.7) — CHẾ ĐỘ CHẨN ĐOÁN, chỉ ghi log, không
/// chặn, không gỡ entry (xem lý do tại điểm gọi trong register_bcd:
/// logic phần tử đã được log thực địa chứng minh đúng nguyên văn).
/// Mọi lệnh enum đều chạy qua `run` của register_bcd (run_logged)
/// nên output NGUYÊN VĂN — gồm cả dump `/enum all` — chảy hết vào
/// log cài đặt: lần test sau nhìn log là thấy đúng những gì đã
/// đăng ký, không cần thêm vòng gửi/nhận. Các đối chiếu chỉ dựa
/// trên GIÁ TRỊ không phụ thuộc ngôn ngữ hiển thị (chuỗi
/// ramdisk=..., đường dẫn winload, partition=..., GUID), không dựa
/// vào tên phần tử có thể đã bản địa hoá; systemroot/winpe là kiểm
/// mềm. Điểm không khớp được ghi thành dòng "KHÔNG KHỚP" trong log.
fn verify_bcd(
    log: &Arc<Logger>,
    run: &dyn Fn(&[&str]) -> Result<String, String>,
    plan: &ExecPlan,
    settings: &Settings,
    dev_guid: &str,
    load_guid: &str,
    winload: &str,
) {
    log.line("--- Kiểm chứng BCD sau đăng ký (chẩn đoán — chỉ ghi log) ---");
    // Dump toàn bộ store vào log trước — bằng chứng nguyên văn cho
    // lần chẩn đoán sau. Lỗi ở riêng lệnh dump này không chặn các
    // đối chiếu chi tiết ở dưới (vẫn còn output của từng object).
    match run(&["/enum", "all"]) {
        Ok(_) => log.line("Đã dump `bcdedit /enum all` vào log (các dòng ngay phía trên)."),
        Err(e) => log.line(&format!(
            "CẢNH BÁO: dump bcdedit /enum all thất bại: {} — vẫn đối chiếu theo từng object.",
            e
        )),
    }
    let enum_loader = match run(&["/enum", load_guid]) {
        Ok(t) => t,
        Err(e) => {
            log.line(&format!(
                "Kiểm chứng BCD (chẩn đoán): không đọc lại được loader {}: {}",
                load_guid, e
            ));
            return;
        }
    };
    let enum_dev = match run(&["/enum", dev_guid]) {
        Ok(t) => t,
        Err(e) => {
            log.line(&format!(
                "Kiểm chứng BCD (chẩn đoán): không đọc lại được device options {}: {}",
                dev_guid, e
            ));
            return;
        }
    };
    let enum_bootmgr = match run(&["/enum", "{bootmgr}"]) {
        Ok(t) => t,
        Err(e) => {
            log.line(&format!(
                "Kiểm chứng BCD (chẩn đoán): không đọc lại được {{bootmgr}}: {}",
                e
            ));
            return;
        }
    };

    let loader_l = enum_loader.to_lowercase();
    let dev_l = enum_dev.to_lowercase();
    let bootmgr_l = enum_bootmgr.to_lowercase();
    let mut mismatches: Vec<String> = Vec::new();
    let note = |ok: bool, ok_msg: &str, bad_msg: String, mismatches: &mut Vec<String>| {
        if ok {
            log.line(&format!("Kiểm chứng BCD (chẩn đoán): khớp — {}.", ok_msg));
        } else {
            log.line(&format!(
                "Kiểm chứng BCD (chẩn đoán): KHÔNG KHỚP — {}.",
                bad_msg
            ));
            mismatches.push(bad_msg);
        }
    };

    // device + osdevice của loader mang CÙNG một chuỗi
    // ramdisk=[<Data>:]\<staging>\winpe.wim,{ramdiskoptions-guid}
    // — đếm số dòng chứa chuỗi đó, phải >= 2.
    let ramdisk_expected = format!(
        "ramdisk=[{}:]\\{}\\winpe.wim,{}",
        plan.data_letter, settings.staging_folder, dev_guid
    )
    .to_lowercase();
    let hits = loader_l
        .lines()
        .filter(|l| l.contains(&ramdisk_expected))
        .count();
    note(
        hits >= 2,
        "device + osdevice của loader khớp chuỗi ramdisk (kèm GUID ramdiskoptions)",
        format!(
            "loader device/osdevice: cần 2 dòng chứa '{}', chỉ thấy {} dòng",
            ramdisk_expected, hits
        ),
        &mut mismatches,
    );

    // path phải khớp firmware (winload.efi trên UEFI, winload.exe
    // trên Legacy/BIOS).
    let winload_expected = winload.to_lowercase();
    note(
        loader_l.lines().any(|l| l.contains(&winload_expected)),
        &format!("path khớp firmware ({})", winload),
        format!("loader path: không thấy '{}'", winload),
        &mut mismatches,
    );

    // ramdisksdidevice = partition=<Data>: trên device options.
    let sdi_dev_expected = format!("partition={}:", plan.data_letter).to_lowercase();
    note(
        dev_l.lines().any(|l| l.contains(&sdi_dev_expected)),
        "ramdisksdidevice khớp (partition ổ Data)",
        format!(
            "device options ramdisksdidevice: không thấy '{}'",
            sdi_dev_expected
        ),
        &mut mismatches,
    );

    // ramdisksdipath = \<staging>\boot.sdi trên device options.
    let sdi_path_expected = format!("\\{}\\boot.sdi", settings.staging_folder).to_lowercase();
    note(
        dev_l.lines().any(|l| l.contains(&sdi_path_expected)),
        "ramdisksdipath khớp (\\<staging>\\boot.sdi)",
        format!(
            "device options ramdisksdipath: không thấy '{}'",
            sdi_path_expected
        ),
        &mut mismatches,
    );

    // bootsequence của {bootmgr} phải chứa GUID loader. GUID này
    // không thể xuất hiện ở phần tử nào khác của {bootmgr} vì
    // wizard không hề đụng displayorder — thấy GUID trong output
    // {bootmgr} tức là nó nằm trong bootsequence (boot MỘT LẦN,
    // không phải thay đổi thứ tự hiển thị vĩnh viễn).
    note(
        bootmgr_l.contains(&load_guid.to_lowercase()),
        "bootsequence của {bootmgr} chứa loader WinPE (boot một lần; displayorder không đổi)",
        format!("{{bootmgr}} bootsequence: không thấy GUID loader {}", load_guid),
        &mut mismatches,
    );

    // Kiểm mềm (chỉ log): systemroot \windows và winpe = Yes —
    // tên phần tử có thể đã bản địa hoá nên không lấy làm điều
    // kiện cứng ngay cả trong chẩn đoán.
    if loader_l.lines().any(|l| l.contains("\\windows")) {
        log.line("Kiểm chứng BCD (chẩn đoán, mềm): thấy systemroot \\windows trong loader.");
    }
    if loader_l
        .lines()
        .any(|l| l.trim_start().starts_with("winpe") && (l.contains("yes") || l.contains("true")))
    {
        log.line("Kiểm chứng BCD (chẩn đoán, mềm): winpe = Yes.");
    }

    if mismatches.is_empty() {
        log.line("Kiểm chứng BCD (chẩn đoán): mọi phần tử then chốt đều khớp.");
    } else {
        log.line(&format!(
            "Kiểm chứng BCD (chẩn đoán): {} điểm KHÔNG KHỚP (chỉ ghi log — entry vẫn giữ nguyên): {}",
            mismatches.len(),
            mismatches.join(" | ")
        ));
    }
}

/// Tìm và chép `boot.sdi` vào gốc staging (register_bcd trỏ
/// `ramdisksdipath` tới `\<staging>\boot.sdi`). ISO mod thường lược bỏ
/// file này khỏi `sources` nên cần chuỗi dự phòng: boot.sdi chỉ là SDI
/// ramdisk dùng chung của Microsoft, bản nằm trong Windows đang chạy
/// (`%WINDIR%\Boot\DVD\...`) thay thế an toàn và không phụ thuộc build.
/// Thứ tự hai bản trong Windows đang chạy XOAY THEO FIRMWARE (v0.7.7):
/// `%WINDIR%\Boot\DVD` có hai biến thể — thư mục PCAT là biến thể cho
/// boot BIOS, thư mục EFI là biến thể cho boot UEFI. Bản cũ luôn thử
/// PCAT trước EFI bất kể firmware; thực địa v0.7.6 trên máy UEFI đã
/// nạp đúng bản PCAT vào staging rồi Boot Manager lỗi 0xc000000e khi
/// dựng ramdisk — biến thể SDI không khớp firmware là nghi phạm hàng
/// đầu. Nay máy UEFI thử bản EFI trước, máy Legacy thử PCAT trước,
/// và dòng log chép thành công nêu rõ biến thể đã nạp.
/// Mọi vị trí đã thử đều được ghi log; chép xong phải kiểm chứng file
/// đích tồn tại (bản cũ nuốt lỗi copy bằng `let _ =`). Không thấy ở
/// đâu thì trả Err liệt kê toàn bộ vị trí đã thử để lần test thực địa
/// sau tự chẩn đoán được — lỗi vẫn nổ TRƯỚC mọi thao tác phá huỷ.
fn stage_boot_sdi(
    log: &Arc<Logger>,
    src_dir: &Path,
    plan: &ExecPlan,
    stage_dir: &Path,
) -> Result<(), String> {
    let windir = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("{}:\\Windows", plan.os_letter)));
    let dvd = windir.join("Boot").join("DVD");
    let (sys_first, sys_first_label, sys_second, sys_second_label) = if plan.uefi {
        (
            dvd.join("EFI").join("boot.sdi"),
            "bản EFI — biến thể UEFI",
            dvd.join("PCAT").join("boot.sdi"),
            "bản PCAT — biến thể BIOS",
        )
    } else {
        (
            dvd.join("PCAT").join("boot.sdi"),
            "bản PCAT — biến thể BIOS",
            dvd.join("EFI").join("boot.sdi"),
            "bản EFI — biến thể UEFI",
        )
    };
    let candidates: Vec<(PathBuf, &str)> = vec![
        // 1) ISO đã mount: sources\boot.sdi (đường chuẩn trước đây)
        (src_dir.join("boot.sdi"), "ISO, thư mục sources"),
        // 2) ISO đã mount: \boot.sdi ở gốc đĩa
        (
            PathBuf::from(format!("{}:\\boot.sdi", plan.mounted_letter)),
            "ISO, gốc đĩa",
        ),
        // 3+4) Windows đang chạy: Boot\DVD\<theo firmware trước>
        //    rồi biến thể còn lại — SDI ramdisk dùng chung mà chính
        //    bộ cài của Microsoft cũng dùng.
        (sys_first, sys_first_label),
        (sys_second, sys_second_label),
        // 5) Thư mục WinRE của Windows đang chạy (nếu sẵn có boot.sdi)
        (
            PathBuf::from(format!(
                "{}:\\Recovery\\WindowsRE\\boot.sdi",
                plan.os_letter
            )),
            "WinRE của Windows đang chạy",
        ),
    ];
    log.line(&format!(
        "boot.sdi: firmware {} — thứ tự thử: {}",
        if plan.uefi {
            "UEFI (bản EFI trước PCAT)"
        } else {
            "Legacy/BIOS (bản PCAT trước EFI)"
        },
        candidates
            .iter()
            .map(|(p, l)| format!("{} [{}]", p.display(), l))
            .collect::<Vec<_>>()
            .join(" -> ")
    ));
    let dst = stage_dir.join("boot.sdi");
    let mut tried: Vec<String> = Vec::new();
    for (cand, label) in &candidates {
        if cand.is_file() {
            match std::fs::copy(cand, &dst) {
                Ok(n) if dst.is_file() => {
                    // Bản chép từ ISO mang read-only — xoá luôn cho
                    // đồng nhất với mọi file staging khác.
                    let ro_note = if clear_readonly_file(&dst) {
                        " (đã xoá thuộc tính read-only)"
                    } else {
                        ""
                    };
                    log.line(&format!(
                        "boot.sdi: dùng {} [{}] ({} byte) -> {}{}",
                        cand.display(),
                        label,
                        n,
                        dst.display(),
                        ro_note
                    ));
                    return Ok(());
                }
                Ok(_) => {
                    let note = format!(
                        "{} [{}] (copy xong nhưng không thấy file đích)",
                        cand.display(),
                        label
                    );
                    log.line(&format!("boot.sdi: {} — thử tiếp vị trí sau.", note));
                    tried.push(note);
                }
                Err(e) => {
                    let note = format!("{} [{}] (lỗi copy: {})", cand.display(), label, e);
                    log.line(&format!("boot.sdi: {} — thử tiếp vị trí sau.", note));
                    tried.push(note);
                }
            }
        } else {
            log.line(&format!(
                "boot.sdi: thử {} [{}] — không có.",
                cand.display(),
                label
            ));
            tried.push(format!("{} [{}]", cand.display(), label));
        }
    }
    Err(format!(
        "Không tìm thấy boot.sdi — không thể tạo ramdisk WinPE. Đã thử {} vị trí:\n{}",
        candidates.len(),
        tried
            .iter()
            .map(|t| format!("- {}", t))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

/// Mount một WIM WinPE vào `mnt`, inject qsu_start.cmd /
/// startnet.cmd / winpeshl.ini / WinPEInstall.exe, rồi unmount
/// /Commit. Output DISM chảy vào log như trước VÀ được thu lại để
/// khi lỗi, thông điệp trả về là dạng đọc được (mã thoát thập phân
/// + hex + dòng lỗi của chính DISM) — không còn debug repr
/// `Ok(Some(...))` như thực địa v0.7.5. Mount lỗi thì thử dọn
/// trạng thái mount dở bằng /Unmount-Wim /Discard trước khi trả
/// lỗi. DISM gọi kèm /English như mọi lệnh DISM khác của wizard
/// để dòng lỗi trích ra ổn định trên mọi ngôn ngữ Windows.
fn mount_inject_winpe(
    log: &Arc<Logger>,
    wim: &Path,
    index: u32,
    mnt: &Path,
    stage_dir: &Path,
    settings: &Settings,
) -> Result<(), String> {
    // Chốt chặn cuối ngay trước mount: file WIM phải ghi được.
    // exec_clean đã chuẩn hoá thuộc tính sau khi chép từ ISO, nhưng
    // mount_inject tự đảm bảo lần nữa — kể cả khi WIM là file có
    // sẵn trong staging từ lần chạy trước (wizard dùng lại staging)
    // hay bản vừa export. DISM mount một WIM read-only để sửa sẽ
    // lỗi 0xC1510111 (xem clear_readonly_file).
    if clear_readonly_file(wim) {
        log.line(&format!(
            "Đã xoá thuộc tính read-only của {} ngay trước khi mount.",
            wim.display()
        ));
    }
    // Dọn thư mục mount còn sót từ lần thử trước (nếu có) rồi tạo mới.
    let _ = std::fs::remove_dir_all(mnt);
    let _ = std::fs::create_dir_all(mnt);
    let wim_arg = format!("/WimFile:{}", wim.to_string_lossy());
    let idx_arg = format!("/Index:{}", index);
    let mnt_arg = format!("/MountDir:{}", mnt.to_string_lossy());
    log.line(&format!(
        "$ dism /English /Mount-Wim {} {} {}",
        wim_arg, idx_arg, mnt_arg
    ));
    let mnt_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let log2 = log.clone();
    let mnt_lines2 = mnt_lines.clone();
    let mount_res = cmd::run_streaming(
        "dism",
        &["/English", "/Mount-Wim", &wim_arg, &idx_arg, &mnt_arg],
        move |line| {
            let t = line.trim();
            if !t.is_empty() {
                log2.line(&format!("  | {}", t));
                if let Ok(mut v) = mnt_lines2.lock() {
                    v.push(t.to_string());
                }
            }
        },
    );
    let mount_code = match &mount_res {
        Ok(c) => *c,
        Err(_) => None,
    };
    if mount_res.is_err() || mount_code != Some(0) {
        // Dọn trạng thái mount dở (nếu có) để lần mount sau không
        // vướng "mount directory already in use". Nhưng mount đã
        // THẤT BẠI thì thường không có gì để dọn và /Unmount-Wim
        // /Discard trả mã 50 — đó là nhiễu vô hại: chỉ ghi một dòng
        // log trung tính, KHÔNG để output/mã của nó thành lỗi được
        // báo (lỗi được báo luôn là lỗi mount ở dưới).
        match cmd::run_capture("dism", &["/English", "/Unmount-Wim", &mnt_arg, "/Discard"]) {
            Ok(out) if out.code == Some(0) => {
                log.line("Đã dọn trạng thái mount dở bằng /Unmount-Wim /Discard.");
            }
            _ => {
                log.line("Không có trạng thái mount dở cần dọn sau mount thất bại (bỏ qua).");
            }
        }
        return Err(match &mount_res {
            Err(e) => format!("không chạy được DISM để mount WinPE: {}", e),
            Ok(_) => {
                let collected = mnt_lines
                    .lock()
                    .map(|v| v.join("\n"))
                    .unwrap_or_default();
                cmd_fail_msg("DISM mount WinPE", mount_code, &collected)
            }
        });
    }
    let sys32 = mnt.join("Windows").join("System32");
    let start = qsu_start_cmd(&settings.staging_folder);
    let _ = std::fs::write(sys32.join("qsu_start.cmd"), &start);
    let _ = std::fs::write(sys32.join("startnet.cmd"), &start);
    let _ = std::fs::write(
        sys32.join("winpeshl.ini"),
        "[LaunchApps]\r\n%SYSTEMDRIVE%\\Windows\\System32\\qsu_start.cmd\r\n",
    );
    let _ = std::fs::copy(stage_dir.join("WinPEInstall.exe"), sys32.join("WinPEInstall.exe"));
    log.line("Đã inject qsu_start.cmd / startnet.cmd / winpeshl.ini / WinPEInstall.exe vào WinPE.");
    log.line(&format!("$ dism /English /Unmount-Wim {} /Commit", mnt_arg));
    let um_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let log3 = log.clone();
    let um_lines2 = um_lines.clone();
    let unmount_res = cmd::run_streaming(
        "dism",
        &["/English", "/Unmount-Wim", &mnt_arg, "/Commit"],
        move |line| {
            let t = line.trim();
            if !t.is_empty() {
                log3.line(&format!("  | {}", t));
                if let Ok(mut v) = um_lines2.lock() {
                    v.push(t.to_string());
                }
            }
        },
    );
    let unmount_code = match &unmount_res {
        Ok(c) => *c,
        Err(_) => None,
    };
    if unmount_res.is_err() || unmount_code != Some(0) {
        return Err(match &unmount_res {
            Err(e) => format!("không chạy được DISM để unmount WinPE: {}", e),
            Ok(_) => {
                let collected = um_lines
                    .lock()
                    .map(|v| v.join("\n"))
                    .unwrap_or_default();
                cmd_fail_msg("DISM unmount WinPE (/Commit)", unmount_code, &collected)
            }
        });
    }
    let _ = std::fs::remove_dir_all(mnt);
    Ok(())
}

/// Đóng gói lại WinPE thành WIM MỘT ẢNH bootable (v0.7.15).
///
/// Gốc lỗi thực địa (log install-20261117-1746.log, máy
/// DESKTOP-704R89L, ISO mod): wizard chọn đúng index 1 theo tên
/// ("Microsoft Windows PE"), mount + inject + commit thành công,
/// BCD ramdisk một lần đăng ký đúng công thức — nhưng khi boot,
/// winload nạp ảnh "Microsoft Windows Setup" (index 2) thay vì
/// ảnh PE đã inject, máy lên thẳng Windows Setup đòi driver.
/// Nguyên nhân: winpe.wim staging vẫn mang CẢ HAI ảnh và header
/// của nó không chỉ định ảnh PE đã inject là ảnh boot (WIM mod
/// không có "Boot Index", chi tiết index báo "WIM Bootable: No").
///
/// Sửa tận gốc theo đúng nguyên tắc của tool gốc (nó trích ảnh PE
/// ra WIM riêng một ảnh): xuất RIÊNG index vừa inject sang
/// winpe_single.wim bằng /Export-Image /Bootable /Compress:Max,
/// xoá bản hai-ảnh, đổi tên bản một-ảnh thành winpe.wim. WIM
/// đích chỉ còn đúng một ảnh và ảnh đó mang cờ boot — winload
/// không còn ảnh nào khác để nạp nhầm.
///
/// Đây là bước ĐÓNG GÓI, không phải cổng kiểm chứng: hàm không
/// đọc/phán cờ bootable từ output DISM (cổng phán quyết đó đã gỡ
/// ở v0.7.11 và không quay lại). Chỉ khi chính lệnh export lỗi,
/// hoặc file đích không tồn tại/rỗng, mới dừng — dừng TRƯỚC đăng
/// ký BCD, staging chưa hề có mục boot nào được ghi.
///
/// Trả về kích thước (byte) của winpe.wim sau đóng gói lại.
fn repack_winpe_single_image(log: &Arc<Logger>, wim: &Path, index: u32) -> Result<u64, String> {
    let single = wim.with_file_name("winpe_single.wim");
    // File đích còn sót từ lần chạy trước phải xoá trước — DISM
    // sẽ APPEND thêm index vào file đã tồn tại thay vì tạo mới,
    // và toàn bộ ý nghĩa "một ảnh" sẽ mất.
    let _ = clear_readonly_file(&single);
    let _ = std::fs::remove_file(&single);
    let src_arg = format!("/SourceImageFile:{}", wim.to_string_lossy());
    let sidx_arg = format!("/SourceIndex:{}", index);
    let dst_arg = format!("/DestinationImageFile:{}", single.to_string_lossy());
    log.line(&format!(
        "$ dism /English /Export-Image {} {} {} /Bootable /Compress:Max",
        src_arg, sidx_arg, dst_arg
    ));
    let exp_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let log_e = log.clone();
    let exp_lines2 = exp_lines.clone();
    let export_res = cmd::run_streaming(
        "dism",
        &["/English", "/Export-Image", &src_arg, &sidx_arg, &dst_arg, "/Bootable", "/Compress:Max"],
        move |line| {
            let t = line.trim();
            if !t.is_empty() {
                log_e.line(&format!("  | {}", t));
                if let Ok(mut v) = exp_lines2.lock() {
                    v.push(t.to_string());
                }
            }
        },
    );
    let export_code = match &export_res {
        Ok(c) => *c,
        Err(_) => None,
    };
    if export_res.is_err() || export_code != Some(0) {
        return Err(match &export_res {
            Err(e) => format!("không chạy được DISM để đóng gói lại WinPE: {}", e),
            Ok(_) => {
                let collected = exp_lines
                    .lock()
                    .map(|v| v.join("\n"))
                    .unwrap_or_default();
                cmd_fail_msg("DISM đóng gói lại WinPE (Export-Image một ảnh)", export_code, &collected)
            }
        });
    }
    // File đích phải tồn tại và không rỗng mới được thay bản gốc.
    let single_size = match std::fs::metadata(&single) {
        Ok(m) if m.len() > 0 => m.len(),
        Ok(_) => {
            return Err(format!(
                "DISM báo export thành công nhưng file {} rỗng — dừng trước đăng ký BCD, winpe.wim gốc giữ nguyên.",
                single.display()
            ));
        }
        Err(e) => {
            return Err(format!(
                "DISM báo export thành công nhưng không thấy file {}: {} — dừng trước đăng ký BCD, winpe.wim gốc giữ nguyên.",
                single.display(),
                e
            ));
        }
    };
    // Thay tên: xoá bản hai-ảnh, bản một-ảnh đã inject mang tên
    // winpe.wim — BCD (trỏ \<staging>\winpe.wim) và engine WinPE
    // ở các bước sau hoàn toàn không thay đổi.
    let _ = clear_readonly_file(wim);
    std::fs::remove_file(wim).map_err(|e| {
        format!("không xoá được winpe.wim hai-ảnh để thay bằng bản một ảnh: {}", e)
    })?;
    std::fs::rename(&single, wim).map_err(|e| {
        format!("không đổi tên được winpe_single.wim thành winpe.wim: {}", e)
    })?;
    // Bản mới không được mang read-only (DISM/engine các bước sau
    // và lần chạy kế tiếp đều cần ghi được).
    let _ = clear_readonly_file(wim);
    log.line(&format!(
        "Đã đóng gói lại WinPE: winpe.wim nay chỉ còn một ảnh (index {} đã inject), {} byte.",
        index, single_size
    ));
    Ok(single_size)
}

fn exec_clean(
    shared: &SharedRef,
    sender: &nwg::NoticeSender,
    log: &Arc<Logger>,
    settings: &Settings,
    exe_dir: &Path,
    plan: &ExecPlan,
    data_root: &Path,
) {
    let fail = |msg: String| {
        log.line(&format!("LỖI: {}", msg));
        lock_shared(shared).exec_done = Some(Err(msg));
        sender.notice();
    };
    let stage_dir = data_root.join(&settings.staging_folder);

    // --- Staging: thư mục + WinPEInstall.exe
    set_exec_progress(shared, sender, 18, "Chuẩn bị thư mục staging...");
    // Phòng thủ tầng cuối: không bao giờ xoá đệ quy nếu đường dẫn staging
    // trỏ về gốc ổ Data hoặc chính ổ OS (settings đã được kiểm hợp lệ ở
    // qsu_common::Settings::load, đây là chốt chặn bổ sung).
    if stage_dir == data_root || stage_dir == PathBuf::from(format!("{}:\\", plan.os_letter)) {
        fail("Đường dẫn thư mục staging không an toàn (trùng gốc ổ đĩa). Dừng.".to_string());
        return;
    }
    if stage_dir.exists() {
        let _ = std::fs::remove_dir_all(&stage_dir);
    }
    if let Err(e) = std::fs::create_dir_all(&stage_dir) {
        fail(format!("không tạo được thư mục staging: {}", e));
        return;
    }
    let wpi_src = [
        exe_dir.join("install").join("WinPEInstall.exe"),
        exe_dir.join("WinPEInstall.exe"),
    ]
    .into_iter()
    .find(|p| p.exists());
    match wpi_src {
        Some(p) => {
            if let Err(e) = std::fs::copy(&p, stage_dir.join("WinPEInstall.exe")) {
                fail(format!("không copy được WinPEInstall.exe vào staging: {}", e));
                return;
            }
        }
        None => {
            fail("Thiếu WinPEInstall.exe (cần đặt tại modules\\install\\WinPEInstall.exe cạnh wizard).".to_string());
            return;
        }
    }

    if let Err(e) = qsu_common::post_install::copy_host_telegram_config(&stage_dir) {
        fail(format!("Không chuẩn bị được cấu hình thông báo Telegram: {}", e));
        return;
    }
    // [Cơ chế] (v0.7.24): dòng đầu ca staging ghi rõ hồ sơ triển
    // khai — hai ca test v0.7.21/v0.7.22 chạy nhầm hồ sơ STANDARD
    // vì nút Core chưa BẬT (mặc định TẮT, không lưu trạng thái);
    // từ v0.7.24 nút Core nhớ trạng thái qua settings.json và log
    // phải cho thấy hồ sơ ngay từ đầu ca.
    if plan.core_profile {
        log.line("[Cơ chế] HỒ SƠ TRIỂN KHAI: CORE — ảnh rút gọn Tiny11 Core (stage.json imageProfile=core; unattend chỉ pha OOBE, không inject driver, không chép WinRE, không agent hậu cài).");
    } else {
        log.line("[Cơ chế] HỒ SƠ TRIỂN KHAI: STANDARD — hồ sơ đầy đủ (unattend đầy đủ + tự cài ứng dụng sau đăng nhập). Nếu đang cài ảnh rút gọn Tiny11 Core mà thấy dòng này là CHƯA BẬT nút 'Ảnh rút gọn (Tiny11 Core)' ở tab Tuỳ chọn cài đặt.");
    }
    // [Cơ chế] (v0.7.12 — dòng thuyết minh công nghệ trong log,
    // thuần chuỗi log, không đổi hành vi): staging trên ổ Data.
    log.line("[Cơ chế] Staging trên ổ Data: toàn bộ ca cài được chuẩn bị vào thư mục QSU-Stage trên ổ Data (bộ nguồn, WinPE, driver, stage.json); ổ Data đã được đánh dấu bằng file .qsu_data_drive và stage.json ghi lại toàn bộ kế hoạch — vào WinPE, engine tìm lại ổ Data bằng marker + đối chiếu serial trong stage.json, không dựa vào ký tự ổ (ký tự ổ trong WinPE khác hẳn trong Windows).");

    // --- Copy bộ nguồn cài (sources) từ ISO đã mount: 22% -> 52%
    let src_dir = PathBuf::from(format!("{}:\\sources", plan.mounted_letter));
    if !src_dir.exists() {
        fail(format!("không thấy thư mục sources tại {}", src_dir.display()));
        return;
    }
    set_exec_progress(shared, sender, 22, "Copy bộ nguồn cài (sources)...");
    let total = dir_size(&src_dir);
    let mut done = 0u64;
    let sh = shared.clone();
    let snd = sender.clone();
    let mut last_pct = 0u32;
    let copy_res = copy_dir_progress(&src_dir, &stage_dir.join("sources"), total, &mut done, &mut |d, t| {
        if t > 0 {
            let pct = 22 + (d.saturating_mul(30) / t) as u32;
            if pct != last_pct {
                last_pct = pct;
                set_exec_progress(&sh, &snd, pct, "Copy bộ nguồn cài (sources)...");
            }
        }
    });
    if let Err(e) = copy_res {
        fail(format!("copy sources thất bại: {}", e));
        return;
    }
    // Chuẩn hoá thuộc tính: file trên ISO luôn read-only và
    // fs::copy giữ nguyên ở đích — xoá read-only trên toàn bộ cây
    // sources đã chép để các bước sau (ghi/xoá/mount) không bị
    // chặn bởi thuộc tính của đĩa quang.
    let n_ro = clear_readonly_tree(&stage_dir.join("sources"));
    log.line(&format!(
        "Đã chuẩn hoá thuộc tính cây sources trong staging (xoá read-only trên {} file).",
        n_ro
    ));
    if let Err(e) = stage_boot_sdi(log, &src_dir, plan, &stage_dir) {
        fail(e);
        return;
    }
    log.line("Đã copy xong sources + boot.sdi vào staging.");
    // [Cơ chế] (v0.7.12): vì sao boot.sdi phải theo chuỗi dự phòng
    // phân biệt firmware — xem stage_boot_sdi.
    log.line("[Cơ chế] boot.sdi là file mồi để Boot Manager nạp WinPE từ ramdisk, và biến thể của nó phải khớp kiểu firmware (UEFI dùng bản EFI, Legacy/BIOS dùng bản PCAT) — nên wizard thử lần lượt: sources\\boot.sdi trong ISO, \\boot.sdi ở gốc ISO, bản Boot\\DVD của Windows đang chạy (biến thể theo firmware trước), cuối cùng là thư mục WinRE; mọi vị trí đã thử đều có trong log ở trên.");

    // --- Thư mục custom của người dùng -> staging\custom (winpe_install
    // sẽ chép tiếp vào ProgramData\QSU\custom của bản Windows mới)
    let custom_src = exe_dir.join("custom");
    if custom_src.is_dir() {
        let total = dir_size(&custom_src);
        let mut d2 = 0u64;
        let mut noop = |_: u64, _: u64| {};
        match copy_dir_progress(&custom_src, &stage_dir.join("custom"), total, &mut d2, &mut noop) {
            Ok(()) => log.line("Đã copy thư mục custom vào staging."),
            Err(e) => log.line(&format!("CẢNH BÁO: copy custom thất bại: {}", e)),
        }
    }

    // --- WinPE WIM: 56% -> 72%
    set_exec_progress(shared, sender, 56, "Chuẩn bị WinPE...");
    let wim_src = if !settings.winpe_wim_path.is_empty()
        && Path::new(&settings.winpe_wim_path).exists()
    {
        PathBuf::from(&settings.winpe_wim_path)
    } else {
        src_dir.join("boot.wim")
    };
    if !wim_src.exists() {
        fail(format!(
            "Không có WinPE WIM: settings.winpeWimPath không hợp lệ và ISO thiếu sources\\boot.wim ({}).",
            wim_src.display()
        ));
        return;
    }
    let wim_dst = stage_dir.join("winpe.wim");
    if let Err(e) = std::fs::copy(&wim_src, &wim_dst) {
        fail(format!("copy WinPE WIM thất bại: {}", e));
        return;
    }
    // ★ Fix gốc rễ của lỗi mount thực địa v0.7.5: boot.wim trên
    // ISO mang thuộc tính read-only, fs::copy giữ nguyên ở
    // winpe.wim, và DISM /Mount-Wim (mount để SỬA) từ chối file
    // read-only với lỗi 0xC1510111 "You do not have permissions to
    // mount and modify this image". Xoá read-only ngay sau khi
    // chép (mount_inject_winpe còn kiểm lại lần nữa trước mount).
    if clear_readonly_file(&wim_dst) {
        log.line("Đã xoá thuộc tính read-only của winpe.wim (thuộc tính chép theo từ ISO — DISM không mount để sửa được file read-only, lỗi 0xC1510111).");
    }
    // Chẩn đoán WIM trước khi mount: log toàn bộ /Get-WimInfo rồi
    // chọn index WinPE có chủ đích (Boot Index do DISM báo, hoặc
    // quy ước boot.wim) — thay closure dò thầm trước đây vốn nuốt
    // cả kết quả lẫn lỗi của /Get-WimInfo.
    let boot_index = probe_winpe_wim(log, &wim_dst);
    log.line(&format!("WinPE WIM: {} (boot index {})", wim_dst.display(), boot_index));
    // [Cơ chế] (v0.7.12): quy tắc chọn ảnh WinPE — xem probe_winpe_wim.
    log.line("[Cơ chế] Chọn ảnh WinPE trong boot.wim: ưu tiên 'Boot Index' do header của WIM ghi sẵn (ảnh mà chính bộ cài Microsoft sẽ boot); WIM không mang Boot Index — thường là ISO mod đóng gói lại — thì chọn theo tên ảnh 'Windows PE', không bao giờ chọn ảnh 'Windows Setup'.");

    // Mount WIM để inject startnet/winpeshl + WinPEInstall.exe.
    // Nếu mount bản gốc vẫn thất bại (ảnh thật sự không mount
    // được — nén solid/LZMS, dạng ESD, layout index lạ), còn một
    // đường dự phòng cuối: /Export-Image index WinPE sang WIM
    // mới nén chuẩn (/Compress:Max — KHÔNG phải recovery/LZMS
    // solid) rồi mount bản xuất. Bản xuất sau khi inject xong sẽ
    // thay tên thành winpe.wim, nên BCD (trỏ \<staging>\winpe.wim)
    // và engine WinPE ở các bước sau hoàn toàn không thay đổi.
    let mnt = stage_dir.join("mnt");
    // Index THẬT SỰ đã inject trong winpe.wim cuối cùng: đường
    // mount thẳng là boot_index; đường dự phòng export là index
    // của bản xuất (export_index). Bước đóng gói lại một ảnh
    // (v0.7.15) ở dưới xuất đúng index này.
    let mut injected_index = boot_index;
    if let Err(first_err) = mount_inject_winpe(log, &wim_dst, boot_index, &mnt, &stage_dir, settings) {
        log.line(&format!("Mount WinPE bản gốc thất bại: {}", first_err));
        log.line("Thử đường dự phòng cuối: DISM /Export-Image sang WIM nén chuẩn rồi mount lại.");
        set_exec_progress(shared, sender, 64, "Xuất lại WinPE WIM (nén chuẩn)...");
        let export_dst = stage_dir.join("winpe_export.wim");
        // File đích export còn sót (nếu có) phải xoá trước — DISM
        // sẽ APPEND thêm index vào file đã tồn tại thay vì tạo mới.
        let _ = clear_readonly_file(&export_dst);
        let _ = std::fs::remove_file(&export_dst);
        let src_arg = format!("/SourceImageFile:{}", wim_dst.to_string_lossy());
        let sidx_arg = format!("/SourceIndex:{}", boot_index);
        let dst_arg = format!("/DestinationImageFile:{}", export_dst.to_string_lossy());
        log.line(&format!(
            "$ dism /English /Export-Image {} {} {} /Compress:Max /Bootable",
            src_arg, sidx_arg, dst_arg
        ));
        let exp_lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let log_e = log.clone();
        let exp_lines2 = exp_lines.clone();
        let export_res = cmd::run_streaming(
            "dism",
            &["/English", "/Export-Image", &src_arg, &sidx_arg, &dst_arg, "/Compress:Max", "/Bootable"],
            move |line| {
                let t = line.trim();
                if !t.is_empty() {
                    log_e.line(&format!("  | {}", t));
                    if let Ok(mut v) = exp_lines2.lock() {
                        v.push(t.to_string());
                    }
                }
            },
        );
        let export_code = match &export_res {
            Ok(c) => *c,
            Err(_) => None,
        };
        if export_res.is_err() || export_code != Some(0) {
            let export_err = match &export_res {
                Err(e) => format!("không chạy được DISM để export WinPE: {}", e),
                Ok(_) => {
                    let collected = exp_lines
                        .lock()
                        .map(|v| v.join("\n"))
                        .unwrap_or_default();
                    cmd_fail_msg("DISM export WinPE", export_code, &collected)
                }
            };
            fail(format!(
                "Mount WinPE thất bại và đường dự phòng export cũng thất bại.\n- Lỗi mount bản gốc: {}\n- {}",
                first_err, export_err
            ));
            return;
        }
        log.line(&format!(
            "Đã xuất WinPE (index {}) sang {} — WIM nén chuẩn, không solid.",
            boot_index,
            export_dst.display()
        ));
        // Bản xuất chỉ còn đúng một index — vẫn dò lại + log cho chắc.
        let export_index = probe_winpe_wim(log, &export_dst);
        set_exec_progress(shared, sender, 68, "Mount WinPE (bản xuất lại)...");
        if let Err(second_err) = mount_inject_winpe(log, &export_dst, export_index, &mnt, &stage_dir, settings) {
            fail(format!(
                "Mount WinPE thất bại ở cả bản gốc lẫn bản xuất lại.\n- Lỗi mount bản gốc: {}\n- Lỗi mount bản xuất lại: {}",
                first_err, second_err
            ));
            return;
        }
        injected_index = export_index;
        // Thay tên: xoá bản gốc không mount được, bản xuất đã
        // inject mang tên winpe.wim — mọi bước sau (BCD,
        // stage.json, engine WinPE) giữ nguyên hệt như khi mount
        // thẳng thành công.
        let _ = clear_readonly_file(&wim_dst);
        match std::fs::remove_file(&wim_dst).and_then(|_| std::fs::rename(&export_dst, &wim_dst)) {
            Ok(()) => log.line("Đã thay winpe.wim gốc bằng bản xuất lại đã inject — giữ nguyên tên winpe.wim cho BCD/engine."),
            Err(e) => {
                fail(format!("không thay được winpe.wim bằng bản xuất lại: {}", e));
                return;
            }
        }
    }

    // --- Đóng gói lại WinPE thành WIM một ảnh bootable: 69% (v0.7.15)
    // Cả hai đường mount ở trên đều kết thúc với winpe.wim ĐÃ
    // INJECT nhưng vẫn mang CẢ HAI ảnh của boot.wim gốc, và
    // header của nó không chỉ định ảnh PE đã inject là ảnh boot
    // — thực địa 2026-11-17 (log 17:46) winload đã nạp nhầm ảnh
    // Windows Setup, máy lên thẳng Setup đòi driver thay vì
    // engine của tool. Đóng gói lại MỘT ẢNH áp dụng VÔ ĐIỀU KIỆN
    // cho mọi ca cài mới (ISO mod hay ISO gốc, bất kỳ index nào
    // đã chọn) để hành vi đồng nhất: xem
    // repack_winpe_single_image. Lỗi ở bước này dừng TRƯỚC đăng
    // ký BCD — đây là lỗi đóng gói, KHÔNG phải cổng kiểm chứng
    // cờ bootable (cổng đó vẫn gỡ hẳn, xem mốc 71% ngay dưới).
    set_exec_progress(shared, sender, 69, "Đóng gói lại WinPE (WIM một ảnh bootable)...");
    // [Cơ chế] (v0.7.15): vì sao phải đóng gói lại một ảnh.
    log.line("[Cơ chế] Đóng gói lại WinPE thành WIM một ảnh: boot.wim của ISO mang hai ảnh (Windows PE + Windows Setup) và header không chỉ định ảnh PE đã inject là ảnh boot, nên khi khởi động winload có thể nạp ảnh Setup thay vì engine của tool — wizard xuất riêng ảnh PE đã inject thành một WIM mới chỉ có một ảnh, gắn cờ bootable, để lúc boot winload chỉ có thể khởi động đúng engine của tool (cùng nguyên tắc đóng gói lại WinPE của tool gốc).");
    if let Err(e) = repack_winpe_single_image(log, &wim_dst, injected_index) {
        fail(format!(
            "Đóng gói lại WinPE thành WIM một ảnh thất bại — dừng trước đăng ký BCD, máy chưa có mục boot mới nào.\n{}",
            e
        ));
        return;
    }

    // --- Bỏ qua kiểm tra cờ bootable của WinPE WIM: 71% (v0.7.11)
    // Chỉ thị trực tiếp của người dùng ("Bỏ qua kiểm tra"): cổng
    // kiểm chứng + chuẩn hoá cờ bootable của v0.7.7 (phân tích
    // text /Get-WimInfo của DISM, xuất lại /Bootable rồi kiểm
    // chứng lại) đã chặn oan 4/4 lần chạy thực địa — kể cả với
    // ISO Microsoft gốc — trong khi các tool khác vẫn cài bình
    // thường từ chính các ISO đó. Cổng phán quyết đó vẫn GỠ HẲN:
    // wizard không đọc/phán cờ bootable ở bất kỳ đâu. Khác với
    // "nguyên trạng" của v0.7.11–v0.7.14, từ v0.7.15 winpe.wim
    // đã inject được ĐÓNG GÓI LẠI thành WIM một ảnh bootable ở
    // mốc 69% ngay trên (bước đóng gói thuần tuý, không phán
    // quyết, không kiểm chứng) rồi flow đi thẳng sang đăng ký
    // BCD. Mục boot vẫn là boot một lần (/bootsequence) nên nếu
    // WinPE không lên được, máy quay về Windows cũ và không có
    // gì bị xoá.
    set_exec_progress(shared, sender, 71, "Hoàn tất chuẩn bị WinPE...");
    log.line("Bỏ qua kiểm tra cờ bootable (theo yêu cầu) — không phán cờ boot bằng output DISM; WinPE đã inject được đóng gói lại thành WIM một ảnh bootable ở bước trên.");
    // [Cơ chế] (v0.7.12): lý do gỡ cổng kiểm chứng — bản đầy đủ ở
    // tab "Tài liệu" mục 9.
    log.line("[Cơ chế] Không kiểm tra cờ bootable: cổng kiểm chứng cũ phán cờ boot bằng cách đọc chữ trong output DISM /Get-WimInfo và đã kết luận sai 4/4 lần chạy thực địa — chặn cả ISO Microsoft gốc — trong khi các tool khác vẫn cài bình thường từ chính các ISO đó; an toàn được đảm bảo bằng mục boot một lần và đối chiếu serial thay vì cổng phán quyết này.");

    // --- Driver lưu trữ (IRST/NVMe) cho khâu inject offline: 74%
    set_exec_progress(shared, sender, 74, "Chuẩn bị driver lưu trữ...");
    // [Cơ chế] (v0.7.12): driver đi vào bản Windows mới bằng đường
    // inject offline trong WinPE, không phải cài sau khi Windows lên.
    log.line("[Cơ chế] Inject driver offline: driver lưu trữ (IRST/NVMe) được chép vào staging\\Drivers, và driver của Windows hiện tại đã sao lưu vào QSU-DriverBackup — trong WinPE, sau khi bung ảnh, engine nạp các driver này thẳng vào bản Windows mới bằng DISM /Add-Driver, nên máy chạy RAID/VMD vẫn nhận ổ ngay lần boot đầu tiên.");
    let drv_src = if !settings.storage_drivers_path.is_empty()
        && Path::new(&settings.storage_drivers_path).is_dir()
    {
        Some(PathBuf::from(&settings.storage_drivers_path))
    } else if exe_dir.join("drivers").is_dir() {
        Some(exe_dir.join("drivers"))
    } else {
        None
    };
    match drv_src {
        Some(d) => {
            let total = dir_size(&d);
            let mut dd = 0u64;
            let mut noop = |_: u64, _: u64| {};
            match copy_dir_progress(&d, &stage_dir.join("Drivers"), total, &mut dd, &mut noop) {
                Ok(()) => log.line(&format!("Đã copy driver lưu trữ từ {} vào staging\\Drivers.", d.display())),
                Err(e) => log.line(&format!("CẢNH BÁO: copy driver thất bại: {}", e)),
            }
        }
        None => log.line("Không có thư mục driver lưu trữ — bỏ qua inject driver offline."),
    }

    // --- autounattend.xml: 78%
    set_exec_progress(shared, sender, 78, "Sinh autounattend.xml...");
    if plan.core_profile {
        // [Cơ chế] (v0.7.23 → v0.7.26): hồ sơ CORE — wizard sinh
        // autounattend.xml CHỈ chứa pha oobeSystem (build_unattend
        // với core_profile=true): ca cài vẫn tự động qua màn hình
        // thiết lập + tạo tài khoản + AutoLogon như tiêu chí của
        // tool, trong khi engine ngoài file này ra giữ đúng công
        // thức WinNTSetup (không inject driver, không chép
        // WinRE, không agent hậu cài) + từ v0.7.26 ghi thêm
        // SetupComplete.cmd tối thiểu dọn cờ đổi mật khẩu.
        let unattend = build_unattend(settings, &plan.computer_name, &plan.admin_user, &plan.admin_password, true);
        if let Err(e) = std::fs::write(stage_dir.join("autounattend.xml"), unattend) {
            fail(format!("không ghi được autounattend.xml: {}", e));
            return;
        }
        log.line("[Cơ chế] Hồ sơ CORE (v0.7.23 → v0.7.26): wizard đã sinh autounattend.xml CHỈ chứa pha oobeSystem (tự qua màn hình thiết lập + tạo tài khoản + AutoLogon; không pha specialize, không FirstLogonCommands) và stage.json ghi imageProfile=core — trong WinPE, engine chép unattend này và ghi SetupComplete.cmd tối thiểu dọn cờ đổi mật khẩu (v0.7.26): không inject driver, không chép WinRE, không agent hậu cài.");
    } else {
        let unattend = build_unattend(settings, &plan.computer_name, &plan.admin_user, &plan.admin_password, plan.core_profile);
        // [Cơ chế] (v0.7.19): dọn cờ "phải đổi mật khẩu ở lần đăng nhập đầu" của tài khoản admin.
        log.line("[Cơ chế] autounattend.xml mang hai lệnh FirstLogonCommands cho tài khoản admin — `net user <tên> /logonpasswordchg:no` và đặt PasswordNeverExpires: tài khoản Setup tạo từ unattend mang cờ phải đổi mật khẩu ở lần đăng nhập đầu (pwdLastSet=0), không dọn cờ thì AutoLogon bị chặn ở màn hình đổi mật khẩu thay vì vào thẳng desktop (thực địa v0.7.18). Lệnh chạy ở lần đăng nhập đầu, áp dụng cả khi mật khẩu để trống lẫn khi có đặt.");
        if let Err(e) = std::fs::write(stage_dir.join("autounattend.xml"), unattend) {
            fail(format!("không ghi được autounattend.xml: {}", e));
            return;
        }
    }

    // --- BCD boot một lần: 84%
    set_exec_progress(shared, sender, 84, "Đăng ký BCD boot vào WinPE...");
    // [Cơ chế] (v0.7.12): /bootsequence là chốt an toàn chính của
    // toàn bộ luồng cài mới — xem register_bcd.
    log.line("[Cơ chế] Mục boot WinPE được đăng ký vào BCD dạng DÙNG MỘT LẦN (/bootsequence): chỉ lần khởi động kế tiếp máy mới vào WinPE, các lần sau không còn hiệu lực — nếu WinPE không lên được vì bất kỳ lý do gì, máy tự quay về Windows cũ, chưa có gì bị xoá và không bao giờ lặp boot vào WinPE.");
    let bcd_guid = match register_bcd(log, plan, settings) {
        Ok(g) => g,
        Err(e) => {
            fail(e);
            return;
        }
    };

    // --- stage.json: 92% — ĐÃ LOẠI BỎ apps / ultraviewerPassword /
    // adminPassword khỏi file ghi ra (bản rebuild không có agent hậu
    // cài; mật khẩu chỉ nằm trong autounattend.xml).
    set_exec_progress(shared, sender, 92, "Ghi stage.json...");
    // --- Thông báo trạng thái (v0.7.27): nếu bật thông báo và
    // đã cấu hình relay Zalo, xin TOKEN DÙNG MỘT LẦN từ relay để
    // engine trong WinPE gửi tin CÀI XONG / LỖI (stage.json chỉ
    // mang token một lần này — secret dài hạn KHÔNG vào
    // stage.json; xem ghi chú bảo mật trong stage.rs). Xin token
    // lỗi thì bỏ qua thông báo, KHÔNG chặn ca cài.
    let mut notify_relay_url = String::new();
    let mut notify_once_token = String::new();
    if settings.notify_enabled
        && !settings.zalo_relay_url.trim().is_empty()
        && !settings.zalo_relay_secret.trim().is_empty()
    {
        match qsu_common::notify::relay_register(
            settings.zalo_relay_url.trim(),
            settings.zalo_relay_secret.trim(),
            &plan.computer_name,
        ) {
            Ok(tok) => {
                notify_relay_url = settings.zalo_relay_url.trim().to_string();
                notify_once_token = tok;
                log.line("[Cơ chế] Thông báo (v0.7.27): đã xin token dùng một lần từ relay Zalo — engine trong WinPE sẽ gửi tin CÀI XONG / LỖI qua relay (best-effort, cần mạng tới server).");
            }
            Err(e) => {
                log.line(&format!(
                    "CẢNH BÁO: không xin được token thông báo từ relay Zalo ({}). Ca cài vẫn tiếp tục; tin CÀI XONG / LỖI sẽ không được gửi.",
                    e
                ));
            }
        }
    }
    let cfg = StageConfig {
        version: 1,
        mode: "clean".to_string(),
        image_file: format!("sources\\{}", plan.image_name),
        image_swm: if plan.image_name.eq_ignore_ascii_case("install.swm") {
            "sources\\install*.swm".to_string()
        } else {
            String::new()
        },
        image_index: plan.edition.index as u64,
        image_name: plan.edition.name.clone(),
        boot_mode: if plan.uefi { "UEFI".to_string() } else { "Legacy".to_string() },
        partition_mode: plan.part_mode.stage_str().to_string(),
        target_disk_serial: plan.target_disk_serial.clone(),
        rebuild_partitions: plan.rebuild_parts.clone(),
        os_volume_serial: plan.os_serial,
        os_volume_label: plan.os_label.clone(),
        data_volume_serial: plan.data_serial,
        bcd_guid,
        computer_name: plan.computer_name.clone(),
        admin_user: plan.admin_user.clone(),
        admin_password: String::new(), // không ghi vào stage.json
        ultraviewer_password: String::new(),
        apply_tweaks: plan.apply_tweaks,
        image_profile: if plan.core_profile {
            "core".to_string()
        } else {
            "standard".to_string()
        },
        apps: Vec::new(),
        driver_backup_rel: DRIVER_BACKUP_FOLDER.to_string(),
        logs_folder: settings.logs_folder.clone(),
        staging_folder: settings.staging_folder.clone(),
        notify_relay_url,
        notify_once_token,
    };
    let mut stage_json = cfg.to_json();
    if let Json::Obj(ref mut m) = stage_json {
        m.remove("apps");
        m.remove("ultraviewerPassword");
        m.remove("adminPassword");
    }
    // [Cơ chế] (v0.7.12): hai chốt nhận diện + chế độ phân vùng mà
    // stage.json sắp mang sang WinPE cho engine.
    log.line("[Cơ chế] Nhận diện ổ đích bằng SERIAL: stage.json ghi serial của phân vùng OS (và serial đĩa vật lý nếu xoá/chia lại phân vùng theo đĩa); trong WinPE, ngay trước khi format/xoá, engine đối chiếu lại serial thật — không khớp là dừng hẳn, không đoán theo ký tự ổ hay nhãn ổ. Riêng chế độ WIPE DISK, đĩa chứa ổ Data/staging bị cấm tuyệt đối làm đĩa đích; chế độ REBUILD cho phép trùng đĩa nhưng bảo vệ tuyệt đối phân vùng chứa staging.");
    match plan.part_mode {
        PartMode::WipeDisk => {
            log.line("[Cơ chế] Chế độ phân vùng WIPE DISK: xoá toàn bộ đĩa đích, chia lại theo công thức đã kiểm chứng thực địa (v0.7.22) — UEFI/GPT: EFI 260MB FAT32 + MSR 500MB + Recovery 1024MB (đặt đúng GUID và thuộc tính phân vùng WinRE; hồ sơ chuẩn chép Winre.wim vào và đăng ký bằng reagentc, hồ sơ CORE để trống như cài tay) + Windows (phần còn lại); Legacy/MBR: System Reserved 100MB (active) + Windows + Recovery 1024MB (id 27).");
        }
        PartMode::Rebuild => {
            log.line("[Cơ chế] Chế độ phân vùng REBUILD: chỉ các phân vùng hệ thống đã liệt kê trong stage.json (rebuildPartitions — EFI, MSR, Windows cũ, Recovery cũ) mới bị xoá và chia lại thành layout mới trong đúng vùng trống của chúng; mọi phân vùng data trên cùng đĩa được giữ nguyên — chúng không bao giờ được phép chọn để xoá, và phân vùng chứa bộ cài/staging được bảo vệ tuyệt đối. Trong WinPE, engine đối chiếu lại từng phân vùng bằng GUID (GPT) hoặc offset+kích thước (MBR), kiểm tra chúng tạo thành một vùng liên tục và đủ lớn trước khi xoá — lệch bất kỳ điểm nào là dừng hẳn (exit 13), không đụng vào đĩa.");
        }
        PartMode::Preserve => {
            log.line("[Cơ chế] Chế độ phân vùng PRESERVE: chỉ format phân vùng Windows cũ rồi bung ảnh lên đó — mọi phân vùng khác (dữ liệu, EFI, Recovery hiện có) được giữ nguyên và tái sử dụng.");
        }
    }
    if let Err(e) = std::fs::write(stage_dir.join("stage.json"), json::stringify(&stage_json)) {
        fail(format!("không ghi được stage.json: {}", e));
        return;
    }

    // Nguồn đã copy xong — gỡ mount ISO cho sạch
    dismount_iso(&plan.iso);

    set_exec_progress(shared, sender, 100, "Staging hoàn tất.");
    log.line("=== Staging cài mới hoàn tất — sẵn sàng reboot vào WinPE ===");
    // --- Gửi tin BẮT ĐẦU (v0.7.27): wizard còn ở Windows (mạng
    // đầy đủ) nên gửi qua CẢ HAI kênh đã cấu hình ngay tại đây;
    // tin CÀI XONG / LỖI do engine gửi qua relay bằng token dùng
    // một lần trong stage.json. Gửi lỗi chỉ ghi cảnh báo — không
    // đổi kết quả staging, không chặn reboot.
    if settings.notify_enabled {
        let mode_desc = format!(
            "{}{}",
            match plan.part_mode {
                PartMode::Preserve => "Cài mới — giữ nguyên phân vùng",
                PartMode::WipeDisk => "Cài mới — xoá toàn bộ đĩa đích, chia lại",
                PartMode::Rebuild => "Cài mới — chia lại các phân vùng hệ thống đã chọn",
            },
            if plan.core_profile {
                " — hồ sơ CORE (ảnh rút gọn)"
            } else {
                ""
            }
        );
        let start_text = qsu_common::notify::text_start(
            &plan.computer_name,
            &plan.edition.name,
            &mode_desc,
        );
        if !settings.telegram_bot_token.trim().is_empty()
            && !settings.telegram_chat_id.trim().is_empty()
        {
            match qsu_common::notify::telegram_send(
                settings.telegram_bot_token.trim(),
                settings.telegram_chat_id.trim(),
                &start_text,
            ) {
                Ok(()) => log.line("Đã gửi thông báo BẮT ĐẦU qua Telegram."),
                Err(e) => log.line(&format!(
                    "CẢNH BÁO: gửi thông báo BẮT ĐẦU qua Telegram thất bại: {}",
                    e
                )),
            }
        }
        if !settings.zalo_relay_url.trim().is_empty()
            && !settings.zalo_relay_secret.trim().is_empty()
        {
            match qsu_common::notify::relay_send(
                settings.zalo_relay_url.trim(),
                settings.zalo_relay_secret.trim(),
                &start_text,
            ) {
                Ok(()) => log.line("Đã gửi thông báo BẮT ĐẦU qua Zalo (relay)."),
                Err(e) => log.line(&format!(
                    "CẢNH BÁO: gửi thông báo BẮT ĐẦU qua Zalo (relay) thất bại: {}",
                    e
                )),
            }
        }
    }
    lock_shared(shared).exec_done = Some(Ok(
        "Đã staging xong. Khởi động lại máy để boot vào WinPE và tự động cài Windows mới (không cần thao tác thêm).".to_string(),
    ));
    sender.notice();
}

// ---------------------------------------------------------------------------
// Nội dung tĩnh cho tab "Hướng dẫn" và tab "Tài liệu" (v0.7.12)
// ---------------------------------------------------------------------------
// Hai tab chỉ-đọc cộng thêm vào khung tab của v0.7.10 — thuần tài
// liệu hiển thị, không handler, không chạm luồng nghiệp vụ. Nội
// dung dùng ký tự xuống dòng \n trong mã nguồn; lúc dựng giao
// diện được đổi sang \r\n (định dạng ô EDIT nhiều dòng của
// Windows cần). Bản Tài liệu đồng thời được lưu thành file
// docs/co_che_v0.7.13.md trong kho nguồn — sửa nội dung thì sửa
// cả hai nơi cho khớp.

/// Tab 3 — hướng dẫn sử dụng từng bước cho người dùng cuối.
const HUONG_DAN_TEXT: &str = r#"HƯỚNG DẪN CÀI ĐẶT WINDOWS BẰNG WIZARD NÀY

Tài liệu này luôn nằm sẵn ở tab "Hướng dẫn". Thao tác cài đặt thực hiện ở hai tab đầu: "Nguồn cài đặt" và "Tuỳ chọn cài đặt". Khối tiến độ + ô log + nút TIẾN HÀNH CÀI ĐẶT ở dưới cửa sổ luôn nhìn thấy ở mọi tab.

BƯỚC 1 — CHUẨN BỊ
- Để sẵn file ISO Windows trên ổ Data (ổ dữ liệu, ví dụ ổ D: hoặc E:) — KHÔNG để trên ổ C: sắp cài lại.
- Cắm sạc / nguồn AC cho máy trong suốt quá trình cài. Mất điện giữa chừng có thể làm hỏng bản Windows đang cài dở.
- Nếu cài mới: sao lưu trước dữ liệu quan trọng trên ổ C: (Desktop, Documents...) sang ổ Data — cài mới sẽ xoá ổ C:.

BƯỚC 2 — QUÉT MÁY (tab "Nguồn cài đặt", mục 1)
- Mở wizard và chờ mục "1 — KIỂM TRA HỆ THỐNG" quét xong: ổ OS, Boot Mode, ổ Data, nguồn điện và dòng Kết luận.
- Nếu Kết luận báo chưa đủ điều kiện (ví dụ BitLocker chưa giải mã xong, ổ Data không đủ chỗ trống), xử lý theo dòng hướng dẫn đó rồi bấm "Quét lại".
- Nếu máy có nhiều ổ Data, chọn đúng ổ chứa ISO trong danh sách ổ Data.

BƯỚC 3 — CHỌN ISO VÀ PHIÊN BẢN (tab "Nguồn cài đặt", mục 2 và 3)
- Các ISO tìm thấy trên ổ Data hiện ở mục 2. Chọn một ISO trong danh sách, hoặc bấm "Chọn ISO..." để chỉ tới file khác.
- Muốn tải ISO từ kho online: bấm "Tải danh mục", chọn bản cần tải rồi bấm "Tải ISO đã chọn" — file tải về được kiểm SHA-256 tự động.
- Sau khi chọn ISO, mục 3 liệt kê các phiên bản trong ISO — chọn đúng phiên bản cần cài (ví dụ Windows 11 Pro). Wizard tự chọn sẵn phiên bản trùng với Windows đang chạy.

BƯỚC 4 — CHỌN CHẾ ĐỘ CÀI (tab "Tuỳ chọn cài đặt", mục 4)
- "Cài đè (giữ ứng dụng + dữ liệu)": nâng cấp tại chỗ, giữ nguyên ứng dụng và dữ liệu. Windows cũ được giữ lại trong thư mục Windows.old để quay lui nếu cần.
- "Cài mới từ xa (qua WinPE)": cài sạch hoàn toàn qua môi trường WinPE — dùng khi muốn xoá trắng ổ C: hoặc cài từ xa mà không cần USB.

BƯỚC 5 — NẾU CÀI MỚI: CHỌN CHẾ ĐỘ PHÂN VÙNG VÀ Ổ ĐÍCH
- "Giữ nguyên phân vùng" (mặc định, nên dùng): chỉ format phân vùng Windows cũ, mọi phân vùng dữ liệu được giữ nguyên.
- "Cài lại phân vùng hệ thống (giữ data)": CHỈ xoá các phân vùng hệ thống (EFI, MSR, Windows cũ, Recovery) trên đĩa đích rồi chia lại layout mới trong đúng vùng trống đó; phân vùng DATA trên cùng đĩa được giữ nguyên. Đây là chế độ dành cho máy CHỈ CÓ 1 Ổ ĐĨA (Windows cũ và data nằm chung một đĩa). Khi chọn chế độ này, danh sách phân vùng hiện dạng có ô tick: phân vùng hệ thống được tick sẵn (có thể bỏ tick, ví dụ muốn giữ Recovery cũ), phân vùng Windows cũ bắt buộc phải xoá (không bỏ tick được), phân vùng data và phân vùng chứa bộ cài KHÔNG tick được và luôn được giữ nguyên.
- "Xoá toàn bộ ổ đĩa, chia lại từ đầu": xoá SẠCH mọi phân vùng trên đĩa đích rồi chia lại từ đầu. CHỈ dùng khi chắc chắn không còn dữ liệu cần giữ trên đĩa đó.
- Khi chọn một trong hai chế độ chia lại: chọn đúng ổ đĩa đích trong danh sách, đọc kỹ danh sách phân vùng sắp bị xoá và dòng cảnh báo, rồi GÕ ĐÚNG SERIAL của đĩa vào ô xác nhận. Thiếu hoặc sai serial thì wizard không cho cài.
- Riêng chế độ xoá toàn bộ: đĩa chứa ổ Data/staging không bao giờ được làm đĩa đích — wizard tự cấm, engine trong WinPE cũng kiểm tra lại độc lập. (Chế độ cài lại phân vùng hệ thống thì CHO PHÉP trùng đĩa, nhưng phân vùng chứa bộ cài vẫn được bảo vệ tuyệt đối.)

BƯỚC 6 — TÊN MÁY VÀ TÀI KHOẢN (chỉ áp dụng cho cài mới)
- Điền tên máy tính và tên tài khoản admin cho bản Windows mới.
- Mật khẩu: để trống cả hai ô = tài khoản KHÔNG đặt mật khẩu (phù hợp khi cài hàng loạt — người dùng tự đặt sau). Nếu đặt mật khẩu, hai ô phải giống hệt nhau. Mật khẩu không bao giờ được ghi vào log.

BƯỚC 7 — BẤM "TIẾN HÀNH CÀI ĐẶT"
- Bấm nút TIẾN HÀNH CÀI ĐẶT ở khối ghim dưới cửa sổ, đọc hộp tóm tắt toàn bộ kế hoạch rồi bấm Yes.
- Theo dõi thanh tiến độ và ô log ngay bên dưới. Các dòng bắt đầu bằng [Cơ chế] trong log giải thích ngắn gọn công nghệ đang áp dụng ở từng bước — bản đầy đủ nằm ở tab "Tài liệu".
- Giai đoạn chuẩn bị (staging) chạy ngay trong Windows hiện tại: chép bộ nguồn, chuẩn bị WinPE, sao lưu driver, đăng ký boot.

BƯỚC 8 — MÁY TỰ KHỞI ĐỘNG LẠI VÀ TỰ CÀI
- Chuẩn bị xong, máy tự khởi động lại vào WinPE và tự động cài Windows mới — KHÔNG cần thao tác thêm và KHÔNG tắt máy giữa chừng.
- Cài xong, máy khởi động vào Windows mới với đúng tên máy và tài khoản đã chọn ở Bước 6.
- Thư mục "custom" đặt cạnh wizard (nếu có) được chép nguyên trạng vào Windows mới tại ProgramData\QSU\custom. Wizard không tự chạy nội dung trong thư mục này.

NẾU WINPE KHÔNG LÊN ĐƯỢC
- Mục boot WinPE chỉ có hiệu lực MỘT LẦN: nếu WinPE không khởi động được, lần khởi động sau máy tự quay về Windows cũ và chưa có gì bị xoá.
- Khi đó hãy mở lại wizard, quét lại máy, và đọc file log trong thư mục QSU-Logs trên ổ Data để biết bước nào lỗi — hoặc gửi file log đó cho người hỗ trợ.
"#;

/// Tab 4 — tài liệu cơ chế công nghệ của CHÍNH bản build hiện
/// tại (cơ chế staging/WinPE của v0.7.11 + chế độ phân vùng
/// rebuild của v0.7.13 + đóng gói lại WinPE một ảnh của
/// v0.7.15 + engine tự gán ký tự ổ cho phân vùng boot sau khi
/// chia của v0.7.16 + mount ISO ba lớp của v0.7.17 + hiển thị
/// tiến trình hai bề mặt trong WinPE của v0.7.18 + chốt boot
/// cùng đĩa cho đường preserve, module WinPE tự mount ISO /
/// trình duyệt file tích hợp / chống kẹt nút, và dọn cờ đổi
/// mật khẩu trong autounattend của v0.7.19; các mục sau v0.7.19
/// xem file markdown — mới nhất là thông báo trạng thái cài
/// đặt qua Telegram / Zalo relay của v0.7.27, mục 19). Bản sao
/// dạng markdown: docs/co_che_v0.7.27.md.
const TAI_LIEU_TEXT: &str = r#"TÀI LIỆU CƠ CHẾ — CÀI MỚI QUA WINPE (cơ chế của bản build hiện tại)

Tài liệu này mô tả đúng những gì wizard thực thi khi chọn "Cài mới từ xa (qua WinPE)". Quá trình gồm hai giai đoạn: chuẩn bị (staging) chạy trong Windows hiện tại bằng wizard này, và cài thật chạy trong WinPE bằng engine WinPEInstall.exe.

1. STAGING TRÊN Ổ DATA
- Mọi thứ cần cho ca cài được chép vào thư mục staging (mặc định QSU-Stage) trên ổ Data: bộ nguồn sources, winpe.wim đã inject, boot.sdi, driver, autounattend.xml, thư mục custom và file stage.json.
- Ổ Data được đánh dấu bằng file marker ".qsu_data_drive" ở gốc ổ. Khi vào WinPE — nơi ký tự ổ được gán lại khác hẳn — engine tìm ổ Data bằng marker này, rồi đối chiếu serial phân vùng ghi trong stage.json.
- stage.json là "hợp đồng" giữa wizard và engine: phiên bản Windows + index, chế độ boot (UEFI/Legacy), chế độ phân vùng, serial phân vùng OS, serial đĩa đích, GUID mục BCD, tên máy, tài khoản. stage.json KHÔNG chứa mật khẩu (mật khẩu chỉ nằm trong autounattend.xml) và không chứa danh sách ứng dụng hậu cài.
- Nguồn sources được đọc từ ISO đã mount. Mount ISO có BA LỚP (từ v0.7.17): (1) ISO đã gắn sẵn (lần đọc trước, Explorer...) thì dùng lại ký tự ổ, đọc trạng thái qua Get-DiskImage; (2) mount bằng PowerShell Mount-DiskImage với lỗi ép thành lỗi dừng (terminating) — lỗi thật của cmdlet hiện nguyên văn trong hộp thoại thay vì thông báo rỗng như các bản trước; (3) nếu tầng PowerShell/cmdlet Storage không mount được (hay gặp trên các bản Windows lite bị cắt thành phần), wizard mount thẳng bằng VirtDisk API của Windows: mở ảnh ISO ở chế độ đọc-chỉ-đọc rồi dò ký tự ổ DVD mới xuất hiện sau khi gắn. Gỡ mount đối xứng theo đúng cơ chế đã mount: Dismount-DiskImage với mount PowerShell, DetachVirtualDisk với mount VirtDisk.

2. CHỌN ẢNH WINPE TRONG boot.wim
- boot.wim của ISO thường có 2 ảnh: "Microsoft Windows PE" và "Microsoft Windows Setup". Wizard chọn ảnh để mount và inject theo thứ tự: (1) "Boot Index" do header của WIM ghi sẵn — chính là ảnh mà bộ cài của Microsoft sẽ boot; (2) nếu header không mang Boot Index (hay gặp ở ISO mod đóng gói lại), chọn theo TÊN ảnh chứa "Windows PE" và không chứa "Setup".
- Ảnh được chọn được mount bằng DISM, inject các file khởi động của engine (qsu_start.cmd, startnet.cmd, winpeshl.ini) cùng WinPEInstall.exe, rồi unmount /Commit. Nếu mount bản gốc thất bại (WIM nén solid / dạng ESD), wizard xuất lại ảnh đó (Export-Image, nén Max) rồi mount bản xuất và thay tên thành winpe.wim — các bước sau không thay đổi.
- Ngay sau khi inject xong (từ v0.7.15, áp dụng cho MỌI ca cài mới): wizard ĐÓNG GÓI LẠI ảnh PE đã inject thành một WIM mới chỉ có đúng một ảnh — DISM /Export-Image /Bootable /Compress:Max xuất riêng index đã inject sang winpe_single.wim, xoá bản hai-ảnh, đổi tên bản một-ảnh thành winpe.wim. Lý do: winpe.wim sau inject vẫn mang cả hai ảnh của boot.wim gốc và header không chỉ định ảnh PE đã inject là ảnh boot, nên khi khởi động winload có thể nạp ảnh Windows Setup thay vì engine của tool (thực địa 2026-11-17: máy lên thẳng Windows Setup đòi driver). WIM một ảnh mang cờ boot thì winload chỉ có thể khởi động đúng engine — cùng nguyên tắc đóng gói lại WinPE của tool gốc. Đây là bước đóng gói thuần tuý: wizard vẫn KHÔNG đọc/phán cờ bootable từ output DISM (mục 9); chỉ khi chính lệnh export lỗi hoặc file đích rỗng thì dừng, và dừng TRƯỚC khi đăng ký BCD.

3. boot.sdi THEO FIRMWARE
- boot.sdi là file mồi để Boot Manager nạp WinPE từ ramdisk; biến thể của nó phải khớp kiểu firmware: UEFI dùng bản EFI, Legacy/BIOS dùng bản PCAT.
- Wizard lấy boot.sdi theo chuỗi dự phòng đã xếp theo firmware (UEFI thử bản EFI trước, Legacy thử bản PCAT trước); nguồn lần lượt là: sources\boot.sdi trong ISO -> \boot.sdi ở gốc ISO -> thư mục Boot\DVD của Windows đang chạy -> thư mục Recovery\WindowsRE của Windows đang chạy. Mọi vị trí đã thử đều được ghi vào log; không vị trí nào có file thì dừng với thông báo liệt kê đủ các vị trí.

4. DRIVER
- Trước khi staging, wizard sao lưu toàn bộ driver của Windows hiện tại (pnputil /export-driver) vào thư mục QSU-DriverBackup trên ổ Data.
- Driver lưu trữ (IRST/NVMe...) từ thư mục drivers cạnh wizard (hoặc đường dẫn khai trong settings) được chép vào staging\Drivers. Trong WinPE, sau khi bung ảnh Windows xong, engine inject offline cả hai nguồn vào bản Windows mới bằng DISM /Add-Driver /Recurse — staging\Drivers trước, rồi QSU-DriverBackup (tên thư mục đọc từ stage.json, chỉ inject khi tên tương đối an toàn và thư mục không rỗng; từ v0.7.14, các bản trước chỉ inject staging\Drivers nên driver của chính máy đó không được nạp lại) — máy chạy RAID/VMD vẫn nhận ổ ngay lần boot đầu tiên.

5. MỤC BOOT MỘT LẦN (/bootsequence)
- Wizard đăng ký vào BCD một cặp entry ramdisk (device options + loader) trỏ tới winpe.wim và boot.sdi trong staging, đặt đường dẫn loader theo firmware (winload.efi cho UEFI, winload.exe cho Legacy), rồi kích hoạt bằng /bootsequence — chỉ hiệu lực cho lần khởi động KẾ TIẾP, đúng một lần.
- Nhờ vậy, nếu WinPE không lên được vì bất kỳ lý do gì, lần khởi động sau đó máy tự quay về Windows cũ: hệ thống chưa bị thay đổi gì và không bao giờ xảy ra tình trạng lặp boot vào WinPE.

6. NHẬN DIỆN Ổ ĐÍCH BẰNG SERIAL
- Cả wizard lẫn engine đều KHÔNG nhận diện ổ đĩa theo ký tự ổ hay nhãn ổ (ký tự thay đổi trong WinPE, nhãn có thể bị đổi hoặc trùng). Căn cứ duy nhất là serial: serial của phân vùng (volume serial) và serial phần cứng của đĩa vật lý (đọc bằng IOCTL, không phân tích output chữ của lệnh nào).
- Wizard ghi serial phân vùng OS và serial đĩa đích vào stage.json. Trong WinPE, ngay trước bước format/xoá, engine đối chiếu lại serial thật: không khớp là dừng hẳn, không đụng vào bất kỳ ổ nào. Ở chế độ WIPE DISK, đĩa chứa ổ Data/staging bị cấm tuyệt đối làm đĩa đích — engine kiểm tra lại độc lập, không tin hoàn toàn vào wizard. Ở chế độ REBUILD (mục 7), đĩa chứa staging ĐƯỢC làm đích nhưng phân vùng staging tự nó được bảo vệ bằng đối chiếu vị trí vật lý (extent) — engine chứng minh nó không nằm trong danh sách xoá trước khi động vào đĩa.

7. BA CHẾ ĐỘ PHÂN VÙNG
- PRESERVE (mặc định): chỉ format phân vùng Windows cũ rồi bung ảnh lên đó; mọi phân vùng khác (dữ liệu, EFI, Recovery hiện có) được giữ nguyên và tái sử dụng.
- WIPE DISK: xoá toàn bộ đĩa đích và chia lại theo công thức đã kiểm chứng thực địa (từ v0.7.22). UEFI/GPT: EFI 260 MB FAT32 -> MSR 500 MB -> Recovery 1024 MB NTFS (đặt đúng GUID và thuộc tính phân vùng WinRE; hồ sơ chuẩn chép Winre.wim vào và đăng ký bằng reagentc, hồ sơ CORE để trống như cài tay) -> Windows NTFS (phần còn lại). Legacy/MBR: System Reserved 100 MB (đặt active) -> Windows -> Recovery 1024 MB (id 27). Chế độ này chỉ chạy khi kỹ thuật viên đã gõ đúng serial của đĩa ở wizard để xác nhận xoá.
- REBUILD (mới ở v0.7.13): chỉ xoá CÁC PHÂN VÙNG HỆ THỐNG ĐÃ CHỌN trên đĩa đích — EFI, MSR, Windows cũ, Recovery cũ — rồi chia lại layout mới (cùng layout như WIPE DISK) trong đúng VÙNG TRỐNG của chúng; mọi phân vùng data trên cùng đĩa được giữ nguyên tuyệt đối. Sinh ra cho máy chỉ có 1 ổ đĩa (Windows cũ và data chung đĩa, đĩa chứa staging cũng chính là đĩa đích). Ở wizard, danh sách phân vùng của đĩa đích hiện dạng ô tick: phân vùng hệ thống tick sẵn (bỏ tick được, trừ phân vùng Windows cũ là bắt buộc), phân vùng data và phân vùng chứa bộ cài không tick được; xác nhận cũng phải gõ đúng serial đĩa như WIPE DISK. stage.json ghi thêm mảng rebuildPartitions: từng phân vùng sẽ xoá kèm loại, kích thước, offset và GUID duy nhất của phân vùng (đĩa GPT). Trong WinPE, engine đối chiếu lại TỪNG phân vùng bằng GUID (GPT) hoặc offset + kích thước (MBR, dung sai 1 MB), kiểm tra phân vùng chứa staging không nằm trong danh sách xoá, kiểm tra các phân vùng cần xoá tạo thành MỘT vùng liên tục và vùng đó đủ lớn cho layout mới (tối thiểu: boot + Recovery + Windows 10 GB) — TẤT CẢ trước khi xoá bất cứ thứ gì; lệch bất kỳ điểm nào là dừng cứng với exit code 13, không đụng vào đĩa. Khi chia lại, các phân vùng mới được ghim đúng offset vào vùng vừa trống nên không bao giờ lấn sang phân vùng data kề bên; sau khi chia, engine xác minh lại cả phân vùng mới lẫn các phân vùng được giữ (còn nguyên vị trí và kích thước).
- TỰ GÁN KÝ TỰ Ổ CHO PHÂN VÙNG BOOT SAU KHI CHIA (từ v0.7.16 — áp dụng cho cả WIPE DISK và REBUILD): sau khi diskpart chia xong, engine đối chiếu lại layout thật trên đĩa đích. Phân vùng boot (EFI/System Reserved) có thể có mặt nhưng KHÔNG có ký tự ổ dùng được — thực địa đã gặp: diskpart báo "đã gán ký tự thành công" mà phân vùng EFI vẫn không có ký tự, và các bản trước dừng cứng ngay tại đó dù đĩa đã bị xoá trắng (máy mất cả Windows cũ lẫn đường vào Windows mới). Từ v0.7.16 engine TỰ CHỮA: tự gán ký tự cho phân vùng boot (ưu tiên S như script chia đĩa đã định; gán bằng script diskpart phụ, dự phòng bằng gán điểm mount trực tiếp lên volume), rồi kiểm chứng bằng cách TRUY CẬP THẬT đường dẫn gốc của ký tự đó — truy cập được mới tính là có ký tự, không tin riêng phần liệt kê. Chỉ khi phân vùng boot vắng mặt hẳn trong layout mới, hoặc tự gán + truy cập thử đều thất bại sau khi thử lại, engine mới dừng. Ký tự đã chốt được các bước sau (bung ảnh, bcdboot, chép WinRE) dùng tiếp như bình thường.

8. THƯ MỤC CUSTOM
- Thư mục "custom" đặt cạnh wizard được chép nguyên trạng vào staging, rồi vào ProgramData\QSU\custom của Windows mới. Wizard và engine chỉ CHÉP, không bao giờ thực thi nội dung trong đó — kỹ thuật viên tự chạy bộ cài / script của mình sau khi Windows mới đã lên.

9. KHÔNG KIỂM TRA CỜ BOOTABLE (đã gỡ ở v0.7.11)
- Các bản trước từng có một cổng kiểm chứng cờ bootable của winpe.wim: đọc chữ trong output DISM /Get-WimInfo, nếu phán "chưa đạt" thì xuất lại với /Bootable rồi kiểm chứng lại, vẫn "chưa đạt" thì chặn đứng trước khi đăng ký BCD. Thực địa cho thấy cổng này kết luận sai 4/4 lần chạy — chặn cả ISO Microsoft gốc — trong khi các tool khác vẫn cài bình thường từ chính các ISO đó.
- Theo yêu cầu của người dùng, từ v0.7.11 cổng này bị gỡ hẳn và KHÔNG quay lại: wizard không đọc/phán cờ bootable ở bất kỳ đâu. Từ v0.7.15, thay cho việc dùng WinPE nguyên trạng, wizard đóng gói lại ảnh PE đã inject thành WIM một ảnh bootable (mục 2) — đó là bước đóng gói cơ học để winload không nạp nhầm ảnh Setup, không kèm bất kỳ phán quyết hay kiểm chứng nào. Tính an toàn được đảm bảo bằng mục boot một lần (mục 5) và đối chiếu serial (mục 6), thay vì bằng một cổng phán quyết đã chứng minh là phán sai.

10. LOG
- Toàn bộ quá trình được ghi vào file install-<thời gian>.log trong thư mục QSU-Logs trên ổ Data, đồng thời hiện ở ô log của wizard. Các dòng bắt đầu bằng [Cơ chế] giải thích ngắn gọn công nghệ đang áp dụng tại bước đó.

11. HIỂN THỊ TIẾN TRÌNH TRONG WINPE (từ v0.7.18)
- Bối cảnh: ở các bản trước engine chỉ ghi tiến trình vào file log winpe-*.log trên ổ Data — màn hình WinPE không hiện gì (console của launcher trống, không có cửa sổ nào), khiến kỹ thuật viên tưởng máy treo và tắt nguồn giữa chừng trong khi DISM đã bung ảnh tới 82% (thực địa winpe-20261117-1852.log).
- Từ v0.7.18 engine có HAI bề mặt tiến trình. Bề mặt CHÍNH là console của chính launcher qsu_start.cmd: engine thừa hưởng console đó nên mọi dòng log đồng thời in thẳng ra console, giai đoạn hiện tại in thành dòng riêng, và phần trăm bung ảnh cập nhật liên tục tại chỗ bằng dòng \r "Đang bung ảnh: <edition> — NN%". DISM xả tiến độ bằng ký tự \r (không phải \n) nên engine đọc output của DISM theo từng đoạn cắt ở cả \r lẫn \n để phần trăm tới được màn hình ngay khi DISM chạy, không đợi tới lúc xong.
- Bề mặt PHỤ là màn hình tiến trình toàn màn hình tông xanh (thiết kế theo màn hình mẫu của tool gốc, thương hiệu của bản rebuild): tiêu đề "CÀI ĐẶT WINDOWS TỰ ĐỘNG", dòng cảnh báo không tắt máy, thẻ giữa hiện bước đang chạy + số % lớn + thanh tiến trình + đồng hồ thời gian đã trôi chạy từng giây (tín hiệu còn sống ngay cả khi DISM lâu không nhả % mới), kèm đường dẫn file log. Ngoài bước bung ảnh, số % là tiến độ tổng thể theo giai đoạn; trong bước bung ảnh, số % là phần trăm trực tiếp của DISM. Màn hình này mở trước mọi công việc và chạy trên luồng riêng — lỗi hiển thị (nếu có) không ảnh hưởng luồng cài đặt hay dòng console.
- Khi có lỗi, engine KHÔNG thoát và KHÔNG reboot: nó neo lại trong WinPE, giữ nguyên bước đang dừng + nội dung lỗi + đường dẫn log trên cả console lẫn màn hình tiến trình để kỹ thuật viên đọc/chụp, rồi tắt máy bằng nút nguồn. Chỉ khi cài xong xuôi engine mới tự reboot vào Windows mới như trước.

12. CHỐT BOOT CÙNG ĐĨA CHO ĐƯỜNG PRESERVE + MODULE WINPE TỰ MOUNT ISO (từ v0.7.19)
- Bối cảnh: ca thực địa 2026-11-18, cài theo phân vùng bằng module Cài Đặt Win = WinPE trên WinPE USB — DISM bung ảnh đạt 100% nhưng bước bcdboot thất bại, kỹ thuật viên phải tự gán ký tự cho phân vùng EFI rồi chạy bcdboot bằng tay mới boot được. Nguyên nhân: đường preserve cũ (cả engine lẫn module WinPE) tìm phân vùng EFI bằng cách quét các ổ ĐÃ CÓ ký tự ổ để tìm thư mục \EFI ở gốc, không thấy thì mountvol /s. Phân vùng EFI trên đĩa đích hôm đó không có ký tự ổ nào nên vô hình với cách tìm này; mountvol /s lại không phân biệt được đĩa khi máy có hai phân vùng EFI (mỗi đĩa một cái), có thể trỏ nhầm sang EFI của đĩa khác. Lỗi khi đó cũng chỉ là một câu chung chung, không kèm output của bcdboot nên không biết lý do thật.
- Từ v0.7.19 bước chốt boot của cả hai đường preserve dùng chung một hàm trong thư viện dùng chung (disk::finalize_boot): xác định đĩa vật lý chứa phân vùng Windows đích, tìm phân vùng boot TRÊN CHÍNH ĐĨA ĐÓ theo loại phân vùng — GPT là phân vùng EFI System theo GUID loại, MBR là phân vùng System Reserved đang active (không có thì dùng chính phân vùng Windows) — thiếu ký tự ổ thì tự gán bằng cùng cơ chế tự chữa của mục 7 (script diskpart phụ / SetVolumeMountPointW, chốt bằng truy cập thử đường dẫn thật), rồi mới chạy bcdboot (/f UEFI cho GPT, /f BIOS cho MBR) với output được thu trọn vào log. Thành công chỉ được tính khi KIỂM CHỨNG ĐƯỢC file boot thật trên phân vùng boot (EFI\Microsoft\Boot\bootmgfw.efi + BCD với UEFI; bootmgr + Boot\BCD với BIOS) — mã thoát 0 của bcdboot chưa đủ; lần chạy đầu chưa đạt thì tự chạy lại một lần. Thất bại hẳn thì thông báo lỗi mang theo nguyên văn output của bcdboot. Không tồn tại đường nào bỏ qua bước boot lặng lẽ. Hai chế độ WIPE DISK / REBUILD giữ nguyên đường chốt boot riêng như trước.
- Module Cài Đặt Win = WinPE bỏ phụ thuộc PowerShell khi chọn nguồn: file ISO được mount bằng VirtDisk API gốc (cùng cơ chế lớp 3 của mục 1, nay đặt trong thư viện dùng chung) rồi đọc thẳng sources\install.* — các WinPE phổ biến không có cmdlet Storage vẫn mount được ISO; gỡ mount đối xứng bằng DetachVirtualDisk khi đổi nguồn hoặc đóng module. Nếu chính VirtDisk cũng không attach được, thông báo hướng dẫn mount sẵn ISO bằng công cụ của WinPE rồi chọn/gõ thẳng file sources\install.wim (hoặc .esd/.swm) — đúng đường đã cứu ca thực địa. Nút "Chọn file..." nay mở TRÌNH DUYỆT FILE TÍCH HỢP của module (liệt kê mọi ổ đĩa, duyệt thư mục, lọc *.iso/*.wim/*.esd/*.swm, hiện tên file đầy đủ, nhấp đúp để chọn) thay cho hộp thoại file của Windows — hộp thoại đó không mở được trong các WinPE này; ô gõ đường dẫn trực tiếp vẫn dùng được như trước. Và sau bất kỳ lần nạp nguồn nào — thành công hay lỗi — các nút bấm đều được mở khoá lại: luồng nền được bọc bắt lỗi toàn diện và hộp thoại chỉ hiện sau khi trạng thái nút đã cập nhật, nút chỉ khoá trong lúc thao tác thật sự đang chạy (bản cũ từng kẹt nút "Nạp nguồn" sau một lần nạp lỗi, phải mở lại module).
- autounattend.xml (cả wizard lẫn module WinPE sinh) nay mang hai lệnh chạy ở lần đăng nhập đầu cho tài khoản admin: net user <tên> /logonpasswordchg:no và đặt mật khẩu không hết hạn (PasswordNeverExpires qua PowerShell Set-LocalUser). Tài khoản Setup tạo từ unattend mang cờ "phải đổi mật khẩu ở lần đăng nhập đầu tiên" (pwdLastSet = 0) — thực địa bản v0.7.18 cho thấy AutoLogon bị chặn ở màn hình đổi mật khẩu (User name / Old password / New password) thay vì vào thẳng desktop. Hai lệnh này dọn cờ đó ngay ở lần đăng nhập đầu, áp dụng cả khi mật khẩu để trống lẫn khi có đặt, để ca cài hoàn toàn không cần người trông vẫn về được desktop.

15. HỒ SƠ CORE CHO ẢNH RÚT GỌN — TINY11 CORE (v0.7.21 → v0.7.22 → v0.7.23)

Ảnh Windows rút gọn kiểu Tiny11 Core (chế từ ISO gốc Microsoft rồi cắt giảm component) boot vòng lỗi CRITICAL_PROCESS_DIED (0xEF) ở lần khởi động đầu khi cài bằng hồ sơ chuẩn. Diễn tiến xử lý qua thực địa 2026-11-20: hồ sơ CORE của v0.7.21 (unattend rút gọn + SetupComplete.cmd) vẫn 0xEF; hồ sơ CORE của v0.7.22 (bỏ hẳn unattend, driver, WinRE, agent — bản sao đúng công thức cài tay WinNTSetup) boot QUA được 0xEF nhưng bản Windows mới vào OOBE gốc của ảnh, kỹ thuật viên phải ngồi bấm qua các màn hình thiết lập và nhập tên thủ công — trái tiêu chí cài TỰ ĐỘNG của bộ công cụ. Từ v0.7.23, hồ sơ CORE giữ phần công thức WinNTSetup đã kiểm chứng (không inject driver, không chép WinRE, không agent/SetupComplete) và đưa tự động hoá quay lại bằng một autounattend.xml CHỈ chứa pha oobeSystem:
- Wizard sinh autounattend.xml rút gọn cho hồ sơ CORE và ghi vào staging; engine chép vào Windows\Panther như hồ sơ chuẩn. Module Cài Đặt Win=WinPE sinh cùng một nội dung (ô tick CORE độc lập — tick CORE là tự sinh unattend này từ các ô tài khoản, không cần tick thêm "Tự thiết lập Windows").
- Nội dung unattend CORE: CHỈ pha oobeSystem — locale, các cờ bỏ qua màn hình thiết lập (EULA, tài khoản online, wifi), LocalAccount thuộc nhóm Administrators (mật khẩu tuỳ chọn như hồ sơ chuẩn) và AutoLogon 1 lần. KHÔNG có pha specialize, KHÔNG có FirstLogonCommands.
- Đánh đổi: tên máy và múi giờ không đặt được (hai thiết lập này chỉ nằm ở pha specialize — tên máy để Windows tự sinh dạng DESKTOP-xxxxxxx, múi giờ theo mặc định của ảnh; đổi sau trong Windows nếu cần); không có lệnh dọn cờ "phải đổi mật khẩu ở lần đăng nhập đầu" — nếu ảnh đặt cờ đó cho tài khoản tạo từ unattend, lần đăng nhập đầu có thể dừng ở màn hình đổi mật khẩu (để trống cả ba ô rồi Enter) như ca thực địa v0.7.18; không tự cài ứng dụng sau đăng nhập.
- stage.json ghi "imageProfile": "core" (hồ sơ chuẩn ghi "standard"; stage.json cũ không có trường này được hiểu là standard). Staging sinh bởi v0.7.22 (không có autounattend.xml) vẫn tương thích: engine khi đó cài thuần như WinNTSetup và vào OOBE gốc của ảnh.

16. HỒ SƠ TRIỂN KHAI ĐƯỢC LƯU TRẠNG THÁI VÀ GHI RÕ TRONG LOG (v0.7.24)

Hai ca test thực địa v0.7.21 và v0.7.22 chạy nhầm hồ sơ STANDARD trong khi tưởng đang test hồ sơ CORE: nút "Ảnh rút gọn (Tiny11 Core)" ở wizard mặc định TẮT, nằm ở tab Tuỳ chọn và không lưu trạng thái — ca chạy vì thế nhận đủ unattend đầy đủ, agent hậu cài và bước inject driver sao lưu, rồi vẫn lỗi 0xEF như hồ sơ chuẩn (đọc rõ trong log engine của ca: có chép autounattend, có chuẩn bị tự cài ứng dụng, có thử inject driver từ bản sao lưu). Từ v0.7.24:
- Nút Core ở wizard khởi tạo từ khoá defaultCoreProfile trong settings.json cạnh wizard, và TỰ GHI LẠI vào settings.json mỗi lần bật/tắt — bật một lần là các lần mở wizard sau vẫn hiện BẬT.
- Log của wizard (ngay đầu ca staging), của engine (theo stage.json) và của module WinPE đều có dòng "[Cơ chế] HỒ SƠ TRIỂN KHAI: CORE ..." hoặc "STANDARD ..." — ca chạy thuộc hồ sơ nào, mở log là biết ngay, không phải suy đoán từ ảnh chụp màn hình.
- Khi cài ảnh rút gọn, luôn kiểm tra nút Core đang BẬT (hoặc dòng HỒ SƠ TRIỂN KHAI: CORE trong log staging) trước khi để máy reboot vào WinPE; thấy STANDARD nghĩa là ca chạy chưa bật Core.

17. CHUẨN HOÁ ENTRY BOOT SAU BCDBOOT — SỬA LỖI 0xEF (v0.7.25)

Chẩn đoán thực địa ngày 2026-11-20 trên DESKTOP-704R89L với ảnh Tiny11 Core (build 26300.9457) đã khép lại: hồ sơ CORE chạy thật (log engine ghi imageProfile=core, không inject driver, không chép WinRE, unattend chỉ pha OOBE) vẫn lỗi 0xEF; xoá hẳn unattend khỏi Windows\Panther vẫn 0xEF; đổi phân vùng Recovery từ kiểu WinRE sang kiểu dữ liệu thường vẫn 0xEF; thư mục Panther của bản lỗi chỉ có setup.etl — setupact.log và setuperr.log chưa từng được tạo, máy chết trước khi Windows Setup bắt đầu ghi log. Phép thử quyết định: WinNTSetup bung đúng ảnh đó lên đúng layout phân vùng tool vừa tạo thì boot qua "Getting ready", vào OOBE và lên desktop bình thường. Trên chính bản Windows đang chạy tốt đó, chọn lại entry boot cũ "Windows 11" do bcdboot của engine tạo thì lại CRITICAL_PROCESS_DIED (0xEF). Cùng đĩa, cùng phân vùng, cùng một bản Windows: entry của WinNTSetup boot được, entry của bcdboot thì không — thủ phạm là entry boot do bcdboot tạo ra từ ảnh rút gọn.

Đối chiếu bcdedit /enum all của cả hai entry: các phần tử còn lại đều giống nhau (device và osdevice đều trỏ đúng phân vùng C:), chỉ khác ở path và systemroot — entry của bcdboot mang path "\\Windows\system32\winload.efi" và systemroot "\\Windows" (hai dấu gạch chéo ở đầu), entry chuẩn mang "\Windows\system32\winload.efi" và "\Windows". Nguyên nhân: bcdboot dựng entry theo BCD-Template lưu trong chính ảnh (Windows\System32\config\BCD-Template); template của ảnh rút gọn này đã bị phía build ảnh sửa thành dạng hai gạch chéo, nên mọi ca cài dùng bcdboot với ảnh này đều nhận entry lỗi, bất kể hồ sơ CORE hay STANDARD — đó cũng là lý do ảnh gốc Microsoft (template lành) cài bằng tool vẫn boot bình thường. WinNTSetup tự dựng entry nên không dính lỗi này.

Từ v0.7.25, ngay sau khi bcdboot thành công, mọi đường cài đều chạy thêm bước chuẩn hoá entry boot (hàm dùng chung disk::normalize_boot_entry trong qsu_common):
- Đọc entry {default} trong BCD store trên phân vùng boot và ghi log nguyên trạng hai dòng path/systemroot TRƯỚC khi sửa — log thực địa của ca cài sẽ cho thấy tận mắt giá trị bcdboot vừa tạo.
- Đặt lại path = "\Windows\system32\winload.efi" (UEFI) hoặc "\Windows\system32\winload.exe" (BIOS) và systemroot = "\Windows" bằng bcdedit /store.
- Áp dụng ở cả ba điểm gọi bcdboot: engine chế độ wipeDisk/rebuild; finalize_boot dùng chung (engine cài theo phân vùng + module cài theo phân vùng); module chế độ wipeDisk.
- Bước chuẩn hoá chỉ sửa hai phần tử trên; nếu bcdedit lỗi, log ghi CẢNH BÁO và ca cài vẫn tiếp tục — kết quả thành bại của bcdboot không đổi.
- Hồ sơ CORE/STANDARD, unattend và layout phân vùng giữ nguyên như v0.7.24.

18. CORE VÀO THẲNG DESKTOP: KHÔI PHỤC SETUPCOMPLETE DỌN CỜ MẬT KHẨU (v0.7.26)

Ca thực địa thành công của v0.7.25 (2026-11-20) vẫn còn một điểm dừng thủ công cuối cùng: bản Windows mới boot qua 0xEF, chạy hết pha setup, nhưng ở lần đăng nhập đầu AutoLogon bị chặn ở màn hình "The user's password must be changed before signing in" — đúng đánh đổi đã ghi ở mục 15 (unattend chỉ chứa pha oobeSystem nên không có lệnh nào dọn cờ "phải đổi mật khẩu ở lần đăng nhập đầu" cho tài khoản tạo từ unattend). Kỹ thuật viên phải bấm OK rồi để trống cả ba ô mật khẩu mới vào được desktop — chưa đạt trọn vẹn tiêu chí cài TỰ ĐỘNG.

Từ v0.7.26, hồ sơ CORE khôi phục đúng cơ chế của v0.7.21 mà v0.7.22 đã bỏ: sau khi bung ảnh, engine WinPEInstall (và module Cài Đặt Win=WinPE ở bước ghi unattend) ghi thêm một file Windows\Setup\Scripts\SetupComplete.cmd TỐI THIỂU — chỉ một dòng lệnh "net user <tên tài khoản admin> /logonpasswordchg:no". Windows chạy SetupComplete.cmd bằng quyền SYSTEM ở cuối pha setup, TRƯỚC lần đăng nhập đầu tiên, nên cờ được dọn kịp và AutoLogon vào thẳng desktop. Nếu ảnh đã có sẵn SetupComplete.cmd (từ bộ tuỳ biến của ảnh), tool NỐI thêm dòng lệnh vào cuối file chứ không ghi đè nội dung sẵn có.

Vì sao cơ chế này an toàn để đưa lại: nó từng bị loại ở v0.7.22 trong quá trình loại trừ lỗi 0xEF theo hướng "giữ đúng công thức WinNTSetup tối thiểu". Chẩn đoán ngày 2026-11-20 (mục 17) đã chốt thủ phạm 0xEF là entry boot do bcdboot dựng theo BCD-Template đã bị sửa trong ảnh — KHÔNG phải SetupComplete, không phải unattend, không phải agent hậu cài. Cơ chế SetupComplete vì thế được minh oan hoàn toàn và được khôi phục nguyên trạng cho hồ sơ CORE.

Phạm vi thay đổi của v0.7.26 chỉ nằm ở hồ sơ CORE:
- Hồ sơ STANDARD không đổi: cờ mật khẩu vẫn được dọn bằng hai lệnh FirstLogonCommands trong unattend đầy đủ như trước.
- Hồ sơ CORE vẫn giữ: unattend chỉ pha oobeSystem, không inject driver, không chép WinRE, không agent hậu cài, không FirstLogonCommands — chỉ thêm file SetupComplete.cmd tối thiểu nói trên. Log của engine/module có dòng "[Cơ chế] Hồ sơ CORE (v0.7.26): đã ghi SetupComplete.cmd tối thiểu ..." để đối chiếu khi nghiệm thu.
- Đánh đổi "có thể gặp màn hình đổi mật khẩu ở lần đăng nhập đầu" ở mục 15 nay ĐÃ HẾT: ca cài CORE từ v0.7.26 vào thẳng desktop bằng AutoLogon. Các đánh đổi còn lại của mục 15 (không đặt trước tên máy và múi giờ) giữ nguyên.

19. THÔNG BÁO TRẠNG THÁI CÀI ĐẶT QUA TELEGRAM / ZALO RELAY (v0.7.27)

Từ v0.7.27 wizard có thêm tab "Thông báo": kỹ thuật viên bật thông báo và cấu hình một hoặc cả hai kênh để nhận trạng thái ca cài trên điện thoại, không phải ngồi canh máy:
- Kênh Telegram: bot Telegram của CHÍNH kỹ thuật viên (tạo qua @BotFather trên Telegram). Tab có nút "Lấy Chat ID" — kỹ thuật viên nhắn trước một tin bất kỳ cho bot của mình, wizard đọc getUpdates của Bot API và tự điền Chat ID kèm tên người gửi — và nút "Gửi thử Telegram". Bot Token và Chat ID chỉ lưu trong settings.json cạnh wizard.
- Kênh Zalo qua relay Golden Farm: một relay nhỏ chạy trên chính server của đơn vị (cạnh bot Zalo Golden Farm) nhận trạng thái rồi chuyển tiếp vào Zalo của kỹ thuật viên. Wizard và engine chỉ POST về địa chỉ relay kèm khoá bí mật (Relay URL + Relay Secret khai trong tab); bộ công cụ không đăng nhập Zalo và không giữ phiên Zalo nào.
- Tin BẮT ĐẦU do wizard gửi ngay khi staging hoàn tất — máy còn ở Windows hiện tại, mạng đầy đủ — qua cả hai kênh đã cấu hình. Tin CÀI XONG / LỖI do engine trong WinPE gửi và CHỈ đi qua kênh Zalo relay: ngay lúc staging, wizard dùng Relay Secret xin trước một TOKEN DÙNG MỘT LẦN từ relay (hết hạn tối đa 12 giờ, tối đa 3 tin) và ghi token đó vào stage.json; engine dùng token để gửi đúng tin kết thúc của ca. Relay Secret dài hạn và Bot Token Telegram KHÔNG BAO GIỜ vào stage.json hay file log.
- Nội dung mọi tin chỉ gồm: loại trạng thái (bắt đầu / cài xong / lỗi kèm bước dừng), tên máy và thời gian. Không gửi bất kỳ thông tin đăng nhập nào — không mật khẩu, không ID/mật khẩu công cụ điều khiển từ xa.
- Mọi lần gửi đều là best-effort với timeout ngắn (8 giây): gửi lỗi chỉ ghi CẢNH BÁO vào log, không chặn ca cài, không đổi mã thoát của engine, không chặn reboot hay màn hình giữ lỗi. Trong WinPE tin chỉ đi được khi WinPE có driver mạng và với tới được địa chỉ relay (cùng LAN / Tailscale); không có mạng thì ca cài vẫn chạy bình thường, chỉ thiếu tin kết thúc.
- Nút "Lưu cấu hình thông báo" ghi 5 khoá (notifyEnabled, telegramBotToken, telegramChatId, zaloRelayUrl, zaloRelaySecret) vào settings.json tại đúng vị trí khoá, giữ nguyên các khoá khác, đồng thời cập nhật bản cấu hình trong bộ nhớ để ca staging bắt đầu ngay sau đó dùng đúng giá trị vừa lưu."#;

// ---------------------------------------------------------------------------
// Trạng thái giao diện (luồng UI) + dựng cửa sổ
// ---------------------------------------------------------------------------
// Bố cục thích ứng của cửa sổ wizard (v0.7.9, gom tab ở v0.7.10)
//
// Bối cảnh thực địa: cửa sổ wizard có kích thước thiết kế cố định
// (ngoài 1060x1102) — trên màn hình nhỏ (laptop 1366x768, session từ
// xa thấp, màn hình phụ độ phân giải thấp) phần đáy chứa thanh tiến
// độ + ô log + nút TIẾN HÀNH bị tràn khỏi màn hình, kỹ thuật viên
// phải ẩn taskbar mới bấm được nút cài đặt. Từ v0.7.9 cửa sổ được
// căn giữa vùng làm việc, bị chặn ở kích thước vùng làm việc, cho
// phép kéo giãn (khung WS_THICKFRAME) và bố cục được tính lại mỗi
// lần đổi kích thước theo bảng rect thiết kế: khối đáy bám đáy, các
// danh sách ở giữa hấp thụ phần chiều cao thay đổi.
//
// v0.7.10 gom biểu mẫu thành HAI TAB cho gọn chiều cao: tab 1
// "Nguồn cài đặt" (kiểm tra hệ thống + nguồn ISO + phiên bản), tab
// 2 "Tuỳ chọn cài đặt" (chế độ cài + đĩa đích + tài khoản). Khối
// tiến độ + ô log + hai nút hành động ghim cố định DƯỚI khung tab,
// luôn nhìn thấy ở cả hai tab. Kích thước thiết kế rút xuống
// ~1036x748 (ngoài) nên trên màn hình 1366x768 toàn bộ cửa sổ nằm
// gọn trên taskbar mà không cần co giãn gì thêm. Bố cục thích ứng
// chia theo ba sân toạ độ (GROUP_MAIN / GROUP_TAB1 / GROUP_TAB2):
// khung tab là một phần tử co giãn của sân chính (FLEX_TABS), hai
// trang tab được NWG tự khớp kích thước theo khung, và các danh
// sách trong từng trang hấp thụ đúng phần delta chiều cao của
// trang đó.
// ---------------------------------------------------------------------------

/// Chỉ số các phần tử co giãn trong bảng hệ số co giãn. Năm danh
/// sách như cũ, cộng FLEX_TABS: khung tab của v0.7.10 — nó hấp thụ
/// phần thay đổi chiều cao của cửa sổ chính rồi truyền xuống hai
/// trang tab (trang nào cũng cao bằng khung hiển thị của khung tab).
const FLEX_ISO: usize = 0;
const FLEX_CAT: usize = 1;
const FLEX_ED: usize = 2;
const FLEX_PARTS: usize = 3;
const FLEX_LOG: usize = 4;
const FLEX_TABS: usize = 5;
/// Chiều cao tối thiểu của từng phần tử co giãn (đủ hiện 1–2 dòng,
/// không bao giờ co về 0 hay chồng lên control khác; khung tab giữ
/// tối thiểu 240 để hai trang vẫn thao tác được).
const FLEX_MIN_H: [i32; 6] = [30, 30, 30, 30, 44, 240];
/// Trọng số chia phần chiều cao THỪA (cửa sổ cao hơn thiết kế):
/// ô log được ưu tiên mở rộng nhất.
const FLEX_GROW_W: [i32; 6] = [2, 1, 1, 1, 3, 2];
/// Trọng số chia phần chiều cao THIẾU (cửa sổ thấp hơn thiết kế):
/// ô log co ít nhất vì đó là nơi theo dõi quá trình cài đặt.
const FLEX_SHRINK_W: [i32; 6] = [3, 2, 2, 2, 1, 2];
/// Nhóm bố cục (v0.7.10): mỗi control thuộc đúng một "sân" toạ độ —
/// cửa sổ chính, trang tab 1 hoặc trang tab 2 — và `apply_layout`
/// tính riêng cho từng sân theo kích thước client của sân đó.
const GROUP_MAIN: u8 = 0;
const GROUP_TAB1: u8 = 1;
const GROUP_TAB2: u8 = 2;
/// Sân tab 3 "Hướng dẫn" và tab 4 "Tài liệu" (v0.7.12): mỗi trang
/// chỉ có MỘT ô text phủ toàn trang. Ô text đăng ký với chỉ số
/// FLEX_TABS — trong lần gọi `apply_layout` cho sân của nó, nó là
/// phần tử co giãn duy nhất nên hấp thụ trọn delta chiều cao của
/// trang, đúng vai trò khung tab đang giữ ở sân chính; các mảng hệ
/// số co giãn dùng chung không cần thêm chỉ số mới.
const GROUP_TAB3: u8 = 3;
const GROUP_TAB4: u8 = 4;
/// Sân tab 5 "Thông báo" (v0.7.27): trang cấu hình thông báo
/// trạng thái — mọi control đều chiều cao cố định (không phần
/// tử co giãn), `apply_layout` chỉ dịch/nở theo bề ngang.
const GROUP_TAB5: u8 = 5;
/// Kích thước ngoài tối thiểu của cửa sổ (áp qua WM_GETMINMAXINFO):
/// dưới mức này các control bắt đầu thiếu chỗ thực sự.
const MIN_WINDOW_W: i32 = 900;
const MIN_WINDOW_H: i32 = 640;

/// Cách một control xử lý khi bề ngang cửa sổ thay đổi.
#[derive(Clone, Copy, PartialEq)]
enum XMode {
    /// Giữ nguyên toạ độ x (cột trái / giữa).
    Fixed,
    /// Dịch x theo đúng delta bề ngang (cột phải — bám lề phải).
    ShiftRight,
}

/// Cách một control xử lý bề rộng khi cửa sổ thay đổi.
#[derive(Clone, Copy, PartialEq)]
enum WMode {
    Fixed,
    /// Bề rộng cộng/trừ đúng delta bề ngang (danh sách + nhãn dài).
    Grow,
}

/// Một control trong bảng bố cục: rect ở bản thiết kế gốc cộng vai
/// trò của nó khi cửa sổ đổi kích thước. Toạ độ luôn tương đối với
/// cửa sổ cha trực tiếp của control (cửa sổ chính hoặc trang tab).
#[derive(Clone)]
struct LayoutItem {
    hwnd: HWND,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    x_mode: XMode,
    w_mode: WMode,
    /// Some(chỉ số) nếu đây là một phần tử co giãn theo chiều cao
    /// (xem các hằng FLEX_*).
    flex: Option<usize>,
    /// Sân toạ độ của control (GROUP_MAIN / GROUP_TAB1 / GROUP_TAB2)
    /// — `apply_layout` chỉ tính các control cùng sân trong một lần
    /// gọi, với mốc thiết kế và kích thước client của sân đó.
    group: u8,
}

// ---------------------------------------------------------------------------
struct Model {
    settings: Settings,
    exe_dir: PathBuf,
    preflight: Option<Preflight>,
    /// Chẩn đoán liệt kê đĩa của lần quét gần nhất (đã ghi ra file
    /// wizard-scan-*.log lúc quét xong — v0.7.2; vẫn chép thêm vào
    /// file log khi bắt đầu cài để cặp screenshot + log chẩn đoán được).
    disk_diag: Vec<String>,
    data_letter: Option<char>,
    logger: Option<Arc<Logger>>,
    local_isos: Vec<IsoFile>,
    catalogue: Vec<CatalogueImage>,
    loading_iso: Option<PathBuf>,
    mounted_iso: Option<PathBuf>,
    mounted_letter: Option<char>,
    image_name: Option<String>,
    editions: Vec<EditionInfo>,
    mode_clean: bool,
    /// Chế độ phân vùng khi cài mới — mặc định Preserve (giữ nguyên
    /// phân vùng là hành vi mặc định an toàn).
    part_mode: PartMode,
    /// Rebuild: trạng thái tick của danh sách phân vùng ứng với đĩa
    /// đang hiển thị trong lst_disk_parts (song song với
    /// `preflight.disks[cmb_disk].partitions`) — giữ lại khi đổi
    /// qua đĩa khác rồi quay lại. `rebuild_checks_disk` = số đĩa mà
    /// trạng thái này thuộc về (None = chưa có / phải tính mặc định).
    rebuild_checks: Vec<bool>,
    rebuild_checks_disk: Option<u32>,
    /// Số đĩa mà lst_disk_parts đang hiển thị (để biết khi nào cần
    /// chụp lại trạng thái tick trước khi vẽ lại danh sách).
    parts_disk: Option<u32>,
    apply_tweaks: bool,
    /// Hồ sơ CORE cho ảnh Windows rút gọn (Tiny11 Core) — v0.7.21:
    /// unattend tối thiểu, xem `build_unattend`.
    core_profile: bool,
    /// Tab "Thông báo" (v0.7.27): đang có thao tác mạng chạy nền
    /// (lấy Chat ID / gửi thử) — bốn nút của tab bị khoá cho tới
    /// khi `refresh` nhận kết quả qua `Shared::notify_done`.
    notify_busy: bool,
    busy_ed: bool,
    running: bool,
    last_clean: bool,
    log_rendered: usize,
    /// Vị trí/kích thước cửa sổ trước khi bắt đầu cài đặt — lúc bắt
    /// đầu, cửa sổ được dịch lên để vùng tiến độ/log ở đáy không bị
    /// khuất khỏi màn hình (xem `shift_window_for_exec`); khi quá
    /// trình kết thúc, cửa sổ trở về đúng vị trí này.
    saved_window_rect: Option<(i32, i32, i32, i32)>,
    /// Bảng bố cục thích ứng (v0.7.9): rect THIẾT KẾ của mọi control,
    /// ghi lại ngay sau khi dựng giao diện, kèm sân toạ độ của từng
    /// control (v0.7.10) — xem `apply_layout`. Rỗng cho tới khi
    /// `build_ui` điền vào.
    layout: Vec<LayoutItem>,
    /// Kích thước vùng client của cửa sổ chính ở bản thiết kế gốc
    /// (đo ngay khi tạo cửa sổ theo thông số thiết kế v0.7.10). Mọi
    /// delta bố cục của sân chính đều tính so với mốc này.
    design_client: (i32, i32),
    /// Kích thước vùng client của một trang tab ở bản thiết kế gốc
    /// (đo sau khi NWG khớp trang vào khung tab ở kích thước thiết
    /// kế; hai trang luôn bằng nhau). Mốc delta cho hai sân tab.
    design_page: (i32, i32),
}

struct App {
    notice: nwg::Notice,
    /// Khung tab + các trang (2 trang biểu mẫu ở v0.7.10, cộng 2
    /// trang tài liệu ở v0.7.12, cộng trang "Thông báo" ở
    /// v0.7.27). Giữ trong App để handle sống
    /// cùng tiến trình như mọi control khác; NWG tự hiện/ẩn trang
    /// theo lựa chọn của người dùng và tự khớp kích thước trang khi
    /// khung đổi kích thước. `tabs`/`tab_opt` chỉ giữ quyền sở hữu
    /// (không đọc trực tiếp — trang 1 được đọc qua `tab_src` trong
    /// `layout_window`) nên mang allow(dead_code) có chủ đích.
    #[allow(dead_code)]
    tabs: nwg::TabsContainer,
    tab_src: nwg::Tab,
    #[allow(dead_code)]
    tab_opt: nwg::Tab,
    /// Hai trang tài liệu của v0.7.12 — được đọc trong
    /// `layout_window` để đo kích thước trang như `tab_src`.
    tab_guide: nwg::Tab,
    tab_docs: nwg::Tab,
    /// Trang "Thông báo" (v0.7.27) — được đọc trong
    /// `layout_window` như hai trang tài liệu.
    tab_notify: nwg::Tab,
    l_os: nwg::Label,
    l_boot: nwg::Label,
    l_data_info: nwg::Label,
    l_power: nwg::Label,
    l_verdict: nwg::Label,
    cmb_data: nwg::ComboBox<String>,
    lst_iso: nwg::ListBox<String>,
    lst_cat: nwg::ListBox<String>,
    lst_ed: nwg::ListBox<String>,
    lbl_dl: nwg::Label,
    lbl_ed_info: nwg::Label,
    lbl_stage_info: nwg::Label,
    lbl_exec: nwg::Label,
    bar_dl: nwg::ProgressBar,
    bar_exec: nwg::ProgressBar,
    inp_computer: nwg::TextInput,
    inp_user: nwg::TextInput,
    inp_pw1: nwg::TextInput,
    inp_pw2: nwg::TextInput,
    txt_log: nwg::TextBox,
    /// Hai ô text chỉ-đọc phủ toàn trang của tab "Hướng dẫn" và
    /// tab "Tài liệu" (v0.7.12) — thuần hiển thị, không handler
    /// nào đọc lại nội dung, chỉ giữ quyền sở hữu như các control
    /// khác trong App.
    #[allow(dead_code)]
    txt_guide: nwg::TextBox,
    #[allow(dead_code)]
    txt_docs: nwg::TextBox,
    btn_rescan: nwg::Button,
    btn_browse: nwg::Button,
    btn_fetch: nwg::Button,
    btn_download: nwg::Button,
    btn_mode_inplace: nwg::Button,
    btn_mode_clean: nwg::Button,
    btn_part_preserve: nwg::Button,
    btn_part_wipe: nwg::Button,
    btn_part_rebuild: nwg::Button,
    lbl_disk_cap: nwg::Label,
    cmb_disk: nwg::ComboBox<String>,
    lbl_disk_hint: nwg::Label,
    /// Danh sách phân vùng của đĩa đích: ListView dạng bảng — ở chế
    /// độ rebuild nó mang checkbox theo từng phân vùng hệ thống
    /// (cùng helper Win32 thô như module Tuỳ Chỉnh Windows).
    lst_disk_parts: nwg::ListView,
    lbl_disk_warn: nwg::Label,
    lbl_serial_cap: nwg::Label,
    inp_disk_serial: nwg::TextInput,
    /// Dòng định danh đĩa đích đầy đủ (số đĩa — model — dung lượng
    /// — serial, KHÔNG cắt) ngay trên ô nhập serial (v0.7.17).
    lbl_disk_id: nwg::Label,
    /// Phản hồi trực tiếp cạnh ô nhập serial: đã khớp / chưa khớp
    /// serial đĩa đích theo từng ký tự gõ (v0.7.17).
    lbl_serial_fb: nwg::Label,
    btn_tweaks: nwg::Button,
    btn_core: nwg::Button,
    btn_start: nwg::Button,
    btn_close: nwg::Button,
    /// Tab "Thông báo" (v0.7.27): ô bật thông báo, hai nhóm cấu
    /// hình kênh (Telegram trực tiếp / Zalo qua relay), các nút
    /// thao tác mạng và dòng trạng thái của tab.
    chk_notify: nwg::CheckBox,
    inp_tg_token: nwg::TextInput,
    inp_tg_chat: nwg::TextInput,
    inp_zalo_url: nwg::TextInput,
    inp_zalo_secret: nwg::TextInput,
    btn_tg_fetch: nwg::Button,
    btn_tg_test: nwg::Button,
    btn_zalo_test: nwg::Button,
    btn_notify_save: nwg::Button,
    lbl_notify_status: nwg::Label,
    model: RefCell<Model>,
    shared: SharedRef,
}

fn build_ui() -> (nwg::Window, Rc<App>) {
    // --- Fonts: chuẩn dùng chung qsu_common::ui (v0.7.20) ---
    let font_title = qsu_common::ui::font_title();
    let font_section = qsu_common::ui::font_section();
    let font_normal = qsu_common::ui::font_normal();
    let font_small = qsu_common::ui::font_small();
    let font_button = qsu_common::ui::font_button();

    // --- Cửa sổ ---
    let mut window = Default::default();
    nwg::Window::builder()
        .size((1036, 748))
        .position((120, 30))
        .title("Cài Đặt Windows Tự Động — Utility Rebuild")
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE | nwg::WindowFlags::MINIMIZE_BOX)
        .build(&mut window)
        .expect("window");
    qsu_common::ui::set_window_icon(&window);
    // Kích thước client ở bản thiết kế gốc: cửa sổ vừa tạo đúng theo
    // thông số thiết kế v0.7.10 (ngoài 1036x748) nên số đo này là
    // mốc "thiết kế" cho bố cục thích ứng — xem `adapt_window_frame`
    // và `apply_layout`.
    let (client_w, client_h) = {
        let mut rc: RECT = unsafe { std::mem::zeroed() };
        unsafe {
            GetClientRect(window.handle.hwnd().unwrap(), &mut rc);
        }
        (rc.right, rc.bottom)
    };

    macro_rules! add_label {
        ($parent:expr, $text:expr, $font:expr, $pos:expr, $size:expr) => {{
            let mut lbl = Default::default();
            nwg::Label::builder()
                .text($text)
                .font(Some($font))
                .parent($parent)
                .build(&mut lbl)
                .expect("label");
            lbl.set_position($pos.0, $pos.1);
            lbl.set_size($size.0, $size.1);
            lbl
        }};
    }

    // --- Header (sân chính, phía trên khung tab) ---
    let _t1 = add_label!(&window, "Cài Đặt Windows Tự Động", font_title, (16, 6), (640, 30));
    let _t2 = add_label!(
        &window,
        "Cài Windows từ xa: cài đè giữ dữ liệu hoặc cài mới qua WinPE — không cần người tại chỗ",
        font_small, (16, 36), (900, 20)
    );

    // --- Khung tab (v0.7.10) ---
    // Hai trang biểu mẫu nằm trong khung tab; khối tiến độ + ô log +
    // hai nút hành động nằm NGOÀI khung, ghim cố định bên dưới (xem
    // cuối hàm) nên luôn bấm được ở cả hai tab. NWG tự hiện trang
    // được chọn / ẩn trang còn lại mỗi khi người dùng đổi tab, và tự
    // khớp kích thước hai trang khi khung đổi kích thước — handle
    // của mọi control con giữ nguyên hiệu lực khi trang bị ẩn, nên
    // các luồng cập nhật lập trình (refresh, render log...) không
    // hề bị ảnh hưởng.
    // v0.7.12: khung có BỐN trang — thêm "Hướng dẫn" và "Tài liệu",
    // mỗi trang là một ô text chỉ-đọc phủ toàn trang (xem ngay dưới
    // phần đo trang). Cơ chế hiện/ẩn + khớp kích thước của NWG áp
    // cho mọi trang như nhau (EnumChildWindows trên WM_SIZE của
    // khung), nên hai trang mới không cần xử lý riêng nào khác
    // ngoài đăng ký vào bảng bố cục (sân GROUP_TAB3 / GROUP_TAB4).
    // v0.7.27: thêm trang THỨ NĂM "Thông báo" (sân GROUP_TAB5) —
    // cùng cơ chế, không xử lý riêng nào khác.
    let mut tabs = Default::default();
    nwg::TabsContainer::builder()
        .font(Some(font_normal))
        .parent(&window)
        .build(&mut tabs)
        .expect("tabs");
    tabs.set_position(16, 60);
    tabs.set_size(988, 523);
    let mut tab_src = Default::default();
    nwg::Tab::builder().text("Nguồn cài đặt").parent(&tabs).build(&mut tab_src).expect("tab");
    let mut tab_opt = Default::default();
    nwg::Tab::builder().text("Tuỳ chọn cài đặt").parent(&tabs).build(&mut tab_opt).expect("tab");
    // v0.7.12: thêm hai trang tài liệu chỉ-đọc. Thứ tự tạo Tab là
    // thứ tự hiển thị trên khung: Nguồn cài đặt / Tuỳ chọn cài đặt
    // / Hướng dẫn / Tài liệu.
    let mut tab_guide = Default::default();
    nwg::Tab::builder().text("Hướng dẫn").parent(&tabs).build(&mut tab_guide).expect("tab");
    let mut tab_docs = Default::default();
    nwg::Tab::builder().text("Tài liệu").parent(&tabs).build(&mut tab_docs).expect("tab");
    // v0.7.27: trang "Thông báo" — trang biểu mẫu thứ ba, nối sau
    // "Tài liệu" đúng thứ tự tạo.
    let mut tab_notify = Default::default();
    nwg::Tab::builder().text("Thông báo").parent(&tabs).build(&mut tab_notify).expect("tab");
    // Ép NWG khớp lại hai trang theo đúng kích thước khung thiết kế
    // (qua hàm xử lý WM_SIZE nội bộ của TabsContainer — SetWindowPos
    // gửi WM_SIZE đồng bộ) rồi đo client của trang: đó là mốc thiết
    // kế cho hai sân tab trong bảng bố cục.
    tabs.set_size(989, 523);
    tabs.set_size(988, 523);
    let (page_w, page_h) = {
        let mut rc: RECT = unsafe { std::mem::zeroed() };
        unsafe {
            GetClientRect(tab_src.handle.hwnd().unwrap(), &mut rc);
        }
        (rc.right, rc.bottom)
    };

    // --- Tab 3 + Tab 4 (v0.7.12): mỗi trang MỘT ô text chỉ-đọc
    // phủ gần trọn trang (chừa lề 8 như nội dung hai tab đầu, đáy
    // trang ở cùng mốc ~472 như nội dung tab 1). TextBox của NWG
    // mặc định đã nhiều dòng + có thanh cuộn dọc (WS_VSCROLL trong
    // flags mặc định, ES_MULTILINE trong forced flags) — chỉ cần
    // thêm ES_READONLY như ô log. Nội dung tĩnh lấy từ hai hằng
    // HUONG_DAN_TEXT / TAI_LIEU_TEXT, đổi \n -> \r\n vì ô EDIT
    // nhiều dòng của Windows cần CRLF mới ngắt dòng đúng. Giới hạn
    // ký tự nới lên 1_000_000 cho chắc (mặc định của EDIT thấp hơn
    // nhiều so với nhu cầu nếu text tài liệu dài ra sau này).
    let guide_text = HUONG_DAN_TEXT.replace('\n', "\r\n");
    let mut txt_guide = Default::default();
    nwg::TextBox::builder()
        .font(Some(font_normal))
        .text(&guide_text)
        .limit(1_000_000)
        .parent(&tab_guide)
        .build(&mut txt_guide)
        .expect("textbox");
    txt_guide.set_position(8, 6);
    txt_guide.set_size(961, 466);
    set_edit_style(txt_guide.handle.hwnd(), ES_READONLY);

    let docs_text = TAI_LIEU_TEXT.replace('\n', "\r\n");
    let mut txt_docs = Default::default();
    nwg::TextBox::builder()
        .font(Some(font_normal))
        .text(&docs_text)
        .limit(1_000_000)
        .parent(&tab_docs)
        .build(&mut txt_docs)
        .expect("textbox");
    txt_docs.set_position(8, 6);
    txt_docs.set_size(961, 466);
    set_edit_style(txt_docs.handle.hwnd(), ES_READONLY);

    // --- Tab 5 "Thông báo" (v0.7.27): cấu hình thông báo trạng
    // thái cài đặt qua hai kênh của chính kỹ thuật viên — bot
    // Telegram của họ và relay Zalo Golden Farm trên server của
    // đơn vị. Giá trị khởi tạo từ settings được điền SAU khi App
    // dựng xong (settings nạp ở cuối build_ui, như nút Core).
    // Token/secret dùng ô nhập che ký tự (ES_PASSWORD) như ô mật
    // khẩu ở tab 2; chúng chỉ lưu trong settings.json cạnh
    // wizard, không vào log, không vào stage.json.
    let _s5 = add_label!(&tab_notify, "5 — THÔNG BÁO TRẠNG THÁI CÀI ĐẶT", font_section, (8, 6), (560, 24));
    let mut chk_notify = Default::default();
    nwg::CheckBox::builder()
        .text("Bật thông báo trạng thái cài đặt (bắt đầu / cài xong / lỗi, kèm tên máy)")
        .font(Some(font_normal))
        .parent(&tab_notify)
        .build(&mut chk_notify)
        .expect("checkbox");
    chk_notify.set_position(8, 34);
    chk_notify.set_size(700, 24);

    let _tg_sec = add_label!(&tab_notify, "KÊNH TELEGRAM — BOT CỦA CHÍNH BẠN (TẠO QUA @BOTFATHER):", font_section, (8, 66), (760, 24));
    let _l_tg_token = add_label!(&tab_notify, "Bot Token:", font_normal, (8, 98), (110, 22));
    let mut inp_tg_token = Default::default();
    nwg::TextInput::builder().font(Some(font_normal)).parent(&tab_notify).build(&mut inp_tg_token).expect("input");
    inp_tg_token.set_position(124, 96);
    inp_tg_token.set_size(560, 24);
    set_edit_style(inp_tg_token.handle.hwnd(), ES_PASSWORD);
    let _l_tg_chat = add_label!(&tab_notify, "Chat ID:", font_normal, (8, 128), (110, 22));
    let mut inp_tg_chat = Default::default();
    nwg::TextInput::builder().font(Some(font_normal)).parent(&tab_notify).build(&mut inp_tg_chat).expect("input");
    inp_tg_chat.set_position(124, 126);
    inp_tg_chat.set_size(200, 24);
    let mut btn_tg_fetch = Default::default();
    nwg::Button::builder().text("Lấy Chat ID").font(Some(font_button)).parent(&tab_notify).build(&mut btn_tg_fetch).expect("btn");
    btn_tg_fetch.set_position(336, 122);
    btn_tg_fetch.set_size(130, 32);
    let mut btn_tg_test = Default::default();
    nwg::Button::builder().text("Gửi thử Telegram").font(Some(font_button)).parent(&tab_notify).build(&mut btn_tg_test).expect("btn");
    btn_tg_test.set_position(478, 122);
    btn_tg_test.set_size(170, 32);
    let _tg_hint = add_label!(
        &tab_notify,
        "Lấy Chat ID: mở Telegram, nhắn 1 tin bất kỳ cho bot của bạn, quay lại bấm \"Lấy Chat ID\" — wizard tự điền Chat ID và hiện tên người gửi ở dòng trạng thái dưới cùng.",
        font_small, (8, 162), (961, 34)
    );

    let _z_sec = add_label!(&tab_notify, "KÊNH ZALO — QUA RELAY GOLDEN FARM TRÊN SERVER CỦA BẠN:", font_section, (8, 206), (760, 24));
    let _l_z_url = add_label!(&tab_notify, "Relay URL:", font_normal, (8, 238), (110, 22));
    let mut inp_zalo_url = Default::default();
    nwg::TextInput::builder().font(Some(font_normal)).parent(&tab_notify).build(&mut inp_zalo_url).expect("input");
    inp_zalo_url.set_position(124, 236);
    inp_zalo_url.set_size(560, 24);
    let _l_z_secret = add_label!(&tab_notify, "Relay Secret:", font_normal, (8, 268), (110, 22));
    let mut inp_zalo_secret = Default::default();
    nwg::TextInput::builder().font(Some(font_normal)).parent(&tab_notify).build(&mut inp_zalo_secret).expect("input");
    inp_zalo_secret.set_position(124, 266);
    inp_zalo_secret.set_size(560, 24);
    set_edit_style(inp_zalo_secret.handle.hwnd(), ES_PASSWORD);
    let mut btn_zalo_test = Default::default();
    nwg::Button::builder().text("Gửi thử Zalo").font(Some(font_button)).parent(&tab_notify).build(&mut btn_zalo_test).expect("btn");
    btn_zalo_test.set_position(124, 298);
    btn_zalo_test.set_size(170, 32);
    let _z_hint = add_label!(
        &tab_notify,
        "Relay nhận trạng thái và chuyển tiếp vào Zalo của bạn qua bot Zalo Golden Farm. Tin BẮT ĐẦU gửi qua cả hai kênh; tin CÀI XONG / LỖI trong WinPE chỉ đi qua relay này — máy đang cài phải với tới được địa chỉ relay (cùng LAN / Tailscale).",
        font_small, (8, 338), (961, 34)
    );

    let mut btn_notify_save = Default::default();
    nwg::Button::builder().text("Lưu cấu hình thông báo").font(Some(font_button)).parent(&tab_notify).build(&mut btn_notify_save).expect("btn");
    btn_notify_save.set_position(8, 388);
    btn_notify_save.set_size(230, 36);
    let lbl_notify_status = add_label!(&tab_notify, "", font_normal, (250, 394), (719, 24));
    let _sec_note = add_label!(
        &tab_notify,
        "Token/secret chỉ lưu trong settings.json cạnh wizard và không ghi vào log. Nội dung tin chỉ gồm trạng thái + tên máy — không gửi bất kỳ thông tin đăng nhập nào.",
        font_small, (8, 432), (961, 30)
    );

    // --- Tab 1, mục 1: tiền đề ---
    let _s1 = add_label!(&tab_src, "1 — KIỂM TRA HỆ THỐNG", font_section, (8, 6), (420, 24));
    let l_os = add_label!(&tab_src, "Ổ Cài Windows (OS): đang quét...", font_normal, (8, 34), (961, 20));
    let l_boot = add_label!(&tab_src, "Boot Mode: ...", font_normal, (8, 56), (961, 20));
    let _l_data = add_label!(&tab_src, "Ổ Chứa Dữ Liệu (Data):", font_normal, (8, 80), (262, 20));
    let l_data_info = add_label!(&tab_src, "", font_normal, (652, 80), (317, 20));
    let l_power = add_label!(&tab_src, "Nguồn điện: ...", font_normal, (8, 102), (961, 20));
    let l_verdict = add_label!(&tab_src, "Kết luận: đang quét hệ thống...", font_normal, (8, 124), (961, 20));

    let mut cmb_data = Default::default();
    nwg::ComboBox::builder().parent(&tab_src).build(&mut cmb_data).expect("combo");
    cmb_data.set_position(274, 77);
    cmb_data.set_size(366, 24);

    let mut btn_rescan = Default::default();
    nwg::Button::builder().text("Quét lại").font(Some(font_button)).parent(&tab_src).build(&mut btn_rescan).expect("btn");
    btn_rescan.set_position(845, 2);
    btn_rescan.set_size(124, 32);

    // --- Tab 1, mục 2: nguồn ISO ---
    let _s2 = add_label!(&tab_src, "2 — NGUỒN ISO", font_section, (8, 150), (420, 24));
    let mut lst_iso = Default::default();
    nwg::ListBox::builder().parent(&tab_src).build(&mut lst_iso).expect("list");
    lst_iso.set_position(8, 176);
    lst_iso.set_size(632, 84);

    let mut btn_browse = Default::default();
    nwg::Button::builder().text("Chọn ISO...").font(Some(font_button)).parent(&tab_src).build(&mut btn_browse).expect("btn");
    btn_browse.set_position(652, 176);
    btn_browse.set_size(150, 32);
    let _iso_hint = add_label!(
        &tab_src,
        "Chọn một ISO trong danh sách (hoặc bấm Chọn ISO...) — wizard sẽ mount và đọc danh sách phiên bản.",
        font_small, (652, 210), (317, 50)
    );

    let _l_cat = add_label!(&tab_src, "Kho online (catalogueUrl trong settings.json cạnh file wizard):", font_normal, (8, 264), (632, 20));
    let mut btn_fetch = Default::default();
    nwg::Button::builder().text("Tải danh mục").font(Some(font_button)).parent(&tab_src).build(&mut btn_fetch).expect("btn");
    btn_fetch.set_position(652, 260);
    btn_fetch.set_size(150, 32);

    let mut lst_cat = Default::default();
    nwg::ListBox::builder().parent(&tab_src).build(&mut lst_cat).expect("list");
    lst_cat.set_position(8, 288);
    lst_cat.set_size(632, 56);

    let mut btn_download = Default::default();
    nwg::Button::builder().text("Tải ISO đã chọn").font(Some(font_button)).parent(&tab_src).build(&mut btn_download).expect("btn");
    btn_download.set_position(652, 288);
    btn_download.set_size(150, 32);

    let mut bar_dl = Default::default();
    nwg::ProgressBar::builder().parent(&tab_src).build(&mut bar_dl).expect("bar");
    bar_dl.set_position(8, 350);
    bar_dl.set_size(632, 14);
    bar_dl.set_range(0..101);
    bar_dl.set_pos(0);
    let lbl_dl = add_label!(&tab_src, "", font_small, (652, 348), (317, 20));

    // --- Tab 1, mục 3: phiên bản ---
    let _s3 = add_label!(&tab_src, "3 — PHIÊN BẢN TRONG ISO", font_section, (8, 370), (460, 24));
    let mut lst_ed = Default::default();
    nwg::ListBox::builder().parent(&tab_src).build(&mut lst_ed).expect("list");
    lst_ed.set_position(8, 396);
    lst_ed.set_size(632, 68);
    let lbl_ed_info = add_label!(&tab_src, "Chưa chọn ISO.", font_small, (652, 396), (317, 68));

    // --- Tab 2, mục 4: tuỳ chọn ---
    let _s4 = add_label!(&tab_opt, "4 — TUỲ CHỌN CÀI ĐẶT", font_section, (8, 6), (460, 24));
    let mut btn_mode_inplace = Default::default();
    nwg::Button::builder().text("● Cài đè (giữ ứng dụng + dữ liệu)").font(Some(font_button)).parent(&tab_opt).build(&mut btn_mode_inplace).expect("btn");
    btn_mode_inplace.set_position(8, 34);
    btn_mode_inplace.set_size(320, 32);
    let mut btn_mode_clean = Default::default();
    nwg::Button::builder().text("○ Cài mới từ xa (qua WinPE)").font(Some(font_button)).parent(&tab_opt).build(&mut btn_mode_clean).expect("btn");
    btn_mode_clean.set_position(340, 34);
    btn_mode_clean.set_size(320, 32);

    // --- Chế độ phân vùng khi cài mới (§6b): giữ nguyên (mặc định),
    // xoá toàn bộ ổ đĩa đích + chia lại từ đầu (wipeDisk), hoặc chỉ
    // xoá các phân vùng hệ thống trên đĩa đích và giữ nguyên phân
    // vùng data (rebuild, v0.7.13). Khối này + khối chọn đĩa bên
    // dưới chỉ hiện khi đang ở chế độ Cài mới.
    let mut btn_part_preserve = Default::default();
    nwg::Button::builder().text("● Giữ nguyên phân vùng (mặc định)").font(Some(font_button)).parent(&tab_opt).build(&mut btn_part_preserve).expect("btn");
    btn_part_preserve.set_position(8, 72);
    btn_part_preserve.set_size(312, 32);
    let mut btn_part_wipe = Default::default();
    nwg::Button::builder().text("○ Xoá toàn bộ ổ đĩa, chia lại").font(Some(font_button)).parent(&tab_opt).build(&mut btn_part_wipe).expect("btn");
    btn_part_wipe.set_position(328, 72);
    btn_part_wipe.set_size(312, 32);
    let mut btn_part_rebuild = Default::default();
    nwg::Button::builder().text("○ Cài lại phân vùng hệ thống (giữ data)").font(Some(font_button)).parent(&tab_opt).build(&mut btn_part_rebuild).expect("btn");
    btn_part_rebuild.set_position(648, 72);
    btn_part_rebuild.set_size(312, 32);

    let lbl_disk_cap = add_label!(&tab_opt, "Ổ đĩa đích (toàn bộ đĩa vật lý):", font_normal, (8, 112), (262, 22));
    let mut cmb_disk = Default::default();
    nwg::ComboBox::builder().parent(&tab_opt).build(&mut cmb_disk).expect("combo");
    cmb_disk.set_position(274, 110);
    cmb_disk.set_size(366, 24);
    // Danh sách xổ xuống rộng hơn hẳn ô chọn (v0.7.17): dòng chọn
    // chứa số đĩa — model — dung lượng — serial — kiểu đĩa nên
    // với bề rộng ô mặc định serial bị cắt mất (thực địa: kỹ
    // thuật viên không thấy serial đĩa đích để gõ xác nhận).
    if let Some(hwnd) = cmb_disk.handle.hwnd() {
        unsafe {
            SendMessageW(hwnd, winapi::um::winuser::CB_SETDROPPEDWIDTH, 780, 0);
        }
    }
    // Gợi ý tĩnh trong chế độ wipe: xoá theo ĐĨA VẬT LÝ, không theo
    // ký tự ổ — tránh nhầm lẫn kiểu "chọn ổ C đúng không" khi ổ C và
    // ổ dữ liệu nằm chung một đĩa (khi đó đĩa đó bị cấm, phải giữ
    // nguyên phân vùng hoặc đi đường WinPE/USB).
    let lbl_disk_hint = add_label!(
        &tab_opt,
        "Chế độ này xoá theo Ổ ĐĨA VẬT LÝ, không theo ký tự ổ.\nNếu ổ C và ổ dữ liệu nằm chung một đĩa vật lý, hãy dùng chế độ 'Giữ nguyên phân vùng dữ liệu'\nhoặc boot WinPE từ USB với nguồn cài trên USB.",
        font_small, (8, 138), (961, 48)
    );
    // Danh sách phân vùng của đĩa đích — ListView dạng bảng (4
    // cột). Ở chế độ rebuild, helper lv_set_checkboxes bật checkbox
    // theo hàng để chọn phân vùng hệ thống sẽ xoá; hai chế độ còn
    // lại tắt checkbox (chỉ hiển thị).
    let mut lst_disk_parts = Default::default();
    nwg::ListView::builder()
        .parent(&tab_opt)
        .list_style(nwg::ListViewStyle::Detailed)
        .flags(nwg::ListViewFlags::VISIBLE | nwg::ListViewFlags::TAB_STOP | nwg::ListViewFlags::ALWAYS_SHOW_SELECTION)
        .ex_flags(nwg::ListViewExFlags::FULL_ROW_SELECT)
        .build(&mut lst_disk_parts)
        .expect("list");
    lst_disk_parts.set_position(8, 190);
    lst_disk_parts.set_size(632, 104);
    lst_disk_parts.set_headers_enabled(true);
    for (i, (name, w)) in [("Phân vùng", 168), ("Loại", 128), ("Dung lượng", 88), ("Ghi chú", 240)].iter().enumerate() {
        lst_disk_parts.insert_column(nwg::InsertListViewColumn {
            index: Some(i as i32),
            text: Some(name.to_string()),
            width: Some(*w),
            ..Default::default()
        });
    }
    let lbl_disk_warn = add_label!(&tab_opt, "", font_small, (652, 190), (317, 104));
    // Dòng định danh đĩa đích ĐẦY ĐỦ ngay trên ô nhập serial
    // (v0.7.17): số đĩa — model — dung lượng — serial trọn vẹn
    // trên một dòng rộng hết trang, không bị cắt như trong ô chọn
    // đĩa; nội dung do `update_serial_ui` đổ theo đĩa đang chọn.
    let lbl_disk_id = add_label!(&tab_opt, "", font_normal, (8, 300), (961, 22));
    let lbl_serial_cap = add_label!(&tab_opt, "Nhập serial của ổ đĩa đích để xác nhận XOÁ TOÀN BỘ:", font_normal, (8, 328), (434, 22));
    let mut inp_disk_serial = Default::default();
    nwg::TextInput::builder().font(Some(font_normal)).parent(&tab_opt).build(&mut inp_disk_serial).expect("input");
    inp_disk_serial.set_position(446, 326);
    inp_disk_serial.set_size(214, 26);
    // LƯU Ý: ô nhập serial KHÔNG đặt ES_PASSWORD — serial phải
    // hiện rõ từng ký tự đang gõ để đối chiếu với dòng định danh
    // bên trên (khác hẳn hai ô mật khẩu ở dưới, có che).
    // Nhãn phản hồi trực tiếp ngay cạnh ô nhập (v0.7.17): gõ tới
    // đâu biết khớp hay chưa tới đó — `update_serial_feedback`.
    let lbl_serial_fb = add_label!(&tab_opt, "", font_small, (668, 328), (301, 22));

    let _lc = add_label!(&tab_opt, "Tên máy tính:", font_normal, (8, 358), (130, 22));
    let mut inp_computer = Default::default();
    nwg::TextInput::builder().font(Some(font_normal)).parent(&tab_opt).build(&mut inp_computer).expect("input");
    inp_computer.set_position(142, 356);
    inp_computer.set_size(220, 24);
    let _lu = add_label!(&tab_opt, "Tài khoản admin:", font_normal, (378, 358), (140, 22));
    let mut inp_user = Default::default();
    nwg::TextInput::builder().font(Some(font_normal)).parent(&tab_opt).build(&mut inp_user).expect("input");
    inp_user.set_position(522, 356);
    inp_user.set_size(190, 24);

    let _lp = add_label!(&tab_opt, "Mật khẩu (để trống = không đặt mật khẩu):", font_normal, (8, 386), (330, 22));
    let mut inp_pw1 = Default::default();
    nwg::TextInput::builder().font(Some(font_normal)).parent(&tab_opt).build(&mut inp_pw1).expect("input");
    inp_pw1.set_position(342, 384);
    inp_pw1.set_size(170, 24);
    set_edit_style(inp_pw1.handle.hwnd(), ES_PASSWORD);
    let _lp2 = add_label!(&tab_opt, "Nhập lại mật khẩu:", font_normal, (528, 386), (140, 22));
    let mut inp_pw2 = Default::default();
    nwg::TextInput::builder().font(Some(font_normal)).parent(&tab_opt).build(&mut inp_pw2).expect("input");
    inp_pw2.set_position(672, 384);
    inp_pw2.set_size(150, 24);
    set_edit_style(inp_pw2.handle.hwnd(), ES_PASSWORD);
    let _pw_note = add_label!(
        &tab_opt,
        "(Chỉ cài mới. Không ghi vào log.)",
        font_small, (830, 386), (139, 32)
    );

    let mut btn_tweaks = Default::default();
    nwg::Button::builder().text("Tuỳ chỉnh Windows sau cài: TẮT").font(Some(font_button)).parent(&tab_opt).build(&mut btn_tweaks).expect("btn");
    btn_tweaks.set_position(8, 414);
    btn_tweaks.set_size(320, 32);
    // Nút bật/tắt hồ sơ CORE cho ảnh rút gọn (v0.7.21) — cùng mẫu
    // nút tweaks: trạng thái nằm ở Model, nhãn nút phản ánh BẬT/TẮT.
    let mut btn_core = Default::default();
    nwg::Button::builder().text("Ảnh rút gọn (Tiny11 Core): TẮT").font(Some(font_button)).parent(&tab_opt).build(&mut btn_core).expect("btn");
    btn_core.set_position(8, 452);
    btn_core.set_size(320, 32);
    let lbl_stage_info = add_label!(&tab_opt, "", font_small, (340, 418), (629, 22));

    // --- Tiến trình + log + nút hành động (sân chính, GHIM dưới
    // khung tab — luôn nhìn thấy và bấm được ở cả hai tab) ---
    let mut bar_exec = Default::default();
    nwg::ProgressBar::builder().parent(&window).build(&mut bar_exec).expect("bar");
    bar_exec.set_position(16, 593);
    bar_exec.set_size(640, 16);
    bar_exec.set_range(0..101);
    bar_exec.set_pos(0);
    let lbl_exec = add_label!(&window, "", font_small, (668, 591), (336, 22));

    let mut txt_log = Default::default();
    nwg::TextBox::builder().font(Some(font_small)).parent(&window).build(&mut txt_log).expect("log");
    txt_log.set_position(16, 617);
    txt_log.set_size(640, 80);
    set_edit_style(txt_log.handle.hwnd(), ES_READONLY);

    let mut btn_start = Default::default();
    nwg::Button::builder().text("TIẾN HÀNH CÀI ĐẶT").font(Some(font_button)).parent(&window).build(&mut btn_start).expect("btn");
    btn_start.set_position(668, 617);
    btn_start.set_size(336, 44);
    btn_start.set_enabled(false);

    let mut btn_close = Default::default();
    nwg::Button::builder().text("Đóng").font(Some(font_button)).parent(&window).build(&mut btn_close).expect("btn");
    btn_close.set_position(668, 667);
    btn_close.set_size(336, 32);

    let mut notice = Default::default();
    nwg::Notice::builder().parent(&window).build(&mut notice).expect("notice");

    // --- Bảng bố cục thích ứng (v0.7.9, chia sân ở v0.7.10) ---
    // Ghi lại rect THIẾT KẾ của mọi control (đọc thẳng từ control vừa
    // dựng, không chép tay toạ độ) kèm vai trò khi cửa sổ đổi kích
    // thước: cột phải bám lề phải (ShiftRight), danh sách/nhãn dài nở
    // theo bề ngang (Grow), các phần tử co giãn theo chiều cao
    // (flex), và SÂN toạ độ của control (chính / tab 1 / tab 2).
    // `apply_layout` tính lại vị trí thuần tuý từ bảng này, riêng
    // cho từng sân theo kích thước client của sân đó.
    let mut layout: Vec<LayoutItem> = Vec::new();
    macro_rules! lay {
        ($c:expr, $g:expr, $xm:expr, $wm:expr, $flex:expr) => {{
            let (px, py) = $c.position();
            let (sw, sh) = $c.size();
            layout.push(LayoutItem {
                hwnd: $c.handle.hwnd().unwrap_or(std::ptr::null_mut()),
                x: px,
                y: py,
                w: sw as i32,
                h: sh as i32,
                x_mode: $xm,
                w_mode: $wm,
                flex: $flex,
                group: $g,
            });
        }};
    }
    // Sân chính: header + khung tab (co giãn FLEX_TABS) + khối ghim.
    lay!(_t1, GROUP_MAIN, XMode::Fixed, WMode::Fixed, None);
    lay!(_t2, GROUP_MAIN, XMode::Fixed, WMode::Fixed, None);
    lay!(tabs, GROUP_MAIN, XMode::Fixed, WMode::Grow, Some(FLEX_TABS));
    lay!(bar_exec, GROUP_MAIN, XMode::Fixed, WMode::Grow, None);
    lay!(lbl_exec, GROUP_MAIN, XMode::ShiftRight, WMode::Fixed, None);
    lay!(txt_log, GROUP_MAIN, XMode::Fixed, WMode::Grow, Some(FLEX_LOG));
    lay!(btn_start, GROUP_MAIN, XMode::ShiftRight, WMode::Fixed, None);
    lay!(btn_close, GROUP_MAIN, XMode::ShiftRight, WMode::Fixed, None);
    // Sân tab 1 — "Nguồn cài đặt".
    lay!(_s1, GROUP_TAB1, XMode::Fixed, WMode::Fixed, None);
    lay!(l_os, GROUP_TAB1, XMode::Fixed, WMode::Grow, None);
    lay!(l_boot, GROUP_TAB1, XMode::Fixed, WMode::Grow, None);
    lay!(_l_data, GROUP_TAB1, XMode::Fixed, WMode::Fixed, None);
    lay!(l_data_info, GROUP_TAB1, XMode::ShiftRight, WMode::Fixed, None);
    lay!(l_power, GROUP_TAB1, XMode::Fixed, WMode::Grow, None);
    lay!(l_verdict, GROUP_TAB1, XMode::Fixed, WMode::Grow, None);
    lay!(cmb_data, GROUP_TAB1, XMode::Fixed, WMode::Grow, None);
    lay!(btn_rescan, GROUP_TAB1, XMode::ShiftRight, WMode::Fixed, None);
    lay!(_s2, GROUP_TAB1, XMode::Fixed, WMode::Fixed, None);
    lay!(lst_iso, GROUP_TAB1, XMode::Fixed, WMode::Grow, Some(FLEX_ISO));
    lay!(btn_browse, GROUP_TAB1, XMode::ShiftRight, WMode::Fixed, None);
    lay!(_iso_hint, GROUP_TAB1, XMode::ShiftRight, WMode::Fixed, None);
    lay!(_l_cat, GROUP_TAB1, XMode::Fixed, WMode::Fixed, None);
    lay!(btn_fetch, GROUP_TAB1, XMode::ShiftRight, WMode::Fixed, None);
    lay!(lst_cat, GROUP_TAB1, XMode::Fixed, WMode::Grow, Some(FLEX_CAT));
    lay!(btn_download, GROUP_TAB1, XMode::ShiftRight, WMode::Fixed, None);
    lay!(bar_dl, GROUP_TAB1, XMode::Fixed, WMode::Grow, None);
    lay!(lbl_dl, GROUP_TAB1, XMode::ShiftRight, WMode::Fixed, None);
    lay!(_s3, GROUP_TAB1, XMode::Fixed, WMode::Fixed, None);
    lay!(lst_ed, GROUP_TAB1, XMode::Fixed, WMode::Grow, Some(FLEX_ED));
    lay!(lbl_ed_info, GROUP_TAB1, XMode::ShiftRight, WMode::Fixed, None);
    // Sân tab 2 — "Tuỳ chọn cài đặt".
    lay!(_s4, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(btn_mode_inplace, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(btn_mode_clean, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(btn_part_preserve, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(btn_part_wipe, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(btn_part_rebuild, GROUP_TAB2, XMode::ShiftRight, WMode::Fixed, None);
    lay!(lbl_disk_cap, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(cmb_disk, GROUP_TAB2, XMode::Fixed, WMode::Grow, None);
    lay!(lbl_disk_hint, GROUP_TAB2, XMode::Fixed, WMode::Grow, None);
    lay!(lst_disk_parts, GROUP_TAB2, XMode::Fixed, WMode::Grow, Some(FLEX_PARTS));
    lay!(lbl_disk_warn, GROUP_TAB2, XMode::ShiftRight, WMode::Fixed, None);
    lay!(lbl_disk_id, GROUP_TAB2, XMode::Fixed, WMode::Grow, None);
    lay!(lbl_serial_cap, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(inp_disk_serial, GROUP_TAB2, XMode::Fixed, WMode::Grow, None);
    lay!(lbl_serial_fb, GROUP_TAB2, XMode::ShiftRight, WMode::Fixed, None);
    lay!(_lc, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(inp_computer, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(_lu, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(inp_user, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(_lp, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(inp_pw1, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(_lp2, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(inp_pw2, GROUP_TAB2, XMode::ShiftRight, WMode::Fixed, None);
    lay!(_pw_note, GROUP_TAB2, XMode::ShiftRight, WMode::Fixed, None);
    lay!(btn_tweaks, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(btn_core, GROUP_TAB2, XMode::Fixed, WMode::Fixed, None);
    lay!(lbl_stage_info, GROUP_TAB2, XMode::Fixed, WMode::Grow, None);
    // Sân tab 3 — "Hướng dẫn" (v0.7.12): ô text duy nhất của trang,
    // nở theo cả hai chiều; chỉ số flex FLEX_TABS ở đây nghĩa là
    // "phần tử co giãn duy nhất của sân này" (xem hằng GROUP_TAB3).
    lay!(txt_guide, GROUP_TAB3, XMode::Fixed, WMode::Grow, Some(FLEX_TABS));
    // Sân tab 4 — "Tài liệu" (v0.7.12): cùng cấu trúc với tab 3.
    lay!(txt_docs, GROUP_TAB4, XMode::Fixed, WMode::Grow, Some(FLEX_TABS));
    // Sân tab 5 — "Thông báo" (v0.7.27): biểu mẫu cấu hình — ô
    // nhập nở theo bề ngang, nhãn/nút giữ nguyên; không phần tử
    // co giãn theo chiều cao.
    lay!(_s5, GROUP_TAB5, XMode::Fixed, WMode::Fixed, None);
    lay!(chk_notify, GROUP_TAB5, XMode::Fixed, WMode::Grow, None);
    lay!(_tg_sec, GROUP_TAB5, XMode::Fixed, WMode::Fixed, None);
    lay!(_l_tg_token, GROUP_TAB5, XMode::Fixed, WMode::Fixed, None);
    lay!(inp_tg_token, GROUP_TAB5, XMode::Fixed, WMode::Grow, None);
    lay!(_l_tg_chat, GROUP_TAB5, XMode::Fixed, WMode::Fixed, None);
    lay!(inp_tg_chat, GROUP_TAB5, XMode::Fixed, WMode::Fixed, None);
    lay!(btn_tg_fetch, GROUP_TAB5, XMode::Fixed, WMode::Fixed, None);
    lay!(btn_tg_test, GROUP_TAB5, XMode::Fixed, WMode::Fixed, None);
    lay!(_tg_hint, GROUP_TAB5, XMode::Fixed, WMode::Grow, None);
    lay!(_z_sec, GROUP_TAB5, XMode::Fixed, WMode::Fixed, None);
    lay!(_l_z_url, GROUP_TAB5, XMode::Fixed, WMode::Fixed, None);
    lay!(inp_zalo_url, GROUP_TAB5, XMode::Fixed, WMode::Grow, None);
    lay!(_l_z_secret, GROUP_TAB5, XMode::Fixed, WMode::Fixed, None);
    lay!(inp_zalo_secret, GROUP_TAB5, XMode::Fixed, WMode::Grow, None);
    lay!(btn_zalo_test, GROUP_TAB5, XMode::Fixed, WMode::Fixed, None);
    lay!(_z_hint, GROUP_TAB5, XMode::Fixed, WMode::Grow, None);
    lay!(btn_notify_save, GROUP_TAB5, XMode::Fixed, WMode::Fixed, None);
    lay!(lbl_notify_status, GROUP_TAB5, XMode::Fixed, WMode::Grow, None);
    lay!(_sec_note, GROUP_TAB5, XMode::Fixed, WMode::Grow, None);

    // Căn cửa sổ vào vùng làm việc + thêm khung kéo giãn được. Khi
    // màn hình đủ lớn, kích thước client giữ NGUYÊN mốc thiết kế ở
    // trên nên bố cục không lệch một pixel so với thiết kế v0.7.10.
    if let Some(hwnd) = window.handle.hwnd() {
        adapt_window_frame(hwnd, (client_w, client_h));
    }

    // --- Nạp settings cạnh file exe của wizard ---
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let (settings, settings_err) = Settings::load(&exe_dir);
    // v0.7.24: nút Core khởi tạo từ settings (defaultCoreProfile)
    // và tự lưu lại mỗi lần bật/tắt — xem persist_core_default.
    let core_default = settings.default_core_profile;
    // v0.7.27: giá trị tab "Thông báo" khởi tạo từ settings —
    // điền vào các ô sau khi App dựng xong (như nhãn nút Core).
    let notify_init = (
        settings.notify_enabled,
        settings.telegram_bot_token.clone(),
        settings.telegram_chat_id.clone(),
        settings.zalo_relay_url.clone(),
        settings.zalo_relay_secret.clone(),
    );
    let shared: SharedRef = Arc::new(Mutex::new(Shared::default()));
    if let Some(e) = &settings_err {
        shared
            .lock()
            .unwrap()
            .log_lines
            .push_back(format!("CẢNH BÁO: {}", e));
    }
    let app = Rc::new(App {
        notice,
        tabs, tab_src, tab_opt, tab_guide, tab_docs, tab_notify,
        txt_guide, txt_docs,
        l_os, l_boot, l_data_info, l_power, l_verdict,
        cmb_data, lst_iso, lst_cat, lst_ed,
        lbl_dl, lbl_ed_info, lbl_stage_info, lbl_exec,
        bar_dl, bar_exec,
        inp_computer, inp_user, inp_pw1, inp_pw2,
        txt_log,
        btn_rescan, btn_browse, btn_fetch, btn_download,
        btn_mode_inplace, btn_mode_clean, btn_tweaks, btn_core, btn_start, btn_close,
        btn_part_preserve, btn_part_wipe, btn_part_rebuild,
        lbl_disk_cap, cmb_disk, lbl_disk_hint, lst_disk_parts, lbl_disk_warn, lbl_serial_cap, inp_disk_serial,
        lbl_disk_id, lbl_serial_fb,
        chk_notify, inp_tg_token, inp_tg_chat, inp_zalo_url, inp_zalo_secret,
        btn_tg_fetch, btn_tg_test, btn_zalo_test, btn_notify_save, lbl_notify_status,
        model: RefCell::new(Model {
            settings,
            exe_dir,
            preflight: None,
            disk_diag: Vec::new(),
            data_letter: None,
            logger: None,
            local_isos: Vec::new(),
            catalogue: Vec::new(),
            loading_iso: None,
            mounted_iso: None,
            mounted_letter: None,
            image_name: None,
            editions: Vec::new(),
            mode_clean: false,
            part_mode: PartMode::Preserve,
            rebuild_checks: Vec::new(),
            rebuild_checks_disk: None,
            parts_disk: None,
            apply_tweaks: false,
            core_profile: core_default,
            notify_busy: false,
            busy_ed: false,
            running: false,
            last_clean: false,
            log_rendered: 0,
            saved_window_rect: None,
            layout,
            design_client: (client_w, client_h),
            design_page: (page_w, page_h),
        }),
        shared,
    });
    if core_default {
        app.btn_core.set_text("Ảnh rút gọn (Tiny11 Core): BẬT");
    }
    // v0.7.27: điền cấu hình thông báo đã nạp vào các ô của tab 5.
    app.chk_notify.set_check_state(if notify_init.0 {
        nwg::CheckBoxState::Checked
    } else {
        nwg::CheckBoxState::Unchecked
    });
    app.inp_tg_token.set_text(&notify_init.1);
    app.inp_tg_chat.set_text(&notify_init.2);
    app.inp_zalo_url.set_text(&notify_init.3);
    app.inp_zalo_secret.set_text(&notify_init.4);
    finish_build(window, app)
}

fn finish_build(window: nwg::Window, app: Rc<App>) -> (nwg::Window, Rc<App>) {
    let handler_app = app.clone();
    nwg::full_bind_event_handler(&window.handle, move |evt, _data, handle| {
        // Lưới an toàn ngoài cùng (v0.7.4): closure này được NWG gọi
        // từ wndproc (`extern "system"`) — bất kỳ panic nào thoát ra
        // đều unwind qua biên FFI và abort tiến trình im lặng. Bọc
        // MỌI sự kiện ở đây; các handler quan trọng (on_start,
        // refresh) có lớp catch_unwind riêng với khôi phục trạng
        // thái bên trong, lớp này chỉ là chốt cuối — hộp đen panic
        // vẫn ghi lại đầy đủ vị trí/nội dung của panic bị bắt.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle_event(&handler_app, evt, handle);
        }));
    });
    // Trạng thái ban đầu: chế độ cài đè (nút chọn chế độ thể hiện qua ký tự ●/○ trong nhãn)
    if app.model.borrow().settings.default_apply_tweaks {
        app.model.borrow_mut().apply_tweaks = true;
        app.btn_tweaks.set_text("Tuỳ chỉnh Windows sau cài: BẬT");
    }
    // Khối chọn chế độ phân vùng + đĩa đích chỉ hiện ở chế độ Cài mới
    update_part_ui(&app);
    // v0.7.9: chặn kích thước tối thiểu khi người dùng kéo giãn cửa
    // sổ, và chạy bố cục thích ứng lần đầu (cửa sổ đã được
    // `adapt_window_frame` đặt kích thước/vị trí trong build_ui;
    // WM_SIZE phát sinh lúc đó xảy ra TRƯỚC khi handler sự kiện được
    // gắn nên phải chạy tay một lần ở đây — hàm là luỹ đẳng, các lần
    // WM_SIZE sau tự cân lại).
    bind_min_window_size(&window);
    layout_window(&app);
    (window, app)
}

/// HWND của cửa sổ chính (lấy qua GetParent của một control con —
/// cùng cách `on_browse` đang dùng để đặt cửa sổ cha cho hộp chọn file).
fn main_window_hwnd(app: &Rc<App>) -> Option<HWND> {
    let child = app.btn_close.handle.hwnd()?;
    let parent = unsafe { GetParent(child) };
    if parent.is_null() {
        None
    } else {
        Some(parent)
    }
}

/// Dịch cửa sổ chính LÊN để đáy cửa sổ (thanh tiến độ + ô log + hai
/// nút hành động — toàn bộ phản hồi trực quan của quá trình cài đặt)
/// nằm trọn trong vùng làm việc của màn hình. Bối cảnh thực địa
/// v0.7.2: cửa sổ wizard cao 1102px trong khi cửa sổ session từ xa
/// của kỹ thuật viên thấp hơn — sau khi bấm Yes ở hộp xác nhận, mọi
/// cập nhật tiến độ đều xuất hiện ở vùng đáy cửa sổ ĐANG BỊ KHUẤT
/// khỏi màn hình, trông hệt như "bấm xong không có hành động gì".
/// Chỉ dịch khi đáy cửa sổ thật sự vượt khỏi vùng làm việc; vị trí
/// gốc được lưu vào model để khôi phục khi quá trình kết thúc.
fn shift_window_for_exec(app: &Rc<App>) {
    let hwnd = match main_window_hwnd(app) {
        Some(h) => h,
        None => return,
    };
    unsafe {
        let mut work: RECT = std::mem::zeroed();
        if SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut work as *mut RECT as *mut winapi::ctypes::c_void,
            0,
        ) == 0
        {
            return;
        }
        let mut wr: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut wr) == 0 {
            return;
        }
        let overflow = wr.bottom - work.bottom;
        if overflow <= 0 {
            return; // đáy cửa sổ đã nằm trong vùng nhìn thấy
        }
        {
            let mut m = app.model.borrow_mut();
            if m.saved_window_rect.is_none() {
                m.saved_window_rect = Some((wr.left, wr.top, wr.right - wr.left, wr.bottom - wr.top));
            }
        }
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            wr.left,
            wr.top - overflow,
            0,
            0,
            SWP_NOZORDER | SWP_NOACTIVATE | winapi::um::winuser::SWP_NOSIZE,
        );
    }
}

/// Đưa cửa sổ chính về đúng vị trí/kích thước trước khi bắt đầu cài
/// đặt (đối tác của `shift_window_for_exec`). Gọi khi quá trình kết
/// thúc — trước khi hiện hộp thoại kết quả — để giao diện trở lại
/// nguyên trạng cho lần thao tác tiếp theo.
fn restore_window_pos(app: &Rc<App>) {
    let saved = app.model.borrow_mut().saved_window_rect.take();
    let (x, y, w, h) = match saved {
        Some(r) => r,
        None => return,
    };
    if let Some(hwnd) = main_window_hwnd(app) {
        unsafe {
            SetWindowPos(hwnd, std::ptr::null_mut(), x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}

/// Chuẩn bị cửa sổ chính cho bố cục thích ứng (v0.7.9), gọi một lần
/// trong `build_ui` ngay sau khi mọi control đã dựng xong:
///
/// 1. Thêm WS_THICKFRAME | WS_MAXIMIZEBOX vào style — cửa sổ v0.7.8
///    dùng khung cố định (WS_CAPTION | WS_SYSMENU) nên không kéo
///    giãn được; đổi style TẠI CHỖ (không tạo lại cửa sổ) giữ
///    nguyên mọi control và handler.
/// 2. Đặt kích thước NGOÀI sao cho kích thước CLIENT bằng đúng mốc
///    thiết kế `design_client` (đo khi cửa sổ còn khung cũ) — nhờ
///    vậy trên màn hình đủ lớn bố cục trùng v0.7.8 từng pixel dù
///    khung mới dày hơn khung cũ. Nếu vùng làm việc nhỏ hơn, kích
///    thước ngoài bị chặn ở vùng làm việc và `apply_layout` co các
///    danh sách lại cho vừa.
/// 3. Căn giữa cửa sổ trong vùng làm việc của màn hình chính.
fn adapt_window_frame(hwnd: HWND, design_client: (i32, i32)) {
    unsafe {
        let mut work: RECT = std::mem::zeroed();
        if SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut work as *mut RECT as *mut winapi::ctypes::c_void,
            0,
        ) == 0
        {
            return;
        }
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(hwnd, GWL_STYLE, style | WS_THICKFRAME as isize | WS_MAXIMIZEBOX as isize);
        // Áp khung mới ngay (SWP_FRAMECHANGED) rồi đo phần khung mới
        // chiếm: hiệu số rect ngoài trừ rect client.
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOZORDER
                | SWP_NOACTIVATE
                | winapi::um::winuser::SWP_NOMOVE
                | winapi::um::winuser::SWP_NOSIZE
                | winapi::um::winuser::SWP_FRAMECHANGED,
        );
        let mut wr: RECT = std::mem::zeroed();
        let mut cr: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut wr) == 0 || GetClientRect(hwnd, &mut cr) == 0 {
            return;
        }
        let frame_w = (wr.right - wr.left) - cr.right;
        let frame_h = (wr.bottom - wr.top) - cr.bottom;
        let work_w = work.right - work.left;
        let work_h = work.bottom - work.top;
        let outer_w = (design_client.0 + frame_w).min(work_w);
        let outer_h = (design_client.1 + frame_h).min(work_h);
        let x = work.left + (work_w - outer_w) / 2;
        let y = work.top + (work_h - outer_h) / 2;
        SetWindowPos(hwnd, std::ptr::null_mut(), x, y, outer_w, outer_h, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// Chặn kích thước tối thiểu của cửa sổ qua WM_GETMINMAXINFO (hook
/// subclass thô của NWG): người dùng kéo giãn thủ công không thể
/// thu cửa sổ xuống dưới MIN_WINDOW_W x MIN_WINDOW_H, mức mà bố cục
/// thích ứng vẫn giữ mọi control không chồng chéo. Handler không
/// giữ trạng thái nào — hằng số là đủ.
fn bind_min_window_size(window: &nwg::Window) {
    let handler = nwg::bind_raw_event_handler(&window.handle, 0x10001, |_hwnd, msg, _w, l| {
        if msg == winapi::um::winuser::WM_GETMINMAXINFO && l != 0 {
            unsafe {
                let mmi = &mut *(l as *mut winapi::um::winuser::MINMAXINFO);
                mmi.ptMinTrackSize.x = MIN_WINDOW_W;
                mmi.ptMinTrackSize.y = MIN_WINDOW_H;
            }
        }
        None
    });
    if let Ok(h) = handler {
        // Rò có chủ đích như fonts ở build_ui: handler phải sống
        // cùng tiến trình để subclass không bị gỡ giữa chừng.
        Box::leak(Box::new(h));
    }
}

/// Chia `delta_h` (chênh lệch chiều cao client so với thiết kế) cho
/// các phần tử co giãn CÓ MẶT trong sân đang tính, trả về delta
/// chiều cao của từng phần tử. Ô nào vắng mặt trong sân có
/// design_h = 0 và không nhận phần chia nào (từ v0.7.10 mỗi sân chỉ
/// chứa một vài ô co giãn — chia cho ô vắng mặt sẽ làm thất thoát
/// delta và khối đáy không còn bám đáy).
/// Cao thừa: chia theo FLEX_GROW_W, phần dư làm tròn dồn vào ô có
/// trọng số nở lớn nhất trong sân (sân chính: ô log; tab 1: danh
/// sách ISO; tab 2: danh sách phân vùng — ô duy nhất).
/// Cao thiếu: chia theo FLEX_SHRINK_W nhưng mỗi phần tử không co
/// quá FLEX_MIN_H của nó — phần thiếu vượt quá tổng khả năng co thì
/// bỏ qua (tổng delta trả về nhỏ hơn delta_h): các control bên dưới
/// dịch theo TỔNG delta đã hấp thụ được nên không bao giờ chồng lên
/// nhau, phần đáy chỉ bị khuất khi màn hình thấp hơn cả mức co tối
/// đa — vẫn tốt hơn hẳn cửa sổ cố định tràn màn hình của v0.7.8.
fn flex_height_deltas(delta_h: i32, design_h: [i32; 6]) -> [i32; 6] {
    let mut out = [0i32; 6];
    if delta_h > 0 {
        let wsum: i32 = (0..6).filter(|&i| design_h[i] > 0).map(|i| FLEX_GROW_W[i]).sum();
        if wsum > 0 {
            let mut given = 0;
            for i in 0..6 {
                if design_h[i] > 0 {
                    out[i] = delta_h * FLEX_GROW_W[i] / wsum;
                    given += out[i];
                }
            }
            let mut best: Option<usize> = None;
            for i in 0..6 {
                if design_h[i] > 0 && best.map_or(true, |b| FLEX_GROW_W[i] > FLEX_GROW_W[b]) {
                    best = Some(i);
                }
            }
            if let Some(b) = best {
                out[b] += delta_h - given;
            }
        }
    } else if delta_h < 0 {
        let mut need = -delta_h;
        let mut cut = [0i32; 6];
        let mut active = [true; 6];
        while need > 0 && active.iter().any(|a| *a) {
            let wsum: i32 = (0..6).filter(|&i| active[i]).map(|i| FLEX_SHRINK_W[i]).sum();
            if wsum == 0 {
                break;
            }
            let mut spent = 0;
            let mut clamped = false;
            for i in 0..6 {
                if !active[i] {
                    continue;
                }
                let cap = design_h[i] - FLEX_MIN_H[i] - cut[i];
                if cap <= 0 {
                    active[i] = false;
                    clamped = true;
                    continue;
                }
                let share_full = need * FLEX_SHRINK_W[i] / wsum;
                let share = share_full.min(cap);
                if share < share_full {
                    clamped = true;
                }
                cut[i] += share;
                spent += share;
                if cut[i] >= design_h[i] - FLEX_MIN_H[i] {
                    active[i] = false;
                }
            }
            need -= spent;
            if spent == 0 {
                break;
            }
            if !clamped {
                // Không phần tử nào chạm sàn mà vẫn còn thiếu: đó là
                // phần dư làm tròn số nguyên — dồn vào các phần tử
                // còn cắt được, ưu tiên phần tử to trước.
                for i in [FLEX_ISO, FLEX_ED, FLEX_PARTS, FLEX_CAT, FLEX_LOG, FLEX_TABS] {
                    if need == 0 {
                        break;
                    }
                    let cap = design_h[i] - FLEX_MIN_H[i] - cut[i];
                    if cap > 0 {
                        let add = need.min(cap);
                        cut[i] += add;
                        need -= add;
                    }
                }
                break;
            }
        }
        for i in 0..6 {
            out[i] = -cut[i];
        }
    }
    out
}

/// Tính lại vị trí/kích thước các control thuộc MỘT sân (`group`)
/// từ bảng rect thiết kế + kích thước client hiện tại của sân đó.
/// Hàm thuần luỹ đẳng: cùng một kích thước client luôn cho cùng
/// một bố cục, và delta = 0 cho bố cục đúng như thiết kế.
///
/// Quy tắc dọc: một control dịch xuống đúng bằng tổng delta chiều
/// cao của các phần tử co giãn nằm TRÊN nó trong cùng sân (toạ độ
/// thiết kế nhỏ hơn) — các control ngang hàng với một danh sách
/// (nút bấm/caption bên phải) giữ nguyên đỉnh cùng danh sách đó;
/// riêng phần tử co giãn thì chiều cao cộng delta của chính nó.
/// Nhờ vậy khối đáy của mỗi sân bám đáy đúng bằng tổng delta đã
/// hấp thụ. Quy tắc ngang: cột phải dịch theo delta bề ngang,
/// danh sách/nhãn dài nở theo delta bề ngang.
fn apply_layout(items: &[LayoutItem], group: u8, design_client: (i32, i32), client: (i32, i32)) {
    if items.is_empty() {
        return;
    }
    let delta_w = client.0 - design_client.0;
    let delta_h = client.1 - design_client.1;
    let mut flex_design_h = [0i32; 6];
    let mut flex_y = [i32::MAX; 6];
    for it in items {
        if it.group != group {
            continue;
        }
        if let Some(f) = it.flex {
            flex_design_h[f] = it.h;
            flex_y[f] = it.y;
        }
    }
    let fd = flex_height_deltas(delta_h, flex_design_h);
    unsafe {
        let hdwp = winapi::um::winuser::BeginDeferWindowPos(items.len() as i32);
        for it in items {
            if it.group != group || it.hwnd.is_null() {
                continue;
            }
            let mut shift_y = 0;
            for f in 0..6 {
                if flex_y[f] < it.y {
                    shift_y += fd[f];
                }
            }
            let nx = if it.x_mode == XMode::ShiftRight { it.x + delta_w } else { it.x };
            let nw = if it.w_mode == WMode::Grow { (it.w + delta_w).max(40) } else { it.w };
            let ny = it.y + shift_y;
            let nh = if let Some(f) = it.flex { (it.h + fd[f]).max(16) } else { it.h };
            if hdwp.is_null() {
                SetWindowPos(it.hwnd, std::ptr::null_mut(), nx, ny, nw, nh, SWP_NOZORDER | SWP_NOACTIVATE);
            } else {
                winapi::um::winuser::DeferWindowPos(
                    hdwp,
                    it.hwnd,
                    std::ptr::null_mut(),
                    nx,
                    ny,
                    nw,
                    nh,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        if !hdwp.is_null() {
            winapi::um::winuser::EndDeferWindowPos(hdwp);
        }
    }
}

/// Áp bố cục thích ứng theo kích thước client hiện tại. Gọi lúc
/// khởi động (sau khi gắn handler) và mỗi sự kiện
/// OnResize/OnWindowMaximize.
///
/// Thứ tự quan trọng (v0.7.10): sân chính áp TRƯỚC — khung tab đổi
/// kích thước trong bước này thì NWG khớp lại hai trang ngay đồng
/// bộ (WM_SIZE của khung được xử lý nội bộ bởi TabsContainer), nên
/// kích thước trang đo NGAY SAU đó là kích thước thật để tính tiếp
/// hai sân tab. Hai trang luôn bằng nhau nên chỉ cần đo trang 1.
/// v0.7.12: cùng nguyên tắc cho hai trang tài liệu (sân TAB3/TAB4)
/// — đo ngay sau sân chính, mỗi trang đo client của chính nó.
fn layout_window(app: &Rc<App>) {
    let hwnd = match main_window_hwnd(app) {
        Some(h) => h,
        None => return,
    };
    let (cw, ch) = unsafe {
        let mut rc: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut rc) == 0 {
            return;
        }
        (rc.right, rc.bottom)
    };
    let page_hwnd = app.tab_src.handle.hwnd();
    let m = app.model.borrow();
    if m.layout.is_empty() {
        return;
    }
    apply_layout(&m.layout, GROUP_MAIN, m.design_client, (cw, ch));
    if let Some(ph) = page_hwnd {
        let (pw, phg) = unsafe {
            let mut rc: RECT = std::mem::zeroed();
            if GetClientRect(ph, &mut rc) == 0 {
                return;
            }
            (rc.right, rc.bottom)
        };
        apply_layout(&m.layout, GROUP_TAB1, m.design_page, (pw, phg));
        apply_layout(&m.layout, GROUP_TAB2, m.design_page, (pw, phg));
        // v0.7.12: hai trang tài liệu — đo client của TỪNG trang
        // (cả bốn trang luôn bằng nhau vì NWG khớp mọi trang con
        // theo cùng một vùng hiển thị của khung, nhưng đo riêng
        // từng trang thì đúng ngay cả khi một trang đang ẩn).
        // v0.7.27: đo thêm trang "Thông báo" cùng cách.
        for (tab, group) in [
            (&app.tab_guide, GROUP_TAB3),
            (&app.tab_docs, GROUP_TAB4),
            (&app.tab_notify, GROUP_TAB5),
        ] {
            if let Some(th) = tab.handle.hwnd() {
                let (tw, thg) = unsafe {
                    let mut rc: RECT = std::mem::zeroed();
                    if GetClientRect(th, &mut rc) == 0 {
                        continue;
                    }
                    (rc.right, rc.bottom)
                };
                apply_layout(&m.layout, group, m.design_page, (tw, thg));
            }
        }
    }
}

/// Ẩn/hiện một control bằng ShowWindow (cùng cách module WinPE dùng).
fn set_ctrl_visible(handle: &nwg::ControlHandle, visible: bool) {
    if let Some(hwnd) = handle.hwnd() {
        unsafe {
            ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
        }
    }
}

/// Đồng bộ hiển thị khối phân vùng/đĩa đích theo chế độ hiện tại:
/// nút chọn chế độ phân vùng chỉ hiện khi Cài mới; khối chọn đĩa +
/// ô nhập serial chỉ hiện thêm khi chọn wipeDisk hoặc rebuild, với
/// nhãn/gợi ý riêng cho từng chế độ.
fn update_part_ui(app: &Rc<App>) {
    let (clean, mode) = {
        let m = app.model.borrow();
        (m.mode_clean, m.part_mode)
    };
    set_ctrl_visible(&app.btn_part_preserve.handle, clean);
    set_ctrl_visible(&app.btn_part_wipe.handle, clean);
    set_ctrl_visible(&app.btn_part_rebuild.handle, clean);
    let show_disk = clean && mode != PartMode::Preserve;
    set_ctrl_visible(&app.lbl_disk_cap.handle, show_disk);
    set_ctrl_visible(&app.cmb_disk.handle, show_disk);
    set_ctrl_visible(&app.lbl_disk_hint.handle, show_disk);
    set_ctrl_visible(&app.lst_disk_parts.handle, show_disk);
    set_ctrl_visible(&app.lbl_disk_warn.handle, show_disk);
    set_ctrl_visible(&app.lbl_serial_cap.handle, show_disk);
    set_ctrl_visible(&app.inp_disk_serial.handle, show_disk);
    set_ctrl_visible(&app.lbl_disk_id.handle, show_disk);
    set_ctrl_visible(&app.lbl_serial_fb.handle, show_disk);
    if show_disk {
        match mode {
            PartMode::WipeDisk => {
                app.lbl_disk_cap.set_text("Ổ đĩa đích (toàn bộ đĩa vật lý):");
                app.lbl_disk_hint.set_text("Chế độ này xoá theo Ổ ĐĨA VẬT LÝ, không theo ký tự ổ.\nNếu ổ C và ổ dữ liệu nằm chung một đĩa vật lý, hãy dùng chế độ 'Cài lại phân vùng hệ thống (giữ data)'\nhoặc 'Giữ nguyên phân vùng', hoặc boot WinPE từ USB với nguồn cài trên USB.");
                app.lbl_serial_cap.set_text("Nhập serial của ổ đĩa đích để xác nhận XOÁ TOÀN BỘ:");
            }
            PartMode::Rebuild => {
                app.lbl_disk_cap.set_text("Ổ đĩa đích (đĩa chứa phân vùng hệ thống cũ):");
                app.lbl_disk_hint.set_text("Chỉ xoá các phân vùng HỆ THỐNG được tick ở danh sách dưới (EFI, MSR, Windows cũ, Recovery)\nvà chia lại layout mới trong đúng vùng trống đó. Phân vùng DATA trên cùng đĩa — kể cả phân\nvùng chứa bộ cài — KHÔNG được tick và được giữ nguyên tuyệt đối.");
                app.lbl_serial_cap.set_text("Nhập serial của ổ đĩa đích để xác nhận CÀI LẠI PHÂN VÙNG HỆ THỐNG:");
            }
            PartMode::Preserve => {}
        }
    }
}

/// Đĩa vật lý đang chọn trong cmb_disk (theo thứ tự preflight.disks).
fn selected_disk(app: &Rc<App>) -> Option<PhysicalDisk> {
    let idx = app.cmb_disk.selection()?;
    let m = app.model.borrow();
    m.preflight.as_ref().and_then(|pf| pf.disks.get(idx).cloned())
}

/// Lý do một đĩa KHÔNG được làm đích phá huỷ phân vùng trong wizard
/// (None = hợp lệ), theo TỪNG chế độ:
///
/// - wipeDisk: chốt cứng là đĩa chứa ổ Data/staging đang chọn — đối
///   chiếu bằng HAI đường độc lập: ký tự ổ trong bảng phân vùng
///   (contains_letter) và số đĩa vật lý của chính volume Data
///   (IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS qua `volume_disk_number`),
///   để một bảng phân vùng đọc lỗi không làm lọt đĩa Data. Đĩa chứa
///   Windows ĐANG CHẠY (ổ C cũ) KHÔNG bị cấm ở wizard: thao tác wipe
///   diễn ra sau đó trong WinPE chứ không phải từ Windows đang chạy,
///   và đó chính là đích cài lại điển hình.
/// - rebuild (v0.7.13): đĩa chứa ổ Data/staging ĐƯỢC PHÉP làm đích
///   (đó là mục đích của chế độ này — máy 1 đĩa); phân vùng staging
///   được bảo vệ ở tầng danh sách phân vùng + tầng engine. Đổi lại,
///   đĩa liệt kê qua đường DỰ PHÒNG bị cấm: không có GUID/offset
///   phân vùng tin cậy thì không đối chiếu an toàn được khi xoá.
///   Cũng không áp ngưỡng dung lượng đĩa của wipe — vùng hệ thống
///   đủ lớn hay không được kiểm riêng ở bước xác nhận.
///
/// `data_disk_num` = số đĩa của volume Data theo đường extents
/// (None = không xác định được vị trí ổ Data).
fn disk_block_reason(
    d: &PhysicalDisk,
    data_letter: Option<char>,
    data_disk_num: Option<u32>,
    mode: PartMode,
) -> Option<String> {
    if mode == PartMode::Rebuild {
        if d.serial.trim().is_empty() {
            return Some("BỊ CẤM: không đọc được serial ổ đĩa".to_string());
        }
        if d.size == 0 {
            return Some("BỊ CẤM: không đọc được dung lượng ổ đĩa".to_string());
        }
        if d.from_fallback {
            return Some(
                "BỊ CẤM: đĩa chỉ liệt kê được qua đường dự phòng — thiếu GUID/offset phân vùng tin cậy để đối chiếu khi xoá"
                    .to_string(),
            );
        }
        return None;
    }
    if let Some(l) = data_letter {
        if d.contains_letter(l) || data_disk_num == Some(d.number) {
            return Some(format!("BỊ CẤM: chứa ổ dữ liệu/staging {}:", l));
        }
    }
    if d.serial.trim().is_empty() {
        return Some("BỊ CẤM: không đọc được serial ổ đĩa".to_string());
    }
    if d.size == 0 {
        return Some("BỊ CẤM: không đọc được dung lượng ổ đĩa".to_string());
    }
    // Đĩa định danh qua đường dự phòng mà không có phân vùng nào có
    // ký tự ổ, đồng thời không xác định được vị trí ổ Data bằng
    // đường extents → không chứng minh được đĩa không chứa ổ Data.
    if d.from_fallback
        && data_letter.is_some()
        && data_disk_num.is_none()
        && d.partitions.iter().all(|p| p.letter.is_none())
    {
        return Some("BỊ CẤM: không xác minh được phân vùng trên đĩa (liệt kê dự phòng)".to_string());
    }
    if d.size < disk::MIN_WIPE_DISK_BYTES {
        return Some(format!(
            "BỊ CẤM: đĩa quá nhỏ (tối thiểu {})",
            sys::format_gb(disk::MIN_WIPE_DISK_BYTES)
        ));
    }
    None
}

/// Đổ lại danh sách đĩa vật lý vào cmb_disk: MỌI đĩa tìm thấy đều
/// hiện (kể cả đĩa bị cấm — kèm lý do ngay trong dòng chọn, giống
/// cách module WinPE đánh dấu BỊ CẤM), để người dùng phân biệt được
/// "không thấy đĩa nào" với "thấy đĩa nhưng bị cấm". Lý do cấm tính
/// theo chế độ phân vùng hiện tại (wipe cấm đĩa chứa Data; rebuild
/// cho phép nhưng cấm đĩa liệt kê dự phòng). Gọi khi: quét xong
/// (on_preflight), đổi ổ Data, và mỗi lần vào chế độ wipe/rebuild.
fn refresh_disk_picker(app: &Rc<App>) {
    // Giữ lựa chọn hiện tại theo SỐ ĐĨA (thứ tự danh sách có thể đổi
    // sau mỗi lần quét).
    let prev_number = selected_disk(app).map(|d| d.number);
    let (items, selection) = {
        let m = app.model.borrow();
        match &m.preflight {
            None => (Vec::new(), None),
            Some(pf) if pf.disks.is_empty() => (
                vec!["Không liệt kê được ổ đĩa vật lý nào — xem log".to_string()],
                Some(0),
            ),
            Some(pf) => {
                let data_disk_num = m.data_letter.and_then(disk::volume_disk_number);
                let items: Vec<String> = pf
                    .disks
                    .iter()
                    .map(|d| match disk_block_reason(d, m.data_letter, data_disk_num, m.part_mode) {
                        Some(r) => format!("{} — {}", d.summary(), r),
                        None => d.summary(),
                    })
                    .collect();
                // Ưu tiên giữ lựa chọn cũ; không có thì chọn đĩa hợp
                // lệ chứa Windows hiện tại; rồi đĩa hợp lệ đầu tiên;
                // cuối cùng là đĩa đầu tiên (để hiện phân vùng + lý
                // do cấm của nó thay vì để trống khó hiểu).
                let selection = prev_number
                    .and_then(|n| pf.disks.iter().position(|d| d.number == n))
                    .or_else(|| {
                        pf.disks.iter().position(|d| {
                            d.contains_letter(pf.os_letter)
                                && disk_block_reason(d, m.data_letter, data_disk_num, m.part_mode)
                                    .is_none()
                        })
                    })
                    .or_else(|| {
                        pf.disks.iter().position(|d| {
                            disk_block_reason(d, m.data_letter, data_disk_num, m.part_mode)
                                .is_none()
                        })
                    })
                    .or(Some(0));
                (items, selection)
            }
        }
    };
    app.cmb_disk.set_collection(items);
    if let Some(idx) = selection {
        app.cmb_disk.set_selection(Some(idx));
    }
    update_disk_parts(app);
}

/// Phân loại một phân vùng trên đĩa đích trong chế độ rebuild.
#[derive(Clone, Copy, PartialEq)]
enum RebuildClass {
    /// Phân vùng chứa Windows đang chạy — BẮT BUỘC xoá (chỗ cài mới).
    Windows,
    /// EFI / MSR / Recovery / System Reserved — tick sẵn, bỏ tick được.
    System,
    /// Phân vùng chứa volume Data/staging — bảo vệ tuyệt đối.
    Staging,
    /// Mọi phân vùng còn lại (data) — giữ nguyên, không tick được.
    Data,
}

/// Phân loại dựa trên KÝ TỰ Ổ + SERIAL VOLUME (không dựa nhãn):
/// staging xét TRƯỚC Windows để phân vùng chứa bộ cài luôn được
/// bảo vệ dù dữ kiện có lạ (wizard đã chặn Data == OS từ trước).
fn classify_partition(
    p: &DiskPartition,
    os_letter: char,
    os_serial: u64,
    data_letter: Option<char>,
    data_serial: u64,
) -> RebuildClass {
    if data_letter.is_some() && p.letter == data_letter {
        return RebuildClass::Staging;
    }
    if data_serial != 0 && p.volume_serial != 0 && p.volume_serial as u64 == data_serial {
        return RebuildClass::Staging;
    }
    if p.letter == Some(os_letter) {
        return RebuildClass::Windows;
    }
    if os_serial != 0 && p.volume_serial != 0 && p.volume_serial as u64 == os_serial {
        return RebuildClass::Windows;
    }
    match p.kind {
        PartKind::Efi | PartKind::Msr | PartKind::Recovery | PartKind::System => {
            RebuildClass::System
        }
        _ => RebuildClass::Data,
    }
}

/// Chuỗi `kind` ghi vào stage.json rebuildPartitions cho một phân
/// vùng được chọn xoá.
fn rebuild_kind_str(p: &DiskPartition, class: RebuildClass) -> &'static str {
    if class == RebuildClass::Windows {
        return "Windows";
    }
    match p.kind {
        PartKind::Efi => "EFI",
        PartKind::Msr => "MSR",
        PartKind::Recovery => "Recovery",
        PartKind::System => "System",
        _ => "System",
    }
}

/// Chụp trạng thái tick hiện tại của lst_disk_parts vào model (chỉ
/// khi danh sách đang hiển thị đúng một đĩa ở chế độ rebuild) — gọi
/// ở đầu `update_disk_parts` để việc vẽ lại không làm mất lựa chọn
/// của người dùng trên đĩa đang xem.
fn snapshot_rebuild_checks(app: &Rc<App>) {
    let displayed = {
        let m = app.model.borrow();
        if m.part_mode != PartMode::Rebuild {
            return;
        }
        match (m.parts_disk, &m.preflight) {
            (Some(n), Some(pf)) => pf
                .disks
                .iter()
                .find(|d| d.number == n)
                .map(|d| (d.number, d.partitions.len())),
            _ => None,
        }
    };
    let (disk_num, count) = match displayed {
        Some(v) => v,
        None => return,
    };
    if app.lst_disk_parts.len() != count {
        return; // danh sách chưa đồng bộ — không chụp bừa
    }
    let mut checks = Vec::with_capacity(count);
    for i in 0..count {
        checks.push(lv_item_checked(&app.lst_disk_parts, i));
    }
    let mut m = app.model.borrow_mut();
    m.rebuild_checks = checks;
    m.rebuild_checks_disk = Some(disk_num);
}

/// Ép tính hợp lệ của checkbox trong danh sách phân vùng rebuild:
/// phân vùng Windows cũ luôn tick (bắt buộc), phân vùng staging và
/// phân vùng data không bao giờ được tick. Chạy mỗi lần trạng thái
/// một hàng thay đổi; khi code tự đặt trạng thái (lúc vẽ) mọi hàng
/// đều đã hợp lệ nên hàm này không làm gì thêm.
fn enforce_rebuild_checks(app: &Rc<App>) {
    let (os_letter, os_serial, data_letter, data_serial, parts_disk) = {
        let m = app.model.borrow();
        if m.part_mode != PartMode::Rebuild {
            return;
        }
        let pf = match &m.preflight {
            Some(pf) => pf,
            None => return,
        };
        (
            pf.os_letter,
            pf.drive(pf.os_letter).map(|d| d.serial as u64).unwrap_or(0),
            m.data_letter,
            m.data_letter
                .and_then(|l| pf.drive(l))
                .map(|d| d.serial as u64)
                .unwrap_or(0),
            m.parts_disk,
        )
    };
    let d = match selected_disk(app) {
        Some(d) => d,
        None => return,
    };
    if parts_disk != Some(d.number) {
        return; // danh sách đang vẽ lại cho đĩa khác — chưa đồng bộ
    }
    for (i, p) in d.partitions.iter().enumerate() {
        let class = classify_partition(p, os_letter, os_serial, data_letter, data_serial);
        let want = match class {
            RebuildClass::Windows => Some(true),
            RebuildClass::Staging | RebuildClass::Data => Some(false),
            RebuildClass::System => None,
        };
        if let Some(w) = want {
            if lv_item_checked(&app.lst_disk_parts, i) != w {
                lv_set_checked(&app.lst_disk_parts, i, w);
            }
        }
    }
}

/// Dòng định danh đĩa đích + phản hồi khớp serial (v0.7.17).
/// Thực địa: ô chọn đĩa hẹp nên dòng định danh bị cắt đúng ở
/// phần serial — kỹ thuật viên không thấy serial đĩa đích để gõ
/// xác nhận, gõ xong cũng không biết đã khớp chưa cho tới khi
/// bấm TIẾN HÀNH mới bị hộp thoại chặn. Nay: (1) dòng định danh
/// ĐẦY ĐỦ (số đĩa — model — dung lượng — serial) hiện ngay trên
/// ô nhập, cập nhật mỗi lần đĩa chọn thay đổi; (2) nhãn cạnh ô
/// nhập phản hồi TRỰC TIẾP theo từng ký tự. So khớp dùng đúng
/// `normalize_disk_serial` như chốt kiểm tra lúc bấm TIẾN HÀNH
/// (bỏ ký tự lạ + không phân biệt hoa thường) nên phản hồi và
/// chốt chặn không bao giờ lệch nhau.
fn update_serial_ui(app: &Rc<App>) {
    match selected_disk(app) {
        Some(d) => {
            app.lbl_disk_id.set_text(&format!(
                "Đĩa đích: Đĩa {} — {} — {} — serial {}",
                d.number,
                if d.model.trim().is_empty() { "(không rõ model)" } else { d.model.trim() },
                sys::format_gb(d.size),
                if d.serial.trim().is_empty() { "(không đọc được)" } else { d.serial.trim() },
            ));
        }
        None => {
            app.lbl_disk_id.set_text("Đĩa đích: (chưa chọn đĩa)");
        }
    }
    update_serial_feedback(app);
}

/// Phản hồi khớp serial theo nội dung HIỆN TẠI của ô nhập:
/// trống → không hiện gì; khớp (theo normalize_disk_serial) →
/// "Đã khớp"; còn lại → "Chưa khớp".
fn update_serial_feedback(app: &Rc<App>) {
    let typed = app.inp_disk_serial.text();
    if typed.trim().is_empty() {
        app.lbl_serial_fb.set_text("");
        return;
    }
    let matched = match selected_disk(app) {
        Some(d) => {
            let want = disk::normalize_disk_serial(&d.serial);
            !want.is_empty() && disk::normalize_disk_serial(&typed) == want
        }
        None => false,
    };
    app.lbl_serial_fb.set_text(if matched {
        "✓ Đã khớp serial đĩa đích"
    } else {
        "✗ Chưa khớp serial đĩa đích"
    });
}

/// Đổ danh sách phân vùng của đĩa đang chọn + cảnh báo theo chế độ:
/// wipeDisk = danh sách thuần hiển thị; rebuild = danh sách có
/// checkbox (hệ thống tick sẵn, Windows cũ khoá tick, staging +
/// data khoá không-tick — xem `classify_partition`).
fn update_disk_parts(app: &Rc<App>) {
    update_serial_ui(app);
    snapshot_rebuild_checks(app);
    let disks_empty = {
        let m = app.model.borrow();
        m.preflight.as_ref().map(|pf| pf.disks.is_empty()).unwrap_or(false)
    };
    if disks_empty {
        app.lst_disk_parts.clear();
        app.model.borrow_mut().parts_disk = None;
        app.lbl_disk_warn
            .set_text("Không liệt kê được ổ đĩa vật lý nào — xem log để biết chi tiết lỗi liệt kê.");
        return;
    }
    let d = match selected_disk(app) {
        Some(d) => d,
        None => {
            app.lst_disk_parts.clear();
            app.model.borrow_mut().parts_disk = None;
            app.lbl_disk_warn.set_text("Hãy chọn một ổ đĩa đích trong danh sách bên trên.");
            return;
        }
    };
    let (mode, data_letter, data_disk_num, os_letter, os_serial, data_serial) = {
        let m = app.model.borrow();
        let pf = m.preflight.as_ref();
        (
            m.part_mode,
            m.data_letter,
            m.data_letter.and_then(disk::volume_disk_number),
            pf.map(|pf| pf.os_letter).unwrap_or('C'),
            pf.and_then(|pf| pf.drive(pf.os_letter))
                .map(|d| d.serial as u64)
                .unwrap_or(0),
            m.data_letter
                .and_then(|l| pf.and_then(|pf| pf.drive(l)))
                .map(|d| d.serial as u64)
                .unwrap_or(0),
        )
    };
    lv_set_checkboxes(&app.lst_disk_parts, mode == PartMode::Rebuild);
    app.lst_disk_parts.clear();
    if mode == PartMode::Rebuild {
        // Trạng thái tick: khôi phục của chính đĩa này nếu đã có,
        // không thì mặc định theo phân loại; sau đó ép hợp lệ.
        let mut checks: Vec<bool> = {
            let m = app.model.borrow();
            if m.rebuild_checks_disk == Some(d.number)
                && m.rebuild_checks.len() == d.partitions.len()
            {
                m.rebuild_checks.clone()
            } else {
                d.partitions
                    .iter()
                    .map(|p| {
                        matches!(
                            classify_partition(p, os_letter, os_serial, data_letter, data_serial),
                            RebuildClass::Windows | RebuildClass::System
                        )
                    })
                    .collect()
            }
        };
        for (i, p) in d.partitions.iter().enumerate() {
            match classify_partition(p, os_letter, os_serial, data_letter, data_serial) {
                RebuildClass::Windows => checks[i] = true,
                RebuildClass::Staging | RebuildClass::Data => checks[i] = false,
                RebuildClass::System => {}
            }
        }
        {
            let mut m = app.model.borrow_mut();
            m.rebuild_checks = checks.clone();
            m.rebuild_checks_disk = Some(d.number);
        }
        for (i, p) in d.partitions.iter().enumerate() {
            let class = classify_partition(p, os_letter, os_serial, data_letter, data_serial);
            let note = match class {
                RebuildClass::Windows => "Windows cũ — SẼ XOÁ (bắt buộc, không bỏ tick được)",
                RebuildClass::System => "Phân vùng hệ thống — tick = xoá và chia lại",
                RebuildClass::Staging => "Phân vùng chứa bộ cài — ĐƯỢC BẢO VỆ (không xoá được)",
                RebuildClass::Data => "Phân vùng data — GIỮ NGUYÊN (không xoá được)",
            };
            lv_insert_row(
                &app.lst_disk_parts,
                i as i32,
                &[
                    format!(
                        "{} — {}",
                        p.letter.map(|c| format!("{}:", c)).unwrap_or_else(|| "(không ký tự)".to_string()),
                        if p.label.is_empty() { "(không nhãn)".to_string() } else { p.label.clone() }
                    ),
                    p.kind.label_vi().to_string(),
                    sys::format_gb(p.size),
                    note.to_string(),
                ],
            );
        }
        for (i, on) in checks.iter().enumerate() {
            lv_set_checked(&app.lst_disk_parts, i, *on);
        }
    } else {
        for (i, p) in d.partitions.iter().enumerate() {
            lv_insert_row(
                &app.lst_disk_parts,
                i as i32,
                &[
                    format!(
                        "{} — {}",
                        p.letter.map(|c| format!("{}:", c)).unwrap_or_else(|| "(không ký tự)".to_string()),
                        if p.label.is_empty() { "(không nhãn)".to_string() } else { p.label.clone() }
                    ),
                    p.kind.label_vi().to_string(),
                    sys::format_gb(p.size),
                    if mode == PartMode::WipeDisk {
                        "Sẽ bị XOÁ TOÀN BỘ cùng đĩa đích".to_string()
                    } else {
                        String::new()
                    },
                ],
            );
        }
    }
    app.model.borrow_mut().parts_disk = Some(d.number);
    let warn = match disk_block_reason(&d, data_letter, data_disk_num, mode) {
        Some(r) => {
            if mode == PartMode::Rebuild {
                format!(
                    "{} — không thể chọn đĩa này làm đích cài lại phân vùng. Hãy chọn một đĩa khác.",
                    r
                )
            } else {
                format!(
                    "{} — không thể chọn đĩa này làm đích xoá. Hãy chọn một đĩa khác, hoặc chuyển ổ Data sang đĩa khác trước.",
                    r
                )
            }
        }
        None => {
            if mode == PartMode::Rebuild {
                "Cài lại phân vùng hệ thống: chỉ các phân vùng được TICK ở danh sách bên trái mới bị xoá và chia lại. Phân vùng Windows cũ bắt buộc phải xoá; phân vùng data và phân vùng chứa bộ cài không tick được và sẽ giữ nguyên. Hãy nhập đúng serial của đĩa vào ô xác nhận bên dưới.".to_string()
            } else {
                format!(
                    "CẢNH BÁO: XOÁ TOÀN BỘ đĩa này sẽ xoá sạch {} phân vùng liệt kê bên trái. Hãy nhập đúng serial của đĩa vào ô xác nhận bên dưới.",
                    d.partitions.len()
                )
            }
        }
    };
    app.lbl_disk_warn.set_text(&warn);
}

// ---------------------------------------------------------------------------
// Cập nhật giao diện từ kết quả luồng nền (NWG Notice)
// ---------------------------------------------------------------------------
fn render_log(app: &Rc<App>) {
    let (text, total) = {
        let s = lock_shared(&app.shared);
        (
            s.log_lines.iter().cloned().collect::<Vec<_>>().join("\r\n"),
            s.log_lines.len(),
        )
    };
    // Mượn model kiểu KHÔNG panic (v0.7.4): render_log có thể bị gọi
    // TÁI NHẬP đồng bộ ngay giữa một handler khác (mọi thông báo NWG
    // từ luồng giao diện đều dispatch đồng bộ qua SendNotifyMessageW
    // — xem ghi chú ở khối logger trong on_start_inner). Nếu model
    // đang bị mượn ở khung hình bên ngoài, bỏ qua lượt vẽ này: dòng
    // log chưa vẽ sẽ được vẽ ở notice kế tiếp. Tuyệt đối không để
    // việc vẽ log làm sập tiến trình vì double-borrow.
    let rendered = match app.model.try_borrow() {
        Ok(m) => m.log_rendered,
        Err(_) => return,
    };
    if total != rendered {
        app.txt_log.set_text(&text);
        if let Ok(mut m) = app.model.try_borrow_mut() {
            m.log_rendered = total;
        }
    }
}

fn power_text(p: &PowerStatus) -> String {
    if p.ac_online {
        "Đang cắm sạc (AC)".to_string()
    } else if !p.has_battery {
        "Không có pin (máy bàn?)".to_string()
    } else if p.percent == 255 {
        "Pin (không rõ %)".to_string()
    } else {
        format!("Pin {}% — CHƯA cắm sạc", p.percent)
    }
}

fn verdict_of(pf: &Preflight, settings: &Settings) -> String {
    if pf.bitlocker == BitlockerState::InProgress {
        return "Kết luận: BitLocker đang mã hoá/giải mã dở dang — KHÔNG thể cài cho tới khi hoàn tất.".to_string();
    }
    if pf.power.has_battery && !pf.power.ac_online && pf.power.percent < 60 && pf.power.percent != 255 {
        return format!(
            "Kết luận: Máy đang chạy pin {}% — hãy cắm sạc trước khi cài đặt.",
            pf.power.percent
        );
    }
    if pf.non_os_drives().is_empty() {
        return "Kết luận: Không tìm thấy ổ Data — cài mới từ xa bị tắt; chỉ cài đè được bằng ISO có sẵn.".to_string();
    }
    if pf.os_free() < settings.min_free_gb_clean * GB {
        return "Kết luận: Ổ OS sắp hết dung lượng trống — nên dọn dẹp trước khi cài.".to_string();
    }
    "Kết luận: Đủ điều kiện cài đặt.".to_string()
}

fn update_data_info(app: &Rc<App>) {
    let m = app.model.borrow();
    if let (Some(pf), Some(letter)) = (&m.preflight, m.data_letter) {
        if let Some(d) = pf.drive(letter) {
            let marker = if pf.marker_drives.contains(&letter) {
                "đã có marker an toàn"
            } else {
                "chưa có marker (sẽ tạo khi bắt đầu)"
            };
            app.l_data_info.set_text(&format!(
                "trống {} — {}",
                sys::format_gb(d.free),
                marker
            ));
        }
        let text = verdict_of(pf, &m.settings);
        app.l_verdict.set_text(&text);
    }
}

/// Ghi log quét hệ thống ra file NGAY khi quét xong (v0.7.2).
///
/// Trước đây chẩn đoán [Đĩa] chỉ vào file log khi BẮT ĐẦU cài —
/// mà cài không bắt đầu được khi chính liệt kê đĩa lỗi, nên bằng
/// chứng bị kẹt trong log panel. Nay mỗi lần quét xong (kể cả quét
/// lại) wizard ghi toàn bộ kết quả quét + chẩn đoán [Đĩa] của cả
/// hai đường liệt kê ra `<ổ Data>:\<logs>\wizard-scan-<timestamp>.log`;
/// ổ Data chưa chọn được/không ghi được thì ghi cạnh file exe wizard.
/// Đường dẫn file được báo lại qua log panel để người dùng gửi file
/// khi cần chẩn đoán.
fn persist_scan_log(app: &Rc<App>, pf: &Preflight) {
    let (settings, exe_dir, data_letter) = {
        let m = app.model.borrow();
        (m.settings.clone(), m.exe_dir.clone(), m.data_letter)
    };
    // --- Nội dung: tóm tắt tiền đề + đĩa vật lý + chẩn đoán [Đĩa] ---
    let mut c = String::new();
    c.push_str("=== LOG QUÉT HỆ THỐNG — wizard Cài Đặt Windows Tự Động ===\n");
    c.push_str(&format!("Thời gian: {}\n", qsu_common::log::now_string()));
    c.push_str(&format!("Máy hiện tại: {}\n", sys::computer_name()));
    c.push_str(&format!(
        "Ổ Cài Windows (OS): {}: — {} {} (build {}) — trống {}\n",
        pf.os_letter,
        pf.os.product_name,
        pf.os.display_version,
        pf.os.build,
        sys::format_gb(pf.os_free())
    ));
    c.push_str(&format!(
        "Boot Mode: {} ({}) | BitLocker ổ OS: {}\n",
        if pf.uefi { "UEFI" } else { "Legacy" },
        match pf.part {
            PartitionStyle::Gpt => "GPT",
            PartitionStyle::Mbr => "MBR",
            PartitionStyle::Unknown => "không rõ",
        },
        pf.bitlocker.label()
    ));
    c.push_str(&format!("Firmware raw: {}\n", pf.firmware.detail()));
    c.push_str(&format!(
        "Nguồn điện: {} | Key OEM trong BIOS: {}\n",
        power_text(&pf.power),
        if pf.oem_key { "Có" } else { "Không" }
    ));
    c.push_str(&format!("{}\n", verdict_of(pf, &settings)));
    c.push_str("Các ổ cố định:\n");
    for d in &pf.drives {
        c.push_str(&format!(
            "  {}: — {} — tổng {} — trống {}{}\n",
            d.letter,
            if d.label.is_empty() { "(không nhãn)" } else { d.label.as_str() },
            sys::format_gb(d.total),
            sys::format_gb(d.free),
            if pf.marker_drives.contains(&d.letter) {
                " — có marker .qsu_data_drive"
            } else {
                ""
            }
        ));
    }
    c.push_str(&format!(
        "Ổ Data wizard đang chọn: {}\n",
        data_letter
            .map(|l| format!("{}:", l))
            .unwrap_or_else(|| "(chưa chọn được)".to_string())
    ));
    c.push_str(&format!("Đĩa vật lý liệt kê được: {}\n", pf.disks.len()));
    for d in &pf.disks {
        c.push_str(&format!("  {}\n", d.summary()));
        for line in d.describe_partitions().lines() {
            c.push_str(&format!("    {}\n", line.trim_start()));
        }
    }
    c.push_str("--- Chẩn đoán liệt kê đĩa ---\n");
    if pf.disk_diag.is_empty() {
        c.push_str("(không có dòng chẩn đoán)\n");
    } else {
        for line in &pf.disk_diag {
            c.push_str(&format!("[Đĩa] {}\n", line));
        }
    }
    c.push_str("--- Hết log quét ---\n");

    // --- Đường ghi: ưu tiên ổ Data, dự phòng cạnh file exe wizard ---
    let file_name = format!("wizard-scan-{}.log", qsu_common::log::log_stamp());
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(l) = data_letter {
        candidates.push(
            PathBuf::from(format!("{}:\\", l))
                .join(&settings.logs_folder)
                .join(&file_name),
        );
    }
    candidates.push(exe_dir.join(&file_name));
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let zero_disks = pf.disks.is_empty();
    std::thread::spawn(move || {
        let mut saved: Option<PathBuf> = None;
        let mut errors: Vec<String> = Vec::new();
        for path in &candidates {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(path, &c) {
                Ok(()) => {
                    saved = Some(path.clone());
                    break;
                }
                Err(e) => errors.push(format!("{}: {}", path.display(), e)),
            }
        }
        match saved {
            Some(p) => {
                if zero_disks {
                    push_log(
                        &shared,
                        &sender,
                        &format!(
                            "Không liệt kê được đĩa vật lý nào — đã lưu log quét kèm chẩn đoán [Đĩa] vào: {} (hãy gửi file này để chẩn đoán).",
                            p.display()
                        ),
                    );
                } else {
                    push_log(
                        &shared,
                        &sender,
                        &format!("Đã lưu log quét hệ thống: {}", p.display()),
                    );
                }
            }
            None => push_log(
                &shared,
                &sender,
                &format!("Không lưu được log quét hệ thống: {}", errors.join("; ")),
            ),
        }
    });
}

fn on_preflight(app: &Rc<App>, pf: Preflight) {
    app.l_os.set_text(&format!(
        "Ổ Cài Windows (OS): {}: — {} {} (build {}) — trống {}",
        pf.os_letter,
        pf.os.product_name,
        pf.os.display_version,
        pf.os.build,
        sys::format_gb(pf.os_free())
    ));
    app.l_boot.set_text(&format!(
        "Boot Mode: {} ({})      |      BitLocker ổ OS: {}",
        if pf.uefi { "UEFI" } else { "Legacy" },
        match pf.part {
            PartitionStyle::Gpt => "GPT",
            PartitionStyle::Mbr => "MBR",
            PartitionStyle::Unknown => "không rõ",
        },
        pf.bitlocker.label()
    ));
    let items: Vec<String> = pf
        .non_os_drives()
        .iter()
        .map(|d| {
            format!(
                "{}: — {} — tổng {}",
                d.letter,
                if d.label.is_empty() { "(không nhãn)" } else { d.label.as_str() },
                sys::format_gb(d.total)
            )
        })
        .collect();
    // Không có ổ Data nào: hiện rõ trong combo thay vì để trống
    // (handler chọn ổ Data tra theo chỉ số trong non_os_drives nên
    // dòng giả này vô hại — không tra ra ổ nào cả).
    if items.is_empty() {
        app.cmb_data
            .set_collection(vec!["(Không tìm thấy ổ dữ liệu — cắm thêm ổ/USB khác)".to_string()]);
    } else {
        app.cmb_data.set_collection(items);
    }
    let default_data = pf.default_data_letter();
    if let Some(dl) = default_data {
        if let Some(idx) = pf.non_os_drives().iter().position(|d| d.letter == dl) {
            app.cmb_data.set_selection(Some(idx));
        }
    }
    app.l_power.set_text(&format!(
        "Nguồn điện: {}      |      Key OEM trong BIOS: {}      |      Dung lượng trống ổ OS: {}",
        power_text(&pf.power),
        if pf.oem_key { "Có" } else { "Không" },
        sys::format_gb(pf.os_free())
    ));
    let scan_drives: Vec<char> = pf
        .drives
        .iter()
        .filter(|d| d.letter != pf.os_letter)
        .map(|d| d.letter)
        .collect();
    {
        let mut m = app.model.borrow_mut();
        m.data_letter = default_data;
        m.disk_diag = pf.disk_diag.clone();
        m.preflight = Some(pf);
    }
    // Ổ Data đã biết ngay từ lúc quét xong: hộp đen panic ghi thêm
    // bản sao vào thư mục log trên ổ Data kể từ thời điểm này,
    // không đợi tới lúc bắt đầu cài (v0.7.4).
    if let Some(dl) = default_data {
        let logs_dir = PathBuf::from(format!("{}:\\", dl))
            .join(&app.model.borrow().settings.logs_folder);
        set_panic_log_dir(logs_dir);
    }
    // Đổ danh sách đĩa vật lý (kèm lý do cấm trên từng đĩa) — phải
    // sau khi preflight đã vào model vì refresh đọc từ model.
    refresh_disk_picker(app);
    // Ghi log quét ra file NGAY (v0.7.2): chẩn đoán [Đĩa] phải tới
    // được file kể cả khi không bắt đầu cài được (chính ca liệt kê
    // đĩa lỗi). Clone snapshot vì persist đọc lại model.
    {
        let snapshot = app.model.borrow().preflight.clone();
        if let Some(pf) = &snapshot {
            persist_scan_log(app, pf);
        }
    }
    if app.inp_computer.text().is_empty() {
        app.inp_computer.set_text(&sys::computer_name());
    }
    if app.inp_user.text().is_empty() {
        app.inp_user
            .set_text(&std::env::var("USERNAME").unwrap_or_else(|_| "Admin".to_string()));
    }
    {
        let m = app.model.borrow();
        app.lbl_stage_info.set_text(&stage_info_text(&m.settings));
    }
    update_disk_parts(app);
    update_data_info(app);
    update_start_enabled(app);
    push_log(
        &app.shared,
        &app.notice.sender(),
        "Đã quét hệ thống xong. Đang quét file ISO trên các ổ dữ liệu...",
    );
    spawn_iso_scan(app.shared.clone(), app.notice.sender(), scan_drives);
}

fn stage_info_text(st: &Settings) -> String {
    let winpe = if st.winpe_wim_path.is_empty() {
        "WinPE: dùng boot.wim trong ISO".to_string()
    } else {
        format!("WinPE: {}", st.winpe_wim_path)
    };
    let drv = if st.storage_drivers_path.is_empty() {
        "Driver lưu trữ: không có (bỏ qua inject)".to_string()
    } else {
        format!("Driver lưu trữ: {}", st.storage_drivers_path)
    };
    format!("{}   |   {}", winpe, drv)
}

fn refill_iso_list(app: &Rc<App>, select: Option<&Path>) {
    let items: Vec<String> = {
        let m = app.model.borrow();
        m.local_isos
            .iter()
            .map(|f| {
                format!(
                    "{} — {} — {}",
                    f.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    sys::format_gb(f.size),
                    f.path.display()
                )
            })
            .collect()
    };
    app.lst_iso.set_collection(items);
    if let Some(p) = select {
        let pos = app.model.borrow().local_isos.iter().position(|f| f.path == p);
        if let Some(idx) = pos {
            app.lst_iso.set_selection(Some(idx));
        }
    }
}

fn start_edition_load(app: &Rc<App>, iso: PathBuf) {
    {
        let mut m = app.model.borrow_mut();
        if m.busy_ed || m.running {
            return;
        }
        if m.mounted_iso.as_deref() == Some(iso.as_path()) && !m.editions.is_empty() {
            return; // đã đọc rồi
        }
        m.busy_ed = true;
        m.loading_iso = Some(iso.clone());
    }
    app.lbl_ed_info.set_text("Đang mount ISO và đọc danh sách phiên bản...");
    push_log(
        &app.shared,
        &app.notice.sender(),
        &format!("Đang mount + đọc phiên bản trong ISO: {}", iso.display()),
    );
    let prev = {
        let m = app.model.borrow();
        match (&m.mounted_iso, &m.mounted_letter) {
            (Some(p), Some(l)) => Some((p.clone(), *l)),
            _ => None,
        }
    };
    spawn_load_editions(app.shared.clone(), app.notice.sender(), iso, prev);
}

fn on_editions(app: &Rc<App>, result: Result<(char, Vec<EditionInfo>), String>) {
    {
        let mut m = app.model.borrow_mut();
        m.busy_ed = false;
    }
    match result {
        Ok((letter, eds)) => {
            let default_idx = {
                let m = app.model.borrow();
                m.preflight
                    .as_ref()
                    .map(|pf| default_edition_index(&eds, &pf.os.edition_id))
                    .unwrap_or(0)
            };
            let image_name = find_image_file(letter).map(|(_, n)| n);
            {
                let mut m = app.model.borrow_mut();
                m.mounted_letter = Some(letter);
                m.mounted_iso = m.loading_iso.take();
                m.image_name = image_name;
                m.editions = eds;
            }
            let items: Vec<String> = {
                let m = app.model.borrow();
                m.editions
                    .iter()
                    .map(|e| {
                        format!(
                            "{}. {} — {} — build {} — {} — {}",
                            e.index,
                            e.name,
                            e.edition,
                            e.build,
                            e.arch,
                            sys::format_gb(e.size_bytes)
                        )
                    })
                    .collect()
            };
            app.lst_ed.set_collection(items);
            if !app.model.borrow().editions.is_empty() {
                app.lst_ed.set_selection(Some(default_idx));
            }
            update_ed_info(app);
            push_log(
                &app.shared,
                &app.notice.sender(),
                &format!("Đã đọc xong danh sách phiên bản (ISO mount tại {}:).", letter),
            );
        }
        Err(e) => {
            {
                let mut m = app.model.borrow_mut();
                m.loading_iso = None;
                m.mounted_iso = None;
                m.mounted_letter = None;
                m.image_name = None;
                m.editions = Vec::new();
            }
            app.lst_ed.set_collection(Vec::new());
            app.lbl_ed_info.set_text("Đọc ISO thất bại.");
            error_box("Đọc ISO", &e);
        }
    }
    update_start_enabled(app);
}

fn update_ed_info(app: &Rc<App>) {
    let m = app.model.borrow();
    if let (Some(pf), Some(sel)) = (&m.preflight, app.lst_ed.selection()) {
        if let Some(e) = m.editions.get(sel) {
            let inplace_ok = e.build > 0 && e.build >= pf.os.build;
            app.lbl_ed_info.set_text(&format!(
                "Đã chọn: {}\nEdition: {} — Build ISO: {} (hiện tại: {})\nCài đè: {}",
                e.name,
                e.edition,
                e.build,
                pf.os.build,
                if inplace_ok {
                    "Được phép"
                } else {
                    "KHÔNG được phép (build ISO thấp hơn) — chỉ cài mới"
                }
            ));
        }
    }
    drop(m);
    update_start_enabled(app);
}

fn update_start_enabled(app: &Rc<App>) {
    let m = app.model.borrow();
    let ready = m.preflight.is_some() && !m.editions.is_empty() && !m.running;
    app.btn_start.set_enabled(ready);
}

/// Handler notice (tiêu thụ kết quả luồng nền, gồm `exec_done`).
/// Bọc `catch_unwind` toàn thân (v0.7.4): handler này chạy trong
/// NWG wndproc — panic thoát ra ngoài sẽ unwind qua biên FFI và
/// abort tiến trình im lặng. Khi bắt được panic: ghi một dòng vào
/// vùng log (ghi thẳng vào Shared, KHÔNG bắn notice mới để tránh
/// tái nhập đệ quy) — hộp đen panic đã ghi chi tiết kèm vị trí.
fn refresh(app: &Rc<App>) {
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| refresh_inner(app)));
    if let Err(payload) = res {
        let mut s = lock_shared(&app.shared);
        s.log_lines.push_back(format!(
            "Lỗi nội bộ khi cập nhật giao diện ({}). Chi tiết trong CaiDatWinTuDong-panic.log.",
            panic_payload_text(&*payload)
        ));
        while s.log_lines.len() > 400 {
            s.log_lines.pop_front();
        }
    }
}

fn refresh_inner(app: &Rc<App>) {
    render_log(app);
    let (pf, iso_list, catalogue, dl_prog, dl_done, editions, exec_prog, exec_msg, exec_done, notify_done) = {
        let mut s = lock_shared(&app.shared);
        (
            s.preflight.take(),
            s.iso_list.take(),
            s.catalogue.take(),
            s.dl_progress.take(),
            s.dl_done.take(),
            s.editions.take(),
            s.exec_progress.take(),
            s.exec_msg.take(),
            s.exec_done.take(),
            s.notify_done.take(),
        )
    };
    if let Some(pf) = pf {
        on_preflight(app, pf);
    }
    if let Some(list) = iso_list {
        let keep = app.model.borrow().mounted_iso.clone();
        {
            let mut m = app.model.borrow_mut();
            // Giữ lại ISO đang mount/đang chọn nếu lần quét mới không có
            let mut merged = list;
            if let Some(k) = &keep {
                if !merged.iter().any(|f| &f.path == k) {
                    if let Ok(md) = std::fs::metadata(k) {
                        merged.push(IsoFile { path: k.clone(), size: md.len() });
                    }
                }
            }
            m.local_isos = merged;
        }
        refill_iso_list(app, keep.as_deref());
        push_log(&app.shared, &app.notice.sender(), "Đã quét xong danh sách ISO local.");
        render_log(app);
    }
    if let Some(cat) = catalogue {
        match cat {
            Ok(list) => {
                let items: Vec<String> = list
                    .iter()
                    .map(|i| {
                        format!(
                            "{} — {} — {}",
                            i.name,
                            i.edition,
                            if i.size > 0 { sys::format_gb(i.size) } else { "? GB".to_string() }
                        )
                    })
                    .collect();
                app.lst_cat.set_collection(items);
                app.model.borrow_mut().catalogue = list;
            }
            Err(e) => error_box("Kho online", &e),
        }
    }
    if let Some((done, total)) = dl_prog {
        if total > 0 {
            app.bar_dl.set_pos((done.saturating_mul(100) / total) as u32);
            app.lbl_dl.set_text(&format!(
                "Đang tải/kiểm hash: {} / {}",
                sys::format_gb(done),
                sys::format_gb(total)
            ));
        }
    }
    if let Some(r) = dl_done {
        app.btn_download.set_enabled(true);
        match r {
            Ok(path) => {
                app.bar_dl.set_pos(100);
                app.lbl_dl.set_text("Đã tải xong và kiểm SHA-256 OK.");
                push_log(
                    &app.shared,
                    &app.notice.sender(),
                    &format!("Đã tải ISO: {}", path.display()),
                );
                render_log(app);
                let exists = app.model.borrow().local_isos.iter().any(|f| f.path == path);
                if !exists {
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    app.model.borrow_mut().local_isos.push(IsoFile { path: path.clone(), size });
                }
                refill_iso_list(app, Some(&path));
                start_edition_load(app, path);
            }
            Err(e) => {
                app.lbl_dl.set_text("Tải ISO thất bại.");
                error_box("Tải ISO", &e);
            }
        }
    }
    if let Some(r) = editions {
        on_editions(app, r);
    }
    if let Some((pct, label)) = exec_prog {
        app.bar_exec.set_pos(pct);
        app.lbl_exec.set_text(&label);
    }
    if let Some(msg) = exec_msg {
        app.lbl_exec.set_text(&msg);
    }
    if let Some(r) = exec_done {
        {
            let mut m = app.model.borrow_mut();
            m.running = false;
        }
        // Quá trình đã kết thúc (thành công hay lỗi): đưa cửa sổ về
        // vị trí gốc trước khi hiện hộp thoại kết quả.
        restore_window_pos(app);
        app.btn_start.set_enabled(true);
        match r {
            Ok(msg) => {
                let was_clean = app.model.borrow().last_clean;
                if was_clean {
                    if confirm_box("Cài Đặt Windows", &format!("{}\n\nKhởi động lại máy ngay bây giờ?", msg)) {
                        let _ = cmd::run_capture("shutdown", &["/r", "/t", "5"]);
                    }
                } else {
                    info_box("Cài Đặt Windows", &msg);
                }
            }
            Err(e) => error_box("Cài Đặt Windows", &e),
        }
        update_start_enabled(app);
    }
    if let Some(nd) = notify_done {
        // Tab "Thông báo" (v0.7.27): thao tác mạng đã kết thúc —
        // nhả khoá bốn nút TRƯỚC khi hiện kết quả, ở mọi nhánh.
        set_notify_busy(app, false);
        match nd {
            NotifyDone::ChatId(Ok((id, name))) => {
                app.inp_tg_chat.set_text(&id);
                app.lbl_notify_status.set_text(&format!(
                    "Đã lấy Chat ID {} ({}). Bấm \"Lưu cấu hình thông báo\" để ghi lại.",
                    id, name
                ));
                push_log(
                    &app.shared,
                    &app.notice.sender(),
                    &format!("Đã lấy Chat ID Telegram: {} ({}).", id, name),
                );
                render_log(app);
            }
            NotifyDone::ChatId(Err(e)) => {
                app.lbl_notify_status.set_text(&format!("Lấy Chat ID thất bại: {}", e));
            }
            NotifyDone::Test(channel, Ok(())) => {
                app.lbl_notify_status.set_text(&format!(
                    "Đã gửi tin thử qua {} thành công — kiểm tra điện thoại của bạn.",
                    channel
                ));
                push_log(
                    &app.shared,
                    &app.notice.sender(),
                    &format!("Đã gửi tin thử thông báo qua {}.", channel),
                );
                render_log(app);
            }
            NotifyDone::Test(channel, Err(e)) => {
                app.lbl_notify_status.set_text(&format!(
                    "Gửi thử qua {} thất bại: {}",
                    channel, e
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tab "Thông báo" (v0.7.27): đọc ô nhập, khoá/nhả nút, các handler
// ---------------------------------------------------------------------------
/// Đọc 5 giá trị cấu hình ở tab "Thông báo": (bật?, Bot Token,
/// Chat ID, Relay URL, Relay Secret) — chuỗi đã cắt khoảng trắng.
fn notify_inputs(app: &Rc<App>) -> (bool, String, String, String, String) {
    (
        app.chk_notify.check_state() == nwg::CheckBoxState::Checked,
        app.inp_tg_token.text().trim().to_string(),
        app.inp_tg_chat.text().trim().to_string(),
        app.inp_zalo_url.text().trim().to_string(),
        app.inp_zalo_secret.text().trim().to_string(),
    )
}

/// Khoá/nhả đồng loạt bốn nút thao tác của tab "Thông báo" trong
/// lúc một thao tác mạng đang chạy nền. Nhả khoá do `refresh`
/// gọi khi kết quả về (mọi lối ra của luồng nền đều ghi kết quả,
/// kể cả panic — xem các `spawn_notify_*`).
fn set_notify_busy(app: &Rc<App>, busy: bool) {
    app.model.borrow_mut().notify_busy = busy;
    let enabled = !busy;
    app.btn_tg_fetch.set_enabled(enabled);
    app.btn_tg_test.set_enabled(enabled);
    app.btn_zalo_test.set_enabled(enabled);
    app.btn_notify_save.set_enabled(enabled);
}

fn on_notify_fetch_chat_id(app: &Rc<App>) {
    if app.model.borrow().notify_busy {
        return;
    }
    let (_, token, _, _, _) = notify_inputs(app);
    if token.is_empty() {
        app.lbl_notify_status
            .set_text("Hãy nhập Bot Token trước, nhắn 1 tin cho bot rồi bấm \"Lấy Chat ID\".");
        return;
    }
    set_notify_busy(app, true);
    app.lbl_notify_status.set_text("Đang lấy Chat ID từ Telegram...");
    spawn_notify_fetch_chat_id(app.shared.clone(), app.notice.sender(), token);
}

fn on_notify_test_telegram(app: &Rc<App>) {
    if app.model.borrow().notify_busy {
        return;
    }
    let (_, token, chat_id, _, _) = notify_inputs(app);
    if token.is_empty() || chat_id.is_empty() {
        app.lbl_notify_status
            .set_text("Cần có cả Bot Token và Chat ID mới gửi thử Telegram được.");
        return;
    }
    set_notify_busy(app, true);
    app.lbl_notify_status.set_text("Đang gửi tin thử qua Telegram...");
    spawn_notify_test_telegram(app.shared.clone(), app.notice.sender(), token, chat_id);
}

fn on_notify_test_zalo(app: &Rc<App>) {
    if app.model.borrow().notify_busy {
        return;
    }
    let (_, _, _, url, secret) = notify_inputs(app);
    if url.is_empty() || secret.is_empty() {
        app.lbl_notify_status
            .set_text("Cần có cả Relay URL và Relay Secret mới gửi thử Zalo được.");
        return;
    }
    set_notify_busy(app, true);
    app.lbl_notify_status.set_text("Đang gửi tin thử qua relay Zalo...");
    spawn_notify_test_zalo(app.shared.clone(), app.notice.sender(), url, secret);
}

/// "Lưu cấu hình thông báo": ghi 5 khoá vào settings.json (hàm
/// dùng chung trong qsu_common::settings — sửa đúng vị trí khoá,
/// giữ nguyên các khoá khác) ĐỒNG THỜI cập nhật bản settings
/// trong bộ nhớ, để ca staging bắt đầu ngay sau đó dùng đúng
/// giá trị vừa lưu mà không cần mở lại wizard.
fn on_notify_save(app: &Rc<App>) {
    if app.model.borrow().notify_busy {
        return;
    }
    let (enabled, tg_token, tg_chat, z_url, z_secret) = notify_inputs(app);
    let exe_dir = app.model.borrow().exe_dir.clone();
    match qsu_common::settings::persist_notify_keys(
        &exe_dir, enabled, &tg_token, &tg_chat, &z_url, &z_secret,
    ) {
        Ok(()) => {
            {
                let mut m = app.model.borrow_mut();
                m.settings.notify_enabled = enabled;
                m.settings.telegram_bot_token = tg_token;
                m.settings.telegram_chat_id = tg_chat;
                m.settings.zalo_relay_url = z_url;
                m.settings.zalo_relay_secret = z_secret;
            }
            app.lbl_notify_status.set_text("Đã lưu cấu hình thông báo vào settings.json.");
            push_log(
                &app.shared,
                &app.notice.sender(),
                "Đã lưu cấu hình thông báo (tab Thông báo) vào settings.json.",
            );
        }
        Err(e) => {
            app.lbl_notify_status.set_text(&format!("Lưu cấu hình thất bại: {}", e));
        }
    }
}

// ---------------------------------------------------------------------------
// Sự kiện giao diện
// ---------------------------------------------------------------------------
fn handle_event(app: &Rc<App>, evt: nwg::Event, handle: nwg::ControlHandle) {
    match evt {
        nwg::Event::OnNotice => {
            if handle == app.notice.handle {
                refresh(app);
            }
        }
        nwg::Event::OnResize | nwg::Event::OnWindowMaximize => {
            // v0.7.9: cửa sổ chính đổi kích thước (kéo giãn tay, tối
            // đa hoá, hoặc khôi phục vị trí sau khi cài đặt) -> tính
            // lại bố cục thích ứng. NWG tách OnWindowMaximize khỏi
            // OnResize nên phải nghe cả hai. Sự kiện của control con
            // (do chính apply_layout di chuyển) bị bỏ qua nhờ đối
            // chiếu HWND cửa sổ chính.
            if handle.hwnd() == main_window_hwnd(app) {
                layout_window(app);
            }
        }
        nwg::Event::OnButtonClick => {
            if handle == app.btn_close.handle {
                nwg::stop_thread_dispatch();
            } else if handle == app.btn_rescan.handle {
                push_log(&app.shared, &app.notice.sender(), "Đang quét lại hệ thống...");
                spawn_preflight(app.shared.clone(), app.notice.sender());
            } else if handle == app.btn_browse.handle {
                on_browse(app);
            } else if handle == app.btn_fetch.handle {
                let url = app.model.borrow().settings.catalogue_url.clone();
                push_log(&app.shared, &app.notice.sender(), "Đang tải danh mục ISO từ kho online...");
                spawn_catalogue_fetch(app.shared.clone(), app.notice.sender(), url);
            } else if handle == app.btn_download.handle {
                on_download(app);
            } else if handle == app.btn_mode_inplace.handle {
                set_mode(app, false);
            } else if handle == app.btn_mode_clean.handle {
                set_mode(app, true);
            } else if handle == app.btn_part_preserve.handle {
                set_partition_mode(app, PartMode::Preserve);
            } else if handle == app.btn_part_wipe.handle {
                set_partition_mode(app, PartMode::WipeDisk);
            } else if handle == app.btn_part_rebuild.handle {
                set_partition_mode(app, PartMode::Rebuild);
            } else if handle == app.btn_tweaks.handle {
                let on = {
                    let mut m = app.model.borrow_mut();
                    m.apply_tweaks = !m.apply_tweaks;
                    m.apply_tweaks
                };
                app.btn_tweaks
                    .set_text(if on { "Tuỳ chỉnh Windows sau cài: BẬT" } else { "Tuỳ chỉnh Windows sau cài: TẮT" });
            } else if handle == app.btn_core.handle {
                let on = {
                    let mut m = app.model.borrow_mut();
                    m.core_profile = !m.core_profile;
                    // v0.7.24: lưu trạng thái nút vào settings.json
                    // để các lần mở wizard sau vẫn giữ — ca test
                    // không còn phụ thuộc vào việc nhớ bật lại nút.
                    persist_core_default(&m.exe_dir, m.core_profile);
                    m.core_profile
                };
                app.btn_core
                    .set_text(if on { "Ảnh rút gọn (Tiny11 Core): BẬT" } else { "Ảnh rút gọn (Tiny11 Core): TẮT" });
            } else if handle == app.btn_start.handle {
                on_start(app);
            } else if handle == app.btn_tg_fetch.handle {
                on_notify_fetch_chat_id(app);
            } else if handle == app.btn_tg_test.handle {
                on_notify_test_telegram(app);
            } else if handle == app.btn_zalo_test.handle {
                on_notify_test_zalo(app);
            } else if handle == app.btn_notify_save.handle {
                on_notify_save(app);
            }
        }
        nwg::Event::OnListViewItemChanged => {
            if handle == app.lst_disk_parts.handle {
                // Chế độ rebuild: ép các tick bất hợp lệ về đúng
                // luật (Windows cũ luôn tick; staging/data không
                // bao giờ được tick).
                enforce_rebuild_checks(app);
            }
        }
        nwg::Event::OnListBoxSelect => {
            if handle == app.lst_iso.handle {
                if let Some(sel) = app.lst_iso.selection() {
                    let iso = app.model.borrow().local_isos.get(sel).map(|f| f.path.clone());
                    if let Some(iso) = iso {
                        start_edition_load(app, iso);
                    }
                }
            } else if handle == app.lst_ed.handle {
                update_ed_info(app);
            }
        }
        nwg::Event::OnTextInput => {
            // Phản hồi khớp serial TRỰC TIẾP theo từng ký tự gõ
            // (v0.7.17) — xem `update_serial_feedback`.
            if handle == app.inp_disk_serial.handle {
                update_serial_feedback(app);
            }
        }
        nwg::Event::OnComboBoxClosed => {
            if handle == app.cmb_disk.handle {
                update_disk_parts(app);
            } else if handle == app.cmb_data.handle {
                if let Some(idx) = app.cmb_data.selection() {
                    let letter = {
                        let m = app.model.borrow();
                        m.preflight
                            .as_ref()
                            .and_then(|pf| pf.non_os_drives().get(idx).map(|d| d.letter))
                    };
                    if let Some(l) = letter {
                        app.model.borrow_mut().data_letter = Some(l);
                        update_data_info(app);
                        // Ổ Data đổi -> tính hợp lệ của từng đĩa đích
                        // wipe đổi theo (đĩa chứa ổ Data mới bị cấm):
                        // đổ lại cả danh sách đĩa kèm lý do.
                        refresh_disk_picker(app);
                    }
                }
            }
        }
        _ => {}
    }
}

fn set_mode(app: &Rc<App>, clean: bool) {
    {
        let mut m = app.model.borrow_mut();
        m.mode_clean = clean;
    }
    app.btn_mode_inplace.set_text(if clean {
        "○ Cài đè (giữ ứng dụng + dữ liệu)"
    } else {
        "● Cài đè (giữ ứng dụng + dữ liệu)"
    });
    app.btn_mode_clean.set_text(if clean {
        "● Cài mới từ xa (qua WinPE)"
    } else {
        "○ Cài mới từ xa (qua WinPE)"
    });
    update_part_ui(app);
}

/// Chọn chế độ phân vùng khi cài mới: giữ nguyên (mặc định),
/// wipeDisk (xoá toàn bộ ổ đĩa đích, chia lại từ đầu) hoặc rebuild
/// (chỉ xoá phân vùng hệ thống trên đĩa đích, giữ phân vùng data).
fn set_partition_mode(app: &Rc<App>, mode: PartMode) {
    {
        let mut m = app.model.borrow_mut();
        m.part_mode = mode;
    }
    app.btn_part_preserve.set_text(if mode == PartMode::Preserve {
        "● Giữ nguyên phân vùng (mặc định)"
    } else {
        "○ Giữ nguyên phân vùng (mặc định)"
    });
    app.btn_part_wipe.set_text(if mode == PartMode::WipeDisk {
        "● Xoá toàn bộ ổ đĩa, chia lại"
    } else {
        "○ Xoá toàn bộ ổ đĩa, chia lại"
    });
    app.btn_part_rebuild.set_text(if mode == PartMode::Rebuild {
        "● Cài lại phân vùng hệ thống (giữ data)"
    } else {
        "○ Cài lại phân vùng hệ thống (giữ data)"
    });
    update_part_ui(app);
    if mode != PartMode::Preserve {
        // Mỗi lần VÀO chế độ phá huỷ phân vùng đều đổ lại danh sách
        // đĩa (tính hợp lệ phụ thuộc ổ Data đang chọn và chính chế
        // độ — có thể đã đổi từ lần trước).
        refresh_disk_picker(app);
    }
}

fn on_browse(app: &Rc<App>) {
    let mut dlg = Default::default();
    let built = nwg::FileDialog::builder()
        .title("Chọn file ISO Windows")
        .action(nwg::FileDialogAction::Open)
        .filters("Ảnh ISO (*.iso)|*.iso")
        .build(&mut dlg);
    if built.is_err() {
        return;
    }
    // Cửa sổ cha = cửa sổ chính (lấy qua GetParent của một control con)
    let parent = app.btn_close.handle.hwnd().and_then(|h| {
        let p = unsafe { winapi::um::winuser::GetParent(h) };
        if p.is_null() {
            None
        } else {
            Some(nwg::ControlHandle::Hwnd(p))
        }
    });
    if dlg.run(parent) {
        if let Ok(p) = dlg.get_selected_item() {
            let path = PathBuf::from(p);
            let exists = app.model.borrow().local_isos.iter().any(|f| f.path == path);
            if !exists {
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                app.model.borrow_mut().local_isos.push(IsoFile { path: path.clone(), size });
            }
            refill_iso_list(app, Some(&path));
            start_edition_load(app, path);
        }
    }
}

fn on_download(app: &Rc<App>) {
    let sel = match app.lst_cat.selection() {
        Some(s) => s,
        None => {
            info_box("Tải ISO", "Hãy tải danh mục và chọn một bản Windows trong danh sách kho online trước.");
            return;
        }
    };
    let (img, dest_dir) = {
        let m = app.model.borrow();
        let img = match m.catalogue.get(sel) {
            Some(i) => i.clone(),
            None => return,
        };
        let dest_dir = match m.data_letter {
            Some(l) => PathBuf::from(format!("{}:\\", l)).join(&m.settings.download_folder),
            None => {
                error_box("Tải ISO", "Chưa xác định được ổ Data để chứa ISO tải về.");
                return;
            }
        };
        (img, dest_dir)
    };
    app.btn_download.set_enabled(false);
    app.bar_dl.set_pos(0);
    app.lbl_dl.set_text("Đang tải...");
    push_log(
        &app.shared,
        &app.notice.sender(),
        &format!("Bắt đầu tải ISO '{}' vào {}...", img.name, dest_dir.display()),
    );
    spawn_download(app.shared.clone(), app.notice.sender(), img, dest_dir);
}

fn valid_computer_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 15
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !name.chars().all(|c| c.is_ascii_digit())
}

/// Điểm vào của nút Bắt đầu. Toàn bộ thân handler chạy trong
/// `catch_unwind` (v0.7.4): handler chạy trên luồng giao diện, bên
/// trong NWG wndproc (`extern "system"`) — MỘT panic thoát ra
/// ngoài sẽ unwind qua biên FFI và abort tiến trình ngay lập tức,
/// không hộp thoại, không log (đúng ca chết im lặng sau cú bấm Yes
/// ngày 2026-11-17). Khi bắt được panic: ghi vào install log (nếu
/// logger đã kịp tạo), khôi phục trạng thái giao diện như đường
/// lỗi spawn, và hiện hộp thoại lỗi đọc được. Unwind đã hoàn tất
/// khi catch chạy nên mọi borrow của khung hình bị panic đều đã
/// được thả — các try_borrow ở đây chỉ là phòng vệ thêm.
fn on_start(app: &Rc<App>) {
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_start_inner(app)));
    if let Err(payload) = res {
        let msg = panic_payload_text(&*payload);
        if let Ok(m) = app.model.try_borrow() {
            if let Some(lg) = &m.logger {
                lg.line(&format!("PANIC trong on_start: {}", msg));
            }
        }
        if let Ok(mut m) = app.model.try_borrow_mut() {
            m.running = false;
        }
        restore_window_pos(app);
        app.btn_start.set_enabled(true);
        error_box(
            "Cài Đặt Windows",
            &format!(
                "Lỗi nội bộ bất ngờ ({}). Quá trình cài đặt chưa bắt đầu — chi tiết nằm trong CaiDatWinTuDong-panic.log (cạnh file wizard và trên ổ Data) cùng file log trong thư mục QSU-Logs trên ổ Data.",
                msg
            ),
        );
    }
}

fn on_start_inner(app: &Rc<App>) {
    // --- Thu thập + kiểm tra (đặc tả §2/§4/§8) ---
    let plan = {
        let m = app.model.borrow();
        if m.running {
            return;
        }
        let pf = match &m.preflight {
            Some(p) => p,
            None => {
                error_box("Cài Đặt Windows", "Chưa quét xong hệ thống — hãy đợi hoặc bấm Quét lại.");
                return;
            }
        };
        if pf.bitlocker == BitlockerState::InProgress {
            error_box("Cài Đặt Windows", "BitLocker đang mã hoá/giải mã dở dang. Hãy đợi quá trình đó hoàn tất rồi mới cài đặt.");
            return;
        }
        if pf.power.has_battery && !pf.power.ac_online && pf.power.percent < 60 && pf.power.percent != 255 {
            error_box("Cài Đặt Windows", "Máy đang chạy pin dưới 60%. Hãy cắm sạc trước khi cài đặt.");
            return;
        }
        let sel = match app.lst_ed.selection() {
            Some(s) => s,
            None => {
                error_box("Cài Đặt Windows", "Hãy chọn một phiên bản Windows trong danh sách (mục 3).");
                return;
            }
        };
        let edition = match m.editions.get(sel) {
            Some(e) => e.clone(),
            None => return,
        };
        let (iso, mounted_letter, image_name) = match (&m.mounted_iso, &m.mounted_letter, &m.image_name) {
            (Some(i), Some(l), Some(n)) => (i.clone(), *l, n.clone()),
            _ => {
                error_box("Cài Đặt Windows", "ISO chưa được mount/đọc xong. Hãy chọn lại ISO.");
                return;
            }
        };
        let need_gb = if m.mode_clean {
            m.settings.min_free_gb_clean
        } else {
            m.settings.min_free_gb_inplace
        };
        // wipeDisk/rebuild chia lại phân vùng nên dung lượng trống
        // của phân vùng OS hiện tại không còn ý nghĩa — thay bằng
        // kiểm tra cỡ đĩa/vùng ở khối kiểm tra đích bên dưới.
        let wipe = m.mode_clean && m.part_mode == PartMode::WipeDisk;
        let rebuild = m.mode_clean && m.part_mode == PartMode::Rebuild;
        if !wipe && !rebuild && pf.os_free() < need_gb * GB {
            error_box(
                "Cài Đặt Windows",
                &format!(
                    "Ổ OS chỉ còn trống {} — cần tối thiểu {} GB cho chế độ này.",
                    sys::format_gb(pf.os_free()),
                    need_gb
                ),
            );
            return;
        }
        if !m.mode_clean && (edition.build == 0 || edition.build < pf.os.build) {
            error_box(
                "Cài Đặt Windows",
                &format!(
                    "Không thể cài đè: build của ISO ({}) thấp hơn build hiện tại ({}). Hãy chọn ISO mới hơn hoặc chuyển sang Cài mới.",
                    edition.build, pf.os.build
                ),
            );
            return;
        }
        let computer_name = app.inp_computer.text().trim().to_string();
        let admin_user = app.inp_user.text().trim().to_string();
        let pw1 = app.inp_pw1.text();
        let pw2 = app.inp_pw2.text();
        let mut data_letter = m.data_letter;
        // Đích wipeDisk/rebuild (điền khi wipe/rebuild = true):
        // số đĩa + serial phần cứng + danh sách phân vùng sẽ xoá
        let mut target_disk_number: u32 = 0;
        let mut target_disk_serial = String::new();
        let mut target_disk_summary = String::new();
        let mut target_disk_parts = String::new();
        // Rebuild: các mục ghi vào stage.json + mô tả phân vùng giữ lại
        let mut rebuild_parts: Vec<RebuildPart> = Vec::new();
        let mut rebuild_kept = String::new();
        if m.mode_clean {
            if data_letter.is_none() {
                error_box("Cài Đặt Windows", "Cài mới từ xa bắt buộc phải có ổ Data (chứa staging + log).");
                return;
            }
            if data_letter == Some(pf.os_letter) {
                error_box("Cài Đặt Windows", "Ổ Data đang chọn trùng với ổ cài Windows (OS). Hãy chọn một ổ đĩa khác làm ổ Data — ổ OS sẽ bị format khi cài mới.");
                return;
            }
            if !valid_computer_name(&computer_name) {
                error_box("Cài Đặt Windows", "Tên máy tính không hợp lệ (1–15 ký tự chữ/số/gạch ngang, không toàn số).");
                return;
            }
            if admin_user.is_empty() {
                error_box("Cài Đặt Windows", "Hãy nhập tên tài khoản admin cho bản Windows mới.");
                return;
            }
            // Mật khẩu TUỲ CHỌN (v0.7.2): cả hai ô trống = hợp lệ
            // (tài khoản không mật khẩu — kỹ thuật viên cài hàng
            // loạt, người dùng cuối tự đặt sau). Chỉ chặn khi hai ô
            // lệch nhau (một ô trống/một ô có, hoặc gõ sai ô nhập lại).
            if pw1 != pw2 {
                error_box("Cài Đặt Windows", "Hai ô mật khẩu không khớp — để trống cả hai ô nếu không muốn đặt mật khẩu cho tài khoản admin.");
                return;
            }
            // Dung lượng ổ Data: cần chứa sources + WinPE + dự phòng
            if let Some(d) = pf.drive(data_letter.unwrap()) {
                let iso_size = std::fs::metadata(&iso).map(|x| x.len()).unwrap_or(0);
                if d.free < iso_size + 2 * GB {
                    error_box(
                        "Cài Đặt Windows",
                        &format!(
                            "Ổ Data {}: chỉ còn trống {} — không đủ chứa bộ cài staging (cần khoảng {}).",
                            d.letter,
                            sys::format_gb(d.free),
                            sys::format_gb(iso_size + 2 * GB)
                        ),
                    );
                    return;
                }
            }
            if wipe || rebuild {
                // --- Kiểm tra đích phá huỷ phân vùng (wipeDisk §6b /
                // rebuild v0.7.13): cùng khung kiểm tra, khác luật ---
                let d = match selected_disk(app) {
                    Some(d) => d,
                    None => {
                        error_box("Cài Đặt Windows", "Chế độ chia lại phân vùng: hãy chọn một ổ đĩa đích trong danh sách đĩa vật lý.");
                        return;
                    }
                };
                if d.serial.trim().is_empty() {
                    error_box("Cài Đặt Windows", "Không đọc được serial phần cứng của đĩa đích — không thể xác minh đích an toàn. Hãy chọn đĩa khác.");
                    return;
                }
                if wipe {
                    if d.size < disk::MIN_WIPE_DISK_BYTES {
                        error_box(
                            "Cài Đặt Windows",
                            &format!(
                                "Đĩa đích {} chỉ có {} — quá nhỏ để chia lại (cần tối thiểu {}).",
                                d.number,
                                sys::format_gb(d.size),
                                sys::format_gb(disk::MIN_WIPE_DISK_BYTES)
                            ),
                        );
                        return;
                    }
                    // Chốt cứng: đĩa chứa ổ Data (staging + log) KHÔNG
                    // BAO GIỜ được làm đích wipe — trùng là từ chối
                    // tuyệt đối. (Rebuild KHÔNG có chốt này: trùng đĩa
                    // là mục đích của nó, phân vùng staging được bảo
                    // vệ ở tầng phân loại bên dưới + tầng engine.)
                    if d.contains_letter(data_letter.unwrap()) {
                        error_box(
                            "Cài Đặt Windows",
                            &format!(
                                "Đĩa đích (đĩa {}) đang CHỨA chính ổ Data {}: — nơi chứa staging và log.\nXoá đĩa này sẽ phá huỷ cả bộ staging đang dùng để cài. Hãy chọn một đĩa khác, hoặc chuyển ổ Data sang đĩa khác trước.",
                                d.number,
                                data_letter.unwrap()
                            ),
                        );
                        return;
                    }
                }
                // Cổng chung cuối: mọi lý do cấm đang hiển thị trên
                // giao diện đều chặn lại ở đây, theo đúng chế độ —
                // wipe gồm cả đối chiếu vị trí ổ Data bằng đường
                // extents (độc lập với bảng phân vùng, phòng khi
                // bảng phân vùng đọc thiếu); rebuild cấm đĩa liệt kê
                // dự phòng.
                {
                    let data_disk_num = disk::volume_disk_number(data_letter.unwrap());
                    let mode_now = if wipe { PartMode::WipeDisk } else { PartMode::Rebuild };
                    if let Some(r) = disk_block_reason(&d, data_letter, data_disk_num, mode_now) {
                        error_box(
                            "Cài Đặt Windows",
                            &format!("Đĩa đích không hợp lệ: {}", r),
                        );
                        return;
                    }
                }
                // Xác nhận bằng serial gõ tay (chốt cuối phía người dùng)
                let typed = app.inp_disk_serial.text();
                if disk::normalize_disk_serial(&typed) != disk::normalize_disk_serial(&d.serial) {
                    error_box(
                        "Cài Đặt Windows",
                        &format!(
                            "Serial bạn nhập không khớp serial của đĩa đích đã chọn ({}).\nHãy nhìn cột serial trong danh sách đĩa và nhập lại chính xác.",
                            d.serial
                        ),
                    );
                    return;
                }
                target_disk_number = d.number;
                target_disk_serial = d.serial.clone();
                target_disk_summary = d.summary();
                if wipe {
                    target_disk_parts = d.describe_partitions();
                } else {
                    // --- Rebuild: chốt danh sách phân vùng sẽ xoá từ
                    // trạng thái tick THẬT trên ListView (hàng i ứng
                    // với d.partitions[i] — cùng thứ tự lúc vẽ). ---
                    let os_serial = pf.drive(pf.os_letter).map(|x| x.serial as u64).unwrap_or(0);
                    let data_serial = pf
                        .drive(data_letter.unwrap())
                        .map(|x| x.serial as u64)
                        .unwrap_or(0);
                    let mut entries: Vec<RebuildPart> = Vec::new();
                    let mut del_lines: Vec<String> = Vec::new();
                    let mut kept_lines: Vec<String> = Vec::new();
                    let mut region: u64 = 0;
                    let mut win_found = false;
                    for (i, p) in d.partitions.iter().enumerate() {
                        let class = classify_partition(
                            p,
                            pf.os_letter,
                            os_serial,
                            data_letter,
                            data_serial,
                        );
                        if class == RebuildClass::Windows {
                            win_found = true;
                        }
                        let checked = lv_item_checked(&app.lst_disk_parts, i)
                            || class == RebuildClass::Windows;
                        if checked {
                            // Kiểm lại ở tầng xác nhận: phân vùng
                            // staging/data KHÔNG BAO GIỜ được vào
                            // danh sách xoá, kể cả khi UI có lỗi.
                            if class == RebuildClass::Staging || class == RebuildClass::Data {
                                error_box(
                                    "Cài Đặt Windows",
                                    &format!(
                                        "Phân vùng {} được đánh dấu xoá nhưng thuộc nhóm được bảo vệ/giữ nguyên — dừng lại để an toàn. Hãy mở lại danh sách phân vùng và kiểm tra.",
                                        p.describe()
                                    ),
                                );
                                return;
                            }
                            region = region.saturating_add(p.size);
                            entries.push(RebuildPart {
                                kind: rebuild_kind_str(p, class).to_string(),
                                size_bytes: p.size,
                                gpt_guid: p.part_guid.clone(),
                                offset_bytes: p.offset,
                            });
                            del_lines.push(format!("  - {}", p.describe()));
                        } else {
                            kept_lines.push(format!("  - {}", p.describe()));
                        }
                    }
                    if !win_found {
                        error_box("Cài Đặt Windows", "Không xác định được phân vùng chứa Windows hiện tại trên đĩa đích — không có chỗ cài mới an toàn. Hãy kiểm tra lại danh sách phân vùng.");
                        return;
                    }
                    if entries.is_empty() {
                        error_box("Cài Đặt Windows", "Chưa có phân vùng hệ thống nào được chọn để xoá. Hãy tick các phân vùng hệ thống (EFI/MSR/Windows cũ/Recovery) trong danh sách.");
                        return;
                    }
                    // Vùng trống sau xoá phải đủ cho layout mới:
                    // boot (+MSR) + Recovery 1024MB + Windows ≥ 10GB —
                    // cùng ngưỡng engine sẽ kiểm lại trong WinPE.
                    let min_region = (if pf.uefi { 1784 } else { 1124 }) * 1_048_576 + 10 * GB;
                    if region < min_region {
                        error_box(
                            "Cài Đặt Windows",
                            &format!(
                                "Tổng dung lượng các phân vùng hệ thống đã chọn ({}) nhỏ hơn mức tối thiểu {} cho layout mới (boot + Recovery 1024MB + Windows tối thiểu 10GB). Hãy tick thêm phân vùng hệ thống (ví dụ Recovery cũ) hoặc chọn chế độ khác.",
                                sys::format_gb(region),
                                sys::format_gb(min_region)
                            ),
                        );
                        return;
                    }
                    target_disk_parts = del_lines.join("\n");
                    rebuild_kept = if kept_lines.is_empty() {
                        "(không còn phân vùng nào khác trên đĩa)".to_string()
                    } else {
                        kept_lines.join("\n")
                    };
                    rebuild_parts = entries;
                }
            }
        } else if data_letter.is_none() {
            // Cài đè không có ổ Data: log/báo cáo để cạnh wizard
            data_letter = Some(pf.os_letter);
        }
        let data_letter = data_letter.unwrap();
        ExecPlan {
            mode_clean: m.mode_clean,
            iso,
            mounted_letter,
            image_name,
            edition,
            os_letter: pf.os_letter,
            data_letter,
            os_serial: pf.drive(pf.os_letter).map(|d| d.serial as u64).unwrap_or(0),
            os_label: pf.drive(pf.os_letter).map(|d| d.label.clone()).unwrap_or_default(),
            data_serial: pf.drive(data_letter).map(|d| d.serial as u64).unwrap_or(0),
            uefi: pf.uefi,
            firmware_detail: pf.firmware.detail(),
            bitlocker: pf.bitlocker,
            computer_name,
            admin_user,
            admin_password: pw1,
            apply_tweaks: m.apply_tweaks,
            core_profile: m.core_profile,
            part_mode: if wipe {
                PartMode::WipeDisk
            } else if rebuild {
                PartMode::Rebuild
            } else {
                PartMode::Preserve
            },
            target_disk_number,
            target_disk_serial,
            target_disk_summary,
            target_disk_parts,
            rebuild_parts,
            rebuild_kept,
        }
    };

    // --- Hộp xác nhận cuối (đặc tả §8) ---
    // Dòng trạng thái tài khoản admin (chỉ có nghĩa ở cài mới):
    // không đặt mật khẩu là lựa chọn hợp lệ (v0.7.2) và phải được
    // nói rõ trong tóm tắt trước khi kỹ thuật viên xác nhận.
    let account_note = if !plan.mode_clean {
        String::new()
    } else if plan.admin_password.is_empty() {
        format!(
            "\nTài khoản {}: KHÔNG mật khẩu — người dùng tự đặt mật khẩu sau khi cài",
            plan.admin_user
        )
    } else {
        format!("\nTài khoản {}: có đặt mật khẩu (không hiển thị)", plan.admin_user)
    };
    let summary = if plan.part_mode == PartMode::WipeDisk {
        format!(
            "ISO: {}\nPhiên bản: {} (index {})\nChế độ: CÀI MỚI HOÀN TOÀN — XOÁ TOÀN BỘ Ổ ĐĨA ĐÍCH, CHIA LẠI TỪ ĐẦU\n\nĐĩa đích: {}\nCác phân vùng trên đĩa đích:\n{}\n\nTOÀN BỘ DỮ LIỆU TRÊN CÁC PHÂN VÙNG NÀY SẼ BỊ XOÁ VĨNH VIỄN.\nĐĩa sẽ được chia lại: phân vùng boot (EFI/System Reserved) + MSR (nếu UEFI) + Recovery 1024MB + Windows (phần còn lại).\nỔ Data được bảo vệ: {}:{} (nằm trên đĩa khác, chứa staging + log){}\n\nSau khi máy reboot vào WinPE, quá trình KHÔNG huỷ được.",
            plan.iso.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            plan.edition.name,
            plan.edition.index,
            plan.target_disk_summary,
            plan.target_disk_parts,
            plan.data_letter,
            {
                let m = app.model.borrow();
                match (&m.preflight, m.data_letter) {
                    (Some(pf), Some(l)) if pf.marker_drives.contains(&l) => " (đã có marker)",
                    _ => " (sẽ tạo marker khi bắt đầu)",
                }
            },
            account_note,
        )
    } else if plan.part_mode == PartMode::Rebuild {
        format!(
            "ISO: {}\nPhiên bản: {} (index {})\nChế độ: CÀI MỚI — CHỈ XOÁ PHÂN VÙNG HỆ THỐNG TRÊN ĐĨA ĐÍCH, GIỮ NGUYÊN PHÂN VÙNG DATA\n\nĐĩa đích: {}\nCác phân vùng hệ thống sẽ bị XOÁ:\n{}\n\nDỮ LIỆU TRÊN CÁC PHÂN VÙNG NÀY SẼ BỊ XOÁ VĨNH VIỄN.\nCác phân vùng trên đĩa đích được GIỮ NGUYÊN:\n{}\n\nVùng trống sau khi xoá sẽ được chia lại: phân vùng boot (EFI 260MB/System Reserved 100MB){} + Recovery 1024MB + Windows (phần còn lại).\nỔ Data: {}:{} (chứa staging + log — phân vùng của nó KHÔNG nằm trong danh sách xoá){}\n\nSau khi máy reboot vào WinPE, quá trình KHÔNG huỷ được.",
            plan.iso.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            plan.edition.name,
            plan.edition.index,
            plan.target_disk_summary,
            plan.target_disk_parts,
            plan.rebuild_kept,
            if plan.uefi { " + MSR 500MB" } else { "" },
            plan.data_letter,
            {
                let m = app.model.borrow();
                match (&m.preflight, m.data_letter) {
                    (Some(pf), Some(l)) if pf.marker_drives.contains(&l) => " (đã có marker)",
                    _ => " (sẽ tạo marker khi bắt đầu)",
                }
            },
            account_note,
        )
    } else {
        format!(
            "ISO: {}\nPhiên bản: {} (index {})\nChế độ: {}\nỔ OS bị tác động: {}:\nỔ Data được bảo vệ: {}:{}{}\n\n{}",
            plan.iso.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            plan.edition.name,
            plan.edition.index,
            if plan.mode_clean { "Cài mới từ xa (qua WinPE)" } else { "Cài đè (giữ ứng dụng + dữ liệu)" },
            plan.os_letter,
            plan.data_letter,
            {
                let m = app.model.borrow();
                match (&m.preflight, m.data_letter) {
                    (Some(pf), Some(l)) if pf.marker_drives.contains(&l) => " (đã có marker)",
                    _ => " (sẽ tạo marker khi bắt đầu)",
                }
            },
            account_note,
            if plan.mode_clean {
                "CẢNH BÁO: Phân vùng OS sẽ bị FORMAT. Sau khi máy reboot vào WinPE, quá trình KHÔNG huỷ được."
            } else {
                "Máy sẽ tự khởi động lại nhiều lần trong quá trình cài đè."
            }
        )
    };
    let summary = if plan.mode_clean {
        format!("{}\n\nSau đăng nhập Windows mới: tự cài UltraViewer, Chrome và 7-Zip (cần Internet).", summary)
    } else { summary };
    if !confirm_box("Xác nhận cài đặt", &summary) {
        return;
    }

    // --- Logger (tạo lười biếng theo ổ Data đang chọn) ---
    // ⚠️ GỐC LỖI CHẾT IM LẶNG SAU CÚ BẤM YES (v0.7.4 — thực địa
    // 2026-11-17 11:06): bản cũ GIỮ `borrow_mut` của model trong
    // suốt khối này rồi gọi `lg.line(...)`. Sink của logger gọi
    // `NoticeSender::notice()` — NWG 1.0.13 hiện thực bằng
    // `SendNotifyMessageW` (controls/notice.rs), mà với cửa sổ do
    // CHÍNH luồng hiện tại tạo ra thì hàm này dispatch thông điệp
    // ĐỒNG BỘ ngay tại chỗ: `refresh()` chạy tái nhập ngay trong
    // `lg.line`, `render_log` mượn model → panic "RefCell already
    // borrowed" → unwind qua wndproc (biên FFI) → abort tiến
    // trình. Vì Logger ghi file TRƯỚC khi gọi sink nên file log
    // cài đặt chỉ chứa đúng dòng header chẩn đoán — khớp tuyệt
    // đối với bằng chứng thực địa. Cấu trúc dưới đây KHÔNG giữ
    // bất kỳ borrow nào của model trong lúc ghi log: đọc snapshot
    // (logger sẵn có / logs_folder / disk_diag) trong một borrow
    // ngắn, thả borrow, ghi log, rồi cất logger bằng một borrow
    // ngắn khác.
    let logger = {
        let (existing, logs_folder, disk_diag) = {
            let m = app.model.borrow();
            (
                m.logger.clone(),
                m.settings.logs_folder.clone(),
                m.disk_diag.clone(),
            )
        };
        match existing {
            Some(lg) => lg,
            None => {
                let base = PathBuf::from(format!("{}:\\", plan.data_letter));
                set_panic_log_dir(base.join(&logs_folder));
                let lg = make_logger(&base, &logs_folder, &app.shared, app.notice.sender());
                // Mốc bước đã flush từng dòng (Logger::line luôn
                // flush): nếu tiến trình còn chết ở đoạn sau, dòng
                // cuối của file log nêu đúng bước đã qua.
                lg.line("[step] logger ok (đã qua xác nhận)");
                // Chép chẩn đoán liệt kê đĩa lúc quét tiền đề vào
                // file log — cặp screenshot + file log đủ để chẩn
                // đoán từ xa các ca "không thấy đĩa đích".
                // Marker mịn quanh hai dòng đầu của dump (v0.7.4):
                // Logger ghi file TRƯỚC khi gọi sink, nên nếu tiến
                // trình còn chết trong dump, dòng cuối của file cho
                // biết chính xác sink của dòng nào đã giết nó —
                // phân biệt được panic trong sink với các nguyên
                // nhân không để lại hook (vd tràn stack).
                if !disk_diag.is_empty() {
                    lg.line("--- Chẩn đoán liệt kê đĩa lúc quét tiền đề ---");
                    lg.line("[step] diag header ok");
                    for (i, line) in disk_diag.iter().enumerate() {
                        lg.line(&format!("[Đĩa] {}", line));
                        if i == 0 {
                            lg.line("[step] diag dong 1 ok");
                        }
                    }
                    lg.line("--- Hết chẩn đoán liệt kê đĩa ---");
                }
                lg.line("[step] diag ok");
                app.model.borrow_mut().logger = Some(lg.clone());
                lg
            }
        }
    };
    let (settings, exe_dir) = {
        let m = app.model.borrow();
        (m.settings.clone(), m.exe_dir.clone())
    };
    {
        let mut m = app.model.borrow_mut();
        m.running = true;
        m.last_clean = plan.mode_clean;
    }
    app.btn_start.set_enabled(false);
    app.bar_exec.set_pos(0);
    // Phản hồi NGAY trong chính cú bấm Yes — không chờ vòng lặp qua
    // luồng nền: nhãn trạng thái + thanh tiến độ đổi tức thì, và cửa
    // sổ được dịch lên để vùng tiến độ/log ở đáy không bị khuất khỏi
    // màn hình trong suốt quá trình staging (xem shift_window_for_exec).
    app.lbl_exec.set_text("Đã xác nhận — đang bắt đầu quá trình cài đặt...");
    app.bar_exec.set_pos(1);
    shift_window_for_exec(app);
    // Kể cả khi việc tạo luồng nền tự nó lỗi (cạn tài nguyên hệ
    // thống khiến thread::spawn panic), giao diện cũng không được
    // chết âm thầm: bắt panic của chính lời gọi spawn, mở khoá lại
    // giao diện và báo lỗi rõ ràng.
    let logger_for_step = logger.clone();
    let spawn_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        spawn_exec(app.shared.clone(), app.notice.sender(), logger, settings, exe_dir, plan);
    }));
    if let Err(payload) = spawn_res {
        {
            let mut m = app.model.borrow_mut();
            m.running = false;
        }
        restore_window_pos(app);
        app.btn_start.set_enabled(true);
        error_box(
            "Cài Đặt Windows",
            &format!(
                "Không khởi động được quá trình cài đặt ({}). Chưa có thay đổi nào được thực hiện.",
                panic_payload_text(&*payload)
            ),
        );
    } else {
        // Mốc cuối của đoạn sau-xác-nhận trên luồng giao diện
        // (v0.7.4): ghi được dòng này nghĩa là logger + dump chẩn
        // đoán + spawn đều đã qua — từ đây luồng nền tự ghi tiếp
        // tiến độ staging ("=== Bắt đầu quá trình cài đặt ===").
        // Không borrow nào của model đang được giữ tại điểm này
        // nên lời gọi sink/notice trong Logger::line an toàn.
        logger_for_step.line("[step] spawn ok");
    }
}

// ---------------------------------------------------------------------------
fn main() {
    // Hộp đen panic phải cài đầu tiên: mọi panic từ đây về sau (kể cả
    // trong lúc khởi tạo giao diện) đều để lại dấu vết trong file
    // CaiDatWinTuDong-panic.log cạnh file exe thay vì biến mất cùng
    // cửa sổ như mặc định của app GUI.
    install_panic_hook();
    // Đặc tả §2: bắt buộc quyền admin — tự xin nâng quyền rồi thoát
    if !sys::is_admin() {
        if sys::relaunch_as_admin() {
            return;
        }
        return;
    }
    nwg::init().expect("Không khởi tạo được native-windows-gui");
    let (_window, app) = build_ui();
    push_log(
        &app.shared,
        &app.notice.sender(),
        "Đang quét hệ thống (giai đoạn 0 — kiểm tra tiền đề)...",
    );
    spawn_preflight(app.shared.clone(), app.notice.sender());
    nwg::dispatch_thread_events();
}
