//! Thông báo trạng thái cài đặt (v0.7.27) — tab "Thông báo" của
//! wizard + engine WinPE dùng chung module này.
//!
//! Hai kênh, cả hai đều là hạ tầng của CHÍNH kỹ thuật viên:
//! - Telegram: bot Telegram của kỹ thuật viên (tạo qua @BotFather),
//!   wizard/engine gọi thẳng Bot API bằng Bot Token + Chat ID lưu
//!   trong settings.json cạnh wizard.
//! - Zalo qua relay Golden Farm: một relay nhỏ chạy trên server
//!   của đơn vị (cạnh bot Zalo) nhận trạng thái rồi chuyển tiếp
//!   vào Zalo của kỹ thuật viên. Tool chỉ POST về địa chỉ relay —
//!   không đăng nhập Zalo, không giữ phiên Zalo nào.
//!
//! Ranh giới nội dung (cố định): tin CHỈ gồm loại trạng thái (bắt
//! đầu / cài xong / lỗi + bước dừng), tên máy và thời gian. TUYỆT
//! ĐỐI không gửi thông tin đăng nhập nào (mật khẩu, ID/mật khẩu
//! công cụ remote...). Token/secret không bao giờ xuất hiện trong
//! thông điệp lỗi trả về (nơi gọi ghi các lỗi này vào log): lỗi
//! của đường Telegram được lọc bỏ Bot Token trước khi trả.

use crate::http;
use crate::json::{self, Json};
use crate::log;

/// Timeout cho mọi lần gửi thông báo: ngắn để best-effort không
/// làm chậm ca cài (WinPE có thể không có mạng — gửi lỗi chỉ ghi
/// cảnh báo ở nơi gọi, không chặn bước nào).
pub const TIMEOUT_MS: u32 = 8_000;

/// Lọc một giá trị bí mật khỏi chuỗi lỗi (đường Telegram nhúng
/// Bot Token trong URL nên lỗi tầng HTTP có thể mang theo nó).
fn sanitize(err: String, secret: &str) -> String {
    if secret.is_empty() {
        err
    } else {
        err.replace(secret, "***")
    }
}

fn machine_label(machine: &str) -> &str {
    let m = machine.trim();
    if m.is_empty() {
        "(tên máy do Windows tự sinh)"
    } else {
        m
    }
}

/// Tin BẮT ĐẦU — wizard gửi ngay khi staging hoàn tất.
pub fn text_start(machine: &str, image_desc: &str, mode_desc: &str) -> String {
    format!(
        "BẮT ĐẦU CÀI WINDOWS\nMáy: {}\nẢnh: {}\nChế độ: {}\nThời gian: {}",
        machine_label(machine),
        image_desc,
        mode_desc,
        log::now_string()
    )
}

/// Tin CÀI XONG — engine gửi ngay trước lệnh reboot vào Windows mới.
pub fn text_done(machine: &str) -> String {
    format!(
        "CÀI WINDOWS XONG\nMáy: {}\nThời gian: {}",
        machine_label(machine),
        log::now_string()
    )
}

/// Tin LỖI — engine gửi khi ca cài dừng ở một bước (`step` là tên
/// bước/pha đang dừng, lấy từ màn hình tiến trình).
pub fn text_error(machine: &str, step: &str) -> String {
    format!(
        "CÀI WINDOWS LỖI\nMáy: {}\nBước dừng: {}\nThời gian: {}",
        machine_label(machine),
        step,
        log::now_string()
    )
}

/// Tin GỬI THỬ từ tab "Thông báo" của wizard.
pub fn text_test(machine: &str) -> String {
    format!(
        "TIN NHẮN THỬ — QSU Utility Rebuild\nCấu hình thông báo trạng thái cài đặt đã hoạt động.\nMáy đang cấu hình: {}\nThời gian: {}",
        machine_label(machine),
        log::now_string()
    )
}

