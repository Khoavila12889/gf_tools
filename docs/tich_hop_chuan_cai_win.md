# Tích hợp chuẩn cộng đồng vào module Cài Đặt Windows + Hậu cài tự động

Phiên bản: v1 — 2026-11-16
Tài liệu nghiên cứu phục vụ bản rebuild (Rust). Đặc tả gốc:
`docs/logic_cai_dat_win_online.md` (gọi tắt là "đặc tả logic", dẫn theo §).
Yêu cầu mới của người dùng được thiết kế trong tài liệu này:

1. Sau khi cài xong và reboot, máy **tự đăng nhập vào Windows mới** (AutoLogon).
2. **Tự cài im lặng và tự chạy UltraViewer**, cấu hình sẵn truy cập không
   cần người trực (unattended access).
3. **Tự cài im lặng Google Chrome**.

Quy ước độ tin cậy trong tài liệu:

- **[ĐÃ KIỂM CHỨNG]** — có nguồn chính thức (trang vendor/Microsoft/repo) hậu thuẫn.
- **[TIN CẬY CAO]** — nhiều nguồn phụ khớp nhau, chưa có tài liệu chính thức trực tiếp.
- **[CHƯA KIỂM CHỨNG]** — mới là manh mối/giả thuyết, bắt buộc phải test trên
  máy thật trước khi bật mặc định.

---

## 1. Bảng khảo sát repo/công cụ chuẩn

