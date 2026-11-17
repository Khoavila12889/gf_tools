# Kế hoạch nâng cấp module theo repo mã nguồn mở "hạng sang"

> Tài liệu khảo sát & kế hoạch áp dụng — chỉ là tài liệu, không sửa code.
> Ngày khảo sát: 2026-11-16. Mọi repo dưới đây đều đã được kiểm chứng tồn tại,
> license và mức độ hoạt động qua trang GitHub/trang chủ chính thức tại thời
> điểm khảo sát. Số sao là con số xấp xỉ tại thời điểm khảo sát.
>
> Phạm vi: 31 module còn lại + launcher. Module Cài Đặt Windows (số 6, 7) đã
> có tài liệu riêng (`docs/tich_hop_chuan_cai_win.md` — OSDCloud, Schneegans
> unattend-generator, WinNTSetup, Dism++) nên không khảo sát lại.

## 0. Nguyên tắc bản quyền áp dụng cho toàn dự án

Bản rebuild là phần mềm Rust đóng gói dạng exe độc lập. Quy tắc cứng:

| Nhóm license | Repo tiêu biểu | Được làm gì |
|---|---|---|
| **Permissive** (MIT, Apache-2.0, BSD, Unlicense) | Sophia Script, winutil, BCUninstaller, UniGetUI, restic, kopia, fastfetch, CrystalDiskInfo, WinSparkle, PSWindowsUpdate, UUPMediaCreator | Được tái sử dụng ở mức code/dữ liệu khi cần, kèm ghi công (attribution) và giữ lại file LICENSE. Thực tế dự án viết bằng Rust còn các repo này là PowerShell/C#/C++ nên phần "tái sử dụng" chủ yếu là **danh mục dữ liệu** (tweak catalogue, cleaner definitions, field model) được dịch lại sang định dạng của ta. |
| **Copyleft mạnh** (GPL-2.0, GPL-3.0, AGPL) | DriverStore Explorer, BleachBit, WuMgr, SDIO, ContextMenuManager, Sandboxie, Rescuezilla/Clonezilla, Optimizer, Fido | **Chỉ tham khảo hành vi** (behavior reference): đọc để hiểu tính năng, danh mục thao tác, cách tổ chức UI — rồi tự viết lại (clean-room). Không copy code, không dịch code, **không đóng gói kèm binary** của chúng trong bản phân phối của ta. |
| **Copyleft mức file** (MPL-2.0) | LibreHardwareMonitor | File nào lấy về thì file đó phải giữ MPL-2.0. Khuyến nghị: chỉ tham khảo danh mục cảm biến, tự viết bằng WMI/IOCTL. |
| **LGPL** (một phần) | wimlib (thư viện chọn được LGPL-3.0+, công cụ CLI là GPL-3.0+) | Về lý thuyết link động thư viện LGPL thì được, nhưng dự án đã có DISM tích sẵn của Microsoft — không cần wimlib, chỉ tham khảo ngữ nghĩa WIM. |
| **License không rõ ràng** | ConfigureDefender (repo không khai SPDX rõ; dự án mẹ Hard_Configurator là GPL), Office Tool Plus (repo GitHub chủ yếu là bản địa hoá/tài liệu; bản thân ứng dụng là freeware đóng gói sẵn) | Coi như GPL/freeware: chỉ tham khảo hành vi và danh mục thiết lập. |
| **Không phải mã nguồn mở** | Microsoft ADK (WinPE), Office Deployment Tool, gói ADMX/Office policy templates, Intel RST driver | Đây là **tài nguyên chính hãng của hãng** — tải từ Microsoft/Intel, dùng theo điều khoản của hãng. Không phân phối lại binary của Microsoft trong gói của ta; để công cụ tự tải hoặc yêu cầu người dùng có sẵn. |

Hai lưu ý thực tế:

1. **Dữ kiện kỹ thuật không phải tác phẩm sáng tạo**: đường dẫn registry, tên
   service, mã lỗi, thông số SMART, phím tắt BIOS của từng hãng… là dữ kiện.
   Ta được biên soạn danh mục riêng dựa trên các dữ kiện đó (và tài liệu hãng),
   nhưng **không sao chép nguyên văn** phần mô tả/diễn giải dài của repo GPL.
2. Mọi module đều phải ghi rõ nguồn tham khảo trong file `NOTICE`/credit nội
   bộ của dự án, kể cả khi chỉ tham khảo hành vi — để sau này truy vết được
   quyết định thiết kế.

## 1. Bảng tổng hợp nhanh

| # | Module | Repo tham chiếu chính | License | Mức áp dụng |
|---|---|---|---|---|
| – | Launcher (tự cập nhật) | WinSparkle | MIT | Dùng thư viện (FFI) |
| 0 | Kho Driver IRST | Intel RST chính hãng + SDIO (khái niệm kho pack) | – / GPL-3.0 | Nguồn driver hãng; SDIO chỉ tham khảo khái niệm |
| 1 | Kho Windows | Fido (phương thức lấy link ISO) + UUPMediaCreator | GPL-3.0 / MIT | Tham khảo phương thức; danh mục tự quản |
| 2 | Backup License MS | — | — | Clean-room (tokens.dat) |
| 3 | Backup/Restore Driver | DriverStore Explorer (RAPR) + SDIO | GPL-2.0 / GPL-3.0 | Tham khảo hành vi (đã build bằng pnputil) |
| 4 | Tắt Bitlocker | — | — | Clean-room (manage-bde) |
| 5 | Tạo Boot WinPE | PhoenixPE + PEBakery (+ ADK của Microsoft) | MIT* / – | Tham khảo quy trình build; payload tự build từ ADK |
| 8 | Cài Đặt Office Online | Office Deployment Tool (Microsoft) + Office Tool Plus | – / freeware | Hành vi + catalogue product ID |
| 9 | Cài Đặt Office OFFLINE | Office Deployment Tool (chế độ offline) | – | Wrapper mỏng, clean-room |
| 10 | Kích Hoạt Win/Office | Tài liệu Microsoft (SPP/WMI) | – | Chỉ kích hoạt hợp lệ — chủ đích không dùng repo kích hoạt |
| 11 | Cài Đặt Phần Mềm | winget (microsoft/winget-cli) + UniGetUI | MIT / MIT | Engine winget; UX theo UniGetUI |
| 12 | Sao Lưu Dữ Liệu | restic / kopia (thiết kế) | BSD-2 / Apache-2.0 | Áp dụng thiết kế; engine robocopy |
| 13 | Sao Lưu Wifi | — | — | Clean-room (netsh wlan) |
| 14 | Sao Lưu Chrome-Edge | — | — | Clean-room (copy profile + lưu ý DPAPI) |
| 15 | Backup/Restore Zalo | — | — | Clean-room (copy ZaloData) |
| 16 | Check Cấu Hình | fastfetch + LibreHardwareMonitor | MIT / MPL-2.0 | Mô hình trường dữ liệu + danh mục cảm biến |
| 17 | Check / Test | CrystalDiskInfo + LibreHardwareMonitor | MIT / MPL-2.0 | Mô hình diễn giải SMART/sức khoẻ ổ |
| 18 | Phím Tắt BIOS | — | — | Dữ liệu tĩnh tự biên soạn |
| 19 | Phân Vùng Ổ Cứng | — (GParted là Linux, không dùng được) | — | Clean-room (MSFT Storage WMI) |
| 20 | Tuỳ Chỉnh Windows | **Sophia Script for Windows** + winutil | MIT / MIT | **Danh mục tweak — áp dụng ở mức dữ liệu** |
| 21 | Tuỳ Chỉnh Office | ADMX/Office policy của Microsoft + Office Tool Plus | – / freeware | Danh mục policy chính hãng |
| 22 | Windows Security | ConfigureDefender | không rõ (coi như GPL) | Danh mục thiết lập Defender — tham khảo hành vi |
| 23 | Quản Lý Update Windows | PSWindowsUpdate + WuMgr | MIT / GPL-3.0 (đã ngừng) | Ngữ nghĩa thao tác; engine WUA COM tự viết |
| 24 | Dọn Rác Máy Tính | BleachBit (CleanerML) | GPL-3.0 | Tham khảo danh mục vị trí rác; tự viết định nghĩa |
| 25 | Gỡ Bỏ Phần Mềm | **BCUninstaller** | Apache-2.0 | **Logic gỡ + quét leftover — áp dụng sâu** |
| 26 | Fix Lỗi Máy In | — | — | Clean-room (danh mục fix tự biên soạn) |
| 27 | Quản Lý Máy In | — | — | Clean-room (Win32_Printer/printui) |
| 28 | Fix Share Mạng LAN | — | — | Clean-room (danh mục service/registry chuẩn) |
| 29 | Quản lý File/Folder | ContextMenuManager | GPL-3.0 | Tham khảo danh mục context menu |
| 30 | Chuyển Windows OS | wimlib + Rescuezilla/Clonezilla | GPL/LGPL / GPL | Tham khảo ngữ nghĩa + UX; pipeline DISM tự viết |
| 31 | Multi Zalo PC | Sandboxie Plus (tuỳ chọn tích hợp ngoài) | GPL-3.0 + license riêng bản Plus | Chỉ gọi tiến trình ngoài, không nhúng code |

