//! Hiển thị tiến trình trong WinPE (v0.7.18).
//!
//! Bối cảnh thực địa (winpe-20261117-1852.log): engine khoẻ mạnh —
//! bản sửa ensure_boot_letter của v0.7.16 chạy đúng (tự gán T: khi
//! S: bận, probe đạt) và DISM bung ảnh chạy tới 82% — nhưng người
//! vận hành tưởng máy treo và tắt nguồn ở giữa chừng, vì engine
//! KHÔNG hiển thị gì cả: cửa sổ tiến trình không hề xuất hiện trong
//! WinPE của ISO mod, còn console của launcher (qsu_start.cmd) thì
//! trống trơn do engine chỉ ghi tiến trình vào file log. Từ bản
//! này có HAI bề mặt tiến trình:
//!
//! 1. **Console là bề mặt CHÍNH.** Engine được qsu_start.cmd chạy
//!    thẳng trong console của launcher nên thừa hưởng đúng console
//!    đó — mọi dòng log đồng thời in ra stdout (xem sink gắn ở
//!    `main.rs`), và phần trăm bung ảnh cập nhật liên tục ngay trên
//!    console bằng dòng `\r`. `ensure_console` chỉ là chốt dự
//!    phòng: stdout chưa bám console nào thì AttachConsole về
//!    tiến trình cha, bất đắc dĩ mới AllocConsole (AllocConsole
//!    mở thêm cửa sổ thứ hai nên là lựa chọn cuối).
//! 2. **Màn hình tiến trình toàn màn hình là bề mặt PHỤ** — mở
//!    TRƯỚC mọi công việc (trước cả quét stage) để màn hình có nội
//!    dung từ những giây đầu. Thiết kế theo màn hình mẫu của tool
//!    gốc do người dùng cung cấp (nền xanh toàn màn hình, thẻ giữa
//!    chứa bước hiện tại + số % lớn + thanh tiến trình + đồng hồ
//!    thời gian đã trôi) nhưng mang thương hiệu riêng của bản
//!    rebuild — KHÔNG dùng logo/tên tác giả của tool gốc. Đồng hồ
//!    `mm:ss` chạy từng giây là tín hiệu "còn sống" ngay cả khi
//!    DISM lâu không nhả phần trăm mới. Màn hình chạy trên luồng
//!    riêng, chỉ ĐỌC trạng thái dùng chung; mọi lỗi GUI đều bị
//!    nuốt trong luồng đó — tuyệt đối không ảnh hưởng luồng cài
//!    đặt và dòng console.
//!
//! Quy ước phần trăm hiển thị: ngoài bước bung ảnh, số % là tiến
//! độ tổng thể theo giai đoạn (các mốc cố định trong `main.rs`);
//! trong bước bung ảnh, số % và thanh tiến trình là phần trăm TRỰC
//! TIẾP của DISM.

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

/// Mốc phần trăm tổng thể lúc bắt đầu bước bung ảnh (trước khi
/// DISM nhả phần trăm đầu tiên).
pub const APPLY_BASE_PCT: u32 = 15;

#[derive(Clone)]
struct UiState {
    phase: String,
    detail: String,
    pct: u32,
    log_path: String,
    last_error: String,
    failed: bool,
    complete: bool,
    started: Instant,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            phase: "Đang khởi động engine cài đặt".to_string(),
            detail: String::new(),
            pct: 0,
            log_path: String::new(),
            last_error: String::new(),
            failed: false,
            complete: false,
            started: Instant::now(),
        }
    }
}

static STATE: OnceLock<Arc<Mutex<UiState>>> = OnceLock::new();

/// Tay cầm trạng thái tiến trình dùng chung giữa luồng chính (ghi)
/// và luồng màn hình (đọc). Mọi thao tác đều không thể panic ra
/// ngoài: khoá hỏng thì bỏ qua lần cập nhật đó.
#[derive(Clone)]
pub struct ProgressUi {
    state: Arc<Mutex<UiState>>,
}

impl ProgressUi {
    /// Mở màn hình tiến trình trên luồng riêng và trả tay cầm. Gọi
    /// càng sớm càng tốt — trước cả bước quét stage.
    pub fn start() -> ProgressUi {
        let state = Arc::new(Mutex::new(UiState::default()));
        let _ = STATE.set(state.clone());
        std::thread::spawn(|| {
            // Mọi lỗi GUI đều kết thúc lặng lẽ trong luồng này.
            let _ = std::panic::catch_unwind(run_window);
        });
        ProgressUi { state }
    }

