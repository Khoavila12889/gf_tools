#![windows_subsystem = "windows"]
//! QuanLyUpdateWindows — Quản Lý Update Windows.
//!
//! - Hiện trạng thái 3 dịch vụ cốt lõi của Windows Update (wuauserv,
//!   UsoSvc, BITS) qua Service Control Manager API gốc (không qua
//!   `sc.exe`).
//! - Bật cập nhật tự động: kiểu khởi động = Thủ công/Theo yêu cầu
//!   (mặc định của Windows), xoá chính sách NoAutoUpdate, chạy dịch vụ.
//! - Tắt cập nhật kiểu "tạm dừng": dừng + vô hiệu hoá dịch vụ, đặt
//!   chính sách NoAutoUpdate=1. Đảo ngược hoàn toàn bằng nút Bật.
//! - Kiểm tra cập nhật ngay: mở trang ms-settings:windowsupdate.
//! - Đặt lại Windows Update: dừng dịch vụ liên quan, đăng ký lại danh
//!   sách DLL chuẩn bằng regsvr32 /s (một số DLL không còn tồn tại ở
//!   bản Windows mới sẽ báo lỗi trong log — bình thường), chạy lại
//!   dịch vụ. Mọi bước đều ghi UpdateTool_Log.txt cạnh file exe.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use native_windows_gui as nwg;
use qsu_common::cmd;
use qsu_common::log::Logger;
use qsu_common::svc;
use qsu_common::sys;
use winapi::shared::windef::RECT;
use winapi::um::shellapi::ShellExecuteW;
use winapi::um::winuser::{GetClientRect, SW_SHOWNORMAL};
use winapi::shared::minwindef::HKEY;
use winapi::um::winreg::{RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, HKEY_LOCAL_MACHINE};
use winapi::um::winnt::{KEY_WRITE, REG_DWORD};

const HEADER_H: i32 = 66;

const SERVICES: [(&str, &str); 3] = [
    ("wuauserv", "Windows Update"),
    ("UsoSvc", "Update Orchestrator (điều phối cập nhật)"),
    ("BITS", "Background Intelligent Transfer Service (tải nền)"),
];

/// Danh sách DLL chuẩn của quy trình "Reset Windows Update Components".
const RESET_DLLS: [&str; 34] = [
    "atl.dll", "urlmon.dll", "mshtml.dll", "shdocvw.dll", "browseui.dll", "jscript.dll",
    "vbscript.dll", "scrrun.dll", "msxml.dll", "msxml3.dll", "msxml6.dll", "actxprxy.dll",
    "softpub.dll", "wintrust.dll", "dssenh.dll", "rsaenh.dll", "gpkcsp.dll", "sccbase.dll",
    "slbcsp.dll", "cryptdlg.dll", "oleaut32.dll", "ole32.dll", "shell32.dll", "initpki.dll",
    "wuapi.dll", "wuaueng.dll", "wuaueng1.dll", "wucltui.dll", "wups.dll", "wups2.dll",
    "wuweb.dll", "qmgr.dll", "qmgrprxy.dll", "wucltux.dll",
];

#[derive(Clone)]
struct SvcRow {
    name: String,
    display: String,
    state: String,
    start_type: String,
}

fn query_rows() -> Vec<SvcRow> {
    SERVICES
        .iter()
        .map(|(name, display)| {
            let info = svc::query(name);
            if info.exists {
                SvcRow {
                    name: name.to_string(),
                    display: display.to_string(),
                    state: svc::state_label(info.state),
                    start_type: svc::start_type_label(info.start_type),
                }
            } else {
                SvcRow {
                    name: name.to_string(),
                    display: display.to_string(),
                    state: "Không tồn tại trên máy này".to_string(),
                    start_type: String::new(),
                }
            }
        })
        .collect()
}

