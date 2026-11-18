#![windows_subsystem = "windows"]
//! DonRacMayTinh — Dọn Rác Máy Tính.
//!
//! Quét và hiển thị dung lượng từng hạng mục trước, người dùng tick
//! chọn rồi mới dọn (có hộp xác nhận). Các hạng mục:
//!   - Temp của người dùng (%TEMP%) và Temp Windows (%WINDIR%\Temp)
//!   - Thùng rác (Recycle Bin trên mọi ổ đĩa)
//!   - Cache Windows Update (SoftwareDistribution\Download — tự dừng /
//!     chạy lại dịch vụ wuauserv trong lúc dọn)
//!   - Cache thumbnail + icon (Explorer)
//!   - Cache Google Chrome / Microsoft Edge (mọi hồ sơ người dùng)
//!   - File Delivery Optimization
//! TUYỆT ĐỐI không đụng vào Downloads/Documents hay dữ liệu cá nhân:
//! danh sách đường dẫn là cố định, chỉ xoá NỘI DUNG thư mục cache.
//! Mọi thao tác ghi vào DonRac_Log.txt cạnh file exe.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use native_windows_gui as nwg;
use qsu_common::log::Logger;
use qsu_common::svc;
use qsu_common::sys;
use winapi::shared::minwindef::{LPARAM, UINT, WPARAM};
use winapi::shared::windef::{HWND, RECT};
use winapi::um::commctrl::LVITEMW;
use winapi::um::shellapi::SHEmptyRecycleBinW;
use winapi::um::winuser::{
    GetClientRect, GetWindowLongPtrW, InvalidateRect, SendMessageW, SetWindowLongPtrW,
    ES_READONLY, GWL_STYLE,
};

const HEADER_H: i32 = 66;

fn fmt_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

// ---------------------------------------------------------------------------
// Hạng mục dọn dẹp
// ---------------------------------------------------------------------------
#[derive(Clone, PartialEq)]
enum CleanKind {
    /// Xoá toàn bộ nội dung các thư mục này (giữ lại thư mục gốc)
    DirContents,
    /// Chỉ xoá các file có tiền tố tên thuộc danh sách (trong 1 thư mục)
    FilteredFiles(Vec<String>),
    /// Thùng rác toàn hệ thống
    RecycleBin,
    /// Cache Windows Update: dừng wuauserv -> xoá -> chạy lại
    WuCache,
}

#[derive(Clone)]
struct CleanItem {
    name: String,
    display_path: String,
    kind: CleanKind,
    dirs: Vec<PathBuf>,
    size: u64,
    checked: bool,
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        if let Ok(md) = std::fs::metadata(&p) {
            if md.is_dir() {
                if let Ok(rd) = std::fs::read_dir(&p) {
                    for e in rd.flatten() {
                        stack.push(e.path());
                    }
                }
            } else {
                total += md.len();
            }
        }
    }
    total
}

/// Xoá nội dung thư mục (đệ quy), giữ thư mục gốc. Bỏ qua file đang bận.
/// QUAN TRỌNG: phân loại mục bằng `DirEntry::file_type` (KHÔNG đi theo
/// liên kết) thay vì `metadata` (đi theo liên kết): một junction/symlink
/// nằm trong %TEMP% trỏ sang thư mục dữ liệu thật phải bị xoá như một
/// LIÊN KẾT, tuyệt đối không đệ quy vào đích của nó.
fn delete_dir_contents(path: &Path, log: &Arc<Logger>) -> (u64, u64) {
    let mut files = 0u64;
    let mut failed = 0u64;
    let entries: Vec<(PathBuf, bool)> = match std::fs::read_dir(path) {
        Ok(rd) => rd
            .flatten()
            .map(|e| {
                let is_real_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                (e.path(), is_real_dir)
            })
            .collect(),
        Err(_) => return (0, 0),
    };
    for (entry, is_dir) in entries {
        let r = if is_dir {
            std::fs::remove_dir_all(&entry)
        } else {
            // File thường hoặc symlink/junction: chỉ xoá chính liên kết
            std::fs::remove_file(&entry)
        };
        match r {
            Ok(()) => files += 1,
            Err(e) => {
                failed += 1;
                log.line(&format!("  Bỏ qua (đang dùng/không xoá được): {} — {}", entry.display(), e));
            }
        }
    }
    (files, failed)
}

