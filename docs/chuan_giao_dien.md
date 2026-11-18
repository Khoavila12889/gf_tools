# CHUẨN GIAO DIỆN — Utility Rebuild (áp dụng từ v0.7.20)

Tài liệu này là **nguồn chuẩn duy nhất** về diện mạo của bộ công cụ:
launcher, mọi module và icon phải trông như **một sản phẩm**. Mọi giá
trị cụ thể đều đã được hiện thực trong `rust/qsu_common/src/ui.rs` —
module mới **không tự khai báo lại** font/màu/lề, chỉ tiêu thụ từ đó.
Bản v0.7.20 là bản **chỉ đổi lớp giao diện**: hành vi, luồng công
việc, cổng an toàn và nội dung chữ giữ nguyên 100%.

---

## 1. Tổng kết rà soát (Phase 1 — trạng thái trước v0.7.20)

Mỗi module được xây ở một phiên làm việc khác nhau nên dù "trông
giống nhau", chi tiết lệch nhau ở nhiều chỗ:

| Cửa sổ | Kích thước | Font | Header | Icon |
|---|---|---|---|---|
| Launcher | 1120×900 | tự khai báo | tiêu đề (18,10), phụ đề (20,50) | 9/32 nút |
| Wizard Cài Đặt Win | 1036×748 | tự khai báo (5 font) | (16,6) / (16,36), phụ đề font_small | không |
| Cài Đặt Win = WinPE | 1060×940 (+ trình duyệt file 720×520) | tự khai báo (6 font) | macro riêng, (16,6)/(16,36) | không |
| Backup/Restore Driver | 1000×714 | tự khai báo | (16,8)/(16,40) | không |
| Tắt BitLocker | 820×544 | tự khai báo | (16,8)/(16,40), nội dung lề 14 | không |
| Sao Lưu Wifi | 870×694 | tự khai báo | (16,8)/(16,40), nội dung lề 14 | không |
| Dọn Rác | 840×664 | tự khai báo | (16,8)/(16,40) | không |
| Quản Lý Update | 880×664 | tự khai báo (+Consolas) | (16,8)/(16,40) | không |
| Phím Tắt BIOS | 800×604 | tự khai báo (chỉ 3 font) | HEADER_H=66 riêng, lề trộn 14/16 | không |
| Tuỳ Chỉnh Windows | 1080×784 | tự khai báo (+Consolas) | (16,8)/(16,40) | không |
| Engine WinPEInstall | — | — | không có cửa sổ NWG (console + màn tiến trình toàn màn hình) | — |

**Các điểm không nhất quán chính:**

1. **Font bị sao chép tay ở 10 nơi.** Cùng một bộ số (Segoe UI
   21/17/15/14) được khai báo lại y hệt trong từng crate kèm nghi
   thức `Box::leak` — sửa một nơi là lệch cả bộ. PhimTatBIOS thiếu
   hẳn font nút/font mono.
2. **Lề 14 và 16 lẫn nhau** ngay trong cùng một cửa sổ (header 16,
   nội dung 14); toạ độ header lệch vài pixel giữa các module
   (y = 6/8/10).
3. **Icon nửa vời:** chỉ 9/32 nút launcher có glyph (commit
   114c02e), 23 nút còn lại trống; **không module nào đặt icon cửa
   sổ** — title bar/taskbar toàn icon mặc định của Windows.
4. **Ô log không theo quy tắc:** nơi dùng font chữ nhỏ (Segoe UI
   14), nơi dùng Consolas — cùng là log kỹ thuật.
5. **Launcher tự mâu thuẫn:** hai đường tính lưới nút (lúc dựng và
   lúc lọc tìm kiếm) lệch nhau tới ~9px; nút danh mục cao 36 trong
   khi mọi nút bấm khác cao 32; ô tìm kiếm cao 32 so với chuẩn ô
   nhập 26–28 của các module.
6. **Chiều cao nút trong cùng một hàng lệch nhau** (32 cạnh 34)
   ở Quản Lý Update, Backup Driver, Tuỳ Chỉnh Windows.