\* PhoenixPE: script của dự án là MIT, nhưng repo tổng hợp nhiều công cụ bên
thứ ba với license riêng — xem chi tiết ở mục module 5.

## 2. Chi tiết theo module

### Launcher — tự cập nhật an toàn

- **Repo chính: WinSparkle** — https://github.com/vslavik/winsparkle — **MIT**.
  Thư viện cập nhật kiểu Sparkle cho Windows: một DLL C thuần, đọc appcast XML
  có **ký số Ed25519**, hiển thị hộp thoại cập nhật chuẩn. Vẫn bảo trì đều
  (nhánh 0.9.x phát hành trong 2026).
- **Áp dụng:** thay cơ chế tự cập nhật kiểu cũ của bản gốc (tải file version
  text không ký từ GitHub của tác giả) bằng appcast **ký Ed25519 trên hạ tầng
  của ta**; link thư viện qua FFI từ Rust (MIT cho phép, giữ attribution).
- **Không copy:** không có gì bị cấm — đây là một trong số ít thành phần được
  dùng ở mức thư viện thật.

### Module 0 — Kho Driver IRST

- **Nguồn chính: driver Intel RST/VMD chính hãng** (Intel Download Center) —
  đây là dữ liệu của hãng, không phải repo. Module gốc gần như chắc chắn trỏ
  về kho pack của tác giả; bản rebuild **phải** thay bằng nguồn Intel chính
  thức (đúng tinh thần đã ghi trong BUILD_NOTES).
- **Repo tham khảo: Snappy Driver Installer Origin (SDIO)** —
  https://www.snappy-driver-installer.org (source trên SourceForge) —
  **GPL-3.0**, bản 2.0.6, còn cập nhật pack hàng tuần. SDIO chứng minh mô hình
  "kho pack theo chỉ mục": tải file chỉ mục nhỏ, đối chiếu HWID, chỉ tải pack
  chứa driver cần thiết thay vì cả kho ~70 GB.
- **Áp dụng:** mô hình catalogue theo chỉ mục + đối chiếu HWID cho kho driver
  của ta (bắt đầu với pack storage IRST/NVMe — thứ bắt buộc phải có khi cài
  Win trên laptop Intel đời mới); chữ ký số của driver Intel được kiểm tra
  trước khi cho dùng.
- **Không copy:** code SDIO (GPL-3.0); và lưu ý bản thân các driver pack của
  SDIO là bản đóng gói lại của bên thứ ba (nguồn Windows Update Catalog /
  DriverPacks) — môi trường công ty nên ưu tiên driver chính hãng Intel/hãng
  laptop, SDIO chỉ là phương án dự phòng tham khảo.

### Module 1 — Kho Windows

- **Repo tham khảo 1: Fido** — https://github.com/pbatard/Fido — **GPL-3.0**,
  của tác giả Rufus. Fido là script nhỏ chứng minh cách **truy vấn API tải
  ISO bán lẻ chính thức của Microsoft** để lấy link ISO retail thật (theo
  phiên bản/ngôn ngữ/kiến trúc), link có hạn dùng.
- **Repo tham khảo 2: UUPMediaCreator** — https://github.com/gus33000/UUPMediaCreator
  — **MIT**, C#. Tạo ISO/WIM/VHD từ file UUP (Unified Update Platform) của
  chính Microsoft: quét build, tải payload UUP, dựng ISO. Hoạt động chậm lại
  từ 2024 (lần push cuối 2024-04, dự án đã chuyển sang org OSTooling) nhưng
  thiết kế và giao thức vẫn là tài liệu tham chiếu tốt nhất cho đường UUP.
- **Áp dụng:** (a) phương thức lấy link ISO chính hãng kiểu Fido cho các bản
  retail phổ thông; (b) đường UUP cho các build mới/chưa có ISO retail; (c)
  quan trọng nhất: **danh mục ISO là file JSON trên hạ tầng của ta** (đúng
  thiết kế đã chốt ở đặc tả cài Win: URL + SHA-256 bắt buộc, tải tiếp dở dang
  được) — thay hoàn toàn kho link của tác giả.
- **Không copy:** code Fido (GPL-3.0) — chỉ học phương thức truy vấn API công
  khai của Microsoft rồi tự viết. UUPMediaCreator là MIT nên về license được
  dùng sâu hơn, nhưng là C# nên thực tế vẫn là tham khảo giao thức.

### Module 3 — Backup/Restore Driver (đã build — hướng nâng cấp)

Module này đã build xong từ source dịch ngược (pnputil + WMI). Hai tham chiếu
để nâng cấp:

- **DriverStore Explorer (RAPR)** — https://github.com/lostindark/DriverStoreExplorer
  — **GPL-2.0**, ≈12k sao, còn cập nhật đều (có bản phát hành 2026). Đây là
  công cụ chuẩn mực cho việc quản lý Driver Store: liệt kê driver theo INF,
  xoá driver cũ/trùng (dọn Driver Store phình to), export hàng loạt, phân biệt
  driver có thiết bị đang dùng và driver "mồ côi".
- **SDIO** (xem module 0) cho khái niệm đối chiếu HWID khi restore lên máy khác.
- **Áp dụng (hành vi):** bổ sung cho module driver của ta: (1) chế độ xem
  Driver Store kèm dung lượng từng gói và cờ "đang dùng/mồ côi"; (2) xoá gói
  driver cũ an toàn (pnputil /delete-driver, có xác nhận); (3) khi restore,
  cảnh báo driver không khớp HWID nào trên máy đích.