| Tên | URL | Làm gì | License | Ta lấy (adopt) | Ta bỏ (skip) | Lý do |
|---|---|---|---|---|---|---|
| **OSDCloud** (OSDeploy — David Segura, nay Recast Software bảo trì) | Repo module mới: https://github.com/OSDeploy/OSDCloud · Repo module OSD gốc: https://github.com/OSDeploy/OSD · Docs: https://www.osdeploy.com/guides/pscloudscript/pscloudscript-examples/osdcloud-live | Chuẩn cộng đồng cho zero-touch deployment từ WinPE: build WinPE từ ADK (`Edit-OSDCloudWinPE`), nhúng driver (kể cả gói driver theo hãng — CloudDriver Dell/HP/Lenovo…), `Start-OSDCloud -ZTI` tự tải image Windows từ catalogue cloud của Microsoft, chia đĩa, apply image bằng DISM, inject driver offline, copy unattend vào Panther, reboot. Có bản GUI (`Start-OSDCloudGUI`/`Deploy-OSDCloud`). | OSD (module gốc): **MIT** [ĐÃ KIỂM CHỨNG qua GitHub API/GitPlanet]. OSDCloud (module mới): có file LICENSE trong repo — kiểm tra lại trước khi vendor bất kỳ đoạn code nào. Ta chỉ học workflow, không nhúng code, nên không bị ràng buộc thực tế. | • Mô hình tham số ZTI (chọn OS version/build/edition/language/license một lần, sau đó không chạm nữa) — khớp triết lý "bấm một lần" của đặc tả §0. • Layout chia đĩa GPT chuẩn (EFI/MSR/Windows/Recovery) làm chuẩn cho winpe_install. • Inject driver offline SAU apply-image và TRƯỚC reboot (đặc tả §6 đã có, giữ nguyên và coi đây là chuẩn). • Copy `unattend.xml` vào `C:\Windows\Panther\` trước reboot (đặc tả §6 đã có). • WinPE tự chạy engine ngay khi boot (startnet hook) — ta đã có qua winpeshl/startnet. • Ý tưởng catalogue image dạng dữ liệu (JSON) — đặc tả §3 đã có, giữ nguyên. | • Runtime PowerShell trong WinPE (ta là native Rust, không kéo theo PS + module). • Autopilot/Entra ID join, OSDCloudGUI. • Telemetry "deployment analytics" mà OSDCloud gửi đi (repo có PRIVACY.md thừa nhận việc này) — bản rebuild không gửi telemetry, đúng chính sách ở §9/§10 đặc tả. | OSDCloud là chuẩn thực tế được cộng đồng OSD dùng rộng rãi nhất; workflow của nó chứng minh hướng staging + DISM + unattend của ta là đúng đường. Khác biệt cốt lõi chỉ ở runtime (PS vs Rust) và nguồn image (catalogue Microsoft vs kho ISO của người dùng). |
| **Schneegans' Unattend Generator** (Christoph Schneegans) | Repo: https://github.com/cschneegans/unattend-generator · Bản web: schneegans.de/windows/unattend-generator | Thư viện .NET (chính là engine của trang web schneegans.de) sinh `autounattend.xml` tuỳ biến sâu cho Win10/11: tài khoản local, AutoLogon, ẩn toàn bộ OOBE, bypass yêu cầu Win11, script ở các pass specialize/first-logon, nhúng file phụ vào XML. | **MIT** [ĐÃ KIỂM CHỨNG trên trang repo]. | • Cấu trúc XML theo pass `windowsPE` / `specialize` / `oobeSystem` và cách đặt các component `Microsoft-Windows-Shell-Setup` (UserAccounts, AutoLogon, OOBE, FirstLogonCommands) — xem §2.1. • Mẫu AutoLogon: `<AutoLogon>` với `LogonCount` giới hạn + agent tự tắt AutoLogon khi hoàn tất (thiết kế của ta, cùng triết lý với bản sinh của Schneegans). • Tuỳ chọn "obscure password bằng Base64" trong unattend — ta áp dụng cho mọi mật khẩu ghi vào XML (lưu ý: đây là mã hoá che mắt, KHÔNG phải mã hoá bảo mật — xem §2.5). • Cơ chế nhúng file (Extensions `<File>` + script trích xuất chạy ở pass specialize) — ta KHÔNG cần nhúng vào XML vì winpe_install copy file trực tiếp, nhưng giữ nguyên tắc: file hậu cài phải nằm sẵn ở `C:\Windows\Setup\Scripts\` và `C:\ProgramData\QSU\` trước reboot đầu tiên. • Tuỳ chọn sinh script cho giai đoạn PE (partition + apply + copy unattend vào Panther + `wpeutil reboot`) — khớp gần như 1-1 với engine WinPE ở đặc tả §6. | • Các tweak giao diện/debloat (taskbar, Start, gỡ app inbox…) — thuộc phạm vi module Tuỳ Chỉnh Windows, không thuộc module này. • Bản web (ta tự sinh XML trong wizard bằng code Rust). | Đây là nguồn tham chiếu tốt nhất hiện nay cho autounattend.xml "đúng chuẩn Microsoft mà vẫn thực dụng": mọi thành phần §2 đều đối chiếu được với file do generator này sinh ra. License MIT cho phép tham khảo cấu trúc tự do. |
| **Dism++** (Chuyu-Team) | Repo hỗ trợ/đa ngôn ngữ + phát hành: https://github.com/Chuyu-Team/Dism-Multi-language (repo này KHÔNG chứa source của app — chỉ là resource/localization/báo lỗi; org: https://github.com/Chuyu-Team) | GUI trên nền DISM API (CBS) của Microsoft: quản lý image offline/online (apply/export/convert WIM-ESD-SWM, giải mã ESD), thêm/bớt/export driver vào image offline, sửa BCD/boot, WIMBoot/CompactOS, backup/restore hệ thống không cần vào PE. | App là **freeware đóng nguồn** (repo GitHub đi kèm là MIT nhưng chỉ cho phần resource). Không nhúng, không phân phối lại. | Chỉ lấy làm **chuẩn hành vi** để đối chiếu tính năng: (a) quản lý driver trên image offline + export driver từ máy đang chạy (khớp module Backup Driver và bước inject ở §6); (b) có đường sửa BCD dự phòng khi `bcdboot` lỗi; (c) hỗ trợ ESD đầy đủ (giải mã/convert) — bản ta dùng DISM trực tiếp với install.esd nên chỉ cần ghi nhận là đủ; (d) tuỳ chọn CompactOS khi apply cho máy ổ nhỏ (đưa vào wizard dạng tuỳ chọn nâng cao). | • Nhúng binary Dism++ vào bộ cài (không được phép + không cần). • Các tính năng cleanup/tweak (ngoài phạm vi). | Dism++ cho thấy bộ tính năng DISM "chuẩn kỹ thuật viên" gồm những gì; ta bám theo danh sách đó để không thiếu tính năng, nhưng triển khai bằng DISM API/lệnh chuẩn trong Rust. |
| **WinNTSetup** (JFX) | Diễn đàn MSFN, topic 149612: https://msfn.org/board/topic/149612-winntsetup-v5352/page/146/ (không có repo GitHub) | Công cụ cài Windows từ WinPE tối giản, không cần setup.exe: apply WIM/ESD/SWM, tích hợp driver PnP + textmode vào driver store, nhận unattend.xml, cài vào VHD, chạy được hoàn toàn bằng command line + file ini (`WinNTSetup_x64.exe NT6 -source:<iso> -wimindex:<n> -unattend:<xml> -drivers:<folder> -syspart:<part> -tempdrive:<drv> -Sysletter:C -RunAfter:<cmd> -setup -reboot`). | **Freeware đóng nguồn**, phát hành qua MSFN/Mega/MediaFire. Không nhúng, không phân phối lại. | • Mô hình tham số CLI của engine WinPE: tách bạch `-source / -wimindex / -unattend / -drivers / -syspart / -tempdrive / -RunAfter` — áp dụng làm giao diện dòng lệnh nội bộ cho crate `winpe_install` (config file thay ini). • Khái niệm **RunAfter**: hook chạy ngay sau khi apply image xong, còn trong WinPE (ta dùng để copy payload hậu cài + ghi log + kiểm tra lại BCD). • Cảnh báo quan trọng từ chính tài liệu WinNTSetup: khi cài kiểu apply-image (không qua setup.exe), **các setting thuộc pass `windowsPE` trong unattend.xml sẽ KHÔNG được áp dụng** — chỉ `specialize`/`oobeSystem` từ bản copy ở Panther mới có hiệu lực. Đặc tả của ta phải ghi rõ điều này (xem delta §3). | • Nhúng binary (đóng nguồn). • VHD/Windows-To-Go, cài NT5 (XP) — ngoài phạm vi. | WinNTSetup là bằng chứng lâu năm rằng apply-image thuần DISM + unattend ở Panther là đường cài hợp lệ và ổn định; mô hình CLI của nó là khuôn tốt cho engine WinPE của ta. |
| **UltraViewer** (DucFabulous) | Trang tải chính thức: https://www.ultraviewer.net/en/download.html · Bài triển khai GPO chính thức: https://www.ultraviewer.net/EN/200000186-deploying-software-installations-through-group-policy-object-gpo-without-msi-file.html · Hướng dẫn unattended chính thức: https://www.ultraviewer.net/EN/200000171-ultraviewer-unattended-access-a-step-by-step-guide.html | Phần mềm điều khiển từ xa (mục tiêu hậu cài số 1). | Phần mềm thương mại (có bản free giới hạn tính năng); chỉ tải từ nguồn chính thức và cài, không phân phối lại ngoài việc tải trực tiếp. | Toàn bộ §2.3: bộ cài Inno Setup + switch silent chính thức, dịch vụ Windows đi kèm, cơ chế Custom Password cho unattended. | • Bản portable (không cài service → không unattended thật). • winget `DucFabulous.UltraViewer` / Chocolatey `ultraviewer` làm đường chính (manifest do cộng đồng duy trì — chỉ dùng làm phương án dự phòng khi không tải được từ ultraviewer.net). | Xem chi tiết và mức độ kiểm chứng ở §2.3. |
| **Google Chrome Enterprise** | Trang tải enterprise: chromeenterprise.google (mục download) · Trang tổng hợp file + SHA-256 theo manifest của Google: https://file.wiki/software/chrome · Hướng dẫn triển khai MSI: https://techygeekshome.info/google-chrome-msi-installers/ | Trình duyệt (mục tiêu hậu cài số 2). | Phần mềm miễn phí của Google; MSI enterprise được phép triển khai trong tổ chức. | Toàn bộ §2.4: MSI `GoogleChromeStandaloneEnterprise64.msi` + `msiexec /qn /norestart`, hoặc winget `Google.Chrome`. | • Bản cài consumer (stub online — không dùng được offline, hành vi không đoán trước trong script). | MSI enterprise là đường triển khai chuẩn duy nhất nên dùng cho cài im lặng hàng loạt. |

