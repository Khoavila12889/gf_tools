//! Tải file qua WinHTTP (API gốc của Windows — hỗ trợ sẵn HTTPS/TLS và
//! proxy hệ thống, không cần nhúng TLS stack). Có resume bằng header
//! Range: file tải dở được nối tiếp; nếu server không hỗ trợ Range
//! (trả 200 thay vì 206) thì tự tải lại từ đầu.
//!
//! Đặc tả §3 yêu cầu tải ISO có resume + kiểm SHA-256 bắt buộc: hàm này
//! lo phần tải + báo tiến độ; việc kiểm hash do nơi gọi thực hiện bằng
//! sha256::hash_file (streaming) ngay sau khi tải xong.

use std::io::Write;
use winapi::ctypes::c_void;
use std::path::Path;
use std::ptr;

use winapi::um::winhttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetTimeouts, HINTERNET, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct ParsedUrl {
    secure: bool,
    host: String,
    port: u16,
    path: String, // gồm cả query
}

fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(format!("URL không hỗ trợ (cần http/https): {}", url));
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rfind(':') {
        Some(i) if authority[i + 1..].chars().all(|c| c.is_ascii_digit()) => {
            let p: u16 = authority[i + 1..]
                .parse()
                .map_err(|_| "port không hợp lệ".to_string())?;
            (&authority[..i], p)
        }
        _ => (authority, if secure { 443 } else { 80 }),
    };
    if host.is_empty() {
        return Err("URL thiếu host".to_string());
    }
    Ok(ParsedUrl {
        secure,
        host: host.to_string(),
        port,
        path: if path.is_empty() { "/".to_string() } else { path.to_string() },
    })
}

struct Session(HINTERNET);
impl Drop for Session {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn open_session() -> Result<Session, String> {
    unsafe {
        let agent = wide("QSU-Utility/1.0");
        let h = WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        );
        if h.is_null() {
            return Err("WinHttpOpen thất bại".to_string());
        }
        // resolve, connect, send, receive: 30s / 30s / 60s / 60s
        WinHttpSetTimeouts(h, 30_000, 30_000, 60_000, 60_000);
        Ok(Session(h))
    }
}

fn request(
    session: &Session,
    u: &ParsedUrl,
    extra_headers: &str,
) -> Result<(Session, u32), String> {
    unsafe {
        let host = wide(&u.host);
        let conn = WinHttpConnect(session.0, host.as_ptr(), u.port as u32 as _, 0);
        if conn.is_null() {
            return Err(format!("WinHttpConnect tới {} thất bại", u.host));
        }
        let conn = Session(conn);
        let verb = wide("GET");
        let path = wide(&u.path);
        let flags = if u.secure { WINHTTP_FLAG_SECURE } else { 0 };
        let req = WinHttpOpenRequest(
            conn.0,
            verb.as_ptr(),
            path.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            flags,
        );
        if req.is_null() {
            return Err("WinHttpOpenRequest thất bại".to_string());
        }
        let req = Session(req);
        let headers = wide(extra_headers);
        let ok = WinHttpSendRequest(
            req.0,
            if extra_headers.is_empty() { ptr::null() } else { headers.as_ptr() },
            if extra_headers.is_empty() { 0 } else { u32::MAX }, // (DWORD)-1: chuỗi kết thúc NUL
            ptr::null_mut(),
            0,
            0,
            0,
        );
        if ok == 0 {
            return Err("WinHttpSendRequest thất bại".to_string());
        }
        if WinHttpReceiveResponse(req.0, ptr::null_mut()) == 0 {
            return Err("WinHttpReceiveResponse thất bại".to_string());
        }
        let mut status: u32 = 0;
        let mut len: u32 = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            &mut status as *mut u32 as *mut c_void,
            &mut len,
            ptr::null_mut(),
        );
        // Trả cả request handle (trong Session) để nơi gọi đọc dữ liệu
        let raw = req.0;
        std::mem::forget(req);
        Ok((Session(raw), status))
    }
}

fn content_length(req: HINTERNET) -> Option<u64> {
    unsafe {
        let mut val: u32 = 0;
        let mut len: u32 = std::mem::size_of::<u32>() as u32;
        let ok = WinHttpQueryHeaders(
            req,
            WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            &mut val as *mut u32 as *mut c_void,
            &mut len,
            ptr::null_mut(),
        );
        if ok != 0 {
            Some(val as u64)
        } else {
            None
        }
    }
}

