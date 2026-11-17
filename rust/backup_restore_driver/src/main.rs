#![windows_subsystem = "windows"]
//! BackupRestoreDriver — Backup / Restore Driver (bản Rust).
//!
//! Hành vi bám theo bản port PowerShell đã kiểm chứng
//! (modules/BackupRestoreDriver.ps1), vốn bám theo module gốc [SOURCE]:
//!
//! - Liệt kê driver bên thứ ba bằng WMI Win32_PnPSignedDriver qua COM
//!   gốc (IWbemLocator/IWbemServices). Chọn WMI thay vì phân tích
//!   `pnputil /enum-drivers` vì output pnputil bị bản địa hoá theo
//!   ngôn ngữ Windows (máy tiếng Việt sẽ trả nhãn tiếng Việt) — phân
//!   tích text rất dễ sai; WMI trả thuộc tính có cấu trúc, ổn định.
//! - Lọc theo 6 nhóm thiết bị (GUID lớp -> Graphics/Audio/Network/
//!   Storage/Input/Other), tuỳ chọn chỉ driver không phải Microsoft
//!   (mặc định bật), tìm kiếm bỏ dấu tiếng Việt.
//! - Backup: pnputil /export-driver <oemXX.inf> <đích>\<tên INF> cho
//!   từng driver đã tick (mã thoát 0 = thành công).
//! - Restore: pnputil /add-driver <inf> /install cho mọi *.inf đệ quy
//!   (mã 0/259/3010 = thành công) rồi pnputil /scan-devices.
//! - Xuất HTML: DanhSachDriver.html cạnh file exe rồi mở lên.
//! Mọi thao tác ghi vào DriverTool_Log.txt cạnh file exe. Module tự
//! nâng quyền Administrator khi mở.

use std::cell::RefCell;
use std::collections::HashMap;
use winapi::ctypes::c_void;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use native_windows_gui as nwg;
use qsu_common::cmd;
use qsu_common::log::{self, Logger};
use qsu_common::sys;
use unicode_normalization::UnicodeNormalization;
use winapi::shared::minwindef::{LPARAM, UINT, WPARAM};
use winapi::shared::windef::RECT;
use winapi::shared::wtypes::BSTR;
use winapi::shared::wtypesbase::CLSCTX_INPROC_SERVER;
use winapi::um::combaseapi::{CoCreateInstance, CoInitializeEx, CoInitializeSecurity, CoSetProxyBlanket, CoUninitialize};
use winapi::um::oaidl::VARIANT;
use winapi::um::oleauto::{SysAllocString, SysFreeString, SysStringLen, VariantClear, VariantInit};
use winapi::um::shellapi::ShellExecuteW;
use winapi::um::wbemcli::{
    IEnumWbemClassObject, IWbemClassObject, IWbemLocator, IWbemServices, CLSID_WbemLocator,
    IID_IWbemLocator,
};
use winapi::um::winuser::{GetClientRect, SendMessageW, SW_SHOWNORMAL};

const HEADER_H: i32 = 66;

const CATEGORIES: [&str; 6] = ["Graphics", "Audio", "Network", "Storage", "Input", "Other"];