    fn with_state<F: FnOnce(&mut UiState)>(&self, f: F) {
        if let Ok(mut s) = self.state.lock() {
            f(&mut s);
        }
    }

    pub fn set_phase(&self, phase: &str, pct: u32) {
        self.with_state(|s| {
            s.phase = phase.to_string();
            s.pct = pct;
        });
        mirror_line(&format!(">> {}", phase));
    }

    pub fn set_detail(&self, detail: &str) {
        self.with_state(|s| {
            s.detail = detail.to_string();
        });
    }

    /// Cập nhật theo phần trăm DISM của bước bung ảnh (0..=100):
    /// số % hiển thị là phần trăm trực tiếp của DISM (theo thiết
    /// kế màn hình mẫu), dòng chi tiết mang theo tên edition.
    pub fn set_apply_progress(&self, dism_pct: u32, edition: &str) {
        self.with_state(|s| {
            s.pct = dism_pct;
            s.detail = format!("Đang bung ảnh: {} — {}%", edition, dism_pct);
        });
    }

    pub fn set_log_path(&self, path: &str) {
        self.with_state(|s| {
            s.log_path = path.to_string();
        });
    }

    /// Ghi lại dòng lỗi gần nhất (sink của Logger gọi mỗi khi gặp
    /// dòng bắt đầu bằng "LỖI") — màn hình kết thúc hiển thị đúng
    /// lỗi đã dừng ca cài.
    pub fn note_error(&self, error: &str) {
        self.with_state(|s| {
            s.last_error = error.to_string();
        });
    }

    pub fn mark_failed(&self) {
        self.with_state(|s| {
            s.failed = true;
        });
    }

    pub fn mark_complete(&self) {
        self.with_state(|s| {
            s.complete = true;
            s.pct = 100;
        });
    }

    pub fn phase_text(&self) -> String {
        self.state.lock().map(|s| s.phase.clone()).unwrap_or_default()
    }

    pub fn error_text(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .map(|s| s.last_error.clone())
            .filter(|e| !e.is_empty())
    }

    pub fn log_path_text(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .map(|s| s.log_path.clone())
            .filter(|p| !p.is_empty())
    }
}

// ---------------------------------------------------------------------------
// Console — bề mặt tiến trình chính
// ---------------------------------------------------------------------------

/// Bảo đảm stdout bám vào console của launcher trước khi in chữ
/// đầu tiên. Engine vốn là tiến trình console được qsu_start.cmd
/// chạy trực tiếp nên thường đã thừa hưởng console sẵn; chỉ khi
/// handle stdout không hợp lệ mới AttachConsole về tiến trình cha,
/// và cuối cùng mới AllocConsole.
pub fn ensure_console() {
    unsafe {
        let h = winapi::um::processenv::GetStdHandle(winapi::um::winbase::STD_OUTPUT_HANDLE);
        if h.is_null() || h == winapi::um::handleapi::INVALID_HANDLE_VALUE {
            if winapi::um::wincon::AttachConsole(winapi::um::wincon::ATTACH_PARENT_PROCESS) == 0 {
                let _ = winapi::um::consoleapi::AllocConsole();
            }
        }
    }
}

/// Đang có một dòng tiến độ `\r` chưa được chốt bằng newline hay
/// không (để dòng đầy đủ tiếp theo tự xuống dòng trước khi in).
static PROGRESS_OPEN: Mutex<bool> = Mutex::new(false);

/// In một dòng đầy đủ ra console (chốt dòng tiến độ `\r` đang mở
/// nếu có). Đây là đường ra của gương console: sink của Logger đẩy
/// mọi dòng log qua đây.
pub fn mirror_line(text: &str) {
    let mut open = match PROGRESS_OPEN.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if *open {
        let _ = out.write_all(b"\r\n");
        *open = false;
    }
    let _ = out.write_all(text.as_bytes());
    let _ = out.write_all(b"\r\n");
    let _ = out.flush();
}

/// Cập nhật dòng tiến độ tại chỗ trên console bằng `\r` (dùng cho
/// phần trăm bung ảnh — cập nhật liên tục mà không cuộn màn hình).
pub fn mirror_progress(text: &str) {
    let mut open = match PROGRESS_OPEN.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let _ = out.write_all(b"\r");
    let _ = out.write_all(text.as_bytes());
    // Xoá phần dư của dòng tiến độ dài hơn trước đó.
    let width = text.chars().count();
    if width < 78 {
        let _ = out.write_all(" ".repeat(78 - width).as_bytes());
    }
    let _ = out.flush();
    *open = true;
}

