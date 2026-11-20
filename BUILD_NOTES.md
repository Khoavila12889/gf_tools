# BUILD NOTES — Trạng thái bản rebuild

Cập nhật theo từng giai đoạn. Quy ước trạng thái:

- **Built** — module đã viết xong, sẵn sàng test trên máy Windows.
- **Stub** — mới có khung/điểm vào, chưa có logic.
- **Pending source recovery** — chưa viết được vì source gốc chưa khôi phục
  được (31/32 module gốc bị bọc Enigma Protector — chỉ giải mã trong bộ nhớ
  khi chạy, không dịch ngược tĩnh được). Cần **memory dump** khi chạy module
  trong máy ảo Windows, hoặc **ảnh chụp giao diện (UI capture)** để dựng lại
  theo hành vi quan sát được.
- **Pending spec** — hành vi đã có bản đồ suy luận (INFERRED) đủ để viết,
  đang chờ xếp lịch/đối chiếu.

## Chuyển hướng sang Rust (2026-11-16)

Theo quyết định của người dùng, bản rebuild **chuyển từ PowerShell sang
Rust**: mỗi chương trình là một file .exe native duy nhất, không phụ thuộc
PowerShell trên máy đích. Đặc tả logic (đặc biệt
`docs/logic_cai_dat_win_online.md`) **không đổi** và vẫn là nguồn chuẩn —
chỉ thay ngôn ngữ triển khai.

**Đã có:**

- Toolchain trên máy build (Linux): rustup tại `~/.cargo`
  (cargo/rustc 1.100.0), target `x86_64-pc-windows-gnu`, linker
  mingw-w64 GCC 13 trích xuất cục bộ tại `~/mingw` (không cài hệ thống),
  cấu hình sẵn trong `rust/.cargo/config.toml` — chỉ cần
  `cargo build --release` trong thư mục `rust/`.
- Cargo workspace `rust/` với crate **`launcher`** đã build thành công:
  `rust/target/x86_64-pc-windows-gnu/release/launcher.exe`
  (PE32+ x86-64 GUI, ~2,3 MB). Launcher mô phỏng trung thực
  `launcher/Utility.ps1`: header (tiêu đề + OS/PC-Name/User),
  lưới 4x8 đủ 32 nút tiếng Việt, ô tìm kiếm lọc nút không phân biệt dấu,
  nút Xoá/Giới thiệu. (Bản đầu dùng giao diện tối vẽ bằng owner-draw +
  WM_ERASEBKGND/WM_CTLCOLORSTATIC vì NWG không hỗ trợ màu nền cho
  Button/Window — **đã bỏ ở v0.5**, xem mục "Giao diện chuẩn Windows"
  bên dưới.) Bấm nút sẽ chạy
  `modules\<TênModule>.exe` cạnh launcher bằng ShellExecute "runas";
  module chưa có file thì hiện thông báo "Chưa có module" như bản PS.
- Launcher Rust **chưa chạy thử trên máy Windows thật** — cần test đầu
  tiên trên máy test (kiểm tra hiển thị giao diện chuẩn + mở module
  bằng runas) giống như bản PS trước đây.

**Vai trò mới của các file PowerShell:** giữ nguyên không sửa, dùng làm
**tài liệu tham chiếu hành vi** khi port sang Rust — `launcher/Utility.ps1`
là chuẩn giao diện launcher, `modules/BackupRestoreDriver.ps1` là chuẩn
hành vi module driver. Không đóng gói PS2EXE nữa.

**Thứ tự port tiếp theo:**

1. **BackupRestoreDriver** — đã có bản PS hoàn chỉnh làm tham chiếu.
2. **Cài Đặt Win Tự Động** (`CaiDatWinTuDong`) — theo đặc tả
   `docs/logic_cai_dat_win_online.md`. **Đã build xong (2026-11-16):**
   wizard + engine WinPE là hai crate/binary riêng trong workspace
   `rust/`. Agent hậu cài (chạy lần đăng nhập đầu, cài ứng dụng âm thầm)
   **chủ đích KHÔNG xây dựng** — xem mục riêng bên dưới.
3. Các module còn lại theo bảng trạng thái bên dưới (cột "File module"
   đổi đuôi `.ps1` → `.exe` khi port).

## Giao diện chuẩn Windows (v0.5 — 2026-11-17)

Theo phản hồi của người dùng ("giao diện tối khó đọc — chuẩn GUI"),
**toàn bộ giao diện đã bỏ lớp sơn tối tự vẽ** và chuyển về giao diện
Windows chuẩn sáng:

- Xoá khỏi cả 9 crate GUI (launcher + 8 module): các hằng màu `COL_*`,
  hàm `rgb()`, `make_owner_draw()`, `raw_event_handler` chuyên sơn
  (WM_ERASEBKGND/WM_CTLCOLORSTATIC/WM_DRAWITEM), 4 brush HBRUSH +
  `label_colors` trong struct App, và các lời gọi
  `set_background_color`/`set_text_color` trên ListView. Mọi control
  nay render bằng màu hệ thống mặc định (nền sáng, chữ đen).
  (`WinPEInstall` là console — không có GUI nên không đổi.)
- Cỡ chữ chuẩn hoá (NWG size = chiều cao pixel ở 96 DPI): tiêu đề 21
  in đậm (~14pt), tiêu đề mục 17 in đậm (wizard), nội dung/danh sách
  15 (~11pt), nút 15 in đậm, chữ nhỏ/trạng thái 14, monospace
  (Consolas) 14. Trước đây nội dung chỉ 14 và chữ nhỏ 12 nên khó đọc.
- Nút bấm cao tối thiểu 32px (các nút 26/28/30px cũ đã nâng lên 32),
  `HEADER_H` 58→66 (launcher 78→86), cửa sổ mỗi module lớn hơn
  ~20×24px (wizard +20×30), nhãn tiêu đề/phụ đề/trạng thái cao hơn
  để không cắt chữ với font mới.
- **Chức năng không đổi**: giữ nguyên mọi control, nhãn tiếng Việt,
  handler, luồng worker thread, logic nghiệp vụ. Hai hệ quả trình
  bày cần biết: nhãn CẢNH BÁO của TatBitlocker và nhãn kết luận
  (verdict) của wizard CaiDatWin trước đây có màu riêng
  (vàng/xanh/đỏ) nay hiện màu chữ mặc định — nội dung chữ giữ
  nguyên; các nút chọn chế độ của wizard thể hiện trạng thái bằng
  ký hiệu ●/○ trong chữ như trước.
- Build release toàn workspace sạch (0 lỗi, 0 cảnh báo); đóng gói
  tại `drop_v0.5/` + `UtilityRebuild-v0.5.zip`. Chưa chạy trên
  Windows thật — cần chụp màn hình kiểm tra: font ở tỉ lệ phóng
  125%, nhãn dài có bị cắt (gợi ý ISO 3 dòng, thông tin edition
  4 dòng, ghi chú mật khẩu 2 dòng trong wizard), nút launcher có
  nhãn dài ("Quản Lý Update Windows", "Backup/Restore Driver").

## Bảng trạng thái 32 module

| # | Nút chức năng | File module | Trạng thái | Ghi chú |
|---|---|---|---|---|
| 0 | Kho Driver IRST | KhoDriverIRST.ps1 | Pending source recovery | Cần link kho driver của tác giả thay bằng nguồn Intel chính thức; chờ dump |
| 1 | Kho Windows | KhoWindows.ps1 | Pending source recovery | Danh mục ISO nằm trên hạ tầng tác giả; bản rebuild cần nguồn ISO riêng |
| 2 | Backup License MS | BackupLicenseMS.ps1 | Pending spec | Thao tác chuẩn: sao lưu tokens.dat/data.dat của sppsvc/ClipSvc |
| 3 | Backup/Restore Driver | BackupRestoreDriver.exe | **Built (Rust)** | Port từ bản PS tham chiếu (vốn port từ source dịch ngược CRC-valid); liệt kê bằng WMI COM gốc — KHÔNG phân tích `pnputil /enum-drivers` vì output bị bản địa hoá; backup/restore vẫn qua pnputil; xuất HTML; log DriverTool_Log.txt; chưa test Windows thật |
| 4 | Tắt Bitlocker | TatBitlocker.exe | **Built (Rust)** | Trạng thái qua `Get-BitLockerVolume` xuất JSON (helper PowerShell duy nhất, có chủ ý — output manage-bde bị bản địa hoá); `manage-bde -off` + poll 5 giây tới khi giải mã xong; hộp xác nhận kèm cảnh báo; log Bitlocker_Log.txt; chưa test Windows thật |
| 5 | Tạo Boot WinPE | TaoBootWinPE.ps1 | Pending source recovery | Cần biết WinPE payload lấy từ đâu (khả năng cao là image của tác giả) |
| 6 | Cài Đặt Win Tự Động | CaiDatWinTuDong.exe | **Built (Rust)** | Wizard Rust + engine WinPE `WinPEInstall.exe` (mục modules/install/) theo đặc tả `docs/logic_cai_dat_win_online.md` + Implementation notes v1.1; **v0.7: thêm chế độ "Cài mới hoàn toàn" (wipeDisk, §6b)** — chọn đích là cả đĩa vật lý theo serial, bắt buộc gõ serial đĩa + liệt kê mọi phân vùng sẽ bị xoá, stage.json ghi `partitionMode`/`targetDiskSerial`; đã chạy thật trên Windows (kiểm chứng 2026-11-17: tiền đề, quét/mount ISO, đọc edition, chặn build cài đè); đường wipeDisk chưa test máy thật |
| 7 | Cài Đặt Win=WinPE | CaiDatWinWinPE.exe | **Built (Rust)** | Mặt trước TƯƠNG TÁC chạy trong WinPE (crate `cai_dat_win_winpe`, mới ở v0.6): nhận diện WinPE qua MiniNT/PEBootType; liệt kê phân vùng bằng Win32 API (không WMI) + đối chiếu diskpart/vol; chọn ISO (mount PowerShell Mount-DiskImage, thiếu PowerShell thì báo chọn trực tiếp install.wim) hoặc WIM/ESD/SWM; đọc edition bằng DISM như wizard; loại trừ mặc định ổ chứa exe + ổ chứa nguồn (ô ghi đè riêng + thực thi kiểm tra lại), format phải nhập serial ổ đích, thực thi đối chiếu lại serial trước khi ghi; chuỗi format → DISM apply → inject driver → autounattend (cùng bản sinh của wizard) → copy `custom` → bcdboot như engine WinPEInstall; log vào `<ổ nguồn>:\QSU-Logs\winpe-manual-*.log`; nút Khởi động lại chỉ bật khi cài xong. Ở Windows thường chỉ hiện chế độ Chuẩn bị (chép exe + WinPEInstall.exe ra USB/ổ dữ liệu). Engine staging tự động `WinPEInstall.exe` vẫn giữ cho luồng wizard. **v0.7: danh sách đích có thêm dòng "Toàn bộ ổ đĩa" cho từng đĩa vật lý** — xoá & chia lại từ đầu qua `qsu_common::disk` (script diskpart dùng chung với engine), đĩa chứa ổ công cụ/nguồn bị CẤM tuyệt đối (không qua ô ghi đè), xác nhận bằng serial của ĐĨA, thực thi đối chiếu lại serial + số đĩa của các ổ cấm, copy WinRE vào phân vùng Recovery mới, bcdboot trỏ phân vùng boot mới. Chưa test Windows/WinPE thật |
| 8 | Cài Đặt Office Online | CaiDatOfficeOnline.ps1 | Pending spec | ODT + configuration.xml, tải từ Microsoft CDN |
| 9 | Cài Đặt Office OFFLINE | CaiDatOfficeOffline.ps1 | Pending spec | Module mỏng: mount ISO/thư mục rồi chạy setup.exe |
| 10 | Kích Hoạt Win/Office | KichHoatWinOffice.ps1 | Pending — chủ đích giới hạn | Bản rebuild CHỈ làm kích hoạt hợp lệ (key chính hãng + xem trạng thái); không làm KMS bên thứ ba |
| 11 | Cài Đặt Phần Mềm | CaiDatPhanMem.ps1 | Pending source recovery | Danh sách link tải do tác giả tuyển chọn; cần tuyển lại toàn bộ |
| 12 | Sao Lưu Dữ Liệu | SaoLuuDuLieu.ps1 | Pending spec | Copy thư mục hồ sơ người dùng sang ổ khác |
| 13 | Sao Lưu/Quản lý Wifi | SaoLuuWifi.exe | **Built (Rust)** | Liệt kê hồ sơ từ XML trong Wlansvc\Profiles\Interfaces (không phụ thuộc ngôn ngữ; dự phòng netsh); export/import bằng netsh key=clear; xem mật khẩu qua thẻ `<keyMaterial>` của bản export tạm (xoá sau khi đọc); log WifiTool_Log.txt; chưa test Windows thật |
| 14 | Sao Lưu Chrome-Edge | SaoLuuChromeEdge.ps1 | Pending spec | Copy thư mục User Data của Chrome/Edge (lưu ý DPAPI) |
| 15 | Backup/Restore Zalo | BackupRestoreZalo.ps1 | Pending spec | Đóng tiến trình Zalo, copy thư mục dữ liệu ZaloData |
| 16 | Check Cấu Hình Máy Tính | CheckCauHinh.ps1 | Pending spec | Truy vấn WMI phần cứng/OS, hiển thị báo cáo |
| 17 | Check / Test | CheckTest.ps1 | Pending source recovery | Danh sách bài test chính xác chưa rõ; cần UI capture |
| 18 | Phím Tắt BIOS | PhimTatBIOS.exe | **Built (Rust)** | Bảng tra tĩnh 16 hãng (thêm ASRock, Fujitsu, Intel NUC, LG so với danh sách tối thiểu), tìm kiếm bỏ dấu; module tham khảo thuần — không cần quyền admin |
| 19 | Phân Vùng Ổ Cứng | PhanVungOCung.ps1 | Pending source recovery | Chưa rõ engine bên trong; bản rebuild dự kiến dùng Storage cmdlets gốc |
| 20 | Tuỳ Chỉnh Windows | TuyChinhWindows.exe | **Built (Rust)** | Catalogue dữ liệu 49 tweak theo chuẩn Sophia Script (MIT, ghi công trong hộp Giới thiệu): Riêng tư & Telemetry 9, Giao diện & Explorer 11, Hiệu năng 9, Hệ thống 11, Menu ngữ cảnh & Tính năng 9 — mỗi tweak có áp dụng + hoàn tác về mặc định + dò trạng thái thật (registry/dịch vụ/tác vụ/gói nguồn); thêm nhóm động Gỡ ứng dụng rác (27 gói Appx tuyển chọn, chỉ gỡ cho người dùng hiện tại, KHÔNG gỡ provisioned; qua PowerShell — ngoại lệ có ghi chép trong appx.rs); đề nghị tạo điểm khôi phục trước lần áp dụng đầu tiên + nút tạo điểm khôi phục riêng; ngoại lệ công cụ ngoài ghi rõ trong engine.rs (schtasks, powercfg, Checkpoint-Computer); log TuyChinh_Log.txt; chưa test Windows thật. Danh sách tweak riêng của bản gốc vẫn chờ dump để đối chiếu bổ sung, không còn chặn module |
| 21 | Tuỳ Chỉnh Office | TuyChinhOffice.ps1 | Pending spec | Ghi registry HKCU\Software\Microsoft\Office theo checklist |
| 22 | Windows Security | WindowsSecurity.ps1 | Pending spec | Bật/tắt Defender, Firewall, UAC, SmartScreen |
| 23 | Quản Lý Update Windows | QuanLyUpdateWindows.exe | **Built (Rust)** | Trạng thái/thao tác dịch vụ qua SCM API gốc (qsu_common::svc); policy NoAutoUpdate bằng Registry API gốc; kiểm tra cập nhật = mở ms-settings:windowsupdate; đặt lại = regsvr32 danh sách DLL chuẩn + chạy lại dịch vụ; mọi hành động đảo ngược được; log UpdateTool_Log.txt; chưa test Windows thật |
| 24 | Dọn Rác Máy Tính | DonRacMayTinh.exe | **Built (Rust)** | Quét dung lượng từng mục trước + checkbox (mặc định chỉ tick mục an toàn); thùng rác qua SHEmptyRecycleBin; dừng wuauserv khi dọn cache Windows Update; cache Chrome/Edge theo từng profile; KHÔNG đụng Downloads/Documents; xác nhận trước khi xoá; log DonRac_Log.txt; chưa test Windows thật |
| 25 | Gỡ Bỏ Phần Mềm | GoBoPhanMem.ps1 | Pending spec | Đọc khóa Uninstall + Remove-AppxPackage cho Store app |
| 26 | Fix Lỗi Máy In | FixLoiMayIn.ps1 | Pending spec | Khởi động lại Spooler, xóa hàng đợi, fix 0x0000011b (RpcAuthnLevelPrivacyEnabled) |
| 27 | Quản Lý Máy In | QuanLyMayIn.ps1 | Pending spec | WMI Win32_Printer: mặc định/chia sẻ/đổi tên/test page |
| 28 | Fix Share Mạng LAN | FixShareMangLAN.ps1 | Pending source recovery | Blob lớn bất thường (~1 MB, gấp ~30 lần module thường) — chưa giải thích được; cần dump |
| 29 | Quản lý File/Folder | QuanLyFileFolder.ps1 | Pending spec | takeown/icacls, attrib, hiện file ẩn/đuôi file |
| 30 | Chuyển Windows OS | ChuyenWindowsOS.ps1 | Pending source recovery | Chưa rõ dùng tool ngoài nhúng sẵn hay pipeline DISM capture/apply |
| 31 | Multi Zalo PC | MultiZaloPC.ps1 | Pending spec | Chạy nhiều phiên Zalo PC bằng thư mục dữ liệu riêng |

Launcher (`launcher/Utility.ps1`): **Built** — lưới 32 nút, tìm kiếm không
dấu, chạy module với `-Verb RunAs`, báo rõ module chưa build.

## Còn bị chặn ở đâu

1. **Memory dump (máy ảo Windows):** các module ghi "Pending source recovery"
   ở trên chỉ khôi phục được source khi chạy bản gốc trong VM và dump bộ
   nhớ sau khi Enigma giải mã. Ưu tiên dump theo thứ tự người dùng quan
   tâm: **Cài Đặt Win Tự Động → Cài Đặt Win=WinPE →
   Fix Share Mạng LAN → Chuyển Windows OS → Tạo Boot WinPE → Kho Windows /
   Kho Driver IRST / Cài Đặt Phần Mềm** (ba module "Kho" cần dump chủ yếu
   để lấy cấu trúc danh mục, còn link tải phải thay bằng nguồn riêng).
   Riêng **Tuỳ Chỉnh Windows đã hết bị chặn**: bản Rust dùng catalogue
   chuẩn cộng đồng (Sophia Script, MIT) theo `docs/repo_nang_cap.md`;
   dump sau này chỉ để đối chiếu/bổ sung tweak đặc thù của bản gốc.
2. **UI capture:** với Check / Test và các module tweak, chụp toàn bộ màn
   hình/cây điều khiển của bản gốc cũng đủ để dựng lại checklist chức năng
   ngay cả trước khi có dump.
3. **Đối chiếu sau cùng:** mọi module "Pending spec" sau khi viết xong vẫn
   cần chạy song song với bản gốc trên máy test để xác nhận tương đương
   (đặc biệt: danh sách tweak của Tuỳ Chỉnh Windows/Office và danh sách fix
   của Fix Lỗi Máy In / Fix Share Mạng LAN).

## Module Cài Đặt Win Tự Động — bản Rust (2026-11-16)

Toàn bộ workspace `rust/` build release sạch (0 lỗi, 0 cảnh báo), cả ba
exe đều PE32+ x86-64:

- `launcher.exe` → đóng gói tên `UtilityRebuild.exe` (2.318.121 byte)
- `CaiDatWinTuDong.exe` — wizard cài đặt (2.659.510 byte): kiểm tra tiền
  đề §2, quét ISO song song + tải từ kho online kèm kiểm SHA-256 bắt buộc
  §3, đọc edition + chặn tương thích §4, cài đè bằng setup.exe + tail log
  Panther §5, cài mới từ xa qua WinPE staging + BCD một lần §6, sinh
  autounattend §7. Mọi thao tác dài chạy ở luồng nền, UI không bị khóa.
- `WinPEInstall.exe` — engine phía WinPE (1.456.851 byte): xác thực
  serial ổ trước khi format, apply image bằng DISM, inject driver
  offline, bcdboot UEFI/Legacy, tự reboot về Windows mới.

Ghi chú kỹ thuật:

