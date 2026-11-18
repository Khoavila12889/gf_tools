#![windows_subsystem = "windows"]
//! TatBitlocker — Tắt BitLocker.
//!
//! - Trạng thái BitLocker lấy bằng lệnh trợ giúp PowerShell
//!   `Get-BitLockerVolume` xuất JSON (giống cách module Cài Đặt Win
//!   đang dùng): output của `manage-bde -status` bị bản địa hoá theo
//!   ngôn ngữ Windows nên phân tích text rất dễ sai; cmdlet trả về
//!   enum ổn định. Đây là điểm dùng PowerShell duy nhất và có chủ ý.
//! - Tắt mã hoá: `manage-bde -off X:` (chạy nền trong Windows), công
//!   cụ poll trạng thái mỗi 5 giây để hiện % còn mã hoá.
//! - Trước khi tắt luôn có hộp xác nhận kèm cảnh báo rõ ràng.
//! Mọi thao tác ghi vào Bitlocker_Log.txt cạnh file exe.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use native_windows_gui as nwg;
use qsu_common::cmd;
use qsu_common::json::{self, Json};
use qsu_common::log::Logger;
use qsu_common::sys;
use winapi::shared::windef::RECT;
use winapi::um::winuser::GetClientRect;

const HEADER_H: i32 = 66;

// ---------------------------------------------------------------------------
// Truy vấn BitLocker
// ---------------------------------------------------------------------------
#[derive(Clone)]
struct BlVol {
    mount: String, // "C:"
    label: String,
    size_gb: f64,
    volume_status: String,
    protection: String,
    pct: i64,
}

impl BlVol {
    fn can_decrypt(&self) -> bool {
        self.volume_status != "FullyDecrypted" && self.volume_status != "DecryptionInProgress"
    }
}

fn volume_status_vi(s: &str) -> String {
    match s {
        "FullyEncrypted" => "Đã mã hoá toàn bộ".to_string(),
        "EncryptionInProgress" => "Đang mã hoá".to_string(),
        "DecryptionInProgress" => "Đang giải mã".to_string(),
        "FullyDecrypted" => "Chưa mã hoá / đã tắt".to_string(),
        "PartiallyEncrypted" => "Mã hoá một phần".to_string(),
        "EncryptionPaused" => "Tạm dừng mã hoá".to_string(),
        "DecryptionPaused" => "Tạm dừng giải mã".to_string(),
        other => format!("Không rõ ({})", other),
    }
}

fn protection_vi(s: &str) -> String {
    match s {
        "On" => "Bật".to_string(),
        "Off" => "Tắt".to_string(),
        "Suspended" => "Tạm treo".to_string(),
        other => format!("Không rõ ({})", other),
    }
}

const BL_SCRIPT: &str = "Get-BitLockerVolume | ForEach-Object { [PSCustomObject]@{ MountPoint=$_.MountPoint.ToString(); VolumeStatus=$_.VolumeStatus.ToString(); ProtectionStatus=$_.ProtectionStatus.ToString(); EncryptionPercentage=[int]$_.EncryptionPercentage; SizeGB=[math]::Round($_.CapacityGB,1) } } | ConvertTo-Json -Compress";