fn env_path(var: &str) -> PathBuf {
    PathBuf::from(std::env::var(var).unwrap_or_default())
}

fn browser_cache_dirs(base: &Path) -> Vec<PathBuf> {
    // base = ...\<Browser>\User Data ; duyệt các hồ sơ Default / Profile *
    let mut dirs = Vec::new();
    if let Ok(rd) = std::fs::read_dir(base) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if name == "Default" || name == "Guest Profile" || name.starts_with("Profile ") {
                for sub in ["Cache\\Cache_Data", "Code Cache", "GPUCache"] {
                    let d = p.join(sub);
                    if d.exists() {
                        dirs.push(d);
                    }
                }
            }
        }
    }
    dirs
}

fn scan_items() -> Vec<CleanItem> {
    let windir = env_path("WINDIR");
    let local = env_path("LOCALAPPDATA");
    let mut items: Vec<CleanItem> = Vec::new();
    // `default_on`: chỉ tick sẵn các mục an toàn (Temp, cache thumbnail,
    // Delivery Optimization). Thùng rác, cache Windows Update và cache
    // trình duyệt mặc định KHÔNG tick — người dùng chủ động chọn.
    let mut push = |name: &str, display: String, kind: CleanKind, dirs: Vec<PathBuf>, size: u64, default_on: bool| {
        items.push(CleanItem {
            name: name.to_string(),
            display_path: display,
            kind,
            dirs,
            size,
            checked: default_on && size > 0,
        });
    };

    // Temp người dùng
    let temp = env_path("TEMP");
    if !temp.as_os_str().is_empty() {
        let size = dir_size(&temp);
        push("Temp của người dùng", temp.display().to_string(), CleanKind::DirContents, vec![temp], size, true);
    }
    // Temp Windows
    let wtemp = windir.join("Temp");
    let size = dir_size(&wtemp);
    push("Temp của Windows", wtemp.display().to_string(), CleanKind::DirContents, vec![wtemp], size, true);
    // Thùng rác: cộng dung lượng $Recycle.Bin trên mọi ổ cố định
    let mut rb_size = 0u64;
    for d in sys::list_fixed_drives() {
        let rb = PathBuf::from(format!("{}:\\$Recycle.Bin", d.letter));
        if rb.exists() {
            rb_size += dir_size(&rb);
        }
    }
    push("Thùng rác (Recycle Bin)", "Mọi ổ đĩa".to_string(), CleanKind::RecycleBin, vec![], rb_size, false);
    // Cache Windows Update
    let wu = windir.join("SoftwareDistribution\\Download");
    let size = dir_size(&wu);
    push("Cache Windows Update", wu.display().to_string(), CleanKind::WuCache, vec![wu], size, false);
    // Cache thumbnail + icon
    let explorer = local.join("Microsoft\\Windows\\Explorer");
    let prefixes = vec!["thumbcache_".to_string(), "iconcache_".to_string()];
    let mut tc_size = 0u64;
    if let Ok(rd) = std::fs::read_dir(&explorer) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if prefixes.iter().any(|p| name.starts_with(p)) && name.ends_with(".db") {
                tc_size += e.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    push(
        "Cache hình thu nhỏ + icon",
        explorer.display().to_string(),
        CleanKind::FilteredFiles(prefixes),
        vec![explorer],
        tc_size,
        true,
    );
    // Cache Chrome
    let chrome_dirs = browser_cache_dirs(&local.join("Google\\Chrome\\User Data"));
    let size: u64 = chrome_dirs.iter().map(|d| dir_size(d)).sum();
    push(
        "Cache Google Chrome",
        local.join("Google\\Chrome\\User Data").display().to_string(),
        CleanKind::DirContents,
        chrome_dirs,
        size,
        false,
    );
    // Cache Edge
    let edge_dirs = browser_cache_dirs(&local.join("Microsoft\\Edge\\User Data"));
    let size: u64 = edge_dirs.iter().map(|d| dir_size(d)).sum();
    push(
        "Cache Microsoft Edge",
        local.join("Microsoft\\Edge\\User Data").display().to_string(),
        CleanKind::DirContents,
        edge_dirs,
        size,
        false,
    );
    // Delivery Optimization
    let do_dir = windir.join("SoftwareDistribution\\DeliveryOptimization");
    if do_dir.exists() {
        let size = dir_size(&do_dir);
        push("File Delivery Optimization", do_dir.display().to_string(), CleanKind::DirContents, vec![do_dir], size, true);
    }
    items
}

// ---------------------------------------------------------------------------
// Trạng thái chia sẻ luồng nền -> giao diện
// ---------------------------------------------------------------------------
#[derive(Default)]
struct Shared {
    log_lines: VecDeque<String>,
    scan: Option<Vec<CleanItem>>,
    status: Option<String>,
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

// ---------------------------------------------------------------------------
// Hộp thoại
// ---------------------------------------------------------------------------
fn info_box(title: &str, content: &str) {
    let p = nwg::MessageParams { title, content, buttons: nwg::MessageButtons::Ok, icons: nwg::MessageIcons::Info };
    nwg::message(&p);
}

fn confirm_box(title: &str, content: &str) -> bool {
    let p = nwg::MessageParams { title, content, buttons: nwg::MessageButtons::YesNo, icons: nwg::MessageIcons::Question };
    nwg::message(&p) == nwg::MessageChoice::Yes
}

// ---------------------------------------------------------------------------
// ListView checkbox (qua thông điệp Win32 thô)
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
    lbl_status: nwg::Label,
    lbl_total: nwg::Label,
    txt_log: nwg::TextBox,
    btn_scan: nwg::Button,
    btn_all: nwg::Button,
    btn_none: nwg::Button,
    btn_clean: nwg::Button,
    items: RefCell<Vec<CleanItem>>,
    shared: SharedRef,
    exe_dir: PathBuf,
    log_rendered: RefCell<usize>,
}

/// Logger ghi DonRac_Log.txt cạnh exe, đồng thời đẩy từng dòng lên
/// vùng log của giao diện qua NWG Notice.
fn make_logger(app: &Rc<App>) -> Arc<Logger> {
    let path = app.exe_dir.join("DonRac_Log.txt");
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let sink = Box::new(move |line: &str| {
        push_log(&shared, &sender, line);
    });
    Arc::new(Logger::new(path, Some(sink)))
}

fn harvest_checks(app: &Rc<App>) {
    let mut items = app.items.borrow_mut();
    for (i, it) in items.iter_mut().enumerate() {
        it.checked = lv_item_checked(&app.list, i);
    }
}

fn refill_list(app: &Rc<App>) {
    app.list.clear();
    let items = app.items.borrow();
    for (i, it) in items.iter().enumerate() {
        lv_insert_row(
            &app.list,
            i as i32,
            &[
                it.name.clone(),
                fmt_size(it.size),
                it.display_path.clone(),
            ],
        );
        lv_set_checked(&app.list, i, it.checked);
    }
    let total: u64 = items.iter().filter(|i| i.checked).map(|i| i.size).sum();
    app.lbl_total.set_text(&format!("Đã chọn: {}", fmt_size(total)));
}

fn spawn_scan(app: &Rc<App>) {
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let logger = make_logger(app);
    {
        let mut s = shared.lock().unwrap();
        s.busy = true;
    }
    set_status(&shared, &sender, "Đang quét dung lượng rác...");
    std::thread::spawn(move || {
        logger.line("--- Quét dung lượng các hạng mục dọn rác ---");
        let items = scan_items();
        for it in &items {
            logger.line(&format!("{}: {} ({})", it.name, fmt_size(it.size), it.display_path));
        }
        let mut s = shared.lock().unwrap();
        s.scan = Some(items);
        s.busy = false;
        drop(s);
        sender.notice();
    });
}

fn spawn_clean(app: &Rc<App>, chosen: Vec<CleanItem>) {
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let logger = make_logger(app);
    {
        let mut s = shared.lock().unwrap();
        s.busy = true;
    }
    std::thread::spawn(move || {
        logger.line(&format!("=== DỌN RÁC: {} hạng mục ===", chosen.len()));
        let mut total_freed = 0u64;
        for it in &chosen {
            set_status(&shared, &sender, &format!("Đang dọn: {}...", it.name));
            logger.line(&format!("Dọn: {} ({})", it.name, it.display_path));
            let freed = match &it.kind {
                CleanKind::RecycleBin => {
                    let rc = unsafe {
                        SHEmptyRecycleBinW(
                            std::ptr::null_mut(),
                            std::ptr::null(),
                            0x1 | 0x2 | 0x4, // NOCONFIRMATION | NOPROGRESSUI | NOSOUND
                        )
                    };
                    logger.line(&format!("  SHEmptyRecycleBin -> HRESULT 0x{:08x}", rc));
                    // Chỉ tính là đã giải phóng khi API báo thành công
                    // (S_OK = 0); thất bại thì không cộng vào tổng.
                    if rc == 0 {
                        it.size
                    } else {
                        0
                    }
                }
                CleanKind::WuCache => {
                    let was_running = svc::query("wuauserv").state == svc::STATE_RUNNING;
                    if was_running {
                        match svc::stop("wuauserv") {
                            Ok(()) => logger.line("  Đã dừng dịch vụ wuauserv"),
                            Err(e) => logger.line(&format!("  Lỗi dừng wuauserv: {}", e)),
                        }
                    }
                    let mut f = 0u64;
                    for d in &it.dirs {
                        let before = dir_size(d);
                        let (n, fail) = delete_dir_contents(d, &logger);
                        let after = dir_size(d);
                        logger.line(&format!("  {}: xoá {} mục, {} lỗi", d.display(), n, fail));
                        f += before.saturating_sub(after);
                    }
                    if was_running {
                        match svc::start("wuauserv") {
                            Ok(()) => logger.line("  Đã chạy lại dịch vụ wuauserv"),
                            Err(e) => logger.line(&format!("  Lỗi chạy lại wuauserv: {}", e)),
                        }
                    }
                    f
                }
                CleanKind::FilteredFiles(prefixes) => {
                    let mut f = 0u64;
                    for d in &it.dirs {
                        if let Ok(rd) = std::fs::read_dir(d) {
                            for e in rd.flatten() {
                                let name = e.file_name().to_string_lossy().to_lowercase();
                                if prefixes.iter().any(|p| name.starts_with(p)) && name.ends_with(".db") {
                                    let len = e.metadata().map(|m| m.len()).unwrap_or(0);
                                    match std::fs::remove_file(e.path()) {
                                        Ok(()) => f += len,
                                        Err(err) => logger.line(&format!("  Bỏ qua {}: {}", e.path().display(), err)),
                                    }
                                }
                            }
                        }
                    }
                    f
                }
                CleanKind::DirContents => {
                    let mut f = 0u64;
                    for d in &it.dirs {
                        let before = dir_size(d);
                        let (n, fail) = delete_dir_contents(d, &logger);
                        let after = dir_size(d);
                        logger.line(&format!("  {}: xoá {} mục, {} lỗi", d.display(), n, fail));
                        f += before.saturating_sub(after);
                    }
                    f
                }
            };
            logger.line(&format!("  -> Giải phóng {}", fmt_size(freed)));
            total_freed += freed;
        }
        logger.line(&format!("=== XONG: tổng giải phóng {} ===", fmt_size(total_freed)));
        {
            let mut s = shared.lock().unwrap();
            s.busy = false;
            s.status = Some(format!("Dọn xong — đã giải phóng {}. Đang quét lại...", fmt_size(total_freed)));
        }
        sender.notice();
        // Quét lại để cập nhật dung lượng còn lại
        let items = scan_items();
        let mut s = shared.lock().unwrap();
        s.scan = Some(items);
        drop(s);
        sender.notice();
    });
}

fn build_ui(exe_dir: PathBuf) -> (nwg::Window, Rc<App>) {
    let f_normal = qsu_common::ui::font_normal();
    let f_mono = qsu_common::ui::font_mono();
    let f_button = qsu_common::ui::font_button();

    let mut window = Default::default();
    nwg::Window::builder()
        .size((840, 664))
        .position((200, 110))
        .title("Dọn Rác Máy Tính")
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

    let (_t, _s) = qsu_common::ui::add_header(
        &window,
        "Dọn Rác Máy Tính",
        "Quét dung lượng từng hạng mục, tick chọn rồi dọn — không đụng vào tài liệu cá nhân",
    );

    let mut list = Default::default();
    nwg::ListView::builder()
        .parent(&window)
        .list_style(nwg::ListViewStyle::Detailed)
        .flags(nwg::ListViewFlags::VISIBLE | nwg::ListViewFlags::TAB_STOP | nwg::ListViewFlags::ALWAYS_SHOW_SELECTION)
        .ex_flags(nwg::ListViewExFlags::GRID | nwg::ListViewExFlags::FULL_ROW_SELECT)
        .build(&mut list)
        .expect("listview");
    list.set_headers_enabled(true);
    list.set_position(14, HEADER_H + 12);
    list.set_size((client_w - 28) as u32, 288);
    for (i, (name, w)) in [("Hạng mục", 250), ("Dung lượng", 110), ("Đường dẫn", 420)].iter().enumerate() {
        list.insert_column(nwg::InsertListViewColumn {
            index: Some(i as i32),
            text: Some(name.to_string()),
            width: Some(*w),
            ..Default::default()
        });
    }
    lv_enable_checkboxes(&list);

    let btn_y = HEADER_H + 12 + 288 + 10;
    let mk_btn = |text: &str, pos: (i32, i32), size: (u32, u32)| -> nwg::Button {
        let mut b = Default::default();
        nwg::Button::builder().text(text).font(Some(f_button)).parent(&window).build(&mut b).expect("button");
        b.set_position(pos.0, pos.1);
        b.set_size(size.0, size.1);
        b
    };
    let btn_scan = mk_btn("Quét lại", (14, btn_y), (110, 32));
    let btn_all = mk_btn("Chọn tất cả", (132, btn_y), (120, 32));
    let btn_none = mk_btn("Bỏ chọn", (260, btn_y), (100, 32));
    let btn_clean = mk_btn("Dọn rác", (client_w - 164, btn_y), (150, 32));
    qsu_common::ui::make_primary(&btn_clean);
    let lbl_total = add_label("Đã chọn: 0 B", f_normal, (372, btn_y + 6), (220, 22));

    let lbl_status = add_label("Đang khởi tạo...", f_normal, (14, btn_y + 42), ((client_w - 28) as u32, 22));

    let mut txt_log = Default::default();
    nwg::TextBox::builder()
        .text("")
        .font(Some(f_mono))
        .parent(&window)
        .flags(nwg::TextBoxFlags::VISIBLE | nwg::TextBoxFlags::VSCROLL | nwg::TextBoxFlags::AUTOVSCROLL)
        .build(&mut txt_log)
        .expect("log");
    txt_log.set_position(14, btn_y + 68);
    txt_log.set_size((client_w - 28) as u32, (client_h - (btn_y + 68) - 10) as u32);
    set_edit_style(txt_log.handle.hwnd(), ES_READONLY as u32);

    let mut notice = Default::default();
    nwg::Notice::builder().parent(&window).build(&mut notice).expect("notice");

    let shared: SharedRef = Arc::new(Mutex::new(Shared::default()));
    let app = Rc::new(App {
        notice,
        list,
        lbl_status,
        lbl_total,
        txt_log,
        btn_scan,
        btn_all,
        btn_none,
        btn_clean,
        items: RefCell::new(Vec::new()),
        shared,
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
                if handler.shared.lock().unwrap().busy {
                    return;
                }
                if handle == handler.btn_scan.handle {
                    spawn_scan(&handler);
                } else if handle == handler.btn_all.handle {
                    for i in 0..handler.items.borrow().len() {
                        lv_set_checked(&handler.list, i, true);
                    }
                    harvest_checks(&handler);
                    update_total(&handler);
                } else if handle == handler.btn_none.handle {
                    for i in 0..handler.items.borrow().len() {
                        lv_set_checked(&handler.list, i, false);
                    }
                    harvest_checks(&handler);
                    update_total(&handler);
                } else if handle == handler.btn_clean.handle {
                    harvest_checks(&handler);
                    let chosen: Vec<CleanItem> = handler.items.borrow().iter().filter(|i| i.checked && i.size > 0).cloned().collect();
                    if chosen.is_empty() {
                        info_box("Dọn Rác", "Chưa chọn hạng mục nào có dữ liệu để dọn.");
                        return;
                    }
                    let total: u64 = chosen.iter().map(|i| i.size).sum();
                    let names = chosen.iter().map(|i| format!("• {} ({})", i.name, fmt_size(i.size))).collect::<Vec<_>>().join("\n");
                    if confirm_box(
                        "Xác nhận dọn rác",
                        &format!("Sẽ dọn {} hạng mục, tổng {}:\n\n{}\n\nTiếp tục?", chosen.len(), fmt_size(total), names),
                    ) {
                        spawn_clean(&handler, chosen);
                    }
                }
            }
            _ => {}
        }
    });

    (window, app)
}