- Thư viện `qsu_common` của phiên trước **chưa từng biên dịch thành
  công** (11 lỗi đường dẫn/feature winapi) — đã sửa tại chỗ, API không
  đổi; test unit của thư viện biên dịch cho target Windows nên chưa chạy
  được trên máy build Linux.
- **Agent hậu cài chủ đích không xây dựng** (chi tiết:
  `rust/post_install/README.md`): không cài ứng dụng âm thầm, không lưu
  mật khẩu UltraViewer; wizard loại các khoá `apps`,
  `ultraviewerPassword`, `adminPassword` khỏi stage.json đã ghi (mật
  khẩu admin chỉ nằm trong autounattend.xml). Thay thế: thư mục
  `custom` cạnh exe wizard được sao nguyên trạng vào
  `ProgramData\QSU\custom` của ảnh mới, công cụ không diễn giải/không
  thực thi nội dung đó. `WinPEInstall.exe` giữ móc nối tuỳ chọn: chỉ
  stage `PostInstall.exe` nếu người dùng tự đặt file đó vào staging.
- **Chưa có gì chạy trên Windows thật**: giao diện wizard, mount ISO +
  parse DISM, cả hai đường cài trọn vẹn đều chờ test đầu tiên trên máy
  thật (danh sách kiểm chứng ở cuối phần Implementation notes v1.1
  trong `docs/logic_cai_dat_win_online.md`).

## Module Tuỳ Chỉnh Windows — bản Rust (2026-11-16)

- `TuyChinhWindows.exe` (2.521.588 byte, PE32+ GUI x86-64) — crate
  `rust/tuy_chinh_windows/` gồm `tweaks.rs` (catalogue dữ liệu),
  `engine.rs` (thực thi + dò trạng thái), `reg.rs` (Registry API gốc),
  `appx.rs` (gỡ gói Store), `main.rs` (giao diện).
- **49 tweak tĩnh** theo 5 nhóm (Riêng tư & Telemetry 9, Giao diện &
  Explorer 11, Hiệu năng 9, Hệ thống 11, Menu ngữ cảnh & Tính năng 9),
  biên soạn theo catalogue **Sophia Script for Windows (farag2, MIT)**
  đối chiếu winutil — ghi công trong hộp Giới thiệu của module. Mỗi
  tweak: áp dụng + hoàn tác về mặc định Windows + trạng thái đọc thật
  (Đã áp dụng / Chưa áp dụng / Không rõ), áp dụng từng mục, nhấp đúp,
  hoặc hàng loạt theo ô tick; tìm kiếm không dấu; lọc theo nhóm.
- **Nhóm động thứ sáu — Gỡ ứng dụng rác:** liệt kê các gói Appx trong
  danh sách tuyển chọn 27 gói đang cài trên máy, gỡ cho **người dùng
  hiện tại** (không gỡ gói provisioned). Không mục nào được tick sẵn —
  gỡ phải tick tay và xác nhận lại danh sách gói trong hộp thoại.
- **An toàn:** trước lần áp dụng đầu tiên của mỗi phiên, hộp thoại đề
  nghị tạo điểm khôi phục (Có/Không/Huỷ); có nút "Tạo điểm khôi phục"
  riêng. Tạo điểm khôi phục dùng `Checkpoint-Computer` (PowerShell) —
  không có Win32 API công khai cho System Restore; tần suất 24 giờ
  được gỡ bằng Registry API gốc trước đó. Các ngoại lệ công cụ ngoài
  khác (schtasks cho tác vụ theo lịch, powercfg cho gói nguồn, Appx
  qua PowerShell) đều ghi lý do trong code. GUID gói Ultimate
  Performance do module tạo được lưu ở `TuyChinhWindows_state.txt`
  cạnh exe để dò trạng thái/hoàn tác chính xác (output powercfg bị bản
  địa hoá nên chỉ trích GUID theo mẫu hex).
- **Chưa chạy trên Windows thật** — cần test: dò trạng thái khớp thực
  tế (đặc biệt hai tweak tác vụ CEIP/Appraiser qua XML schtasks, gói
  Ultimate Performance, menu chuột phải cổ điển Win 11), áp dụng +
  hoàn tác một tweak an toàn (Hiện đuôi file), tạo điểm khôi phục,
  liệt kê/gỡ thử một gói Appx an toàn.

## Chế độ Cài mới hoàn toàn — wipeDisk (v0.7 — 2026-11-17)

Theo yêu cầu người dùng: đường "Cài mới" trước đây chỉ format phân
vùng Windows và giữ nguyên cấu trúc đĩa; nay có thêm chế độ đích là
**toàn bộ ổ đĩa vật lý** — xoá sạch mọi phân vùng, chia lại từ đầu.
Chế độ giữ phân vùng (`preserve`) vẫn là mặc định. Đặc tả đầy đủ:
`docs/logic_cai_dat_win_online.md` §6b.

Ba file build lại (toàn workspace vẫn 0 lỗi / 0 cảnh báo, PE32+ x86-64):

- `CaiDatWinTuDong.exe` — 2.733.141 byte (wizard: khối chọn chế độ
  phân vùng + chọn đĩa đích hiện ở chế độ Cài mới; chặn cứng đĩa chứa
  ổ Data, đĩa < 25GB, serial không đọc được, serial gõ tay sai; hộp
  xác nhận cuối ghi rõ chế độ + liệt kê mọi phân vùng sẽ bị xoá;
  cửa sổ cao thêm 174px cho khối chọn đĩa).
- `CaiDatWinWinPE.exe` — 2.649.351 byte (danh sách đích thêm dòng đĩa
  vật lý; xác nhận bằng serial của đĩa; thực thi đối chiếu lại).
- `WinPEInstall.exe` — 1.530.042 byte (engine đọc `partitionMode`/
  `targetDiskSerial` từ stage.json, thiếu khoá → `preserve`; mã thoát
  mới 10/11/12 cho các chốt an toàn wipe).

Mã dùng chung mới: **`qsu_common::disk`** — liệt kê đĩa vật lý thuần
IOCTL (không phụ thuộc ngôn ngữ output), khớp đĩa theo serial chuẩn
hoá (yêu cầu duy nhất), sinh + thực thi script diskpart chia lại
(GPT: EFI 260MB + MSR 16MB + Windows + Recovery 600MB có type GUID và
thuộc tính chuẩn; MBR: System Reserved 100MB active + Windows +
Recovery 600MB id=27), copy WinRE vào phân vùng Recovery mới. Bốn
chốt an toàn cứng (đĩa chứa staging/Data không bao giờ là đích; bắt
buộc gõ serial; nhận diện bằng serial; đối chiếu lần cuối ngay trước
diskpart) ghi chi tiết ở §6b của đặc tả. **Chưa test trên máy thật**
— checklist test ở HUONG-DAN.txt của bản đóng gói v0.7 (ưu tiên: máy
2 đĩa chạy trọn vòng; máy 1 đĩa phải bị từ chối ở cả 3 tầng; WinRE
sau cài).

## Sửa lỗi liệt kê đĩa wipeDisk (v0.7.1 — 2026-11-17)

Người dùng test v0.7 trên máy thật (một đĩa: C = Windows cũ, E =
Data/ISO): ô chọn đĩa đích wipe **trống hoàn toàn**, cạnh danh sách
phân vùng hiện "Chưa liệt kê được đĩa vật lý nào." Root cause trong
`qsu_common::disk::list_physical_disks` (bản v0.7):

1. **Gate tồn tại đĩa theo một IOCTL duy nhất** — đĩa chỉ được tính
   khi `IOCTL_DISK_GET_LENGTH_INFO` thành công (`query_disk_length
   != 0`); driver lưu trữ từ chối IOCTL đó (RAID/RST, đầu đọc thẻ,
   một số controller) là đĩa bị loại âm thầm. Vòng quét còn **dừng
   sớm sau 4 số đĩa lỗi liên tiếp** (từ số 4), nên một dãy thiết bị
   lỗi ở số thấp che mất đĩa thật ở số cao hơn.
2. **Mọi lỗi đều bị nuốt** — không một dòng log/mã lỗi nào, nên
   không thể chẩn đoán từ xa; giao diện cũng không phân biệt được
   "0 đĩa" với "có đĩa nhưng bị cấm" (combo chỉ hiện đĩa hợp lệ là
   đích đến của lỗi hiển thị này).
3. **Layout đọc lỗi = mất ký tự ổ trên phân vùng** (GET_DRIVE_LAYOUT_EX
   lỗi → phân vùng rỗng) — `contains_letter` khi đó luôn false, chốt
   "đĩa chứa ổ Data" ở phía wizard chỉ dựa vào đường này là chưa đủ
   cứng; nay đối chiếu thêm `volume_disk_number` (extents) độc lập.
4. `execute_wipe` đối chiếu serial tại thời điểm thực thi chỉ bằng
   IOCTL tại chỗ — đĩa định danh qua dự phòng sẽ không bao giờ khớp.

Đã sửa (toàn workspace vẫn 0 lỗi / 0 cảnh báo):

- `qsu_common::disk`: `enumerate_disks()` mới trả `DiskEnumeration
  { disks, diag }` (`list_physical_disks()` giữ chữ ký cũ, là
  wrapper). Tiêu chí tồn tại đĩa = **mở được handle** PhysicalDriveN;
  quét đủ 0..16 không dừng sớm; dung lượng IOCTL lỗi thì suy từ
  tổng phân vùng; mọi lỗi IOCTL ghi kèm GetLastError vào `diag`.
  Khi thiếu đĩa/serial/dung lượng: hợp nhất dự phòng **PowerShell
  Get-Disk (JSON)** rồi **diskpart** (`list disk` + `detail disk`,
  phân tích theo mẫu số/đơn vị/GUID — model lấy theo bố cục: dòng
  ngay trước dòng Disk ID; serial dự phòng = `DP-<Disk ID>`, trường
  mới `PhysicalDisk.from_fallback`). `execute_wipe` đối chiếu serial
  qua chính `enumerate_disks()` và ghi lại chẩn đoán vào log trước
  khi wipe.
- Wizard (`CaiDatWinTuDong.exe` — 2.799.998 byte): chẩn đoán liệt
  kê đẩy vào log panel lúc quét (dòng `[Đĩa] ...`) + chép vào file
  log khi bắt đầu cài; `refresh_disk_picker()` đổ MỌI đĩa vào combo
  kèm hậu tố lý do cấm (`disk_block_reason`: chứa ổ Data/staging —
  chốt cứng duy nhất, đối chiếu 2 đường; không đọc được serial;
  dung lượng 0; đĩa dự phòng không xác minh được phân vùng; đĩa <
  25GB) — đĩa chứa Windows đang chạy **không** bị cấm ở wizard
  (wipe chạy trong WinPE); đổ lại picker khi quét xong, khi đổi ổ
  Data, và mỗi lần vào chế độ wipe; 0 đĩa → dòng rõ ràng "Không
  liệt kê được ổ đĩa vật lý nào — xem log" trong combo + nhãn cảnh
  báo; nhãn gợi ý tĩnh dưới ô chọn đĩa (xoá theo đĩa vật lý, không
  theo ký tự ổ; C và Data chung đĩa thì giữ phân vùng hoặc boot
  WinPE từ USB); cổng `disk_block_reason` kiểm tra lại ở `on_start`;
  cửa sổ cao thêm 48px (1102) cho nhãn gợi ý. Combo ổ Data rỗng
  cũng hiện dòng giải thích thay vì để trống.
- Hai binary còn lại link cùng `qsu_common::disk` nên build lại
  đồng bộ: `CaiDatWinWinPE.exe` — 2.707.545 byte,
  `WinPEInstall.exe` — 1.587.511 byte. Gói:
  `UtilityRebuild-v0.7.1.zip` (drop_v0.7.1 = drop_v0.7 + 3 binary
  trên + HUONG-DAN cập nhật). **Chưa test trên máy thật** — ca test
  quyết định là chính máy đã gặp lỗi: combo phải hiện đĩa kèm dòng
  BỊ CẤM (chứa E:); nếu vẫn trống thì gửi log `[Đĩa]`.

## Mật khẩu admin tuỳ chọn + log quét bền (v0.7.2 — 2026-11-17)

Người dùng test v0.7.1 trên máy thật (máy WIN10 đang cài lại hàng
loạt) và nêu logic triển khai: **cài Windows không cần mật khẩu** —
tài khoản admin tạo ra để trống mật khẩu, người dùng cuối tự đặt
sau. Trước đây wizard chặn ngay ở validation ("Mật khẩu tài khoản
admin trống hoặc hai ô nhập không khớp") kể cả khi chủ đích để
trống. Đồng thời ca liệt kê đĩa ở v0.7.1 cho thấy chẩn đoán [Đĩa]
chỉ vào file log khi bắt đầu cài — mà lỗi liệt kê đĩa lại khiến
không bắt đầu cài được, bằng chứng bị kẹt trong log panel.

Đã sửa (toàn workspace vẫn 0 lỗi / 0 cảnh báo):

**Fix 1 — mật khẩu admin thành TUỲ CHỌN** (wizard + module WinPE,
logic đồng nhất):
- Validation cả hai nơi: chặn **chỉ khi** hai ô mật khẩu lệch nhau
  (một ô trống/một ô có, hoặc nhập lại sai) với thông báo mới "Hai
  ô mật khẩu không khớp — để trống cả hai ô nếu không muốn đặt mật
  khẩu...". Cả hai ô trống = hợp lệ; cả hai ô giống nhau = như cũ.
- `build_unattend` (cả hai crate dùng chung một bản sinh): mật
  khẩu trống → `LocalAccount` **không có** phần tử `<Password>` và
  `AutoLogon` **không có** phần tử `<Password>` — không sinh
  `<Value>` rỗng (đối chiếu cấu trúc Schneegans: AutoLogon không
  mật khẩu là cấu hình Setup hỗ trợ cho đăng nhập nội bộ). Có mật
  khẩu → khối `<Password>` như cũ, thụt lề giữ nguyên (đã mô phỏng
  kết xuất kiểm tra: XML hợp lệ cả hai trường hợp, trường hợp
  trống có đúng 0 phần tử Password).
- Hộp xác nhận cuối: wizard (cả tóm tắt wipeDisk lẫn tóm tắt
  thường, chỉ ở chế độ cài mới) và module WinPE (cả hai biến thể)
  đều thêm dòng "Tài khoản \<tên\>: KHÔNG mật khẩu — người dùng tự
  đặt mật khẩu sau khi cài" (hoặc "có đặt mật khẩu (không hiển
  thị)"). Module WinPE bỏ hộp hỏi riêng khi mật khẩu trống —
  trạng thái nay nằm ở hộp xác nhận cuối như wizard; `InstallPlan`
  thêm `admin_user` + `admin_passwordless` (chỉ trạng thái, plan
  không mang mật khẩu).
- Log: cả hai luồng ghi "Tài khoản admin: không đặt mật khẩu
  (theo lựa chọn)" khi để trống (wizard chỉ ở đường cài mới —
  đường cài đè không tạo tài khoản). Giá trị mật khẩu vẫn không
  bao giờ vào log; `stage.json` không đổi (vẫn strip khoá mật
  khẩu; không mật khẩu thì không có gì để strip).
- Nhãn ô mật khẩu ở cả hai giao diện: "Mật khẩu (để trống = không
  đặt mật khẩu):" — hàng nhập được bố cục lại (ô nhập 170/150px,
  nhãn hai 140px, ghi chú rút gọn còn "(Chỉ cài mới. Không ghi
  vào log.)" ở wizard và "(Không ghi vào log.)" ở module WinPE).

**Fix 2 — log quét ghi ra file ngay khi quét xong** (wizard):
- `persist_scan_log()` chạy ở cuối mỗi lần `on_preflight` (quét
  đầu + mọi lần Quét lại): nội dung = tóm tắt tiền đề (OS, boot
  mode, BitLocker, nguồn điện, OEM key, kết luận), danh sách ổ cố
  định + marker, ổ Data đang chọn, mọi đĩa vật lý (summary + phân
  vùng), và **toàn bộ** dòng chẩn đoán `[Đĩa]` của cả hai đường
  liệt kê (IOCTL + dự phòng).
- Đường ghi: `<ổ Data>:\QSU-Logs\wizard-scan-<yyyyMMdd-HHmm>.log`
  (theo `logsFolder` trong settings, tạo thư mục nếu cần); ổ Data
  chưa chọn được hoặc ghi lỗi → cạnh file exe wizard. Ghi ở luồng
  nền; đường dẫn file đẩy vào log panel khi ghi xong (ca 0 đĩa có
  câu chữ riêng yêu cầu gửi file để chẩn đoán). Bản chép chẩn đoán
  vào file log lúc bắt đầu cài (v0.7.1) vẫn giữ.

Sản phẩm: `CaiDatWinTuDong.exe` — **2.820.945 byte**,
`CaiDatWinWinPE.exe` — **2.708.447 byte** (cả hai PE32+ GUI
x86-64, kiểm bằng `file`). `WinPEInstall.exe` **không đổi**
(md5 trùng bản v0.7.1 — engine không đọc mật khẩu từ stage.json).
Gói: `UtilityRebuild-v0.7.2.zip` (drop_v0.7.2 = drop_v0.7.1 + 2
binary trên + HUONG-DAN cập nhật). **Chưa test trên máy thật** —
ca test quyết định: (1) cài mới để trống cả hai ô mật khẩu →
wizard qua validation, hộp xác nhận hiện dòng KHÔNG mật khẩu,
Windows mới đăng nhập thẳng bằng tài khoản không mật khẩu và
autounattend.xml trong staging không có phần tử Password; (2)
điền lệch một ô → vẫn bị chặn; (3) mở wizard trên máy lỗi liệt
kê đĩa → file `wizard-scan-*.log` xuất hiện trên ổ Data kèm đủ
dòng [Đĩa] dù không bắt đầu cài.

## Sửa lỗi "bấm Yes xong không có hành động gì" (v0.7.3 — 2026-11-17)

Người dùng test v0.7.2 trên máy thật: đi hết wizard tới hộp xác
nhận cuối của cài mới wipeDisk (đĩa 0 liệt kê qua đường dự phòng,
serial DP-759ED391 gõ khớp, hộp xác nhận đầy đủ/đúng, tài khoản
không mật khẩu) — **bấm "Yes" xong không thấy hành động gì và
wizard như thoát/đóng**, thay vì chạy chuỗi staging (copy WinPE +
sources + driver vào staging trên ổ Data, ghi stage.json +
autounattend, đăng ký BCD boot một lần, báo tiến độ, hỏi reboot).

**Truy vết toàn bộ đường sau xác nhận trong mã nguồn** (`on_start`
→ validation → `confirm_box` → tạo logger → `spawn_exec` →
`exec_clean`/`exec_inplace` → handler `exec_done` trong `refresh`):

- Mọi bước staging đều đã có xử lý lỗi qua `fail(...)` →
  `exec_done = Err` → hộp thoại lỗi + mở khoá giao diện. Các hàm
  phụ trên đường này (`Logger`, `cmd::run_*`, parse DISM
  `parse_kv`/`kv_get`/`extract_guid`, `register_bcd`, ghi
  stage.json) **không có lời gọi nào panic được** với dữ liệu của
  máy này; `stop_thread_dispatch()` chỉ gắn với nút Đóng; profile
  release **không** đặt `panic = "abort"` nên kể cả panic ở luồng
  nền cũng không thể làm tiến trình thoát (chỉ làm giao diện đứng
  im). **Kết luận: trong mã nguồn không tồn tại đường nào khiến
  wizard tự thoát im lặng sau Yes — lỗi không tái hiện được từ
  source.**
- Đối chiếu wipeDisk với dữ liệu máy này (đĩa dự phòng
  `from_fallback`, phân vùng size 0, serial DP-): định danh
  khớp nhau hai phía wizard↔engine (cùng `enumerate_disks`, hợp
  nhất theo số đĩa, serial DP- điền vào serial trống và set
  `from_fallback`); engine tìm đích theo SERIAL trước (không phụ
  thuộc số đĩa), chốt đĩa Data bằng extents fail-closed,
  `execute_wipe` đối chiếu lại serial ngay trước diskpart. Size
  phân vùng 0 chỉ ảnh hưởng danh sách hiển thị, không chạm thực
  thi. **Không có điểm nào engine từ chối oan đĩa dự phòng ở khâu
  thực thi.**
- Nguyên nhân khớp nhất với triệu chứng: **cửa sổ wizard cao
  1102px** (vùng tiến độ ở y=918–1048: thanh tiến độ, nhãn trạng
  thái, ô log, hai nút) trong khi cửa sổ session từ xa của người
  dùng thấp hơn hẳn — trong mọi ảnh chụp thực địa, đáy cửa sổ đều
  nằm ngoài vùng nhìn. Sau Yes, toàn bộ phản hồi (tiến độ
  3→10→18→22%…, dòng log) xuất hiện đúng ở vùng bị khuất đó:
  staging có thể đã chạy mà người dùng không thấy bất kỳ dấu hiệu
  nào, và kết luận "không có hành động gì, out".