- **Không copy:** toàn bộ code RAPR/SDIO (GPL) — mọi thao tác đều quy về
  `pnputil` và WMI `Win32_PnPSignedDriver` mà ta đã tự viết.

### Module 5 — Tạo Boot WinPE

- **Repo tham khảo: PhoenixPE** — https://github.com/PhoenixPE/PhoenixPE —
  script dự án **MIT**; bản phát hành mới nhất 2026-10-25, còn phát triển tích
  cực. PhoenixPE là dự án PEBakery dựng WinPE cứu hộ từ nguồn Win10/Win11
  (boot.wim hoặc WinRE.wim), boot được từ USB/CD/PXE/ổ cứng. Engine
  **PEBakery** (https://github.com/pebakery/pebakery) là bộ máy script mã
  nguồn mở thay thế WinBuilder đã chết.
- **Áp dụng:** (a) checklist thành phần của một WinPE cứu hộ hoàn chỉnh (trình
  quản lý file, công cụ đĩa, mạng, trình duyệt nhẹ…) để định hướng WinPE của
  ta; (b) quy trình build: nguồn Windows của người dùng + **Windows ADK của
  Microsoft** (copype) — ta đã có sẵn engine `WinPEInstall.exe`, nên WinPE của
  ta chỉ cần tối thiểu: boot.wim từ ADK/WinRE + engine + driver storage/mạng;
  (c) cách PhoenixPE tách "project script" khỏi "ứng dụng đóng gói kèm" là
  mẫu tổ chức tốt.
- **Không copy:** repo PhoenixPE ở mức tổng thể có license hỗn hợp (chứa nhiều
  công cụ bên thứ ba, license "other" theo GitHub API) — không bê nguyên gói;
  và tuyệt đối **không phân phối WinPE dựng sẵn chứa binary của Microsoft**
  (điều khoản ADK/Windows không cho) — công cụ phải tự build trên máy người
  dùng từ nguồn Windows/ADK của họ, đúng như PhoenixPE làm.

### Module 8 + 9 — Cài Đặt Office Online / OFFLINE

- **Nền tảng chính hãng: Office Deployment Tool (ODT)** của Microsoft —
  `setup.exe /download` và `/configure configuration.xml`. Toàn bộ sức mạnh
  của hai module này nằm ở file `configuration.xml` (product ID, ngôn ngữ
  vi-vn, kênh cập nhật, loại trừ app con). Tài liệu cấu hình chuẩn là Office
  Customization Tool (config.office.com) và bộ ADMX của Microsoft.
- **Repo tham khảo hành vi: Office Tool Plus** —
  https://github.com/YerongAI/Office-Tool (YerongAI) — công cụ triển khai
  Office phổ biến nhất trong giới kỹ thuật: sinh cấu hình triển khai trực
  quan, quản lý license/key, gỡ Office sạch. Lưu ý: repo GitHub chủ yếu chứa
  bản địa hoá/tài liệu; bản thân ứng dụng là freeware đóng gói — **chỉ tham
  khảo hành vi và cách tổ chức tuỳ chọn**, không có code để tái sử dụng.
- **Áp dụng:** (a) catalogue product ID + preset cấu hình (Office 2021/2024
  LTSC, Microsoft 365 Apps, chỉ Word/Excel/PowerPoint…) viết thành JSON của
  ta; (b) module Online = sinh XML → ODT tải từ CDN Microsoft → chạy
  `/configure` và theo dõi tiến độ; (c) module OFFLINE = cùng engine, nguồn
  là thư mục/ISO local đã tải sẵn bằng `/download` từ trước — hai module dùng
  chung một lõi, khác nguồn media.
- **Không copy:** không nhúng binary Office Tool Plus; không dùng các tính
  năng quản lý KMS của nó (xem module 10).

### Module 10 — Kích Hoạt Win/Office (giới hạn chủ đích — giữ nguyên quyết định cũ)

- **Không áp dụng repo kích hoạt nào.** Các repo "kích hoạt" phổ biến trên
  GitHub (dòng KMS emulator, HWID giả lập…) đều là công cụ lách bản quyền —
  nằm ngoài phạm vi dự án theo quyết định đã chốt: module này chỉ làm
  **xem trạng thái bản quyền + nhập key chính hãng + kích hoạt trực tuyến
  chính thức**.
- **Tham chiếu duy nhất:** tài liệu Microsoft về Software Protection Platform:
  WMI class `SoftwareLicensingService`/`SoftwareLicensingProduct` (đọc trạng
  thái, kênh, hạn grace), `slmgr.vbs /dli /xpr`, và với Office là `ospp.vbs`
  / SPP API. Tất cả là API chính hãng, tự viết clean-room.

### Module 21 — Tuỳ Chỉnh Office

- **Tham chiếu chính: bộ policy ADMX của Microsoft cho Office** (Office
  Administrative Template files) + tài liệu Office policies — đây là nguồn
  chuẩn và đầy đủ nhất cho mọi thiết lập dạng policy
  (`HKCU\Software\Policies\Microsoft\Office\<ver>\...`): tắt telemetry, tắt
  trải nghiệm kết nối, định dạng lưu mặc định, kênh cập nhật, macro security…
- **Tham khảo phụ:** Office Tool Plus (mục 8) ở cách gom nhóm tuỳ chọn theo
  ứng dụng Word/Excel/PowerPoint cho dễ dùng.
- **Áp dụng:** biên soạn danh mục tweak Office của ta trực tiếp từ ADMX chính
  hãng (tên giá trị, kiểu dữ liệu, giá trị mặc định, cách hoàn tác) — dữ kiện
  policy là của Microsoft, phần diễn giải tiếng Việt tự viết.
- **Không copy:** không có repo GPL nào cần đụng tới ở module này.

### Module 11 — Cài Đặt Phần Mềm

- **Engine chính: winget** — https://github.com/microsoft/winget-cli —
  **MIT**, ≈26k sao, do Microsoft phát triển, **có sẵn trong Windows 11 và
  Windows 10 bản mới** (App Installer). Kho manifest cộng đồng
  microsoft/winget-pkgs có sẵn hàng nghìn ứng dụng kèm hash kiểm chứng.
- **Repo tham khảo UX: UniGetUI** — https://github.com/Devolutions/UniGetUI
  (trước đây là marticliment/UniGetUI, nay thuộc org Devolutions) — **MIT**,
  ≈27k sao, phát hành rất đều (bản 2026.3.x, 11/2026). Là giao diện hợp nhất
  cho winget/Scoop/Chocolatey: tìm kiếm, chọn nhiều gói, cài/gỡ/cập nhật hàng
  loạt, xuất/nhập danh sách gói.
- **Phương án phụ:** Scoop (https://github.com/ScoopInstaller/Scoop —
  **Unlicense/MIT**, ≈25k sao; cài không cần admin, manifest JSON theo bucket)
  và Chocolatey (https://github.com/chocolatey/choco — **Apache-2.0**, ≈11,5k
  sao; kho gói lớn nhất, có kiểm duyệt) — chỉ cân nhắc nếu cần gói mà winget
  chưa có.
- **Áp dụng:** (a) bỏ hẳn mô hình "danh sách link tải do tác giả tuyển" của
  bản gốc — thay bằng **catalogue JSON của ta ánh xạ sang winget ID** (Chrome,
  UltraViewer, 7-Zip, VLC, Foxit…), cài bằng `winget install --id ... --silent
  --accept-*`, hash do chính kho winget đảm bảo; (b) học UniGetUI ở các điểm:
  hàng đợi cài đặt có trạng thái từng gói, nút "cập nhật tất cả", xuất/nhập
  bundle danh sách phần mềm (rất hợp với kịch bản cài máy mới hàng loạt);
  (c) ứng dụng không có trên winget mới rơi về cơ chế `custom` (file cài do
  người dùng tự cung cấp, công cụ không tự ý tải link lạ).
- **Không copy:** không cần copy code — winget là công cụ hệ thống ta gọi
  ngoài; UniGetUI là MIT nên nếu cần tham khảo sâu ở mức code cũng hợp lệ
  (ghi công), nhưng kiến trúc Rust của ta chỉ cần hành vi.

### Module 12 — Sao Lưu Dữ Liệu

- **Repo tham khảo thiết kế 1: restic** — https://github.com/restic/restic —
  **BSD-2-Clause**, ≈30k sao, bản 0.19.1 (07/2026), rất tích cực. Backup dạng
  snapshot có khử trùng lặp (content-defined chunking), mã hoá mặc định, kiểm
  tra toàn vẹn (`restic check`), và trên Windows hỗ trợ sao lưu từ **VSS
  snapshot** (`--use-fs-snapshot`) để copy được cả file đang bị khoá.
- **Repo tham khảo thiết kế 2: kopia** — https://github.com/kopia/kopia —
  **Apache-2.0**, ≈10k+ sao, bản 0.23.1 (06/2026). Cùng họ restic nhưng có
  sẵn chính sách (policy) lưu giữ snapshot và giao diện — mẫu tốt cho phần
  "chính sách sao lưu" nếu sau này nâng cấp.
- **Áp dụng:** nhu cầu cốt lõi của module (chép hồ sơ người dùng sang ổ khác
  trước khi cài lại Win) vẫn dùng **robocopy** (có sẵn trong Windows, cờ
  `/MIR /XJ /COPY:DAT /R:1 /W:1`, ghi log) do ta tự viết. Học từ restic/kopia
  ba điểm nâng cấp: (1) **VSS snapshot** làm nguồn copy để không lỗi file đang
  mở (Outlook/Zalo đang chạy); (2) bước **kiểm chứng sau sao lưu** (đối chiếu
  số file/dung lượng, tuỳ chọn checksum mẫu); (3) danh sách loại trừ chuẩn
  (thư mục cache, junction hệ thống) và báo cáo cuối phiên rõ ràng. Nếu sau
  này muốn sao lưu có phiên bản thật sự, restic là BSD-2 — **được phép đóng
  gói kèm binary restic** (giữ LICENSE) làm chế độ nâng cao; đó là quyết định
  riêng, chưa cần ngay.
- **Không copy:** không tự chế định dạng kho backup riêng — hoặc robocopy
  thuần, hoặc dùng nguyên restic; tránh định dạng "nửa vời" không ai khôi
  phục được.

### Module 16 — Check Cấu Hình Máy Tính

- **Repo tham khảo 1: fastfetch** — https://github.com/fastfetch-cli/fastfetch
  — **MIT**, ≈25k sao, phát hành liên tục (nhánh 2.7x, 2026). Công cụ kê khai
  hệ thống đa nền tảng: CPU, GPU, RAM, ổ đĩa, mainboard, BIOS, OS/build, màn
  hình, pin… — giá trị lớn nhất là **mô hình trường dữ liệu** (module nào,
  lấy từ đâu, hiển thị thế nào) đã được cộng đồng rà soát trên hàng nghìn cấu
  hình.
- **Repo tham khảo 2: LibreHardwareMonitor** —
  https://github.com/LibreHardwareMonitor/LibreHardwareMonitor — **MPL-2.0**,
  ≈9,4k sao, bản 0.9.6 (02/2026). Thư viện cảm biến phần cứng .NET: nhiệt độ,
  điện áp, tốc độ quạt, xung nhịp, mức tải — phần mà WMI thuần không có.
- **Áp dụng:** (a) danh sách trường kê khai theo fastfetch, lấy bằng WMI
  (`Win32_Processor/BaseBoard/PhysicalMemory/VideoController/DiskDrive/
  OperatingSystem/BIOS/NetworkAdapter`) — khớp đúng bản đồ suy luận của
  module gốc; (b) mục cảm biến (nhiệt độ CPU/GPU, quạt) tham khảo danh mục
  cảm biến của LibreHardwareMonitor nhưng **tự đọc qua WMI/ACPI và IOCTL của
  hãng** trong Rust.
- **Không copy / lưu ý:** LibreHardwareMonitor là MPL-2.0 (copyleft mức file)
  và lịch sử dùng driver WinRing0 từng bị Defender gắn cờ (dự án đang chuyển
  sang PawnIO) — **không nhúng driver của nó** vào bản phân phối của ta; chỉ
  lấy các chỉ số đọc được qua kênh chính thống, thiếu thì ghi "không đọc
  được" thay vì cài driver lạ.

### Module 17 — Check / Test

- **Repo tham khảo: CrystalDiskInfo** — https://github.com/hiyohiyo/CrystalDiskInfo
  — **MIT**, dự án Crystal Dew World, còn phát hành đều (nhánh 9.9.x, 2026).
  Chuẩn mực de-facto về đọc SMART: ngưỡng sức khoẻ ổ (Good/Caution/Bad),
  diễn giải thuộc tính thô của từng hãng (đặc biệt SSD/NVMe: percentage used,
  power-on hours, host writes, nhiệt độ).
- **Áp dụng:** (a) mô hình diễn giải SMART/NVMe của CrystalDiskInfo cho bài
  test ổ đĩa (đọc qua IOCTL `SMART_RCV_DRIVE_DATA` / StorageDeviceProtocol
  cho NVMe, tự viết trong Rust); (b) các bài test còn lại — điểm chết màn
  hình (cửa sổ màu toàn màn), bàn phím (sơ đồ phím trực quan), pin
  (`powercfg /batteryreport` + WMI `Win32_Battery`), RAM (trỏ tới `mdsched`
  có sẵn) — là tính năng tự viết, không có repo nào làm tốt hơn công cụ có
  sẵn của Windows.
- **Không copy:** CrystalDiskInfo là MIT nên rào cản license thấp, nhưng code
  C++/MFC không dùng lại được trong Rust — giá trị nằm ở bảng diễn giải thuộc
  tính, ta tự biên soạn lại từ tài liệu SMART/NVMe công khai và đối chiếu
  hành vi của nó.

### Module 20 — Tuỳ Chỉnh Windows ⭐ (ưu tiên số 1 của người dùng)

- **Repo chính: Sophia Script for Windows** —
  https://github.com/farag2/Sophia-Script-for-Windows — **MIT**, ≈9,9k sao,
  dự án tweak Windows được bảo trì nghiêm túc nhất hiện nay: 170+ bản phát
  hành, bản 8.0.0 ra 10/2026, hỗ trợ Windows 10/11 theo từng build. Hơn 150
  hàm tweak có cấu trúc thống nhất: mỗi tweak là một hàm có tên rõ ràng, có
  **hàm hoàn tác tương ứng** (áp dụng ↔ khôi phục mặc định), có kiểm tra điều
  kiện theo phiên bản Windows.
- **Repo phụ: ChrisTitusTech/winutil** — https://github.com/ChrisTitusTech/winutil
  — **MIT**, ≈66k sao, phát hành đều (bản 26.10.x). Windows Utility dạng
  PowerShell/WPF: tab Install (cài app qua winget), Tweaks (checklist có mô
  tả + mức độ khuyến nghị), Features, Updates. Giá trị tham khảo nằm ở **cách
  trình bày tweak cho người dùng phổ thông**: mô tả ngắn, phân loại
  Essential/Advanced, cảnh báo rủi ro.
- **Tham khảo thêm:** hellzerg/Optimizer (https://github.com/hellzerg/optimizer
  — **GPL-3.0**, bản 17.0, 11/2026; tác giả đã tuyên bố chuyển hướng sang
  OptimizerNXT) — đáng học ở **định dạng template JSON theo phiên bản
  Windows** (`template-windows11.json`…) cho phép áp dụng tweak theo hồ sơ
  cấu hình (`optimizer.exe /config=...`) — đúng thứ ta cần cho kỹ thuật viên
  áp cùng một bộ tweak lên nhiều máy. Chỉ học định dạng ý tưởng, không dùng
  code/dữ liệu của nó.
- **Áp dụng (quyết định chính của tài liệu này):** xây **danh mục tweak của
  ta dưới dạng dữ liệu** (JSON/TOML trong repo của dự án), mỗi mục gồm: id,
  tên tiếng Việt, mô tả, nhóm (Riêng tư / Hiệu năng / Giao diện / Chuột phải /
  Hệ thống…), thao tác áp dụng (registry/policy/service/task — khai báo dữ
  liệu, engine Rust thực thi), thao tác hoàn tác, điều kiện phiên bản, mức
  cảnh báo. Nguồn nội dung danh mục: dịch/chuyển từ catalogue của **Sophia
  Script (MIT — hợp lệ, ghi công)** và đối chiếu chéo với winutil; mỗi tweak
  phải có hoàn tác — đây là chuẩn chất lượng Sophia đặt ra và bản gốc AutoIt
  không có. Việc này cũng **giải quyết điểm nghẽn "Pending source recovery"**
  của module 20: thay vì chờ memory dump để lấy danh sách tweak của bản gốc,
  ta có catalogue chuẩn cộng đồng để đối chiếu — dump sau này (nếu có) chỉ
  dùng để bổ sung những tweak đặc thù bản gốc có mà catalogue chưa có.
- **Không copy:** code của Optimizer (GPL-3.0); không bê nguyên văn phần mô
  tả tiếng Anh dài của bất kỳ repo nào — diễn giải tiếng Việt tự viết.

### Module 22 — Windows Security

- **Repo tham khảo: ConfigureDefender** — https://github.com/AndyFul/ConfigureDefender
  (AndyFul) — công cụ cấu hình Microsoft Defender chuẩn mực: bản ổn định
  4.1.0.0 (12/2025), là một phần của dự án Hard_Configurator. Điểm đáng học:
  (1) **danh mục đầy đủ các thiết lập Defender ẩn** — những thiết lập chỉ có
  qua `Get/Set-MpPreference`, Group Policy hoặc registry (cloud protection
  level, block-at-first-sight, PUA protection, thư mục kiểm soát truy cập…);
  (2) **các quy tắc ASR** (Attack Surface Reduction) kèm GUID chuẩn của
  Microsoft; (3) mô hình **mức bảo vệ đóng gói sẵn** Default/High/Max — áp một
  "hồ sơ" thay vì 30 công tắc rời.
- **Áp dụng:** biên soạn danh mục thiết lập Defender của ta (tên thiết lập →
  khóa policy/cmdlet tương ứng → giá trị theo từng mức) và áp dụng qua kênh
  chính thống: WMI `MSFT_MpPreference`/policy registry do ta tự ghi. Phần
  còn lại của module (Firewall qua `netsh advfirewall`/INetFwPolicy2, UAC và
  SmartScreen qua policy) là API Windows chuẩn, tự viết. **Khác bản gốc một
  cách có chủ đích:** bản gốc thiên về *tắt* bảo mật cho tiện cài đặt; bản
  rebuild trình bày hai chiều (tắt tạm có hẹn giờ bật lại / áp hồ sơ bảo vệ)
  và luôn hiển thị trạng thái thật đọc từ hệ thống.
- **Không copy:** license của ConfigureDefender không được khai báo rõ ràng
  trên repo (GitHub xếp "other"; dự án mẹ theo GPL) — **coi như GPL: chỉ
  tham khảo danh mục thiết lập và ý tưởng hồ sơ**, không dùng code. Bản thân
  ConfigureDefender cũng chủ đích không tắt real-time monitoring — ta giữ
  nguyên tắc đó.

### Module 23 — Quản Lý Update Windows

- **Nền tảng: Windows Update Agent API (WUA COM)** của Microsoft —
  `IUpdateSearcher` (tìm bản cập nhật), `IUpdateInstaller` (cài), `IAutomaticUpdates`
  (cài đặt AU), cộng với lớp policy `HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate`
  và các service `wuauserv/UsoSvc/BITS/DoSvc`. Đây là API chính hãng, tự gọi
  từ Rust qua COM — không cần thư viện ngoài.
- **Repo tham khảo 1: PSWindowsUpdate** — https://github.com/mgajda83/PSWindowsUpdate
  (Michal Gajda) — **MIT**, ≈490 sao, module PowerShell quản trị WU lâu đời
  (2011–nay, bản 2.2.1.x; nhịp phát hành đã chậm — bản Gallery cuối 12/2023).
  Giá trị: **bề mặt thao tác đã được kiểm chứng thực tế** — tìm/cài/ẩn
  (hide) bản cập nhật, lịch sử cập nhật, và đặc biệt `Set-WUSettings` với
  khái niệm **TargetRelease** (ghim phiên bản tính năng tối đa) — đúng nhu
  cầu "khóa máy ở bản Win hiện tại" của kỹ thuật viên.
- **Repo tham khảo 2: WuMgr** — https://github.com/DavidXanatos/wumgr —
  **GPL-3.0**, bản cuối 1.1 từ 12/2019, **đã ngừng phát triển**. Chỉ còn giá
  trị xác nhận cách dùng WUA COM cho tính năng ẩn/hiện bản cập nhật và quản
  lý hàng đợi tải — đọc để đối chiếu, không dùng gì khác.
- **Áp dụng:** thiết kế bề mặt chức năng theo PSWindowsUpdate (tìm kiếm theo
  tiêu chí, ẩn bản cập nhật lỗi, ghim TargetReleaseVersion, tạm dừng theo
  policy thay vì tắt service thô bạo), cài đặt bằng WUA COM + policy/service
  tự viết. Cách "tắt update" của bản gốc (dừng + disable service) được giữ
  nhưng trình bày kèm cảnh báo và nút khôi phục một chạm.
- **Không copy:** code WuMgr (GPL-3.0, và đã chết — tránh xa); PSWindowsUpdate
  là MIT nhưng là DLL .NET cho PowerShell, ta chỉ lấy ngữ nghĩa thao tác.

### Module 24 — Dọn Rác Máy Tính

- **Repo tham khảo: BleachBit** — https://github.com/bleachbit/bleachbit —
  **GPL-3.0**, ≈7,5k sao, rất tích cực (bản 6.0.6, 10/2026). Hệ thống định
  nghĩa dọn dẹp **CleanerML** (XML khai báo): mỗi ứng dụng một file định
  nghĩa gồm các vị trí file/thư mục/registry cần dọn, theo từng hệ điều hành
  — hàng trăm cleaner cho trình duyệt, Office, Windows Update, log, cache…
- **Áp dụng:** (a) dùng catalogue CleanerML làm **bản đồ vị trí rác** để biên
  soạn tập định nghĩa của ta (định dạng JSON riêng, diễn giải tự viết) — bao
  quát hơn nhiều so với danh sách suy luận ban đầu (Temp, Prefetch,
  SoftwareDistribution\Download, thùng rác); (b) học quy trình an toàn của
  BleachBit: quét trước → báo dung lượng từng hạng mục → xem trước file sẽ
  xoá → mới dọn; (c) các hạng mục hệ thống dùng kênh chính hãng: DISM
  `/StartComponentCleanup`, `cleanmgr` sageset, Clear-RecycleBin.
- **Không copy:** file CleanerML là dữ liệu GPL-3.0 — **không nhúng nguyên
  các file XML đó** vào bản phân phối; ta tự viết định nghĩa từ bản đồ vị trí
  (vị trí file là dữ kiện, cách diễn đạt trong XML là phần phải tránh sao
  chép nguyên văn).

### Module 25 — Gỡ Bỏ Phần Mềm

- **Repo chính: BCUninstaller (Bulk Crap Uninstaller)** —
  https://github.com/BCUninstaller/Bulk-Crap-Uninstaller (trước đây thuộc
  Klocman) — **Apache-2.0**, bản 6.3 (09/2026). Bộ gỡ cài đặt mã nguồn mở
  mạnh nhất hiện nay: liệt kê từ mọi nguồn (registry Uninstall cả 3 hive,
  Store/MSIX, Steam, Windows Features), **gỡ im lặng hàng loạt** bằng cách tự
  suy luận tham số quiet từ loại trình cài đặt (MSI/Inno/NSIS/InstallShield…),
  **quét leftover** (file, thư mục, khóa registry còn sót) sau khi gỡ, chấm
  điểm độ tin cậy của mục gỡ. ⚠️ Repo có banner "đang tìm maintainer mới" —
  code vẫn dùng tốt nhưng cần cân nhắc khi phụ thuộc dài hạn.
