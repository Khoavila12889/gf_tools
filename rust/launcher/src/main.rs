#![windows_subsystem = "windows"]
//! Utility Rebuild — cửa sổ chính (launcher) viết bằng Rust + native-windows-gui.
//!
//! Tương đương giao diện launcher PowerShell (launcher/Utility.ps1):
//! cửa sổ native chuẩn Windows, header (tiêu đề + thông tin OS / PC-Name / User),
//! danh mục và truy cập nhanh công cụ thường dùng, tìm kiếm không dấu,
//! mô tả công việc và trạng thái module thực tế trong gói.
//!
//! Bấm một nút chức năng sẽ chạy file modules\<TênModule>.exe nằm cạnh
//! launcher bằng quyền Administrator (ShellExecute "runas"); nếu module
//! chưa được build, hiện thông báo "chưa có module" thay vì báo lỗi.
//!
//! Đây là mã nguồn viết mới hoàn toàn (clean-room): không cơ chế kiểm
//! tra bản quyền, không telemetry, không tự thay đổi cấu hình bảo mật.
//!
//! Giao diện dùng màu hệ thống mặc định của Windows cho mọi control.

mod icons;
use std::cell::Cell;
use std::env;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr;
use std::rc::Rc;

use native_windows_gui as nwg;
use unicode_normalization::UnicodeNormalization;
use winapi::shared::minwindef::{DWORD, HKEY};
use winapi::shared::windef::RECT;
use winapi::um::shellapi::ShellExecuteW;
use winapi::um::winreg::{RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_LOCAL_MACHINE};
use winapi::um::winnt::KEY_READ;
use winapi::um::winuser::{GetClientRect, GetWindowLongW, SetWindowLongW, BS_MULTILINE, GWL_STYLE, SW_SHOWNORMAL};

// ---------------------------------------------------------------------------
// Kích thước bố cục
// ---------------------------------------------------------------------------
const HEADER_H: i32 = 220;
const FOOTER_H: i32 = 52;
const CATEGORIES: &[&str] = &["Thường dùng", "Tất cả", "Cài đặt", "Sao lưu", "Hệ thống", "Hỗ trợ"];
const FREQUENT: &[usize] = &[6, 7, 3, 24, 20, 23, 13, 18];
// Metadata theo thứ tự MODULES; giữ nguyên tên file module và cách khởi chạy.
const DETAILS: &[(&str, usize)] = &[
    ("Driver lưu trữ Intel", 2), ("Nguồn bộ cài Windows", 2),
    ("Sao lưu bản quyền Microsoft", 3), ("Sao lưu và khôi phục driver", 3),
    ("Giải mã ổ đĩa BitLocker", 4), ("Chuẩn bị môi trường cứu hộ", 2),
    ("Chuẩn bị và cài qua WinPE", 2), ("Cài trực tiếp trong WinPE", 2),
    ("Cài Office qua mạng", 2), ("Cài Office từ bộ cài có sẵn", 2),
    ("Quản lý kích hoạt bản quyền", 2), ("Cài ứng dụng cho máy", 2),
    ("Sao lưu dữ liệu cá nhân", 3), ("Lưu và quản lý mạng Wi-Fi", 3),
    ("Sao lưu dữ liệu trình duyệt", 3), ("Sao lưu và khôi phục Zalo", 3),
    ("Xem thông tin phần cứng", 4), ("Kiểm tra tình trạng máy", 5),
    ("Tra phím BIOS và Boot Menu", 5), ("Quản lý phân vùng ổ đĩa", 4),
    ("Tinh chỉnh và quản lý Appx", 4), ("Thiết lập Microsoft Office", 4),
    ("Quản lý bảo mật Windows", 4), ("Bật hoặc tắt cập nhật", 4),
    ("Dọn file tạm và bộ nhớ đệm", 4), ("Gỡ ứng dụng đã cài", 4),
    ("Khắc phục sự cố in ấn", 5), ("Thiết lập máy in", 5),
    ("Khắc phục chia sẻ nội bộ", 5), ("Thao tác tệp và thư mục", 5),
    ("Chuyển hệ điều hành Windows", 2), ("Sử dụng nhiều phiên Zalo", 5),
];

// ---------------------------------------------------------------------------
// Danh sách 32 chức năng: nhãn hiển thị -> file module .exe trong modules\
// (giữ đúng thứ tự và tên gốc của bản PowerShell, đổi đuôi .ps1 -> .exe)
// ---------------------------------------------------------------------------
struct ModuleDef {
    label: &'static str,
    file: &'static str,
}