Đã sửa/cứng hoá (chỉ wizard — `CaiDatWinTuDong.exe` **2.824.505
byte**; `WinPEInstall.exe` và `CaiDatWinWinPE.exe` **không đổi**,
md5 trùng v0.7.2):

1. **Phản hồi tức thì + cửa sổ tự vào khung hình:** ngay trong cú
   bấm Yes (luồng giao diện, trước mọi vòng lặp qua luồng nền):
   nhãn trạng thái đổi thành "Đã xác nhận — đang bắt đầu..." +
   thanh tiến độ nhảy 1%, và `shift_window_for_exec()` dịch cửa
   sổ lên để ĐÁY cửa sổ nằm trọn trong vùng làm việc của màn
   hình (vị trí gốc lưu vào `Model.saved_window_rect`; handler
   `exec_done` gọi `restore_window_pos()` đưa cửa sổ về chỗ cũ
   trước khi hiện hộp thoại kết quả).
2. **Nhịp tim đầu tiên của luồng staging:** `exec_main` mở đầu
   bằng `set_exec_progress(1, "Đã bắt đầu — đang chuẩn bị
   staging...")` trước mọi thao tác dài.
3. **Bọc `catch_unwind` toàn bộ thân luồng cài đặt** (refactor
   `spawn_exec` → wrapper + `exec_main`): mọi panic đều biến
   thành `exec_done = Err` với thông điệp đọc được → hộp thoại
   lỗi + log panel + file log trên ổ Data + giao diện mở khoá.
   Lời gọi `thread::spawn` trong `on_start` cũng được bọc
   `catch_unwind` (spawn lỗi → báo lỗi, khôi phục trạng thái).
4. **Khoá Shared chịu poison** (`lock_shared`): toàn bộ chốt
   tiến độ/hoàn tất/lỗi trên đường thực thi dùng khoá này — một
   panic từng xảy ra khi đang giữ khoá không còn làm các chốt
   sau panic dây chuyền và mất luôn thông báo lỗi.
5. **Hộp đen panic** (`install_panic_hook`, cài đầu `main()`):
   mọi panic ở mọi luồng được ghi kèm vị trí file:dòng vào
   `CaiDatWinTuDong-panic.log` cạnh file exe — bằng chứng để
   chẩn đoán nếu còn ca chết bất thường ngoài thực địa (app GUI
   không console nên mặc định panic không để lại dấu vết nào).

Gói: `UtilityRebuild-v0.7.3.zip` (drop_v0.7.3 = drop_v0.7.2 +
wizard build lại + HUONG-DAN cập nhật). Toàn workspace build
release sạch (0 lỗi, 0 cảnh báo). **Chưa test trên máy thật** —
ca test quyết định là chính máy đã gặp lỗi: bấm Yes → cửa sổ
dịch lên, tiến độ chạy 1% → 3% → 10% → 18% → 22%...; nếu còn
dừng/biến mất, gửi `CaiDatWinTuDong-panic.log` + ảnh chụp hộp
thoại/log tại điểm dừng.

## Sửa lỗi chết im lặng sau cú bấm Yes — GỐC LỖI THẬT (v0.7.4 — 2026-11-17)

Người dùng gửi bằng chứng cứng từ `E:\QSU-Logs`: **ba** file
`install-20261117-1048 / -1050 / -1106.log` (hai lần đầu v0.7.2,
lần 11:06 khả năng cao v0.7.3) — cả ba đều ~1KB và **dừng ở đúng
một dòng duy nhất**: `[2026-11-17 11:06:33] --- Chẩn đoán liệt
kê đĩa lúc quét tiền đề ---`. Không dòng `[Đĩa]` nào theo sau,
dòng "=== Bắt đầu quá trình cài đặt ===" của `spawn_exec` không
bao giờ xuất hiện. Chết tất định, cùng một điểm, qua hai phiên
bản — bác bỏ giả thuyết "cửa sổ khuất" của v0.7.3 (đó chỉ là vấn
đề hiển thị đi kèm, không phải nguyên nhân chết).

**Root cause — tái nhập đồng bộ qua NWG Notice + RefCell
double-borrow, chết trên luồng chính trong handler:**

1. Khối tạo logger trong `on_start` (main.rs:3293 bản v0.7.3)
   giữ `let mut m = app.model.borrow_mut()` trong SUỐT khối,
   và bên trong gọi `lg.line("--- Chẩn đoán liệt kê đĩa...")`
   (main.rs:3301) để chép chẩn đoán vào install log.
2. `Logger::line` (qsu_common/src/log.rs:60-70) ghi file TRƯỚC
   (nên dòng header vào được file — khớp bằng chứng tuyệt đối),
   rồi gọi sink. Sink của `make_logger` (main.rs:256-266) đẩy
   dòng vào `shared.log_lines` rồi gọi `snd.notice()`.
3. `NoticeSender::notice()` của **native-windows-gui 1.0.13**
   (src/controls/notice.rs:129-137) dùng `SendNotifyMessageW` —
   với cửa sổ do chính luồng hiện tại tạo ra, hàm này dispatch
   thông điệp ĐỒNG BỘ ngay tại chỗ (khác luồng mới là post bất
   đồng bộ). Hệ quả: `refresh()` chạy TÁI NHẬP ngay giữa
   `lg.line`, trên cùng luồng chính, trong lúc `borrow_mut` ở
   bước 1 vẫn còn sống.
4. `refresh` gọi `render_log` đầu tiên; render_log đọc
   `app.model.borrow().log_rendered` (main.rs:2226 bản v0.7.3)
   để so với tổng số dòng — mà sink vừa đẩy thêm một dòng nên
   điều kiện vẽ lại đúng, và cả `borrow()` lẫn `borrow_mut()`
   (main.rs:2228) đều **panic "already mutably borrowed"** khi
   đang có `RefMut` sống.
5. Panic unwind ngược qua `refresh` → wndproc của NWG là
   `extern "system"` (biên FFI không unwind được) → **abort
   tiến trình tức thì, im lặng**. Luồng worker chưa từng được
   spawn nên không có dòng nào thêm vào log. Lớp `catch_unwind`
   của v0.7.3 bọc worker thread và lời gọi spawn — cả hai đều
   nằm SAU điểm chết này nên không cứu được.

Vì sao các đường khác không chết: `push_log`/notice từ luồng
nền (preflight, quét ISO, tải) là post bất đồng bộ — refresh
chạy sau, khi không borrow nào sống; các `push_log` trên luồng
chính ở handler khác không giữ borrow ngang qua lời gọi. Chỉ
khối logger của `on_start` giữ `borrow_mut` ngang qua
`lg.line` — và dump chẩn đoán (thêm ở v0.7.1) là lời gọi
`lg.line` đầu tiên trong khối đó. Ở v0.7.1 người dùng chưa từng
bấm được tới Yes (kẹt ở validation/liệt kê đĩa) nên lỗi nằm im
tới v0.7.2.

**Ứng viên đã rà và LOẠI TRỪ** (đoạn sau xác nhận, trên dữ liệu
máy này): format/slice theo byte-index trên chuỗi tiếng Việt —
tóm tắt xác nhận và ExecPlan toàn dùng String nguyên khối,
`summary()`/`describe_partitions()` đã chạy thật khi hiện hộp
xác nhận; `data_letter.unwrap()` (main.rs:3229) đã được chặn
bởi các kiểm tra `is_none` phía trên; vòng dump tự thân chỉ là
`format!("[Đĩa] {}", line)` trên `Vec<String>` có sẵn — không
thể panic. **Tràn stack**: trong đoạn này không có đệ quy nào;
vector đệ quy duy nhất qua notice (refresh → push_log →
notice → refresh) bị chặn vì `refresh_inner` `.take()` mọi
trường shared TRƯỚC khi xử lý, lượt tái nhập chỉ thấy toàn None
và dừng ở render_log — độ sâu tối đa 2. Việc chưa thấy
`CaiDatWinTuDong-panic.log` được giải thích đơn giản: người
dùng tìm trong QSU-Logs trong khi hook của v0.7.3 ghi CẠNH FILE
EXE (đã hướng dẫn lại, chưa có trả lời) — hook luôn bắn trước
khi unwind với mọi panic thường, nên file đó khả năng cao đang
nằm ở thư mục modules ghi đúng dòng panic tại render_log.

**Đã sửa (chỉ wizard — `CaiDatWinTuDong.exe` 2.828.883 byte;
`WinPEInstall.exe` và `CaiDatWinWinPE.exe` không đổi, md5 trùng
v0.7.3):**

1. **Tái cấu trúc khối logger trong `on_start_inner`** — không
   giữ bất kỳ borrow nào của model trong lúc ghi log: đọc
   snapshot (`logger` sẵn có, `logs_folder`, `disk_diag` clone)
   trong một borrow ngắn → thả → tạo logger + ghi dump → cất
   logger bằng một `borrow_mut` ngắn khác.
2. **`render_log` mượn kiểu không-panic**: `try_borrow` /
   `try_borrow_mut` — model đang bận thì bỏ qua lượt vẽ (dòng
   được vẽ bù ở notice kế tiếp). Diệt cả lớp lỗi tái nhập cho
   mọi đường log khác. Khoá Shared trong render_log/refresh/
   push_log/sink đổi sang `lock_shared` chịu poison.
3. **`catch_unwind` bọc toàn thân `on_start`** (wrapper +
   `on_start_inner`): bắt được panic → ghi vào install log (nếu
   logger đã tạo), `running = false`, khôi phục vị trí cửa sổ,
   mở khoá nút Bắt đầu, hiện hộp thoại lỗi. **`refresh` cũng
   được bọc tương tự** (wrapper + `refresh_inner`; khi bắt
   panic chỉ ghi thẳng vào Shared, không bắn notice mới tránh
   đệ quy). Thêm lưới ngoài cùng: `catch_unwind` ngay tại
   closure bind sự kiện trong `finish_build` — không handler
   nào còn có thể đưa panic qua biên FFI.
4. **Step marker flush từng dòng trong install log** quanh
   đoạn sau xác nhận: `[step] logger ok (đã qua xác nhận)` →
   header dump → `[step] diag header ok` → dòng `[Đĩa]` đầu
   tiên → `[step] diag dong 1 ok` → hết dump → `[step] diag
   ok` → (worker) "=== Bắt đầu quá trình cài đặt ===" →
   `[step] spawn ok`. Logger vốn flush sau mỗi dòng nên dòng
   cuối của file chỉ đúng bước đã chết (marker mịn quanh hai
   dòng đầu của dump là để phân biệt sink-của-dòng-nào với các
   nguyên nhân không qua hook).
5. **Hộp đen panic ghi thêm bản sao vào ổ Data**: static
   `PANIC_LOG_DIR` đặt ở `on_preflight` (ngay khi biết ổ Data)
   và lúc tạo logger cài đặt; hook ghi cả cạnh exe lẫn
   `<ổ Data>:\QSU-Logs\CaiDatWinTuDong-panic.log`.

Gói: `UtilityRebuild-v0.7.4.zip` (drop_v0.7.4 = drop_v0.7.3 +
wizard build lại + HUONG-DAN cập nhật). Toàn workspace build
release sạch (0 lỗi, 0 cảnh báo). **Chưa test trên máy thật** —
ca test quyết định là chính kịch bản đã chết ba lần: Cài mới →
wipeDisk → Yes; kỳ vọng install log chạy qua đủ các [step] và
tiến độ staging chạy; nếu còn dừng, dòng [step] cuối + file
panic trên ổ Data sẽ chỉ đúng điểm chết mới.

## Chuỗi dự phòng boot.sdi cho ramdisk WinPE (v0.7.5 — 2026-11-17)

Người dùng test v0.7.4 trên máy thật (DESKTOP-J8LPB75): bản sửa
RefCell chạy đúng — wizard không còn chết ở Yes, staging chạy tới
~50% rồi dừng bằng **hộp thoại lỗi đọc được** (hành vi mới hoạt
động như thiết kế):

> "ISO thiếu sources\boot.sdi — không thể tạo ramdisk WinPE."

**Rà soát bước ramdisk trong `exec_clean`** (wizard
`cai_dat_win/src/main.rs`): bước này cần đúng hai file từ ISO đã
mount — `sources\boot.wim` (làm WinPE WIM, về sau) và
`sources\boot.sdi` (SDI cho BCD ramdisk; `register_bcd` trỏ
`ramdisksdipath` tới `\<staging>\boot.sdi` nên file phải nằm ở gốc
staging). boot.sdi được kiểm tra ngay sau khi copy xong toàn bộ
`sources` (22%→52% — khớp mốc ~50% thực địa) tại đúng MỘT vị trí
`src_dir.join("boot.sdi")` với `src_dir = <ký tự mount>:\sources`.
Loại trừ khả năng tìm nhầm chỗ: thư mục sources tồn tại và copy
thành công (tiến độ đã chạy hết đoạn copy) nên ký tự mount đúng;
file kiểm tra trên bản mount gốc (không phải bản đã copy) nên
không có lệch đường dẫn — kết luận ISO mod
`Windows11-Pro-HDD4G-26300.9457-x64.iso` (ảnh bên trong build
19041) **thật sự không có `sources\boot.sdi`** (bản mod thường
lược bỏ file này). Điểm yếu phụ phát hiện khi rà: bản cũ chép
boot.sdi bằng `let _ = std::fs::copy(...)` — nuốt lỗi copy, file
đích thiếu vẫn đi tiếp tới BCD.

**Đã sửa (chỉ wizard — `CaiDatWinTuDong.exe` 2.841.140 byte;
`WinPEInstall.exe` và `CaiDatWinWinPE.exe` không đổi):** hàm mới
`stage_boot_sdi` thử lần lượt, **ghi log mọi vị trí đã thử** và vị
trí cung cấp được file, chép xong kiểm chứng file đích tồn tại:

1. `<mount>:\sources\boot.sdi` (hành vi cũ);
2. `<mount>:\boot.sdi` (gốc đĩa ISO);
3. `%WINDIR%\Boot\DVD\PCAT\boot.sdi` của Windows đang chạy;
4. `%WINDIR%\Boot\DVD\EFI\boot.sdi` của Windows đang chạy — hai
   bản này có sẵn trong mọi Windows chuẩn, là chính SDI ramdisk
   dùng chung mà Microsoft dùng cho entry boot setup của mình,
   thay thế an toàn, không phụ thuộc build;
5. `<ổ OS>:\Recovery\WindowsRE\boot.sdi` (WinRE nếu sẵn có).

Lỗi copy ở một ứng viên → log lại và thử ứng viên kế. Không thấy
ở cả 5 vị trí: vẫn fail TRƯỚC mọi thao tác phá huỷ (bước này vẫn
thuần staging), thông điệp lỗi liệt kê toàn bộ vị trí đã thử kèm
ghi chú lỗi copy nếu có — báo cáo thực địa lần sau tự chẩn đoán
được. **boot.wim KHÔNG được thay thế** (bắt buộc từ ISO hoặc
`settings.winpeWimPath` như cũ; thiếu vẫn báo lỗi nêu đường dẫn
đã thử). Không đổi hành vi nào khác; mọi chốt an toàn giữ nguyên.

Gói: `UtilityRebuild-v0.7.5.zip` (9.614.842 byte, 16 file,
testzip sạch; drop_v0.7.5 = drop_v0.7.4 + wizard build lại +
HUONG-DAN cập nhật). Toàn workspace build release sạch (0 lỗi,
0 cảnh báo). **Chưa test trên máy thật** — ca test quyết định:
chạy lại kịch bản cũ trên DESKTOP-J8LPB75, kỳ vọng chạy qua mốc
50%, log có dòng "boot.sdi: dùng ..." (khả năng cao rơi vào ứng
viên 3/4 — bản trong Windows đang chạy) và đi tiếp hết staging
tới BCD + hỏi reboot.

## Sửa lỗi DISM mount WinPE 0xC1510111 — read-only chép từ ISO (v0.7.6 — 2026-11-17)

Người dùng test v0.7.5 trên máy thật: chuỗi dự phòng boot.sdi
chạy đúng (log: `boot.sdi: dùng C:\Windows\Boot\DVD\PCAT\boot.sdi
(3170304 byte) -> E:\QSU-Stage\boot.sdi`), staging đi tiếp tới
bước mount WinPE rồi dừng bằng hộp thoại:

> "DISM mount WinPE thất bại (Ok(Some(-1051655919)))"

**Bằng chứng quyết định từ install log** — lời của chính DISM:

```
$ dism /Mount-Wim /WimFile:E:\QSU-Stage\winpe.wim /Index:2 /MountDir:E:\QSU-Stage\mnt
Error: 0xc1510111
"You do not have permissions to mount and modify this image.
Verify that you have Read/Write permissions or mount the image
using the /ReadOnly option."
```

**Phỏng đoán ban đầu SAI**: không phải nén solid/LZMS hay ESD.
**Root cause thật**: `winpe.wim` trong staging được chép từ ISO
đang mount (F:) — mọi file trên đĩa quang đều mang thuộc tính
**read-only**, và `std::fs::copy` giữ nguyên thuộc tính ở file
đích. DISM /Mount-Wim là mount để SỬA (inject file) nên từ chối
file read-only với đúng lỗi quyền 0xC1510111. Mọi file khác chép
từ ISO (cây sources, boot.sdi khi lấy từ ISO) cũng mang thuộc
tính này.

**Đã sửa (chỉ wizard — `CaiDatWinTuDong.exe` 2.870.682 byte;
`WinPEInstall.exe` và `CaiDatWinWinPE.exe` không đổi, md5 trùng
v0.7.5):**

1. **Fix chính — chuẩn hoá thuộc tính sau khi chép từ ISO:**
   helper mới `clear_readonly_file` (metadata → set_readonly
   (false) → set_permissions) và `clear_readonly_tree` (đệ quy,
   trả số file đã đổi). Áp dụng tại: (a) toàn bộ cây
   `staging\sources` ngay sau copy (log số file đã chuẩn hoá);
   (b) `winpe.wim` ngay sau copy từ ISO (log rõ dòng 0xC1510111);
   (c) `boot.sdi` đích trong `stage_boot_sdi` (ghi chú trong
   dòng log sẵn có); (d) **kiểm lại ngay trong
   `mount_inject_winpe` trước mỗi lần mount** — chốt chặn cuối
   bao phủ cả file staging có sẵn từ lần chạy trước và bản WIM
   vừa export.
2. **Lỗi đọc được cho người dùng — diệt debug repr:** helper
   `fmt_exit_code` (mã thập phân + hex `0x{:08X}`),
   `last_error_line` (trích dòng lỗi tiêu biểu của chính công
   cụ: ưu tiên dòng cuối chứa mã 0x..., rồi dòng "Error"/"Lỗi",
   rồi dòng không rỗng cuối), `cmd_fail_msg` (ghép hai thứ
   trên). Áp dụng tại cả 6 điểm format kết quả lệnh ngoài vào
   thông điệp lỗi/hộp thoại trong wizard: DISM /Get-WimInfo
   (trước đây còn lấy stderr — nơi DISM gần như không bao giờ
   ghi lỗi; nay gộp stdout+stderr và trích từ đó), manage-bde
   suspend BitLocker, cảnh báo pnputil /export-driver, bcdedit
   trong register_bcd, DISM mount WinPE và DISM unmount
   /Commit (hai điểm thực địa). Output đầy đủ vẫn vào log như
   trước.
3. **Chẩn đoán trước mount:** hàm mới `probe_winpe_wim` thay
   closure chọn boot index cũ (vốn nuốt cả kết quả lẫn lỗi
   /Get-WimInfo): ghi vào log kích thước file WIM + **toàn bộ**
   output /Get-WimInfo + chi tiết của index được chọn, và lý
   do chọn index — giữ nguyên logic chọn cũ (ưu tiên "Boot
   Index" do DISM báo; không có thì index 2 nếu WIM có ≥2
   index, else index đầu tiên). /Get-WimInfo lỗi → log lỗi đọc
   được, tạm dùng index 1, mount/export kết luận sau.