fn unaccent(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.nfd() {
        if unicode_normalization::char::is_combining_mark(ch) {
            continue;
        }
        match ch {
            'đ' => out.push('d'),
            'Đ' => out.push('D'),
            _ => out.push(ch),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Dữ liệu driver + liệt kê bằng WMI COM
// ---------------------------------------------------------------------------
#[derive(Clone)]
struct Driver {
    device_name: String,
    manufacturer: String,
    category: String,
    version: String,
    date: String, // dd/MM/yyyy
    inf_name: String,
}

fn category_of(device_class: &str) -> &'static str {
    let guid = device_class.replace(['{', '}'], "").trim().to_lowercase();
    match guid.as_str() {
        "4d36e968-e325-11ce-bfc1-08002be10318" => "Graphics", // Display
        "4d36e96c-e325-11ce-bfc1-08002be10318" => "Audio",    // MEDIA
        "4d36e972-e325-11ce-bfc1-08002be10318" => "Network",  // Net
        "4d36e97b-e325-11ce-bfc1-08002be10318" => "Storage",  // SCSIAdapter
        "4d36e96a-e325-11ce-bfc1-08002be10318" => "Storage",  // HDC
        "4d36e967-e325-11ce-bfc1-08002be10318" => "Storage",  // DiskDrive
        "4d36e96b-e325-11ce-bfc1-08002be10318" => "Input",    // Keyboard
        "4d36e96f-e325-11ce-bfc1-08002be10318" => "Input",    // Mouse
        "745a17a0-74d3-11d0-b6fe-00a0c90f57da" => "Input",    // HIDClass
        _ => "Other",
    }
}

/// "20210315000000.000000+***" -> "15/03/2021"
fn cim_date(s: &str) -> String {
    if s.len() >= 8 && s.chars().take(8).all(|c| c.is_ascii_digit()) {
        format!("{}/{}/{}", &s[6..8], &s[4..6], &s[0..4])
    } else {
        String::new()
    }
}

unsafe fn make_bstr(s: &str) -> BSTR {
    let w: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    SysAllocString(w.as_ptr())
}

unsafe fn get_prop(obj: *mut IWbemClassObject, name: &str) -> String {
    let wname = sys::wide(name);
    let mut var: VARIANT = std::mem::zeroed();
    VariantInit(&mut var);
    let hr = (*obj).Get(wname.as_ptr(), 0, &mut var, std::ptr::null_mut(), std::ptr::null_mut());
    let mut out = String::new();
    if hr == 0 && var.n1.n2().vt == 8u16 {
        // VT_BSTR = 8
        let b = *var.n1.n2().n3.bstrVal();
        if !b.is_null() {
            let len = SysStringLen(b) as usize;
            let slice = std::slice::from_raw_parts(b, len);
            out = String::from_utf16_lossy(slice);
        }
    }
    VariantClear(&mut var);
    out
}

/// Liệt kê Win32_PnPSignedDriver bằng WMI COM. Phải chạy trên luồng
/// riêng (COM apartment của luồng đó).
fn enumerate_drivers() -> Result<Vec<Driver>, String> {
    unsafe {
        const RPC_E_CHANGED_MODE: i32 = -2147417850; // 0x80010106
        let hr = CoInitializeEx(std::ptr::null_mut(), 0); // COINIT_MULTITHREADED
        if hr < 0 && hr != RPC_E_CHANGED_MODE {
            return Err(format!("CoInitializeEx lỗi (0x{:08x})", hr as u32));
        }
        // RPC_C_AUTHN_LEVEL_DEFAULT=0, RPC_C_IMP_LEVEL_IMPERSONATE=3
        let _ = CoInitializeSecurity(
            std::ptr::null_mut(),
            -1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            3,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
        );
        let mut locator: *mut IWbemLocator = std::ptr::null_mut();
        let hr = CoCreateInstance(
            &CLSID_WbemLocator,
            std::ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_IWbemLocator,
            &mut locator as *mut *mut IWbemLocator as *mut *mut c_void,
        );
        if hr < 0 || locator.is_null() {
            CoUninitialize();
            return Err(format!("Không tạo được WbemLocator (0x{:08x})", hr as u32));
        }
        let ns = make_bstr("ROOT\\CIMV2");
        let mut services: *mut IWbemServices = std::ptr::null_mut();
        let hr = (*locator).ConnectServer(
            ns,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut services,
        );
        SysFreeString(ns);
        (*locator).Release();
        if hr < 0 || services.is_null() {
            CoUninitialize();
            return Err(format!("Không kết nối được ROOT\\CIMV2 (0x{:08x})", hr as u32));
        }
        // RPC_C_AUTHN_WINNT=10, RPC_C_AUTHZ_NONE=0, RPC_C_AUTHN_LEVEL_CALL=3
        let _ = CoSetProxyBlanket(
            services as *mut winapi::um::unknwnbase::IUnknown,
            10,
            0,
            std::ptr::null_mut(),
            3,
            3,
            std::ptr::null_mut(),
            0,
        );
        let lang = make_bstr("WQL");
        let query = make_bstr("SELECT * FROM Win32_PnPSignedDriver WHERE DeviceName IS NOT NULL");
        let mut enumerator: *mut IEnumWbemClassObject = std::ptr::null_mut();
        // WBEM_FLAG_FORWARD_ONLY(0x20) | WBEM_FLAG_RETURN_IMMEDIATELY(0x10)
        let hr = (*services).ExecQuery(lang, query, 0x20 | 0x10, std::ptr::null_mut(), &mut enumerator);
        SysFreeString(lang);
        SysFreeString(query);
        if hr < 0 || enumerator.is_null() {
            (*services).Release();
            CoUninitialize();
            return Err(format!("Truy vấn WMI lỗi (0x{:08x})", hr as u32));
        }
        let mut drivers = Vec::new();
        loop {
            let mut obj: *mut IWbemClassObject = std::ptr::null_mut();
            let mut returned: u32 = 0;
            let hr = (*enumerator).Next(-1, 1, &mut obj, &mut returned);
            if hr < 0 || returned == 0 || obj.is_null() {
                break;
            }
            let device_name = get_prop(obj, "DeviceName");
            let manufacturer = get_prop(obj, "Manufacturer");
            let device_class = get_prop(obj, "DeviceClass");
            let version = get_prop(obj, "DriverVersion");
            let date_raw = get_prop(obj, "DriverDate");
            let inf_name = get_prop(obj, "InfName");
            (*obj).Release();
            if device_name.is_empty() {
                continue;
            }
            drivers.push(Driver {
                device_name,
                manufacturer,
                category: category_of(&device_class).to_string(),
                version,
                date: cim_date(&date_raw),
                inf_name,
            });
        }
        (*enumerator).Release();
        (*services).Release();
        CoUninitialize();
        drivers.sort_by(|a, b| {
            a.category
                .to_lowercase()
                .cmp(&b.category.to_lowercase())
                .then(a.device_name.to_lowercase().cmp(&b.device_name.to_lowercase()))
        });
        Ok(drivers)
    }
}

fn pnputil_path() -> String {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string());
    format!("{}\\System32\\pnputil.exe", windir)
}

/// Thư mục backup mặc định: thư mục backup_driver* có sẵn trên D:\,
/// không có thì D:\backup_driver_<yyyyMMdd>, không có ổ D: thì Desktop.
fn default_backup_folder() -> String {
    let stamp = log::log_stamp(); // yyyyMMdd-HHmm
    let day = &stamp[..8.min(stamp.len())];
    let d_root = Path::new("D:\\");
    if d_root.exists() {
        if let Ok(rd) = std::fs::read_dir(d_root) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                        if name.to_lowercase().starts_with("backup_driver") {
                            return p.display().to_string();
                        }
                    }
                }
            }
        }
        return format!("D:\\backup_driver_{}", day);
    }
    let profile = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Public".to_string());
    format!("{}\\Desktop\\backup_driver_{}", profile, day)
}