### Kết luận khảo sát

- **Workflow**: bám OSDCloud (staging + DISM + driver offline + unattend ở Panther)
  và mô hình CLI của WinNTSetup cho engine WinPE.
- **Unattend**: bám cấu trúc của Schneegans' generator (MIT) — đây là khuôn
  XML cho §2.1.
- **Hậu cài**: bám cơ chế chuẩn của Windows (`SetupComplete.cmd` +
  `FirstLogonCommands` + Scheduled Task retry) thay vì tự chế giao thức mới.

---

## 2. Thiết kế hậu cài tích hợp

### 2.1. Thành phần unattend (autounattend.xml do wizard sinh)

Wizard sinh XML theo đúng khung của Schneegans' generator, gồm các pass:

**Pass `windowsPE`** (chỉ có hiệu lực ở đường cài đè §5 — đường apply-image
§6 bỏ qua pass này, xem cảnh báo WinNTSetup ở §1):

- `Microsoft-Windows-International-Core-WinPE`: ngôn ngữ vi-VN (mặc định),
  bàn phím theo lựa chọn ở wizard.
- `Microsoft-Windows-Setup`: chọn image index đã chốt ở đặc tả §4 (khi đi
  đường setup.exe), `UserData/AcceptEula=true`.

**Pass `specialize`**:

- `Microsoft-Windows-Shell-Setup`: `ComputerName` (giữ tên cũ hoặc theo mẫu),
  `TimeZone` = `SE Asia Standard Time`, `RegisteredOwner` (tuỳ chọn).
- (Tuỳ chọn) `Microsoft-Windows-Deployment/RunSynchronous`: chỉ dùng nếu
  cần thao tác cấp máy trước OOBE — mặc định để trống, việc cấp máy đã có
  SetupComplete lo (xem §2.2).

**Pass `oobeSystem`** — phần quan trọng nhất cho yêu cầu mới:

- `Microsoft-Windows-International-Core`: locale vi-VN.
- `Microsoft-Windows-Shell-Setup`:
  - `OOBE`: `HideEULAPage=true`, `HideOEMRegistrationScreen=true`,
    `HideOnlineAccountScreens=true`, `HideWirelessSetupInOOBE=true`
    (nếu máy dùng LAN; nếu chỉ có Wi-Fi thì KHÔNG ẩn trang Wi-Fi mà để
    wizard ghi sẵn profile Wi-Fi — xem delta §3), `ProtectYourPC=3`,
    `SkipMachineOOBE`/`SkipUserOOBE` chỉ dùng thận trọng (Microsoft đã
    deprecate — ưu tiên các cờ Hide* như generator của Schneegans).
  - `UserAccounts/LocalAccounts`: tạo tài khoản quản trị đầu tiên (tên +
    mật khẩu từ wizard; mật khẩu ghi dạng Base64-obscured,
    `PlainText=false`).
  - **`AutoLogon`** (yêu cầu mới #1):
    ```xml
    <AutoLogon>
      <Username><tai-khoan-quan-tri></Username>
      <Password><Value>***</Value><PlainText>false</PlainText></Password>
      <Enabled>true</Enabled>
      <LogonCount>3</LogonCount>
    </AutoLogon>
    ```
    `LogonCount=3` là số lần tự đăng nhập tối đa — đủ cho agent hậu cài
    chạy + 2 lần reboot/retry dự phòng. Agent khi hoàn tất sẽ tự vô hiệu
    AutoLogon (xoá `DefaultPassword`, đặt `AutoLogonCount=0` — xem §2.2),
    không để máy tự đăng nhập vĩnh viễn.
  - **`FirstLogonCommands`**: đúng MỘT lệnh đồng bộ duy nhất:
    ```xml
    <FirstLogonCommands>
      <SynchronousCommand wcm:action="add">
        <Order>1</Order>
        <CommandLine>C:\ProgramData\QSU\post_install.exe --first-logon</CommandLine>
        <Description>QSU post-install agent</Description>
        <RequiresUserInput>false</RequiresUserInput>
      </SynchronousCommand>
    </FirstLogonCommands>
    ```
    Chỉ một điểm vào duy nhất → toàn bộ logic nằm trong agent (dễ log,
    dễ retry), XML giữ mỏng như cách generator của Schneegans trỏ vào
    `C:\Windows\Setup\Scripts\FirstLogon.ps1`.

### 2.2. SetupComplete.cmd vs FirstLogonCommands — phân vai

| Tiêu chí | `C:\Windows\Setup\Scripts\SetupComplete.cmd` | `FirstLogonCommands` (oobeSystem) |
|---|---|---|
| Thời điểm chạy | Sau khi setup/OOBE hoàn tất, TRƯỚC màn hình đăng nhập lần đầu | Khi tài khoản admin đăng nhập lần đầu (ở ta = lần AutoLogon đầu tiên) |
| Quyền/bối cảnh | LocalSystem, chưa có profile người dùng | Người dùng đang đăng nhập (admin, đã nâng quyền qua manifest của agent) |
| Mạng | Có thể chưa sẵn sàng hoàn toàn (driver NIC có thể vừa được nhận) | Thường đã có mạng (NIC + DHCP đã chạy trong phiên người dùng) |
| Hạn chế đã biết | Theo tài liệu Microsoft về Windows Setup scripts, SetupComplete.cmd bị bỏ qua trong một số kịch bản dùng OEM product key trên Windows 10/11 [TIN CẬY CAO — phải kiểm chứng trên máy test với đúng loại key của người dùng] | Cần có logon xảy ra — ở ta được đảm bảo bằng AutoLogon |
| Vai trò trong thiết kế này | **Chỉ làm việc cấp máy, không cần mạng, không cần user**: (1) đảm bảo Scheduled Task `QSU-PostInstall` tồn tại (tạo lại nếu thiếu), (2) ghi mốc "setup-complete" vào state file, (3) xoá bản staging của `autounattend.xml` nếu còn sót ngoài Panther. Do winpe_install đặt sẵn file này cùng payload (xem §2.6), KHÔNG phụ thuộc việc Windows có tự chạy nó hay không — kể cả khi nó bị bỏ qua, FirstLogonCommands vẫn là đường chính. | **Đường chính**: khởi chạy agent hậu cài (cài app, cấu hình, báo cáo). Mọi việc cần mạng/user đều ở đây. |

Ngoài hai cơ chế trên, agent tự trang bị thêm **Scheduled Task
`QSU-PostInstall`** (trigger: AtLogOn của tài khoản quản trị + lặp lại sau
mỗi lần khởi động khi state chưa "done") làm đường retry — xem §2.5. Ba
lớp này chồng nhau có chủ đích: mất một lớp vẫn còn lớp sau.

### 2.3. UltraViewer — cài im lặng + unattended access

**Nguồn tải** [ĐÃ KIỂM CHỨNG]: trang download chính thức
`ultraviewer.net/en/download.html`, file cài dạng Inno Setup
(`UltraViewer_setup_en.exe`; một số bản đặt tên kèm phiên bản, ví dụ
`UltraViewer_setup_6.6_en.exe`). Nhà phát hành **không cung cấp MSI**
(trang GPO chính thức xác nhận điều này).

**Switch cài im lặng** [ĐÃ KIỂM CHỨNG — trích từ bài hướng dẫn GPO trên
chính ultraviewer.net]:

```
UltraViewer_setup_en.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART
```

- `/VERYSILENT`: không wizard, không cửa sổ tiến trình (`/SILENT` nếu
  muốn hiện tiến trình).
- `/SUPPRESSMSGBOXES`: không hộp thoại nào được phép chặn tiến trình.
- `/NORESTART`: không tự khởi động lại máy.
- Lưu ý của chính nhà phát hành: switch sai chính tả sẽ bị Inno Setup
  **bỏ qua lặng lẽ** và wizard hiện ra như cài thường → agent PHẢI kiểm
  tra kết quả bằng success-check (mục dưới), không tin vào exit code suông.
- Yêu cầu quyền admin (cài vào Program Files + đăng ký Windows service);
  agent chạy bằng tài khoản quản trị qua AutoLogon nên đạt. Dùng bản
  6.6.70 trở lên (bản cũ từng kèm phần mềm bundle khi cài silent — chính
  changelog của hãng ghi nhận đã sửa ở 6.6.70).
- Đường dự phòng: `winget install --id DucFabulous.UltraViewer -e --silent`
  [TIN CẬY CAO — gói tồn tại trên winget/Chocolatey, manifest do cộng đồng
  duy trì] — chỉ dùng khi tải trực tiếp thất bại, và vẫn phải qua
  success-check + kiểm SHA-256 nếu có.

**Tự chạy / service** [ĐÃ KIỂM CHỨNG một phần]:

- Setup tự đăng ký một **Windows service** cho UltraViewer (bài GPO chính
  thức nêu rõ việc này; tên service theo dữ liệu gỡ-cài-đặt của bên thứ
  ba là `UltraViewService`, file `UltraViewer_Service.exe` trong
  `C:\Program Files (x86)\UltraViewer` [TIN CẬY CAO]).
- Tuỳ chọn "Run UltraViewer with Windows" (Settings → Options → Access)
  bật/tắt việc chạy cùng Windows ở tầng ứng dụng — với service đã chạy,
  máy có thể nhận kết nối ngay cả khi chưa ai đăng nhập.
- Việc agent phải làm sau khi cài: kiểm tra service tồn tại, đặt
  `StartupType=Automatic` (nếu chưa), start service, và xác nhận tiến
  trình/service đang Running trước khi ghi "done" cho bước này.

**Mật khẩu unattended (Custom Password)** — phần phải nói thẳng:

- Cơ chế chính thức [ĐÃ KIỂM CHỨNG]: trong giao diện UltraViewer, bấm
  biểu tượng chìa khoá vàng tại mục **Custom Password** ("Private custom
  password") để đặt mật khẩu cố định cho máy; kết hợp "Run UltraViewer
  with Windows"/service → truy cập không cần người trực. Đặt mật khẩu
  này đòi hỏi quyền Administrator.
- **KHÔNG tìm thấy phương thức chính thức nào để đặt trước mật khẩu này
  bằng command line / registry / file cấu hình** trong tài liệu của hãng
  (trang GPO chỉ hướng dẫn cài silent, không nhắc cấu hình mật khẩu).
  [Kết luận âm tính — đã kiểm chứng trong phạm vi tài liệu công khai
  của ultraviewer.net tính đến 2026-11-16.]
- Dữ liệu phụ [CHƯA KIỂM CHỨNG]: các bản ghi gỡ-cài-đặt của bên thứ ba
  cho thấy UltraViewer lưu cấu hình tại `HKLM\Software\UltraViewer`,
  `HKCU\Software\VB and VBA Program Settings\UltraViewer_Desktop`, và
  thư mục `%AppData%\Roaming\UltraViewer\` (các file `Reg.ini`,
  `Reg2.ini`, `Reg3.ini`, `TempPass.ini`). Chưa rõ mật khẩu custom nằm
  ở đâu trong số này, ở dạng nào, và có mang sang máy khác được không.
- Manh mối phụ [CHƯA KIỂM CHỨNG]: changelog 6.6.81 của hãng nhắc tới một
  "command -silentinstall" từng lỗi khi không có mạng — có thể tồn tại
  tham số dòng lệnh chưa được tài liệu hoá; cần test thực tế.

→ **Thiết kế cho bước mật khẩu (bắt buộc theo đúng mức kiểm chứng này):**

1. Agent cài UltraViewer + bật service như trên (phần đã kiểm chứng —
   làm mặc định).
2. Bước "đặt mật khẩu unattended" được cài sau một **cờ tính năng trong
   settings** (`ultraviewer_password_mode`):
   - `manual` (MẶC ĐỊNH cho tới khi kiểm chứng xong): agent không tự đặt
     mật khẩu; báo cáo hoàn tất ghi rõ trạng thái "UltraViewer: đã cài,
     service đang chạy, **chưa đặt mật khẩu unattended**" + ID của máy
     nếu đọc được. Kỹ thuật viên đặt mật khẩu trong phiên remote đầu
     tiên (dùng mật khẩu ngẫu nhiên hiển thị trên UltraViewer, hoặc có
     mặt tại máy).
   - `seed-config` (CHỈ bật sau khi hoàn thành nhiệm vụ kiểm chứng ở
     §3): agent chép bộ file/registry cấu hình mẫu (đã tạo sẵn trên một
     máy chuẩn bằng cách đặt mật khẩu qua GUI) vào đúng vị trí, restart
     service, rồi tự kiểm tra lại.
3. Nhiệm vụ kiểm chứng (đưa vào §3): trên máy test, đặt Custom Password
   qua GUI → chụp snapshot registry + thư mục cấu hình trước/sau → xác
   định chính xác khoá/file chứa mật khẩu và tính mang-sang-máy-khác →
   chỉ khi tái lập thành công trên máy thứ hai mới được bật `seed-config`.

### 2.4. Chrome — cài im lặng

[ĐÃ KIỂM CHỨNG]

- Gói chuẩn: **MSI enterprise 64-bit** `GoogleChromeStandaloneEnterprise64.msi`
  từ trang Chrome Enterprise (bản standalone đầy đủ, cài cho toàn máy,
  không cần mạng lúc cài; Google công bố SHA-256 theo manifest của từng
  bản phát hành).
- Lệnh cài:
  ```
  msiexec /i GoogleChromeStandaloneEnterprise64.msi /qn /norestart /L*v C:\ProgramData\QSU\logs\chrome-msi.log
  ```
  Exit code `0` = thành công, `3010` = thành công nhưng cần reboot (agent
  ghi nhận là thành công, KHÔNG tự reboot giữa chừng).
- Phương án thay thế: `winget install --id Google.Chrome -e --silent
  --accept-package-agreements --accept-source-agreements` (winget tự chọn
  đúng kiến trúc; gói `Google.Chrome` cài chính MSI machine-wide của
  Google) — dùng khi không mang theo MSI trong staging.
- Success-check: tồn tại `C:\Program Files\Google\Chrome\Application\chrome.exe`
  và đọc được version từ registry `HKLM\Software\Google\Update\Clients`
  hoặc từ file version của thư mục cài.
- KHÔNG dùng bản cài consumer (stub tải-thêm-khi-cài): không chạy offline
  được và hành vi trong script không đoán trước được.

### 2.5. Thứ tự chạy, log, xử lý lỗi

**Thứ tự (trong agent, mỗi bước ghi state riêng):**

```
0. Đọc state file + secrets (xem §2.6) — bước nào "done" thì bỏ qua.
1. Chờ mạng: poll kết nối (thử phân giải DNS + HTTPS tới nguồn tải)
   tối đa 5 phút; hết giờ vẫn tiếp tục với những gì đã có trong staging.
2. Khôi phục driver từ backup (đặc tả §7 bước 1 — giữ nguyên, làm trước
   để NIC/thiết bị ổn định sớm nhất có thể).
3. Cài UltraViewer (từ staging nếu có, không thì tải) → success-check
   → cấu hình service → (đặt mật khẩu nếu mode cho phép).
4. Cài Chrome (MSI từ staging nếu có, không thì tải/winget) → success-check.
5. Áp profile Tuỳ Chỉnh Windows nếu được chọn (đặc tả §7 bước 2 — giữ nguyên).
6. Hoàn tất: vô hiệu AutoLogon, xoá secrets an toàn, xoá staging,
   gỡ Scheduled Task + RunOnce, viết báo cáo hoàn tất (đặc tả §7 bước 4)
   + bổ sung: trạng thái từng app, UltraViewer ID (nếu đọc được),
   trạng thái mật khẩu unattended.
```

**Log:** một file log duy nhất cho hậu cài tại
`C:\ProgramData\QSU\logs\post-install.log` (và bản sao cuối cùng chép về
`<Data>:\QSU-Logs\` như đặc tả §8). Mỗi dòng có timestamp + tên bước +
kết quả. **Tuyệt đối không log mật khẩu** (kể cả dạng che một phần) —
chỉ log "đã nạp secrets: có/không".

**Xử lý lỗi / retry (yêu cầu: máy offline, tải hỏng → thử lại lần đăng
nhập sau):**

- Mỗi app có trạng thái riêng trong state file JSON:
  `pending → downloading → installing → verifying → done | failed`.
- Lỗi tải (mất mạng, timeout, sai SHA-256): xoá file dở, đánh dấu app
  `pending` kèm `attempts+1` và lý do; agent **đăng ký lại chính nó**
  bằng cả hai đường (RunOnce `QSU-PostInstall` + Scheduled Task
  `QSU-PostInstall` trigger AtLogOn) rồi thoát êm — lần đăng nhập sau
  (AutoLogon vẫn còn lượt, hoặc người dùng đăng nhập tay) agent chạy
  tiếp đúng bước dở dang.
- Một app `failed` dứt điểm (installer trả lỗi lạ sau 3 lần thử) KHÔNG
  chặn các app còn lại; báo cáo cuối ghi rõ app nào failed + mã lỗi.
- Trần retry: tối đa 5 lần đăng nhập liên tiếp cho mỗi app; quá trần thì
  dừng retry, giữ nguyên báo cáo lỗi trên ổ Data để kỹ thuật viên xử lý
  — tránh vòng lặp cài-hỏng vĩnh viễn trên máy người dùng.
- Toàn bộ hậu cài chỉ được coi là "done" khi mọi app bắt buộc ở trạng
  thái done; khi đó mới chạy bước 6 (tắt AutoLogon, dọn dẹp).

### 2.6. Bí mật (secrets) — nguồn gốc và vòng đời

Áp dụng cho: mật khẩu tài khoản quản trị/AutoLogon và mật khẩu
unattended của UltraViewer.

- **Nguồn duy nhất**: file settings phía kỹ thuật viên (cùng nơi chứa
  catalogue URL ở đặc tả §3) / nhập tại wizard lúc chuẩn bị cài. Không
  hard-code trong binary, không để trong repo, không dùng chung một mật
  khẩu cho mọi máy nếu settings không yêu cầu tường minh (wizard nên
  hỗ trợ biến mẫu, ví dụ hậu tố theo tên máy — tuỳ chọn, mặc định tắt).
- **Đường đi**: wizard ghi mật khẩu AutoLogon vào `autounattend.xml`
  (Base64-obscured — chỉ là che mắt: bất kỳ ai đọc được file XML đều
  khôi phục được, phải đối xử với file này như chính mật khẩu) và ghi
  cả hai mật khẩu vào `secrets.json` đặt trong payload tại
  `C:\ProgramData\QSU\secrets.json` với ACL chỉ SYSTEM + Administrators.
- **Vòng đời**: winpe_install chỉ *copy* payload, không đọc nội dung
  secrets. Agent đọc secrets khi cần, **không bao giờ ghi ra log**.
  Ngay khi hậu cài done (hoặc failed dứt điểm), agent ghi đè + xoá
  `secrets.json`, xoá bản `autounattend.xml` trong staging; bản ở
  `C:\Windows\Panther\unattend.xml` do Windows quản lý (Windows tự che
  các trường mật khẩu sau cài — không dựa vào việc này, đó chỉ là lớp
  phụ).
- Báo cáo hoàn tất chỉ ghi **trạng thái** ("mật khẩu unattended: đã đặt /
  chưa đặt"), không ghi giá trị.

---

## 3. Checklist delta — áp vào đặc tả logic §6–§7 và các crate Rust

Đánh dấu: [LOGIC] sửa tài liệu `logic_cai_dat_win_online.md`,
[WIZ] crate wizard, [WPE] crate `winpe_install`, [POST] crate
`post_install`.

### Delta cho đặc tả logic

- [ ] [LOGIC] §6: thêm ghi chú chuẩn WinNTSetup — đường apply-image bỏ
      qua pass `windowsPE` của unattend; chỉ `specialize`/`oobeSystem`
      trong bản ở `C:\Windows\Panther\unattend.xml` có hiệu lực. Mọi
      thiết lập cần cho đường §6 phải nằm ở hai pass này.
- [ ] [LOGIC] §6 bước 4: bổ sung bước "copy payload hậu cài" vào danh
      sách việc trong WinPE: `SetupComplete.cmd`, `post_install.exe`,
      `apps.json` (§4), `secrets.json`, thư mục `payload\` (installer
      mang sẵn nếu có) → đích `C:\Windows\Setup\Scripts\` và
      `C:\ProgramData\QSU\`.
- [ ] [LOGIC] §6: thêm bước kiểm tra sau apply (RunAfter kiểu WinNTSetup):
      xác nhận `bcdboot` thành công + file unattend đã ở Panther + payload
      đủ file; thiếu bất kỳ mục nào thì ghi log lỗi rõ ràng TRƯỚC khi
      reboot (đây là cơ hội sửa cuối cùng còn rẻ).
- [ ] [LOGIC] §7: thay toàn bộ nội dung "File unattend" bằng cấu trúc ở
      §2.1 của tài liệu này (thêm `AutoLogon` với `LogonCount`, `OOBE`
      đầy đủ cờ Hide*, `FirstLogonCommands` một lệnh duy nhất).
- [ ] [LOGIC] §7: thêm mục "Danh sách app hậu cài" trỏ tới `apps.json`
      (§4) với hai app mặc định UltraViewer + Chrome, và mục "Bí mật"
      theo §2.6.
- [ ] [LOGIC] §7: viết lại phần agent hậu cài theo thứ tự 6 bước ở §2.5
      (thay 4 bước hiện tại), gồm cơ chế state file + retry ở lần đăng
      nhập sau.
- [ ] [LOGIC] §9 [VERIFY]: thêm mục 7 — "Phương thức đặt trước mật khẩu
      unattended UltraViewer" (nhiệm vụ kiểm chứng ở §2.3) và mục 8 —
      "SetupComplete.cmd có bị bỏ qua với loại key đang dùng không".

### Delta cho crate wizard (sinh unattend + chuẩn bị payload)

- [ ] [WIZ] Sinh `AutoLogon` (username = tài khoản quản trị vừa tạo,
      `LogonCount=3`, password Base64-obscured, `PlainText=false`).
- [ ] [WIZ] Sinh `OOBE` đầy đủ: HideEULAPage, HideOEMRegistrationScreen,
      HideOnlineAccountScreens, ProtectYourPC=3; `HideWirelessSetupInOOBE`
      chỉ đặt `true` khi máy có LAN hoặc wizard đã ghi sẵn profile Wi-Fi.
- [ ] [WIZ] Sinh `FirstLogonCommands` đúng một lệnh trỏ tới
      `C:\ProgramData\QSU\post_install.exe --first-logon`.
- [ ] [WIZ] Thêm màn hình/tuỳ chọn hậu cài: danh sách app (mặc định tick
      UltraViewer + Chrome), nguồn installer (mang sẵn trong staging /
      tải khi hậu cài), `ultraviewer_password_mode` (manual/seed-config),
      nhập mật khẩu unattended (trường password, không echo ra log/tóm
      tắt xác nhận — màn hình xác nhận cuối ở đặc tả §8 chỉ ghi "có đặt
      mật khẩu: có/không").
- [ ] [WIZ] Xuất `apps.json` (§4) + `secrets.json` vào staging; đặt cờ
      "file nhạy cảm" để các bước sau xử lý đúng vòng đời §2.6.
- [ ] [WIZ] Cho phép đóng gói sẵn file cài UltraViewer/Chrome MSI vào
      `payload\` khi kỹ thuật viên có sẵn file (kèm kiểm SHA-256 lúc đóng
      gói) — hậu cài ưu tiên dùng bản này, tải về chỉ là dự phòng.

### Delta cho crate winpe_install (staging payload)

- [ ] [WPE] Sau apply-image + inject driver: copy `autounattend.xml` →
      `C:\Windows\Panther\unattend.xml` (như hiện tại) VÀ payload hậu cài
      → `C:\Windows\Setup\Scripts\SetupComplete.cmd`,
      `C:\ProgramData\QSU\{post_install.exe, apps.json, secrets.json,
      payload\}`; đặt ACL cho `secrets.json` (SYSTEM + Administrators).
- [ ] [WPE] Sinh `SetupComplete.cmd` với đúng 3 việc ở §2.2 (đảm bảo
      Scheduled Task tồn tại — thực chất là gọi
      `post_install.exe --ensure-task`, ghi mốc state, dọn bản unattend
      staging); nội dung file do wizard quyết định, winpe_install chỉ copy.
- [ ] [WPE] Bước kiểm tra hậu-apply (RunAfter): verify BCD entry boot
      được, unattend ở đúng chỗ, payload đủ theo manifest (danh sách file
      + kích thước); ghi kết quả vào log trên ổ Data trước reboot.
- [ ] [WPE] Không đọc/giải mã nội dung `secrets.json` ở bất kỳ đâu trong
      code WinPE (chỉ copy + đặt ACL).

### Delta cho crate post_install (agent)

- [ ] [POST] Ba điểm vào: `--first-logon` (từ FirstLogonCommands),
      `--ensure-task` (từ SetupComplete.cmd), `--scheduled` (từ
      Scheduled Task/RunOnce khi retry). Cả ba hội tụ vào một engine
      state-machine duy nhất.
- [ ] [POST] State file `C:\ProgramData\QSU\state.json`: trạng thái từng
      bước/app + số lần thử + lý do lỗi cuối; đọc-ghi an toàn (ghi file
      tạm rồi rename).
- [ ] [POST] Bước chờ mạng (poll tối đa 5 phút) trước các bước cần tải;
      quá giờ thì dùng staging, phần thiếu để retry.
- [ ] [POST] Cài UltraViewer: lệnh silent ở §2.3 + success-check kép
      (thư mục cài tồn tại VÀ service `UltraViewService` tồn tại) →
      đặt service Automatic + start + verify Running.
- [ ] [POST] Bước mật khẩu UltraViewer sau cờ `ultraviewer_password_mode`
      (§2.3); ở mode `manual`, ghi trạng thái "chưa đặt" vào báo cáo và
      (nếu đọc được) kèm ID máy.
- [ ] [POST] Cài Chrome: lệnh msiexec ở §2.4 (+ /L*v log riêng), chấp
      nhận exit 0/3010, success-check bằng chrome.exe + version.
- [ ] [POST] Cơ chế retry: khi còn app chưa done và chưa quá trần 5 lần
      → đăng ký RunOnce + đảm bảo Scheduled Task `QSU-PostInstall`
      (trigger AtLogOn, quyền cao nhất, user = tài khoản quản trị) rồi
      thoát với state pending.
- [ ] [POST] Bước hoàn tất: tắt AutoLogon (xoá `DefaultPassword` +
      `AutoLogonCount=0` tại `HKLM\SOFTWARE\Microsoft\Windows NT\
      CurrentVersion\Winlogon`), ghi đè + xoá `secrets.json`, xoá
      RunOnce + Scheduled Task của chính mình, xoá staging (giữ log),
      viết báo cáo hoàn tất mở rộng (trạng thái từng app, ID UltraViewer
      nếu có, trạng thái mật khẩu).
- [ ] [POST] Redaction: mọi đường log đều qua một hàm lọc, cấm ghi giá
      trị của bất kỳ trường nào trong `secrets.json`.

### Nhiệm vụ kiểm chứng (làm trên máy test, không phải code)

- [ ] [TEST] UltraViewer: đặt Custom Password qua GUI trên máy chuẩn →
      diff registry (`HKLM\Software\UltraViewer`, HKCU VB/VBA settings)
      và thư mục `%AppData%\Roaming\UltraViewer\` trước/sau → xác định
      nơi lưu mật khẩu; thử mang cấu hình sang máy thứ hai + restart
      service → kết nối thử bằng mật khẩu đó. Chỉ khi qua được mới bật
      `seed-config`.
- [ ] [TEST] UltraViewer: thử tham số `-silentinstall` được changelog
      6.6.81 nhắc tới trên bản cài hiện tại; ghi lại hành vi thực tế.
- [ ] [TEST] Kiểm tra SetupComplete.cmd có chạy với loại product key mà
      người dùng thực tế dùng (OEM/digital license) không; nếu bị bỏ
      qua, xác nhận FirstLogonCommands + Scheduled Task vẫn gánh đủ.
- [ ] [TEST] Chạy thử trọn luồng trên máy không mạng: cài từ staging
      phải xong UltraViewer + Chrome; app thiếu file phải chuyển sang
      retry và tự hoàn tất ở lần đăng nhập sau khi có mạng.

---

## 4. Cấu trúc danh sách app hậu cài mặc định (apps.json)

Nguyên tắc: một app = một mục JSON; thêm app mới = thêm mục, không sửa
code. Agent xử lý theo `installer_type` và `success_check` khai báo sẵn.
Installer được tìm theo thứ tự: (1) file trong `payload\` của staging,
(2) tải từ `url` (kiểm `sha256` bắt buộc nếu có khai).

```json
{
  "version": 1,
  "settings": {
    "network_wait_seconds": 300,
    "max_logon_attempts_per_app": 5,
    "log_dir": "C:\\ProgramData\\QSU\\logs"
  },
  "apps": [
    {
      "id": "ultraviewer",
      "name": "UltraViewer",
      "required": true,
      "source": {
        "file_name": "UltraViewer_setup_en.exe",
        "url": "https://www.ultraviewer.net/en/download.html",
        "sha256": null,
        "note": "URL la trang tai chinh thuc; wizard nen dong goi san file vao payload va dien sha256 tai thoi diem dong goi. Nha phat hanh khong cung cap MSI."
      },
      "installer_type": "inno",
      "silent_args": ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"],
      "success_check": {
        "type": "service_and_file",
        "service_name": "UltraViewService",
        "file": "C:\\Program Files (x86)\\UltraViewer\\UltraViewer_Desktop.exe"
      },
      "post_config": {
        "service_startup": "automatic",
        "service_start": true,
        "unattended_password": {
          "mode_setting": "ultraviewer_password_mode",
          "secret_key": "ultraviewer_password",
          "note": "Chua co phuong thuc chinh thuc de dat truoc; xem muc 2.3 cua tai lieu tich hop. Mac dinh mode=manual."
        }
      }
    },
    {
      "id": "google-chrome",
      "name": "Google Chrome",
      "required": true,
      "source": {
        "file_name": "GoogleChromeStandaloneEnterprise64.msi",
        "url": "https://chromeenterprise.google/browser/download/",
        "sha256": null,
        "note": "Tai MSI enterprise 64-bit tu Chrome Enterprise; dien sha256 theo manifest cua ban phat hanh tai thoi diem dong goi."
      },
      "installer_type": "msi",
      "silent_args": ["/qn", "/norestart"],
      "msiexec_log": "chrome-msi.log",
      "accept_exit_codes": [0, 3010],
      "fallback": {
        "type": "winget",
        "command": "winget install --id Google.Chrome -e --silent --accept-package-agreements --accept-source-agreements"
      },
      "success_check": {
        "type": "file",
        "file": "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"
      }
    }
  ]
}
```

Định dạng `secrets.json` đi kèm (chỉ tồn tại trong staging/payload,
vòng đời theo §2.6 — không đưa vào repo, không log):

```json
{
  "admin_password": "***",
  "ultraviewer_password": "***"
}
```

Ví dụ thêm một app trong tương lai (minh hoạ tính mở rộng — chỉ cần
thêm một mục cùng khuôn):

```json
{
  "id": "7zip",
  "name": "7-Zip",
  "required": false,
  "source": { "file_name": "7z-setup.exe", "url": "<url-chinh-thuc>", "sha256": "<64-hex>" },
  "installer_type": "exe",
  "silent_args": ["/S"],
  "success_check": { "type": "file", "file": "C:\\Program Files\\7-Zip\\7zFM.exe" }
}
```

---

## Nguồn tham khảo chính

- OSDCloud/OSD (OSDeploy): https://github.com/OSDeploy/OSDCloud ·
  https://github.com/OSDeploy/OSD ·
  https://www.osdeploy.com/guides/pscloudscript/pscloudscript-examples/osdcloud-live
- Schneegans' Unattend Generator (MIT): https://github.com/cschneegans/unattend-generator
  (kèm các file autounattend.xml mẫu do generator sinh, công khai trên GitHub)
- Dism++ (Chuyu-Team): https://github.com/Chuyu-Team/Dism-Multi-language
- WinNTSetup (JFX/MSFN): https://msfn.org/board/topic/149612-winntsetup-v5352/page/146/
- UltraViewer — triển khai GPO/silent (chính thức):
  https://www.ultraviewer.net/EN/200000186-deploying-software-installations-through-group-policy-object-gpo-without-msi-file.html
- UltraViewer — unattended access (chính thức):
  https://www.ultraviewer.net/EN/200000171-ultraviewer-unattended-access-a-step-by-step-guide.html
- Chrome Enterprise MSI: https://file.wiki/software/chrome ·
  https://techygeekshome.info/google-chrome-msi-installers/