4. **Mount qua hàm chung `mount_inject_winpe`** (mount →
   inject 4 file → unmount /Commit, output DISM vừa chảy vào
   log vừa được thu lại để trích dòng lỗi). Hai thay đổi kèm
   theo: các lệnh mount/unmount/export nay gọi kèm `/English`
   như mọi lệnh DISM khác của wizard (trước đây thiếu — dòng
   lỗi trích ra phụ thuộc ngôn ngữ Windows); và bước dọn
   /Unmount-Wim /Discard sau mount THẤT BẠI chuyển sang
   `run_capture` với một dòng log trung tính — mount hỏng thì
   lệnh dọn này trả mã 50 và trước đây in như một lỗi thứ hai
   gây nhiễu kết quả; nay nó không bao giờ thành lỗi được báo.
5. **Export fallback — chỉ còn là lưới an toàn cuối** (giữ
   theo chỉ đạo, sau khi fix thuộc tính đã xử lý nguyên nhân
   đã xác nhận): mount bản gốc lỗi → `DISM /Export-Image
   /SourceIndex:<boot index> /Compress:Max /Bootable` sang
   `winpe_export.wim` (xoá file đích sót trước vì DISM append
   vào file có sẵn; /Compress:Max là nén chuẩn không solid,
   KHÔNG dùng recovery/LZMS) → probe lại bản xuất → mount +
   inject bản xuất → **thay tên thành `winpe.wim`** (xoá bản
   gốc, rename bản xuất). BCD vẫn trỏ `\<staging>\winpe.wim`
   và stage.json/engine WinPE không hề biết có export xảy
   ra — tên file nhất quán từ đầu tới cuối. Export hoặc mount
   lần hai cũng lỗi → dừng (vẫn trước BCD, chưa phá huỷ gì)
   bằng thông điệp ghép cả hai lỗi, đều dạng đọc được.

Gói: `UtilityRebuild-v0.7.6.zip` (9.625.445 byte, 16 file,
testzip sạch; drop_v0.7.6 = drop_v0.7.5 + wizard build lại +
HUONG-DAN cập nhật). Toàn workspace build release sạch (0 lỗi,
0 cảnh báo). **Chưa test trên máy thật** — ca test quyết định:
chạy lại kịch bản cũ trên DESKTOP-J8LPB75, kỳ vọng log có dòng
xoá read-only của winpe.wim, mount qua được, staging chạy hết
tới BCD + hỏi reboot; nếu còn dừng, hộp thoại phải hiện mã
thoát + dòng lỗi của chính DISM.

## Sửa lỗi Boot Manager 0xc000000e — WIM mất cờ bootable + rà soát BCD/firmware (v0.7.7 — 2026-11-17)

Người dùng test v0.7.6 trên máy thật: **staging hoàn tất trọn vẹn
lần đầu tiên** (chép sources, xoá read-only, mount + inject
WinPE, đăng ký BCD, reboot vào entry một lần) — nhưng Windows
Boot Manager báo:

> Status: **0xc000000e** — "A required device isn't connected or
> can't be accessed."

WinPE không hề nạp; quá trình wipe KHÔNG chạy, đĩa đích nguyên
vẹn (mọi thao tác phá huỷ vẫn nằm trong WinPE như thiết kế).

**Bằng chứng quyết định — install log của chính lần chạy đó
(install-20261117-1335.log):** mọi lệnh bcdedit đều thành công
và các phần tử BCD đúng NGUYÊN VĂN công thức ramdisk chuẩn.
Nhưng chi tiết `DISM /Get-WimInfo` của index 2 trong boot.wim
của ISO mod báo **`WIM Bootable : No`** và bước dò của wizard đã
ghi "WIM không báo 'Boot Index'" — header WIM không mang boot
index nào. boot.wim của bản mod này là build **26100.9457** trong
khi install.wim là 19041 (mod trộn hai build); tác giả mod đóng
gói lại boot.wim và làm mất cờ bootable ở header. Boot Manager
mở được file ramdisk trên ổ Data nhưng không tìm thấy ảnh boot
bên trong để nạp winload → 0xc000000e. Mount + /Commit của bước
inject KHÔNG khôi phục cờ header này.

**Đã sửa (chỉ wizard — `CaiDatWinTuDong.exe` 2.905.595 byte,
md5 `1ef8974c…`; `WinPEInstall.exe` + `CaiDatWinWinPE.exe`
không đổi, md5 trùng v0.7.6):**

1. **Fix gốc rễ — chuẩn hoá cờ bootable của WIM sau inject**
   (hàm mới `check_wim_bootable` + `ensure_winpe_bootable`, gọi
   ở mốc 71% của exec_clean — sau khi cả hai đường mount hội tụ
   về winpe.wim đã inject, TRƯỚC driver/autounattend/BCD):
   kiểm chứng cờ bằng /Get-WimInfo (header có "Boot Index" > 0,
   hoặc chi tiết index báo "WIM Bootable : Yes") và ghi toàn bộ
   output + dòng kết luận vào log. Đã bootable → bỏ qua, log rõ.
   Chưa bootable (hoặc không kết luận được — thận trọng theo
   hướng an toàn) → xuất lại đúng index WinPE:
   `DISM /Export-Image /SourceImageFile:<staging>\winpe.wim
   /SourceIndex:<boot index>
   /DestinationImageFile:<staging>\winpe_bootable.wim /Bootable
   /Compress:Max` (xoá file đích sót trước vì DISM append) →
   xoá bản gốc, rename bản xuất thành `winpe.wim` (đúng mẫu thay
   tên của đường export dự phòng v0.7.6) → **kiểm chứng lại trên
   chính file winpe.wim** (bản xuất một index: header phải báo
   Boot Index / index 1 báo "WIM Bootable : Yes"). Export lỗi
   hoặc kiểm chứng lại không đạt → dừng bằng lỗi đọc được
   TRƯỚC đăng ký BCD và trước mọi lời mời reboot. BCD,
   stage.json và engine WinPE vẫn chỉ thấy tên `winpe.wim` —
   một WIM một-index bootable là đúng thứ chúng cần.
2. **Dò firmware hai API + log giá trị thô** (struct
   `FirmwareProbe` + `probe_firmware`, khai báo extern kernel32
   ngay tại wizard — không mở thêm feature winapi, không đụng
   qsu_common): (A) `GetFirmwareType` có tài liệu — 1=BIOS,
   2=UEFI, 0=Unknown; kết quả dứt khoát 1/2 thì phán quyết theo
   A. (B) trick cũ `GetFirmwareEnvironmentVariableW` (tên rỗng +
   GUID toàn 0; Legacy ⇔ lỗi ERROR_INVALID_FUNCTION) chỉ còn là
   đường dự phòng khi A không kết luận. Giá trị thô của cả hai
   API + phán quyết được: đẩy vào log panel lúc quét (dòng
   `[Firmware]`), ghi vào file log quét (dòng `Firmware raw:`),
   ghi vào log cài đặt ngay đầu quá trình (dòng `Firmware:` qua
   trường mới `ExecPlan.firmware_detail`), và ghi lại tại
   register_bcd kèm tín hiệu thứ ba — đối chiếu
   `bcdedit /enum {fwbootmgr}` (chỉ tồn tại trên máy UEFI):
   khớp thì log xác nhận, lệch thì log CẢNH BÁO (chỉ đối chiếu,
   phán quyết không đổi tại điểm đó). Bối cảnh: máy thực địa báo
   "UEFI (MBR)" nhưng đĩa mang tên thiết bị kiểu Hyper-V Gen-1
   "Virtual HD ATA Device" — từ nay mọi tín hiệu đều nằm trong
   log để phân xử mà không cần thêm vòng gửi/nhận.
3. **Chuỗi boot.sdi xoay theo firmware**: hai bản trong
   `%WINDIR%\Boot\DVD` nay thử theo phán quyết — UEFI: bản
   `EFI` trước, bản `PCAT` sau; Legacy: ngược lại. (Bản cũ luôn
   PCAT trước; lần chạy v0.7.6 trên máy UEFI đã nạp đúng bản
   PCAT vào staging — với WIM đã sửa, biến thể khớp firmware
   loại trừ nốt nghi phạm thứ hai này.) Hai vị trí ISO giữ
   nguyên ở đầu chuỗi, WinRE giữ cuối; mọi dòng log thử/dùng
   nay kèm nhãn biến thể ("bản EFI — biến thể UEFI" /
   "bản PCAT — biến thể BIOS" / ...), và một dòng log mở đầu
   nêu thứ tự thử theo firmware.
4. **Kiểm chứng BCD sau đăng ký — chế độ chẩn đoán log-only**
   (hàm mới `verify_bcd`, gọi ngay sau `/bootsequence`): dump
   `bcdedit /enum all` NGUYÊN VĂN vào log cài đặt (mọi lệnh enum
   chạy qua run_logged như các lệnh BCD khác), rồi đối chiếu
   từng phần tử trên output của loader / device options /
   {bootmgr} và ghi dòng khớp/KHÔNG KHỚP. Chỉ dựa trên giá trị
   không phụ thuộc ngôn ngữ (chuỗi ramdisk=..., winload path,
   partition=..., GUID); systemroot/winpe là kiểm mềm. KHÔNG
   chặn, KHÔNG gỡ entry — logic phần tử đã được log thực địa
   chứng minh đúng nguyên văn, cổng chặn thật nằm ở bước WIM
   (mục 1); dump này là bằng chứng để lần test sau đối chiếu
   mà không cần thêm vòng gửi log.

**Bảng đối chiếu phần tử BCD — bản cũ (≤ v0.7.6) vs v0.7.7:**

| Phần tử | Bản cũ (≤ v0.7.6) | v0.7.7 |
|---|---|---|
| Device options: tạo object | `bcdedit /create /d "QSU WinPE Ramdisk" /device` | Không đổi |
| `ramdisksdidevice` | `partition=<Data>:` | Không đổi; được đối chiếu lại trong dump chẩn đoán |
| `ramdisksdipath` | `\<stagingFolder>\boot.sdi` (chỉ path, không volume) | Không đổi; được đối chiếu lại trong dump chẩn đoán |
| Loader: tạo object | `bcdedit /create /d "QSU WinPE" /application osloader` | Không đổi |
| Loader `device` | `ramdisk=[<Data>:]\<stagingFolder>\winpe.wim,{ramdiskoptions-guid}` | Không đổi; đối chiếu: ≥2 dòng (device + osdevice) chứa đúng chuỗi này trong `/enum` của loader |
| Loader `osdevice` | Cùng chuỗi ramdisk như `device` | Không đổi (đếm chung với `device` ở trên) |
| Loader `path` | UEFI: `\windows\system32\boot\winload.efi`; Legacy: `\windows\system32\winload.exe` — chọn theo `plan.uefi` | Logic không đổi, NHƯNG nguồn phán quyết đổi: `plan.uefi` nay từ probe hai API (GetFirmwareType ưu tiên) thay cho trick một API; path được đối chiếu lại trong dump |
| Loader `systemroot` | `\windows` | Không đổi; kiểm mềm trong chẩn đoán |
| Loader `winpe` | `yes` | Không đổi; kiểm mềm trong chẩn đoán |
| Loader `detecthal` | `yes` | Không đổi |
| Kích hoạt một lần | `bcdedit /bootsequence <loader-guid>` — không hề đụng displayorder | Không đổi; chẩn đoán xác nhận GUID loader xuất hiện trong `/enum {bootmgr}` (chỉ có thể nằm ở bootsequence) |
| boot.sdi nạp vào staging | Chuỗi dự phòng thứ tự cố định: ISO sources → gốc ISO → **PCAT → EFI** → WinRE | ISO sources → gốc ISO → **theo firmware: UEFI thì EFI → PCAT, Legacy thì PCAT → EFI** → WinRE; log nêu biến thể đã nạp |
| Cờ bootable của winpe.wim | Không kiểm tra — WIM của ISO mod mất "Boot Index" vẫn được đăng ký BCD y nguyên → Boot Manager 0xc000000e | **Kiểm chứng sau inject; chưa bootable thì export lại `/Bootable` + thay tên + kiểm chứng lại; không đạt thì dừng trước BCD** |
| Đọc lại sau đăng ký | Không có — đăng ký xong ghi stage.json và mời reboot ngay | Dump `/enum all` nguyên văn + đối chiếu từng phần tử vào log (chẩn đoán, không chặn) |
| Bằng chứng firmware trong log | Chỉ một dòng verdict "UEFI (MBR)" ở header | Giá trị thô hai API ở log panel + log quét + log cài đặt; thêm đối chiếu {fwbootmgr} tại register_bcd |

Gói: `UtilityRebuild-v0.7.7.zip` (9.638.305 byte, 16 file,
testzip sạch; drop_v0.7.7 = drop_v0.7.6 + wizard build lại +
HUONG-DAN cập nhật). Toàn workspace build release sạch (0 lỗi,
0 cảnh báo). **Chưa test trên máy thật** — ca test quyết định:
chạy lại kịch bản cũ trên DESKTOP-J8LPB75, kỳ vọng log có dòng
"Kiểm chứng WIM: ... 'WIM Bootable : No'" → export /Bootable →
"Kiểm chứng đạt: winpe.wim sau chuẩn hoá BÁO BOOTABLE", boot.sdi
nạp bản EFI (máy UEFI), dump `/enum all` nằm trong log, và máy
reboot vào WinPE để engine cài thật.

## v0.7.8 — Sửa chọn nhầm index WinPE (2026-11-17, sau ca test v0.7.7)

- **Gốc rễ mới (đọc từ install-20261117-1335.log + ảnh lỗi v0.7.7):**
  boot.wim của ISO mod có index 1 = "Microsoft Windows PE (amd64)",
  index 2 = "Microsoft Windows Setup (amd64)". Quy ước cũ trong
  `probe_winpe_wim` ("index 2 là ảnh WinPE boot") là sai với cả
  boot.wim chuẩn (thường index 1 là WinPE, index 2 là Setup) lẫn
  ISO mod này — nên v0.7.6/0.7.7 đã mount/inject/xuất lại nhầm ảnh
  Setup, cờ /Bootable không bao giờ gắn đúng ảnh WinPE, kiểm chứng
  lại báo `Some(false)` và dừng trước BCD (chốt an toàn hoạt động
  đúng). Đây cũng là một phần nguyên nhân 0xc000000e của v0.7.6.
- **Sửa:** `probe_winpe_wim` chọn index theo TÊN ảnh (chứa
  "Windows PE", không chứa "Setup"); không phân biệt được bằng tên
  thì rơi về index 1, rồi index đầu tiên. Log ghi rõ tên ảnh đã
  chọn. Chốt chặn kiểm chứng sau xuất lại giữ nguyên.
- **Phụ:** HUONG-DAN v0.7.8 bổ sung lưu ý máy Gen 2: cài Windows 11
  chuẩn làm máy nền phải bật TPM ảo (Settings → Security → Enable
  Trusted Platform Module, kèm Secure Boot template Microsoft
  Windows), không bật sẽ bị chặn "This PC can't run Windows 11".
- Gói: `~/workspace/UtilityRebuild-v0.7.8.zip` (9.640.837 byte,
  16 file, testzip sạch; drop_v0.7.8 = drop_v0.7.7 + wizard
  `CaiDatWinTuDong.exe` build lại 2.911.183 byte + HUONG-DAN cập
  nhật). Build release sạch (0 lỗi, 0 cảnh báo), đã xác thực binary
  trong zip chứa chuỗi mới. **Chưa test trên máy thật; chỉ gửi khi
  anh nhắn "gửi" lúc đang ở máy** — ca test quyết định: chạy lại
  kịch bản cũ, kỳ vọng log chọn index 1 (WinPE thật), export
  /Bootable rồi "Kiểm chứng đạt: ... BÁO BOOTABLE".

## v0.7.9 — Cửa sổ wizard thích ứng kích thước màn hình (2026-11-17)

- **Bối cảnh:** thực địa trên PC thật màn hình nhỏ — cửa sổ wizard
  cố định (ngoài 1060x1102, khung WS_CAPTION | WS_SYSMENU không
  kéo giãn được) tràn khỏi màn hình, khối đáy (thanh tiến độ + ô
  log + nút TIẾN HÀNH) bị khuất; người dùng yêu cầu cửa sổ tự
  vừa/thu nhỏ được.
- **Phạm vi:** CHỈ crate `cai_dat_win`, CHỈ trình bày/bố cục.
  Không đụng logic cài đặt — sửa chọn index WinPE của v0.7.8 giữ
  nguyên; mọi crate khác không đổi (đối chứng: WinPEInstall.exe
  trong gói trùng md5 v0.7.8 `bc2f6fbf…`, mọi file khác trong zip
  ngoài wizard + HUONG-DAN đều trùng byte với v0.7.8).
- **Cách làm:**
  - `build_ui` ghi rect thiết kế của cả 52 control vào bảng
    `Model.layout` (đọc thẳng từ control vừa dựng qua
    position()/size(), không chép tay toạ độ) kèm vai trò bố cục,
    và mốc `Model.design_client` = kích thước client đo ngay khi
    cửa sổ tạo theo đúng thông số v0.7.8.
  - `adapt_window_frame` (gọi một lần trong build_ui): thêm
    WS_THICKFRAME | WS_MAXIMIZEBOX tại chỗ qua
    SetWindowLongPtrW + SWP_FRAMECHANGED, đo khung mới rồi đặt
    kích thước ngoài sao cho client = mốc thiết kế — màn hình đủ
    lớn thì bố cục trùng v0.7.8 từng pixel dù khung dày hơn; màn
    hình nhỏ thì chặn ở vùng làm việc (SPI_GETWORKAREA); cửa sổ
    được căn giữa vùng làm việc.
  - `apply_layout` (luỹ đẳng — hàm thuần của bảng thiết kế +
    kích thước client hiện tại, áp bằng DeferWindowPos một lượt):
    khối đáy bám đáy; 5 danh sách co giãn (ISO FLEX_ISO, danh mục
    FLEX_CAT, phiên bản FLEX_ED, phân vùng FLEX_PARTS, log
    FLEX_LOG) hấp thụ delta chiều cao — thừa chia theo trọng số
    [2,1,1,1,3] (log nở nhất), thiếu chia theo [3,2,2,2,1] (log
    co ít nhất) với sàn chiều cao [30,30,30,30,44] nên control
    không bao giờ chồng nhau (thiếu vượt khả năng co thì chỉ
    khuất đáy, không vỡ bố cục); ngang: cột phải bám lề phải
    (ShiftRight), danh sách/nhãn dài nở theo bề ngang (Grow).
    Chạy lúc khởi động (trong finish_build, vì WM_SIZE lúc adapt
    xảy ra trước khi handler được gắn) và mỗi
    OnResize/OnWindowMaximize — NWG tách maximize thành sự kiện
    riêng nên phải nghe cả hai, đối chiếu HWND cửa sổ chính để bỏ
    qua sự kiện của control con.
  - Kích thước tối thiểu 900x640 áp qua WM_GETMINMAXINFO bằng raw
    handler (bind_raw_event_handler của NWG, id 0x10001); handler
    rò có chủ đích (Box::leak) như fonts.
  - Tương tác với cơ chế cũ giữ nguyên: `shift_window_for_exec`
    chỉ dịch vị trí (SWP_NOSIZE — không phát sinh bố cục lại),
    `restore_window_pos` khôi phục cả kích thước thì WM_SIZE tự
    cân lại bố cục theo rect đã lưu.
- Gói: `~/workspace/UtilityRebuild-v0.7.9.zip` (9.654.306 byte,
  16 file, testzip sạch; drop_v0.7.9 = drop_v0.7.8 + wizard
  `CaiDatWinTuDong.exe` build lại 2.924.759 byte, md5
  `17f1160a…` + HUONG-DAN cập nhật mục v0.7.9). Build release
  sạch (0 lỗi, 0 cảnh báo). **Chưa test trên máy thật** — ca
  test: mở wizard trên màn hình nhỏ/session từ xa thấp (khối đáy
  phải thấy trọn), kéo giãn + phóng to (danh sách và ô log nở
  theo, cột phải bám lề), màn hình lớn (bố cục như v0.7.8), chạy
  một ca cài đặt để xác nhận shift/restore cửa sổ lúc thực thi
  vẫn như cũ.

## v0.7.10 — Wizard gom thành 2 tab, cửa sổ gọn hẳn (2026-11-17)

- **Bối cảnh:** thực địa tiếp theo của v0.7.9 trên PC thật — dù cửa
  sổ đã tự co theo vùng làm việc, biểu mẫu một cột vẫn quá cao:
  nút TIẾN HÀNH CÀI ĐẶT nằm dưới taskbar, kỹ thuật viên phải bật
  ẩn taskbar mới bấm được. Người dùng yêu cầu gom lại thành 2 tab
  và cân đối lại biểu mẫu.
- **Phạm vi:** CHỈ crate `cai_dat_win`, CHỈ trình bày/bố cục.
  Không đụng một dòng logic cài đặt nào (preflight, ISO/WIM,
  staging, BCD, đĩa, stage.json giữ nguyên hành vi từng byte);
  mọi crate khác không đổi — đối chứng: trong gói chỉ
  `CaiDatWinTuDong.exe` khác byte so với v0.7.9, kèm HUONG-DAN.