fn collect_infs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().map(|x| x.eq_ignore_ascii_case("inf")).unwrap_or(false) {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ---------------------------------------------------------------------------
// Trạng thái chia sẻ
// ---------------------------------------------------------------------------
#[derive(Default)]
struct Shared {
    drivers: Option<Result<Vec<Driver>, String>>,
    status: Option<String>,
    done_msg: Option<String>,
    err_msg: Option<String>,
    busy: bool,
}

type SharedRef = Arc<Mutex<Shared>>;

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
// ListView checkbox (thông điệp Win32 thô)
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
        let mut item: winapi::um::commctrl::LVITEMW = unsafe { std::mem::zeroed() };
        item.stateMask = LVIS_STATEIMAGEMASK;
        item.state = (if checked { 2u32 } else { 1u32 }) << 12;
        unsafe {
            SendMessageW(hwnd, LVM_SETITEMSTATE, row as WPARAM, &item as *const winapi::um::commctrl::LVITEMW as LPARAM);
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
    cat_boxes: Vec<nwg::CheckBox>,
    chk_non_ms: nwg::CheckBox,
    txt_search: nwg::TextInput,
    lbl_count: nwg::Label,
    list: nwg::ListView,
    txt_folder: nwg::TextInput,
    lbl_status: nwg::Label,
    btn_browse: nwg::Button,
    btn_rescan: nwg::Button,
    btn_backup: nwg::Button,
    btn_restore: nwg::Button,
    btn_html: nwg::Button,
    btn_all: nwg::Button,
    btn_none: nwg::Button,
    all_drivers: RefCell<Vec<Driver>>,
    visible: RefCell<Vec<usize>>,
    checked_map: RefCell<HashMap<String, bool>>,
    shared: SharedRef,
    exe_dir: PathBuf,
}

fn make_logger(app: &Rc<App>) -> Arc<Logger> {
    Arc::new(Logger::new(app.exe_dir.join("DriverTool_Log.txt"), None))
}

fn harvest_checks(app: &Rc<App>) {
    let visible = app.visible.borrow();
    let all = app.all_drivers.borrow();
    let mut map = app.checked_map.borrow_mut();
    for (row, &idx) in visible.iter().enumerate() {
        if let Some(d) = all.get(idx) {
            map.insert(d.inf_name.clone(), lv_item_checked(&app.list, row));
        }
    }
}

fn refill(app: &Rc<App>) {
    harvest_checks(app);
    let query = unaccent(&app.txt_search.text().trim().to_lowercase());
    let non_ms_only = app.chk_non_ms.check_state() == nwg::CheckBoxState::Checked;
    let cat_on: Vec<bool> = app
        .cat_boxes
        .iter()
        .map(|b| b.check_state() == nwg::CheckBoxState::Checked)
        .collect();
    let all = app.all_drivers.borrow();
    let map = app.checked_map.borrow();
    let mut vis: Vec<usize> = Vec::new();
    app.list.clear();
    for (i, d) in all.iter().enumerate() {
        let cat_idx = CATEGORIES.iter().position(|c| *c == d.category).unwrap_or(5);
        if !cat_on.get(cat_idx).copied().unwrap_or(true) {
            continue;
        }
        if non_ms_only && d.manufacturer.to_lowercase().contains("microsoft") {
            continue;
        }
        if !query.is_empty() {
            let hay = unaccent(&format!("{} {} {}", d.device_name, d.manufacturer, d.inf_name).to_lowercase());
            if !hay.contains(&query) {
                continue;
            }
        }
        let row = vis.len() as i32;
        lv_insert_row(
            &app.list,
            row,
            &[
                d.device_name.clone(),
                d.manufacturer.clone(),
                d.category.clone(),
                d.version.clone(),
                d.date.clone(),
                d.inf_name.clone(),
            ],
        );
        lv_set_checked(&app.list, row as usize, map.get(&d.inf_name).copied().unwrap_or(false));
        vis.push(i);
    }
    app.lbl_count.set_text(&format!("Hiển thị: {} / {} driver", vis.len(), all.len()));
    *app.visible.borrow_mut() = vis;
}

fn spawn_scan(app: &Rc<App>) {
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let logger = make_logger(app);
    {
        shared.lock().unwrap().busy = true;
    }
    set_status(&shared, &sender, "Đang quét driver bằng WMI...");
    std::thread::spawn(move || match enumerate_drivers() {
        Ok(drivers) => {
            logger.line(&format!("Đã quét {} driver.", drivers.len()));
            let mut s = shared.lock().unwrap();
            s.drivers = Some(Ok(drivers));
            s.busy = false;
            drop(s);
            sender.notice();
        }
        Err(e) => {
            logger.line(&format!("Lỗi quét driver: {}", e));
            finish(&shared, &sender, None, Some(format!("Không quét được danh sách driver.\n{}", e)));
        }
    });
}

fn spawn_backup(app: &Rc<App>, selected: Vec<Driver>, dest_root: PathBuf) {
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let logger = make_logger(app);
    {
        shared.lock().unwrap().busy = true;
    }
    std::thread::spawn(move || {
        logger.line(&format!("=== BACKUP: {} driver -> {}", selected.len(), dest_root.display()));
        let _ = std::fs::create_dir_all(&dest_root);
        let pnp = pnputil_path();
        let mut ok = 0usize;
        let mut fail = 0usize;
        for drv in &selected {
            set_status(&shared, &sender, &format!("Đang backup: {} ({})...", drv.device_name, drv.inf_name));
            let sub_name = Path::new(&drv.inf_name)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| drv.inf_name.clone());
            let dest = dest_root.join(&sub_name);
            let _ = std::fs::create_dir_all(&dest);
            let dest_s = dest.display().to_string();
            match cmd::run_capture(&pnp, &["/export-driver", &drv.inf_name, &dest_s]) {
                Ok(out) if out.code == Some(0) => {
                    ok += 1;
                    logger.line(&format!("OK   {} -> {}", drv.inf_name, dest_s));
                }
                Ok(out) => {
                    fail += 1;
                    let detail = out.stdout.trim();
                    logger.line(&format!("FAIL {} (exit {:?}): {}", drv.inf_name, out.code, detail.chars().take(300).collect::<String>()));
                }
                Err(e) => {
                    fail += 1;
                    logger.line(&format!("FAIL {} (không chạy được pnputil: {})", drv.inf_name, e));
                }
            }
        }
        logger.line(&format!("=== BACKUP xong: thành công {}, lỗi {}", ok, fail));
        finish(
            &shared,
            &sender,
            Some(format!(
                "Backup hoàn tất.\nThành công: {}\nLỗi: {}\n\nChi tiết xem DriverTool_Log.txt cạnh công cụ.",
                ok, fail
            )),
            None,
        );
    });
}