// ---------------------------------------------------------------------------
// Màn hình tiến trình toàn màn hình (bề mặt phụ) — Win32 thuần,
// tự vẽ toàn bộ (không phụ thuộc comctl32), luồng riêng.
// ---------------------------------------------------------------------------

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
mod window {
    use super::{wide, STATE};
    use std::cell::RefCell;
    use std::ptr::null_mut;
    use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
    use winapi::shared::windef::{HBRUSH, HDC, HFONT, HWND, RECT};
    use winapi::um::libloaderapi::GetModuleHandleW;
    use winapi::um::wingdi::{
        CreateFontW, CreateSolidBrush, DeleteObject, SelectObject, SetBkMode, SetTextColor,
        FW_BOLD, FW_SEMIBOLD, TRANSPARENT,
    };
    use winapi::um::winuser::{
        BeginPaint, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        DrawTextW, EndPaint, FillRect, GetClientRect, GetMessageW, GetSystemMetrics,
        InvalidateRect, PostQuitMessage, RegisterClassW, SetTimer, ShowWindow,
        TranslateMessage, UpdateWindow, MSG, PAINTSTRUCT, SM_CXSCREEN, SM_CYSCREEN,
        SW_SHOW, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WM_TIMER,
        WNDCLASSW, WS_EX_TOPMOST, WS_POPUP, DT_CENTER, DT_SINGLELINE, DT_VCENTER,
        DT_WORDBREAK,
    };

    fn rgb(r: u32, g: u32, b: u32) -> u32 {
        r | (g << 8) | (b << 16)
    }

    // Bảng màu (tông xanh dương như màn hình mẫu; hai đầu dải
    // gradient nền là rgb(7,26,58) → rgb(14,74,132), nội suy
    // trực tiếp trong paint()).
    fn col_card() -> u32 { rgb(10, 45, 92) }
    fn col_card_edge() -> u32 { rgb(64, 156, 220) }
    fn col_title() -> u32 { rgb(255, 255, 255) }
    fn col_warn() -> u32 { rgb(168, 222, 255) }
    fn col_step() -> u32 { rgb(255, 255, 255) }
    fn col_pct() -> u32 { rgb(64, 224, 255) }
    fn col_pct_fail() -> u32 { rgb(255, 110, 110) }
    fn col_bar_track() -> u32 { rgb(190, 205, 225) }
    fn col_bar_fill() -> u32 { rgb(0, 176, 240) }
    fn col_bar_fail() -> u32 { rgb(220, 80, 80) }
    fn col_small() -> u32 { rgb(190, 215, 240) }

    struct Fonts {
        title: HFONT,
        warn: HFONT,
        step: HFONT,
        pct: HFONT,
        small: HFONT,
    }

    thread_local! {
        static FONTS: RefCell<Option<Fonts>> = const { RefCell::new(None) };
        static LAST_KEY: RefCell<String> = const { RefCell::new(String::new()) };
    }