- **Cách làm:**
  - Dùng `nwg::TabsContainer` + `nwg::Tab` thật của NWG 1.0.13
    (không phải trang giả lập): NWG tự hiện trang được chọn / ẩn
    trang còn lại khi đổi tab (TCN_SELCHANGE) và tự khớp kích
    thước hai trang khi khung đổi kích thước (xử lý WM_SIZE nội
    bộ); handle của mọi control con giữ nguyên hiệu lực khi trang
    bị ẩn nên refresh/render log/cập nhật lập trình chạy y như cũ,
    kể cả trên trang đang ẩn. Không tự chuyển tab khi bắt đầu cài.
  - Tab 1 "Nguồn cài đặt": mục 1 tiền đề (5 dòng trạng thái + chọn
    ổ Data + Quét lại), mục 2 nguồn ISO (list ISO + Chọn ISO, kho
    online + Tải danh mục/Tải ISO, thanh tải + nhãn), mục 3 phiên
    bản (list edition + thông tin).
  - Tab 2 "Tuỳ chọn cài đặt": mục 4 (hai nút chế độ cài, hai nút
    chế độ phân vùng, khối đĩa đích: combo + hint + list phân
    vùng + cảnh báo + ô serial), tên máy/tài khoản/mật khẩu, nút
    Tuỳ Chỉnh Windows + dòng trạng thái staging.
  - Khối ghim ở SÂN CHÍNH dưới khung tab (luôn thấy ở cả hai tab):
    thanh tiến độ thực thi + nhãn, ô log, TIẾN HÀNH CÀI ĐẶT, Đóng.
    `main_window_hwnd` (GetParent của btn_close) và owner hộp chọn
    file trong `on_browse` không đổi vì btn_close vẫn là con trực
    tiếp của cửa sổ chính.
  - Kích thước thiết kế: ngoài 1036x748 (client ~1020x709, trang
    tab ~977x485) — trên màn hình 1366x768 (vùng làm việc ~728)
    toàn bộ cửa sổ nằm gọn trên taskbar ngay ở kích thước thiết
    kế, không cần co.
  - Bố cục thích ứng v0.7.9 mở rộng theo 3 sân toạ độ: LayoutItem
    thêm trường `group` (GROUP_MAIN/GROUP_TAB1/GROUP_TAB2),
    `apply_layout` tính riêng từng sân theo client của sân đó;
    `layout_window` áp sân chính trước (khung tab đổi kích thước
    → NWG khớp trang đồng bộ) rồi đo client trang và áp hai sân
    tab. Khung tab là phần tử co giãn mới FLEX_TABS (trọng số
    nở 2 / co 2, sàn 240) ở sân chính cùng FLEX_LOG;
    `flex_height_deltas` nâng lên 6 ô và CHỈ chia delta cho các ô
    có mặt trong sân (design_h > 0) — phần dư nở dồn vào ô có
    trọng số nở lớn nhất của sân (trước đây cứng vào FLEX_LOG,
    đúng khi cả 5 ô luôn có mặt). Kiểm tra biên bằng số: ở mức co
    tối đa của cửa sổ tối thiểu 900x640, sân tab 1 cần co 72 ≤
    khả năng 118, tab 2 cần 72 ≤ khả năng 74 của list phân vùng —
    không control nào bị cắt.
  - Mọi control giữ nguyên ID/handler/hành vi; `update_part_ui`
    (ShowWindow khối đĩa) chạy y như cũ trên trang 2 đang ẩn/hiện.
  - Hàm/đoạn đã sửa trong `cai_dat_win/src/main.rs`: hằng FLEX_*
    + GROUP_* (mở rộng), `LayoutItem` (thêm group), `Model` (thêm
    design_page), `App` (thêm tabs/tab_src/tab_opt),
    `build_ui` (dựng lại toàn bộ: cửa sổ 1036x748, khung tab +
    hai trang, mọi control gắn cha mới + toạ độ mới, bảng bố cục
    ghi thêm group, đo design_page), `flex_height_deltas`,
    `apply_layout`, `layout_window`. Xoá hằng HEADER_H (chỉ còn
    được dùng bởi toạ độ cũ). Không hàm nghiệp vụ nào bị đụng.
- Gói: `~/workspace/UtilityRebuild-v0.7.10.zip` (9.646.666 byte,
  16 file, testzip sạch; drop_v0.7.10 = drop_v0.7.9 + wizard
  `CaiDatWinTuDong.exe` build lại 2.923.469 byte, md5
  `59914d3a…` — đã đối chứng md5 của file trong zip — + HUONG-DAN
  cập nhật mục v0.7.10). Build release sạch (0 lỗi, 0 cảnh báo).
  **Chưa test trên máy thật** — ca test: mở wizard trên màn hình
  1366x768 (nút TIẾN HÀNH phải thấy và bấm được ngay, không cần
  ẩn taskbar), đổi qua lại 2 tab, chạy một ca cài ở cả hai chế độ
  (khối đĩa ở tab 2 hiện/ẩn đúng theo chế độ), kéo giãn + phóng
  to (ba sân co giãn đúng, khối ghim bám đáy), kiểm tra hộp chọn
  ISO mở từ tab 1.

## v0.7.11 — Gỡ cổng kiểm tra cờ bootable của WinPE (2026-11-17)

- **Chỉ thị người dùng (nguyên văn):** "Bỏ qua kiểm tra. Các tool
  khác cài được" — gỡ cổng kiểm chứng bootable, dùng WinPE nguyên
  trạng của ISO như các tool khác.
- **Lịch sử thực địa của cổng (v0.7.7 → v0.7.10):** cổng gồm
  `check_wim_bootable` (phân tích text output DISM /Get-WimInfo:
  tìm "Boot Index" ở header, "WIM Bootable : Yes/No" ở chi tiết
  index) + `ensure_winpe_bootable` (chưa bootable thì xuất lại
  /Bootable rồi kiểm chứng lại, không đạt thì dừng TRƯỚC đăng ký
  BCD). Kết quả thực địa: **chặn oan 4/4 lần chạy** —
  (1) v0.7.7 máy ảo Legacy + ISO mod: xuất lại vẫn Some(false);
  (2) v0.7.7 PC UEFI thật + ISO mod: y hệt; (3) v0.7.9 (đã có sửa
  chọn index theo tên của v0.7.8) PC UEFI thật + ISO mod: y hệt;
  (4) v0.7.9 PC UEFI thật + **ISO Microsoft gốc**: vẫn xuất lại
  và vẫn Some(false) — bằng chứng quyết định là cơ chế đọc text
  DISM tự nó phán sai, không phải ISO hỏng. Cả 4 lần chốt chặn
  đều dừng trước BCD nên không máy nào bị xoá hay hỏng boot.
- **Thay đổi (CHỈ crate `cai_dat_win`, đúng một điểm):**
  - Xoá hẳn hai hàm `check_wim_bootable` và
    `ensure_winpe_bootable` cùng toàn bộ doc-comment của chúng
    (190 dòng). Mọi helper chúng dùng (`dism_wim_info`,
    `log_dism_output`, `parse_kv`, `kv_get`,
    `clear_readonly_file`, `cmd_fail_msg`) đều còn được
    probe/mount dùng chung nên không phát sinh dead code.
  - Tại `exec_clean`, bước 71% cũ ("Chuẩn hoá WinPE (cờ
    bootable)..." + gọi cổng, lỗi thì `fail` + dừng) thay bằng:
    nhãn tiến độ "Hoàn tất chuẩn bị WinPE..." + một dòng log
    "Bỏ qua kiểm tra cờ bootable (theo yêu cầu) — dùng WinPE
    nguyên trạng của ISO." rồi flow đi thẳng sang bước driver
    74% → unattend → đăng ký BCD. Comment tại `register_bcd`
    (nơi mô tả cổng là "cổng chặn thật") cũng sửa cho khớp.
  - **KHÔNG đụng:** `probe_winpe_wim` (chọn index) giữ nguyên;
    đường export dự phòng khi MOUNT lỗi trong luồng mount
    (`DISM /Export-Image /Compress:Max /Bootable` — đường khôi
    phục mount của v0.7.6, không phải cổng) giữ nguyên từng byte;
    giao diện 2 tab v0.7.10, staging, BCD, đĩa, stage.json và
    mọi crate khác giữ nguyên.
- **Ghi chú an toàn (đứng vững sau khi gỡ cổng):** mục boot WinPE
  vẫn là boot MỘT LẦN (`/bootsequence`) — WinPE không lên được
  thì lần khởi động kế tiếp máy tự về Windows cũ, chưa có gì bị
  xoá; việc xoá/chia ổ chỉ xảy ra trong WinPE sau khi engine đối
  chiếu lại serial ổ đích. Cái mất đi chỉ là một lớp phán quyết
  bằng text đã chứng minh là phán sai 4/4.
- Gói: `~/workspace/UtilityRebuild-v0.7.11.zip` (9.642.196 byte,
  16 file, testzip sạch; drop_v0.7.11 = drop_v0.7.10 + wizard
  `CaiDatWinTuDong.exe` build lại 2.910.664 byte, md5
  `c9f70ceb…` — đã đối chứng md5 của file trong zip). Build
  release sạch (0 lỗi, 0 cảnh báo, đã ép biên dịch lại toàn bộ
  crate để xác nhận). **Chưa test trên máy thật** — ca test quyết
  định: chạy lại kịch bản cũ trên PC UEFI thật với ISO mod: kỳ
  vọng log có dòng "Bỏ qua kiểm tra cờ bootable…", staging chạy
  một mạch tới đăng ký BCD, máy reboot vào WinPE và engine bắt
  đầu cài thật.

## v0.7.12 — Wizard thêm tab Hướng dẫn + Tài liệu, log có dòng [Cơ chế] (2026-11-17)

- **Phạm vi: CHỈ tài liệu + giao diện, trên đúng cơ chế v0.7.11.**
  Không một dòng logic cài đặt nào thay đổi: staging, chọn index,
  mount/inject, boot.sdi, BCD, đĩa, stage.json, engine — tất cả
  giữ nguyên từng byte hành vi. File nguồn duy nhất bị đụng là
  `rust/cai_dat_win/src/main.rs`; trong gói, chỉ
  `modules/CaiDatWinTuDong.exe` khác v0.7.11 (đối chứng diff
  toàn bộ drop: mọi binary còn lại trùng byte).
- **Hai tab mới (3 "Hướng dẫn", 4 "Tài liệu"):** khung
  TabsContainer của v0.7.10 mở rộng từ 2 lên 4 trang, đúng cách
  làm cũ — NWG tự hiện/ẩn trang và khớp kích thước MỌI trang con
  khi khung đổi kích thước (EnumChildWindows trong WM_SIZE của
  TabsContainer), handle control con giữ nguyên hiệu lực khi
  trang ẩn nên các luồng cập nhật lập trình không bị ảnh hưởng.
  Mỗi trang mới chứa đúng MỘT `nwg::TextBox` chỉ-đọc phủ toàn
  trang (8,6 / 961×466 ở mốc thiết kế): TextBox của NWG mặc định
  đã nhiều dòng + WS_VSCROLL, thêm ES_READONLY bằng cùng helper
  `set_edit_style` như ô log; nội dung lấy từ hai hằng
  `HUONG_DAN_TEXT` / `TAI_LIEU_TEXT` trong main.rs, đổi
  `\n` → `\r\n` lúc dựng (ô EDIT nhiều dòng cần CRLF), giới hạn
  ký tự nới lên 1.000.000.
  - Tab "Hướng dẫn": 8 bước cho người dùng cuối (chuẩn bị ISO
    trên ổ Data + cắm sạc → quét máy → chọn ISO/phiên bản → chọn
    chế độ cài → phân vùng + ổ đích + gõ serial → tên
    máy/tài khoản, mật khẩu để trống được → bấm TIẾN HÀNH →
    máy tự reboot và tự cài) + mục "Nếu WinPE không lên được"
    (boot một lần nên tự về Windows cũ).
  - Tab "Tài liệu": tài liệu cơ chế của CHÍNH bản build này
    (10 mục: staging + marker/stage.json; chọn ảnh WinPE theo
    Boot Index/tên; boot.sdi theo firmware; driver offline;
    /bootsequence một lần; nhận diện serial; preserve/wipeDisk
    + layout GPT/MBR + WinRE; custom chép nguyên trạng; lý do
    gỡ cổng bootable ở v0.7.11; log). Bản sao file:
    `docs/co_che_v0.7.12.md` — sửa nội dung phải sửa cả hai nơi.
- **Bố cục:** thêm hai sân GROUP_TAB3/GROUP_TAB4. Ô text của mỗi
  trang đăng ký flex với chỉ số FLEX_TABS: trong lần gọi
  `apply_layout` cho sân của nó, nó là phần tử co giãn DUY NHẤT
  nên hấp thụ trọn delta chiều cao của trang — không cần thêm
  chỉ số flex mới, không đụng `flex_height_deltas`. Khối ghim
  dưới khung tab, kích thước thiết kế 1036×748, chặn vùng làm
  việc và min 900×640 giữ nguyên.
- **Dòng [Cơ chế] trong log:** 9 lệnh `log.line("[Cơ chế] …")`
  cộng vào `exec_clean` tại các bước chính — Logger ghi đồng
  thời ra file install-*.log và ô log panel như mọi dòng khác:
  (a) staging trên ổ Data + marker/stage.json (đầu exec_clean);
  (b1) chọn ảnh WinPE theo Boot Index/tên (sau probe); (b2) vì
  sao không kiểm tra cờ bootable (bước 71%, cạnh dòng cũ của
  v0.7.11); (c) boot.sdi theo firmware (sau khi stage xong
  boot.sdi); (d) inject driver offline (bước 74%); (e) mục boot
  một lần /bootsequence (bước 84%, trước register_bcd); (f)
  nhận diện ổ đích bằng serial + (g) chế độ phân vùng đang dùng
  — preserve một dòng riêng, wipeDisk một dòng riêng ghi layout
  GPT/MBR (bước 92%, trước khi ghi stage.json). Thuần chuỗi
  log: không điều kiện, không nhánh, không đổi thứ tự các bước.
- Gói: `~/workspace/UtilityRebuild-v0.7.12.zip` (9.649.797 byte,
  16 file, testzip sạch; drop_v0.7.12 = drop_v0.7.11 + wizard
  `CaiDatWinTuDong.exe` build lại 2.929.925 byte, md5
  `e975fd2e…` — đã đối chứng md5 của file trong zip). Build
  release sạch (0 lỗi, 0 cảnh báo, đã ép biên dịch lại toàn bộ
  crate để xác nhận). **Chưa test trên máy thật** — ca test:
  mở wizard, qua lại đủ 4 tab (hai tab tài liệu đọc được, cuộn
  được, không tab nào che khối ghim), chạy một ca cài mới và
  đối chiếu log có đủ các dòng [Cơ chế] theo đúng thứ tự bước.

## v0.7.13 — Chế độ phân vùng "rebuild": cài lại phân vùng hệ thống, giữ phân vùng data (2026-11-17)

- **Nhu cầu (người dùng, 2026-11-17):** tối ưu phần đĩa cho máy
  CHỈ CÓ 1 ĐĨA — chọn các phân vùng EFI, MSR, Windows cũ trên
  chính đĩa đó, xoá và cài lên đúng các phân vùng này, phân vùng
  data trên cùng đĩa giữ lại. Hai chế độ cũ không đáp ứng được:
  preserve chỉ format phân vùng OS (không chia lại được khi
  layout cũ lệch chuẩn), wipeDisk cấm tuyệt đối đĩa chứa
  staging/data (máy 1 đĩa = staging nằm trên chính đĩa đích).
- **stage.json:** `partitionMode` nhận thêm giá trị `"rebuild"`;
  mảng mới `"rebuildPartitions"`: mỗi mục `{kind, sizeBytes,
  gptGuid, offsetBytes}` cho từng phân vùng sẽ xoá, chụp từ liệt
  kê đĩa lúc staging. `qsu_common::stage::RebuildPart` +
  `StageConfig.rebuild_partitions` (from_json mặc định vec rỗng
  → stage.json cũ hành vi y nguyên v0.7.12). `targetDiskSerial`
  nay dùng cho cả wipeDisk và rebuild.
- **qsu_common::disk:** `DiskPartition` có thêm `part_guid` (GUID
  DUY NHẤT của phân vùng GPT — PartitionId từ
  IOCTL_DISK_GET_DRIVE_LAYOUT_EX; rỗng với MBR và đường dự phòng)
  + RawPart tương ứng. Thêm `volume_disk_extent(letter)` (extent
  vật lý của một volume theo ký tự ổ). Thêm khối REBUILD:
  `rebuild_layout(region, uefi)` (None nếu vùng < boot+MSR+
  Recovery+Windows 10GB), `rebuild_script` (xoá theo số phân
  vùng LIVE, offset giảm dần; tạo mới GHIM offset bằng KB vào
  đúng vùng trống; GPT EFI 260/MSR 16/Windows/Recovery 600 +
  GUID WinRE + attributes; MBR SysRes 100 active/Windows/
  Recovery 600 id 27; KHÔNG clean/convert), và
  `execute_rebuild(...)` theo đúng khuôn execute_wipe: liệt kê
  lại + đối chiếu serial → đối chiếu từng mục spec (GPT theo
  GUID, MBR theo offset+size ±1MB, đúng 1 khớp, không khớp
  trùng) → chứng minh extent staging không giao danh sách xoá
  → kiểm tra liên tục (kề nhau ±1MB) → kiểm tra đủ chỗ — TẤT CẢ
  trước khi xoá → chạy diskpart → xác minh phân vùng mới trong
  vùng + mọi phân vùng giữ lại còn nguyên offset/size.
- **Wizard:** `partition_wipe: bool` thay bằng enum `PartMode`
  {Preserve, WipeDisk, Rebuild} (Model + ExecPlan;
  `stage_str()`). Radio thứ ba "Cài lại phân vùng hệ thống (giữ
  data)" (hàng radio co còn 312px/nút). `lst_disk_parts` đổi từ
  ListBox sang **ListView** 4 cột (Phân vùng/Loại/Dung lượng/Ghi
  chú) + helper checkbox Win32 thô (cùng khuôn Tuỳ Chỉnh/Dọn
  Rác, NHƯNG hằng số `LVS_EX_CHECKBOXES` ở đây là **0x4** — giá
  trị đúng theo winapi/commctrl; hai module cũ đang dùng 0x400
  = LVS_EX_INFOTIP, lỗi tiềm ẩn ghi nhận riêng, ngoài phạm vi
  bản này). Phân loại phân vùng (`classify_partition`, theo ký
  tự ổ + serial volume, staging xét trước Windows): Windows cũ
  = tick + khoá; EFI/MSR/Recovery/System = tick sẵn, bỏ được;
  staging = khoá không-tick ("được bảo vệ"); data = khoá
  không-tick ("giữ nguyên"). Ép hợp lệ qua handler
  OnListViewItemChanged (`enforce_rebuild_checks`); trạng thái
  tick được chụp lại khi vẽ lại (`snapshot_rebuild_checks` +
  `Model.rebuild_checks/parts_disk`). `disk_block_reason` nhận
  thêm PartMode: rebuild cho phép đĩa chứa Data, cấm đĩa
  from_fallback, không áp ngưỡng 25GB của wipe. Xác nhận mức
  wipe: tóm tắt liệt kê từng phân vùng sẽ xoá + các phân vùng
  giữ nguyên + bắt buộc gõ serial đĩa; wizard kiểm lại độc lập
  (phân vùng bảo vệ/data không bao giờ vào danh sách xoá; vùng
  đã chọn ≥ (876|700)MB + 10GB). Log cài đặt thêm dòng [Cơ chế]
  riêng cho rebuild (cạnh dòng wipe/preserve ở bước 92%).
- **Engine (winpe_install):** nhánh rebuild mới — thiếu serial
  hoặc rebuildPartitions rỗng → **exit 13**; không tìm thấy đĩa
  theo serial → 13; đĩa Data trùng đĩa đích thì BẮT BUỘC đọc
  được extent của nó (không đọc được → 13), khác đĩa thì thôi;
  `execute_rebuild` Err → 13. Exit 13 = "rebuild bị chặn ở khâu
  đối chiếu/kiểm tra — đĩa chưa bị đụng vào ở các lỗi đối
  chiếu" (phân biệt với 10/11/12 của wipe). Sau khi chia: format
  riêng được bỏ qua như wipe, WinRE chạy theo recovery letter
  mới, bcdboot /f tường minh trỏ vào EFI MỚI (hai biến
  wipe_boot/recovery_letter đổi tên thành layout_* dùng chung
  cho wipe + rebuild). preserve/wipeDisk không đổi hành vi.