/// Đọc toàn bộ response vào bộ nhớ (chỉ dùng cho file nhỏ: catalogue JSON).
pub fn get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let u = parse_url(url)?;
    let session = open_session()?;
    let (req, status) = request(&session, &u, "")?;
    if status != 200 {
        return Err(format!("HTTP {} khi tải {}", status, url));
    }
    let mut out = Vec::new();
    read_all(req.0, &mut |chunk| {
        out.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(out)
}

/// POST một thân JSON qua WinHTTP (v0.7.27 — dùng cho thông báo
/// trạng thái cài đặt: Telegram Bot API + relay Zalo). Khác đường
/// tải file ở trên: timeout cấu hình được cả bốn pha (resolve /
/// connect / send / receive đều = `timeout_ms`, 0 = mặc định
/// 30 giây) để nơi gọi cần phản hồi nhanh (thông báo best-effort)
/// không bị treo lâu. Trả (HTTP status, toàn bộ thân phản hồi);
/// status khác 200 KHÔNG phải lỗi ở tầng này — nơi gọi tự phán
/// theo API của đích (Telegram/relay đều trả JSON kèm mô tả lỗi).
pub fn post_json(url: &str, body: &str, timeout_ms: u32) -> Result<(u32, Vec<u8>), String> {
    let u = parse_url(url)?;
    let session = open_session()?;
    let t = if timeout_ms == 0 {
        30_000
    } else {
        timeout_ms.min(120_000) as i32
    };
    unsafe {
        WinHttpSetTimeouts(session.0, t, t, t, t);
        let host = wide(&u.host);
        let conn = WinHttpConnect(session.0, host.as_ptr(), u.port as u32 as _, 0);
        if conn.is_null() {
            return Err(format!("WinHttpConnect tới {} thất bại", u.host));
        }
        let conn = Session(conn);
        let verb = wide("POST");
        let path = wide(&u.path);
        let flags = if u.secure { WINHTTP_FLAG_SECURE } else { 0 };
        let req = WinHttpOpenRequest(
            conn.0,
            verb.as_ptr(),
            path.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            flags,
        );
        if req.is_null() {
            return Err("WinHttpOpenRequest thất bại".to_string());
        }
        let req = Session(req);
        let headers = wide("Content-Type: application/json\r\n");
        let bytes = body.as_bytes();
        let ok = WinHttpSendRequest(
            req.0,
            headers.as_ptr(),
            u32::MAX, // (DWORD)-1: chuỗi kết thúc NUL
            bytes.as_ptr() as *mut c_void,
            bytes.len() as u32,
            bytes.len() as u32,
            0,
        );
        if ok == 0 {
            return Err("WinHttpSendRequest thất bại".to_string());
        }
        if WinHttpReceiveResponse(req.0, ptr::null_mut()) == 0 {
            return Err("WinHttpReceiveResponse thất bại".to_string());
        }
        let mut status: u32 = 0;
        let mut len: u32 = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            &mut status as *mut u32 as *mut c_void,
            &mut len,
            ptr::null_mut(),
        );
        let mut out = Vec::new();
        read_all(req.0, &mut |chunk| {
            out.extend_from_slice(chunk);
            Ok(())
        })?;
        Ok((status, out))
    }
}

fn read_all<F: FnMut(&[u8]) -> Result<(), String>>(req: HINTERNET, sink: &mut F) -> Result<(), String> {
    unsafe {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut avail: u32 = 0;
            if WinHttpQueryDataAvailable(req, &mut avail) == 0 {
                return Err("WinHttpQueryDataAvailable thất bại".to_string());
            }
            if avail == 0 {
                return Ok(());
            }
            let want = (avail as usize).min(buf.len());
            let mut read: u32 = 0;
            if WinHttpReadData(req, buf.as_mut_ptr() as *mut c_void, want as u32, &mut read) == 0 {
                return Err("WinHttpReadData thất bại".to_string());
            }
            if read == 0 {
                return Ok(());
            }
            sink(&buf[..read as usize])?;
        }
    }
}

pub struct DownloadOutcome {
    /// Tổng số byte của file sau khi tải xong
    pub total_bytes: u64,
    /// Số byte đã có sẵn từ lần tải trước (0 = tải mới hoàn toàn)
    pub resumed_from: u64,
}

/// Tải `url` xuống `dest`, resume nếu `dest` đã tồn tại dở dang.
/// `progress(done, total_estimate)` được gọi sau mỗi khối 64 KiB.
pub fn download_file<F: FnMut(u64, u64)>(
    url: &str,
    dest: &Path,
    mut progress: F,
) -> Result<DownloadOutcome, String> {
    let u = parse_url(url)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("không tạo được thư mục: {}", e))?;
    }
    let existing: u64 = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
    let session = open_session()?;

    // Thử resume nếu đã có dữ liệu
    let mut start_at: u64 = 0;
    let mut req_status: u32 = 0;
    let mut req_handle: Option<Session> = None;
    if existing > 0 {
        let headers = format!("Range: bytes={}-\r\n", existing);
        if let Ok((req, status)) = request(&session, &u, &headers) {
            if status == 206 {
                start_at = existing;
                req_status = status;
                req_handle = Some(req);
            } else {
                // Server không hỗ trợ Range -> tải lại từ đầu ở dưới
                drop(req);
            }
        }
    }
    let (req, _status) = match req_handle {
        Some(r) => (r, req_status),
        None => {
            let (r, s) = request(&session, &u, "")?;
            if s != 200 {
                return Err(format!("HTTP {} khi tải {}", s, url));
            }
            (r, s)
        }
    };
    let body_len = content_length(req.0).unwrap_or(0);
    let total_estimate = start_at + body_len;

    let mut file = if start_at > 0 {
        std::fs::OpenOptions::new()
            .append(true)
            .open(dest)
            .map_err(|e| format!("không mở được file để nối tiếp: {}", e))?
    } else {
        std::fs::File::create(dest).map_err(|e| format!("không tạo được file: {}", e))?
    };
    let mut done = start_at;
    progress(done, total_estimate);
    read_all(req.0, &mut |chunk| {
        file.write_all(chunk)
            .map_err(|e| format!("lỗi ghi file: {}", e))?;
        done += chunk.len() as u64;
        progress(done, total_estimate);
        Ok(())
    })?;
    file.flush().ok();
    Ok(DownloadOutcome { total_bytes: done, resumed_from: start_at })
}
