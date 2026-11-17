#![windows_subsystem = "windows"]
//! Utility Rebuild — cửa sổ chính (launcher) viết bằng Rust + native-windows-gui.
//!
//! Tương đương giao diện launcher PowerShell (launcher/Utility.ps1):
//! cửa sổ native chuẩn Windows, header (tiêu đề + thông tin OS / PC-Name / User),
//! lưới 4 cột x 8 hàng gồm 32 nút chức năng tiếng Việt, ô tìm kiếm lọc
//! nút (không phân biệt dấu), nút Xoá và Giới thiệu ở footer.
//!
//! Bấm một nút chức năng sẽ chạy file modules\<TênModule>.exe nằm cạnh
//! launcher bằng quyền Administrator (ShellExecute "runas"); nếu module
//! chưa được build, hiện thông báo "chưa có module" thay vì báo lỗi.
//!
//! Đây là mã nguồn viết mới hoàn toàn (clean-room): không cơ chế kiểm
//! tra bản quyền, không telemetry, không tự thay đổi cấu hình bảo mật.
//!
//! Giao diện dùng màu hệ thống mặc định của Windows cho mọi control.

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
use winapi::um::winuser::{GetClientRect, SW_SHOWNORMAL};

// ---------------------------------------------------------------------------
// Kích thước bố cục
// ---------------------------------------------------------------------------
const HEADER_H: i32 = 86;
const FOOTER_H: i32 = 52;

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
    ModuleDef { label: "Backup/Restore Driver", file: "BackupRestoreDriver.exe" },
    ModuleDef { label: "Tắt Bitlocker", file: "TatBitlocker.exe" },
    ModuleDef { label: "Tạo Boot WinPE", file: "TaoBootWinPE.exe" },
    ModuleDef { label: "Cài Đặt Win Tự Động", file: "CaiDatWinTuDong.exe" },
    ModuleDef { label: "Cài Đặt Win=WinPE", file: "CaiDatWinWinPE.exe" },
    ModuleDef { label: "Cài Đặt Office Online", file: "CaiDatOfficeOnline.exe" },
    ModuleDef { label: "Cài Đặt Office OFFLINE", file: "CaiDatOfficeOffline.exe" },
    ModuleDef { label: "Kích Hoạt Win/Office", file: "KichHoatWinOffice.exe" },
    ModuleDef { label: "Cài Đặt Phần Mềm", file: "CaiDatPhanMem.exe" },
    ModuleDef { label: "Sao Lưu Dữ Liệu", file: "SaoLuuDuLieu.exe" },
    ModuleDef { label: "Sao Lưu/Quản lý Wifi", file: "SaoLuuWifi.exe" },
    ModuleDef { label: "Sao Lưu Chrome-Edge", file: "SaoLuuChromeEdge.exe" },
    ModuleDef { label: "Backup/Restore Zalo", file: "BackupRestoreZalo.exe" },
    ModuleDef { label: "Check Cấu Hình Máy Tính", file: "CheckCauHinh.exe" },
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
    for (i, btn) in state.buttons.iter().enumerate() {
        let visible = query.is_empty()
            || unaccented(MODULES[i].label).to_lowercase().contains(&query);
        btn.set_visible(visible);
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
        .size((940, 714))
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
        "Bộ công cụ hỗ trợ cài đặt Windows - Software",
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
    for (i, module) in MODULES.iter().enumerate() {
        let col = (i % 4) as i32;
        let row = (i / 4) as i32;
        let mut btn = Default::default();
        nwg::Button::builder()
            .text(module.label)
            .font(Some(font_button))
            .parent(&window)
            .build(&mut btn)
            .expect("button");
        btn.set_position(grid_x0 + col * cell_w + 6, grid_y0 + row * cell_h + 6);
        btn.set_size((cell_w - 12) as u32, (cell_h - 12) as u32);
        buttons.push(btn);
    }

    // --- Footer: tìm kiếm + Xoá + Giới thiệu ---
    let footer_y = client_h - FOOTER_H;
    let _lbl_search = add_label("Tìm kiếm :", font_normal, (14, footer_y + 15), (84, 24));

    let mut search = Default::default();
    nwg::TextInput::builder()
        .text("")
        .font(Some(font_normal))
        .parent(&window)
        .build(&mut search)
        .expect("search");
    search.set_position(100, footer_y + 11);
    search.set_size(240, 30);

    let mut btn_clear = Default::default();
    nwg::Button::builder()
        .text("Xoá")
        .font(Some(font_button))
        .parent(&window)
        .build(&mut btn_clear)
        .expect("btn clear");
    btn_clear.set_position(350, footer_y + 10);
    btn_clear.set_size(70, 32);

    let mut btn_about = Default::default();
    nwg::Button::builder()
        .text("Giới thiệu")
        .font(Some(font_button))
        .parent(&window)
        .build(&mut btn_about)
        .expect("btn about");
    btn_about.set_position(client_w - 124, footer_y + 10);
    btn_about.set_size(110, 32);

    let state = Rc::new(AppState {
        buttons,
        search,
        btn_clear,
        btn_about,
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
            _ => {}
        }
    });

    state.search.set_focus();
    (window, state)
}

fn main() {
    nwg::init().expect("Không khởi tạo được native-windows-gui");
    let (_window, _state) = build_ui();
    nwg::dispatch_thread_events();
}