- **Áp dụng (áp dụng sâu nhất trong các module, nhờ Apache-2.0):** (1) thuật
  toán liệt kê đa nguồn và khử trùng lặp; (2) bảng suy luận tham số gỡ im
  lặng theo loại installer; (3) quy trình quét leftover hai lớp (đường dẫn
  suy ra từ mục gỡ + khóa registry mồ côi) có xem trước và xác nhận từng mục;
  (4) xử lý Store app bằng Appx/MSIX API song song với chương trình desktop.
  Vì dự án là Rust, "áp dụng" nghĩa là cài đặt lại các thuật toán này theo
  đúng hành vi đã mô tả trong tài liệu/source của BCU — Apache-2.0 cho phép
  cả việc tham khảo ở mức code, kèm NOTICE ghi công.
- **Không copy:** không bê giao diện WinForms; không dùng tính năng gỡ ép
  (force uninstall xóa tay) làm mặc định — luôn qua uninstaller chính thức
  trước, leftover chỉ dọn sau khi người dùng duyệt danh sách.

### Module 29 — Quản lý File/Folder

- **Repo tham khảo: ContextMenuManager** —
  https://github.com/BluePointLilac/ContextMenuManager — **GPL-3.0**, ≈19k
  sao, bản 3.3.3.1. Công cụ quản lý menu chuột phải tốt nhất dạng mã nguồn
  mở: bật/tắt/gỡ mục trong menu của file, thư mục, Send To, Open With, WinX,
  kèm cơ sở dữ liệu nhận diện mục của phần mềm bên thứ ba.