fn spawn_restore(app: &Rc<App>, infs: Vec<PathBuf>, src_root: PathBuf) {
    let shared = app.shared.clone();
    let sender = app.notice.sender();
    let logger = make_logger(app);
    {
        shared.lock().unwrap().busy = true;
    }
    std::thread::spawn(move || {
        logger.line(&format!("=== RESTORE: {} file .inf từ {}", infs.len(), src_root.display()));
        let pnp = pnputil_path();
        let mut ok = 0usize;
        let mut fail = 0usize;
        for inf in &infs {
            set_status(&shared, &sender, &format!("Đang cài: {}...", inf.display()));
            let inf_s = inf.display().to_string();
            match cmd::run_capture(&pnp, &["/add-driver", &inf_s, "/install"]) {
                Ok(out) if matches!(out.code, Some(0) | Some(259) | Some(3010)) => {
                    ok += 1;
                    logger.line(&format!("OK   {} (exit {:?})", inf_s, out.code));
                }
                Ok(out) => {
                    fail += 1;
                    let detail = out.stdout.trim();
                    logger.line(&format!("FAIL {} (exit {:?}): {}", inf_s, out.code, detail.chars().take(300).collect::<String>()));
                }
                Err(e) => {
                    fail += 1;
                    logger.line(&format!("FAIL {} (không chạy được pnputil: {})", inf_s, e));
                }
            }
        }
        set_status(&shared, &sender, "Đang quét lại thiết bị (scan-devices)...");
        let _ = cmd::run_capture(&pnp, &["/scan-devices"]);
        logger.line(&format!("=== RESTORE xong: thành công {}, lỗi {}; đã chạy /scan-devices", ok, fail));
        finish(
            &shared,
            &sender,
            Some(format!(
                "Restore hoàn tất.\nThành công: {}\nLỗi: {}\n\nChi tiết xem DriverTool_Log.txt cạnh công cụ.",
                ok, fail
            )),
            None,
        );
    });
}

