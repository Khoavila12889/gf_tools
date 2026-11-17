#![windows_subsystem = "windows"]
//! TuyChinhWindows — Tuỳ Chỉnh Windows.
//!
//! Catalogue tinh chỉnh dạng dữ liệu (src/tweaks.rs, 49 tweak) biên
//! soạn theo chuẩn **Sophia Script for Windows** (farag2, MIT) và đối
//! chiếu winutil (MIT): mỗi tweak có ÁP DỤNG + HOÀN TÁC + dò trạng thái
//! thật. Nhóm thứ sáu liệt kê động các gói Appx tuyển chọn để gỡ an
//! toàn cho người dùng hiện tại (src/appx.rs).
//!
//! - Thao tác registry/dịch vụ bằng Win32 API gốc; schtasks/powercfg/
//!   PowerShell chỉ ở các điểm không có API công khai (ghi rõ trong
//!   engine.rs và appx.rs).
//! - Trước lần áp dụng đầu tiên của mỗi phiên, công cụ đề nghị tạo
//!   điểm khôi phục hệ thống; có nút tạo điểm khôi phục riêng.
//! - Mọi thao tác ghi vào TuyChinh_Log.txt cạnh file exe.

mod appx;
mod engine;
mod reg;
mod tweaks;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use native_windows_gui as nwg;
use qsu_common::log::Logger;
use qsu_common::sys;
use winapi::shared::minwindef::{LPARAM, UINT, WPARAM};
use winapi::shared::windef::RECT;
use winapi::um::commctrl::LVITEMW;
use winapi::um::winuser::{GetClientRect, SendMessageW};

use appx::AppxInfo;
use tweaks::{Category, TweakState, TWEAKS};

const HEADER_H: i32 = 66;
/// Chỉ số trong ListBox nhóm: 0 = Tất cả, 1..=5 = nhóm tweak, 6 = Appx.
const CAT_ALL: usize = 0;
const CAT_APPX: usize = 6;

/// Bỏ dấu tiếng Việt để tìm kiếm không phân biệt dấu.
fn unaccented(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        let rep = match ch {
            'à' | 'á' | 'ả' | 'ã' | 'ạ' | 'ă' | 'ằ' | 'ắ' | 'ẳ' | 'ẵ' | 'ặ' | 'â' | 'ầ' | 'ấ' | 'ẩ' | 'ẫ' | 'ậ' => 'a',
            'À' | 'Á' | 'Ả' | 'Ã' | 'Ạ' | 'Ă' | 'Ằ' | 'Ắ' | 'Ẳ' | 'Ẵ' | 'Ặ' | 'Â' | 'Ầ' | 'Ấ' | 'Ẩ' | 'Ẫ' | 'Ậ' => 'A',
            'è' | 'é' | 'ẻ' | 'ẽ' | 'ẹ' | 'ê' | 'ề' | 'ế' | 'ể' | 'ễ' | 'ệ' => 'e',
            'È' | 'É' | 'Ẻ' | 'Ẽ' | 'Ẹ' | 'Ê' | 'Ề' | 'Ế' | 'Ể' | 'Ễ' | 'Ệ' => 'E',
            'ì' | 'í' | 'ỉ' | 'ĩ' | 'ị' => 'i',
            'Ì' | 'Í' | 'Ỉ' | 'Ĩ' | 'Ị' => 'I',
            'ò' | 'ó' | 'ỏ' | 'õ' | 'ọ' | 'ô' | 'ồ' | 'ố' | 'ổ' | 'ỗ' | 'ộ' | 'ơ' | 'ờ' | 'ớ' | 'ở' | 'ỡ' | 'ợ' => 'o',
            'Ò' | 'Ó' | 'Ỏ' | 'Õ' | 'Ọ' | 'Ô' | 'Ồ' | 'Ố' | 'Ổ' | 'Ỗ' | 'Ộ' | 'Ơ' | 'Ờ' | 'Ớ' | 'Ở' | 'Ỡ' | 'Ợ' => 'O',
            'ù' | 'ú' | 'ủ' | 'ũ' | 'ụ' | 'ư' | 'ừ' | 'ứ' | 'ử' | 'ữ' | 'ự' => 'u',
            'Ù' | 'Ú' | 'Ủ' | 'Ũ' | 'Ụ' | 'Ư' | 'Ừ' | 'Ứ' | 'Ử' | 'Ữ' | 'Ự' => 'U',
            'ỳ' | 'ý' | 'ỷ' | 'ỹ' | 'ỵ' => 'y',
            'Ỳ' | 'Ý' | 'Ỷ' | 'Ỹ' | 'Ỵ' => 'Y',
            'đ' => 'd',
            'Đ' => 'D',
            other => other,
        };
        out.push(rep);
    }
    out.to_lowercase()
}

