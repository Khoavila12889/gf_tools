#![windows_subsystem = "windows"]
//! SaoLuuWifi — Sao Lưu / Quản Lý Wifi.
//!
//! - Liệt kê hồ sơ Wifi đã lưu: đọc trực tiếp thư mục hồ sơ của dịch
//!   vụ WLAN (C:\ProgramData\Microsoft\Wlansvc\Profiles\Interfaces),
//!   lấy tên hồ sơ từ chính file XML — KHÔNG phân tích output netsh
//!   (vốn bị bản địa hoá theo ngôn ngữ Windows). Chỉ khi thư mục này
//!   không đọc được mới dự phòng bằng `netsh wlan show profiles`.
//! - Xuất hồ sơ đã tick ra thư mục tuỳ chọn kèm mật khẩu rõ
//!   (`netsh wlan export profile key=clear`) để mang sang máy khác.
//! - Nhập lại hồ sơ từ thư mục chứa file XML (`netsh wlan add profile`).
//! - Xem mật khẩu đã lưu: xuất tạm ra thư mục Temp, đọc thẻ
//!   <keyMaterial> trong XML (cách lấy mật khẩu không phụ thuộc ngôn
//!   ngữ hiển thị), hiện lên lưới rồi xoá thư mục tạm.
//! Mọi thao tác ghi vào WifiTool_Log.txt cạnh file exe.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use native_windows_gui as nwg;
use qsu_common::cmd;
use qsu_common::log::Logger;
use qsu_common::sys;
use winapi::shared::minwindef::{LPARAM, UINT, WPARAM};
use winapi::shared::windef::RECT;
use winapi::um::commctrl::LVITEMW;
use winapi::um::winuser::{GetClientRect, SendMessageW};

const HEADER_H: i32 = 66;

// ---------------------------------------------------------------------------
// Hồ sơ Wifi
// ---------------------------------------------------------------------------
#[derive(Clone)]
struct WifiProfile {
    name: String,
    xml_path: PathBuf,
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Lấy nội dung thẻ XML đầu tiên có tên cho trước (không namespace).
fn xml_tag_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml_unescape(xml[start..end].trim()))
}