/// Đặt/xoá chính sách NoAutoUpdate (HKLM\...\WindowsUpdate\AU) bằng
/// Win32 Registry API gốc.
fn set_no_auto_update(disable: bool, logger: &Logger) {
    unsafe {
        let subkey = sys::wide("SOFTWARE\\Policies\\Microsoft\\Windows\\WindowsUpdate\\AU");
        let mut hkey: HKEY = std::ptr::null_mut();
        let rc = RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            subkey.as_ptr(),
            0,
            std::ptr::null_mut(),
            0,
            KEY_WRITE,
            std::ptr::null_mut(),
            &mut hkey,
            std::ptr::null_mut(),
        );
        if rc != 0 || hkey.is_null() {
            logger.line(&format!("Không mở được khoá chính sách Windows Update (mã {})", rc));
            return;
        }
        let value_name = sys::wide("NoAutoUpdate");
        if disable {
            let val: u32 = 1;
            let rc2 = RegSetValueExW(
                hkey,
                value_name.as_ptr(),
                0,
                REG_DWORD,
                &val as *const u32 as *const u8,
                4,
            );
            if rc2 == 0 {
                logger.line("Đã đặt chính sách NoAutoUpdate = 1");
            } else {
                logger.line(&format!("Lỗi đặt NoAutoUpdate (mã {})", rc2));
            }
        } else {
            let rc2 = RegDeleteValueW(hkey, value_name.as_ptr());
            if rc2 == 0 {
                logger.line("Đã xoá chính sách NoAutoUpdate");
            } else {
                logger.line("Không có chính sách NoAutoUpdate để xoá (bỏ qua)");
            }
        }
        RegCloseKey(hkey);
    }
}

