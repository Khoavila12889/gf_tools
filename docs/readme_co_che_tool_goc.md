# README — Cơ chế hoạt động của module "Cài Đặt Windows Tự Động" (tool gốc)

Tài liệu tham chiếu công nghệ thực thi, tổng hợp từ log chạy thực địa ngày
2026-11-17 (máy PC UEFI thật, ISO mod `Windows11-Pro-HDD4G-26300.9457-x64.iso`,
cài thành công trọn vẹn). Mọi chi tiết ghi **[QUAN SÁT]** là đọc trực tiếp từ
log/ảnh chụp của lần chạy đó; chi tiết ghi **[SUY LUẬN]** là kết luận kỹ thuật
suy ra từ các dòng log, chưa được xác chứng trực tiếp.

Module thuộc Quick System Utility v2026.1.3 - Pro (Bùi Hữu Thắng). Giao diện
module có 2 tab: **"Cài Đè Trên Windows Hiện Tại"** và **"Cài Mới Từ WinPE
(USB)"**. Lần chạy thực địa dùng luồng cài mới KHÔNG cần USB: tool tự tạo môi
trường boot WinPE ngay trên ổ cứng, khởi động lại vào WinPE và WinPE tự bung
ảnh Windows.

---

## 1. Tổng quan kiến trúc

Ý tưởng cốt lõi: **biến chính ổ cứng của máy thành "USB boot"**.

1. Tách một phân vùng nhỏ ngay trên ổ đĩa hệ thống làm phân vùng boot
   chuyên dụng (đặt tên `WSAPBOOT`).
2. Đặt lên phân vùng đó: file boot lấy từ ISO + một file `boot.wim` WinPE
   đã được trích xuất, nhúng kịch bản tự chạy và đóng gói lại.
3. Nạp một mục vào BCD Boot Menu trỏ tới WinPE trên phân vùng boot.
4. Khởi động lại: firmware → BCD → WinPE (nạp vào RAM) → trình cài trong
   WinPE tự format ổ C: và bung ảnh `install.esd` đã chép sẵn sang ổ dữ
   liệu, nạp driver, gắn unattend, hoàn tất.

Không có bước nào kiểm tra "cờ bootable" của WIM bằng cách đọc output của
DISM. Tính bootable đạt được **bằng cấu trúc**: WIM được trích một index
duy nhất và đóng gói lại tươi, file boot lấy nguyên từ ISO, mục BCD theo
đúng công thức ramdisk.

---

## 2. Giai đoạn 0 — Kiểm tra tiền đề và chuẩn bị driver

**[QUAN SÁT]** Tool quét và hiển thị:

- Ổ OS (C:) và ổ Data (D:\) — ổ Data là nơi chứa file ISO và là nơi lưu
  bản sao lưu + bộ nguồn cài.
- Boot Mode: `UEFI (GPT)`.
- Trạng thái BitLocker ổ OS: "Đã giải mã hoàn toàn (An toàn)".
- Key OEM trong BIOS: không có (không phải máy OEM).

**[QUAN SÁT]** Kho driver IRST đặt cạnh tool, gồm 3 gói zip:

- `IntelRST-Gen.11-12-13.zip`
- `IntelRST-Gen.13-14-15.zip`
- `IntelRST-Core.Ultra.Series.3.zip`

Mỗi gói chứa 3 file `.inf`. Log ghi "cần đủ 3 gói driver" — thiếu gói là
không đạt điều kiện nạp driver. Mỗi gói được kiểm tra bằng file
`SHA256SUMS.txt` đi kèm; nếu không có file này, log cảnh báo "Không có
SHA256SUMS.txt để xác minh … (bỏ qua kiểm tra)" và vẫn tiếp tục.

Mục đích của driver IRST **[SUY LUẬN]**: máy Intel đời mới để chế độ
RAID/VMD thì WinPE và Windows mới không thấy ổ NVMe nếu thiếu driver
Intel RST — đây là lý do tool nhúng sẵn cả 3 thế hệ driver.

---

## 3. Giai đoạn sao lưu cấu hình hệ thống

**[QUAN SÁT]** Trước khi đụng vào ổ đĩa, tool sao lưu vào ổ Data (D:\):