/// Ngắt dòng thủ công cho nhãn mô tả (Label của NWG không tự wrap).
fn wrap_text(text: &str, width: usize, max_lines: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if cur.is_empty() {
            cur = word.to_string();
        } else if cur.chars().count() + 1 + word.chars().count() <= width {
            cur.push(' ');
            cur.push_str(word);
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
            if lines.len() == max_lines {
                break;
            }
        }
    }
    if lines.len() < max_lines && !cur.is_empty() {
        lines.push(cur);
    }
    if lines.len() == max_lines {
        let truncated = text.chars().count() > lines.join(" ").chars().count();
        if truncated {
            lines.last_mut().unwrap().push_str("…");
        }
    }
    lines.join("\r\n")
}

// ---------------------------------------------------------------------------
// Trạng thái chia sẻ giữa luồng UI và luồng nền
// ---------------------------------------------------------------------------
#[derive(Default)]
struct Shared {
    log_lines: VecDeque<String>,
    states: Option<Vec<TweakState>>,
    appx: Option<Result<Vec<AppxInfo>, String>>,
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
        while s.log_lines.len() > 600 {
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
    let p = nwg::MessageParams { title, content, buttons: nwg::MessageButtons::YesNo, icons: nwg::MessageIcons::Question };
    nwg::message(&p) == nwg::MessageChoice::Yes
}

// ---------------------------------------------------------------------------
// ListView checkbox (thông điệp Win32 thô, như DonRac)
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
#[derive(Clone, Copy, PartialEq)]
enum RowRef {
    Tweak(usize), // chỉ số trong TWEAKS
    Appx(usize),  // chỉ số trong danh sách appx đã nạp
}

struct App {
    notice: nwg::Notice,
    lst_cat: nwg::ListBox<String>,
    txt_search: nwg::TextBox,
    list: nwg::ListView,
    lbl_desc: nwg::Label,
    txt_log: nwg::TextBox,
    lbl_status: nwg::Label,
    btn_restore: nwg::Button,
    btn_refresh: nwg::Button,
    btn_about: nwg::Button,
    btn_apply_one: nwg::Button,
    btn_undo_one: nwg::Button,
    btn_apply_checked: nwg::Button,
    btn_undo_checked: nwg::Button,
    btn_remove_appx: nwg::Button,
    btn_check_all: nwg::Button,
    btn_uncheck_all: nwg::Button,
    shared: SharedRef,
    exe_dir: PathBuf,
    log_rendered: RefCell<usize>,
    states: RefCell<Vec<TweakState>>,
    appx_items: RefCell<Vec<AppxInfo>>,
    rows: RefCell<Vec<RowRef>>,
    cat_sel: RefCell<usize>,
    restore_prompted: RefCell<bool>,
    columns_appx: RefCell<bool>,
}

fn make_logger(app: &Rc<App>) -> Arc<Logger> {
    let path = app.exe_dir.join("TuyChinh_Log.txt");
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let sink = Box::new(move |line: &str| {
        push_log(&shared, &sender, line);
    });
    Arc::new(Logger::new(path, Some(sink)))
}

fn cat_of_index(sel: usize) -> Option<Category> {
    match sel {
        1..=5 => Some(Category::tweak_categories()[sel - 1]),
        _ => None,
    }
}

fn is_appx_mode(app: &Rc<App>) -> bool {
    *app.cat_sel.borrow() == CAT_APPX
}

/// Nạp lại toàn bộ trạng thái (49 probe + danh sách Appx) trên luồng nền.
fn spawn_probe_all(app: &Rc<App>) {
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let exe_dir = app.exe_dir.clone();
    let logger = make_logger(app);
    {
        shared.lock().unwrap().busy = true;
    }
    set_status(&shared, &sender, "Đang đọc trạng thái hệ thống...");
    std::thread::spawn(move || {
        let states: Vec<TweakState> = TWEAKS.iter().map(|t| engine::probe(t, &exe_dir)).collect();
        let appx_res = appx::list_installed(&logger);
        let mut s = shared.lock().unwrap();
        s.states = Some(states);
        s.appx = Some(appx_res);
        s.busy = false;
        drop(s);
        sender.notice();
    });
}

/// Áp dụng/hoàn tác một loạt tweak. `with_restore` = tạo điểm khôi phục
/// trước (lỗi tạo điểm khôi phục chỉ ghi log, vẫn tiếp tục theo lựa chọn
/// của người dùng).
fn spawn_apply(app: &Rc<App>, indices: Vec<usize>, undo: bool, with_restore: bool) {
    if indices.is_empty() {
        return;
    }
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let exe_dir = app.exe_dir.clone();
    let logger = make_logger(app);
    {
        shared.lock().unwrap().busy = true;
    }
    set_status(
        &shared,
        &sender,
        if undo { "Đang hoàn tác..." } else { "Đang áp dụng..." },
    );
    std::thread::spawn(move || {
        if with_restore {
            match engine::create_restore_point(&logger) {
                Ok(()) => {}
                Err(e) => logger.line(&format!("Không tạo được điểm khôi phục: {} — vẫn tiếp tục theo lựa chọn của bạn", e)),
            }
        }
        let mut ok_count = 0usize;
        let mut err_count = 0usize;
        for &i in &indices {
            let t = &TWEAKS[i];
            logger.line(&format!("=== {}: {} [{}] ===", if undo { "HOÀN TÁC" } else { "ÁP DỤNG" }, t.name, t.id));
            let ops = if undo { t.undo } else { t.apply };
            let (ok, bad, _) = engine::run_ops(ops, &exe_dir, &logger);
            if bad == 0 && ok > 0 {
                ok_count += 1;
            } else {
                err_count += 1;
            }
        }
        // Đọc lại toàn bộ trạng thái để giao diện khớp thực tế
        let states: Vec<TweakState> = TWEAKS.iter().map(|t| engine::probe(t, &exe_dir)).collect();
        let msg = if err_count == 0 {
            Some(format!(
                "Đã {} {} tinh chỉnh thành công. Một số tinh chỉnh cần đăng xuất/khởi động lại Explorer hoặc khởi động lại máy mới thấy hiệu quả.",
                if undo { "hoàn tác" } else { "áp dụng" },
                ok_count
            ))
        } else {
            None
        };
        let err = if err_count > 0 {
            Some(format!("Có {} tinh chỉnh lỗi khi {} — xem chi tiết trong nhật ký.", err_count, if undo { "hoàn tác" } else { "áp dụng" }))
        } else {
            None
        };
        {
            let mut s = shared.lock().unwrap();
            s.states = Some(states);
        }
        finish(&shared, &sender, msg, err);
    });
}

fn spawn_remove_appx(app: &Rc<App>, items: Vec<AppxInfo>) {
    if items.is_empty() {
        return;
    }
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let logger = make_logger(app);
    {
        shared.lock().unwrap().busy = true;
    }
    set_status(&shared, &sender, "Đang gỡ ứng dụng...");
    std::thread::spawn(move || {
        let mut ok_count = 0usize;
        let mut err_count = 0usize;
        for pkg in &items {
            match appx::remove(pkg, &logger) {
                Ok(()) => ok_count += 1,
                Err(e) => {
                    err_count += 1;
                    logger.line(&format!("  LỖI: {}", e));
                }
            }
        }
        let appx_res = appx::list_installed(&logger);
        let msg = if err_count == 0 {
            Some(format!("Đã gỡ {} ứng dụng. Các ứng dụng này vẫn cài lại được từ Microsoft Store.", ok_count))
        } else {
            None
        };
        let err = if err_count > 0 {
            Some(format!("Có {} ứng dụng gỡ thất bại — xem chi tiết trong nhật ký.", err_count))
        } else {
            None
        };
        {
            let mut s = shared.lock().unwrap();
            s.appx = Some(appx_res);
        }
        finish(&shared, &sender, msg, err);
    });
}

fn spawn_restore_point(app: &Rc<App>) {
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let logger = make_logger(app);
    {
        shared.lock().unwrap().busy = true;
    }
    set_status(&shared, &sender, "Đang tạo điểm khôi phục...");
    std::thread::spawn(move || {
        match engine::create_restore_point(&logger) {
            Ok(()) => finish(&shared, &sender, Some("Đã tạo điểm khôi phục hệ thống. Khi cần, dùng 'Khôi phục hệ thống' (rstrui) để quay lại.".to_string()), None),
            Err(e) => finish(&shared, &sender, None, Some(format!("Không tạo được điểm khôi phục: {}", e))),
        }
    });
}

/// Hỏi tạo điểm khôi phục trước lần áp dụng đầu tiên của phiên.
/// Trả None nếu người dùng huỷ thao tác.
fn ensure_restore_prompt(app: &Rc<App>) -> Option<bool> {
    if *app.restore_prompted.borrow() {
        return Some(false);
    }
    *app.restore_prompted.borrow_mut() = true;
    let p = nwg::MessageParams {
        title: "Tạo điểm khôi phục trước?",
        content: "Đây là lần áp dụng đầu tiên trong phiên này.\n\n\
            • Bấm CÓ (Yes): tạo điểm khôi phục hệ thống rồi mới áp dụng — an toàn nhất, mất thêm ít phút.\n\
            • Bấm KHÔNG (No): áp dụng ngay không tạo điểm khôi phục (mọi tinh chỉnh trong công cụ này đều hoàn tác được bằng nút Hoàn tác).\n\
            • Bấm HUỶ (Cancel): không làm gì cả.",
        buttons: nwg::MessageButtons::YesNoCancel,
        icons: nwg::MessageIcons::Question,
    };
    match nwg::message(&p) {
        nwg::MessageChoice::Yes => Some(true),
        nwg::MessageChoice::No => Some(false),
        _ => None,
    }
}

/// Các tweak index đang được tick trong danh sách hiện tại.
fn checked_tweaks(app: &Rc<App>) -> Vec<usize> {
    let rows = app.rows.borrow();
    rows.iter()
        .enumerate()
        .filter(|(r, _)| lv_item_checked(&app.list, *r))
        .filter_map(|(_, rr)| match rr {
            RowRef::Tweak(i) => Some(*i),
            RowRef::Appx(_) => None,
        })
        .collect()
}

fn checked_appx(app: &Rc<App>) -> Vec<AppxInfo> {
    let rows = app.rows.borrow();
    let items = app.appx_items.borrow();
    rows.iter()
        .enumerate()
        .filter(|(r, _)| lv_item_checked(&app.list, *r))
        .filter_map(|(_, rr)| match rr {
            RowRef::Appx(i) => items.get(*i).cloned(),
            RowRef::Tweak(_) => None,
        })
        .collect()
}

fn selected_tweak(app: &Rc<App>) -> Option<usize> {
    let sel = app.list.selected_item()?;
    match app.rows.borrow().get(sel) {
        Some(RowRef::Tweak(i)) => Some(*i),
        _ => None,
    }
}

fn update_desc(app: &Rc<App>) {
    let sel = app.list.selected_item();
    let text = match sel.and_then(|s| app.rows.borrow().get(s).copied()) {
        Some(RowRef::Tweak(i)) => {
            let t = &TWEAKS[i];
            format!("{} — {}", t.name, t.desc)
        }
        Some(RowRef::Appx(i)) => match app.appx_items.borrow().get(i) {
            Some(p) => format!(
                "{} (gói {}) — gỡ cho người dùng hiện tại, cài lại được từ Microsoft Store.{}",
                p.display,
                p.name,
                if p.risk == tweaks::Risk::Caution { " Mức 'Cần cân nhắc': kiểm tra người dùng có đang dùng ứng dụng này không trước khi gỡ." } else { "" }
            ),
            None => String::new(),
        },
        None => "Chọn một mục để xem mô tả chi tiết. Tick ô vuông để chọn nhiều mục rồi bấm Áp dụng/Hoàn tác hàng loạt; nhấp đúp một tinh chỉnh để áp dụng/hoàn tác ngay.".to_string(),
    };
    app.lbl_desc.set_text(&wrap_text(&text, 118, 3));
}

/// Dựng lại ListView theo nhóm + từ khoá tìm kiếm + trạng thái đã nạp.
fn rebuild_list(app: &Rc<App>) {
    let appx_mode = is_appx_mode(app);
    // Đổi tiêu đề cột theo chế độ
    if *app.columns_appx.borrow() != appx_mode {
        *app.columns_appx.borrow_mut() = appx_mode;
        let cols: [(&str, i32); 4] = if appx_mode {
            [("Ứng dụng", 300), ("Tên gói", 280), ("Phiên bản", 110), ("Mức", 110)]
        } else {
            [("Tên tinh chỉnh", 300), ("Trạng thái", 110), ("Mức", 110), ("Mô tả tóm tắt", 480)]
        };
        for (i, (name, w)) in cols.iter().enumerate() {
            app.list.update_column(
                i,
                nwg::InsertListViewColumn {
                    text: Some(name.to_string()),
                    width: Some(*w),
                    ..Default::default()
                },
            );
        }
    }

    let query = unaccented(&app.txt_search.text());
    let cat = cat_of_index(*app.cat_sel.borrow());
    let mut rows: Vec<RowRef> = Vec::new();
    app.list.clear();

    if appx_mode {
        let items = app.appx_items.borrow();
        for (i, p) in items.iter().enumerate() {
            if !query.is_empty()
                && !unaccented(&p.display).contains(&query)
                && !unaccented(&p.name).contains(&query)
            {
                continue;
            }
            let r = rows.len() as i32;
            lv_insert_row(&app.list, r, &[p.display.clone(), p.name.clone(), p.version.clone(), p.risk.label().to_string()]);
            rows.push(RowRef::Appx(i));
        }
    } else {
        let states = app.states.borrow();
        for (i, t) in TWEAKS.iter().enumerate() {
            if let Some(c) = cat {
                if t.category != c {
                    continue;
                }
            }
            if !query.is_empty()
                && !unaccented(t.name).contains(&query)
                && !unaccented(t.desc).contains(&query)
            {
                continue;
            }
            let state = states.get(i).copied().unwrap_or(TweakState::Unknown);
            let short_desc: String = t.desc.chars().take(90).collect();
            let r = rows.len() as i32;
            lv_insert_row(
                &app.list,
                r,
                &[t.name.to_string(), state.label().to_string(), t.risk.label().to_string(), short_desc],
            );
            rows.push(RowRef::Tweak(i));
        }
    }
    *app.rows.borrow_mut() = rows;

    // Bật/tắt các nút theo chế độ
    let busy = app.shared.lock().unwrap().busy;
    app.btn_apply_one.set_visible(!appx_mode);
    app.btn_undo_one.set_visible(!appx_mode);
    app.btn_apply_checked.set_visible(!appx_mode);
    app.btn_undo_checked.set_visible(!appx_mode);
    app.btn_remove_appx.set_visible(appx_mode);
    for b in [
        &app.btn_apply_one,
        &app.btn_undo_one,
        &app.btn_apply_checked,
        &app.btn_undo_checked,
        &app.btn_remove_appx,
        &app.btn_check_all,
        &app.btn_uncheck_all,
        &app.btn_refresh,
        &app.btn_restore,
    ] {
        b.set_enabled(!busy);
    }
    update_desc(app);
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
        .size((1080, 784))
        .position((120, 80))
        .title("Tuỳ Chỉnh Windows")
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

    let _t = add_label("Tuỳ Chỉnh Windows", f_title, (16, 8), (400, 30));
    let _s = add_label(
        "Catalogue tinh chỉnh theo chuẩn Sophia Script — mọi tinh chỉnh đều hoàn tác được về mặc định Windows",
        f_normal, (16, 40), (860, 22),
    );

    // --- Cột trái: nhóm ---
    let _lc = add_label("Nhóm:", f_normal, (14, HEADER_H + 10), (200, 22));
    let mut cat_items: Vec<String> = vec![format!("Tất cả tinh chỉnh ({})", TWEAKS.len())];
    for c in Category::tweak_categories() {
        let n = TWEAKS.iter().filter(|t| t.category == c).count();
        cat_items.push(format!("{} ({})", c.label(), n));
    }
    cat_items.push(Category::Appx.label().to_string());
    let mut lst_cat = Default::default();
    nwg::ListBox::builder()
        .parent(&window)
        .font(Some(f_normal))
        .collection(cat_items)
        .build(&mut lst_cat)
        .expect("listbox");
    lst_cat.set_position(14, HEADER_H + 36);
    lst_cat.set_size(212, 380);
    lst_cat.set_selection(Some(0));

    // --- Hàng trên phải: tìm kiếm + nút tiện ích ---
    let _ls = add_label("Tìm kiếm:", f_normal, (238, HEADER_H + 10), (90, 22));
    let mut txt_search = Default::default();
    nwg::TextBox::builder()
        .parent(&window)
        .font(Some(f_normal))
        .flags(nwg::TextBoxFlags::VISIBLE | nwg::TextBoxFlags::TAB_STOP)
        .build(&mut txt_search)
        .expect("search");
    txt_search.set_position(238, HEADER_H + 34);
    txt_search.set_size(330, 26);

    let mk_btn = |text: &str, pos: (i32, i32), size: (u32, u32)| -> nwg::Button {
        let mut b = Default::default();
        nwg::Button::builder().text(text).font(Some(f_button)).parent(&window).build(&mut b).expect("button");
        b.set_position(pos.0, pos.1);
        b.set_size(size.0, size.1);
        b
    };
    let btn_restore = mk_btn("Tạo điểm khôi phục", (580, HEADER_H + 32), (168, 32));
    let btn_refresh = mk_btn("Làm mới trạng thái", (756, HEADER_H + 32), (150, 32));
    let btn_about = mk_btn("Giới thiệu", (914, HEADER_H + 32), (110, 32));

    // --- Danh sách chính ---
    let list_x = 238;
    let list_y = HEADER_H + 68;
    let list_w = (client_w - list_x - 14) as u32;
    let list_h = 318u32;
    let mut list = Default::default();
    nwg::ListView::builder()
        .parent(&window)
        .list_style(nwg::ListViewStyle::Detailed)
        .flags(nwg::ListViewFlags::VISIBLE | nwg::ListViewFlags::TAB_STOP | nwg::ListViewFlags::ALWAYS_SHOW_SELECTION)
        .ex_flags(nwg::ListViewExFlags::GRID | nwg::ListViewExFlags::FULL_ROW_SELECT)
        .build(&mut list)
        .expect("listview");
    list.set_headers_enabled(true);
    list.set_position(list_x, list_y);
    list.set_size(list_w, list_h);
    for (i, (name, w)) in [("Tên tinh chỉnh", 300), ("Trạng thái", 110), ("Mức", 110), ("Mô tả tóm tắt", 480)].iter().enumerate() {
        list.insert_column(nwg::InsertListViewColumn {
            index: Some(i as i32),
            text: Some(name.to_string()),
            width: Some(*w),
            ..Default::default()
        });
    }
    lv_enable_checkboxes(&list);

    // --- Mô tả chi tiết ---
    let desc_y = list_y + list_h as i32 + 8;
    let lbl_desc = add_label(
        "Đang nạp...",
        f_small,
        (list_x, desc_y),
        (list_w, 48),
    );

    // --- Hàng nút thao tác ---
    let btn_y = desc_y + 54;
    let btn_apply_one = mk_btn("Áp dụng mục chọn", (list_x, btn_y), (150, 34));
    let btn_undo_one = mk_btn("Hoàn tác mục chọn", (list_x + 158, btn_y), (150, 34));
    let btn_apply_checked = mk_btn("Áp dụng mục đã tick", (list_x + 316, btn_y), (168, 34));
    let btn_undo_checked = mk_btn("Hoàn tác mục đã tick", (list_x + 492, btn_y), (168, 34));
    let btn_remove_appx = mk_btn("Gỡ các gói đã tick", (list_x, btn_y), (200, 34));
    btn_remove_appx.set_visible(false);
    let btn_check_all = mk_btn("Tick tất cả", (client_w - 196, btn_y), (86, 34));
    let btn_uncheck_all = mk_btn("Bỏ tick", (client_w - 102, btn_y), (86, 34));

    // --- Nhật ký ---
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
        lst_cat,
        txt_search,
        list,
        lbl_desc,
        txt_log,
        lbl_status,
        btn_restore,
        btn_refresh,
        btn_about,
        btn_apply_one,
        btn_undo_one,
        btn_apply_checked,
        btn_undo_checked,
        btn_remove_appx,
        btn_check_all,
        btn_uncheck_all,
        shared: Arc::new(Mutex::new(Shared::default())),
        exe_dir,
        log_rendered: RefCell::new(0),
        states: RefCell::new(vec![TweakState::Unknown; TWEAKS.len()]),
        appx_items: RefCell::new(Vec::new()),
        rows: RefCell::new(Vec::new()),
        cat_sel: RefCell::new(CAT_ALL),
        restore_prompted: RefCell::new(false),
        columns_appx: RefCell::new(false),
    });

    let handler = app.clone();
    nwg::full_bind_event_handler(&window.handle, move |evt, _data, handle| {
        match evt {
            nwg::Event::OnNotice => {
                if handle == handler.notice.handle {
                    refresh(&handler);
                }
            }
            nwg::Event::OnListBoxSelect => {
                if handle == handler.lst_cat.handle {
                    if let Some(sel) = handler.lst_cat.selection() {
                        *handler.cat_sel.borrow_mut() = sel;
                        rebuild_list(&handler);
                    }
                }
            }
            nwg::Event::OnTextInput => {
                if handle == handler.txt_search.handle {
                    rebuild_list(&handler);
                }
            }
            nwg::Event::OnListViewClick | nwg::Event::OnListViewItemChanged => {
                if handle == handler.list.handle {
                    update_desc(&handler);
                }
            }
            nwg::Event::OnListViewDoubleClick => {
                if handle == handler.list.handle && !is_appx_mode(&handler) {
                    if handler.shared.lock().unwrap().busy {
                        return;
                    }
                    if let Some(i) = selected_tweak(&handler) {
                        let state = handler.states.borrow().get(i).copied().unwrap_or(TweakState::Unknown);
                        let undo = state == TweakState::Applied;
                        if !undo {
                            match ensure_restore_prompt(&handler) {
                                Some(with_restore) => spawn_apply(&handler, vec![i], false, with_restore),
                                None => {}
                            }
                        } else {
                            spawn_apply(&handler, vec![i], true, false);
                        }
                    }
                }
            }
            nwg::Event::OnButtonClick => {
                if handle == handler.btn_about.handle {
                    info_box(
                        "Tuỳ Chỉnh Windows — Giới thiệu",
                        "Công cụ tinh chỉnh Windows thuộc bộ Utility Rebuild.\n\n\
                        Danh mục tinh chỉnh được biên soạn theo catalogue của Sophia Script for Windows \
                        (tác giả farag2 — giấy phép MIT, https://github.com/farag2/Sophia-Script-for-Windows) \
                        và đối chiếu với winutil (ChrisTitusTech, MIT). Mô tả tiếng Việt do dự án tự viết; \
                        mỗi tinh chỉnh đều có thao tác hoàn tác về mặc định Windows.\n\n\
                        Công cụ KHÔNG có cổng key bản quyền, KHÔNG telemetry, KHÔNG tự cài chứng chỉ gốc, \
                        KHÔNG thêm ngoại lệ Defender.",
                    );
                    return;
                }
                if handler.shared.lock().unwrap().busy {
                    return;
                }
                if handle == handler.btn_refresh.handle {
                    spawn_probe_all(&handler);
                } else if handle == handler.btn_restore.handle {
                    if confirm_box(
                        "Tạo điểm khôi phục",
                        "Tạo điểm khôi phục hệ thống ngay bây giờ?\n\nĐiểm khôi phục cho phép quay lại trạng thái hiện tại bằng 'Khôi phục hệ thống' (rstrui) nếu tinh chỉnh gây vấn đề.",
                    ) {
                        spawn_restore_point(&handler);
                    }
                } else if handle == handler.btn_check_all.handle {
                    for r in 0..handler.rows.borrow().len() {
                        lv_set_checked(&handler.list, r, true);
                    }
                } else if handle == handler.btn_uncheck_all.handle {
                    for r in 0..handler.rows.borrow().len() {
                        lv_set_checked(&handler.list, r, false);
                    }
                } else if handle == handler.btn_apply_one.handle {
                    if let Some(i) = selected_tweak(&handler) {
                        match ensure_restore_prompt(&handler) {
                            Some(with_restore) => spawn_apply(&handler, vec![i], false, with_restore),
                            None => {}
                        }
                    } else {
                        info_box("Tuỳ Chỉnh Windows", "Hãy chọn một tinh chỉnh trong danh sách trước.");
                    }
                } else if handle == handler.btn_undo_one.handle {
                    if let Some(i) = selected_tweak(&handler) {
                        spawn_apply(&handler, vec![i], true, false);
                    } else {
                        info_box("Tuỳ Chỉnh Windows", "Hãy chọn một tinh chỉnh trong danh sách trước.");
                    }
                } else if handle == handler.btn_apply_checked.handle {
                    let idx = checked_tweaks(&handler);
                    if idx.is_empty() {
                        info_box("Tuỳ Chỉnh Windows", "Chưa tick tinh chỉnh nào.");
                    } else {
                        match ensure_restore_prompt(&handler) {
                            Some(with_restore) => spawn_apply(&handler, idx, false, with_restore),
                            None => {}
                        }
                    }
                } else if handle == handler.btn_undo_checked.handle {
                    let idx = checked_tweaks(&handler);
                    if idx.is_empty() {
                        info_box("Tuỳ Chỉnh Windows", "Chưa tick tinh chỉnh nào.");
                    } else {
                        spawn_apply(&handler, idx, true, false);
                    }
                } else if handle == handler.btn_remove_appx.handle {
                    let items = checked_appx(&handler);
                    if items.is_empty() {
                        info_box("Tuỳ Chỉnh Windows", "Chưa tick ứng dụng nào.");
                    } else {
                        let names: Vec<String> = items.iter().map(|p| format!("• {}", p.display)).collect();
                        let text = format!(
                            "GỠ {} ứng dụng sau cho người dùng hiện tại?\n\n{}\n\nChỉ gỡ bản cài của người dùng này; cài lại được từ Microsoft Store.",
                            items.len(),
                            names.join("\n")
                        );
                        if confirm_box("Xác nhận gỡ ứng dụng", &text) {
                            spawn_remove_appx(&handler, items);
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
    let (states, appx, status, done, err, new_lines) = {
        let mut s = app.shared.lock().unwrap();
        let rendered = *app.log_rendered.borrow();
        let lines: Vec<String> = s.log_lines.iter().skip(rendered).cloned().collect();
        (s.states.take(), s.appx.take(), s.status.take(), s.done_msg.take(), s.err_msg.take(), lines)
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
    let mut need_rebuild = false;
    if let Some(st) = states {
        *app.states.borrow_mut() = st;
        need_rebuild = true;
    }
    if let Some(res) = appx {
        match res {
            Ok(items) => {
                *app.appx_items.borrow_mut() = items;
            }
            Err(e) => {
                make_logger(app).line(&format!("Không liệt kê được gói Appx: {}", e));
                *app.appx_items.borrow_mut() = Vec::new();
            }
        }
        need_rebuild = true;
    }
    if need_rebuild {
        rebuild_list(app);
        if !app.shared.lock().unwrap().busy {
            app.lbl_status.set_text("Sẵn sàng.");
        }
    }
    if let Some(st) = status {
        app.lbl_status.set_text(&st);
    }
    if let Some(msg) = done {
        app.lbl_status.set_text("Sẵn sàng.");
        rebuild_list(app);
        info_box("Tuỳ Chỉnh Windows", &msg);
    }
    if let Some(msg) = err {
        app.lbl_status.set_text("Có lỗi xảy ra.");
        rebuild_list(app);
        error_box("Tuỳ Chỉnh Windows", &msg);
    }
    // Đồng bộ trạng thái bật/tắt nút theo busy
    let busy = app.shared.lock().unwrap().busy;
    for b in [
        &app.btn_apply_one,
        &app.btn_undo_one,
        &app.btn_apply_checked,
        &app.btn_undo_checked,
        &app.btn_remove_appx,
        &app.btn_check_all,
        &app.btn_uncheck_all,
        &app.btn_refresh,
        &app.btn_restore,
    ] {
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
    make_logger(&app).line("--- Khởi động Tuỳ Chỉnh Windows ---");
    spawn_probe_all(&app);
    nwg::dispatch_thread_events();
}