fn open_update_settings() {
    unsafe {
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let uri: Vec<u16> = "ms-settings:windowsupdate\0".encode_utf16().collect();
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            uri.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

// ---------------------------------------------------------------------------
// Trạng thái chia sẻ
// ---------------------------------------------------------------------------
#[derive(Default)]
struct Shared {
    log_lines: VecDeque<String>,
    rows: Option<Vec<SvcRow>>,
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
        while s.log_lines.len() > 400 {
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
        s.rows = Some(query_rows());
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
    let p = nwg::MessageParams { title, content, buttons: nwg::MessageButtons::YesNo, icons: nwg::MessageIcons::Question };
    nwg::message(&p) == nwg::MessageChoice::Yes
}

// ---------------------------------------------------------------------------
// ListView helper
// ---------------------------------------------------------------------------
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
    txt_log: nwg::TextBox,
    lbl_status: nwg::Label,
    btn_refresh: nwg::Button,
    btn_enable: nwg::Button,
    btn_disable: nwg::Button,
    btn_check: nwg::Button,
    btn_reset: nwg::Button,
    shared: SharedRef,
    exe_dir: PathBuf,
    log_rendered: RefCell<usize>,
}

fn make_logger(app: &Rc<App>) -> Arc<Logger> {
    let path = app.exe_dir.join("UpdateTool_Log.txt");
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let sink = Box::new(move |line: &str| {
        push_log(&shared, &sender, line);
    });
    Arc::new(Logger::new(path, Some(sink)))
}

fn spawn_refresh(app: &Rc<App>) {
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    {
        shared.lock().unwrap().busy = true;
    }
    set_status(&shared, &sender, "Đang đọc trạng thái dịch vụ...");
    std::thread::spawn(move || {
        let rows = query_rows();
        let mut s = shared.lock().unwrap();
        s.rows = Some(rows);
        s.busy = false;
        drop(s);
        sender.notice();
    });
}

/// Chạy một hành động dài trên luồng nền; f trả về thông điệp kết quả.
fn spawn_action<F>(app: &Rc<App>, busy_text: &str, f: F)
where
    F: FnOnce(&Logger) -> String + Send + 'static,
{
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let logger = make_logger(app);
    {
        shared.lock().unwrap().busy = true;
    }
    set_status(&shared, &sender, busy_text);
    std::thread::spawn(move || {
        let msg = f(&logger);
        finish(&shared, &sender, Some(msg), None);
    });
}

fn action_enable(logger: &Logger) -> String {
    logger.line("=== BẬT CẬP NHẬT TỰ ĐỘNG ===");
    set_no_auto_update(false, logger);
    for (name, _) in SERVICES {
        match svc::set_start_type(name, svc::START_DEMAND) {
            Ok(()) => logger.line(&format!("{}: kiểu khởi động = Thủ công (theo yêu cầu)", name)),
            Err(e) => logger.line(&format!("{}: lỗi đặt kiểu khởi động: {}", name, e)),
        }
    }
    for (name, _) in SERVICES {
        match svc::start(name) {
            Ok(()) => logger.line(&format!("{}: đã chạy", name)),
            Err(e) => logger.line(&format!("{}: {}", name, e)),
        }
    }
    logger.line("=== BẬT xong ===");
    "Đã bật cập nhật tự động: dịch vụ Windows Update ở chế độ khởi động theo yêu cầu (mặc định của Windows), chính sách chặn cập nhật đã được gỡ.".to_string()
}

fn action_disable(logger: &Logger) -> String {
    logger.line("=== TẮT CẬP NHẬT (tạm dừng kiểu vô hiệu hoá) ===");
    for (name, _) in SERVICES {
        match svc::stop(name) {
            Ok(()) => logger.line(&format!("{}: đã dừng", name)),
            Err(e) => logger.line(&format!("{}: {}", name, e)),
        }
    }
    for (name, _) in SERVICES {
        match svc::set_start_type(name, svc::START_DISABLED) {
            Ok(()) => logger.line(&format!("{}: kiểu khởi động = Đã tắt", name)),
            Err(e) => logger.line(&format!("{}: lỗi đặt kiểu khởi động: {}", name, e)),
        }
    }
    set_no_auto_update(true, logger);
    logger.line("=== TẮT xong ===");
    "Đã tắt cập nhật Windows: các dịch vụ đã dừng và bị vô hiệu hoá, kèm chính sách NoAutoUpdate=1. Dùng nút 'Bật cập nhật tự động' để khôi phục bất cứ lúc nào.".to_string()
}

fn action_reset(logger: &Logger) -> String {
    logger.line("=== ĐẶT LẠI THÀNH PHẦN WINDOWS UPDATE ===");
    let stop_list = ["wuauserv", "bits", "cryptsvc", "msiserver", "UsoSvc"];
    for name in stop_list {
        match svc::stop(name) {
            Ok(()) => logger.line(&format!("{}: đã dừng", name)),
            Err(e) => logger.line(&format!("{}: {} (bỏ qua)", name, e)),
        }
    }
    logger.line(&format!("Đăng ký lại {} DLL bằng regsvr32 /s...", RESET_DLLS.len()));
    let mut ok = 0usize;
    for dll in RESET_DLLS {
        match cmd::run_capture("regsvr32.exe", &["/s", dll]) {
            Ok(out) if out.code == Some(0) => ok += 1,
            Ok(out) => logger.line(&format!("  {}: regsvr32 trả mã {:?} (DLL có thể không còn tồn tại ở bản Windows này)", dll, out.code)),
            Err(e) => logger.line(&format!("  {}: không chạy được regsvr32: {}", dll, e)),
        }
    }
    logger.line(&format!("Đăng ký xong: {}/{} DLL thành công", ok, RESET_DLLS.len()));
    // Khởi động lại dịch vụ. Chỉ sửa kiểu khởi động khi dịch vụ đang bị
    // Vô hiệu hoá (Disabled) — và khôi phục về ĐÚNG mặc định Windows của
    // từng dịch vụ (CryptSvc mặc định là Tự động, không phải Thủ công);
    // dịch vụ đang ở kiểu khởi động khác thì giữ nguyên, không tự ý đổi.
    let start_list: [(&str, u32); 4] = [
        ("cryptsvc", svc::START_AUTO),
        ("bits", svc::START_DEMAND),
        ("wuauserv", svc::START_DEMAND),
        ("UsoSvc", svc::START_DEMAND),
    ];
    for (name, default_start) in start_list {
        let info = svc::query(name);
        if info.exists && info.start_type == svc::START_DISABLED {
            match svc::set_start_type(name, default_start) {
                Ok(()) => logger.line(&format!(
                    "{}: khôi phục kiểu khởi động mặc định ({})",
                    name,
                    svc::start_type_label(default_start)
                )),
                Err(e) => logger.line(&format!("{}: lỗi khôi phục kiểu khởi động: {}", name, e)),
            }
        }
        match svc::start(name) {
            Ok(()) => logger.line(&format!("{}: đã chạy lại", name)),
            Err(e) => logger.line(&format!("{}: {}", name, e)),
        }
    }
    logger.line("=== ĐẶT LẠI xong ===");
    "Đã đặt lại thành phần Windows Update: đăng ký lại DLL và khởi động lại các dịch vụ liên quan. Hãy bấm 'Kiểm tra cập nhật ngay' để thử lại.".to_string()
}

fn build_ui(exe_dir: PathBuf) -> (nwg::Window, Rc<App>) {
    let mut f_title = Default::default();
    nwg::Font::builder().family("Segoe UI").size(21).weight(700).build(&mut f_title).expect("font");
    let mut f_normal = Default::default();
    nwg::Font::builder().family("Segoe UI").size(15).build(&mut f_normal).expect("font");
    let mut f_small = Default::default();
    nwg::Font::builder().family("Segoe UI").size(14).build(&mut f_small).expect("font");
    let mut f_button = Default::default();
    nwg::Font::builder().family("Segoe UI").size(15).weight(700).build(&mut f_button).expect("font");
    let mut f_mono = Default::default();
    nwg::Font::builder().family("Consolas").size(14).build(&mut f_mono).expect("font");
    let f_title = Box::leak(Box::new(f_title));
    let f_normal = Box::leak(Box::new(f_normal));
    let f_small = Box::leak(Box::new(f_small));
    let f_button = Box::leak(Box::new(f_button));
    let f_mono = Box::leak(Box::new(f_mono));

    let mut window = Default::default();
    nwg::Window::builder()
        .size((880, 664))
        .position((180, 110))
        .title("Quản Lý Update Windows")
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE | nwg::WindowFlags::MINIMIZE_BOX)
        .build(&mut window)
        .expect("window");

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

    let _t = add_label("Quản Lý Update Windows", f_title, (16, 8), (400, 30));
    let _s = add_label(
        "Bật/tắt cập nhật tự động, kiểm tra cập nhật và đặt lại thành phần Windows Update",
        f_normal, (16, 40), (700, 22),
    );
    let _l1 = add_label("Trạng thái dịch vụ:", f_normal, (14, HEADER_H + 12), (300, 22));

    let mut list = Default::default();
    nwg::ListView::builder()
        .parent(&window)
        .list_style(nwg::ListViewStyle::Detailed)
        .flags(nwg::ListViewFlags::VISIBLE | nwg::ListViewFlags::TAB_STOP | nwg::ListViewFlags::ALWAYS_SHOW_SELECTION)
        .ex_flags(nwg::ListViewExFlags::GRID | nwg::ListViewExFlags::FULL_ROW_SELECT)
        .build(&mut list)
        .expect("listview");
    list.set_headers_enabled(true);
    list.set_position(14, HEADER_H + 38);
    list.set_size((client_w - 28) as u32, 110);
    for (i, (name, w)) in [("Dịch vụ", 120), ("Tên", 330), ("Trạng thái", 130), ("Kiểu khởi động", 190)].iter().enumerate() {
        list.insert_column(nwg::InsertListViewColumn {
            index: Some(i as i32),
            text: Some(name.to_string()),
            width: Some(*w),
            ..Default::default()
        });
    }

    let btn_y = HEADER_H + 38 + 110 + 12;
    let mk_btn = |text: &str, pos: (i32, i32), size: (u32, u32)| -> nwg::Button {
        let mut b = Default::default();
        nwg::Button::builder().text(text).font(Some(f_button)).parent(&window).build(&mut b).expect("button");
        b.set_position(pos.0, pos.1);
        b.set_size(size.0, size.1);
        b
    };
    let btn_refresh = mk_btn("Làm mới", (14, btn_y), (90, 34));
    let btn_enable = mk_btn("Bật cập nhật tự động", (112, btn_y), (170, 34));
    let btn_disable = mk_btn("Tắt cập nhật", (290, btn_y), (120, 34));
    let btn_check = mk_btn("Kiểm tra cập nhật ngay", (418, btn_y), (180, 34));
    let btn_reset = mk_btn("Đặt lại Windows Update", (606, btn_y), (190, 34));

    let log_y = btn_y + 46;
    let _l2 = add_label("Nhật ký thao tác:", f_normal, (14, log_y), (300, 22));
    let mut txt_log = Default::default();
    nwg::TextBox::builder()
        .parent(&window)
        .font(Some(f_mono))
        .flags(
            nwg::TextBoxFlags::VISIBLE
                | nwg::TextBoxFlags::VSCROLL
                | nwg::TextBoxFlags::AUTOVSCROLL
                | nwg::TextBoxFlags::TAB_STOP,
        )
        .build(&mut txt_log)
        .expect("textbox");
    txt_log.set_readonly(true);
    txt_log.set_position(14, log_y + 26);
    txt_log.set_size((client_w - 28) as u32, (client_h - (log_y + 26) - 36) as u32);

    let lbl_status = add_label(
        "Đang khởi tạo...",
        f_small,
        (14, client_h - 28),
        ((client_w - 28) as u32, 22),
    );

    let mut notice = Default::default();
    nwg::Notice::builder().parent(&window).build(&mut notice).expect("notice");

    let app = Rc::new(App {
        notice,
        list,
        txt_log,
        lbl_status,
        btn_refresh,
        btn_enable,
        btn_disable,
        btn_check,
        btn_reset,
        shared: Arc::new(Mutex::new(Shared::default())),
        exe_dir,
        log_rendered: RefCell::new(0),
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
                if handle == handler.btn_check.handle {
                    make_logger(&handler).line("Mở trang Cài đặt > Windows Update");
                    open_update_settings();
                    return;
                }
                if handler.shared.lock().unwrap().busy {
                    return;
                }
                if handle == handler.btn_refresh.handle {
                    spawn_refresh(&handler);
                } else if handle == handler.btn_enable.handle {
                    spawn_action(&handler, "Đang bật cập nhật tự động...", action_enable);
                } else if handle == handler.btn_disable.handle {
                    let text = "TẮT cập nhật Windows?\n\n\
                        • Dừng và VÔ HIỆU HOÁ các dịch vụ wuauserv, UsoSvc, BITS.\n\
                        • Đặt chính sách NoAutoUpdate = 1.\n\
                        • Máy sẽ KHÔNG nhận bản vá bảo mật cho tới khi bật lại.\n\n\
                        Thao tác đảo ngược được bằng nút 'Bật cập nhật tự động'. Tiếp tục?";
                    if confirm_box("Xác nhận tắt cập nhật", text) {
                        spawn_action(&handler, "Đang tắt cập nhật...", action_disable);
                    }
                } else if handle == handler.btn_reset.handle {
                    let text = "ĐẶT LẠI thành phần Windows Update?\n\n\
                        • Dừng các dịch vụ liên quan (wuauserv, BITS, CryptSvc, MSI, UsoSvc).\n\
                        • Đăng ký lại các DLL hệ thống chuẩn bằng regsvr32.\n\
                        • Khởi động lại dịch vụ và khôi phục kiểu khởi động mặc định.\n\n\
                        Dùng khi Windows Update báo lỗi tải/cài bản cập nhật. Tiếp tục?";
                    if confirm_box("Xác nhận đặt lại Windows Update", text) {
                        spawn_action(&handler, "Đang đặt lại Windows Update...", action_reset);
                    }
                }
            }
            _ => {}
        }
    });

    (window, app)
}

