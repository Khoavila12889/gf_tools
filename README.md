# Utility Rebuild — Bộ công cụ hỗ trợ kỹ thuật Windows

Bản dựng lại **độc lập (clean-room)** của bộ tiện ích "Quick System Utility
v2026.1.3 - Pro" (32 chức năng), viết hoàn toàn mới bằng **Rust** — mỗi module
là một file `.exe` native độc lập, giao diện tiếng Việt. Mục tiêu là **tương
đương về chức năng** với công cụ gốc mà người dùng đang sử dụng hợp pháp,
không sao chép mã nguồn, tài nguyên đồ họa hay văn bản của bản gốc (ngoại trừ
các nhãn nút chức năng — vốn thuần túy mô tả tính năng).

**Phiên bản hiện tại: v0.7.28** — bản chốt trên nhánh `final`
(nhánh maintain về sau); xem lịch sử chi tiết trong
[BUILD_NOTES.md](BUILD_NOTES.md) và tài liệu kiểm thử đầy đủ trong
[docs/README_FULL.md](docs/README_FULL.md).

## Trạng thái

- **10/32 module đã hoạt động thật** (liệt kê bên dưới); các nút còn lại trả
  lời "Chưa có module" và được xây dựng **từng module một** khi có yêu cầu.
- Module trọng điểm — **Cài Đặt Win Tự Động** — đã chạy trọn vẹn trên máy
  thật (máy test chuyên dụng, vòng lặp: chạy thử → gửi log → chẩn đoán →
  sửa → bản mới): wizard staging → engine trong WinPE chia đĩa, bung ảnh,
  gắn boot → Windows mới tự chạy qua OOBE vào desktop bằng tài khoản đã
  đặt, không cần người trông. Riêng ảnh Windows rút gọn (Tiny11 Core)
  từng lỗi CRITICAL_PROCESS_DIED (0xEF) ở lần boot đầu — đã chẩn đoán
  đến gốc (entry boot do bcdboot dựng theo BCD-Template đã bị sửa trong
  ảnh) và sửa ở v0.7.25 bằng bước chuẩn hoá entry boot; kiểm chứng thực
  địa thành công ngày 2026-11-20.

## Các module đã xây dựng

| Module | Mô tả ngắn |
|---|---|
| Launcher | Cửa sổ chính: lưới nút chức năng + tìm kiếm |
| Cài Đặt Win Tự Động | Wizard phía Windows + engine cài đặt chạy trong WinPE (chi tiết bên dưới) |
| Cài Đặt Win (WinPE) | Trình cài tương tác chạy ngay trong WinPE, dùng chung engine |
| Backup / Restore Driver | Sao lưu & khôi phục driver (port từ source đã dịch ngược, đối chiếu CRC) |
| Tắt BitLocker | Giải mã / tắt BitLocker cho ổ hệ thống |
| Sao Lưu WiFi | Xuất & khôi phục các profile WiFi đã lưu |
| Dọn Rác Máy Tính | Dọn file tạm, cache, thùng rác… theo danh mục tick chọn |
| Quản Lý Update Windows | Bật/tắt Windows Update theo chính sách |
| Phím Tắt BIOS | Tra cứu phím vào BIOS/Boot Menu theo hãng máy |
| Tuỳ Chỉnh Windows | 49 tinh chỉnh + gỡ 27 Appx, mỗi mục có hoàn tác, hỏi tạo điểm khôi phục trước |

## Module Cài Đặt Win Tự Động

Luồng hai giai đoạn:

1. **Wizard (Windows):** quét hệ thống → chọn ISO (gắn ISO 3 lớp: dùng lại
   bản đã gắn / PowerShell có bắt lỗi đầy đủ / API Virtual Disk dự phòng) →
   chọn edition → sao lưu driver → chép bộ cài vào phân vùng dữ liệu
   (staging) → nạp engine vào ảnh WinPE → **đóng gói lại ảnh PE thành WIM
   một ảnh bootable** → đăng ký BCD một lần (bootsequence) → restart.