- Driver phần cứng của Windows hiện tại.
- Mật khẩu WiFi đã lưu.
- UltraViewer ID/Pass.

Log: "Đã sao lưu Driver, WiFi và Ultraviewer vào ổ D:\". Tổng thời gian
giai đoạn này khoảng 35 giây (riêng bước mật khẩu WiFi chiếm phần lớn).

**[SUY LUẬN]** Các bản sao lưu này được khôi phục vào Windows mới sau khi
bung ảnh (driver để máy nhận đủ phần cứng ngay lần boot đầu, WiFi để máy
tự nối lại mạng, UltraViewer để kỹ thuật viên kết nối từ xa tiếp tục cài
phần mềm nghiệp vụ).

---

## 4. Bước 1 — Tạo phân vùng boot mồi (WSAPBOOT)

**[QUAN SÁT]** Chuỗi log:

```
>>> BẮT ĐẦU TẠO PHÂN VÙNG BOOT MỒI...
-> Tắt Hibernate và thực thi DiskPart để Shrink ổ C: (Có thể mất 10-30 giây)...
✓ Tạo phân vùng WSAPBOOT (W:\) thành công!
>>> HOÀN TẤT BƯỚC 1.
```

Cơ chế:

1. Tắt Hibernate (`powercfg /hibernate off`) — **[SUY LUẬN]** file
   `hiberfil.sys` nằm cuối vùng dữ liệu thường chặn shrink; tắt hibernate
   giúp DiskPart co ổ C: được sâu hơn và tránh xung đột trạng thái ngủ
   đông khi can thiệp phân vùng.
2. Dùng **DiskPart shrink** ổ C: để lấy ra một khoảng trống, tạo phân
   vùng mới trong khoảng trống đó, format và gán ký tự **W:**, đặt nhãn
   **WSAPBOOT**.

Điểm quan trọng: phân vùng boot nằm **trên cùng ổ đĩa vật lý với C:**
nhưng là một phân vùng độc lập — khi WinPE format/cài lại C:, phân vùng
này không bị đụng tới, và vì WinPE chạy từ RAM nên ngay cả khi cần xử lý
lại toàn bộ vùng phân vùng Windows, môi trường cài vẫn sống.

**[SUY LUẬN]** Dung lượng phân vùng không thấy trong log; ước lượng theo
nội dung chứa (boot.wim ~600 MB + file boot + $WinPEDriver$) thì khoảng
2–4 GB.

---

## 5. Bước 2 — Trích xuất ISO

**[QUAN SÁT]** Chuỗi log:

```
>>> TIẾN HÀNH BƯỚC 2 (TRÍCH XUẤT ISO).
-> Đang Mount file ISO ảo...
-> Dùng PowerShell Mount-DiskImage...
✓ Mount ISO thành công tại ổ đĩa: E:\
-> Đang chép file bộ cài gốc (install.esd) sang D:\WinSetupData\
   (robocopy mã thoát: 1)
✓ Đã chép xong và xác minh kích thước file bộ cài.
-> Đang chép thư mục sxs (Hỗ trợ .NET 3.5 offline)...
-> Phiên bản đã chọn: Index 1.
>>> HOÀN TẤT BƯỚC 2.
```

Cơ chế:

- Mount ISO bằng PowerShell `Mount-DiskImage` (không dùng phần mềm ổ ảo
  bên thứ ba).