---

## 2. Design tokens (trong `qsu_common::ui`)

### 2.1. Font — Segoe UI, một thang duy nhất

| Getter | Font | Dùng cho |
|---|---|---|
| `ui::font_title()` | Segoe UI 21, bold 700 | Tiêu đề cửa sổ (header) |
| `ui::font_section()` | Segoe UI 17, bold 700 | Tiêu đề mục trong cửa sổ |
| `ui::font_normal()` | Segoe UI 15 | Nhãn, ô nhập, phụ đề, nội dung |
| `ui::font_small()` | Segoe UI 14 | Chú thích, trạng thái ngắn |
| `ui::font_button()` | Segoe UI 15, bold 700 | Nút bấm |
| `ui::font_mono()` | Consolas 14 | Ô log kỹ thuật, dữ liệu dạng cột |

Đây là đúng bộ số đã chạy ổn định ở thực địa từ v0.5 — v0.7.20
không đổi kích thước hiển thị, chỉ gom về một nơi. Mỗi getter xây
font một lần cho cả tiến trình (rò có chủ đích như quy ước cũ).

### 2.2. Màu

- Nền/control: **màu hệ thống mặc định của Windows** (giao diện
  sáng chuẩn Windows như quyết định từ v0.5) — không tự sơn nền.
- `ui::ACCENT = (0,120,215)` — màu nhấn Windows, dùng làm tham
  chiếu cho glyph/điểm nhấn khi control cho phép.
- `ui::GLYPH_INK = (40,93,145)` — màu mực của bộ icon (thép xanh).
- `ui::DANGER = (196,43,28)` — đỏ lỗi WinUI, dành cho nhãn cảnh
  báo phá huỷ ở nơi control đặt được màu chữ.
- **Nút bấm native không nhuộm màu được** nếu không vẽ tay
  (owner-draw) — chuẩn này cố ý KHÔNG vẽ tay nút: nút chính thể
  hiện bằng font nút + viền nút mặc định (mục 2.4), hành động phá
  huỷ thể hiện bằng câu chữ + hộp xác nhận sẵn có, không bằng màu.

### 2.3. Spacing — lưới 8px, lề 16

| Token | Giá trị | Ý nghĩa |
|---|---|---|
| `ui::MARGIN` | 16 | Lề trái/phải chuẩn của cửa sổ |
| `ui::GAP` | 8 | Khoảng cách chuẩn giữa hai control |
| `ui::HEADER_H` | 72 | Chiều cao dải header; nội dung bắt đầu từ y=72 |
| `ui::BUTTON_H` | 32 | Chiều cao chuẩn của nút bấm |
| `ui::INPUT_H` | 28 | Chiều cao chuẩn của ô nhập |

### 2.4. Header chuẩn + nút chính

- Mọi cửa sổ module mở đầu bằng **một dải header duy nhất** qua
  `ui::add_header(&window, title, subtitle)`:
  - Tiêu đề: vị trí (16, 8), kích thước (760, 30), `font_title`.
  - Phụ đề một dòng: vị trí (16, 40), kích thước (960, 22),
    `font_normal` — mô tả ngắn việc module làm, giọng văn như các
    phụ đề hiện có.
- **Nút hành động chính** của cửa sổ được đánh dấu bằng
  `ui::make_primary(&btn)` — thêm style `BS_DEFPUSHBUTTON` (viền
  đậm của nút mặc định Windows), vẫn là nút native. Mỗi cửa sổ tối
  đa **một** nút chính; nút phá huỷ không được làm nút chính nếu
  chưa qua hộp xác nhận (mọi luồng hiện tại đều đã có xác nhận).
- Hộp thoại dùng `ui::info_box` / `ui::error_box` /
  `ui::confirm_box` (Yes/No + icon Warning, trả `bool`) — cùng mẫu
  hộp thoại mọi module vẫn dùng, gom về một nơi.

### 2.5. Log và trạng thái