fn query_volumes() -> Result<Vec<BlVol>, String> {
    let res = cmd::run_capture(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-Command", BL_SCRIPT],
    )
    .map_err(|e| format!("Không chạy được PowerShell: {}", e))?;
    let out = res.stdout.trim().to_string();
    if out.is_empty() {
        return Err(
            "Không lấy được trạng thái BitLocker. Máy có thể đang dùng bản Windows không có BitLocker (Home) hoặc tính năng chưa được bật."
                .to_string(),
        );
    }
    let parsed = json::parse(&out).map_err(|e| format!("Không đọc được kết quả BitLocker: {}", e))?;
    let items: Vec<&Json> = match parsed.as_arr() {
        Some(a) => a.iter().collect(),
        None => vec![&parsed],
    };
    // Nhãn + dung lượng từ liệt kê ổ đĩa gốc (không phụ thuộc ngôn ngữ)
    let drives = sys::list_fixed_drives();
    let mut vols = Vec::new();
    for it in items {
        let mount = it.str_or("MountPoint", "");
        if mount.is_empty() {
            continue;
        }
        let letter = mount.chars().next().unwrap_or('?');
        let size_json = it.get("SizeGB").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let (label, total_gb) = drives
            .iter()
            .find(|d| d.letter == letter)
            .map(|d| (d.label.clone(), d.total as f64 / 1_073_741_824.0))
            .unwrap_or_else(|| (String::new(), size_json));
        vols.push(BlVol {
            mount,
            label,
            size_gb: total_gb,
            volume_status: it.str_or("VolumeStatus", "Unknown"),
            protection: it.str_or("ProtectionStatus", "Unknown"),
            pct: it.get("EncryptionPercentage").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64,
        });
    }
    vols.sort_by(|a, b| a.mount.cmp(&b.mount));
    Ok(vols)
}

// ---------------------------------------------------------------------------
// Trạng thái chia sẻ
// ---------------------------------------------------------------------------
#[derive(Default)]
struct Shared {
    log_lines: VecDeque<String>,
    volumes: Option<Vec<BlVol>>,
    status: Option<String>,
    done_msg: Option<String>,
    err_msg: Option<String>,
    busy: bool,
}

type SharedRef = Arc<Mutex<Shared>>;

fn push_log(shared: &SharedRef, sender: &nwg::NoticeSender, msg: &str) {
    {
        let mut s = shared.lock().unwrap();
        s.log_lines.push_back(msg.to_string());
        while s.log_lines.len() > 300 {
            s.log_lines.pop_front();
        }
    }
    sender.notice();
}

fn set_status(shared: &SharedRef, sender: &nwg::NoticeSender, msg: &str) {
    {
        let mut s = shared.lock().unwrap();
        s.status = Some(msg.to_string());
    }
    sender.notice();
}

fn finish(shared: &SharedRef, sender: &nwg::NoticeSender, ok_msg: Option<String>, err_msg: Option<String>) {
    {
        let mut s = shared.lock().unwrap();
        s.busy = false;
        s.done_msg = ok_msg;
        s.err_msg = err_msg;
    }
    sender.notice();
}

// ---------------------------------------------------------------------------
// Hộp thoại
// ---------------------------------------------------------------------------
fn info_box(title: &str, content: &str) {
    let p = nwg::MessageParams { title, content, buttons: nwg::MessageButtons::Ok, icons: nwg::MessageIcons::Info };
    nwg::message(&p);
}

fn error_box(title: &str, content: &str) {
    let p = nwg::MessageParams { title, content, buttons: nwg::MessageButtons::Ok, icons: nwg::MessageIcons::Error };
    nwg::message(&p);
}

fn confirm_box(title: &str, content: &str) -> bool {
    let p = nwg::MessageParams { title, content, buttons: nwg::MessageButtons::YesNo, icons: nwg::MessageIcons::Warning };
    nwg::message(&p) == nwg::MessageChoice::Yes
}

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
// Ứng dụng
// ---------------------------------------------------------------------------
struct App {
    notice: nwg::Notice,
    list: nwg::ListView,
    lbl_status: nwg::Label,
    btn_refresh: nwg::Button,
    btn_off: nwg::Button,
    volumes: RefCell<Vec<BlVol>>,
    shared: SharedRef,
    exe_dir: PathBuf,
}

fn make_logger(app: &Rc<App>) -> Arc<Logger> {
    let path = app.exe_dir.join("Bitlocker_Log.txt");
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let sink = Box::new(move |line: &str| {
        push_log(&shared, &sender, line);
    });
    Arc::new(Logger::new(path, Some(sink)))
}