const MODULES: &[ModuleDef] = &[
    ModuleDef { label: "Kho Driver IRST", file: "KhoDriverIRST.exe" },
    ModuleDef { label: "Kho Windows", file: "KhoWindows.exe" },
    ModuleDef { label: "Backup License MS", file: "BackupLicenseMS.exe" },
    ModuleDef { label: "Sao lưu / Khôi phục driver", file: "BackupRestoreDriver.exe" },
    ModuleDef { label: "Tắt Bitlocker", file: "TatBitlocker.exe" },
    ModuleDef { label: "Tạo Boot WinPE", file: "TaoBootWinPE.exe" },
    ModuleDef { label: "Cài Đặt Win Tự Động", file: "CaiDatWinTuDong.exe" },
    ModuleDef { label: "Cài Windows trong WinPE", file: "CaiDatWinWinPE.exe" },
    ModuleDef { label: "Cài Đặt Office Online", file: "CaiDatOfficeOnline.exe" },
    ModuleDef { label: "Cài Đặt Office OFFLINE", file: "CaiDatOfficeOffline.exe" },
    ModuleDef { label: "Kích Hoạt Win/Office", file: "KichHoatWinOffice.exe" },
    ModuleDef { label: "Cài Đặt Phần Mềm", file: "CaiDatPhanMem.exe" },
    ModuleDef { label: "Sao Lưu Dữ Liệu", file: "SaoLuuDuLieu.exe" },
    ModuleDef { label: "Sao Lưu/Quản lý Wifi", file: "SaoLuuWifi.exe" },
    ModuleDef { label: "Sao Lưu Chrome-Edge", file: "SaoLuuChromeEdge.exe" },
    ModuleDef { label: "Backup/Restore Zalo", file: "BackupRestoreZalo.exe" },
    ModuleDef { label: "Xem cấu hình máy tính", file: "CheckCauHinh.exe" },
    ModuleDef { label: "Check / Test", file: "CheckTest.exe" },
    ModuleDef { label: "Phím Tắt BIOS", file: "PhimTatBIOS.exe" },
    ModuleDef { label: "Phân Vùng Ổ Cứng", file: "PhanVungOCung.exe" },
    ModuleDef { label: "Tuỳ Chỉnh Windows", file: "TuyChinhWindows.exe" },
    ModuleDef { label: "Tuỳ Chỉnh Office", file: "TuyChinhOffice.exe" },
    ModuleDef { label: "Windows Security", file: "WindowsSecurity.exe" },
    ModuleDef { label: "Quản Lý Update Windows", file: "QuanLyUpdateWindows.exe" },
    ModuleDef { label: "Dọn Rác Máy Tính", file: "DonRacMayTinh.exe" },
    ModuleDef { label: "Gỡ Bỏ Phần Mềm", file: "GoBoPhanMem.exe" },
    ModuleDef { label: "Fix Lỗi Máy In", file: "FixLoiMayIn.exe" },
    ModuleDef { label: "Quản Lý Máy In", file: "QuanLyMayIn.exe" },
    ModuleDef { label: "Fix Share Mạng LAN", file: "FixShareMangLAN.exe" },
    ModuleDef { label: "Quản lý File/Folder", file: "QuanLyFileFolder.exe" },
    ModuleDef { label: "Chuyển Windows OS", file: "ChuyenWindowsOS.exe" },
    ModuleDef { label: "Multi Zalo PC", file: "MultiZaloPC.exe" },
];

// ---------------------------------------------------------------------------
// Trạng thái ứng dụng (giữ sống suốt vòng đời cửa sổ)
// ---------------------------------------------------------------------------
struct AppState {
    buttons: Vec<nwg::Button>,
    search: nwg::TextInput,
    btn_clear: nwg::Button,
    btn_about: nwg::Button,
    categories: Vec<nwg::Button>,
    category: Cell<usize>,
    summary: nwg::Label,
    _labels: Vec<nwg::Label>,
    window: nwg::ControlHandle,
    _icons: Vec<icons::ButtonIcon>,
}