- **Áp dụng:** phần lõi của module ta (take ownership bằng `takeown`/`icacls`,
  sửa thuộc tính `attrib -h -r -s`, hiện file ẩn/đuôi file qua khóa Explorer
  Advanced, xoá file cứng đầu qua `PendingFileRenameOperations`, cài mục
  "Take Ownership" vào menu chuột phải) đều là lệnh/API Windows chuẩn — tự
  viết. ContextMenuManager chỉ đáng tham khảo cho **danh mục vị trí registry
  của menu ngữ cảnh** nếu sau này mở rộng module theo hướng quản lý menu.
- **Không copy:** code và cơ sở dữ liệu diễn giải của nó (GPL-3.0).

### Module 30 — Chuyển Windows OS

- **Repo tham khảo 1: wimlib** — https://github.com/ebiggers/wimlib (bản
  gương của wimlib.net) — **hai lựa chọn license GPL-3.0+ hoặc LGPL-3.0+**
  (một số file CC0), bản 1.14.5 (01/2026). Thư viện/công cụ thao tác ảnh WIM
  hoàn chỉnh nhất ngoài DISM: capture/apply có kiểm chứng, nén, split, và tài
  liệu ngữ nghĩa WIM rất sâu.
- **Repo tham khảo 2: Rescuezilla / Clonezilla** —
  https://github.com/rescuezilla/rescuezilla — **GPL-3.0**, ≈2,8k sao, bản
  2.6.2 (05/2026); Clonezilla **GPL-2.0**. Cả hai là môi trường **Linux boot
  từ USB** (dựa trên partclone) — không phải code chạy trong Windows.