fn export_html(app: &Rc<App>) {
    let all = app.all_drivers.borrow();
    let visible = app.visible.borrow();
    let mut rows = String::new();
    for (i, &idx) in visible.iter().enumerate() {
        if let Some(d) = all.get(idx) {
            rows.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                i + 1,
                html_escape(&d.device_name),
                html_escape(&d.manufacturer),
                html_escape(&d.category),
                html_escape(&d.version),
                html_escape(&d.date),
                html_escape(&d.inf_name)
            ));
        }
    }
    let machine = std::env::var("COMPUTERNAME").unwrap_or_default();
    let now = log::now_string(); // yyyy-MM-dd HH:mm:ss
    let stamp = if now.len() >= 19 {
        format!("{}/{}/{} {}", &now[8..10], &now[5..7], &now[0..4], &now[11..19])
    } else {
        now.clone()
    };
    let html = format!(
        "<!DOCTYPE html>\n<html lang=\"vi\">\n<head>\n<meta charset=\"utf-8\">\n<title>Danh sách Driver</title>\n<style>\n  body {{ font-family: 'Segoe UI', Arial, sans-serif; margin: 24px; }}\n  table {{ border-collapse: collapse; width: 100%; }}\n  th, td {{ border: 1px solid #999; padding: 6px 8px; font-size: 13px; text-align: left; }}\n  th {{ background: #e8e8e8; }}\n</style>\n</head>\n<body>\n<h2>Danh sách Driver</h2>\n<p>Máy: {} - Xuất lúc: {}</p>\n<table>\n<tr><th>STT</th><th>Thiết bị</th><th>Hãng sản xuất</th><th>Nhóm</th><th>Phiên bản</th><th>Ngày</th><th>File INF</th></tr>\n{}</table>\n</body>\n</html>\n",
        html_escape(&machine),
        stamp,
        rows
    );
    let path = app.exe_dir.join("DanhSachDriver.html");
    match std::fs::write(&path, html) {
        Ok(()) => {
            make_logger(app).line(&format!("Đã xuất HTML: {} ({} driver)", path.display(), visible.len()));
            let verb: Vec<u16> = "open\0".encode_utf16().collect();
            let file: Vec<u16> = path.display().to_string().encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                ShellExecuteW(
                    std::ptr::null_mut(),
                    verb.as_ptr(),
                    file.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    SW_SHOWNORMAL,
                );
            }
        }
        Err(e) => error_box("Xuất HTML", &format!("Không ghi được file HTML.\n{}", e)),
    }
}