// ---------------------------------------------------------------------------
// Tiện ích chuỗi / Win32
// ---------------------------------------------------------------------------
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_os(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Bỏ dấu tiếng Việt để tìm kiếm không phân biệt dấu ("tuy chinh" ~ "Tuỳ Chỉnh").
/// Tách ký tự theo NFD rồi bỏ các dấu kết hợp U+0300..U+036F;
/// 'đ'/'Đ' không tự tách nên xử lý riêng (giống bản PowerShell).
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

/// Đọc một giá trị REG_SZ trong HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion
fn reg_current_version(value: &str) -> Option<String> {
    unsafe {
        let subkey = wide("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
        let mut hkey: HKEY = ptr::null_mut();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey.as_ptr(), 0, KEY_READ, &mut hkey) != 0 {
            return None;
        }
        let name = wide(value);
        let mut buf = vec![0u16; 512];
        let mut len: DWORD = (buf.len() * 2) as DWORD;
        let mut typ: DWORD = 0;
        let rc = RegQueryValueExW(
            hkey,
            name.as_ptr(),
            ptr::null_mut(),
            &mut typ,
            buf.as_mut_ptr() as *mut u8,
            &mut len,
        );
        RegCloseKey(hkey);
        if rc != 0 {
            return None;
        }
        let n = (len as usize / 2).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..n]);
        Some(s.trim_end_matches('\0').trim().to_string())
    }
}

/// Chuỗi thông tin hệ điều hành ở header, ví dụ "Windows 11 Pro 24H2 (64-bit)".
/// (Registry ProductName của Windows 11 vẫn ghi "Windows 10" — sửa lại
/// theo số build >= 22000 cho đúng tên thương mại.)
fn os_caption() -> String {
    let mut product = reg_current_version("ProductName").unwrap_or_default();
    if let Some(stripped) = product.strip_prefix("Microsoft ") {
        product = stripped.to_string();
    }
    let build: u32 = reg_current_version("CurrentBuildNumber")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if build >= 22000 && product.starts_with("Windows 10") {
        product = product.replacen("Windows 10", "Windows 11", 1);
    }
    let ver = reg_current_version("DisplayVersion")
        .filter(|v| !v.is_empty())
        .or_else(|| reg_current_version("ReleaseId"))
        .unwrap_or_default();
    let bits = if cfg!(target_arch = "x86_64") { "64-bit" } else { "32-bit" };
    format!("{} {} ({})", product, ver, bits).trim().to_string()
}

/// Thư mục modules\ nằm cạnh file launcher.exe
fn modules_dir() -> PathBuf {
    let exe = env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    dir.join("modules")
}

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

/// Chạy một module với quyền Administrator; báo rõ nếu module chưa được build
fn start_module(index: usize) {
    let module = &MODULES[index];
    let dir = modules_dir();
    let path = dir.join(module.file);
    if !path.exists() {
        info_box(
            "Chưa có module",
            &format!(
                "Module '{}' chưa được xây dựng trong bản rebuild này.\n\nFile module dự kiến: modules\\{}",
                module.label, module.file
            ),
        );
        return;
    }
    let op = wide("runas");
    let file = wide_os(path.as_os_str());
    let workdir = wide_os(dir.as_os_str());
    let rc = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            ptr::null(),
            workdir.as_ptr(),
            SW_SHOWNORMAL,
        ) as isize
    };
    if rc <= 32 {
        error_box(
            "Lỗi",
            &format!(
                "Không chạy được module '{}'.\n(Mã lỗi ShellExecute: {})",
                module.label, rc
            ),
        );
    }
}

/// Lọc nút theo ô tìm kiếm (không phân biệt dấu)
fn apply_search_filter(state: &AppState) {
    let query = unaccented(&state.search.text()).trim().to_lowercase();
    let category = state.category.get();
    let mut rc: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetClientRect(state.window.hwnd().unwrap(), &mut rc); }
    let cell_w = (rc.right - 40) / 4;
    let cell_h = (rc.bottom - HEADER_H - FOOTER_H) / 8;
    let order: Vec<usize> = if query.is_empty() && category == 0 {
        FREQUENT.to_vec()
    } else { (0..MODULES.len()).collect() };
    for btn in &state.buttons { btn.set_visible(false); }
    let mut count = 0;
    for i in order {
        let visible = if query.is_empty() {
            category <= 1 || DETAILS[i].1 == category
        } else {
            let text = unaccented(&format!("{} {} {}", MODULES[i].label, DETAILS[i].0, CATEGORIES[DETAILS[i].1])).to_lowercase();
            query.split_whitespace().all(|word| text.contains(word))
        };
        if visible {
            let btn = &state.buttons[i];
            btn.set_position(20 + (count % 4) * cell_w, HEADER_H + (count / 4) * cell_h);
            btn.set_size((cell_w - 8) as u32, (cell_h - 8) as u32);
            btn.set_visible(true);
            count += 1;
        }
    }
    let title = if query.is_empty() { CATEGORIES[category] } else { "Kết quả tìm kiếm (tất cả danh mục)" };
    state.summary.set_text(&if count == 0 {
        "Không tìm thấy công cụ. Thử từ khóa khác hoặc xoá tìm kiếm.".to_string()
    } else { format!("{} · {} công cụ", title, count) });
    for (i, btn) in state.categories.iter().enumerate() {
        btn.set_text(&if i == category { format!("[{}]", CATEGORIES[i]) } else { CATEGORIES[i].to_string() });
    }
}