- **Áp dụng:** (a) đường chính của ta giữ nguyên pipeline Windows thuần:
  `DISM /Capture-Image` từ WinPE (dùng lại engine WinPE đã có) →
  `/Apply-Image` sang ổ đích → `bcdboot` — wimlib là tài liệu đối chiếu ngữ
  nghĩa khi DISM báo lỗi khó hiểu; (b) học Rescuezilla ở **trình tự UX chống
  chọn nhầm ổ đích** (hiển thị model/serial/dung lượng, xác nhận hai bước) —
  lỗi chọn nhầm ổ là rủi ro lớn nhất của tính năng này; (c) ghi nhận giới hạn
  thật: DISM capture/apply là sao chép mức file, không phải clone mức block
  (không giữ được mọi metadata lạ) — trình bày đúng với người dùng.
- **Không copy:** không nhúng wimlib/Rescuezilla/Clonezilla (GPL; và
  Rescuezilla là cả một hệ điều hành Linux — nằm ngoài kiến trúc của ta).

### Module 31 — Multi Zalo PC

- **Bối cảnh kỹ thuật:** Zalo PC (ứng dụng Electron) khóa một phiên bản bằng
  mutex toàn cục — chỉ đổi thư mục dữ liệu (`--user-data-dir`) là **không
  đủ** (tiến trình thứ hai vẫn tự thoát khi thấy mutex/session của bản đầu,
  theo đúng bản chất khóa single-instance). Có hai đường khả thi:
  (1) chạy mỗi tài khoản trong một **phiên người dùng Windows riêng**
  (Fast User Switching / `runas` tài khoản phụ) — sạch, không cần phần mềm
  ngoài, nhưng bất tiện; (2) chạy trong **sandbox có namespace riêng**.
