//! ui — design tokens + helper giao diện DÙNG CHUNG cho mọi cửa sổ
//! của bộ Utility Rebuild (chuẩn đầy đủ: docs/chuan_giao_dien.md).
//!
//! Nguyên tắc: giao diện sáng, 100% control Windows gốc qua NWG —
//! module KHÔNG tự khai báo lại font/màu/lề nữa, mọi giá trị lấy từ
//! module này để launcher và từng module trông như một sản phẩm.
//! Các số font dưới đây chính là bộ số đã chạy ổn định ở thực địa
//! từ v0.5 (Segoe UI 21/17/15/14 + Consolas 14), nay gom về một nơi.

use native_windows_gui as nwg;
use winapi::um::winuser::{GetWindowLongW, SetWindowLongW, BS_DEFPUSHBUTTON, GWL_STYLE};

// ---------------------------------------------------------------------------
// Token bố cục (đơn vị: pixel ở 96 DPI, NWG tự co giãn theo DPI hệ thống)
// ---------------------------------------------------------------------------
/// Lề trái/phải chuẩn của cửa sổ module.
pub const MARGIN: i32 = 16;
/// Khoảng cách chuẩn giữa hai control kề nhau.
pub const GAP: i32 = 8;
/// Chiều cao dải header chuẩn (tiêu đề + phụ đề), nội dung bắt đầu từ đây.
pub const HEADER_H: i32 = 72;
/// Vị trí/kích thước nhãn tiêu đề trong header chuẩn.
pub const TITLE_POS: (i32, i32) = (MARGIN, 8);
pub const TITLE_SIZE: (u32, u32) = (760, 30);
/// Vị trí/kích thước nhãn phụ đề trong header chuẩn.
pub const SUBTITLE_POS: (i32, i32) = (MARGIN, 40);
pub const SUBTITLE_SIZE: (u32, u32) = (960, 22);
/// Chiều cao chuẩn của nút bấm và ô nhập.
pub const BUTTON_H: u32 = 32;
pub const INPUT_H: u32 = 28;

// ---------------------------------------------------------------------------
// Token màu (chỉ dùng ở nơi NWG/Win32 cho phép đặt màu trên control gốc;
// nút bấm native không nhuộm màu được nếu không vẽ tay — xem chuẩn)
// ---------------------------------------------------------------------------
/// Màu nhấn hệ thống Windows (tham chiếu cho glyph/điểm nhấn).
pub const ACCENT: [u8; 3] = [0, 120, 215];
/// Màu mực của bộ icon launcher (thép xanh) — khớp draw-icons.py.
pub const GLYPH_INK: [u8; 3] = [40, 93, 145];
/// Màu nguy hiểm chuẩn cho nhãn cảnh báo phá huỷ (WinUI error red).
pub const DANGER: [u8; 3] = [196, 43, 28];

// ---------------------------------------------------------------------------
// Font chuẩn — mỗi font xây MỘT lần cho cả tiến trình (rò có chủ đích,
// sống cùng cửa sổ như quy ước sẵn có của các module).
// ---------------------------------------------------------------------------
macro_rules! std_font {
    ($name:ident, $cell:ident, $family:expr, $size:expr, $weight:expr) => {
        thread_local! {
            static $cell: &'static nwg::Font = {
                let mut f = Default::default();
                nwg::Font::builder()
                    .family($family)
                    .size($size)
                    .weight($weight)
                    .build(&mut f)
                    .expect("font chuan qsu_common::ui");
                Box::leak(Box::new(f))
            };
        }
        pub fn $name() -> &'static nwg::Font {
            $cell.with(|f| *f)
        }
    };
}

std_font!(font_title, F_TITLE, "Segoe UI", 21, 700);
std_font!(font_section, F_SECTION, "Segoe UI", 17, 700);
std_font!(font_normal, F_NORMAL, "Segoe UI", 15, 400);
std_font!(font_small, F_SMALL, "Segoe UI", 14, 400);
std_font!(font_button, F_BUTTON, "Segoe UI", 15, 700);
std_font!(font_mono, F_MONO, "Consolas", 14, 400);