/// Liệt kê hồ sơ từ thư mục Wlansvc (nguồn chính, không phụ thuộc ngôn ngữ).
fn list_profiles_from_wlansvc() -> Vec<WifiProfile> {
    let base = PathBuf::from("C:\\ProgramData\\Microsoft\\Wlansvc\\Profiles\\Interfaces");
    let mut out = Vec::new();
    if let Ok(ifaces) = std::fs::read_dir(&base) {
        for iface in ifaces.flatten() {
            let dir = iface.path();
            if !dir.is_dir() {
                continue;
            }
            if let Ok(files) = std::fs::read_dir(&dir) {
                for f in files.flatten() {
                    let p = f.path();
                    let is_xml = p.extension().map(|e| e.eq_ignore_ascii_case("xml")).unwrap_or(false);
                    if !is_xml {
                        continue;
                    }
                    if let Ok(content) = std::fs::read_to_string(&p) {
                        if let Some(name) = xml_tag_text(&content, "name") {
                            if !name.is_empty() {
                                out.push(WifiProfile { name, xml_path: p });
                            }
                        }
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out.dedup_by(|a, b| a.name == b.name);
    out
}

/// Dự phòng: phân tích `netsh wlan show profiles` (lấy phần sau " : ").
fn list_profiles_from_netsh() -> Vec<WifiProfile> {
    let mut out = Vec::new();
    if let Ok(res) = cmd::run_capture("netsh.exe", &["wlan", "show", "profiles"]) {
        for line in res.stdout.lines() {
            if let Some(pos) = line.find(" : ") {
                let name = line[pos + 3..].trim().to_string();
                if !name.is_empty() && name != "<None>" && !out.iter().any(|p: &WifiProfile| p.name == name) {
                    out.push(WifiProfile { name, xml_path: PathBuf::new() });
                }
            }
        }
    }
    out
}

fn list_profiles() -> Vec<WifiProfile> {
    let v = list_profiles_from_wlansvc();
    if v.is_empty() {
        list_profiles_from_netsh()
    } else {
        v
    }
}

// ---------------------------------------------------------------------------
// Trạng thái chia sẻ
// ---------------------------------------------------------------------------
#[derive(Default)]
struct Shared {
    log_lines: VecDeque<String>,
    profiles: Option<Vec<WifiProfile>>,
    keys: Option<Vec<(String, String)>>,
    status: Option<String>,
    done_msg: Option<String>,
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

fn finish(shared: &SharedRef, sender: &nwg::NoticeSender, msg: &str) {
    {
        let mut s = shared.lock().unwrap();
        s.busy = false;
        s.done_msg = Some(msg.to_string());
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

// ---------------------------------------------------------------------------
// ListView checkbox
// ---------------------------------------------------------------------------
const LVM_SETEXTENDEDLISTVIEWSTYLE: UINT = 0x1036;
const LVM_GETITEMSTATE: UINT = 0x102C;
const LVM_SETITEMSTATE: UINT = 0x102B;
const LVS_EX_CHECKBOXES: LPARAM = 0x4;
const LVIS_STATEIMAGEMASK: u32 = 0xF000;

fn lv_enable_checkboxes(lv: &nwg::ListView) {
    if let Some(hwnd) = lv.handle.hwnd() {
        unsafe {
            SendMessageW(hwnd, LVM_SETEXTENDEDLISTVIEWSTYLE, LVS_EX_CHECKBOXES as WPARAM, LVS_EX_CHECKBOXES);
        }
    }
}

fn lv_item_checked(lv: &nwg::ListView, row: usize) -> bool {
    if let Some(hwnd) = lv.handle.hwnd() {
        let st = unsafe { SendMessageW(hwnd, LVM_GETITEMSTATE, row as WPARAM, LVIS_STATEIMAGEMASK as LPARAM) };
        return (st as u32 >> 12) == 2;
    }
    false
}

fn lv_set_checked(lv: &nwg::ListView, row: usize, checked: bool) {
    if let Some(hwnd) = lv.handle.hwnd() {
        let mut item: LVITEMW = unsafe { std::mem::zeroed() };
        item.stateMask = LVIS_STATEIMAGEMASK;
        item.state = (if checked { 2u32 } else { 1u32 }) << 12;
        unsafe {
            SendMessageW(hwnd, LVM_SETITEMSTATE, row as WPARAM, &item as *const LVITEMW as LPARAM);
        }
    }
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
    keys: nwg::ListView,
    lbl_status: nwg::Label,
    btn_refresh: nwg::Button,
    btn_all: nwg::Button,
    btn_none: nwg::Button,
    btn_export: nwg::Button,
    btn_import: nwg::Button,
    btn_keys: nwg::Button,
    profiles: RefCell<Vec<WifiProfile>>,
    checked: RefCell<Vec<bool>>,
    shared: SharedRef,
    exe_dir: PathBuf,
}

fn make_logger(app: &Rc<App>) -> Arc<Logger> {
    let path = app.exe_dir.join("WifiTool_Log.txt");
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let sink = Box::new(move |line: &str| {
        push_log(&shared, &sender, line);
    });
    Arc::new(Logger::new(path, Some(sink)))
}

fn harvest_checks(app: &Rc<App>) {
    let mut checked = app.checked.borrow_mut();
    for i in 0..checked.len() {
        checked[i] = lv_item_checked(&app.list, i);
    }
}

fn refill_list(app: &Rc<App>) {
    app.list.clear();
    let profiles = app.profiles.borrow();
    let checked = app.checked.borrow();
    for (i, p) in profiles.iter().enumerate() {
        lv_insert_row(&app.list, i as i32, &[p.name.clone(), p.xml_path.display().to_string()]);
        lv_set_checked(&app.list, i, checked.get(i).copied().unwrap_or(true));
    }
}

fn refill_keys(app: &Rc<App>, rows: &[(String, String)]) {
    app.keys.clear();
    for (i, (name, key)) in rows.iter().enumerate() {
        lv_insert_row(&app.keys, i as i32, &[name.clone(), key.clone()]);
    }
}

fn spawn_refresh(app: &Rc<App>) {
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    {
        shared.lock().unwrap().busy = true;
    }
    set_status(&shared, &sender, "Đang đọc danh sách hồ sơ Wifi...");
    std::thread::spawn(move || {
        let profiles = list_profiles();
        let mut s = shared.lock().unwrap();
        s.profiles = Some(profiles);
        s.busy = false;
        drop(s);
        sender.notice();
    });
}

fn spawn_export(app: &Rc<App>, names: Vec<String>, dest: PathBuf) {
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let logger = make_logger(app);
    {
        shared.lock().unwrap().busy = true;
    }
    set_status(&shared, &sender, "Đang xuất hồ sơ Wifi...");
    std::thread::spawn(move || {
        logger.line(&format!("=== XUẤT {} hồ sơ Wifi -> {}", names.len(), dest.display()));
        let _ = std::fs::create_dir_all(&dest);
        let mut ok = 0usize;
        let mut fail = 0usize;
        for name in &names {
            let folder = format!("folder={}", dest.display());
            let name_arg = format!("name={}", name);
            match cmd::run_logged(&logger, "netsh.exe", &["wlan", "export", "profile", &name_arg, &folder, "key=clear"]) {
                Ok(out) if out.code == Some(0) => ok += 1,
                Ok(out) => {
                    fail += 1;
                    logger.line(&format!("  Lỗi xuất '{}': exit {:?}", name, out.code));
                }
                Err(e) => {
                    fail += 1;
                    logger.line(&format!("  Lỗi chạy netsh cho '{}': {}", name, e));
                }
            }
        }
        logger.line(&format!("=== XUẤT xong: thành công {}, lỗi {}", ok, fail));
        finish(&shared, &sender, &format!("Đã xuất {} hồ sơ vào:\n{}\nLỗi: {}", ok, dest.display(), fail));
    });
}

fn spawn_import(app: &Rc<App>, folder: PathBuf) {
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let logger = make_logger(app);
    {
        shared.lock().unwrap().busy = true;
    }
    set_status(&shared, &sender, "Đang nhập hồ sơ Wifi...");
    std::thread::spawn(move || {
        let mut xmls: Vec<PathBuf> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&folder) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().map(|x| x.eq_ignore_ascii_case("xml")).unwrap_or(false) {
                    xmls.push(p);
                }
            }
        }
        logger.line(&format!("=== NHẬP {} file hồ sơ từ {}", xmls.len(), folder.display()));
        let mut ok = 0usize;
        let mut fail = 0usize;
        for p in &xmls {
            let file_arg = format!("filename={}", p.display());
            match cmd::run_logged(&logger, "netsh.exe", &["wlan", "add", "profile", &file_arg, "user=all"]) {
                Ok(out) if out.code == Some(0) => ok += 1,
                Ok(out) => {
                    fail += 1;
                    logger.line(&format!("  Lỗi nhập {}: exit {:?}", p.display(), out.code));
                }
                Err(e) => {
                    fail += 1;
                    logger.line(&format!("  Lỗi chạy netsh cho {}: {}", p.display(), e));
                }
            }
        }
        logger.line(&format!("=== NHẬP xong: thành công {}, lỗi {}", ok, fail));
        // Nạp lại danh sách sau khi nhập
        let profiles = list_profiles();
        {
            let mut s = shared.lock().unwrap();
            s.profiles = Some(profiles);
        }
        finish(&shared, &sender, &format!("Đã nhập {} hồ sơ Wifi.\nLỗi: {}", ok, fail));
    });
}

fn spawn_show_keys(app: &Rc<App>, names: Vec<String>) {
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let logger = make_logger(app);
    {
        shared.lock().unwrap().busy = true;
    }
    set_status(&shared, &sender, "Đang đọc mật khẩu đã lưu...");
    std::thread::spawn(move || {
        logger.line(&format!("=== XEM MẬT KHẨU {} hồ sơ ===", names.len()));
        let tmp = std::env::temp_dir().join(format!("QSU-WifiKeys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let _ = std::fs::create_dir_all(&tmp);
        let folder_arg = format!("folder={}", tmp.display());
        for name in &names {
            let name_arg = format!("name={}", name);
            let _ = cmd::run_logged(&logger, "netsh.exe", &["wlan", "export", "profile", &name_arg, &folder_arg, "key=clear"]);
        }
        let mut rows: Vec<(String, String)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&tmp) {
            for e in rd.flatten() {
                let p = e.path();
                if !p.extension().map(|x| x.eq_ignore_ascii_case("xml")).unwrap_or(false) {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(&p) {
                    let pname = xml_tag_text(&content, "name").unwrap_or_default();
                    let key = xml_tag_text(&content, "keyMaterial")
                        .unwrap_or_else(|| "(mạng mở / không có mật khẩu)".to_string());
                    if !pname.is_empty() {
                        rows.push((pname, key));
                    }
                }
            }
        }
        rows.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        let _ = std::fs::remove_dir_all(&tmp);
        logger.line(&format!("Đã đọc {} mật khẩu; đã xoá thư mục tạm", rows.len()));
        {
            let mut s = shared.lock().unwrap();
            s.keys = Some(rows);
            s.busy = false;
        }
        sender.notice();
    });
}

fn pick_folder(title: &str) -> Option<PathBuf> {
    let mut dlg = Default::default();
    let built = nwg::FileDialog::builder()
        .title(title)
        .action(nwg::FileDialogAction::OpenDirectory)
        .build(&mut dlg);
    if built.is_err() {
        return None;
    }
    if dlg.run(None::<&nwg::Window>) {
        if let Ok(p) = dlg.get_selected_item() {
            return Some(PathBuf::from(p));
        }
    }
    None
}

fn build_ui(exe_dir: PathBuf) -> (nwg::Window, Rc<App>) {
    let f_normal = qsu_common::ui::font_normal();
    let f_small = qsu_common::ui::font_small();
    let f_button = qsu_common::ui::font_button();

    let mut window = Default::default();
    nwg::Window::builder()
        .size((870, 694))
        .position((190, 100))
        .title("Sao Lưu / Quản Lý Wifi")
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

    let (_t, _s) = qsu_common::ui::add_header(&window, "Sao Lưu / Quản Lý Wifi", "Sao lưu, khôi phục hồ sơ Wifi kèm mật khẩu và xem mật khẩu đã lưu");
    let _l1 = add_label("Hồ sơ Wifi đã lưu trên máy:", f_normal, (16, HEADER_H + 12), (300, 22));

    let mk_list = |pos: (i32, i32), size: (u32, u32), cols: &[(&str, i32)]| -> nwg::ListView {
        let mut lv = Default::default();
        nwg::ListView::builder()
            .parent(&window)
            .list_style(nwg::ListViewStyle::Detailed)
            .flags(nwg::ListViewFlags::VISIBLE | nwg::ListViewFlags::TAB_STOP | nwg::ListViewFlags::ALWAYS_SHOW_SELECTION)
            .ex_flags(nwg::ListViewExFlags::GRID | nwg::ListViewExFlags::FULL_ROW_SELECT)
            .build(&mut lv)
            .expect("listview");
        lv.set_headers_enabled(true);
        lv.set_position(pos.0, pos.1);
        lv.set_size(size.0, size.1);
        for (i, (name, w)) in cols.iter().enumerate() {
            lv.insert_column(nwg::InsertListViewColumn {
                index: Some(i as i32),
                text: Some(name.to_string()),
                width: Some(*w),
                ..Default::default()
            });
        }
        lv
    };

    let list = mk_list(
        (16, HEADER_H + 38),
        ((client_w - 32) as u32, 200),
        &[("Tên Wifi (SSID)", 300), ("File hồ sơ", 470)],
    );
    lv_enable_checkboxes(&list);

    let btn_y = HEADER_H + 38 + 200 + 10;
    let mk_btn = |text: &str, pos: (i32, i32), size: (u32, u32)| -> nwg::Button {
        let mut b = Default::default();
        nwg::Button::builder().text(text).font(Some(f_button)).parent(&window).build(&mut b).expect("button");
        b.set_position(pos.0, pos.1);
        b.set_size(size.0, size.1);
        b
    };
    let btn_refresh = mk_btn("Làm mới", (16, btn_y), (100, 32));
    let btn_all = mk_btn("Chọn tất cả", (124, btn_y), (110, 32));
    let btn_none = mk_btn("Bỏ chọn", (242, btn_y), (90, 32));
    let btn_export = mk_btn("Xuất hồ sơ", (340, btn_y), (120, 32));
    let btn_import = mk_btn("Nhập hồ sơ", (468, btn_y), (120, 32));
    let btn_keys = mk_btn("Xem mật khẩu", (596, btn_y), (140, 32));
    qsu_common::ui::make_primary(&btn_export);

    let keys_y = btn_y + 44;
    let _l2 = add_label("Mật khẩu đã lưu:", f_normal, (16, keys_y), (300, 22));
    let keys = mk_list(
        (16, keys_y + 26),
        ((client_w - 32) as u32, (client_h - (keys_y + 26) - 34) as u32),
        &[("Tên Wifi (SSID)", 300), ("Mật khẩu", 470)],
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
        keys,
        lbl_status,
        btn_refresh,
        btn_all,
        btn_none,
        btn_export,
        btn_import,
        btn_keys,
        profiles: RefCell::new(Vec::new()),
        checked: RefCell::new(Vec::new()),
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
                } else if handle == handler.btn_all.handle {
                    for i in 0..handler.profiles.borrow().len() {
                        lv_set_checked(&handler.list, i, true);
                    }
                    harvest_checks(&handler);
                } else if handle == handler.btn_none.handle {
                    for i in 0..handler.profiles.borrow().len() {
                        lv_set_checked(&handler.list, i, false);
                    }
                    harvest_checks(&handler);
                } else if handle == handler.btn_export.handle {
                    harvest_checks(&handler);
                    let names: Vec<String> = handler
                        .profiles
                        .borrow()
                        .iter()
                        .zip(handler.checked.borrow().iter())
                        .filter(|(_, c)| **c)
                        .map(|(p, _)| p.name.clone())
                        .collect();
                    if names.is_empty() {
                        info_box("Xuất hồ sơ Wifi", "Chưa chọn hồ sơ nào để xuất.");
                        return;
                    }
                    if let Some(dest) = pick_folder("Chọn thư mục lưu hồ sơ Wifi đã xuất") {
                        spawn_export(&handler, names, dest);
                    }
                } else if handle == handler.btn_import.handle {
                    if let Some(folder) = pick_folder("Chọn thư mục chứa file hồ sơ Wifi (.xml)") {
                        spawn_import(&handler, folder);
                    }
                } else if handle == handler.btn_keys.handle {
                    harvest_checks(&handler);
                    let names: Vec<String> = handler
                        .profiles
                        .borrow()
                        .iter()
                        .zip(handler.checked.borrow().iter())
                        .filter(|(_, c)| **c)
                        .map(|(p, _)| p.name.clone())
                        .collect();
                    if names.is_empty() {
                        info_box("Xem mật khẩu", "Hãy tick chọn ít nhất một hồ sơ Wifi để xem mật khẩu.");
                        return;
                    }
                    spawn_show_keys(&handler, names);
                }
            }
            _ => {}
        }
    });

    (window, app)
}

fn refresh(app: &Rc<App>) {
    let (profiles, keys, status, done) = {
        let mut s = app.shared.lock().unwrap();
        (s.profiles.take(), s.keys.take(), s.status.take(), s.done_msg.take())
    };
    if let Some(profiles) = profiles {
        let n = profiles.len();
        *app.checked.borrow_mut() = vec![true; n];
        *app.profiles.borrow_mut() = profiles;
        refill_list(app);
        app.lbl_status.set_text(&format!("Tìm thấy {} hồ sơ Wifi.", n));
    }
    if let Some(rows) = keys {
        refill_keys(app, &rows);
        app.lbl_status.set_text(&format!("Đã đọc mật khẩu của {} hồ sơ.", rows.len()));
    }
    if let Some(st) = status {
        app.lbl_status.set_text(&st);
    }
    if let Some(msg) = done {
        app.lbl_status.set_text("Sẵn sàng.");
        info_box("Sao Lưu Wifi", &msg);
    }
    let busy = app.shared.lock().unwrap().busy;
    for b in [&app.btn_refresh, &app.btn_all, &app.btn_none, &app.btn_export, &app.btn_import, &app.btn_keys] {
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
    make_logger(&app).line("--- Khởi động Sao Lưu / Quản Lý Wifi ---");
    spawn_refresh(&app);
    nwg::dispatch_thread_events();
}