/// Gửi một tin qua bot Telegram của kỹ thuật viên.
/// POST https://api.telegram.org/bot<token>/sendMessage.
pub fn telegram_send(bot_token: &str, chat_id: &str, text: &str) -> Result<(), String> {
    let token = bot_token.trim();
    let chat = chat_id.trim();
    if token.is_empty() {
        return Err("thiếu Bot Token Telegram".to_string());
    }
    if chat.is_empty() {
        return Err("thiếu Chat ID Telegram".to_string());
    }
    let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
    let body = json::stringify(&json::obj(vec![
        ("chat_id", json::s(chat)),
        ("text", json::s(text)),
    ]));
    let (status, resp) =
        http::post_json(&url, &body, TIMEOUT_MS).map_err(|e| sanitize(e, token))?;
    if status != 200 {
        // Telegram trả JSON {"ok":false,"description":"..."} —
        // description không chứa token, trích ra cho dễ chẩn đoán.
        let desc = json::parse(&String::from_utf8_lossy(&resp))
            .ok()
            .and_then(|v| {
                v.get("description")
                    .and_then(|d| d.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();
        return Err(if desc.is_empty() {
            format!("Telegram trả HTTP {}", status)
        } else {
            format!("Telegram từ chối: {}", desc)
        });
    }
    Ok(())
}

/// Lấy Chat ID từ getUpdates: kỹ thuật viên nhắn trước 1 tin bất
/// kỳ cho bot của mình, hàm này đọc tin gần nhất và trả
/// (chat_id, tên người gửi / tên nhóm).
pub fn telegram_fetch_chat_id(bot_token: &str) -> Result<(String, String), String> {
    let token = bot_token.trim();
    if token.is_empty() {
        return Err("thiếu Bot Token Telegram".to_string());
    }
    let url = format!("https://api.telegram.org/bot{}/getUpdates", token);
    let bytes = http::get_bytes(&url).map_err(|e| sanitize(e, token))?;
    let text = String::from_utf8_lossy(&bytes);
    let root = json::parse(&text)
        .map_err(|e| format!("phản hồi Telegram không phải JSON hợp lệ: {}", e))?;
    let results = root
        .get("result")
        .and_then(|r| r.as_arr())
        .ok_or_else(|| "phản hồi Telegram thiếu mảng result".to_string())?;
    // Duyệt từ cuối về đầu: ưu tiên tin nhắn gần nhất.
    for item in results.iter().rev() {
        let msg = item
            .get("message")
            .or_else(|| item.get("edited_message"))
            .or_else(|| item.get("channel_post"));
        let msg = match msg {
            Some(m) => m,
            None => continue,
        };
        let chat = match msg.get("chat") {
            Some(c) => c,
            None => continue,
        };
        let id = match chat.get("id").and_then(|v| v.as_f64()) {
            Some(n) => (n as i64).to_string(),
            None => continue,
        };
        // Tên hiển thị: ưu tiên người gửi (first/last name,
        // fallback username), rồi tới tiêu đề nhóm / tên chat.
        let from_name = msg.get("from").and_then(|f| {
            let first = f.str_or("first_name", "");
            let last = f.str_or("last_name", "");
            let full = format!("{} {}", first, last).trim().to_string();
            if !full.is_empty() {
                Some(full)
            } else {
                let u = f.str_or("username", "");
                if u.is_empty() {
                    None
                } else {
                    Some(format!("@{}", u))
                }
            }
        });
        let name = from_name
            .or_else(|| {
                let t = chat.str_or("title", "");
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            })
            .or_else(|| {
                let f = chat.str_or("first_name", "");
                if f.is_empty() {
                    None
                } else {
                    Some(f)
                }
            })
            .unwrap_or_else(|| "(không rõ tên)".to_string());
        return Ok((id, name));
    }
    Err("chưa thấy tin nhắn nào gửi cho bot — hãy mở Telegram, nhắn 1 tin bất kỳ cho bot của bạn rồi bấm lại.".to_string())
}

fn relay_base(relay_url: &str) -> String {
    relay_url.trim().trim_end_matches('/').to_string()
}

/// POST một endpoint của relay và parse JSON phản hồi. Chấp nhận
/// HTTP 200 (kèm "ok": true nếu relay có trả trường đó); lỗi mang
/// theo mô tả "error" của relay nếu có. Thân POST chứa secret /
/// token nhưng lỗi tầng HTTP không bao giờ chứa thân POST, nên
/// không cần lọc ở đây.
fn relay_post(relay_url: &str, path: &str, body: Json) -> Result<Json, String> {
    let base = relay_base(relay_url);
    if base.is_empty() {
        return Err("thiếu Relay URL".to_string());
    }
    let url = format!("{}{}", base, path);
    let (status, resp) = http::post_json(&url, &json::stringify(&body), TIMEOUT_MS)?;
    let parsed = json::parse(&String::from_utf8_lossy(&resp)).unwrap_or(Json::Null);
    let relay_err = parsed
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if status != 200 {
        return Err(if relay_err.is_empty() {
            format!("relay trả HTTP {}", status)
        } else {
            format!("relay từ chối: {}", relay_err)
        });
    }
    if let Some(ok) = parsed.get("ok").and_then(|v| v.as_bool()) {
        if !ok {
            return Err(if relay_err.is_empty() {
                "relay báo không thành công".to_string()
            } else {
                format!("relay từ chối: {}", relay_err)
            });
        }
    }
    Ok(parsed)
}

/// Gửi một tin qua relay bằng SECRET dài hạn (đường của wizard:
/// tin BẮT ĐẦU + tin gửi thử). POST {relay}/notify.
pub fn relay_send(relay_url: &str, secret: &str, text: &str) -> Result<(), String> {
    if secret.trim().is_empty() {
        return Err("thiếu Relay Secret".to_string());
    }
    relay_post(
        relay_url,
        "/notify",
        json::obj(vec![
            ("secret", json::s(secret.trim())),
            ("text", json::s(text)),
        ]),
    )?;
    Ok(())
}

/// Xin TOKEN DÙNG MỘT LẦN từ relay (wizard gọi lúc staging, trước
/// khi ghi stage.json). Token do relay cấp, hết hạn ≤ 12 giờ, tối
/// đa 3 tin — engine trong WinPE dùng nó để gửi tin kết thúc mà
/// không cần (và không được) giữ secret dài hạn.
/// POST {relay}/notify-register → {"ok":true,"token":"..."}.
pub fn relay_register(relay_url: &str, secret: &str, machine: &str) -> Result<String, String> {
    if secret.trim().is_empty() {
        return Err("thiếu Relay Secret".to_string());
    }
    let resp = relay_post(
        relay_url,
        "/notify-register",
        json::obj(vec![
            ("secret", json::s(secret.trim())),
            ("machine", json::s(machine_label(machine))),
        ]),
    )?;
    let token = resp.str_or("token", "");
    if token.is_empty() {
        return Err("relay không trả token dùng một lần".to_string());
    }
    Ok(token)
}

/// Gửi một tin qua relay bằng TOKEN DÙNG MỘT LẦN (đường của
/// engine trong WinPE: tin CÀI XONG / LỖI).
/// POST {relay}/notify-once.
pub fn relay_send_once(relay_url: &str, token: &str, text: &str) -> Result<(), String> {
    if token.trim().is_empty() {
        return Err("thiếu token dùng một lần".to_string());
    }
    relay_post(
        relay_url,
        "/notify-once",
        json::obj(vec![
            ("token", json::s(token.trim())),
            ("text", json::s(text)),
        ]),
    )?;
    Ok(())
}