- **Tài liệu:** tab "Tài liệu" §6 (câu bảo vệ staging theo từng
  chế độ) + §7 đổi thành "BA CHẾ ĐỘ PHÂN VÙNG" với đoạn REBUILD
  đầy đủ; tab "Hướng dẫn" BƯỚC 5 thêm mục rebuild; bản sao
  `docs/co_che_v0.7.13.md` (đã đối chiếu khớp từng chữ với
  TAI_LIEU_TEXT); HUONG-DAN.txt có mục v0.7.13 + 4 ca test ưu
  tiên (đầu bảng: máy 1 đĩa EFI+MSR+C+D data → rebuild → D
  nguyên vẹn, Windows mới boot được).
- Gói: `~/workspace/UtilityRebuild-v0.7.13.zip` (9.706.517
  byte, 16 file, testzip sạch; drop_v0.7.13 = drop_v0.7.12 +
  wizard `CaiDatWinTuDong.exe` build lại 3.010.817 byte, md5
  `f5797b64…` + engine `modules/install/WinPEInstall.exe`
  build lại 1.650.242 byte, md5 `42edf8b0…` — đã đối chứng md5
  của hai file trong zip khớp bản trong drop). Build release
  sạch (0 lỗi, 0 cảnh báo, ép biên dịch lại các crate đã đụng).
  **Chưa test trên máy thật** — ca test quyết định: máy test
  demo 1 đĩa theo mục HUONG-DAN ở trên.

## v0.7.15 — Đóng gói lại WinPE thành WIM một ảnh bootable (2026-11-18)

- **Chẩn đoán từ log thực địa** `install-20261117-1746.log`
  (chạy v0.7.14 trên DESKTOP-704R89L, ISO mod
  Windows11-Pro-HDD4G, chế độ wipeDisk): wizard làm đúng mọi
  bước — 17:48:47 probe `/Get-WimInfo` thấy boot.wim KHÔNG có
  "Boot Index", index 1 = "Microsoft Windows PE (amd64)" (chi
  tiết: "WIM Bootable: No"), index 2 = "Microsoft Windows
  Setup", chọn index 1 theo tên; 17:48:59 mount + inject
  qsu_start.cmd/startnet.cmd/winpeshl.ini/WinPEInstall.exe +
  unmount /Commit thành công; 17:49:19 đăng ký BCD ramdisk
  một lần đúng công thức. Nhưng khi reboot, winload nạp ảnh
  **Windows Setup** thay vì ảnh PE đã inject — máy lên thẳng
  Windows Setup đòi media driver. Gốc lỗi: winpe.wim staging
  vẫn mang CẢ HAI ảnh và header không chỉ định ảnh PE đã
  inject là ảnh boot.
- **Bản sửa (chỉ crate cai_dat_win):** hàm mới
  `repack_winpe_single_image()` chạy ở mốc 69% của
  `exec_clean`, NGAY SAU mount/inject thành công (cả hai
  đường: mount thẳng — index = boot_index; export dự phòng —
  index = export_index, theo dõi bằng biến `injected_index`)
  và TRƯỚC đăng ký BCD:
  `dism /English /Export-Image /SourceImageFile:<staging>\winpe.wim /SourceIndex:<index đã inject> /DestinationImageFile:<staging>\winpe_single.wim /Bootable /Compress:Max`
  (xoá file đích sót trước vì DISM append vào file có sẵn),
  log nguyên văn lệnh + output như mọi lệnh DISM khác; export
  trả mã khác 0 hoặc file đích thiếu/rỗng → dừng bằng thông
  báo đọc được TRƯỚC BCD (winpe.wim gốc còn nguyên). Thành
  công: xoá bản hai-ảnh, đổi tên winpe_single.wim →
  winpe.wim, gỡ read-only, log kích thước mới. Áp dụng VÔ
  ĐIỀU KIỆN cho mọi ca cài mới (ISO mod hay gốc, mọi index).
- **Không phải cổng kiểm chứng quay lại:** hàm không hề đọc
  /Get-WimInfo hay phán "WIM Bootable: Yes/No" — cổng phán
  quyết của v0.7.7 vẫn gỡ hẳn như v0.7.11. Đây là bước đóng
  gói cơ học theo đúng nguyên tắc của tool gốc (nó trích ảnh
  PE ra WIM riêng một ảnh). Vì thế dòng log mốc 71% cũng đổi
  chữ: không còn "dùng WinPE nguyên trạng của ISO" mà ghi rõ
  WinPE đã inject được đóng gói lại thành WIM một ảnh
  bootable ở bước trên. Cộng 1 dòng [Cơ chế] mới ở mốc 69%
  giải thích cơ chế này.
- **Vì sao lần xuất lại của v0.7.7 không cứu được:** bản đó
  export theo quy ước vị trí "index 2 là WinPE" — tức xuất
  nhầm ẢNH SETUP (sau này v0.7.8 mới phát hiện index 1 mới là
  PE và sửa chọn index theo tên), và /Bootable theo tài liệu
  DISM chỉ có tác dụng trên ảnh WinPE thật; cộng thêm cổng
  kiểm chứng đọc chữ DISM phán sai. Lần này export đúng index
  PE ĐÃ INJECT (theo probe) và không phán quyết gì thêm.
- **Tài liệu:** tab "Tài liệu" mục 2 (thêm đoạn đóng gói lại)
  + mục 9 (bỏ câu "nguyên trạng", ghi rõ đóng gói cơ học từ
  v0.7.15); `docs/co_che_v0.7.15.md` = bản sao của
  co_che_v0.7.13.md + các thay đổi trên (đã đối chiếu thân
  tài liệu khớp từng chữ với TAI_LIEU_TEXT; tiện thể đồng bộ
  mục 4 của file này với thay đổi inject QSU-DriverBackup
  của v0.7.14 mà bản v0.7.13 còn thiếu); README_FULL §4.5
  (thêm mốc 69%, sửa dòng 71%), §4.7 (thêm đoạn "Đóng gói
  lại một ảnh") và bảng lịch sử phiên bản; HUONG-DAN.txt có
  mục v0.7.15 + 2 ca test ưu tiên (đầu bảng: lặp lại đúng
  kịch bản log 17:46 — DESKTOP-704R89L, ISO mod, wipe →
  reboot phải vào engine của tool, KHÔNG được lên Windows
  Setup; ca 2: ISO Client gốc).
- Gói: `~/workspace/UtilityRebuild-v0.7.15.zip` (9.980.182
  byte, 16 file, testzip sạch; drop_v0.7.15 = drop_v0.7.14 +
  wizard `CaiDatWinTuDong.exe` build lại 3.028.061 byte, md5
  `4d7a21c7…` + HUONG-DAN.txt cập nhật — 12 file còn lại,
  gồm engine WinPEInstall.exe, byte-identical với v0.7.14,
  đã đối chứng md5 từng file trong zip khớp bản trong drop).
  Build release sạch (0 lỗi, 0 cảnh báo). **Chưa test trên
  máy thật** — ca test quyết định là ca 1 trong HUONG-DAN.

## v0.7.16 — Engine tự gán ký tự ổ cho phân vùng boot sau khi chia (2026-11-18)

- **Chẩn đoán từ log thực địa** `winpe-20261117-1811.log`
  (chạy v0.7.15 trên DESKTOP-704R89L, ISO mod, chế độ
  wipeDisk, đích = SSD Gigabyte serial SN192808960256): bản
  repack WIM một ảnh của v0.7.15 ĐÃ CÓ TÁC DỤNG — WinPE boot
  đúng engine, engine tìm thấy staging theo marker + serial,
  đối chiếu lại serial đĩa đích, chạy script wipe (select
  disk 1, clean, convert gpt, EFI 260MB assign letter=S,
  MSR 16MB, Windows 61559MB assign W, Recovery 600MB
  assign R, set id + gpt attributes) — DiskPart báo thành
  công MỌI bước kể cả "successfully assigned the drive
  letter", exit 0. Nhưng liệt kê sau wipe (dòng 65–69 của
  log) thấy đủ 4 phân vùng đúng loại/nhãn/dung lượng, W: và
  R: có ký tự, còn phân vùng EFI "SYSTEM" thì **không có ký
  tự ổ**. Bước xác minh sau wipe đòi phân vùng boot phải có
  ký tự nên huỷ toàn bộ ca cài (dòng 70) — NGAY SAU bước phá
  huỷ và TRƯỚC khi bung ảnh. Hiện trạng máy khớp log: SSD có
  layout mới, phân vùng Windows trống rỗng, không gì boot
  được.
- **Bản sửa (crate qsu_common — `disk.rs`):** helper mới
  `ensure_boot_letter()` dùng chung cho cả `execute_wipe()`
  và `execute_rebuild()` (rebuild có bản sao cùng lỗi ở khối
  xác minh sau chia — sửa luôn cả hai, cùng một helper nên
  hành vi thống nhất):
  1. Phân vùng boot được khớp từ liệt kê IOCTL theo số đĩa +
     loại phân vùng (GPT: EFI System; MBR: System Reserved/
     cờ boot) — chính dữ kiện liệt kê đã nhận diện đúng trong
     log 1811.
  2. Có ký tự sẵn và probe đạt → dùng luôn. Thiếu ký tự (hoặc
     liệt kê báo có mà probe không truy cập được) → TỰ GÁN:
     ký tự ưu tiên 'S' như script chia đĩa đã định (loại trừ
     ký tự đã chốt cho Windows/Recovery và mọi ký tự đang
     bận theo GetLogicalDrives + probe); cơ chế 1 = script
     diskpart phụ (`select disk N` / `select partition
     <PartitionNumber>` / `assign letter=X`, ghi log nguyên
     văn như các script khác); cơ chế 2 dự phòng =
     `SetVolumeMountPointW` với tên volume tìm bằng quét
     FindFirstVolumeW + extent khớp (đĩa, offset).
  3. Tiêu chí thành công DUY NHẤT là probe thật:
     `GetFileAttributesW("X:\")` truy cập được — ký tự hiện
     hay không trong liệt kê volume không tính. Mỗi lần gán
     đều probe lại; thất bại thì thử lại 1 lần nữa.
  4. Chỉ Err khi: phân vùng boot vắng mặt hẳn trong layout
     sau chia, hết ký tự trống, hoặc cả 2 lần gán + probe
     đều thất bại. Mọi bước đều ghi log: phân vùng khớp (đĩa,
     số thứ tự, loại, dung lượng), ký tự trước/sau, cơ chế
     nào chạy, kết quả probe từng lần.
  Mọi thứ còn lại giữ nguyên hành vi: đối chiếu serial, nội
  dung script wipe/rebuild, ký tự định hướng S/W/R, các bước
  sau (bung ảnh, inject driver, bcdboot `/s <boot>`, chép
  WinRE) dùng ký tự boot đã chốt như trước. Module
  CaiDatWinWinPE (cài tương tác trong WinPE) cũng gọi
  `execute_wipe()` nên hưởng cùng bản sửa.
- **Bài học thiết kế (ghi để các bản sau noi theo):** một
  bước kiểm tra nằm SAU thao tác phá huỷ (xoá đĩa) thì phải
  TỰ CHỮA được lỗi nó phát hiện — hoặc ít nhất không được
  biến một lỗi sửa được (thiếu ký tự ổ) thành huỷ toàn bộ
  ca cài khi đĩa đã trắng. Kiểm tra "cứng" chỉ dành cho các
  điều kiện nằm TRƯỚC điểm phá huỷ (serial, đối chiếu phân
  vùng, đủ chỗ...) — những kiểm tra đó giữ nguyên.
- **Tài liệu:** tab "Tài liệu" mục 7 thêm gạch đầu dòng "Tự
  gán ký tự ổ cho phân vùng boot sau khi chia";
  `docs/co_che_v0.7.16.md` = bản sao của co_che_v0.7.15.md
  + thay đổi trên (đã đối chiếu thân tài liệu khớp từng chữ
  với TAI_LIEU_TEXT); README_FULL §4.11 thêm đoạn mô tả
  `ensure_boot_letter()` + dòng lịch sử v0.7.16;
  HUONG-DAN.txt có mục v0.7.16 + ca test ưu tiên (lặp lại
  đúng kịch bản wipe trên DESKTOP-704R89L → engine phải qua
  được bước phân vùng boot, bung ảnh, đăng ký boot và
  reboot vào OOBE của Windows mới).
- Gói: `~/workspace/UtilityRebuild-v0.7.16.zip` (10.037.752
  byte, 16 file, testzip sạch; drop_v0.7.16 = drop_v0.7.15
  + engine `WinPEInstall.exe` build lại (1.688.742 byte,
  md5 `1fd87006…`), wizard `CaiDatWinTuDong.exe` (3.048.053
  byte, md5 `94f62196…` — chỉ đổi chữ tab Tài liệu) và
  `CaiDatWinWinPE.exe` (2.795.696 byte, md5 `4679cded…` —
  cùng gọi execute_wipe) + HUONG-DAN.txt cập nhật — 10
  file còn lại byte-identical với v0.7.15, đã đối chứng md5
  từng file trong zip khớp bản trong drop). Build release
  sạch (0 lỗi, 0 cảnh báo). **Chưa test trên máy thật** —
  ca test quyết định là ca ưu tiên trong HUONG-DAN.

## v0.7.17 — Mount ISO ba lớp + UX xác nhận serial (2026-11-18)

- **Triệu chứng thực địa 1 (mount ISO):** trên DESKTOP-704R89L
  đang chạy Windows lite, chọn ISO gốc
  `E:\ISO\Win11_25H2_English_x64_v2.iso` rồi bấm "Đọc ISO" →
  hộp thoại "Mount-DiskImage không trả ký tự ổ đĩa (output: )"
  — output rỗng cả stdout lẫn stderr, đọc ISO bị huỷ. Đường
  mount cũ chỉ dựa vào MỘT lệnh PowerShell
  `Mount-DiskImage ... | Get-Volume` và chỉ nhìn stdout: lỗi
  của cmdlet (không thành terminating) bị nuốt — mã thoát vẫn
  0, không chữ nào lọt ra, người dùng không có cách biết lý do.
- **Bản sửa 1 (crate cai_dat_win — đường mount ISO viết lại
  thành BA LỚP, `mount_iso()`/`dismount_iso()`):**
  1. **Dùng lại mount có sẵn:** `Get-DiskImage` báo Attached
     thì lấy ký tự qua `Get-Volume` và dùng luôn (ISO mount
     bởi lần đọc trước / Explorer / tool khác khỏi mount lại).
     Sổ đăng ký mount trong tiến trình (OnceLock<Mutex<...>>)
     cũng giữ mount của chính wizard: đọc lại cùng ISO thì
     trả ngay ký tự đã ghi (kèm kiểm tra gốc ổ còn sống).
  2. **PowerShell với lỗi đầy đủ:** script đặt
     `$ErrorActionPreference = 'Stop'` và `throw` tường minh
     khi không có ký tự — lỗi cmdlet thành terminating, mã
     thoát ≠ 0, stderr mang nguyên văn lỗi. Mọi lệnh PowerShell
     trong đường này chạy qua `run_powershell()` thu cả hai
     luồng; thất bại được gộp bằng `ps_failure_text()` (mã
     thoát thập phân + hex + stderr/stdout, cắt theo biên ký
     tự) — KHÔNG còn "(output: )" rỗng. Lệnh mount thành công
     vẫn trích ký tự theo heuristic cũ (ký tự chữ cái đầu tiên).
  3. **VirtDisk API gốc (dự phòng cuối):** module FFI khai báo
     tay trong `main.rs` (winapi 0.3 không có virtdisk;
     `#[link(name = "virtdisk")]`, đã kiểm tra import table
     của exe có OpenVirtualDisk/AttachVirtualDisk từ
     VirtDisk.dll): `OpenVirtualDisk` với VIRTUAL_STORAGE_TYPE
     device ISO + vendor GUID Microsoft, access
     ATTACH_RO|DETACH; `AttachVirtualDisk` cờ READ_ONLY |
     PERMANENT_LIFETIME. Ký tự ổ dò bằng HIỆU TẬP VOLUME: chụp
     `GetLogicalDrives` trước attach, thăm dò tối đa ~10 giây
     tìm ký tự MỚI thuộc loại DRIVE_CDROM — ưu tiên ký tự có
     `sources\install.*`. Lỗi Open/Attach trả mã lỗi Windows
     (thập phân + hex) trong thông báo.
  - **Gỡ mount đối xứng:** sổ đăng ký ghi cơ chế + ký tự +
    HANDLE (dạng usize) theo từng ISO. `dismount_iso()` tra
    sổ: VirtDisk → `DetachVirtualDisk` + `CloseHandle` (kèm
    `Dismount-DiskImage` dọn dẹp vô hại); PowerShell/không có
    trong sổ → `Dismount-DiskImage` như cũ. Mọi điểm gọi
    dismount giữ nguyên vị trí (đổi ISO, đọc lỗi, cuối ca cài).
  - Nếu cả ba lớp đều hỏng, hộp thoại liệt kê lỗi của TỪNG cơ
    chế (PowerShell: ... / VirtDisk: ...) — ISO hỏng/không đọc
    được từ nay luôn kết thúc bằng lỗi thật của hệ điều hành.
  - Phần sau mount giữ nguyên byte-for-byte về hành vi: liệt
    kê edition từ ký tự ổ, chép sources, dò WIM, inject,
    repack một ảnh (v0.7.15), BCD.
- **Triệu chứng thực địa 2 (xác nhận serial — yêu cầu bổ sung
  của người dùng khi test rebuild):** vùng xác nhận serial
  "không dùng được": dòng định danh đĩa trong ô chọn bị cắt
  mất phần serial (ô chọn rộng 366px trong khi `summary()` gồm
  số đĩa — model — dung lượng — serial — kiểu đĩa — số phân
  vùng), kỹ thuật viên không thấy serial đĩa đích để gõ, gõ
  xong cũng không biết khớp chưa cho tới khi bấm TIẾN HÀNH.
- **Bản sửa 2 (xác nhận serial — cả wipeDisk lẫn rebuild):**
  1. **Dòng định danh đầy đủ** (`lbl_disk_id`, mới): ngay trên
     ô nhập serial, rộng hết trang: "Đĩa đích: Đĩa 1 —
     GIGABYTE GP-GSTFS31120GNTD — 60.4 GB — serial
     SN192808960256". `update_serial_ui()` đổ lại mỗi lần đĩa
     chọn thay đổi (gọi ở đầu `update_disk_parts()` — bao phủ
     mọi đường đổi đĩa/quét lại/đổi chế độ).
  2. **Phản hồi trực tiếp** (`lbl_serial_fb`, mới): nhãn ngay
     cạnh ô nhập; sự kiện `OnTextInput` (EN_CHANGE) gọi
     `update_serial_feedback()` theo từng ký tự: "✓ Đã khớp
     serial đĩa đích" / "✗ Chưa khớp serial đĩa đích" (ô trống
     thì không hiện). So khớp dùng ĐÚNG
     `disk::normalize_disk_serial` như chốt kiểm tra lúc bấm
     TIẾN HÀNH — phản hồi và chốt chặn không lệch nhau; chốt
     chặn cũ (hộp thoại khi bấm nút + `update_start_enabled`)
     giữ nguyên không đổi.
  3. **Ô nhập hiện chữ thật:** `inp_disk_serial` vốn không hề
     đặt ES_PASSWORD (chỉ hai ô mật khẩu có) — ghi rõ thành
     chú thích trong code để không ai "sửa" nhầm về sau.
  4. **Ô chọn đĩa:** nội dung dòng chọn giữ nguyên `summary()`
     (đã có đủ số đĩa + model + dung lượng + serial); thêm
     `CB_SETDROPPEDWIDTH = 780` để danh sách xổ xuống không
     cắt chữ. Bố cục tab 2 dịch xuống 26px từ dòng định danh
     trở đi (trang còn dư chiều cao), đăng ký đủ vào bảng bố
     cục thích ứng (lbl_disk_id: Fixed/Grow; lbl_serial_fb:
     ShiftRight/Fixed) và vào `update_part_ui` (ẩn/hiện cùng
     khối đĩa).
