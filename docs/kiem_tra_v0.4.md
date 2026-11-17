# Báo cáo rà soát / QA — Utility Rebuild v0.4

Ngày kiểm tra: 2026-11-16. Phạm vi: toàn bộ 11 crate trong `rust/` (launcher,
qsu_common, cai_dat_win, winpe_install, backup_restore_driver, tat_bitlocker,
sao_luu_wifi, don_rac, quan_ly_update, phim_tat_bios, tuy_chinh_windows) + các
hợp đồng giữa chúng + gói drop_v0.4. Phương pháp: đọc toàn bộ mã nguồn theo
checklist 7 mục, đối chiếu chéo wizard ↔ winpe_install ↔ settings ↔ stage.json,
grep panic/unwrap/TODO, build + clippy, đối chiếu hash binary trong drop với
bản build. Không chạy thử trên Windows thật (chưa có máy test) — các điểm chỉ
xác nhận được trên Windows được ghi rõ "Cần test Windows để xác nhận".

Kết quả tổng: **3 lỗi Nghiêm trọng, 4 lỗi Trung bình, 6 lỗi Nhỏ — tất cả đã
sửa**; 6 phát hiện dạng đề xuất/ghi nhận (không sửa vì sẽ đổi hành vi đã duyệt
hoặc cần Windows thật để xác nhận). Sau khi sửa: `cargo build --release` sạch
**0 errors, 0 warnings**; clippy chỉ còn 4 cảnh báo phong cách (không phải lỗi).
Gói đã đóng lại: `UtilityRebuild-v0.4.zip` = **8.535.473 byte**.

---

## NGHIÊM TRỌNG (đã sửa)