- **Repo tham khảo: Sandboxie Plus** — https://github.com/sandboxie-plus/Sandboxie
  — lõi **GPL-3.0** (bản Plus có thêm điều khoản riêng và tính năng nâng cao
  cần supporter certificate), ≈20k sao, phát triển rất tích cực (nhánh 1.18.x,
  10/2026). Mỗi "box" là một namespace riêng nên nhiều phiên bản ứng dụng
  single-instance chạy song song được — đây là cách cộng đồng chạy đa phiên
  bản các app chat.
- **Áp dụng:** module của ta làm đường (1) trước (tạo/launcher phiên theo hồ
  sơ, mỗi hồ sơ một thư mục dữ liệu + tuỳ chọn tài khoản Windows phụ); đồng
  thời hỗ trợ đường (2) dạng **tích hợp ngoài**: nếu máy đã có Sandboxie do
  người dùng tự cài, module chỉ gọi `Start.exe /box:<tên> Zalo.exe` như một
  tiến trình độc lập — không liên kết, không nhúng, không phân phối kèm.
- **Không copy / không đóng gói:** tuyệt đối không bundle Sandboxie (GPL-3.0
  + driver hạt nhân cần chữ ký riêng) trong gói cài của ta.

### Nhóm clean-room — các module không có (hoặc không cần) repo tham chiếu

Các module dưới đây đều có cơ chế chuẩn mực rõ ràng của Windows; khảo sát cho
thấy **không tồn tại repo mã nguồn mở nào đủ uy tín/đủ sát** để đáng tham
khảo hơn tài liệu Microsoft — tự viết theo API chuẩn là đáp án đúng:

- **Module 2 — Backup License MS:** dừng `sppsvc`, sao lưu/khôi phục
  `tokens.dat`/`data.dat` của `spp\store\2.0` và ClipSvc (kèm cảnh báo: chỉ
  khôi phục được trên cùng phần cứng/cùng phiên bản — giới hạn do cơ chế kích
  hoạt của Microsoft, không phải của công cụ).
- **Module 4 — Tắt Bitlocker:** `manage-bde -status` / `-off` hoặc
  `Disable-BitLocker`, theo dõi phần trăm giải mã. Lưu ý thiết kế: hiển thị
  trạng thái "đang giải mã" thay vì báo xong sớm — lỗi UX phổ biến của các
  tool tự chế.
- **Module 13 — Sao Lưu Wifi:** `netsh wlan export profile key=clear`,
  `add profile`, `show profile key=clear`. File XML xuất ra chứa mật khẩu
  dạng rõ — module phải cảnh báo và mặc định đặt thư mục sao lưu có quyền
  truy cập hạn chế.
- **Module 14 — Sao Lưu Chrome-Edge:** đóng tiến trình trình duyệt, copy thư
  mục `User Data` (hoặc các file lõi: Bookmarks, History, Login Data,
  Cookies, Preferences, thư mục Extensions). **Giới hạn DPAPI phải nói rõ
  trong UI:** mật khẩu/cookie đã lưu chỉ giải mã được trên cùng máy + cùng
  tài khoản Windows — sao lưu sang máy khác sẽ mất phần đó.
- **Module 15 — Backup/Restore Zalo:** đóng tiến trình Zalo, copy thư mục
  `%AppData%\ZaloData` hai chiều, kiểm chứng dung lượng trước/sau.
- **Module 18 — Phím Tắt BIOS:** thuần dữ liệu tĩnh (Dell F2/F12, HP
  Esc/F10/F9, Lenovo F1/F2/Fn+F2/Novo, Asus F2/Del/Esc/F8, Acer F2/Del/F12,
  MSI Del/F11, Gigabyte Del/F12…) — biên soạn từ tài liệu hãng, có ô tìm kiếm
  không dấu giống launcher. Module dễ nhất toàn dự án.
- **Module 19 — Phân Vùng Ổ Cứng:** không có thư viện phân vùng mã nguồn mở
  nào chạy native trong Windows đáng dùng (GParted/parted là thế giới Linux).
  Dùng **MSFT Storage WMIv2** (`root\Microsoft\Windows\Storage`:
  MSFT_Partition/MSFT_Volume — Resize, Create, Delete, gán ký tự ổ) — chính
  là nền mà PowerShell Storage cmdlets dùng. Bắt buộc có: xem trước thao tác,
  chặn thao tác trên phân vùng hệ thống đang chạy, xác nhận gõ lại ký tự ổ
  với thao tác xoá/format.
- **Module 26 — Fix Lỗi Máy In:** không có "bộ fix máy in" mã nguồn mở uy
  tín; danh mục fix là kiến thức kỹ thuật chuẩn, tự biên soạn: khởi động lại
  Print Spooler + dọn `%System32%\spool\PRINTERS`, đặt StartupType, fix lỗi
  chia sẻ 0x0000011b (`RpcAuthnLevelPrivacyEnabled`), gỡ driver in hỏng bằng
  `pnputil`/printmanagement, bật troubleshooter `PrinterDiagnostic`. Mỗi fix
  là một mục độc lập có mô tả triệu chứng — chạy được từng mục, có log.
- **Module 27 — Quản Lý Máy In:** WMI `Win32_Printer` (SetDefaultPrinter,
  PrintTestPage, RenamePrinter, chia sẻ qua Shared/ShareName) và
  `rundll32 printui.dll,PrintUIEntry` — API chuẩn, tự viết.
- **Module 28 — Fix Share Mạng LAN:** không có repo tham chiếu đáng tin (và
  blob ~1 MB bất thường của bản gốc vẫn chưa giải thích được — chỉ memory
  dump mới trả lời được nó chứa gì thêm). Danh mục fix chuẩn tự biên soạn:
  bật các service `FDResPub/FDPHost/SSDPDiscovery/UPnPHost/LanmanServer/
  LanmanWorkstation` + đặt Automatic, chuyển profile mạng sang Private, mở
  rule firewall File and Printer Sharing, `AllowInsecureGuestAuth` (kèm cảnh
  báo bảo mật rõ ràng — đây là hạ chuẩn bảo mật có chủ đích), tắt
  password-protected sharing khi người dùng chọn. Trình bày dạng checklist
  có trạng thái thật từng mục thay vì "một nút fix tất cả" mù quáng.

## 3. Thứ tự ưu tiên nâng cấp

Nguyên tắc xếp hạng: (1) ưu tiên đã nêu của người dùng — Tuỳ Chỉnh Windows
trước; (2) tần suất kỹ thuật viên dùng hằng ngày; (3) độ chín của repo tham
chiếu (có catalogue/dữ liệu sẵn thì làm nhanh và chất lượng cao); (4) rủi ro
thấp làm trước để tích luỹ đà.

**Đợt 1 — 8 module tiếp theo (thứ tự build đề xuất):**