- **Tài liệu:** tab "Tài liệu" mục 1 thêm gạch đầu dòng mô
  tả mount ISO ba lớp; `docs/co_che_v0.7.17.md` = bản sao của
  co_che_v0.7.16.md + thay đổi trên (đã đối chiếu thân tài
  liệu khớp từng chữ với TAI_LIEU_TEXT bằng script);
  README_FULL §"Gắn ISO" viết lại theo cơ chế ba lớp + dòng
  lịch sử v0.7.17; HUONG-DAN.txt có mục v0.7.17 + ca test ưu
  tiên (đọc lại đúng ISO `Win11_25H2_English_x64_v2.iso` trên
  DESKTOP-704R89L → phải liệt kê được edition; hồi quy: ISO
  mod đọc vẫn tốt; kiểm tra vùng xác nhận serial ở cả hai
  chế độ wipe/rebuild).
- Gói: `~/workspace/UtilityRebuild-v0.7.17.zip` (10.045.568
  byte, 16 file, testzip sạch; drop_v0.7.17 = drop_v0.7.16 + wizard
  `CaiDatWinTuDong.exe` build lại 3.065.406 byte, md5
  `868d5d04…` + HUONG-DAN.txt cập nhật — 12 file còn lại,
  gồm engine WinPEInstall.exe, byte-identical với v0.7.16,
  đã đối chứng md5 từng file trong zip khớp bản trong drop).
  Build release sạch (0 lỗi, 0 cảnh báo). **Chưa test trên
  máy thật** — ca test quyết định là ca ưu tiên trong
  HUONG-DAN.

## v0.7.18 — Hiển thị tiến trình trong WinPE: console là bề mặt chính + màn hình toàn màn hình tông xanh (2026-11-18)

- **Bằng chứng thực địa (winpe-20261117-1852.log):** ca chạy
  bị tưởng là "treo chết" thực ra engine chạy KHOẺ suốt buổi:
  bản sửa `ensure_boot_letter` của v0.7.16 thành công (phân
  vùng EFI thiếu ký tự → engine tự gán — S: bận nên lấy T: —
  probe truy cập thật đạt), rồi DISM bung ảnh stream tới
  **82%** thì người vận hành tắt nguồn vì tưởng máy treo.
  Khuyết điểm DUY NHẤT là tính hiển thị: engine không có bề
  mặt tiến trình nào cả — cửa sổ tiến trình không hề xuất
  hiện trong WinPE của ISO mod (thu nhỏ console launcher thì
  desktop trống trơn), còn console của qsu_start — vốn HIỆN
  rõ suốt buổi — lại trống vì engine chỉ ghi tiến trình vào
  file log, không in một chữ nào ra console. Người dùng xác
  nhận chẩn đoán: phải chờ thì được, nhưng màn hình đen khiến
  anh tưởng không có gì đang chạy; và yêu cầu màn hình tiến
  trình trong PE phải giống màn hình mẫu của tool gốc (toàn
  màn hình, có màu, không thể không nhìn thấy).
- **Bản sửa 1 (console = bề mặt tiến trình CHÍNH — module
  mới `progress.rs` trong crate winpe_install):**
  1. `progress::ensure_console()` chạy đầu tiên trong
     `main()`: engine là tiến trình console do qsu_start.cmd
     chạy trực tiếp nên stdout vốn đã bám console của
     launcher (đã kiểm chứng cơ chế: script chạy exe trực
     tiếp, không `start`, không ẩn) — chỉ khi handle stdout
     không hợp lệ mới `AttachConsole(ATTACH_PARENT_PROCESS)`,
     bất đắc dĩ mới `AllocConsole` (mở cửa sổ thứ hai nên là
     lựa chọn cuối). **qsu_start.cmd giữ nguyên từng byte** —
     không cần thay đổi launcher.
  2. Logger của engine nay mang sink: MỌI dòng log đồng thời
     in thẳng ra console (`mirror_line`, có chốt dòng `\r`
     đang mở), và dòng `LỖI...` gần nhất được giữ lại cho
     trạng thái kết thúc. Mỗi giai đoạn in thêm một dòng
     `>> <giai đoạn>` khi bắt đầu.
  3. **Phần trăm bung ảnh trực tiếp:** DISM xả tiến độ bằng
     ký tự `\r` không kèm `\n`, nên hàm mới
     `cmd::run_streaming_progress()` (qsu_common) đọc pipe
     theo từng khúc byte và cắt segment ở CẢ `\r` lẫn `\n` —
     bộ đọc theo dòng cũ (`run_streaming` + `BufRead::lines`)
     giữ toàn bộ % trong buffer tới khi DISM kết thúc.
     Callback của engine trích % (`parse_dism_percent`), cập
     nhật console tại chỗ bằng dòng `\r` "Đang bung ảnh:
     <edition> — NN%" mỗi khi % đổi, đồng thời cập nhật màn
     hình tiến trình; file log chỉ ghi mốc 10% để khỏi ngập.
     `run_streaming` cũ giữ nguyên cho các caller khác
     (mount/unmount/export của wizard, apply của module
     CaiDatWinWinPE).
- **Bản sửa 2 (màn hình tiến trình toàn màn hình = bề mặt
  PHỤ):** cửa sổ WS_POPUP phủ kín màn hình, tự vẽ toàn bộ
  bằng GDI (không phụ thuộc comctl32), mở TRƯỚC mọi công
  việc — trước cả quét stage — trên luồng riêng chỉ đọc
  trạng thái dùng chung; lỗi GUI bị nuốt trong luồng đó,
  không chạm được vào luồng cài đặt hay dòng console. Thiết
  kế theo màn hình mẫu do người dùng cung cấp (màn hình tiến
  trình trong WinPE của tool gốc) nhưng thương hiệu riêng
  của bản rebuild — KHÔNG dùng logo BH, KHÔNG tên tác giả
  tool gốc: nền gradient xanh dương, tiêu đề lớn "CÀI ĐẶT
  WINDOWS TỰ ĐỘNG", dòng cảnh báo "Hệ thống đang được cài
  đặt tự động — Vui lòng KHÔNG tắt máy", credit nhỏ "Utility
  Rebuild"; thẻ giữa chứa bước hiện tại, số % RẤT LỚN, thanh
  tiến trình tự vẽ, và đồng hồ "Thời gian: mm:ss" chạy từng
  giây (vẽ lại theo timer 200ms, chốt theo giây đã trôi) —
  tín hiệu còn sống ngay cả khi DISM lâu không nhả % mới.
  Quy ước %: ngoài bước bung ảnh là tiến độ tổng thể theo
  giai đoạn (mốc 0/2/5/8/15 khởi đầu → 78 driver → 84
  unattend → 88 WinRE → 93 bcdboot → 100 hoàn tất); trong
  bước bung ảnh số % và thanh là % TRỰC TIẾP của DISM. Giao
  diện màu toàn màn hình này CHỈ dành cho engine trong
  WinPE theo yêu cầu riêng của người dùng — wizard phía
  Windows giữ nguyên GUI sáng tiêu chuẩn.
- **Bản sửa 3 (lỗi thì Ở LẠI trên màn hình):** toàn bộ
  `std::process::exit(N)` trong luồng cài được đổi thành
  `return N` về `main()` (luồng chạy trong `run()`; thứ tự
  và logic từng bước không đổi một dòng). Mã khác 0 →
  `main` chốt trạng thái ĐÃ DỪNG (bước đang dừng + lỗi gần
  nhất + đường dẫn log) trên cả màn hình lẫn console rồi
  NEO tiến trình bằng `thread::park()` vô hạn: qsu_start
  không chạy tiếp tới `wpeutil reboot`, máy đứng nguyên
  trong WinPE cho kỹ thuật viên đọc/chụp. Đường thành công
  giữ nguyên: engine tự `wpeutil reboot` vào Windows mới.
- **Tài liệu:** tab "Tài liệu" của wizard thêm mục 11 "Hiển
  thị tiến trình trong WinPE"; `docs/co_che_v0.7.18.md` =
  bản sao của co_che_v0.7.17.md + mục 11 (đã đối chiếu thân
  tài liệu khớp từng chữ với TAI_LIEU_TEXT bằng script);
  README_FULL §3.3 bổ sung đoạn hiển thị tiến trình + dòng
  lịch sử v0.7.18; HUONG-DAN.txt có mục v0.7.18 + ca test
  ưu tiên (lặp lại ca wipe trên DESKTOP-704R89L: từ lúc PE
  lên phải có màn hình tiến trình suốt ca, % bung ảnh phải
  leo trực tiếp, đồng hồ phải chạy từng giây; nếu dừng thì
  chụp màn hình — nó ghi đúng bước đang dừng).
- Gói: `~/workspace/UtilityRebuild-v0.7.18.zip` (10.065.338
  byte, 16 entry, testzip sạch; drop_v0.7.18 = drop_v0.7.17
  + engine `WinPEInstall.exe` build lại (1.742.119 byte,
  md5 `50cfa224…`) + wizard `CaiDatWinTuDong.exe` build lại
  (3.068.081 byte, md5 `4b575860…` — chỉ đổi nội dung tab
  Tài liệu) + HUONG-DAN.txt cập nhật; 11 file còn lại
  byte-identical với v0.7.17, đã đối chứng md5 từng file
  trong zip khớp bản trong drop).
  Build release sạch (0 lỗi, 0 cảnh báo). **Chưa test trên
  máy thật** — ca test quyết định là ca ưu tiên trong
  HUONG-DAN.

## v0.7.19 — Chốt boot cùng đĩa cho đường preserve + module WinPE tự mount ISO / trình duyệt file tích hợp / chống kẹt nút + dọn cờ đổi mật khẩu trong autounattend (2026-11-18)