### N1. WinPEInstall có đường dự phòng format ổ theo "thư mục Windows" khi thiếu serial
- **Vị trí:** `rust/winpe_install/src/main.rs`, hàm `find_os_drive`.
- **Vấn đề:** Nếu `osVolumeSerial` trong stage.json bằng 0, engine dự phòng
  bằng cách chọn phân vùng đầu tiên (khác ổ Data) có thư mục `\Windows` rồi
  FORMAT nó. Đây là dự phòng theo ký tự ổ/heuristic — trái nguyên tắc an toàn
  đã duyệt trong đặc tả §6 ("chỉ đối chiếu serial, không bao giờ theo ký tự
  ổ"). Máy nhiều bản Windows/nhiều đĩa có thể bị format nhầm phân vùng.
- **Đã sửa:** Xoá hẳn đường dự phòng. Serial bằng 0 hoặc không khớp bất kỳ
  phân vùng nào → dừng an toàn (exit 4), không đụng vào ổ nào. Wizard luôn ghi
  serial thật của ổ OS nên luồng chuẩn không bị ảnh hưởng.

### N2. Dọn Rác có thể xoá đệ quy vào đích của junction/symlink trong %TEMP%
- **Vị trí:** `rust/don_rac/src/main.rs`, hàm `delete_dir_contents`.
- **Vấn đề:** Hàm phân loại mục bằng `std::fs::metadata` — API này ĐI THEO
  liên kết. Một junction/symlink nằm trong thư mục Temp trỏ sang thư mục dữ
  liệu thật (ví dụ Documents) sẽ bị coi là thư mục thường và bị
  `remove_dir_all` xoá cả nội dung đích. Đây là lỗ hổng phá vỡ cam kết
  "không bao giờ đụng Downloads/Documents".
- **Đã sửa:** Phân loại bằng `DirEntry::file_type` (không đi theo liên kết):
  symlink/junction chỉ bị xoá như một liên kết; thư mục thật mới đệ quy.

### N3. `stagingFolder` rỗng/chứa đường dẫn khiến wizard xoá đệ quy cả gốc ổ Data
- **Vị trí:** `rust/qsu_common/src/settings.rs` (`Settings::load`) +
  `rust/cai_dat_win/src/main.rs` (`exec_clean`).
- **Vấn đề:** `exec_clean` chạy `remove_dir_all(<Data>:\<stagingFolder>)` để
  dọn staging cũ. Nếu settings.json của người dùng ghi `stagingFolder` là
  `""`, `"."` hoặc `".."` thì đích xoá thành gốc ổ Data hoặc thoát ra ngoài —
  nguy cơ xoá dữ liệu thật chỉ vì một file cấu hình viết sai.
- **Đã sửa (2 lớp):** (1) `Settings::load` nay kiểm tra hợp lệ cho cả ba
  trường thư mục (downloadFolder/logsFolder/stagingFolder): phải là TÊN thư
  mục đơn, không rỗng, không chứa `\`, `/`, `:` — sai thì về giá trị mặc định.
  (2) `exec_clean` có chốt chặn bổ sung: từ chối chạy nếu đường dẫn staging
  trùng gốc ổ Data hoặc gốc ổ OS.

---

## TRUNG BÌNH (đã sửa)

### T1. Wizard có thể panic khi báo lỗi SHA-256 không khớp → giao diện kẹt vĩnh viễn
- **Vị trí:** `rust/cai_dat_win/src/main.rs`, `spawn_download`.
- **Vấn đề:** Thông báo lỗi cắt chuỗi bằng chỉ số byte
  (`&img.sha256[..len.min(16)]`). `sha256` lấy từ catalogue là dữ liệu ngoài;
  nếu chứa ký tự đa byte (UTF-8), vị trí cắt không đúng biên ký tự sẽ gây
  panic, giết luồng tải — `dl_done` không bao giờ được đặt, nút "Tải ISO"
  kẹt disabled và quá trình tải treo không thông báo.
- **Đã sửa:** Cắt tiền tố bằng `chars().take(16)` cho cả hai chuỗi.

### T2. Marker trên ổ OS khiến ổ OS bị chọn làm "ổ Data mặc định"
- **Vị trí:** `rust/cai_dat_win/src/main.rs`, `Preflight::default_data_letter`
  + `on_start`.
- **Vấn đề:** Đường cài đè trên máy chỉ có một ổ đĩa sẽ tạo marker
  `.qsu_data_drive` ngay trên ổ OS (vì ổ Data được gán tạm = ổ OS). Lần chạy
  sau, logic "ưu tiên ổ có marker" chọn chính ổ OS làm ổ Data → nếu người
  dùng chọn Cài mới, staging sẽ nằm trên ổ OS và WinPE huỷ ở bước đối chiếu
  (an toàn nhưng hỏng việc, khó hiểu).
- **Đã sửa:** `default_data_letter` bỏ qua marker nằm trên ổ OS; `on_start`
  chặn rõ ràng chế độ Cài mới khi ổ Data chọn trùng ổ OS, kèm thông báo
  tiếng Việt giải thích.

### T3. Dọn Rác tick sẵn TẤT CẢ hạng mục có dữ liệu — trái tài liệu và thiết kế đã duyệt
- **Vị trí:** `rust/don_rac/src/main.rs`, `scan_items`.
- **Vấn đề:** Mọi hạng mục có dung lượng > 0 đều được tick sẵn, kể cả Thùng
  rác, cache Windows Update, cache Chrome/Edge — trong khi HUONG-DAN.txt và
  thiết kế đã duyệt ghi "chỉ tick sẵn các mục an toàn". Người dùng bấm qua
  hộp xác nhận theo thói quen có thể xoá nhiều hơn dự định.
- **Đã sửa:** Chỉ tick sẵn Temp người dùng, Temp Windows, cache
  thumbnail/icon, Delivery Optimization. Thùng rác / cache WU / cache trình
  duyệt mặc định không tick.

### T4. "Đặt lại Windows Update" âm thầm đổi kiểu khởi động CryptSvc khỏi mặc định Windows
- **Vị trí:** `rust/quan_ly_update/src/main.rs`, `action_reset`.
- **Vấn đề:** Trước khi chạy lại dịch vụ, code đặt TẤT CẢ về Manual
  (START_DEMAND) — kể cả `cryptsvc`, mà mặc định Windows của nó là Automatic.
  Sau một lần "đặt lại", cấu hình máy bị lệch khỏi mặc định mà không ai hay.
- **Đã sửa:** Chỉ can thiệp khi dịch vụ đang bị Disabled, và khôi phục đúng
  mặc định từng dịch vụ (cryptsvc → Automatic; bits/wuauserv/UsoSvc →
  Manual). Dịch vụ ở kiểu khởi động khác thì giữ nguyên.

---

## NHỎ (đã sửa)

### H1. Parser JSON không bỏ BOM UTF-8
- **Vị trí:** `rust/qsu_common/src/json.rs`, `parse`.
- settings.json/catalogue lưu từ Notepad (có BOM) sẽ báo "lỗi cú pháp" và bị
  bỏ qua. **Đã sửa:** tự loại BOM đầu chuỗi trước khi parse.

### H2. Nhận diện GPT/MBR luôn trả "không rõ" trên hầu hết đĩa thật
- **Vị trí:** `rust/qsu_common/src/sys.rs`, `disk_partition_style`.
- Buffer 64 byte không đủ chứa toàn bộ kết quả `IOCTL_DISK_GET_DRIVE_LAYOUT_EX`
  (mọi phân vùng) nên lời gọi thất bại trên đĩa nhiều phân vùng; wizard hiện
  "Boot Mode: UEFI (không rõ)" dù máy hoàn toàn bình thường. **Đã sửa:**
  buffer heap 64 KiB. (Chỉ ảnh hưởng hiển thị, không chặn cài đặt.)

### H3. Dọn Rác cộng dung lượng thùng rác vào "đã giải phóng" kể cả khi xoá thất bại
- **Vị trí:** `rust/don_rac/src/main.rs`, nhánh `CleanKind::RecycleBin`.
- **Đã sửa:** Chỉ cộng khi `SHEmptyRecycleBinW` trả S_OK (0).

### H4. WinPEInstall gọi cả hai lệnh reboot sau khi cài xong
- **Vị trí:** `rust/winpe_install/src/main.rs`, cuối `main`.
- `wpeutil reboot` chạy xong vẫn chạy tiếp `shutdown /r /t 10` (vô hại trong
  WinPE nhưng sai ngữ nghĩa "dự phòng"). **Đã sửa:** chỉ chạy shutdown khi
  wpeutil lỗi; câu log cũng sửa cho khớp.

### H5. settings.example.json đặt ở gốc gói trong khi wizard chỉ đọc cạnh exe
- **Vị trí:** `drop_v0.4/settings.example.json` + `Settings::load` (đọc tại
  thư mục chứa CaiDatWinTuDong.exe, tức `modules\`).
- Người dùng copy theo file mẫu ở gốc gói có thể đặt settings.json sai chỗ và
  wizard không bao giờ đọc. **Đã sửa:** thêm bản copy vào
  `drop_v0.4/modules/settings.example.json`, viết lại `_comment` chỉ rõ phải
  đặt trong thư mục modules, cập nhật mục cấu trúc gói trong HUONG-DAN.txt.

### H6. Tài liệu trong stage.rs mô tả sai thiết kế bảo mật hiện tại
- **Vị trí:** `rust/qsu_common/src/stage.rs` (doc comment đầu file).
- Comment cũ vẫn ghi stage.json "chứa mật khẩu dạng rõ" — trái thiết kế cuối
  (wizard gán rỗng + loại hẳn 3 khoá khỏi JSON). **Đã sửa:** viết lại comment
  cho đúng thực tế, nêu rõ trường còn giữ chỉ vì tương thích lược đồ.

---

## PHÁT HIỆN CHƯA SỬA (đề xuất / cần xác nhận)

### D1. Nút gạt "Tuỳ Chỉnh Windows sau cài" của wizard hiện không có tác dụng — ĐỀ XUẤT, CHỜ DUYỆT (Trung bình)
`apply_tweaks` được ghi vào stage.json nhưng không thành phần nào tiêu thụ
(agent hậu cài đã bị loại khỏi bản rebuild). Người dùng bật lên sẽ tưởng
tweak được áp sau khi cài xong nhưng thực tế không có gì chạy. Đề xuất: bỏ
nút gạt, hoặc đổi thành ghi chú tĩnh "dùng module Tuỳ Chỉnh Windows sau khi
cài xong". Không tự sửa vì đây là thay đổi giao diện/hành vi đã duyệt.

### D2. Thoát im lặng khi người dùng huỷ UAC — ĐỀ XUẤT (Nhỏ)
Mọi module GUI dùng mẫu `if !is_admin() { relaunch; return }`: nếu người
dùng bấm No ở hộp UAC, tiến trình thoát không một thông báo. Đề xuất thêm
một hộp thoại "cần quyền Administrator" trước khi thoát.

### D3. Probe tác vụ bằng XML của schtasks — CẦN TEST WINDOWS ĐỂ XÁC NHẬN (Nhỏ)
`engine.rs` (`task_is_disabled`) tìm chuỗi `<Enabled>false</Enabled>` trong
output `schtasks /Query /XML` đã giải mã UTF-8 lossy. Nếu schtasks xuất
UTF-16 khi bị redirect, chuỗi sẽ không khớp và trạng thái CEIP/Appraiser
luôn hiện "Chưa áp dụng". Cần kiểm trên máy Windows thật (đặc biệt bản
tiếng Việt) trước khi kết luận.

### D4. mount_iso lấy ký tự ổ đĩa là ký tự chữ cái đầu tiên trong output PowerShell — CẦN TEST WINDOWS (Nhỏ)
Nếu PowerShell in dòng cảnh báo trước kết quả `Mount-DiskImage`, ký tự lấy
được sẽ sai. Thất bại theo hướng an toàn (không thấy `sources\install.*`
thì huỷ + dismount, không thao tác gì thêm) nên chỉ ghi nhận.

### D5. Cài đè: suspend BitLocker không đặt giới hạn số lần reboot — CẦN TEST WINDOWS (Nhỏ)
`manage-bde -protectors -disable` (không kèm `-RebootCount`) giữ protector
tắt cho tới khi được bật lại; Windows Setup thường tự bật lại sau nâng cấp.
Cần xác nhận trên máy test sau một vòng cài đè thật.

### D6. WU Reset dừng msiserver nhưng không khởi động lại — GHI NHẬN (Nhỏ)
msiserver là dịch vụ khởi động theo yêu cầu (demand-start) nên trạng thái
dừng là trạng thái nghỉ mặc định của nó; quy trình reset chính thức của
Microsoft cũng không khởi động lại dịch vụ này. Ghi nhận để minh bạch với
checklist "chạy lại mọi dịch vụ đã dừng".

### D7. Các ghi nhận nhỏ khác (Nhỏ)
- `http::download_file`: nếu file đích đã tải đủ từ lần trước, Range trả
  416 và code tải lại toàn bộ (lãng phí băng thông; tính đúng đắn vẫn được
  bảo đảm vì SHA-256 bắt buộc kiểm sau tải).
- Cam kết "stage.json không chứa mật khẩu" được thực thi tại điểm ghi của
  wizard (gán rỗng + remove khoá), chưa được đóng gói thành API dùng chung
  trong `qsu_common::stage` — caller tương lai tự điền StageConfig rồi gọi
  `save()` vẫn có thể ghi ra. Đề xuất dài hạn: thêm hàm ghi đã lược bỏ.
- Logger bỏ qua lỗi ghi file (ví dụ gói công cụ bị copy vào thư mục
  chỉ-đọc như Program Files): log mất im lặng, chương trình không crash.
- Clippy: 4 cảnh báo phong cách (2× "very complex type", 2× "casting to
  the same type", 1× "collapsible if") — không ảnh hưởng hành vi, không
  sửa để giữ diff tối thiểu.

---

## CÁC KHU VỰC ĐÃ KIỂM TRA VÀ SẠCH (không phát hiện lỗi)

1. **Dispatch launcher ↔ drop:** Bảng 32 nút trong `launcher/src/main.rs`
   khớp chính xác (phân biệt hoa/thường) với tên file; cả 8 exe trong
   `drop_v0.4/modules/` đều tới được từ một nút; không exe nào thừa.
   `WinPEInstall.exe` nằm đúng `modules\install\` — đúng vị trí đầu tiên
   wizard tìm (wizard cũng chấp nhận bản đặt cạnh exe). Binary trong drop
   trước đợt sửa khớp hash SHA-256 với bản build từ source; sau đợt sửa đã
   thay bằng binary mới và đóng zip lại.
2. **Hợp đồng stage.json:** Wizard ghi đủ và đúng kiểu mọi trường
   winpe_install đọc (`imageFile`, `imageSwm`, `imageIndex`, `bootMode`,
   `osVolumeSerial`, `dataVolumeSerial`, `logsFolder`, `stagingFolder`).
   Việc loại `apps`/`adminPassword`/`ultraviewerPassword` có thật trong
   code (gán rỗng + `remove` khỏi map JSON trước khi ghi), không chỉ là
   tuyên bố. Thư mục `custom`: wizard chép từ cạnh exe wizard vào
   staging, winpe_install chép tiếp vào `ProgramData\QSU\custom` — đường
   đi khớp tài liệu, nội dung chỉ copy, không thực thi.
3. **settings.example.json ↔ Settings model:** Mọi khoá trong file mẫu đều
   được `Settings::load` đọc đúng tên; giá trị mặc định trong mẫu khớp
   mặc định trong code (25 GB cài đè / 12 GB cài mới, QSU-Downloads,
   QSU-Logs, QSU-Stage, vi-VN, SE Asia Standard Time).
4. **An toàn panic:** Toàn bộ `unwrap()`/`expect()` còn lại nằm ở khởi tạo
   GUI (builder NWG — hỏng là hỏng ngay lúc mở, không phải giữa thao tác)
   hoặc tại các điểm đã có guard logic chứng minh được (unwrap sau khi đã
   kiểm `is_none`, `prev.unwrap()` trong nhánh đã chắc chắn `Some`,
   `lines.last_mut()` với `max_lines = 3` hằng số). Các parser output lệnh
   ngoài đều qua `from_utf8_lossy`, không cắt chuỗi theo byte trên dữ
   liệu ngoài (điểm duy nhất có nguy cơ là T1 — đã sửa).
5. **Phân tích output lệnh ngoài / ngôn ngữ:** DISM luôn kèm `/English`;
   trạng thái BitLocker qua enum JSON của `Get-BitLockerVolume` (không
   parse text `manage-bde` đã bản địa hoá); danh sách driver qua WMI COM
   (không parse `pnputil /enum-drivers`); hồ sơ Wifi đọc XML từ thư mục
   Wlansvc (netsh chỉ là dự phòng); GUID của bcdedit/powercfg trích bằng
   mẫu hex — tất cả đều không phụ thuộc ngôn ngữ hiển thị của Windows.
   Mã thoát lệnh ngoài được kiểm ở mọi thao tác ghi (pnputil chấp nhận
   0/259/3010 khi restore, còn lại đều yêu cầu 0).
6. **Chốt chặn thao tác phá huỷ:** TatBitlocker chỉ tắt qua một đường duy
   nhất có `confirm_box` kèm cảnh báo đầy đủ, không đường vòng; DonRac
   dùng danh sách đường dẫn cố định suy từ WINDIR/TEMP/LOCALAPPDATA
   (không nhận đường dẫn từ người dùng), có hộp xác nhận liệt kê từng
   mục + tổng dung lượng; winpe_install từ chối tuyệt đối ổ chứa marker
   Data ở cả hai lớp (đối chiếu serial + kiểm marker) trước khi format.
7. **Hoàn tác của Tuỳ Chỉnh Windows:** Đối chiếu từng tweak rủi ro cao —
   menu chuột phải cổ điển: apply đặt (Default)="" ở khoá InprocServer32,
   undo `DelTree` cả cây CLSID (xoá sạch, đúng hoàn tác); Ultimate
   Performance: GUID gói nhân bản được lưu vào file trạng thái cạnh exe,
   probe so GUID gói đang active, undo về Balanced; NumLock: apply/undo
   đối xứng cả HKCU lẫn `HKU\.DEFAULT`; các tweak dạng chính sách dùng
   `DelValue` khi hoàn tác (về "chưa cấu hình") thay vì ghi giá trị ngược
   tuỳ tiện. Đếm catalogue: 9 + 11 + 9 + 11 + 9 = 49 tweak tĩnh + 27 gói
   Appx (19 An toàn + 8 Cần cân nhắc) — khớp HUONG-DAN.txt.
8. **Nâng quyền & luồng:** Mọi module cần admin (trừ PhimTatBIOS thuần
   tham khảo và launcher) tự relaunch bằng verb `runas` đúng một lần khi
   `is_admin()` (TokenElevation) trả false; không có vòng lặp trong điều
   kiện bình thường. Mọi thao tác dài (quét WMI, tải ISO, DISM, pnputil,
   dọn rác, poll BitLocker, áp tweak) đều chạy ở luồng nền và báo về UI
   qua NWG Notice; luồng UI không chạy lệnh ngoài.
9. **WMI COM (BackupRestoreDriver):** Release đầy đủ locator/services/
   enumerator/object theo từng nhánh lỗi; `VariantClear` sau mỗi lần đọc
   thuộc tính; kiểm tra VT_BSTR trước khi đọc chuỗi.
10. **Appx (TuyChinhWindows):** PackageFullName được kiểm tra ký tự cho
    phép trước khi nội suy vào lệnh PowerShell; chỉ gỡ gói thuộc danh
    sách tuyển chọn và chỉ cho người dùng hiện tại; không đụng provisioned.
11. **Vệ sinh build:** Không còn TODO/FIXME/`dbg!`/`println!` debug trong
    code phát hành (một `eprintln!` duy nhất nằm trong winpe_install —
    chương trình console chạy trong WinPE, hợp lệ). Chuỗi tiếng Việt
    nhất quán; file log của từng module ghi cạnh exe với tên đã ghi trong
    HUONG-DAN.txt.

---

## Việc đã làm trong đợt rà soát này
- Sửa 13 lỗi (3 Nghiêm trọng, 4 Trung bình, 6 Nhỏ) như liệt kê ở trên.
- Build lại toàn workspace: 0 errors, 0 warnings.
- Cập nhật toàn bộ binary trong `drop_v0.4/`, bổ sung
  `modules/settings.example.json`, cập nhật HUONG-DAN.txt.
- Đóng lại `UtilityRebuild-v0.4.zip`: **8.535.473 byte**.
- Chưa mục nào trong đợt này được chạy thử trên Windows thật — vẫn áp
  dụng đầy đủ danh sách test đã ghi trong HUONG-DAN.txt, cộng thêm các
  điểm D3/D4/D5 ở trên.