2. **Engine (WinPE):** tìm staging bằng file marker + serial (không tin ký
   tự ổ), đối chiếu serial đĩa đích, thực thi theo một trong **3 chế độ
   phân vùng**:
   - **Giữ phân vùng** — chỉ format phân vùng Windows cũ.
   - **Xoá toàn bộ ổ đĩa (wipe)** — chia lại chuẩn GPT: EFI 260 MB + MSR
     500 MB + Recovery 1024 MB (đặt trước phân vùng Windows) + Windows
     (phần còn lại của đĩa). Đĩa chứa dữ
     liệu staging **không bao giờ** là đĩa đích.
   - **Cài lại phân vùng hệ thống (rebuild)** — chỉ xoá/tạo lại EFI, MSR,
     Windows cũ (và Recovery nếu tick) trên cùng đĩa; phân vùng DATA khoá
     cứng không thể tick.
   Sau chia phân vùng, engine **tự gán ký tự ổ cho phân vùng boot** nếu
   thiếu (kiểm chứng bằng truy cập đường dẫn thật), rồi bung ảnh bằng DISM,
   cài driver đã sao lưu, sinh `autounattend.xml` (mật khẩu admin **tuỳ
   chọn** — cài hàng loạt được để trống), gắn boot bằng `bcdboot` + chuẩn hoá entry boot (v0.7.25), chép
   WinRE và tự restart vào Windows mới.

**An toàn:** thao tác phá huỷ bắt buộc gõ đúng **serial đĩa đích** để xác
nhận (giao diện hiện đầy đủ tên đĩa + serial và báo khớp/chưa khớp trực
tiếp); đĩa đích luôn được nhận diện bằng serial, sai serial là dừng cứng.

**Hiển thị tiến trình trong WinPE (v0.7.18):** màn hình toàn màn hình có
màu theo mẫu trình cài đặt quen thuộc — bước hiện tại, số % rất lớn (lấy
trực tiếp từ DISM khi bung ảnh), thanh tiến trình, đồng hồ thời gian chạy
từng giây, cảnh báo "Vui lòng KHÔNG tắt máy" — kèm dòng tiến trình dạng
chữ trên console khởi động làm kênh dự phòng. Khi lỗi, màn hình đứng lại
ở đúng bước lỗi kèm thông báo và đường dẫn log.

## Kiến trúc & mã nguồn

- **Ngôn ngữ:** Rust (workspace nhiều crate), biên dịch đích
  `x86_64-pc-windows-gnu`; mỗi module một exe native, tự nâng quyền admin
  một lần, tác vụ dài chạy trên luồng nền, log ghi cạnh exe / trong
  `<Ổ dữ liệu>:\QSU-Logs`.
- **Cấu trúc thư mục:**

  ```
  utility_rebuild/
  ├── README.md
  ├── BUILD_NOTES.md            ← nhật ký thay đổi từng phiên bản
  ├── docs/
  │   ├── README_FULL.md        ← tài liệu đầy đủ cho người kiểm thử
  │   └── co_che_v*.md          ← cơ chế module cài Win theo từng bản
  └── rust/
      ├── qsu_common/           ← thư viện dùng chung (đĩa, lệnh, log…)
      ├── launcher/             ← cửa sổ chính
      ├── cai_dat_win/          ← wizard Cài Đặt Win Tự Động
      ├── winpe_install/        ← engine cài đặt trong WinPE
      └── …                     ← mỗi module một crate
  ```

## Chính sách trung thực chức năng (fidelity policy)

Mỗi module được xây dựng để **hành xử tương đương** bản gốc ở phần chức
năng phục vụ người dùng. Các hành vi sau của bản gốc **cố ý KHÔNG tái
tạo**:

- Cổng kiểm tra key bản quyền / kích hoạt online và biến môi trường
  "token" mà các module gốc dùng để tự khóa.
- Gửi telemetry (thông tin máy/người dùng) về server của tác giả.
- Tự cài chứng thư gốc (root CA) của tác giả vào máy.
- Tự thêm ngoại lệ Windows Defender cho chính chương trình.
- Cơ chế kích hoạt KMS bên thứ ba — module kích hoạt chỉ hỗ trợ key
  chính hãng.

## Build

```bash
cd rust
cargo build --release --target x86_64-pc-windows-gnu
```

Gói phát hành nội bộ gồm launcher + các module exe + `HUONG-DAN.txt`;
mã nguồn trong repo này là nguồn sự thật duy nhất của bản rebuild.