// ---------------------------------------------------------------------------
// Header chuẩn: tiêu đề (font_title) + phụ đề một dòng (font_normal),
// đúng TITLE_POS / SUBTITLE_POS. Trả hai nhãn để module giữ sống.
// ---------------------------------------------------------------------------
pub fn add_header(
    window: &nwg::Window,
    title: &str,
    subtitle: &str,
) -> (nwg::Label, nwg::Label) {
    let mut lbl_title = Default::default();
    nwg::Label::builder()
        .text(title)
        .font(Some(font_title()))
        .parent(window)
        .build(&mut lbl_title)
        .expect("header title");
    lbl_title.set_position(TITLE_POS.0, TITLE_POS.1);
    lbl_title.set_size(TITLE_SIZE.0, TITLE_SIZE.1);

    let mut lbl_sub = Default::default();
    nwg::Label::builder()
        .text(subtitle)
        .font(Some(font_normal()))
        .parent(window)
        .build(&mut lbl_sub)
        .expect("header subtitle");
    lbl_sub.set_position(SUBTITLE_POS.0, SUBTITLE_POS.1);
    lbl_sub.set_size(SUBTITLE_SIZE.0, SUBTITLE_SIZE.1);
    (lbl_title, lbl_sub)
}

/// Nhãn thường trên cửa sổ chính theo font chỉ định (giảm boilerplate
/// cho module mới; module cũ giữ closure add_label riêng cũng tương
/// đương miễn là dùng font + toạ độ của module này).
pub fn add_label(
    window: &nwg::Window,
    text: &str,
    font: &nwg::Font,
    pos: (i32, i32),
    size: (u32, u32),
) -> nwg::Label {
    let mut lbl = Default::default();
    nwg::Label::builder()
        .text(text)
        .font(Some(font))
        .parent(window)
        .build(&mut lbl)
        .expect("label");
    lbl.set_position(pos.0, pos.1);
    lbl.set_size(size.0, size.1);
    lbl
}

// ---------------------------------------------------------------------------
// Icon cửa sổ dùng chung (assets/qsu.ico nhúng trong qsu_common):
// đặt cho MỌI cửa sổ chính để title bar + taskbar đồng nhất.
// ---------------------------------------------------------------------------
const QSU_ICO: &[u8] = include_bytes!("../assets/qsu.ico");

thread_local! {
    static APP_ICON: &'static nwg::Icon = {
        let icon = nwg::Icon::from_bin(QSU_ICO).expect("qsu.ico nhung trong qsu_common");
        Box::leak(Box::new(icon))
    };
}

pub fn set_window_icon(window: &nwg::Window) {
    APP_ICON.with(|icon| window.set_icon(Some(icon)));
}

// ---------------------------------------------------------------------------
// Nút hành động chính: giữ nguyên vẽ native, chỉ thêm viền đậm của
// nút mặc định (BS_DEFPUSHBUTTON) — cách Windows báo "nút chính".
// ---------------------------------------------------------------------------
pub fn make_primary(btn: &nwg::Button) {
    if let Some(hwnd) = btn.handle.hwnd() {
        unsafe {
            let style = GetWindowLongW(hwnd, GWL_STYLE);
            SetWindowLongW(hwnd, GWL_STYLE, style | BS_DEFPUSHBUTTON as i32);
        }
    }
}

// ---------------------------------------------------------------------------
// Hộp thoại chuẩn (cùng mẫu mọi module vẫn dùng, gom về một nơi)
// ---------------------------------------------------------------------------
pub fn info_box(title: &str, content: &str) {
    let p = nwg::MessageParams {
        title,
        content,
        buttons: nwg::MessageButtons::Ok,
        icons: nwg::MessageIcons::Info,
    };
    nwg::message(&p);
}

pub fn error_box(title: &str, content: &str) {
    let p = nwg::MessageParams {
        title,
        content,
        buttons: nwg::MessageButtons::Ok,
        icons: nwg::MessageIcons::Error,
    };
    nwg::message(&p);
}

/// Hỏi xác nhận Yes/No với icon cảnh báo; true = người dùng chọn Yes.
pub fn confirm_box(title: &str, content: &str) -> bool {
    let p = nwg::MessageParams {
        title,
        content,
        buttons: nwg::MessageButtons::YesNo,
        icons: nwg::MessageIcons::Warning,
    };
    nwg::message(&p) == nwg::MessageChoice::Yes
}