    unsafe fn make_font(height: i32, weight: i32) -> HFONT {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            1, // DEFAULT_CHARSET
            0,
            0,
            0,
            0,
            wide("Segoe UI").as_ptr(),
        )
    }

    unsafe fn fill(hdc: HDC, rect: &RECT, color: u32) {
        let brush = CreateSolidBrush(color);
        if !brush.is_null() {
            FillRect(hdc, rect, brush as HBRUSH);
            DeleteObject(brush as *mut _);
        }
    }

    unsafe fn text(
        hdc: HDC,
        font: HFONT,
        color: u32,
        s: &str,
        rect: &mut RECT,
        flags: u32,
    ) {
        if !font.is_null() {
            SelectObject(hdc, font as *mut _);
        }
        SetTextColor(hdc, color);
        let mut w = wide(s);
        DrawTextW(hdc, w.as_mut_ptr(), -1, rect, flags);
    }

    fn fmt_elapsed(total_secs: u64) -> String {
        if total_secs >= 3600 {
            format!(
                "{}:{:02}:{:02}",
                total_secs / 3600,
                (total_secs % 3600) / 60,
                total_secs % 60
            )
        } else {
            format!("{:02}:{:02}", total_secs / 60, total_secs % 60)
        }
    }

    /// Vẽ toàn bộ màn hình từ trạng thái dùng chung.
    fn paint(hwnd: HWND) {
        let snap = match STATE.get().and_then(|s| s.lock().ok()) {
            Some(g) => (
                g.phase.clone(),
                g.detail.clone(),
                g.pct.min(100),
                g.log_path.clone(),
                g.last_error.clone(),
                g.failed,
                g.complete,
                g.started.elapsed().as_secs(),
            ),
            None => return,
        };
        let (phase, detail, pct, log_path, last_error, failed, complete, elapsed) = snap;
        unsafe {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if hdc.is_null() {
                return;
            }
            let mut client: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut client);
            let w = client.right;
            let h = client.bottom;
            SetBkMode(hdc, TRANSPARENT as i32);

            // Nền: gradient dọc xanh đậm → xanh sáng (vẽ theo dải).
            let steps = 64i32;
            for i in 0..steps {
                let t = i as f32 / (steps - 1) as f32;
                let mix = |a: u32, b: u32| -> u32 {
                    (a as f32 + (b as f32 - a as f32) * t) as u32
                };
                let c = rgb(
                    mix(7, 14),
                    mix(26, 74),
                    mix(58, 132),
                );
                let strip = RECT {
                    left: 0,
                    top: h * i / steps,
                    right: w,
                    bottom: h * (i + 1) / steps + 1,
                };
                fill(hdc, &strip, c);
            }

            FONTS.with(|f| {
                let mut f = f.borrow_mut();
                if f.is_none() {
                    *f = Some(Fonts {
                        title: make_font(-54, FW_BOLD as i32),
                        warn: make_font(-24, FW_SEMIBOLD as i32),
                        step: make_font(-27, FW_SEMIBOLD as i32),
                        pct: make_font(-96, FW_BOLD as i32),
                        small: make_font(-17, 400),
                    });
                }
                let fonts = f.as_ref().unwrap();
                let single = DT_CENTER | DT_VCENTER | DT_SINGLELINE;

                // Đầu trang: tiêu đề + cảnh báo + credit.
                let mut r = RECT { left: 40, top: h * 8 / 100, right: w - 40, bottom: h * 8 / 100 + 76 };
                text(hdc, fonts.title, col_title(), "CÀI ĐẶT WINDOWS TỰ ĐỘNG", &mut r, single);
                let mut r = RECT { left: 40, top: h * 8 / 100 + 84, right: w - 40, bottom: h * 8 / 100 + 120 };
                text(
                    hdc,
                    fonts.warn,
                    col_warn(),
                    "Hệ thống đang được cài đặt tự động — Vui lòng KHÔNG tắt máy",
                    &mut r,
                    single,
                );

                // Thẻ giữa: viền + nền.
                let card = RECT {
                    left: w * 18 / 100,
                    top: h * 32 / 100,
                    right: w * 82 / 100,
                    bottom: h * 80 / 100,
                };
                fill(hdc, &card, col_card_edge());
                let inner = RECT {
                    left: card.left + 2,
                    top: card.top + 2,
                    right: card.right - 2,
                    bottom: card.bottom - 2,
                };
                fill(hdc, &inner, col_card());
                let cw = inner.right - inner.left;
                let ch = inner.bottom - inner.top;

                // Bước hiện tại.
                let step_text = if failed {
                    format!("ĐÃ DỪNG — {}", phase)
                } else if complete {
                    "Hoàn tất — sắp khởi động lại".to_string()
                } else {
                    phase.clone()
                };
                let mut r = RECT {
                    left: inner.left + 30,
                    top: inner.top + ch * 6 / 100,
                    right: inner.right - 30,
                    bottom: inner.top + ch * 6 / 100 + 44,
                };
                text(hdc, fonts.step, col_step(), &step_text, &mut r, single);

                // Số % lớn.
                let mut r = RECT {
                    left: inner.left + 30,
                    top: inner.top + ch * 22 / 100,
                    right: inner.right - 30,
                    bottom: inner.top + ch * 22 / 100 + 120,
                };
                text(
                    hdc,
                    fonts.pct,
                    if failed { col_pct_fail() } else { col_pct() },
                    &format!("{}%", pct),
                    &mut r,
                    single,
                );

                // Thanh tiến trình tự vẽ.
                let bar = RECT {
                    left: inner.left + cw * 8 / 100,
                    top: inner.top + ch * 58 / 100,
                    right: inner.right - cw * 8 / 100,
                    bottom: inner.top + ch * 58 / 100 + 26,
                };
                fill(hdc, &bar, col_bar_track());
                if pct > 0 {
                    let filled = RECT {
                        left: bar.left,
                        top: bar.top,
                        right: bar.left + (bar.right - bar.left) * pct as i32 / 100,
                        bottom: bar.bottom,
                    };
                    fill(hdc, &filled, if failed { col_bar_fail() } else { col_bar_fill() });
                }

                // Đồng hồ thời gian đã trôi — tín hiệu còn sống.
                let mut r = RECT {
                    left: inner.left + 30,
                    top: bar.bottom + 14,
                    right: inner.right - 30,
                    bottom: bar.bottom + 44,
                };
                text(
                    hdc,
                    fonts.warn,
                    col_small(),
                    &format!("Thời gian: {}", fmt_elapsed(elapsed)),
                    &mut r,
                    single,
                );

                // Dòng chi tiết / lỗi / nhật ký ở đáy thẻ.
                let foot = if failed {
                    let err = if last_error.is_empty() { detail.clone() } else { last_error.clone() };
                    format!(
                        "{}\nMáy KHÔNG tự khởi động lại — đọc lỗi rồi tắt máy bằng nút nguồn.\nNhật ký: {}",
                        err,
                        if log_path.is_empty() { "—".to_string() } else { log_path.clone() }
                    )
                } else if !detail.is_empty() {
                    detail.clone()
                } else if !log_path.is_empty() {
                    format!("Nhật ký: {}", log_path)
                } else {
                    String::new()
                };
                if !foot.is_empty() {
                    let mut r = RECT {
                        left: inner.left + 30,
                        top: inner.bottom - ch * 22 / 100,
                        right: inner.right - 30,
                        bottom: inner.bottom - 8,
                    };
                    text(hdc, fonts.small, col_small(), &foot, &mut r, DT_CENTER | DT_WORDBREAK);
                }

                // Credit ở chân màn hình.
                let mut r = RECT { left: 40, top: h - 44, right: w - 40, bottom: h - 12 };
                text(hdc, fonts.small, col_small(), "Utility Rebuild", &mut r, single);
            });
            EndPaint(hwnd, &ps);
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: UINT,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_PAINT => {
                paint(hwnd);
                0
            }
            WM_ERASEBKGND => {
                // paint() đã phủ kín toàn bộ bề mặt — không cần xoá
                // nền riêng (tránh nhấp nháy).
                1
            }
            WM_TIMER => {
                // Vẽ lại khi trạng thái đổi HOẶC khi giây của đồng
                // hồ trôi đổi — đồng hồ chạy từng giây là bằng
                // chứng màn hình còn sống.
                let key = match STATE.get().and_then(|s| s.lock().ok()) {
                    Some(g) => format!(
                        "{}|{}|{}|{}|{}|{}",
                        g.phase,
                        g.detail,
                        g.pct,
                        g.failed,
                        g.complete,
                        g.started.elapsed().as_secs()
                    ),
                    None => return 0,
                };
                let changed = LAST_KEY.with(|k| {
                    let mut k = k.borrow_mut();
                    if *k != key {
                        *k = key;
                        true
                    } else {
                        false
                    }
                });
                if changed {
                    InvalidateRect(hwnd, null_mut(), 0);
                }
                0
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    pub fn run() {
        unsafe {
            let hinst = GetModuleHandleW(null_mut());
            let class_name = wide("QSUProgressFull0718");
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(wnd_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinst,
                hIcon: null_mut(),
                hCursor: null_mut(),
                hbrBackground: null_mut(),
                lpszMenuName: null_mut(),
                lpszClassName: class_name.as_ptr(),
            };
            if RegisterClassW(&wc) == 0 {
                return;
            }
            let sw = GetSystemMetrics(SM_CXSCREEN);
            let sh = GetSystemMetrics(SM_CYSCREEN);
            let win = CreateWindowExW(
                WS_EX_TOPMOST,
                class_name.as_ptr(),
                wide("QSU WinPEInstall — Đang cài đặt Windows").as_ptr(),
                WS_POPUP,
                0,
                0,
                sw,
                sh,
                null_mut(),
                null_mut(),
                hinst,
                null_mut(),
            );
            if win.is_null() {
                return;
            }
            ShowWindow(win, SW_SHOW);
            UpdateWindow(win);
            SetTimer(win, 1, 200, None);
            let mut msg: MSG = std::mem::zeroed();
            loop {
                let r = GetMessageW(&mut msg, null_mut(), 0, 0);
                if r <= 0 {
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

#[cfg(windows)]
fn run_window() {
    window::run();
}

#[cfg(not(windows))]
fn run_window() {}