- Ô log kỹ thuật (đa dòng, chứa lệnh/đường dẫn/tiến trình):
  `font_mono`. Ô/trạng thái dạng câu văn: `font_small`.
- Thanh tiến độ: dùng control ProgressBar native sẵn có của từng
  module; trong WinPE, màn hình tiến trình toàn màn hình là bề mặt
  riêng (mục 4).

### 2.6. Icon cửa sổ

- Mọi cửa sổ chính (và cửa sổ phụ như trình duyệt file của module
  WinPE) đặt **cùng một icon** qua `ui::set_window_icon(&window)`:
  file `rust/qsu_common/assets/qsu.ico` (16/24/32/48) — nền bo góc
  thép xanh + glyph cửa sổ 4 ô trắng, nhúng thẳng trong qsu_common.

---

## 3. Chuẩn icon launcher

- **Một ngôn ngữ hình:** glyph nét đơn sắc, bút 2px, mực thép xanh
  RGB(40,93,145) trên nền đen (0,0,0) — đen là màu khoá trong suốt
  của `ImageList_AddMasked`; canvas 32×32; hình học primitives
  (đường thẳng, chữ nhật, ellipse, cung); **không chữ, không đổ
  bóng, không gradient**.
- **Đủ 32/32 nút**, mỗi file module đúng một glyph (bảng ánh xạ
  trong `rust/launcher/src/icons.rs`). 9 glyph gốc của commit
  114c02e (unlock, cpu, disk, sliders, document, shield, refresh,
  trash, app-minus) giữ nguyên toạ độ vẽ; 23 glyph còn lại vẽ mới
  cùng ngôn ngữ (irst, windows, key, driver, usb, auto, winpe,
  office-online, office-offline, activate, package, backup-data,
  wifi, globe, chat, gauge, keyboard, printer, printer-fix,
  share, folder, swap, multi-chat).
- **Nguồn sinh:** `packaging/draw-icons.py` (Pillow) — chạy lại
  script là tái sinh toàn bộ `rust/launcher/assets/*.bmp` (đúng
  định dạng BMP 32bpp 4150 byte như bản System.Drawing cũ) +
  `qsu.ico` + bảng đối chiếu `/tmp/qsu-icons-contact.png` để kiểm
  mắt. `packaging/draw-icons.ps1` là bản cũ, giữ lại để tham chiếu
  toạ độ 9 glyph gốc.
- Gắn icon vào nút vẫn bằng cơ chế cũ: `Bitmap::from_bin` +
  ImageList 32×32 + `BCM_SETIMAGELIST` (nút native, hình ở cạnh
  trái, lề hình 8/4).

---

## 4. Ngoại lệ đã biết (có chủ đích, đừng "sửa" theo chuẩn)

1. **Wizard Cài Đặt Win:** header giữ (16,6)/(16,36) và phụ đề
   `font_small` — khung tab bắt đầu ở y=60 nên header chuẩn (phụ
   đề kết thúc ở 62) không vừa. Wizard có engine bố cục thích ứng
   riêng sở hữu toàn bộ hình học; v0.7.20 chỉ gom font + icon.
2. **Module WinPE:** ngoài header chuẩn còn một dòng nhãn môi
   trường/mode riêng ở (16,64) — đặc thù của module (báo đang ở
   WinPE hay Windows thường), giữ lại dưới phụ đề.
3. **Màn hình tiến trình WinPE toàn màn hình** (v0.7.18, thiết kế
   đã duyệt theo ảnh mẫu): giữ nguyên bố cục và thương hiệu; không
   thuộc chuẩn NWG của tài liệu này.
4. **PhimTatBIOS:** cột nội dung giữ nguyên lề cũ (14) vì hàng tìm
   kiếm căn theo nhãn + ô nhập ở x=92 — dịch lề sẽ lệch hàng; chỉ
   gom font/header/icon.
5. **Kích thước cửa sổ** của từng module giữ nguyên theo nội dung
   (800×604 … 1120×900) — chuẩn chỉ cố định font, lề, header,
   chiều cao nút/ô nhập, không ép một cỡ cửa sổ chung.