fn pick_folder() -> Option<PathBuf> {
    let mut dlg = Default::default();
    let built = nwg::FileDialog::builder()
        .title("Chọn thư mục backup driver")
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
    let mut f_title = Default::default();
    nwg::Font::builder().family("Segoe UI").size(21).weight(700).build(&mut f_title).expect("font");
    let mut f_normal = Default::default();
    nwg::Font::builder().family("Segoe UI").size(15).build(&mut f_normal).expect("font");
    let mut f_small = Default::default();
    nwg::Font::builder().family("Segoe UI").size(14).build(&mut f_small).expect("font");
    let mut f_button = Default::default();
    nwg::Font::builder().family("Segoe UI").size(15).weight(700).build(&mut f_button).expect("font");
    let f_title = Box::leak(Box::new(f_title));
    let f_normal = Box::leak(Box::new(f_normal));
    let f_small = Box::leak(Box::new(f_small));
    let f_button = Box::leak(Box::new(f_button));

    let mut window = Default::default();
    nwg::Window::builder()
        .size((1000, 714))
        .position((150, 80))
        .title("Backup / Restore Driver")
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

    let _t = add_label("Backup / Restore Driver", f_title, (16, 8), (400, 30));
    let _s = add_label(
        "Sao lưu và khôi phục driver thiết bị bằng WMI + pnputil",
        f_normal, (16, 40), (600, 22),
    );

    // Hàng lọc nhóm thiết bị
    let filter_y = HEADER_H + 10;
    let _lf = add_label("Nhóm thiết bị:", f_normal, (14, filter_y + 3), (110, 22));
    let mut cat_boxes: Vec<nwg::CheckBox> = Vec::new();
    let cat_widths = [92, 72, 92, 84, 72, 72];
    let mut cx = 128;
    for (i, cat) in CATEGORIES.iter().enumerate() {
        let mut chk = Default::default();
        nwg::CheckBox::builder()
            .text(cat)
            .font(Some(f_normal))
            .check_state(nwg::CheckBoxState::Checked)
            .parent(&window)
            .build(&mut chk)
            .expect("checkbox");
        chk.set_position(cx, filter_y);
        chk.set_size(cat_widths[i], 24);
        cat_boxes.push(chk);
        cx += cat_widths[i] as i32 + 4;
    }
    let mut chk_non_ms = Default::default();
    nwg::CheckBox::builder()
        .text("Chỉ driver không phải Microsoft")
        .font(Some(f_normal))
        .check_state(nwg::CheckBoxState::Checked)
        .parent(&window)
        .build(&mut chk_non_ms)
        .expect("checkbox");
    chk_non_ms.set_position(660, filter_y);
    chk_non_ms.set_size(290, 24);

    // Hàng tìm kiếm
    let search_y = filter_y + 32;
    let _ls = add_label("Tìm kiếm:", f_normal, (14, search_y + 3), (70, 22));
    let mut txt_search = Default::default();
    nwg::TextInput::builder().font(Some(f_normal)).parent(&window).build(&mut txt_search).expect("textinput");
    txt_search.set_position(90, search_y);
    txt_search.set_size(300, 24);
    let lbl_count = add_label("", f_normal, (404, search_y + 3), (400, 22));

    // Danh sách driver
    let list_y = search_y + 34;
    let folder_y = client_h - 108;
    let mut list = Default::default();
    nwg::ListView::builder()
        .parent(&window)
        .list_style(nwg::ListViewStyle::Detailed)
        .flags(nwg::ListViewFlags::VISIBLE | nwg::ListViewFlags::TAB_STOP | nwg::ListViewFlags::ALWAYS_SHOW_SELECTION)
        .ex_flags(nwg::ListViewExFlags::GRID | nwg::ListViewExFlags::FULL_ROW_SELECT)
        .build(&mut list)
        .expect("listview");
    list.set_headers_enabled(true);
    list.set_position(14, list_y);
    list.set_size((client_w - 28) as u32, (folder_y - list_y - 10) as u32);
    for (i, (name, w)) in [
        ("Thiết bị", 320),
        ("Hãng sản xuất", 150),
        ("Nhóm", 75),
        ("Phiên bản", 115),
        ("Ngày", 85),
        ("File INF", 140),
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
    lv_enable_checkboxes(&list);

    // Hàng thư mục
    let _lfo = add_label("Thư mục backup:", f_normal, (14, folder_y + 4), (120, 22));
    let mut txt_folder = Default::default();
    nwg::TextInput::builder().font(Some(f_normal)).parent(&window).build(&mut txt_folder).expect("textinput");
    txt_folder.set_position(140, folder_y);
    txt_folder.set_size((client_w - 140 - 190) as u32, 24);
    txt_folder.set_text(&default_backup_folder());

    let mk_btn = |text: &str, font: &nwg::Font, pos: (i32, i32), size: (u32, u32)| -> nwg::Button {
        let mut b = Default::default();
        nwg::Button::builder().text(text).font(Some(font)).parent(&window).build(&mut b).expect("button");
        b.set_position(pos.0, pos.1);
        b.set_size(size.0, size.1);
        b
    };
    let btn_browse = mk_btn("Chọn...", f_normal, (client_w - 176, folder_y - 4), (80, 32));
    let btn_rescan = mk_btn("Quét lại", f_normal, (client_w - 90, folder_y - 4), (76, 32));

    // Hàng nút thao tác
    let btn_y = folder_y + 40;
    let btn_backup = mk_btn("Backup", f_button, (140, btn_y), (130, 34));
    let btn_restore = mk_btn("Restore", f_button, (280, btn_y), (130, 34));
    let btn_html = mk_btn("Xuất HTML", f_normal, (420, btn_y), (110, 34));
    let btn_all = mk_btn("Chọn tất cả", f_normal, (540, btn_y), (110, 34));
    let btn_none = mk_btn("Bỏ chọn", f_normal, (660, btn_y), (90, 34));

    let lbl_status = add_label(
        "Đang khởi tạo...",
        f_small,
        (14, client_h - 26),
        ((client_w - 28) as u32, 22),
    );

    let mut notice = Default::default();
    nwg::Notice::builder().parent(&window).build(&mut notice).expect("notice");

    let app = Rc::new(App {
        notice,
        cat_boxes,
        chk_non_ms,
        txt_search,
        lbl_count,
        list,
        txt_folder,
        lbl_status,
        btn_browse,
        btn_rescan,
        btn_backup,
        btn_restore,
        btn_html,
        btn_all,
        btn_none,
        all_drivers: RefCell::new(Vec::new()),
        visible: RefCell::new(Vec::new()),
        checked_map: RefCell::new(HashMap::new()),
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
            nwg::Event::OnTextInput => {
                if handle == handler.txt_search.handle {
                    refill(&handler);
                }
            }
            nwg::Event::OnButtonClick => {
                if handler.cat_boxes.iter().any(|b| b.handle == handle)
                    || handle == handler.chk_non_ms.handle
                {
                    refill(&handler);
                    return;
                }
                if handle == handler.btn_all.handle {
                    for row in 0..handler.visible.borrow().len() {
                        lv_set_checked(&handler.list, row, true);
                    }
                    harvest_checks(&handler);
                    return;
                }
                if handle == handler.btn_none.handle {
                    for row in 0..handler.visible.borrow().len() {
                        lv_set_checked(&handler.list, row, false);
                    }
                    harvest_checks(&handler);
                    return;
                }
                if handler.shared.lock().unwrap().busy {
                    return;
                }
                if handle == handler.btn_browse.handle {
                    if let Some(p) = pick_folder() {
                        handler.txt_folder.set_text(&p.display().to_string());
                    }
                } else if handle == handler.btn_rescan.handle {
                    spawn_scan(&handler);
                } else if handle == handler.btn_backup.handle {
                    harvest_checks(&handler);
                    let map = handler.checked_map.borrow();
                    let selected: Vec<Driver> = handler
                        .all_drivers
                        .borrow()
                        .iter()
                        .filter(|d| map.get(&d.inf_name).copied().unwrap_or(false))
                        .cloned()
                        .collect();
                    drop(map);
                    if selected.is_empty() {
                        info_box("Backup Driver", "Chưa chọn driver nào. Hãy tick vào các driver cần backup.");
                        return;
                    }
                    let dest = handler.txt_folder.text().trim().to_string();
                    if dest.is_empty() {
                        info_box("Backup Driver", "Chưa chọn thư mục đích.");
                        return;
                    }
                    spawn_backup(&handler, selected, PathBuf::from(dest));
                } else if handle == handler.btn_restore.handle {
                    let src = handler.txt_folder.text().trim().to_string();
                    let src_path = PathBuf::from(&src);
                    if src.is_empty() || !src_path.is_dir() {
                        info_box("Restore Driver", "Thư mục backup không tồn tại. Hãy chọn thư mục chứa driver đã backup.");
                        return;
                    }
                    let infs = collect_infs(&src_path);
                    if infs.is_empty() {
                        info_box("Restore Driver", "Không tìm thấy file .inf nào trong thư mục đã chọn.");
                        return;
                    }
                    let text = format!("Tìm thấy {} file .inf. Cài đặt toàn bộ vào hệ thống?", infs.len());
                    if confirm_box("Restore Driver", &text) {
                        spawn_restore(&handler, infs, src_path);
                    }
                } else if handle == handler.btn_html.handle {
                    export_html(&handler);
                }
            }
            _ => {}
        }
    });

    (window, app)
}