fn refill_list(app: &Rc<App>) {
    app.list.clear();
    let vols = app.volumes.borrow();
    for (i, v) in vols.iter().enumerate() {
        lv_insert_row(
            &app.list,
            i as i32,
            &[
                v.mount.clone(),
                v.label.clone(),
                format!("{:.0} GB", v.size_gb),
                volume_status_vi(&v.volume_status),
                protection_vi(&v.protection),
                format!("{}%", v.pct),
            ],
        );
    }
}

fn spawn_refresh(app: &Rc<App>) {
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    {
        shared.lock().unwrap().busy = true;
    }
    set_status(&shared, &sender, "Đang đọc trạng thái BitLocker...");
    std::thread::spawn(move || match query_volumes() {
        Ok(vols) => {
            let mut s = shared.lock().unwrap();
            s.volumes = Some(vols);
            s.busy = false;
            drop(s);
            sender.notice();
        }
        Err(e) => finish(&shared, &sender, None, Some(e)),
    });
}

fn spawn_decrypt(app: &Rc<App>, vol: BlVol) {
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let logger = make_logger(app);
    {
        shared.lock().unwrap().busy = true;
    }
    set_status(&shared, &sender, &format!("Đang gửi lệnh tắt BitLocker cho ổ {}...", vol.mount));
    let mount = vol.mount.clone();
    std::thread::spawn(move || {
        logger.line(&format!("=== TẮT BITLOCKER ổ {} ===", mount));
        match cmd::run_logged(&logger, "manage-bde.exe", &["-off", &mount]) {
            Ok(out) if out.code == Some(0) => {}
            Ok(out) => {
                logger.line(&format!("manage-bde -off trả mã {:?}", out.code));
                finish(
                    &shared,
                    &sender,
                    None,
                    Some(format!(
                        "Không tắt được BitLocker cho ổ {} (manage-bde báo lỗi). Chi tiết trong Bitlocker_Log.txt cạnh công cụ.",
                        mount
                    )),
                );
                return;
            }
            Err(e) => {
                finish(&shared, &sender, None, Some(format!("Không chạy được manage-bde: {}", e)));
                return;
            }
        }
        logger.line("Đã gửi lệnh giải mã. Bắt đầu theo dõi tiến độ (mỗi 5 giây)...");
        // Poll tối đa ~1 giờ; giải mã chạy nền trong Windows kể cả khi
        // đóng công cụ.
        for _ in 0..720 {
            std::thread::sleep(std::time::Duration::from_secs(5));
            match query_volumes() {
                Ok(vols) => {
                    if let Some(v) = vols.iter().find(|v| v.mount == mount) {
                        {
                            let mut s = shared.lock().unwrap();
                            s.volumes = Some(vols.clone());
                        }
                        if v.volume_status == "FullyDecrypted" {
                            logger.line(&format!("Ổ {} đã giải mã xong (FullyDecrypted)", mount));
                            finish(
                                &shared,
                                &sender,
                                Some(format!(
                                    "Đã tắt BitLocker cho ổ {} xong. Dữ liệu trên ổ không còn được mã hoá bảo vệ nữa.",
                                    mount
                                )),
                                None,
                            );
                            return;
                        }
                        set_status(
                            &shared,
                            &sender,
                            &format!(
                                "Đang giải mã ổ {}: còn mã hoá {}% — {}",
                                mount,
                                v.pct,
                                volume_status_vi(&v.volume_status)
                            ),
                        );
                    }
                }
                Err(_) => { /* bỏ qua nhịp lỗi, thử lại nhịp sau */ }
            }
        }
        finish(
            &shared,
            &sender,
            Some(
                "Đã hết 1 giờ theo dõi. Quá trình giải mã VẪN tiếp tục chạy nền trong Windows — mở lại công cụ để kiểm tra trạng thái."
                    .to_string(),
            ),
            None,
        );
    });
}

