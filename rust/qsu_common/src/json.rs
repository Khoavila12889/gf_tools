//! JSON tối giản (parser đệ quy + ghi) — đủ cho catalogue ISO, settings
//! và stage config của bộ công cụ. Không phụ thuộc serde để giữ bản build
//! gọn và không cần tải crate ngoài.

use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(m) => m.get(key),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        self.as_f64().map(|n| n as u64)
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_arr(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }
    /// Lấy chuỗi con với giá trị mặc định
    pub fn str_or(&self, key: &str, default: &str) -> String {
        self.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or(default)
            .to_string()
    }
    pub fn u64_or(&self, key: &str, default: u64) -> u64 {
        self.get(key).and_then(|v| v.as_u64()).unwrap_or(default)
    }
    pub fn bool_or(&self, key: &str, default: bool) -> bool {
        self.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
    }
}

pub fn parse(text: &str) -> Result<Json, String> {
    // Bỏ BOM UTF-8 nếu có (Notepad và nhiều trình soạn thảo trên Windows
    // ghi BOM đầu file settings.json / catalogue — không có BOM JSON mới hợp lệ).
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let bytes = text.as_bytes();
    let mut p = Parser { b: bytes, i: 0 };
    p.skip_ws();
    let v = p.value()?;
    p.skip_ws();
    if p.i != bytes.len() {
        return Err(format!("ký tự thừa tại vị trí {}", p.i));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn peek(&self) -> Result<u8, String> {
        self.b.get(self.i).copied().ok_or_else(|| "hết dữ liệu".to_string())
    }
    fn value(&mut self) -> Result<Json, String> {
        match self.peek()? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => Ok(Json::Str(self.string()?)),
            b't' => self.keyword("true", Json::Bool(true)),
            b'f' => self.keyword("false", Json::Bool(false)),
            b'n' => self.keyword("null", Json::Null),
            _ => self.number(),
        }
    }
    fn keyword(&mut self, kw: &str, v: Json) -> Result<Json, String> {
        if self.b[self.i..].starts_with(kw.as_bytes()) {
            self.i += kw.len();
            Ok(v)
        } else {
            Err(format!("từ khoá không hợp lệ tại {}", self.i))
        }
    }
    fn object(&mut self) -> Result<Json, String> {
        self.i += 1; // {
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek()? == b'}' {
            self.i += 1;
            return Ok(Json::Obj(map));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            if self.peek()? != b':' {
                return Err(format!("thiếu ':' tại {}", self.i));
            }
            self.i += 1;
            self.skip_ws();
            let val = self.value()?;
            map.insert(key, val);
            self.skip_ws();
            match self.peek()? {
                b',' => {
                    self.i += 1;
                }
                b'}' => {
                    self.i += 1;
                    return Ok(Json::Obj(map));
                }
                c => return Err(format!("ký tự '{}' không hợp lệ tại {}", c as char, self.i)),
            }
        }
    }
    fn array(&mut self) -> Result<Json, String> {
        self.i += 1; // [
        let mut arr = Vec::new();
        self.skip_ws();
        if self.peek()? == b']' {
            self.i += 1;
            return Ok(Json::Arr(arr));
        }
        loop {
            self.skip_ws();
            arr.push(self.value()?);
            self.skip_ws();
            match self.peek()? {
                b',' => {
                    self.i += 1;
                }
                b']' => {
                    self.i += 1;
                    return Ok(Json::Arr(arr));
                }
                c => return Err(format!("ký tự '{}' không hợp lệ tại {}", c as char, self.i)),
            }
        }
    }
    fn string(&mut self) -> Result<String, String> {
        if self.peek()? != b'"' {
            return Err(format!("mong đợi chuỗi tại {}", self.i));
        }
        self.i += 1;
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            self.i += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = self.peek()?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) {
                                // surrogate cao: cần cặp tiếp theo
                                if self.peek()? == b'\\' {
                                    self.i += 1;
                                    if self.peek()? == b'u' {
                                        self.i += 1;
                                        let lo = self.hex4()?;
                                        let c = 0x10000
                                            + (((cp - 0xD800) as u32) << 10)
                                            + (lo as u32 - 0xDC00);
                                        out.push(char::from_u32(c).unwrap_or('\u{FFFD}'));
                                        continue;
                                    }
                                }
                                out.push('\u{FFFD}');
                            } else {
                                out.push(char::from_u32(cp as u32).unwrap_or('\u{FFFD}'));
                            }
                        }
                        _ => return Err(format!("escape không hợp lệ tại {}", self.i)),
                    }
                }
                _ => {
                    // Byte đầu của một ký tự UTF-8: gom đủ dãy byte
                    let start = self.i - 1;
                    let len = utf8_len(c);
                    let end = (start + len).min(self.b.len());
                    self.i = end;
                    match std::str::from_utf8(&self.b[start..end]) {
                        Ok(s) => out.push_str(s),
                        Err(_) => out.push('\u{FFFD}'),
                    }
                }
            }
        }
    }
    fn hex4(&mut self) -> Result<u16, String> {
        if self.i + 4 > self.b.len() {
            return Err("thiếu 4 ký tự hex".into());
        }
        let s = std::str::from_utf8(&self.b[self.i..self.i + 4])
            .map_err(|_| "hex không hợp lệ".to_string())?;
        self.i += 4;
        u16::from_str_radix(s, 16).map_err(|_| "hex không hợp lệ".to_string())
    }
    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        while self.i < self.b.len()
            && matches!(self.b[self.i], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
        {
            self.i += 1;
        }
        let s = std::str::from_utf8(&self.b[start..self.i]).unwrap_or("");
        s.parse::<f64>()
            .map(Json::Num)
            .map_err(|_| format!("số không hợp lệ tại {}", start))
    }
}

fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first >> 5 == 0b110 {
        2
    } else if first >> 4 == 0b1110 {
        3
    } else if first >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

/// Ghi Json ra chuỗi (đẹp, thụt 2 khoảng) — dùng cho state/stage config.
pub fn stringify(v: &Json) -> String {
    let mut s = String::new();
    write_val(&mut s, v, 0);
    s
}

fn write_val(out: &mut String, v: &Json, indent: usize) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) => {
            if n.fract() == 0.0 && n.is_finite() && *n >= -9.0e15 && *n <= 9.0e15 {
                let _ = write!(out, "{}", *n as i64);
            } else {
                let _ = write!(out, "{}", n);
            }
        }
        Json::Str(s) => write_str(out, s),
        Json::Arr(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (i, item) in a.iter().enumerate() {
                out.push_str(&"  ".repeat(indent + 1));
                write_val(out, item, indent + 1);
                if i + 1 < a.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&"  ".repeat(indent));
            out.push(']');
        }
        Json::Obj(m) => {
            if m.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            let n = m.len();
            for (i, (k, val)) in m.iter().enumerate() {
                out.push_str(&"  ".repeat(indent + 1));
                write_str(out, k);
                out.push_str(": ");
                write_val(out, val, indent + 1);
                if i + 1 < n {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&"  ".repeat(indent));
            out.push('}');
        }
    }
}

fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn obj(pairs: Vec<(&str, Json)>) -> Json {
    Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}
pub fn s(v: &str) -> Json {
    Json::Str(v.to_string())
}
pub fn n(v: u64) -> Json {
    Json::Num(v as f64)
}
pub fn b(v: bool) -> Json {
    Json::Bool(v)
}
