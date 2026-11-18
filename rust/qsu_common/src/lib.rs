//! qsu_common — thư viện dùng chung cho bộ Utility Rebuild (Rust).
//!
//! Gồm: SHA-256 thuần Rust, JSON tối giản, logger có timestamp, chạy
//! lệnh ngoài kèm thu output, tải file WinHTTP có resume, cấu hình
//! settings.json, stage config, và các truy vấn hệ thống Win32.

pub mod cmd;
pub mod disk;
pub mod http;
pub mod iso;
pub mod json;
pub mod log;
pub mod post_install;
pub mod settings;
pub mod sha256;
pub mod stage;
pub mod svc;
pub mod sys;