---

## 5. Áp dụng cho module tương lai (checklist)

1. Thêm `qsu_common` vào `Cargo.toml` (mọi module đều đã có, trừ
   khi là crate hoàn toàn mới).
2. Font: chỉ dùng sáu getter `ui::font_*()` — không khai báo
   `Font::builder` trong module.
3. Sau khi build cửa sổ: `ui::set_window_icon(&window);`
4. Header: `let (_t, _s) = ui::add_header(&window, "Tên Module", "Phụ đề một dòng");`
   rồi bắt đầu nội dung từ `ui::HEADER_H`.
5. Nút cao `ui::BUTTON_H` (32), ô nhập cao `ui::INPUT_H` (28),
   lề trái/phải `ui::MARGIN` (16), khoảng cách `ui::GAP` (8).
   Hàng nút phải đồng đều một chiều cao.
6. Một nút chính duy nhất: `ui::make_primary(&btn)`; hành động
   phá huỷ luôn qua hộp xác nhận trước khi chạy.
7. Log kỹ thuật → TextBox với `ui::font_mono()`; trạng thái câu
   văn → nhãn `ui::font_small()`.
8. Hộp thoại: `ui::info_box` / `ui::error_box` / `ui::confirm_box`.
9. Nút launcher cho module mới: thêm một glyph vào
   `packaging/draw-icons.py` (cùng mực, cùng bút 2px, không chữ),
   chạy lại script, thêm một dòng ánh xạ trong
   `rust/launcher/src/icons.rs`.

---

## 6. Thay đổi theo cửa sổ ở v0.7.20 (tóm tắt)

- **qsu_common:** thêm module `ui` (tokens + helpers + icon nhúng)
  và phụ thuộc `native-windows-gui`; thêm `assets/qsu.ico`.
- **Launcher:** font từ `ui`; header về toạ độ chuẩn (16,8)/(16,40);
  lưới nút dùng **một** công thức chung cho cả lúc dựng và lúc lọc
  (lề 16, khe 12); ô tìm kiếm cao 28; nút danh mục cao 32; icon đủ
  32/32 nút; icon cửa sổ; hộp thoại dùng bản của `ui`.
- **Wizard:** font từ `ui` (giữ tên biến, ~100 điểm gọi không
  đổi); icon cửa sổ. Hình học/header giữ nguyên (ngoại lệ 1).
- **Module WinPE:** font từ `ui`; header chuẩn; nhãn mode dời
  (16,58)→(16,64) cho khỏi chồng phụ đề; icon cho cả cửa sổ chính
  và trình duyệt file; nút BẮT ĐẦU CÀI ĐẶT là nút chính; log đã
  mono sẵn, nay lấy từ `ui`.
- **Backup/Restore Driver:** font + header chuẩn + icon; nút
  Backup là nút chính; hàng nút hành động đồng đều 32.
- **Tắt BitLocker:** font + header chuẩn + icon; cột nội dung
  dịch lề 14→16 đồng bộ; nút Tắt BitLocker là nút chính.
- **Sao Lưu Wifi:** như BitLocker; nút Xuất hồ sơ là nút chính.
- **Dọn Rác:** font + header chuẩn + icon; ô log chuyển sang
  Consolas (font_mono); nút Dọn rác là nút chính.
- **Quản Lý Update:** font + header chuẩn + icon; log đã mono;
  nút Bật cập nhật tự động là nút chính (hành động khôi phục,
  không phá huỷ); cả hàng 5 nút đồng đều 32.
- **Phím Tắt BIOS:** thêm phụ thuộc qsu_common; font + header
  chuẩn + icon (ngoại lệ 4 về lề).
- **Tuỳ Chỉnh Windows:** font + header chuẩn + icon; log mono từ
  `ui`; nút Áp dụng mục đã tick là nút chính; hàng nút đồng đều 32.
- **Engine WinPEInstall:** không đổi mã nguồn; build lại vì thư
  viện chung mở rộng — màn tiến trình và hành vi giữ nguyên.