fn update_total(app: &Rc<App>) {
    let total: u64 = app.items.borrow().iter().filter(|i| i.checked).map(|i| i.size).sum();
    app.lbl_total.set_text(&format!("Đã chọn: {}", fmt_size(total)));
}

fn refresh(app: &Rc<App>) {
    let (scan, status, new_logs) = {
        let mut s = app.shared.lock().unwrap();
        let logs: Vec<String> = s.log_lines.iter().skip(*app.log_rendered.borrow()).cloned().collect();
        (s.scan.take(), s.status.take(), logs)
    };
    if !new_logs.is_empty() {
        let mut rendered = app.log_rendered.borrow_mut();
        *rendered += new_logs.len();
        let mut text = app.txt_log.text();
        for l in &new_logs {
            text.push_str(l);
            text.push_str("\r\n");
        }
        app.txt_log.set_text(&text);
    }
    if let Some(items) = scan {
        *app.items.borrow_mut() = items;
        refill_list(app);
    }
    if let Some(st) = status {
        app.lbl_status.set_text(&st);
    }
    let busy = app.shared.lock().unwrap().busy;
    app.btn_scan.set_enabled(!busy);
    app.btn_clean.set_enabled(!busy);
    app.btn_all.set_enabled(!busy);
    app.btn_none.set_enabled(!busy);
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
    make_logger(&app).line("--- Khởi động Dọn Rác Máy Tính ---");
    spawn_scan(&app);
    nwg::dispatch_thread_events();
}