fn build_ui(exe_dir: PathBuf) -> (nwg::Window, Rc<App>) {
    let f_normal = qsu_common::ui::font_normal();
    let f_small = qsu_common::ui::font_small();
    let f_button = qsu_common::ui::font_button();

    let mut window = Default::default();
    nwg::Window::builder()
        .size((820, 544))
        .position((220, 140))
        .title("Tắt BitLocker")
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE | nwg::WindowFlags::MINIMIZE_BOX)
        .build(&mut window)
        .expect("window");
    qsu_common::ui::set_window_icon(&window);

    let (client_w, client_h) = {
        let mut rc: RECT = unsafe { std::mem::zeroed() };
        unsafe { GetClientRect(window.handle.hwnd().unwrap(), &mut rc) };
        (rc.right, rc.bottom)
    };

    let add_label = |text: &str, font: &nwg::Font, pos: (i32, i32), size: (u32, u32)| -> nwg::Label {
        let mut lbl = Default::default();
        nwg::Label::builder().text(text).font(Some(font)).parent(&window).build(&mut lbl).expect("label");
        lbl.set_position(pos.0, pos.1);
        lbl.set_size(size.0, size.1);
        lbl
    };

    let (_t, _s) = qsu_common::ui::add_header(&window, "Tắt BitLocker", "Xem trạng thái mã hoá các ổ đĩa và tắt BitLocker (giải mã) khi cần");
    let _l1 = add_label("Các ổ đĩa cố định:", f_normal, (16, HEADER_H + 12), (300, 22));

    let mut list = Default::default();
    nwg::ListView::builder()
        .parent(&window)
        .list_style(nwg::ListViewStyle::Detailed)
        .flags(nwg::ListViewFlags::VISIBLE | nwg::ListViewFlags::TAB_STOP | nwg::ListViewFlags::ALWAYS_SHOW_SELECTION)
        .ex_flags(nwg::ListViewExFlags::GRID | nwg::ListViewExFlags::FULL_ROW_SELECT)
        .build(&mut list)
        .expect("listview");
    list.set_headers_enabled(true);
    list.set_position(16, HEADER_H + 38);
    list.set_size((client_w - 32) as u32, 190);
    for (i, (name, w)) in [
        ("Ổ", 50),
        ("Nhãn", 140),
        ("Dung lượng", 100),
        ("Trạng thái mã hoá", 200),
        ("Bảo vệ", 110),
        ("% mã hoá", 90),
    ]
    .iter()
    .enumerate()
    {
        list.insert_column(nwg::InsertListViewColumn {
            index: Some(i as i32),
            text: Some(name.to_string()),
            width: Some(*w),
            ..Default::default()
        });
    }

    let btn_y = HEADER_H + 38 + 190 + 12;
    let mk_btn = |text: &str, pos: (i32, i32), size: (u32, u32)| -> nwg::Button {
        let mut b = Default::default();
        nwg::Button::builder().text(text).font(Some(f_button)).parent(&window).build(&mut b).expect("button");
        b.set_position(pos.0, pos.1);
        b.set_size(size.0, size.1);
        b
    };
    let btn_refresh = mk_btn("Làm mới", (16, btn_y), (110, 34));
    let btn_off = mk_btn("Tắt BitLocker cho ổ đã chọn", (136, btn_y), (260, 34));
    qsu_common::ui::make_primary(&btn_off);

    let _warn = add_label(
        "CẢNH BÁO: Tắt BitLocker sẽ GIẢI MÃ toàn bộ ổ đĩa (mất từ vài phút tới vài giờ). Trong lúc giải mã TUYỆT ĐỐI không tắt máy, không rút nguồn. Sau khi tắt, dữ liệu trên ổ không còn được bảo vệ bằng mã hoá.",
        f_small,
        (16, btn_y + 46),
        ((client_w - 32) as u32, 44),
    );
    let lbl_status = add_label(
        "Đang khởi tạo...",
        f_small,
        (16, client_h - 28),
        ((client_w - 32) as u32, 22),
    );

    let mut notice = Default::default();
    nwg::Notice::builder().parent(&window).build(&mut notice).expect("notice");

    let app = Rc::new(App {
        notice,
        list,
        lbl_status,
        btn_refresh,
        btn_off,
        volumes: RefCell::new(Vec::new()),
        shared: Arc::new(Mutex::new(Shared::default())),
        exe_dir,
    });

    let handler = app.clone();
    nwg::full_bind_event_handler(&window.handle, move |evt, _data, handle| {
        match evt {
            nwg::Event::OnNotice => {
                if handle == handler.notice.handle {
                    refresh(&handler);
                }
            }
            nwg::Event::OnButtonClick => {
                if handler.shared.lock().unwrap().busy {
                    return;
                }
                if handle == handler.btn_refresh.handle {
                    spawn_refresh(&handler);
                } else if handle == handler.btn_off.handle {
                    let sel = handler.list.selected_item();
                    let vol = sel.and_then(|i| handler.volumes.borrow().get(i).cloned());
                    match vol {
                        None => info_box("Tắt BitLocker", "Hãy chọn một ổ đĩa trong danh sách trước."),
                        Some(v) if !v.can_decrypt() => {
                            if v.volume_status == "DecryptionInProgress" {
                                info_box("Tắt BitLocker", &format!("Ổ {} đang trong quá trình giải mã rồi.", v.mount));
                            } else {
                                info_box("Tắt BitLocker", &format!("Ổ {} hiện không bật BitLocker (đã ở trạng thái chưa mã hoá).", v.mount));
                            }
                        }
                        Some(v) => {
                            let text = format!(
                                "Bạn sắp TẮT BitLocker cho ổ {} ({}).\n\n\
                                 • Toàn bộ dữ liệu trên ổ sẽ được GIẢI MÃ — mất từ vài phút đến vài giờ tuỳ dung lượng.\n\
                                 • Trong lúc giải mã KHÔNG tắt máy, KHÔNG rút nguồn, KHÔNG để máy hết pin.\n\
                                 • Quá trình giải mã chạy nền trong Windows; bạn có thể đóng công cụ này, giải mã vẫn tiếp tục.\n\
                                 • Sau khi tắt, dữ liệu trên ổ KHÔNG còn được bảo vệ bằng mã hoá.\n\n\
                                 Tiếp tục tắt BitLocker cho ổ {}?",
                                v.mount,
                                if v.label.is_empty() { "không nhãn".to_string() } else { v.label.clone() },
                                v.mount
                            );
                            if confirm_box("Xác nhận tắt BitLocker", &text) {
                                spawn_decrypt(&handler, v);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    });

    (window, app)
}

fn refresh(app: &Rc<App>) {
    let (volumes, status, done, err) = {
        let mut s = app.shared.lock().unwrap();
        (s.volumes.take(), s.status.take(), s.done_msg.take(), s.err_msg.take())
    };
    if let Some(vols) = volumes {
        *app.volumes.borrow_mut() = vols;
        refill_list(app);
        if !app.shared.lock().unwrap().busy {
            app.lbl_status.set_text("Sẵn sàng.");
        }
    }
    if let Some(st) = status {
        app.lbl_status.set_text(&st);
    }
    if let Some(msg) = done {
        app.lbl_status.set_text("Sẵn sàng.");
        info_box("Tắt BitLocker", &msg);
    }
    if let Some(msg) = err {
        app.lbl_status.set_text("Có lỗi xảy ra.");
        error_box("Tắt BitLocker", &msg);
    }
    let busy = app.shared.lock().unwrap().busy;
    app.btn_refresh.set_enabled(!busy);
    app.btn_off.set_enabled(!busy);
}

fn main() {
    if !sys::is_admin() {
        if sys::relaunch_as_admin() {
            return;
        }
        return;
    }
    nwg::init().expect("Không khởi tạo được native-windows-gui");
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let (_window, app) = build_ui(exe_dir);
    make_logger(&app).line("--- Khởi động Tắt BitLocker ---");
    spawn_refresh(&app);
    nwg::dispatch_thread_events();
}