// ---------------------------------------------------------------------------
// Dựng giao diện
// ---------------------------------------------------------------------------
fn build_ui() -> (nwg::Window, Rc<AppState>) {
    // --- Font ---
    let mut font_title = Default::default();
    nwg::Font::builder()
        .family("Segoe UI")
        .size(21)
        .weight(700)
        .build(&mut font_title)
        .expect("font title");

    let mut font_normal = Default::default();
    nwg::Font::builder()
        .family("Segoe UI")
        .size(15)
        .build(&mut font_normal)
        .expect("font normal");

    let mut font_small = Default::default();
    nwg::Font::builder()
        .family("Segoe UI")
        .size(14)
        .build(&mut font_small)
        .expect("font small");

    let mut font_button = Default::default();
    nwg::Font::builder()
        .family("Segoe UI")
        .size(15)
        .weight(700)
        .build(&mut font_button)
        .expect("font button");

    // Font phải sống lâu bằng cửa sổ: cố ý "rò" các font (quy mô launcher,
    // tiến trình kết thúc cùng cửa sổ) để đơn giản hoá vòng đời.
    let font_title = Box::leak(Box::new(font_title));
    let font_normal = Box::leak(Box::new(font_normal));
    let font_small = Box::leak(Box::new(font_small));
    let font_button = Box::leak(Box::new(font_button));

    // --- Cửa sổ chính ---
    let mut window = Default::default();
    nwg::Window::builder()
        .size((1120, 900))
        .position((200, 120))
        .title("Utility Rebuild")
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE | nwg::WindowFlags::MINIMIZE_BOX)
        .build(&mut window)
        .expect("window");

    // Kích thước vùng client thực tế (sau khi trừ viền + thanh tiêu đề)
    let (client_w, client_h) = {
        let mut rc: RECT = unsafe { std::mem::zeroed() };
        unsafe {
            GetClientRect(window.handle.hwnd().unwrap(), &mut rc);
        }
        (rc.right, rc.bottom)
    };

    let add_label = |text: &str, font: &nwg::Font, pos: (i32, i32), size: (u32, u32)| -> nwg::Label {
        let mut lbl = Default::default();
        nwg::Label::builder()
            .text(text)
            .font(Some(font))
            .parent(&window)
            .build(&mut lbl)
            .expect("label");
        lbl.set_position(pos.0, pos.1);
        lbl.set_size(size.0, size.1);
        lbl
    };

    // --- Header ---
    let _lbl_title = add_label("Utility Rebuild", font_title, (18, 10), (420, 34));
    let _lbl_sub = add_label(
        "Chọn công cụ để bắt đầu công việc",
        font_normal,
        (20, 50),
        (440, 22),
    );
    let pc_name = env::var("COMPUTERNAME").unwrap_or_default();
    let user_name = env::var("USERNAME").unwrap_or_default();
    let sys_text = format!(
        "System OS : {}\nPC-Name : {}   |   User: {}",
        os_caption(),
        pc_name,
        user_name
    );
    let _lbl_sys = add_label(&sys_text, font_small, (client_w - 400, 14), (382, 44));

    // --- Lưới 32 nút chức năng: 4 cột x 8 hàng ---
    let grid_x0 = 14;
    let grid_y0 = HEADER_H + 10;
    let grid_x1 = client_w - 14;
    let grid_y1 = client_h - FOOTER_H - 10;
    let cell_w = (grid_x1 - grid_x0) / 4;
    let cell_h = (grid_y1 - grid_y0) / 8;

    let mut buttons: Vec<nwg::Button> = Vec::with_capacity(MODULES.len());
    let mut button_icons = Vec::new();
    for (i, module) in MODULES.iter().enumerate() {
        let col = (i % 4) as i32;
        let row = (i / 4) as i32;
        let mut btn = Default::default();
        nwg::Button::builder()
            .text(&format!("{}\r\n{}\r\n{}", module.label, DETAILS[i].0,
                if modules_dir().join(module.file).is_file() { "Có sẵn" } else { "Chưa có trong gói" }))
            .font(Some(font_small))
            .parent(&window)
            .build(&mut btn)
            .expect("button");
        btn.set_position(grid_x0 + col * cell_w + 6, grid_y0 + row * cell_h + 6);
        btn.set_size((cell_w - 12) as u32, (cell_h - 12) as u32);
        // NWG không khai báo BS_MULTILINE trong ButtonFlags.
        unsafe {
            let hwnd = btn.handle.hwnd().unwrap();
            SetWindowLongW(hwnd, GWL_STYLE, GetWindowLongW(hwnd, GWL_STYLE) | BS_MULTILINE as i32);
        }
        if let Some(icon) = icons::attach(&btn, module.file) { button_icons.push(icon); }
        buttons.push(btn);
    }

    // --- Footer: tìm kiếm + Xoá + Giới thiệu ---
    let footer_y = 78;
    let _lbl_search = add_label("Tìm kiếm :", font_normal, (14, footer_y + 15), (84, 24));

    let mut search = Default::default();
    nwg::TextInput::builder()
        .text("")
        .font(Some(font_normal))
        .parent(&window)
        .build(&mut search)
        .expect("search");
    search.set_position(100, footer_y + 11);
    search.set_size(470, 32);

    let mut btn_clear = Default::default();
    nwg::Button::builder()
        .text("Xoá tìm kiếm")
        .font(Some(font_button))
        .parent(&window)
        .build(&mut btn_clear)
        .expect("btn clear");
    btn_clear.set_position(582, footer_y + 10);
    btn_clear.set_size(136, 32);

    let mut btn_about = Default::default();
    nwg::Button::builder()
        .text("Giới thiệu")
        .font(Some(font_button))
        .parent(&window)
        .build(&mut btn_about)
        .expect("btn about");
    btn_about.set_position(client_w - 124, client_h - 42);
    btn_about.set_size(110, 32);

    let mut categories = Vec::new();
    for (i, name) in CATEGORIES.iter().enumerate() {
        let mut button = nwg::Button::default();
        nwg::Button::builder().text(name).font(Some(font_normal)).parent(&window)
            .position((20 + i as i32 * 152, 140)).size((144, 36))
            .build(&mut button).expect("category");
        categories.push(button);
    }
    let summary = add_label("", font_button, (20, 188), ((client_w - 40) as u32, 28));
    let hint = add_label("Tìm theo tên hoặc công việc, có thể gõ không dấu.", font_small, (100, 116), (700, 20));
    let footer = add_label("Trạng thái công cụ dựa trên file module trong gói đang mở.", font_small,
        (20, client_h - 34), (740, 24));
    let state = Rc::new(AppState {
        buttons,
        search,
        btn_clear,
        btn_about,
        categories,
        category: Cell::new(0),
        summary,
        _labels: vec![_lbl_title, _lbl_sub, _lbl_sys, _lbl_search, hint, footer],
        window: window.handle,
        _icons: button_icons,
    });

    // --- Sự kiện NWG: bấm nút + gõ ô tìm kiếm ---
    let handler_state = state.clone();
    nwg::full_bind_event_handler(&window.handle, move |evt, _data, handle| {
        match evt {
            nwg::Event::OnButtonClick => {
                if handle == handler_state.btn_clear.handle {
                    handler_state.search.set_text("");
                    apply_search_filter(&handler_state);
                } else if handle == handler_state.btn_about.handle {
                    info_box(
                        "Giới thiệu",
                        "Utility Rebuild\nBộ công cụ hỗ trợ cài đặt Windows - Software.\n\nBản dựng lại độc lập (clean-room) bằng Rust,\ntương đương chức năng với công cụ gốc.",
                    );
                } else if let Some(i) = handler_state.categories.iter().position(|btn| handle == btn.handle) {
                    handler_state.category.set(i);
                    handler_state.search.set_text("");
                    apply_search_filter(&handler_state);
                } else {
                    for (i, btn) in handler_state.buttons.iter().enumerate() {
                        if handle == btn.handle {
                            start_module(i);
                            break;
                        }
                    }
                }
            }
            nwg::Event::OnTextInput => {
                if handle == handler_state.search.handle {
                    apply_search_filter(&handler_state);
                }
            }
            nwg::Event::OnWindowClose => nwg::stop_thread_dispatch(),
            _ => {}
        }
    });

    apply_search_filter(&state);
    state.search.set_focus();
    (window, state)
}

fn main() {
    nwg::init().expect("Không khởi tạo được native-windows-gui");
    let (_window, _state) = build_ui();
    nwg::dispatch_thread_events();
}