- **Bằng chứng thực địa (buổi test 2026-11-18, DESKTOP-704R89L,
  WinPE USB BHT/Anhdv):** v0.7.18 chạy ca wipe trọn vẹn (màn hình
  tiến trình hoạt động, bung ảnh xong, boot vào Windows mới) —
  nhưng Windows MỚI (bản mod) xanh màn hình CRITICAL_PROCESS_DIED;
  ca đối chứng bằng ISO gốc qua module `CaiDatWinWinPE` (cài theo
  phân vùng lên C:) phơi ra 4 lỗi của chính module/đường preserve:
  (1) DISM bung ảnh 100% nhưng bước **bcdboot thất bại** với thông
  báo chung chung — kỹ thuật viên phải tự gán ký tự S: cho phân
  vùng EFI của SSD rồi chạy `bcdboot C:\Windows /s S: /f UEFI`
  bằng tay mới boot được ("Boot files successfully created" →
  Windows vào "Getting ready"); (2) chọn file ISO bị ngõ cụt
  "WinPE này không có PowerShell/cmdlet Storage nên không mount
  được ISO"; (3) nút "Chọn file..." không mở được cửa sổ nào (hộp
  thoại file của Windows chết lặng trong WinPE này) và sau một
  lần nạp nguồn lỗi, nút "Nạp nguồn" **kẹt khoá cứng** ("Không
  cho bấm nạp") phải đóng mở lại module; (4) ca ISO gốc cài xong
  thì lần boot đầu dừng ở **màn hình bắt đổi mật khẩu** thay vì
  AutoLogon vào desktop.
- **Bản sửa 1 (chốt boot):** thêm `disk::finalize_boot()` vào
  `qsu_common::disk` và nối vào CẢ HAI đường preserve
  (CaiDatWinWinPE theo phân vùng + engine WinPEInstall
  preserve); hai bản sao `find_efi_letter()` bị xoá. Gốc lỗi:
  hàm cũ chỉ quét các ổ ĐÃ CÓ ký tự tìm thư mục `\EFI` ở gốc rồi
  dự phòng `mountvol /s` — EFI của SSD hôm đó không có ký tự
  (vô hình), mountvol /s không phân biệt đĩa (máy có 2 EFI, mỗi
  đĩa một cái — cũng là rủi ro R1 của rà soát UEFI), đường tự
  gán ký tự `ensure_boot_letter` của v0.7.16 chỉ nối vào
  wipe/rebuild, và lỗi hiện ra là chuỗi cố định không kèm
  output của bcdboot. Helper mới: `volume_disk_number` → liệt
  kê IOCTL của crate → phân vùng boot TRÊN CÙNG ĐĨA (GPT:
  `PartKind::Efi`; MBR: `PartKind::System`, không có thì chính
  phân vùng Windows) → `ensure_boot_letter` (ưu tiên S, loại
  trừ ký tự ổ Windows) → `bcdboot /f UEFI|BIOS` thu trọn
  output → **kiểm chứng file thật** (`EFI\Microsoft\Boot\
  bootmgfw.efi` + `BCD`, hoặc `bootmgr` + `Boot\BCD`) →
  thử lại 1 lần → Err mang theo output của bcdboot vào hộp
  thoại. Wipe/rebuild giữ nguyên đường riêng (đã đúng ở
  thực địa v0.7.18). Dòng `[Cơ chế]` mô tả cơ chế mới ghi ở
  đầu helper.
- **Bản sửa 2 (mount ISO):** module mới `qsu_common::iso` —
  port VirtDisk FFI của lớp 3 wizard (v0.7.17) thành đường
  mount DÙNG CHUNG: sổ đăng ký mount trong tiến trình,
  OpenVirtualDisk + AttachVirtualDisk READ_ONLY|
  PERMANENT_LIFETIME, dò ký tự bằng hiệu tập volume (ưu
  tiên ổ CDROM có `sources\install.*`), gỡ đối xứng bằng
  DetachVirtualDisk khi đổi nguồn/đóng module. Module
  WinPE bỏ hẳn `powershell_available()`/`Mount-DiskImage`;
  lỗi attach trả thông điệp dự phòng rõ ràng (mount sẵn
  bằng công cụ của WinPE rồi chọn/gõ thẳng
  `F:\sources\install.wim` — đường đã cứu ca thực địa).
  Wizard giữ nguyên bộ ba lớp của nó.
- **Bản sửa 3 (chọn file + kẹt nút):** trình duyệt file
  TÍCH HỢP trong CaiDatWinWinPE — cửa sổ Win32 thứ hai của
  module (full_bind_event_handler riêng): danh sách mọi ổ
  đĩa (GetLogicalDriveStringsW + nhãn + loại), duyệt thư
  mục bằng read_dir, lọc *.iso/*.wim/*.esd/*.swm, hiện tên
  file ĐẦY ĐỦ, nhấp đúp/nút Chọn trả đường dẫn nguyên văn
  vào ô nguồn rồi nạp luôn; chế độ thứ hai chọn thư mục
  cho nút "Duyệt..." driver. Cả hai call site
  `nwg::FileDialog` bị gỡ. Ô gõ đường dẫn giữ nguyên.
  Chống kẹt nút: `spawn_load_source`/`spawn_enumerate`
  bọc `catch_unwind` — panic cũng thành kết quả Err,
  `busy` luôn được nhả + notice luôn được gửi; `refresh()`
  trì hoãn mọi hộp thoại modal xuống SAU `set_busy_ui()`
  (và đồng bộ lại sau khi hộp thoại đóng) — nút chỉ khoá
  khi thao tác thật sự đang chạy.
- **Bản sửa 4 (cờ đổi mật khẩu):** `build_unattend()` ở CẢ
  wizard và module WinPE (hai bản sao độc lập — qsu_common
  không có bản chung) nay sinh `<FirstLogonCommands>` với
  hai SynchronousCommand: `net user <Admin> /logonpasswordchg:no`
  (Order 1) và PowerShell `Set-LocalUser -PasswordNeverExpires
  $true` (Order 2) — dọn cờ pwdLastSet=0 mà Setup gắn cho
  tài khoản tạo từ unattend. Áp dụng cả khi mật khẩu trống
  lẫn có đặt. Mỗi nơi sinh đều ghi dòng `[Cơ chế]` (wizard
  ở bước staging 78%, module ở Bước 4 của spawn_install).
  Đây là lệnh dọn cờ cho chính tài khoản vừa tạo — KHÔNG
  phải lệnh cài ứng dụng hậu cài.
- **Tài liệu:** tab "Tài liệu" của wizard thêm mục 12;
  `docs/co_che_v0.7.19.md` = header của bản v0.7.18 + đoạn
  delta v0.7.19 + thân TAI_LIEU_TEXT mới (đã đối chiếu
  khớp từng chữ bằng script); README_FULL §3.4 (công nghệ,
  cơ chế, hàm chính, checklist, trạng thái thực địa), §4.11
  bước 10, §5.5 (R1 ĐÃ ĐÓNG) + dòng lịch sử v0.7.19;
  HUONG-DAN.txt có mục v0.7.19 + 4 ca test ưu tiên (lặp
  lại đúng buổi test: chọn ISO bằng trình duyệt tích hợp
  → tự mount → cài theo phân vùng → bcdboot tự qua → vào
  thẳng desktop không hỏi đổi mật khẩu).
- Gói: `~/workspace/UtilityRebuild-v0.7.19.zip`;
  drop_v0.7.19 = drop_v0.7.18 + 3 exe build lại
  (`CaiDatWinWinPE.exe`, `WinPEInstall.exe`,
  `CaiDatWinTuDong.exe`) + HUONG-DAN.txt cập nhật; 8 file
  còn lại byte-identical với v0.7.18 (nguồn của chúng
  không đổi; build toàn workspace cho md5 khác chỉ vì
  thư viện dùng chung thay đổi — gói giữ binary v0.7.18
  cho các module không đổi theo đúng quy ước các bản
  trước). Build release sạch (0 lỗi, 0 cảnh báo).
  **Chưa test trên máy thật** — ca test quyết định là 4
  ca ưu tiên trong HUONG-DAN.

## v0.7.20 — Chuẩn hoá giao diện toàn bộ: một design system trong qsu_common::ui, icon đủ 32/32 (2026-11-18)

Bản **chỉ đổi lớp giao diện** theo yêu cầu "chuẩn — từ launcher
cho đến module, icon — phù hợp, tối ưu, đẹp": hành vi, luồng
công việc, cổng an toàn, nội dung chữ và cơ chế hậu cài/Telegram
của 114c02e giữ nguyên 100%. Chuẩn đầy đủ + tổng kết rà soát
từng cửa sổ nằm ở `docs/chuan_giao_dien.md`.

- **Design system mới `qsu_common::ui`** (crate qsu_common thêm
  phụ thuộc native-windows-gui): sáu font chuẩn dạng getter
  (Segoe UI 21/700 title, 17/700 section, 15 normal, 14 small,
  15/700 button, Consolas 14 mono — đúng bộ số thực địa từ
  v0.5, nay một nguồn duy nhất thay cho 10 bản sao chép tay);
  token bố cục (MARGIN 16, GAP 8, HEADER_H 72, BUTTON_H 32,
  INPUT_H 28); `add_header()` (tiêu đề (16,8) + phụ đề (16,40)
  font_normal); `set_window_icon()` (icon chung `qsu.ico`
  16/24/32/48 nhúng trong qsu_common — nền bo góc thép xanh +
  cửa sổ 4 ô trắng); `make_primary()` (viền nút mặc định
  BS_DEFPUSHBUTTON cho đúng một nút chính mỗi cửa sổ); hộp
  thoại chuẩn info/error/confirm gom về một nơi.
- **Icon launcher đủ 32/32:** 9 glyph của 114c02e giữ nguyên
  toạ độ; 23 glyph mới vẽ cùng ngôn ngữ (nét 2px, mực thép
  xanh RGB(40,93,145), nền đen-khoá-trong-suốt, 32×32, không
  chữ). Nguồn sinh mới `packaging/draw-icons.py` (Pillow)
  tái sinh cả 32 BMP (đúng định dạng 4150 byte) + qsu.ico;
  `icons.rs` ánh xạ đủ 32 file module.
- **Launcher:** font/header theo chuẩn; lưới nút dùng một
  công thức chung cho lúc dựng và lúc lọc tìm kiếm (trước
  lệch nhau ~9px); ô tìm kiếm cao 28; nút danh mục cao 32;
  icon cửa sổ; hộp thoại lấy từ `ui`.
- **Từng module:** font + header chuẩn + icon cửa sổ ở
  Backup/Restore Driver, Tắt BitLocker, Sao Lưu Wifi, Dọn
  Rác, Quản Lý Update, Phím Tắt BIOS (thêm phụ thuộc
  qsu_common), Tuỳ Chỉnh Windows, module WinPE (cả cửa sổ
  trình duyệt file; nhãn mode dời (16,58)→(16,64) tránh
  chồng phụ đề chuẩn); wizard chỉ gom font + icon (header/
  hình học do engine bố cục riêng sở hữu — ngoại lệ ghi
  trong chuẩn). Ô log kỹ thuật về Consolas ở Dọn Rác (các
  module khác đã mono sẵn). Hàng nút hành động đồng đều
  cao 32 ở Quản Lý Update, Backup Driver, Tuỳ Chỉnh
  Windows. Mỗi cửa sổ một nút chính: Backup, Tắt BitLocker,
  Xuất hồ sơ (Wifi), Dọn rác, Bật cập nhật tự động, Áp dụng
  mục đã tick, BẮT ĐẦU CÀI ĐẶT — tất cả đều đã qua hộp xác
  nhận/phân quyền sẵn có trước khi thực thi.
- **Không đổi:** engine WinPEInstall (không sửa mã nguồn —
  màn tiến trình toàn màn hình v0.7.18 giữ nguyên thiết kế;
  binary build lại vì thư viện chung mở rộng nên md5 khác
  và kích thước tăng do liên kết thêm NWG qua qsu_common),
  mọi logic cài đặt/đĩa/ISO/unattend, tab Tài liệu/Hướng
  dẫn của wizard (đã rà: hai văn bản chỉ mô tả cơ chế/quy
  trình, không mô tả diện mạo wizard — không cần sửa, giữ
  nguyên quy tắc khớp từng chữ với docs/co_che_v0.7.19.md).
- Gói: `~/workspace/UtilityRebuild-v0.7.20.zip`; drop_v0.7.20
  build mới **cả 11 exe** (bản giao diện nên mọi exe đều
  đổi). Build release sạch (0 lỗi, 0 cảnh báo). **Chưa test
  trên máy thật** — ca test là mở launcher + từng module
  kiểm mắt: header/icon/nút chính/ô log đúng chuẩn, lưới
  launcher đủ 32 icon, và chạy thử một luồng quen thuộc
  (ví dụ ca cài theo phân vùng của v0.7.19) để chắc chắn
  hành vi không đổi.

## v0.7.25 (2026-11-20) — Chuẩn hoá entry boot sau bcdboot: sửa lỗi 0xEF của ảnh rút gọn — BRANCH tiny11-core-profile

- **Chẩn đoán khép lại (thực địa 2026-11-20, DESKTOP-704R89L):** ca CORE chạy thật đầu tiên (v0.7.24, log winpe-20261119-2208 ghi imageProfile='core') vẫn 0xEF. Loại trừ tiếp bằng test tại chỗ: xoá unattend khỏi Panther → vẫn 0xEF; đổi type phân vùng Recovery WinRE → basic-data + attributes 0 → vẫn 0xEF; Panther chỉ có setup.etl, setupact/setuperr chưa từng được tạo (chết trước khi Windows Setup ghi log); MEMORY.DMP có tồn tại trên bản lỗi. A/B quyết định: WinNTSetup bung cùng ảnh lên đúng layout tool vừa tạo (boot = EFI 260MB có sẵn, install = phân vùng 110GB có format, unattend/driver trống) → boot qua "Getting ready" vào OOBE, lên desktop (user ITDEMO). Test tách: trên chính bản Windows đang chạy tốt đó, chọn lại entry cũ "Windows 11" do bcdboot của engine tạo → 0xEF ngay. **Thủ phạm: entry boot do bcdboot tạo.** Diff `bcdedit /enum all`: entry lỗi có path `\\Windows\system32\winload.efi` + systemroot `\\Windows` (hai gạch chéo ở đầu — BCD-Template trong ảnh rút gọn đã bị sửa, bcdboot dựng entry theo template này), entry WinNTSetup chuẩn `\Windows\...` + `\Windows`; các phần tử còn lại giống hệt. DISM apply, layout phân vùng, unattend, driver, WinRE — tất cả vô tội.
- **Fix:** hàm dùng chung `qsu_common::disk::normalize_boot_entry(log, boot_letter, uefi)` — đọc entry {default} trong BCD store trên phân vùng boot, log nguyên trạng path/systemroot TRƯỚC khi sửa, rồi `bcdedit /store ... /set {default}` đặt lại path (winload.efi UEFI / winload.exe BIOS) và systemroot `\Windows`; lỗi bcdedit chỉ ghi CẢNH BÁO, không đổi kết quả bcdboot. Gắn ở cả ba điểm: engine đường wipeDisk/rebuild (sau bcdboot trực tiếp), trong `finalize_boot` sau khi kiểm chứng file boot (phủ engine preserve + module theo phân vùng), module đường wipeDisk.
- **Không đổi:** hồ sơ CORE/STANDARD, unattend, layout, cơ chế staging. Banner engine → v0.7.25.
- **Tài liệu:** docs/co_che_v0.7.25.md (mục 17 mới), TAI_LIEU_TEXT đồng bộ từng chữ (kiểm chứng bằng script trích xuất–so sánh), README_FULL bảng phiên bản (+ đính chính dòng v0.7.23 về ca test v0.7.22 theo log).
- Gói: `~/workspace/UtilityRebuild-v0.7.25.zip`; drop_v0.7.25 build mới cả 11 exe. Build release sạch. **Chưa test trên máy thật** — ca test: wizard + Core BẬT (đã tự nhớ từ settings) + WIPE DISK với ảnh Tiny11 Core — kỳ vọng: log engine có hai dòng "TRƯỚC chuẩn hoá" (cho thấy `\\Windows...` nếu template ảnh thật sự sai) + hai dòng "Đã chuẩn hoá entry boot"; máy boot qua OOBE tự động vào desktop. Nếu vẫn 0xEF, gửi lại log engine để đối chiếu.

## v0.7.24 (2026-11-20) — Chống chạy nhầm hồ sơ: nút Core LƯU trạng thái + log ghi rõ HỒ SƠ TRIỂN KHAI — BRANCH tiny11-core-profile

- **Bối cảnh (log thực địa anh gửi 2026-11-20 11:09–11:10):** hai ca test v0.7.21 và v0.7.22 đều chạy nhầm hồ sơ STANDARD — engine log của ca bản 22 (winpe-20261119-2003, giờ máy test) có đủ dấu vết chuẩn (autounattend vào Panther, agent hậu cài, inject driver sao lưu lỗi 0x80004002) và không có dòng CORE nào. Truy vết code: đường dây nút Core → Model → ExecPlan → stage.json → engine ĐỨNG ĐÚNG toàn bộ (đã đọc lại từng đoạn); điểm gãy là con người–giao diện: nút ở tab Tuỳ chọn, mặc định TẮT, không lưu trạng thái, không có dòng xác nhận hồ sơ nào trước khi máy reboot. Phía build cũng đã đọc sai hai ảnh OOBE (của ca cài tay) thành bằng chứng boot — đã sửa trong ghi chú v0.7.22/v0.7.23 ở trên.
- **Wizard:** Settings thêm khoá `defaultCoreProfile` (qsu_common::settings, mặc định false, settings.example.json đã bổ sung); Model.core_profile khởi tạo từ khoá này, nút Core hiện đúng BẬT/TẮT lúc mở; handler của nút gọi `persist_core_default()` — sửa settings.json tại đúng vị trí khoá (giữ nguyên các khoá/định dạng khác, chưa có khoá thì chèn sau `{` đầu tiên, chưa có file thì tạo tối thiểu; lỗi ghi thì bỏ qua). Đầu ca staging (exec_clean) log dòng "[Cơ chế] HỒ SƠ TRIỂN KHAI: CORE/STANDARD ..." — bản STANDARD còn kèm câu cảnh báo nếu đang cài ảnh rút gọn.
- **Engine + module:** engine log dòng HỒ SƠ TRIỂN KHAI kèm giá trị imageProfile đọc từ stage.json ngay khi tính core_profile; module WinPE log dòng tương tự ngay đầu worker. Từ nay ca nào chạy hồ sơ nào, mở log là thấy ngay.
- **Tài liệu:** docs/co_che_v0.7.24.md (mục 16 mới), TAI_LIEU_TEXT đồng bộ từng chữ, README_FULL bảng phiên bản, banner engine → v0.7.24. Nội dung hồ sơ CORE (mục 15) và layout (mục 5) không đổi so với v0.7.23.
- Gói: `~/workspace/UtilityRebuild-v0.7.24.zip`; drop_v0.7.24 build mới cả 11 exe. Build release sạch (0 lỗi, 0 cảnh báo). **Ca test quyết định:** mở wizard → tab Tuỳ chọn → kiểm tra nút Core (bật BẬT nếu chưa; từ nay nó tự nhớ) → cài WIPE DISK như thường → kỳ vọng log staging có dòng HỒ SƠ TRIỂN KHAI: CORE, máy qua 0xEF và tự vào desktop bằng tài khoản đã đặt; gửi lại cả hai file log (install-*.log + winpe-*.log) để đối chiếu.

## v0.7.23 (2026-11-20) — Hồ sơ CORE lấy lại tự động hoá: unattend CHỈ pha oobeSystem — BRANCH tiny11-core-profile

- **Bối cảnh (thực địa 2026-11-20, ĐÃ SỬA THEO LOG):** lúc build bản này, hai ảnh OOBE ("Just a moment…", màn hình nhập tên) được hiểu là ca v0.7.22 boot qua 0xEF; log anh gửi lúc 11:09 chứng minh điều ngược lại — ca bản 22 chạy hồ sơ STANDARD (nút Core chưa BẬT) và vẫn 0xEF, hai ảnh OOBE là của ca cài tay WinNTSetup sau đó. Thiết kế của v0.7.23 (hồ sơ CORE của v0.7.22 + đúng một biến số: unattend chỉ-pha-OOBE để giữ tiêu chí "cài auto" anh chốt) vẫn giữ nguyên, nhưng hồ sơ CORE tới thời điểm này CHƯA được test thật lần nào; v0.7.24 lo phần chống chạy nhầm hồ sơ trước khi test tiếp.
- **build_unattend (cả hai bản sao, đồng bộ):** pha specialize tách thành `spec_block`; hồ sơ CORE → spec_block rỗng (unattend chỉ còn oobeSystem, vốn đã không có FirstLogonCommands ở CORE). Đầu ra của hồ sơ STANDARD giữ nguyên từng byte (spec_block chép nguyên văn khối specialize cũ).
- **Wizard:** CORE sinh lại autounattend.xml vào staging (nội dung chỉ-OOBE) + dòng [Cơ chế] v0.7.23. **Engine:** CORE chép autounattend của staging vào Panther (cả hai tên file như hồ sơ chuẩn) nhưng vẫn không agent/SetupComplete/driver/WinRE/PostInstall; staging v0.7.22 (không có unattend) vẫn tương thích — engine log rõ và cài thuần WinNTSetup. **Module:** tick CORE tự sinh unattend chỉ-OOBE từ các ô tài khoản (không cần tick "Tự thiết lập Windows"); Bước 4 CORE ghi xml vào Panther, không agent/SetupComplete; hộp xác nhận hiện lại dòng tài khoản cho CORE và nói đúng nội dung hồ sơ.
- **Đánh đổi (đã ghi trong tài liệu):** không đặt tên máy/múi giờ (chỉ ở specialize); có thể gặp màn hình đổi mật khẩu ở lần đăng nhập đầu như ca v0.7.18 vì không còn lệnh dọn cờ — nếu ca test vướng đúng điểm này, bước kế tiếp là thêm lại riêng SetupComplete.cmd dọn cờ (biến số duy nhất tiếp theo).
- **Tài liệu:** docs/co_che_v0.7.23.md (mục 15 thay mục 14), TAI_LIEU_TEXT đồng bộ từng chữ, README_FULL §4.9 + bảng phiên bản, banner engine → v0.7.23.
- Gói: `~/workspace/UtilityRebuild-v0.7.23.zip`; drop_v0.7.23 build mới cả 11 exe. Build release sạch (0 lỗi, 0 cảnh báo). **Chưa test trên máy thật** — ca test: wizard + nút Core BẬT + WIPE DISK — kỳ vọng: qua 0xEF VÀ tự chạy thẳng qua OOBE vào desktop bằng tài khoản đã đặt (không phải nhập tên); nếu dừng ở màn hình đổi mật khẩu thì để trống ba ô + Enter và báo lại.

## v0.7.22 (2026-11-20) — Định nghĩa lại hồ sơ CORE (bản sao công thức WinNTSetup) + layout chia đĩa theo công thức tay — BRANCH tiny11-core-profile

- **Bối cảnh (thực địa 2026-11-20):** v0.7.21 test trên DESKTOP-704R89L — VẪN CRITICAL_PROCESS_DIED (0xEF) ở lần boot đầu. Anh cung cấp đủ công thức cài tay chạy được: MiniTool chia EFI 259MB / MSR 500MB / Recovery 1024MB (trước C:) / C: 'WIN'; WinNTSetup 5.4.1 bung install.esd (index 1 'Dev - Windows 11 Pro', build 26300.9457) — Unattend trống, Add Drivers trống, không tweaks. Log engine ca 2026-11-19 (winpe-20261119-1909, hồ sơ chuẩn) cho thêm hai sự thật: DISM /Add-Driver của QSU-DriverBackup lỗi ngay 0x80004002 'No such interface supported' (không driver nào được nạp, engine nuốt lỗi bằng `let _ =`), và chép Winre.wim thất bại 'not enough space on the disk' (os error 112) vì Recovery 600MB quá chật. Log của ca v0.7.21 không lấy được (máy đã cài lại tay) — quyết định làm theo hướng loại trừ toàn bộ: hồ sơ CORE = bản sao nguyên công thức tay.
- **Hồ sơ CORE v0.7.22:** wizard KHÔNG sinh autounattend.xml khi Core (trước vẫn sinh bản rút gọn); engine + module: Core → không chép unattend vào Panther, không SetupComplete (bỏ cơ chế v0.7.21), không agent hậu cài, bỏ qua cả PostInstall.exe trong staging, không inject driver (staging + backup — commit 4bdcf87), không chép WinRE/reagentc. Module: ô tick CORE đọc độc lập với ô unattend (v0.7.21 phải tick unattend mới có Core); hộp xác nhận nói đúng trạng thái thật (has_unattend = unattend && !core; driver hiện 'Không (hồ sơ CORE bỏ qua)'; dòng Recovery ghi '(để trống — hồ sơ CORE)' khi Core).
- **Layout (mọi hồ sơ):** `disk::wipe_layout`/`rebuild_layout` GPT: MSR 16→500MB, Recovery 600→1024MB; script GPT (wipe + rebuild) tạo Recovery TRƯỚC phân vùng Windows (rebuild ghim offset tương ứng: EFI→MSR→Recovery→Windows); MBR giữ thứ tự, Recovery 600→1024MB. Ngưỡng vùng rebuild tối thiểu đổi 876/700 → 1784/1124MB (cả disk.rs lẫn wizard). Mọi dòng mô tả layout trong wizard/module ([Cơ chế], hộp xác nhận wipe/rebuild, lỗi vùng nhỏ) cập nhật theo.
- **Tài liệu:** docs/co_che_v0.7.22.md (mục 14 thay mục 13 + đoạn WIPE DISK mới), TAI_LIEU_TEXT đồng bộ từng chữ (script trích xuất–so sánh), README_FULL §4.9 + hằng số layout + bảng phiên bản, banner engine → v0.7.22.
- Gói: `~/workspace/UtilityRebuild-v0.7.22.zip`; drop_v0.7.22 build mới cả 11 exe. Build release sạch (0 lỗi, 0 cảnh báo). **KẾT QUẢ TEST THỰC ĐỊA 2026-11-20 — SỬA LẠI THEO LOG (anh gửi 11:09): ca chạy ĐI HỒ SƠ STANDARD, KHÔNG phải CORE.** Engine log winpe-20261119-2003 (giờ máy) cho thấy: chép autounattend vào Panther (dòng 120), chuẩn bị agent hậu cài UltraViewer/Chrome/7-Zip (dòng 119), thử inject driver từ C:\QSU-DriverBackup (dòng 111–118) — toàn dấu vết hồ sơ chuẩn, không một dòng [Cơ chế] CORE. Nút Core chưa BẬT trong ca đó (mặc định TẮT, không lưu trạng thái) → máy vẫn 0xEF như mọi ca chuẩn. Kết luận "boot qua 0xEF" rút ra vội từ hai ảnh OOBE là SAI — hai ảnh đó là của ca cài tay WinNTSetup anh làm sau khi ca tool lỗi. Như vậy hồ sơ CORE (cả v0.7.21 lẫn v0.7.22) CHƯA TỪNG chạy thật trên máy test; v0.7.24 xử lý gốc rễ: nút Core lưu trạng thái + log ghi rõ hồ sơ ở cả ba tầng.

## v0.7.21 (2026-11-20) — Hồ sơ CORE cho ảnh rút gọn (Tiny11 Core) — BRANCH tiny11-core-profile

- **Bối cảnh (thực địa 2026-11-20):** ảnh Windows rút gọn kiểu Tiny11 Core (chế từ ISO gốc Microsoft) cài bằng tool tự boot vòng CRITICAL_PROCESS_DIED (0xEF) ở lần khởi động đầu; cài tay bằng MiniTool Partition Wizard + WinNTSetup (EFI/MSR/Recovery/C + bung ảnh + bcdboot) thì lên bình thường, rất nhanh. Phần khác biệt chính của pipeline chuẩn so với đường cài tối thiểu: unattend đầy đủ (FirstLogonCommands) + bước chuẩn bị agent hậu cài trong bản Windows mới. Bản này thêm hồ sơ triển khai tối thiểu để kiểm chứng giả thuyết đó trên máy test. Toàn bộ thay đổi nằm trên branch `tiny11-core-profile` tạo từ đúng commit v0.7.20 (1051878) và đã push; `main` không đổi — đường lui an toàn.
- **Wizard:** nút "Ảnh rút gọn (Tiny11 Core): TẮT/BẬT" ở tab Tuỳ chọn cài đặt (dưới nút Tuỳ chỉnh Windows sau cài), trạng thái ở Model.core_profile → ExecPlan → stage.json `imageProfile` ("core"/"standard"; StageConfig thêm trường, stage.json cũ thiếu trường hiểu là standard).
- **Module WinPE:** ô tick "Ảnh rút gọn (Tiny11 Core)..." ở cụm cài đặt (chỉ có nghĩa khi tick Tự thiết lập Windows); trạng thái vào InstallPlan.core_profile; hộp xác nhận cuối ghi rõ hồ sơ CORE không tự cài ứng dụng sau đăng nhập.
- **build_unattend (cả hai bản sao, đồng bộ):** thêm tham số `core_profile`. Hồ sơ chuẩn: khối `<FirstLogonCommands>` cho ra XML giữ nguyên từng byte như v0.7.20 (chỉ tái cấu trúc cách dựng chuỗi trong mã nguồn). Hồ sơ CORE: unattend không có khối FirstLogonCommands; locale/tên máy/OOBE/LocalAccount/AutoLogon giữ nguyên.
- **Thực thi sau bung ảnh:** engine WinPEInstall đọc `imageProfile` từ stage.json; module WinPE theo InstallPlan. Hồ sơ CORE: bỏ qua `post_install::write_to_windows` + chép cấu hình Telegram, thay bằng ghi `Windows\Setup\Scripts\SetupComplete.cmd` = một lệnh `net user <admin> /logonpasswordchg:no` (Windows chạy bằng SYSTEM ở cuối pha setup, TRƯỚC lần đăng nhập đầu — cờ "phải đổi mật khẩu" vẫn được dọn, AutoLogon vào thẳng desktop); ảnh có sẵn SetupComplete.cmd thì nối thêm dòng, không ghi đè. Mã nguồn `qsu_common::post_install` không sửa; hồ sơ chuẩn chạy y như cũ.
- **Đánh đổi hồ sơ CORE (đã ghi trong tài liệu + log):** không đặt PasswordNeverExpires, không tự cài ứng dụng sau đăng nhập.
- **Tài liệu:** docs/co_che_v0.7.21.md (mục 13 mới), TAI_LIEU_TEXT của wizard đồng bộ từng chữ (kiểm chứng bằng script trích xuất–so sánh), README_FULL §4.9 + bảng phiên bản, banner engine → v0.7.21.
- Gói: `~/workspace/UtilityRebuild-v0.7.21.zip`; drop_v0.7.21 build mới **cả 11 exe** (qsu_common đổi nên mọi exe liên kết lại). Build release sạch (0 lỗi, 0 cảnh báo). **KẾT QUẢ TEST THỰC ĐỊA 2026-11-20: VẪN 0xEF ở lần boot đầu** (chưa xác nhận được nút Core có bật trong ca đó không — log ca test không lấy lại được vì máy đã cài lại tay). Hồ sơ CORE kiểu unattend rút gọn bị thay thế ở v0.7.22.

## Khác biệt có chủ đích so với bản gốc

- Không cổng key bản quyền, không biến môi trường "token" khóa module.
- Không telemetry, không tự cài root CA, không ngoại lệ Defender,
  không tự cập nhật từ GitHub của tác giả.
- Launcher dùng cửa sổ Windows Forms chuẩn (có viền/title bar) thay cho
  cửa sổ không viền vẽ tay của bản gốc; lưới nút, nhãn và thứ tự giữ nguyên.
- Module driver bỏ liên kết credit ở footer của bản gốc.