fn refresh(app: &Rc<App>) {
    let (rows, status, done, err, new_lines) = {
        let mut s = app.shared.lock().unwrap();
        let rendered = *app.log_rendered.borrow();
        let lines: Vec<String> = s.log_lines.iter().skip(rendered).cloned().collect();
        (s.rows.take(), s.status.take(), s.done_msg.take(), s.err_msg.take(), lines)
    };
    if !new_lines.is_empty() {
        *app.log_rendered.borrow_mut() += new_lines.len();
        let mut text = app.txt_log.text();
        for l in &new_lines {
            text.push_str(l);
            text.push_str("\r\n");
        }
        app.txt_log.set_text(&text);
        app.txt_log.set_selection(text.len() as u32..text.len() as u32);
    }
    if let Some(rows) = rows {
        app.list.clear();
        for (i, r) in rows.iter().enumerate() {
            lv_insert_row(&app.list, i as i32, &[r.name.clone(), r.display.clone(), r.state.clone(), r.start_type.clone()]);
        }
        if !app.shared.lock().unwrap().busy {
            app.lbl_status.set_text("Sẵn sàng.");
        }
    }
    if let Some(st) = status {
        app.lbl_status.set_text(&st);
    }
    if let Some(msg) = done {
        app.lbl_status.set_text("Sẵn sàng.");
        info_box("Quản Lý Update Windows", &msg);
    }
    if let Some(msg) = err {
        app.lbl_status.set_text("Có lỗi xảy ra.");
        error_box("Quản Lý Update Windows", &msg);
    }
    let busy = app.shared.lock().unwrap().busy;
    for b in [&app.btn_refresh, &app.btn_enable, &app.btn_disable, &app.btn_reset] {
        b.set_enabled(!busy);
    }
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
    make_logger(&app).line("--- Khởi động Quản Lý Update Windows ---");
    spawn_refresh(&app);
    nwg::dispatch_thread_events();
}