- Chép **`install.esd`** (ảnh Windows) từ `E:\sources\` sang thư mục
  **`D:\WinSetupData\`** trên ổ Data bằng **robocopy**; mã thoát 1 của
  robocopy nghĩa là "đã chép thành công có file mới" — tool hiểu đúng mã
  này là thành công, không coi là lỗi. Sau chép có bước **đối chiếu kích
  thước file** để xác minh bản chép nguyên vẹn.
- Chép kèm thư mục **`sxs`** từ ISO sang để hỗ trợ bật .NET Framework
  3.5 offline trong Windows mới.
- Phiên bản cài được chọn theo **Index** trong install.esd (lần chạy này:
  Index 1 = Windows 11 Pro).

Lý do chép install.esd sang ổ Data thay vì để WinPE đọc từ ISO
**[SUY LUẬN]**: trong WinPE không còn ổ ảo ISO; bộ nguồn cài phải nằm
sẵn dạng file thường trên một phân vùng WinPE đọc được.

---

## 6. Bước 3 — Chuẩn bị WinPE + driver (bước quyết định)

**[QUAN SÁT]** Chuỗi log:

```
>>> TIẾN HÀNH BƯỚC 3 (CHUẨN BỊ WINPE + DRIVER).
-> Đang chép file mồi (bootmgr, boot, efi) sang W:\ ...
-> Đang trích WinPE (index 1) từ boot.wim...
-> Đang Mount WinPE để nhúng kịch bản tự chạy + driver...
-> Dùng ảnh nền BHT_BG.jpg cạnh tool.
✓ Đã nhúng giao diện tiến trình vào WinPE.
-> Đang nạp (Inject) Driver IRST vào WinPE...
✓ Đã nạp driver vào WinPE.
-> Chép Driver sang W:\$WinPEDriver$ cho Windows mới...
-> Đang đóng gói và lưu lại boot.wim...
-> Đang ngắt kết nối ổ đĩa ảo (Unmount)...
>>> HOÀN TẤT BƯỚC 3.
```

Cơ chế từng dòng:

1. **Chép file mồi**: `bootmgr`, thư mục `boot`, thư mục `efi` — lấy
   nguyên từ ISO — sang phân vùng W:. Đây là bộ file boot chuẩn của
   Microsoft (gồm cả `boot.sdi` trong thư mục `boot`), nên tương thích
   firmware theo đúng thiết kế của Microsoft cho cả UEFI và Legacy.
2. **Trích WinPE index 1**: từ `sources\boot.wim` của ISO, lấy **index
   1** — ảnh "Microsoft Windows PE" thuần. (Index 2 là "Microsoft
   Windows Setup" — tool KHÔNG dùng index 2.) Việc trích một index duy
   nhất rồi đóng gói lại ở cuối bước tạo ra một `boot.wim` mới chỉ có
   đúng 1 ảnh, và ảnh đó chính là ảnh boot — không phụ thuộc vào cờ
   bootable gốc của WIM trong ISO (ISO mod thường làm mất/sai cờ này
   khi đóng gói lại).
3. **Mount WinPE và nhúng**: mount ảnh WinPE đã trích, chép vào trong:
   kịch bản tự chạy (trình cài + giao diện tiến trình toàn màn hình có
   ảnh nền `BHT_BG.jpg` đặt cạnh tool), để khi WinPE khởi động nó tự
   chạy trình cài thay vì mở Windows Setup.
4. **Inject driver IRST vào WinPE** (DISM /Add-Driver): để WinPE thấy
   được ổ NVMe/RAID ngay khi khởi động.
5. **Chép driver sang `W:\$WinPEDriver$`**: thư mục driver đặt tại gốc
   phân vùng boot — **[SUY LUẬN]** đây là quy ước thư mục driver của
   Windows Setup/PnP: driver tại `$WinPEDriver$` được tự nạp cho Windows
   mới trong quá trình cài.
6. **Đóng gói lại boot.wim** (unmount /Commit hoặc capture lại) và lưu
   lên W:. Từ thời điểm này W: là một phân vùng boot WinPE hoàn chỉnh,
   độc lập.
7. Unmount ISO ảo.

---

## 7. Bước 4 — Kịch bản tự động cho Windows mới

**[QUAN SÁT một phần]** — log bị che một phần bởi hộp thoại, đọc được
các mảnh:

- Xử lý Windows cũ (đoạn log nhắc tới hậu tố tên máy "3V66T" và việc
  lưu trữ gì đó từ Windows cũ).
- Đổi nhãn ổ C: thành **'WinOS'** để trình cài trong WinPE nhận diện
  phân vùng đích theo nhãn, không phụ thuộc ký tự ổ (ký tự ổ trong
  WinPE bị xáo trộn khác hẳn trong Windows). **[ĐÃ XÁC CHỨNG thực địa
  — xem mục 12: kịch bản WinPE tìm đúng nhãn 'WinOS' và báo lỗi khi
  không thấy.]**
- Ghi các file kịch bản tự động: `unattend.xml` và các file `.cmd`
  (đoạn log liệt kê tên file bị cắt, thấy "…end.xml," và "…e.cmd,").

**[SUY LUẬN]** unattend.xml mang các lựa chọn: bỏ qua OOBE, tài khoản,
múi giờ/ngôn ngữ, và các lệnh chạy ở lần đăng nhập đầu (FirstLogonCommands)
để khôi phục sao lưu (driver/WiFi/UltraViewer) và hoàn tất sau cài.

---

## 8. Bước 5 — Nạp BCD và khởi động lại

**[QUAN SÁT]** Chuỗi log:

```
>>> TIẾN HÀNH BƯỚC 5 (NẠP BCD Boot Menu để tự khởi động...)
... Entry mới với GUID: {...951d9b00}
-> Nạp Bootloader Path: \windows\system32\boot\winload.efi
✓ Đã nạp Boot thành công! Khởi động lại sau 3 giây...
[ Nút trạng thái: ĐÃ HOÀN TẤT! ĐANG KHỞI ĐỘNG LẠI... ]
```

Cơ chế:

- Tạo một **OS loader entry mới** trong BCD (GUID mới), device trỏ tới
  ảnh ramdisk `boot.wim` trên phân vùng W: (kèm device options
  ramdisksdi trỏ `boot.sdi` trong thư mục `boot` đã chép) **[SUY LUẬN —
  đúng công thức ramdisk chuẩn]**.
- `path` = `\windows\system32\boot\winload.efi` vì máy UEFI (GPT).
- Entry được nạp vào **BCD Boot Menu** (hiển thị trong menu boot) —
  khác với cơ chế boot-một-lần: entry tồn tại trong menu để lần khởi
  động lại sau khi nạp là vào thẳng WinPE.
- Tool ra lệnh khởi động lại sau 3 giây; Windows hiện hộp "You're about
  to be signed out — Windows will shut down in less than a minute".

---

## 9. Giai đoạn trong WinPE — tự bung ảnh

**[QUAN SÁT]** Sau restart, máy boot vào WinPE trên W: và hiện giao
diện toàn màn hình của tool:

```
CÀI ĐẶT WINDOWS TỰ ĐỘNG
Hệ thống đang được cài đặt tự động - Vui lòng KHÔNG tắt máy
Đang khởi động trình cài đặt...  1%   Thời gian: 00:01
```

Trình cài chạy qua **`startnet.cmd`** (cửa sổ `X:\windows\system32\cmd.exe`,
banner "BHT AUTOMATED WINDOWS DEPLOYMENT") **[QUAN SÁT — mục 12]**. Nó
kiểm tra file cài đặt tại `WinSetupData\install.esd` (ký tự ổ trong
WinPE được gán lại, lần quan sát hiện thành `C:\WinSetupData\install.esd`)
và tìm phân vùng đích theo **nhãn 'WinOS'**; không tìm thấy thì **dừng
ngay, không thay đổi gì, quay về Windows cũ**.

Khi tìm thấy, luồng tiếp theo **[SUY LUẬN theo luồng chuẩn của cơ chế
này]**: format phân vùng đích, dùng DISM apply `install.esd` theo index
đã chọn, nạp driver từ `$WinPEDriver$`/driver đã sao lưu, gắn
unattend.xml vào Windows mới, tạo boot bằng bcdboot, dọn phân vùng
WSAPBOOT (hoặc để lại — chưa quan sát được), khởi động lại vào Windows
mới và hoàn tất các bước sau cài ở lần đăng nhập đầu.

---

## 10. Vì sao cơ chế này chạy được ở nơi cơ chế ramdisk-trên-ổ-Data vấp

| Điểm | Tool gốc | Bản rebuild (đến v0.7.11) |
|---|---|---|
| Nơi đặt WinPE | Phân vùng boot riêng tách từ chính ổ C: (WSAPBOOT) | Thư mục staging trên ổ Data, boot bằng ramdisk BCD |
| Ảnh WinPE | Trích index 1, đóng gói lại thành WIM 1 ảnh tươi | Dùng boot.wim nguyên trạng của ISO sau khi mount/inject |
| Cờ bootable | Không kiểm tra — đạt được bằng cấu trúc đóng gói lại | Từng có cổng kiểm tra bằng cách parse output DISM — kết luận sai cả với ISO gốc Microsoft (chặn 4/4 lần chạy thực địa), đã gỡ ở v0.7.11 theo yêu cầu |
| File boot | Chép nguyên `bootmgr`/`boot`/`efi` từ ISO | boot.sdi lấy theo chuỗi dự phòng từ Windows đang chạy |
| Nhận diện ổ đích trong WinPE | Theo nhãn ổ ('WinOS') | Theo serial volume + stage.json |
| Mục BCD | Entry mới trong Boot Menu | Entry ramdisk, boot một lần (/bootsequence) |

Ba bài học công nghệ rút ra:

1. **Đóng gói lại WIM 1 ảnh** là cách chuẩn hoá bootable đáng tin nhất:
   ảnh duy nhất trong WIM đóng gói lại chính là ảnh boot, không cần đọc
   cờ của WIM gốc (vốn bị các bản ISO mod làm sai lệch).
2. **Phân vùng boot riêng trên ổ hệ thống** loại bỏ mọi phụ thuộc vào
   ký tự ổ/thư mục staging trên ổ Data ở thời điểm boot, đồng thời vẫn
   an toàn vì WinPE chạy từ RAM.
3. **File boot lấy từ chính ISO** đảm bảo tương thích firmware tuyệt
   đối (boot.sdi/bootmgr/efi đồng bộ với bộ cài), thay vì lắp ghép từ
   Windows đang chạy.

---

## 11. Phụ lục — Dòng thời gian lần chạy thực địa 2026-11-17

| Mốc | Sự kiện |
|---|---|
| 01:15:47 | Kiểm tra 3 gói driver IRST (cảnh báo thiếu SHA256SUMS.txt, bỏ qua) |
| 01:15:50 | Bắt đầu sao lưu Driver / WiFi / UltraViewer vào D:\ |
| 01:16:25 | Sao lưu xong; bắt đầu Bước 1 — tắt Hibernate, DiskPart shrink C: |
| 01:16:28 | Tạo xong phân vùng WSAPBOOT (W:\); sang Bước 2 |
| 01:16:34 | Mount ISO tại E:\; bắt đầu chép install.esd → D:\WinSetupData\ |
| 01:17:31 | Chép xong install.esd (đã xác minh kích thước); chép sxs; chọn Index 1 |
| 01:17:33 | Bước 3: chép file mồi sang W:\, trích WinPE index 1 |
| 01:17:37–01:17:56 | Mount WinPE, nhúng giao diện/kịch bản, inject driver IRST, chép $WinPEDriver$ |
| 01:17:56–01:18:14 | Đóng gói lại boot.wim; unmount ISO |
| 01:18:17 | Bước 4 xong (unattend + kịch bản); sang Bước 5 |
| 01:18:18 | Nạp Bootloader Path `\windows\system32\boot\winload.efi` |
| 01:18:19 | "Đã nạp Boot thành công! Khởi động lại sau 3 giây" |
| Sau đó | Windows restart → WinPE khởi động → giao diện cài tự động hiện 1% |

Tổng thời gian giai đoạn chuẩn bị trên Windows: khoảng **2 phút rưỡi**.

*Máy đích: PC UEFI (GPT), ổ OS là SSD Gigabyte 120 GB (C:), ổ Data là
HDD Toshiba 500 GB (D:). ISO mod dùng chung với các lần chạy thử của
bản rebuild.*

---

## 12. Ca lỗi thực địa của chính tool gốc (cùng ngày, máy khác)

Chạy trên một máy khác (laptop), tool gốc đi qua hết giai đoạn chuẩn bị,
khởi động vào WinPE thành công — **cơ chế boot (phân vùng WSAPBOOT +
boot.wim đóng gói lại) hoạt động** — nhưng trình cài trong WinPE dừng
ngay ở bước kiểm tra đầu tiên. Nội dung màn hình `startnet.cmd`:

```
====================================================
BHT AUTOMATED WINDOWS DEPLOYMENT
====================================================
File cai dat: C:\WinSetupData\install.esd
Dang kiem tra file cai dat...
LOI: Khong tim thay o dia nhan WinOS
He thong CHUA bi thay doi. Dang quay lai Windows cu...
```

Phân tích:

- Điểm gãy nằm ở **cơ chế nhận diện phân vùng đích theo nhãn ổ
  ('WinOS')**: vì lý do nào đó (bước đổi nhãn ở Bước 4 không hiệu lực
  trên máy này, hoặc phân vùng Windows không hiện diện đúng kỳ vọng
  trong WinPE), kịch bản không tìm thấy ổ nhãn WinOS.
- Hành vi khi lỗi là **an toàn tuyệt đối**: chưa format, chưa bung
  ảnh, hệ thống nguyên vẹn và tự quay về Windows cũ — cùng triết lý
  fail-safe như bản rebuild.
- Bài học cho bản rebuild: nhận diện ổ đích **theo serial volume** (bản
  rebuild đang làm, ghi trong stage.json) bền hơn nhận diện theo nhãn —
  nhãn có thể bị đổi/trùng/không được áp dụng, serial thì không. Khi áp
  dụng kiến trúc phân vùng boot của tool gốc vào bản rebuild, giữ
  nguyên cơ chế nhận diện theo serial, KHÔNG chuyển sang nhãn.

---

## 13. Cơ chế thông báo Zalo/Telegram của tool gốc (quan sát 2026-11-17)

Ảnh chụp module "Cài Đặt Windows Tự Động AIO" chạy trong môi trường
**BHT_Boot_2026** (WinPE-USB "High-Tech Recovery Environment" của cùng
tác giả; máy Dell Latitude E7470, UEFI-GPT) cho thấy module có sẵn một
kênh **thông báo qua chatbot Zalo/Telegram**, cấu hình ngay trong giao
diện tool ("Hướng dẫn nhận thông báo Zalo / Telegram").

**[QUAN SÁT]** Hộp thoại "HƯỚNG DẪN KẾT NỐI NHẬN THÔNG BÁO":

- Bước 1: Quét mã QR bằng điện thoại để truy cập Chatbot (Zalo hoặc
  Telegram), bấm "Bắt đầu / START".
- Bước 2: Bấm "Lấy Chat ID" trong tool, sau đó gửi 1 tin nhắn bất kỳ
  cho Chatbot trên điện thoại. Lưu ý của tool: với Zalo, tool chỉ chờ
  tối đa **25 giây** — phải gửi tin ngay sau khi bấm.
- Bước 3: Sau khi lấy được Chat ID, bấm "Gửi thử tin nhắn"; tin hiện
  trên điện thoại là thành công.
- Cuối cùng bấm "Lưu Tuỳ Chọn" để tool nhớ thiết lập.

Cửa sổ module phía sau cho thấy các tuỳ chọn "GỬI ID…" và tên thông
báo — tức tool gửi thông tin định danh của ca cài về điện thoại kỹ
thuật viên qua bot theo Chat ID đã ghép nối.

**[SUY LUẬN + theo mô tả của người dùng]** Đây là mảnh ghép cuối của
luồng cài từ xa của tool gốc: sau khi cài xong (và ở các mốc tiến
trình), tool đẩy thông báo về Zalo/Telegram của kỹ thuật viên — theo
mô tả của người dùng, gồm cả ID/password UltraViewer của máy mới cài
để kỹ thuật viên kết nối vào cài tiếp phần mềm nghiệp vụ. Ghép nối
Chat ID là thao tác công khai một lần trong giao diện tool; kênh gửi
là bot của chính hệ sinh thái tác giả.

**Ghi chú cho bản rebuild:** thông báo *trạng thái* ca cài (bắt đầu /
tiến trình / hoàn tất / lỗi, kèm tên máy) qua bot do chính kỹ thuật
viên cấu hình là một tính năng hợp lệ và hữu ích cho cài từ xa. Phần
tự động thu thập và gửi credential truy cập (UltraViewer ID/password)
của máy người dùng cuối là phần bản rebuild chủ đích không làm.