fn refresh(app: &Rc<App>) {
    let (drivers, status, done, err) = {
        let mut s = app.shared.lock().unwrap();
        (s.drivers.take(), s.status.take(), s.done_msg.take(), s.err_msg.take())
    };
    if let Some(res) = drivers {
        match res {
            Ok(list) => {
                *app.all_drivers.borrow_mut() = list;
                refill(app);
                app.lbl_status.set_text("Sẵn sàng.");
            }
            Err(e) => {
                app.lbl_status.set_text("Có lỗi khi quét driver.");
                error_box("Backup / Restore Driver", &e);
            }
        }
    }
    if let Some(st) = status {
        app.lbl_status.set_text(&st);
    }
    if let Some(msg) = done {
        app.lbl_status.set_text("Sẵn sàng.");
        info_box("Backup / Restore Driver", &msg);
    }
    if let Some(msg) = err {
        app.lbl_status.set_text("Có lỗi xảy ra.");
        error_box("Backup / Restore Driver", &msg);
    }
    let busy = app.shared.lock().unwrap().busy;
    for b in [&app.btn_browse, &app.btn_rescan, &app.btn_backup, &app.btn_restore, &app.btn_html] {
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
    make_logger(&app).line("--- Khởi động Backup/Restore Driver ---");
    spawn_scan(&app);
    nwg::dispatch_thread_events();
}