| Thứ tự | Module | Lý do / điểm tựa |
|---|---|---|
| 1 | **20 — Tuỳ Chỉnh Windows** | Ưu tiên số 1 của người dùng. Catalogue Sophia Script (MIT) cho sẵn 150+ tweak có hoàn tác — đồng thời gỡ luôn điểm nghẽn "chờ memory dump". |
| 2 | **25 — Gỡ Bỏ Phần Mềm** | Dùng mỗi ngày; BCUninstaller (Apache-2.0) cho thuật toán gỡ im lặng + quét leftover ở mức áp dụng sâu nhất dự án. |
| 3 | **11 — Cài Đặt Phần Mềm** | Engine winget có sẵn trong Windows — thay hẳn mô hình link của tác giả bằng catalogue winget ID an toàn, kiểm chứng hash bởi kho Microsoft. |
| 4 | **24 — Dọn Rác Máy Tính** | Đơn giản, dùng hằng ngày; bản đồ vị trí rác từ BleachBit giúp bao quát ngay từ bản đầu. |
| 5 | **23 — Quản Lý Update Windows** | Nhu cầu kỹ thuật rất cao (ghim phiên bản, chặn bản cập nhật lỗi); bề mặt thao tác theo PSWindowsUpdate, engine WUA COM chuẩn. |
| 6 | **22 — Windows Security** | Đi cặp với module 20/23 (cùng họ thiết lập hệ thống); danh mục Defender theo ConfigureDefender, có sẵn ý tưởng hồ sơ Default/High/Max. |
| 7 | **16 — Check Cấu Hình** | Chỉ đọc, rủi ro bằng không, làm nhanh theo mô hình trường của fastfetch — module "thắng sớm" tốt để kiểm chứng khung module Rust dùng chung. |
| 8 | **12 — Sao Lưu Dữ Liệu** | Điều kiện tiên quyết của luồng cài lại máy (sao lưu xong mới cài); robocopy + bài học VSS/kiểm chứng từ restic. |

**Đợt 2 — hoàn thiện nhóm sao lưu & chẩn đoán:**
9. Sao Lưu Wifi (13) · 10. Sao Lưu Chrome-Edge (14) · 11. Backup/Restore Zalo
(15) · 12. Tắt Bitlocker (4 — điều kiện tiên quyết trước khi cài Win) ·
13. Check/Test (17) · 14. Phím Tắt BIOS (18 — trivial, tranh thủ làm bất cứ
lúc nào) · 15. Backup License MS (2).

**Đợt 3 — nhóm Office, máy in, mạng:**
16. Cài Office Online (8) + OFFLINE (9) (chung một lõi ODT) · 17. Tuỳ Chỉnh
Office (21) · 18. Quản Lý Máy In (27) · 19. Fix Lỗi Máy In (26) · 20. Fix
Share Mạng LAN (28 — chờ thêm thông tin từ memory dump về blob bất thường
nếu có thể) · 21. Quản lý File/Folder (29).

**Đợt 4 — nhóm kho, đĩa và hạng nặng:**
22. Kho Windows (1) + Kho Driver IRST (0) (cần hạ tầng catalogue riêng của
ta sẵn sàng trước) · 23. Tạo Boot WinPE (5 — tận dụng engine WinPE đã có) ·
24. Nâng cấp Backup/Restore Driver (3) theo RAPR · 25. Phân Vùng Ổ Cứng
(19) · 26. Multi Zalo PC (31) · 27. Chuyển Windows OS (30 — phức tạp, rủi ro
dữ liệu cao nhất, làm cuối khi khung xác nhận an toàn đã chín qua các module
đĩa trước) · 28. Kích Hoạt Win/Office (10 — phạm vi hợp lệ nhỏ, làm lúc nào
cũng được, để cuối vì giá trị thấp nhất).

## 4. Danh sách module KHÔNG có repo tốt — clean-room là đáp án đúng

| Module | Lý do không có repo phù hợp | Chuẩn tự viết |
|---|---|---|
| Backup License MS (2) | Chỉ là thao tác file của SPP/ClipSvc; các "tool backup license" trôi nổi đều không rõ nguồn gốc | File ops + điều khiển service `sppsvc` |
| Tắt Bitlocker (4) | Một lệnh chuẩn bao hết chức năng | `manage-bde` / `Disable-BitLocker` |
| Sao Lưu Wifi (13) | netsh wlan đã là giao diện chính thức đầy đủ | `netsh wlan` export/import |
| Sao Lưu Chrome-Edge (14) | Bản chất là copy thư mục profile có chọn lọc | File ops + ghi chú giới hạn DPAPI trong UI |
| Backup/Restore Zalo (15) | Đặc thù ứng dụng Việt Nam, không có hệ sinh thái quốc tế | File ops trên `%AppData%\ZaloData` |
| Phím Tắt BIOS (18) | Dữ kiện tĩnh của hãng, không phải bài toán code | Bảng dữ liệu tự biên soạn từ tài liệu hãng |
| Phân Vùng Ổ Cứng (19) | Mọi engine phân vùng OSS đều thuộc thế giới Linux | MSFT Storage WMIv2 |
| Fix Lỗi Máy In (26) | Kiến thức fix là danh mục thao tác chuẩn, không tồn tại bộ fix OSS uy tín | Spooler service, `RpcAuthnLevelPrivacyEnabled`, printui/pnputil |
| Quản Lý Máy In (27) | API quản lý in của Windows đã đầy đủ | WMI `Win32_Printer`, printui.dll |
| Fix Share Mạng LAN (28) | Các "fix kit" trôi nổi không kiểm chứng được; danh mục fix là kiến thức chuẩn | Service control, firewall API, registry policy có cảnh báo |
| Kích Hoạt Win/Office (10) | Chủ đích chính sách: chỉ kích hoạt hợp lệ — repo kích hoạt bị loại theo quyết định | WMI `SoftwareLicensingService`, slmgr/ospp chính hãng |
| Tuỳ Chỉnh Office (21) | Nguồn chuẩn là ADMX của chính Microsoft, không phải repo cộng đồng | Registry/policy từ ADMX |

## 5. Cảnh báo license — tóm tắt để nhớ nhanh

1. **Được dùng sâu (kèm ghi công):** Sophia Script (MIT), winutil (MIT),
   BCUninstaller (Apache-2.0), UniGetUI (MIT), winget (MIT), Scoop
   (Unlicense/MIT), Chocolatey (Apache-2.0), restic (BSD-2), kopia
   (Apache-2.0), fastfetch (MIT), CrystalDiskInfo (MIT), UUPMediaCreator
   (MIT), PSWindowsUpdate (MIT), WinSparkle (MIT).
2. **Chỉ tham khảo hành vi, tuyệt đối không copy code/không bundle binary:**
   DriverStore Explorer (GPL-2.0), BleachBit + CleanerML (GPL-3.0), WuMgr
   (GPL-3.0, đã ngừng), SDIO (GPL-3.0), Fido (GPL-3.0), Optimizer (GPL-3.0),
   ContextMenuManager (GPL-3.0), Sandboxie (GPL-3.0 + điều khoản riêng bản
   Plus), Rescuezilla (GPL-3.0) / Clonezilla (GPL-2.0), wimlib (GPL/LGPL —
   không cần dùng vì đã có DISM), ConfigureDefender (không khai rõ — coi như
   GPL), Office Tool Plus (freeware — chỉ học hành vi).
3. **Tài nguyên hãng, không phải OSS:** Windows ADK/WinPE, Office Deployment
   Tool, ADMX templates (Microsoft); driver RST (Intel) — tải từ hãng, dùng
   theo điều khoản hãng, không phân phối lại trong gói của ta.
4. **Cạm bẫy riêng:** LibreHardwareMonitor (MPL-2.0) — tránh nhúng driver
   cảm biến của nó (từng bị Defender gắn cờ); PhoenixPE — license hỗn hợp ở
   mức repo, chỉ học quy trình; BCUninstaller — đang tìm maintainer, cân nhắc
   khi phụ thuộc dài hạn.
