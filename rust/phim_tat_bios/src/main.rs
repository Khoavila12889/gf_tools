#![windows_subsystem = "windows"]
//! PhimTatBIOS — bảng tra cứu phím tắt vào BIOS/UEFI và Boot Menu theo
//! hãng máy (Dell, HP, Lenovo, Asus, Acer, MSI, Gigabyte, ASRock,
//! Apple, Microsoft Surface, Toshiba, Samsung, Sony VAIO, Fujitsu,
//! Intel NUC, LG). Module thuần thông tin: không cần quyền admin,
//! không thao tác hệ thống, dữ liệu nhúng sẵn trong mã nguồn.
//!
//! Cách dùng: ngay khi bật máy, nhấn liên tục phím tương ứng lúc logo
//! hãng vừa hiện. Có ô tìm kiếm lọc theo hãng (không phân biệt dấu).

use std::rc::Rc;

use native_windows_gui as nwg;
use unicode_normalization::UnicodeNormalization;
use winapi::shared::windef::RECT;
use winapi::um::winuser::GetClientRect;

// ---------------------------------------------------------------------------
// Kích thước bố cục
// ---------------------------------------------------------------------------
const HEADER_H: i32 = 66;

// ---------------------------------------------------------------------------
// Dữ liệu phím tắt theo hãng (phím phổ biến, đã được cộng đồng kỹ thuật
// kiểm chứng rộng rãi): (Hãng, Vào BIOS/UEFI, Boot Menu, Ghi chú)
// ---------------------------------------------------------------------------
const HOTKEYS: &[(&str, &str, &str, &str)] = &[
    ("Dell", "F2", "F12", "Nhấn liên tục ngay khi logo Dell hiện"),
    ("HP", "F10 (hoặc Esc → F10)", "F9 (hoặc Esc → F9)", "Nhấn Esc mở Startup Menu, rồi chọn F9/F10"),
    ("Lenovo", "F2 / Fn+F2 (ThinkPad: F1)", "F12 / Fn+F12", "Một số IdeaPad có nút Novo cạnh nút nguồn"),
    ("Asus", "F2 / Del", "F8 / Esc", "Giữ phím F2 rồi bật máy nếu máy bật Fast Boot"),
    ("Acer", "F2 / Del", "F12", "Một số dòng phải bật 'F12 Boot Menu' trong BIOS trước"),
    ("MSI", "Del", "F11", ""),
    ("Gigabyte", "Del / F2", "F12", ""),
    ("ASRock", "F2 / Del", "F11", ""),
    ("Apple (Mac Intel)", "Giữ Option (⌥)", "Giữ Option (⌥)", "Mac Apple Silicon: giữ nút nguồn để vào Startup Options"),
    ("Microsoft Surface", "Giữ Tăng âm lượng + Nguồn", "Giữ Giảm âm lượng + Nguồn", "Vào UEFI: tắt máy, giữ Tăng âm lượng rồi bấm Nguồn"),
    ("Toshiba", "F2", "F12", "Một số dòng cũ: Esc rồi F1"),
    ("Samsung", "F2", "F10 / Esc", ""),
    ("Sony VAIO", "F2", "F11", "Hoặc bấm nút ASSIST khi máy đang tắt"),
    ("Fujitsu", "F2", "F12", ""),
    ("Intel NUC", "F2", "F10", ""),
    ("LG", "F2", "F10", ""),
];

fn unaccented(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.nfd() {
        match ch {
            'đ' => out.push('d'),
            'Đ' => out.push('D'),
            c if ('\u{0300}'..='\u{036f}').contains(&c) => {}
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Trạng thái ứng dụng
// ---------------------------------------------------------------------------
struct App {
    search: nwg::TextInput,
    list: nwg::ListView,
    lbl_count: nwg::Label,
}

fn refill_list(app: &App) {
    let query = unaccented(&app.search.text()).trim().to_lowercase();
    app.list.clear();
    let mut shown = 0usize;
    for (i, row) in HOTKEYS.iter().enumerate() {
        let hay = unaccented(&format!("{} {} {} {}", row.0, row.1, row.2, row.3)).to_lowercase();
        if !query.is_empty() && !hay.contains(&query) {
            continue;
        }
        for (c, text) in [row.0, row.1, row.2, row.3].iter().enumerate() {
            app.list.insert_item(nwg::InsertListViewItem {
                index: Some(shown as i32),
                column_index: c as i32,
                text: Some(text.to_string()),
                ..Default::default()
            });
        }
        let _ = i;
        shown += 1;
    }
    app.lbl_count.set_text(&format!("Hiển thị: {} / {} hãng", shown, HOTKEYS.len()));
}

fn build_ui() -> (nwg::Window, Rc<App>) {
    let f_normal = qsu_common::ui::font_normal();
    let f_small = qsu_common::ui::font_small();

    let mut window = Default::default();
    nwg::Window::builder()
        .size((800, 604))
        .position((220, 130))
        .title("Phím Tắt BIOS / Boot Menu")
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

    let (_t, _s) = qsu_common::ui::add_header(&window, "Phím Tắt BIOS / Boot Menu", "Tra cứu phím vào BIOS/UEFI và Boot Menu theo hãng máy");
    let _ls = add_label("Tìm hãng:", f_normal, (14, HEADER_H + 14), (76, 22));

    let mut search = Default::default();
    nwg::TextInput::builder().text("").font(Some(f_normal)).parent(&window).build(&mut search).expect("search");
    search.set_position(92, HEADER_H + 11);
    search.set_size(280, 26);

    let lbl_count = add_label("", f_small, (386, HEADER_H + 16), (300, 22));

    let mut list = Default::default();
    nwg::ListView::builder()
        .parent(&window)
        .list_style(nwg::ListViewStyle::Detailed)
        .flags(nwg::ListViewFlags::VISIBLE | nwg::ListViewFlags::TAB_STOP | nwg::ListViewFlags::ALWAYS_SHOW_SELECTION)
        .ex_flags(nwg::ListViewExFlags::GRID | nwg::ListViewExFlags::FULL_ROW_SELECT)
        .build(&mut list)
        .expect("listview");
    list.set_headers_enabled(true);
    list.set_position(14, HEADER_H + 48);
    list.set_size((client_w - 28) as u32, (client_h - HEADER_H - 48 - 34) as u32);
    let cols: [(&str, i32); 4] = [
        ("Hãng máy", 170),
        ("Vào BIOS / UEFI", 190),
        ("Boot Menu", 170),
        ("Ghi chú", 380),
    ];
    for (i, (name, w)) in cols.iter().enumerate() {
        list.insert_column(nwg::InsertListViewColumn {
            index: Some(i as i32),
            text: Some(name.to_string()),
            width: Some(*w),
            ..Default::default()
        });
    }

    let _note = add_label(
        "Cách dùng: bật máy, nhấn liên tục phím tương ứng ngay khi logo hãng vừa hiện.",
        f_small, (14, client_h - 28), (700, 22),
    );

    let state = Rc::new(App {
        search,
        list,
        lbl_count,
    });

    let handler_state = state.clone();
    nwg::full_bind_event_handler(&window.handle, move |evt, _data, handle| {
        if let nwg::Event::OnTextInput = evt {
            if handle == handler_state.search.handle {
                refill_list(&handler_state);
            }
        }
    });

    refill_list(&state);
    state.search.set_focus();
    (window, state)
}

fn main() {
    nwg::init().expect("Không khởi tạo được native-windows-gui");
    let (_window, _state) = build_ui();
    nwg::dispatch_thread_events();
}
