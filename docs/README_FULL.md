# UtilityRebuild — README đầy đủ cho kiểm thử (bản v0.7.13)

> Tài liệu này mô tả toàn bộ dự án **UtilityRebuild** đúng như mã nguồn
> và tài liệu nội bộ tại thời điểm **v0.7.13** (2026-11-17). Đối tượng
> đọc: kỹ thuật viên / người kiểm thử cần chạy thử, đối chiếu hành vi
> thực tế với mô tả, và báo lỗi.
>
> Nguồn sự thật đã dùng để viết: `BUILD_NOTES.md`,
> `docs/logic_cai_dat_win_online.md` (đặc tả thiết kế gốc),
> `docs/co_che_v0.7.13.md`, `docs/kiem_tra_v0.4.md`,
> `docs/kiem_tra_uefi_v0.7.8.md`, `docs/plan_ban_nang_cao.md`,
> các `HUONG-DAN.txt` trong từng gói phát hành, và mã nguồn Rust trong
> `rust/` (crate dùng chung `qsu_common` + 11 crate chương trình).
>
> Quy ước: nội dung nào **chưa từng được chạy kiểm chứng trên máy
> thật** sẽ ghi rõ cụm **"chưa kiểm chứng trên máy thật"**. Mọi tên
> hàm, tên file, hằng số trong tài liệu đều lấy nguyên văn từ mã
> nguồn. Chỗ nào tài liệu cũ mâu thuẫn với mã nguồn hiện tại, mục 5.6
> liệt kê đầy đủ — không tự ý chọn một phía.

---

## 1. Tổng quan

### 1.1. Mục đích dự án

UtilityRebuild là bản **dựng lại độc lập (clean-room)** của một bộ
tiện ích hỗ trợ kỹ thuật Windows gồm **32 chức năng** (bản gốc:
"Quick System Utility v2026.1.3 - Pro"). Bản dựng lại viết hoàn toàn
mới bằng **Rust**, hướng tới tương đương về chức năng phục vụ người
dùng, nhưng **cố ý không tái tạo** các hành vi sau của bản gốc:

- Cổng kiểm tra key bản quyền / kích hoạt online, biến môi trường
  "token" tự khoá module.
- Gửi telemetry (thông tin máy/người dùng) về máy chủ của tác giả.
- Tự cài chứng thư gốc (root CA) của tác giả vào máy.
- Tự thêm ngoại lệ Windows Defender cho chính chương trình.
- Tự cập nhật từ kênh phát hành của tác giả.
- Kích hoạt Windows/Office qua KMS bên thứ ba (chỉ hỗ trợ key chính
  hãng nếu module đó được xây trong tương lai).

Tại v0.7.13, **10 trên 32 nút** của launcher đã có module thật
(9 module người dùng + 1 engine nội bộ `WinPEInstall.exe`). Khoảng
22 nút còn lại hiện thông báo "Chưa có module" — đây là trạng thái
đúng thiết kế, không phải lỗi.

### 1.2. Kiến trúc

```
UtilityRebuild.exe                  Launcher: lưới 32 nút + ô tìm kiếm
└── modules/
    ├── CaiDatWinTuDong.exe         Wizard cài Windows (chạy trên Windows hiện tại)
    ├── CaiDatWinWinPE.exe          Module cài Windows tương tác (chạy trong WinPE)
    ├── BackupRestoreDriver.exe     Sao lưu / khôi phục driver
    ├── TatBitlocker.exe            Tắt BitLocker
    ├── SaoLuuWifi.exe              Sao lưu / quản lý hồ sơ Wi-Fi
    ├── DonRacMayTinh.exe           Dọn rác máy tính
    ├── QuanLyUpdateWindows.exe     Quản lý Windows Update
    ├── PhimTatBIOS.exe             Tra cứu phím tắt BIOS/Boot Menu
    ├── TuyChinhWindows.exe         Tuỳ chỉnh Windows (tweak) + gỡ Appx
    ├── settings.example.json       Mẫu cấu hình (copy thành settings.json để dùng)
    └── install/
        └── WinPEInstall.exe        Engine cài đặt chạy trong WinPE (wizard tự gọi)
```

- **Launcher điều phối các .exe độc lập**: mỗi module là một chương
  trình riêng trong thư mục con `modules\`, tự kiểm tra/nâng quyền
  Admin, tự ghi log, chạy được kể cả khi không qua launcher.
- **Thư viện chung `qsu_common`** (crate Rust, không phải .exe): mọi
  module liên kết tĩnh với nó — log, chạy lệnh, JSON, SHA-256, tải
  file HTTP, cấu hình `settings.json`, hợp đồng `stage.json`, dò
  thông tin hệ thống, quản lý dịch vụ (SCM gốc) và liệt kê/xoá đĩa
  bằng IOCTL. Chi tiết ở mục 2.
- **Cấu hình `settings.json`**: đặt cạnh `CaiDatWinTuDong.exe`
  (trong thư mục `modules\`). Wizard chỉ đọc file này; thiếu file
  thì dùng toàn bộ giá trị mặc định. Mẫu đi kèm gói:
  `settings.example.json` (có cả ở gốc gói và trong `modules\`).
- **Đóng gói**: gói phát hành là một thư mục (nén `.zip`) giải nén
  nguyên vẹn rồi chạy `UtilityRebuild.exe`. Không có trình cài đặt,
  không ghi gì vào hệ thống ngoài các file log cạnh .exe và các thư
  mục làm việc `QSU-*` trên ổ dữ liệu khi cài Windows.

### 1.3. Công nghệ nền

| Hạng mục | Giá trị (theo mã nguồn) |
|---|---|
| Ngôn ngữ | Rust (workspace 12 crate: `qsu_common` + 11 chương trình) |
| Toolchain build | rustc/cargo 1.100.0, target `x86_64-pc-windows-gnu`, cross-build từ Linux bằng mingw-w64 GCC 13 (cấu hình trong `rust/.cargo/config.toml`) |
| GUI | `native-windows-gui` 1.0.13 + `winapi` 0.3 — toàn bộ là control Windows gốc, giao diện sáng chuẩn Windows (từ v0.5); từ v0.7.20 mọi cửa sổ dùng chung một design system trong `qsu_common::ui` (font/lề/header/icon/nút chính) — chuẩn đầy đủ ở `docs/chuan_giao_dien.md` |
| Định dạng .exe | PE32+ x86-64, liên kết tĩnh thư viện chung |
| Tìm kiếm không dấu | crate `unicode-normalization` (launcher, PhimTatBIOS) |

### 1.4. Nguyên tắc an toàn toàn cục

Các nguyên tắc này áp dụng cho **mọi** module và là tiêu chí kiểm
thử bắt buộc:

1. **Một lần nâng quyền duy nhất**: mỗi .exe tự kiểm tra quyền Admin
   (`qsu_common::sys::is_admin()`); nếu chưa có, tự khởi chạy lại
   chính nó bằng `ShellExecuteW` với verb `runas`
   (`sys::relaunch_as_admin()`) rồi thoát. Ngoại lệ duy nhất:
   `PhimTatBIOS.exe` không cần quyền Admin nên không nâng quyền.
   Trong WinPE, `CaiDatWinWinPE.exe` bỏ qua bước nâng quyền vì WinPE
   luôn chạy với quyền SYSTEM.
2. **Thao tác phá huỷ phải có xác nhận rõ ràng**: xoá đĩa (wipeDisk)
   bắt gõ lại serial đĩa; chia lại phân vùng (rebuild) bắt gõ lại
   serial đĩa; format trong module WinPE bắt gõ serial của phân
   vùng (dạng XXXX-XXXX); tắt BitLocker, dọn rác, gỡ Appx, áp tweak
   đều có hộp xác nhận liệt kê phạm vi trước khi chạy.
3. **Nhận diện đối tượng bằng định danh vật lý, không đoán theo ký
   tự ổ**: ổ Data nhận diện bằng file marker `.qsu_data_drive`; đĩa
   đích nhận diện bằng **serial đĩa** (chuẩn hoá qua
   `disk::normalize_disk_serial`); phân vùng Windows nhận diện bằng
   **volume serial**. Kiểm tra lại định danh ngay trước thao tác ghi.
4. **Mật khẩu không bao giờ vào log, không vào `stage.json`**: wizard
   xoá khoá `adminPassword`, `ultraviewerPassword` và `apps` khỏi
   JSON trước khi ghi đĩa (xem mục 4.6). Mật khẩu Wi-Fi chỉ hiển thị
   trên màn hình khi người dùng bấm xem, không ghi log (chỉ ghi số
   lượng). Khoá OEM trong BIOS chỉ kiểm tra *sự tồn tại* (bảng ACPI
   MSDM), không bao giờ đọc nội dung khoá.
5. **Thư mục `custom` chỉ sao chép nguyên vẹn**: nếu người dùng đặt
   thư mục `custom\` cạnh `CaiDatWinTuDong.exe`, wizard/engine chép
   nguyên vào bản Windows mới tại `C:\ProgramData\QSU\custom`.
   Bộ công cụ **không** đọc hiểu, không thực thi nội dung trong đó;
   việc kích hoạt (RunOnce, scheduled task...) là của chủ máy.
6. **Không có agent hậu cài đặt**: crate `post_install` đã bị loại
   khỏi workspace một cách chủ đích (chỉ còn `rust/post_install/README.md`
   ghi lại quyết định). Engine WinPE chỉ *hỗ trợ móc nối*: nếu trong
   thư mục staging có sẵn `PostInstall.exe` do người dùng tự đặt,
   engine chép vào bản Windows mới và đăng ký RunOnce; không có thì
   bỏ qua và ghi log. Xem thêm mục 5.5.
7. **Việc dài chạy ở luồng nền**: mọi thao tác dài (quét, tải, chép,
   DISM, pnputil...) chạy trên worker thread, giao diện nhận kết quả
   qua NWG `Notice`; nút thao tác bị khoá trong lúc chạy để tránh
   bấm hai lần.

### 1.5. Yêu cầu môi trường kiểm thử

- Máy Windows 10/11 x64, quyền Administrator, có thể hy sinh dữ
  liệu (đặc biệt với các chế độ cài mới).
- Với module cài Windows: tối thiểu nên có máy **2 đĩa** (1 đĩa hệ
  thống + 1 đĩa dữ liệu chứa marker `.qsu_data_drive`) và một máy
  **1 đĩa** riêng cho chế độ rebuild; cả hai biến thể **UEFI/GPT**
  và **Legacy/MBR** nếu có thể.
- File ISO Windows (cả ISO gốc Microsoft và ISO đã tuỳ biến đều là
  ca kiểm thử hợp lệ — xem ma trận mục 6).
- USB WinPE tự tạo (ADK) cho module `CaiDatWinWinPE.exe`.
- Sau mỗi ca kiểm thử cài đặt, thu thập đủ bộ log (đường dẫn ở mục
  4.8) trước khi khởi động lại máy lần nữa — log là căn cứ duy nhất
  để chẩn đoán vì các bước trong WinPE không có người quan sát.

---

## 2. Thư viện chung `qsu_common`

Mã nguồn: `rust/qsu_common/src/`. Mọi module đều dùng chung; lỗi ở
đây ảnh hưởng toàn bộ công cụ.

### 2.1. `sha256.rs` — băm SHA-256 thuần Rust

- Tự cài đặt thuật toán SHA-256 (FIPS 180-4), không phụ thuộc crate
  ngoài.
- Hàm chính: `Sha256::new()`, `update(&[u8])`, `finalize() -> [u8; 32]`,
  `to_hex(&[u8; 32]) -> String`, `hash_file(path, progress_cb)`
  (đọc theo khối 1 MiB, gọi callback báo tiến độ).
- Ai dùng: wizard `CaiDatWinTuDong` kiểm tra toàn vẹn ISO tải từ kho
  online — bản ghi kho **thiếu SHA-256 sẽ bị từ chối tải**, và file
  tải về có tổng băm không khớp sẽ bị xoá (xem mục 3.2).
- Vector kiểm thử FIPS nằm trong `#[cfg(test)]` của chính file này;
  lưu ý các test đó biên dịch cho target Windows nên **chưa chạy trên
  máy build Linux** — khi kiểm thử, đối chiếu bằng một file có tổng
  SHA-256 đã biết.

### 2.2. `json.rs` — JSON tối giản tự viết

- Kiểu `Json` (enum: Null/Bool/Num/Str/Arr/Obj) với `parse(text)`
  (tự strips BOM UTF-8 ở đầu vào — sửa từ phát hiện H1 của đợt rà
  soát v0.4) và `stringify(v)`; các hàm dựng `obj/s/n/b` và truy cập
  `get/as_str/as_u64/as_bool/as_arr/str_or/u64_or/bool_or`.
- Ai dùng: `settings.rs`, `stage.rs`, module driver (phân tích JSON
  của WMI/PowerShell khi cần), catalogue ISO online của wizard.

### 2.3. `log.rs` — ghi log

- Kiểu `Logger { file: Mutex<Option<File>>, sink }`: `Logger::new(path)`
  mở file (append), `line(&str)` ghi file trước rồi mới đẩy lên
  giao diện qua sink (closure `Fn(&str)`); `now_string()` cho dấu
  thời gian hiển thị, `log_stamp()` cho định dạng `yyyyMMdd-HHmm`
  dùng trong tên file log.
- Quy ước tên file log của từng module nằm ở mục 3 và 4.8. Lỗi ghi
  file log bị bỏ qua lặng (không làm chết chương trình) — đây là
  phát hiện mở D7, xem mục 5.4.

### 2.4. `cmd.rs` — chạy lệnh ngoài

- Bọc `std::process::Command` với cờ `CREATE_NO_WINDOW` (không bật
  cửa sổ console). Kiểu kết quả `CmdOut { code: Option<i32>, stdout, stderr }`,
  mọi output giải mã bằng `from_utf8_lossy`.
- Ba hàm chính:
  - `run_capture(exe, args)` — chạy và thu toàn bộ output.
  - `run_logged(logger, exe, args)` — như trên + ghi log theo định
    dạng `$ <lệnh>`, ` | <stdout>`, ` ! <stderr>`, ` -> exit <mã>`.
  - `run_streaming(logger, exe, args, on_line)` — đọc stdout theo
    dòng theo thời gian thực (stderr ở luồng riêng) — dùng cho DISM
    để hiển thị phần trăm tiến độ.
- Ai dùng: hầu như mọi module (DISM, `pnputil`, `manage-bde`,
  `netsh`, `schtasks`, `powercfg`, `bcdedit`, `diskpart`...).

### 2.5. `http.rs` — tải file bằng WinHTTP, có nối tiếp (resume)

- Dùng WinHTTP API gốc (không dùng BITS — đặc tả thiết kế ban đầu
  ghi BITS, hiện thực đã đổi; xem mục 5.6).
- `get_bytes(url)` — tải nhỏ trong bộ nhớ (đọc catalogue ISO).
- `download_file(url, dest, progress_cb) -> DownloadOutcome { total_bytes, resumed_from }`:
  gửi header `Range: bytes=<kích thước file hiện có>-`; máy chủ trả
  **206** thì ghi nối tiếp; trả **200** thì tải lại từ đầu; file đã
  đủ mà máy chủ trả **416** thì xoá tải lại toàn bộ (hành vi này là
  phát hiện mở D7). Ghi theo khối 64 KiB, báo tiến độ qua callback.
- Ai dùng: wizard tải ISO từ `catalogueUrl` trong `settings.json`.

### 2.6. `settings.rs` — mô hình cấu hình `settings.json`

`Settings::load(dir)` đọc `settings.json` trong thư mục chứa .exe
của wizard. Trường và mặc định (theo mã nguồn):

| Khoá JSON | Mặc định | Ý nghĩa |
|---|---|---|
| `catalogueUrl` | `""` (tắt) | URL file JSON danh mục ISO online |
| `locale` / `uiLanguage` / `inputLocale` | `vi-VN` | Ngôn ngữ/bàn phím ghi vào unattend |
| `timezone` | `SE Asia Standard Time` | Múi giờ ghi vào unattend |
| `downloadFolder` | `QSU-Downloads` | Thư mục tải ISO trên ổ Data |
| `logsFolder` | `QSU-Logs` | Thư mục log trên ổ Data |
| `stagingFolder` | `QSU-Stage` | Thư mục staging trên ổ Data |
| `minFreeGbInPlace` | 25 | GB trống tối thiểu trên ổ OS cho cài đè |
| `minFreeGbClean` | 12 | GB trống tối thiểu trên ổ Data cho cài mới |
| `winpeWimPath` | `""` | Đường dẫn boot.wim tuỳ biến (trống = lấy từ ISO) |
| `storageDriversPath` | `""` | Thư mục driver lưu trữ (trống = thư mục `drivers` cạnh wizard) |
| `defaultApplyTweaks` | `false` | Giá trị mặc định ban đầu cho công tắc tweak trong wizard (xem phát hiện D1, mục 5.1) |

- Ba trường tên thư mục (`downloadFolder`, `logsFolder`,
  `stagingFolder`) bị kiểm tra bằng `valid_folder_name()`: chỉ chấp
  nhận **một tên thư mục đơn** (không dấu phân cách đường dẫn, không
  `..`) — lớp phòng vệ 1 của phát hiện N3 (rà soát v0.4).
- Mô hình còn giữ hai trường của schema cũ — `postInstallApps` và
  `ultraviewerPassword` (hàm `default_apps()` còn chứa sẵn mục
  UltraViewer + Chrome). **Wizard hiện tại không tiêu thụ hai trường
  này** khi ghi `stage.json` (luôn ghi `apps` rỗng và xoá khoá mật
  khẩu); chúng là tàn dư schema, xem mục 5.6.

### 2.7. `stage.rs` — hợp đồng `stage.json`

`StageConfig` là cấu trúc dữ liệu wizard ghi ra và engine WinPE đọc
vào — hợp đồng duy nhất giữa hai phía của quá trình cài đặt. Toàn bộ
schema (khoá JSON, kiểu, mặc định khi thiếu) được trình bày ở mục
4.5. Hàm chính: `to_json()` / `from_json(text)` trong
`qsu_common::stage`, dùng `json.rs` ở trên.

### 2.8. `sys.rs` — dò thông tin hệ thống và tiện ích Win32

| Hàm | Việc làm | Ai dùng |
|---|---|---|
| `is_admin()` / `relaunch_as_admin()` | Kiểm tra token Admin; tự nâng quyền bằng `ShellExecuteW` verb `runas` | Mọi module (trừ PhimTatBIOS) |
| `wide()` / `from_wide()` | Chuyển chuỗi UTF-16 cho Win32 API | Khắp nơi |
| `reg_get_sz()` / `reg_set_sz()` / `reg_delete_value()`, `hklm()` | Đọc/ghi registry gốc cho các nhu cầu đơn giản | Wizard (ghi mốc cài đặt), QuanLyUpdateWindows (khoá chính sách AU qua API gốc) |
| `os_info() -> OsInfo` | Đọc `CurrentVersion` trong registry: tên hiển thị, build, EditionID; build ≥ 22000 thì đổi tên "Windows 10" thành "Windows 11" | Wizard (preflight), launcher (tiêu đề cửa sổ) |
| `firmware_is_uefi()` | Mẹo một-API: gọi `GetFirmwareEnvironmentVariableW` với tên biến rỗng — thành công ⟺ UEFI | Các binary phía WinPE (`WinPEInstall`, `CaiDatWinWinPE`) |
| `disk_partition_style(letter)` | `IOCTL_DISK_GET_DRIVE_LAYOUT_EX` (buffer 64 KiB — sửa từ phát hiện H2) → GPT/MBR của đĩa chứa một ổ | Wizard (preflight) |
| `power_status()` | `GetSystemPowerStatus`: đang cắm điện hay pin, % pin | Wizard (cảnh báo pin < 60%) |
| `list_fixed_drives()` | Liệt kê ổ cố định (`DRIVE_FIXED`): ký tự, volume serial, nhãn, dung lượng trống (GetVolumeInformationW + GetDiskFreeSpaceExW) | Wizard, DonRacMayTinh |
| `os_drive_letter()` | Ký tự ổ hệ điều hành hiện tại | Nhiều module |
| `oem_key_present()` | Chỉ kiểm tra **sự tồn tại** của bảng ACPI MSDM | Wizard (preflight) |

### 2.9. `svc.rs` — quản lý dịch vụ qua SCM gốc

- Dùng Service Control Manager API gốc (không qua `sc.exe`):
  `query(name) -> ServiceInfo`, `start()`, `stop()`,
  `set_start_type()`, `wait_for_state()`; hằng kiểu khởi động
  `START_AUTO = 2`, `START_DEMAND = 3`, `START_DISABLED = 4`;
  `state_label()` / `start_type_label()` trả nhãn tiếng Việt.
- Ai dùng: `QuanLyUpdateWindows` (wuauserv/UsoSvc/BITS...) và
  `TuyChinhWindows` (các tweak dạng dịch vụ), `DonRacMayTinh`
  (dừng/khởi động lại wuauserv khi xoá cache Windows Update).

### 2.10. `disk.rs` — liệt kê đĩa/phân vùng bằng IOCTL, xoá & chia lại đĩa

Đây là submodule lớn nhất (~1.700 dòng) và nguy hiểm nhất — nó là
nền của hai chế độ wipeDisk và rebuild. Các thành phần chính:

- **Kiểu dữ liệu**: `PartKind { Normal, Efi, Msr, Recovery, System, Unknown }`;
  `DiskPartition` (ký tự ổ, nhãn, volume serial, offset, kích thước,
  `part_guid` — PartitionId GPT, rỗng với MBR/dự phòng);
  `PhysicalDisk { number, serial, model, size, style, partitions, from_fallback }`.
- **Hằng số**: `MIN_WIPE_DISK_BYTES = 25 GB` (đĩa nhỏ hơn bị cấm
  wipe); GUID loại phân vùng GPT: EFI `C12A7328-F81F-11D2-BA4B-00A0C93EC93B`,
  MSR `E3C9E316-0B5C-4DB8-817D-F92DF00215AE`,
  Recovery `DE94BBA4-06D1-4D40-A16A-BFD50179D6AC`. Bố cục wipe:
  GPT = EFI 260 MB + MSR 16 MB + Windows (phần còn lại trừ 876 MB và
  4 MB cuối) + Recovery 600 MB, yêu cầu phân vùng Windows ≥ 20 GB;
  MBR = System Reserved 100 MB + Windows + Recovery 600 MB. Vùng
  rebuild tối thiểu = (876 MB với GPT | 700 MB với MBR) + Windows
  ≥ 10 GB.
- **Liệt kê** — `enumerate_disks() -> DiskEnumeration { disks, diag }`:
  một đĩa được coi là tồn tại khi **mở được handle** `\\.\PhysicalDriveN`
  (quét N từ 0 đến 16); kích thước lấy từ phân vùng nếu IOCTL độ dài
  thất bại. Khi IOCTL không cho serial/phân vùng, hợp nhất thêm hai
  nguồn dự phòng: `merge_powershell_disks()` (Get-Disk qua
  PowerShell, JSON) rồi `merge_diskpart_disks()` (`list disk` /
  `detail disk` của diskpart; serial dự phòng dạng `DP-<Disk ID>`).
  Mọi bước đều ghi vào chuỗi chẩn đoán `diag` (wizard in ra log với
  tiền tố `[Đĩa]`).
- **Tra cứu an toàn**: `normalize_disk_serial()` (chỉ giữ chữ/số,
  viết hoa); `find_disk_by_serial()` — chỉ trả kết quả khi khớp
  **đúng một** đĩa; `volume_disk_number()` /
  `volume_disk_extent()` (`IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS`)
  để biết một ổ nằm trên đĩa vật lý nào / vùng byte nào.
- **Wipe** — `wipe_layout(style, total_bytes)`,
  `wipe_script(...)` (sinh kịch bản diskpart: `clean` + `convert`,
  đặt `set id` và `gpt attributes=0x8000000000000001` cho Recovery),
  `execute_wipe(serial, style) -> WipeResult { windows_letter, boot_letter, recovery_letter }`:
  liệt kê lại → kiểm tra lại serial → ghi nguyên văn kịch bản vào
  log → chạy diskpart → kiểm chứng sau: phân vùng theo nhãn + loại +
  kích thước (±10%).
- **Rebuild** — `rebuild_layout(...)` (tính theo vùng: giữ nguyên
  mọi phân vùng ngoài vùng, chia lại trong vùng theo offset neo
  bằng KB), `rebuild_script(...)` (**không** `clean`/`convert`;
  xoá theo số phân vùng theo thứ tự offset giảm dần, tạo mới với
  offset ghim), `execute_rebuild(serial, spec)`: kiểm tra spec
  (không rỗng, đúng 1 mục "Windows"), kiểm tra lại serial, khớp từng
  phân vùng cần giữ bằng `part_guid` (GPT) hoặc offset+kích thước
  ±1 MB (MBR) — mỗi mục đúng 1 kết quả khớp, không trùng claim;
  kiểm tra staging không chồng lấn vùng rebuild; kiểm tra tính liên
  tục ±1 MB; **kiểm tra kích thước trước mọi thao tác xoá**; sau khi
  chạy, kiểm chứng lại các phân vùng giữ nguyên không đổi ±1 MB và
  phân vùng mới nằm trong vùng.
- **WinRE** — `copy_winre(win_letter, rec_letter)`: chép `Winre.wim`
  + `ReAgent.xml` vào `<R>:\Recovery\WindowsRE` rồi
  `reagentc /setreimage /path ... /target <W>:\Windows`; best-effort
  (lỗi chỉ ghi log).
- Ai dùng: wizard (hiển thị + cổng chặn `disk_block_reason`) và cả
  hai engine cài đặt (`execute_wipe` / `execute_rebuild` chạy thật
  trong WinPE; `CaiDatWinWinPE` dùng `execute_wipe` trực tiếp).

---

## 3. Từng module

Mỗi module dưới đây có cùng khung mô tả: Mục đích | Công nghệ | Cơ
chế tổ chức | Hàm chính trong code | Quy tắc an toàn riêng |
Checklist kiểm thử | Trạng thái kiểm thử thực tế.

Log riêng của từng module (nếu có) nằm **cạnh file .exe** của module
đó, trừ nhóm cài Windows có quy ước riêng ở mục 4.8.

### 3.1. Launcher — `UtilityRebuild.exe`

- **Mục đích**: cửa sổ chính của bộ công cụ: lưới 32 nút chức năng
  tiếng Việt + ô tìm kiếm; bấm nút là chạy module .exe tương ứng.
- **Công nghệ**: Rust + NWG; bảng ánh xạ tĩnh `MODULES` (32 cặp
  nhãn → tên file) trong `launcher/src/main.rs`; tìm kiếm không phân
  biệt dấu qua `unicode-normalization`.
- **Cơ chế tổ chức**:
  1. Khởi động: đọc thông tin OS từ registry (`sys::os_info()`),
     tên máy và tên người dùng, hiển thị ở thanh tiêu đề/phụ đề.
  2. Người dùng gõ ô tìm kiếm → lọc nút theo nhãn đã bỏ dấu.
  3. Bấm nút → `start_module()`: xác định thư mục module bằng
     `modules_dir()` (= `<thư mục chứa launcher>\modules`); file
     chưa có → hộp thông báo "Chưa có module"; có file →
     `ShellExecuteW` với verb `runas` (mỗi module tự nâng quyền một
     lần nữa nếu cần); mã trả về ≤ 32 → hộp lỗi.
- **Hàm chính**: `MODULES` (hằng bảng 32 mục), `start_module()`,
  `modules_dir()`, hàm bỏ dấu dùng cho tìm kiếm.
- **An toàn riêng**: launcher tự nó không thao tác hệ thống; quyền
  Admin do từng module tự lo.
- **Checklist kiểm thử**:

  | Bước | Thao tác | Kết quả mong đợi |
  |---|---|---|
  | 1 | Mở `UtilityRebuild.exe` trên Windows thường (không Admin) | Cửa sổ hiện đủ 32 nút, phụ đề đúng tên Windows/tên máy/tên user |
  | 2 | Gõ "cai dat" (không dấu) vào ô tìm kiếm | Chỉ còn các nút "Cài Đặt..." khớp |
  | 3 | Bấm một nút chưa có module (vd "Kích Hoạt Win / Office") | Thông báo "Chưa có module", không lỗi, không crash |
  | 4 | Bấm "Cài Đặt Win Tự Động" | `CaiDatWinTuDong.exe` mở (có hộp UAC của chính module nếu cần) |
  | 5 | Xoá thử thư mục `modules` rồi mở launcher, bấm nút bất kỳ | Báo không tìm thấy module, launcher không crash |

- **Trạng thái kiểm thử thực tế**: bản mẫu PowerShell v0.1 đã chạy
  trên máy IT-DJGQZ (2026-11-16). Bản Rust đã được mở và dùng để gọi
  wizard trong các buổi test thực địa trên DESKTOP-J8LPB75
  (2026-11-17). Chưa có đợt kiểm thử hệ thống riêng cho launcher.

### 3.2. Wizard cài Windows — `CaiDatWinTuDong.exe` (tóm tắt)

- **Mục đích**: chuẩn bị cài Windows ngay từ Windows đang chạy, hai
  đường: **cài đè** (in-place upgrade, giữ ứng dụng/dữ liệu) hoặc
  **cài mới** (staging WinPE lên ổ Data, khởi động lại vào WinPE để
  engine cài sạch) với ba chế độ phân vùng: `preserve` (giữ nguyên,
  chỉ format phân vùng Windows), `wipeDisk` (xoá cả đĩa đích, chia
  lại từ đầu), `rebuild` (chỉ xoá/chia lại các phân vùng hệ thống đã
  chọn trên đĩa chứa Windows, giữ phân vùng dữ liệu).
- **Công nghệ**: Rust + NWG (4 tab: Nguồn cài đặt / Tuỳ chọn cài đặt
  / Hướng dẫn / Tài liệu; khối tiến độ + log + nút TIẾN HÀNH ghim
  dưới khung tab; cửa sổ thích ứng work-area, tối thiểu 900×640,
  khung thiết kế 1036×748). Lệnh ngoài: DISM, `pnputil`,
  `manage-bde`, `bcdedit`, PowerShell (gắn ISO, đọc BitLocker).
- **Cơ chế tổ chức** (chi tiết đầy đủ ở mục 4): Quét tiền đề
  (preflight) → chọn nguồn ISO (quét ổ / chọn file / tải online có
  kiểm SHA-256) → gắn ISO, đọc danh sách edition bằng DISM → người
  dùng chọn chế độ/phiên bản/tuỳ chọn → thực thi: cài đè chạy
  `setup.exe` của ISO ngay trong Windows; cài mới chép staging +
  tiêm `WinPEInstall.exe` vào `winpe.wim` + đăng ký BCD một lần
  (`/bootsequence`) + ghi `stage.json`, rồi khởi động lại vào WinPE.
- **Hàm chính**: `run_preflight()`, `probe_bitlocker()`,
  `probe_firmware()`, `verdict_of()`, `persist_scan_log()`,
  `scan_isos_on_drive()`, `spawn_download()`, `mount_iso()`,
  `load_editions()` / `parse_kv()` / `parse_build()`,
  `build_unattend()`, `exec_main()`, `exec_inplace()`,
  `exec_clean()`, `probe_winpe_wim()`, `mount_inject_winpe()`,
  `stage_boot_sdi()`, `register_bcd()` / `verify_bcd()`,
  `disk_block_reason()`, `classify_partition()`.
- **An toàn riêng**: xem mục 4.2 (cổng chặn theo phán quyết
  preflight), 4.4 (ba chế độ phân vùng và cổng chặn theo đĩa), 4.6
  (mật khẩu bị loại khỏi `stage.json`). Mọi panic trong luồng thực
  thi đều bị bắt (`catch_unwind`) và ghi vào hộp đen
  `CaiDatWinTuDong-panic.log` thay vì chết im lặng.
- **Checklist kiểm thử**: xem ma trận tổng hợp ở mục 6 (nhóm A–C).
- **Trạng thái kiểm thử thực tế**: preflight, quét/gắn ISO, đọc
  edition, chặn cài đè khi build ISO thấp hơn — **đã kiểm chứng**
  trên DESKTOP-J8LPB75 (VM Hyper-V thế hệ 1, Legacy/MBR, 2026-11-17);
  quá trình staging cài mới đã chạy trọn 100% một lần trên VM này
  (v0.7.6) và máy khởi động lại vào mục BCD một lần, nhưng WinPE
  không boot được (lỗi Boot Manager 0xc000000e — WIM của ISO mod
  không boot được trong cấu hình đó); các điểm dừng an toàn (thiếu
  `boot.sdi`, cổng kiểm chứng cũ) đã kiểm chứng trên VM và trên máy
  thật UEFI DESKTOP-704R89L. **v0.7.11, v0.7.12, v0.7.13 chưa từng
  chạy trên máy thật** tại thời điểm viết tài liệu.

### 3.3. Engine cài đặt trong WinPE — `WinPEInstall.exe` (tóm tắt)

- **Mục đích**: chạy **bên trong WinPE** (bị wizard tiêm vào
  `winpe.wim` hoặc nằm trong thư mục staging), thực thi bản cài
  theo `stage.json`: format/xoá đĩa → bung ảnh bằng DISM → inject
  driver → chép unattend + thư mục `custom` → tạo boot bằng
  `bcdboot` → khởi động lại vào Windows mới.
- **Công nghệ**: Rust thuần Win32 + lệnh ngoài của WinPE: DISM,
  `format.com`/`diskpart`, `bcdboot`, `wpeutil`; liệt kê/xoá đĩa
  qua `qsu_common::disk`.
- **Cơ chế tổ chức**: xem mục 4.7 (luồng đầy đủ và bảng mã thoát
  2–13).
- **Hàm chính**: `find_stage()`, `find_os_drive()`,
  `format_os_partition()`, `run_apply_image()`, `apply_unattend()`,
  `copy_custom()`, `stage_post_install()`, `mount_scratch()`;
  chốt boot preserve bằng `disk::finalize_boot()` dùng chung
  (v0.7.19 — `find_efi_letter()` đã bị xoá).
- **An toàn riêng**: nhận diện ổ Data/ổ Windows/đĩa đích **chỉ bằng
  serial**; mọi bất định (không khớp serial, đĩa đích chứa ổ Data,
  không rõ đĩa Data) đều dừng với mã thoát riêng **trước khi ghi
  bất cứ thứ gì lên đĩa**.
- **Hiển thị tiến trình (từ v0.7.18)**: hai bề mặt — console của
  launcher `qsu_start.cmd` là bề mặt CHÍNH (mọi dòng log in thẳng
  ra console qua sink của Logger; % bung ảnh cập nhật tại chỗ bằng
  dòng `\r`, đọc từ DISM bằng `cmd::run_streaming_progress()` cắt
  segment ở cả `\r`/`\n` vì tiến độ DISM kết thúc bằng `\r`), và
  màn hình tiến trình toàn màn hình tông xanh tự vẽ bằng GDI là
  bề mặt PHỤ (mở trước mọi công việc trên luồng riêng: tiêu đề +
  cảnh báo không tắt máy + bước hiện tại + số % lớn + thanh tiến
  trình + đồng hồ thời gian đã trôi chạy từng giây + đường dẫn
  log; thiết kế theo màn hình mẫu của tool gốc, thương hiệu của
  bản rebuild). Khi lỗi, engine neo lại trong WinPE giữ bước đang
  dừng + lỗi + đường dẫn log trên cả hai bề mặt thay vì thoát cho
  launcher reboot (mã thoát nay được `run()` trả về `main()` chốt
  trạng thái cuối; logic cài đặt không đổi).
- **Checklist kiểm thử**: xem mục 6 (nhóm B, C).
- **Trạng thái kiểm thử thực tế**: **chưa kiểm chứng trên máy thật**
  — WinPE do chính công cụ staging chưa từng boot thành công tới
  bước engine chạy (xem mốc còn thiếu ở mục 5.5). Mọi luồng của
  engine hiện mới ở mức rà soát mã nguồn + kiểm chứng gián tiếp qua
  log wizard.

### 3.4. Module cài Windows tương tác — `CaiDatWinWinPE.exe`

- **Mục đích**: bản cài Windows **thao tác tay hoàn toàn** cho kỹ
  thuật viên: boot máy bằng USB WinPE bất kỳ (ADK, Hiren's...), chạy
  module ngay trong WinPE và tự chọn nguồn → ổ đích → cài. Khi chạy
  trên Windows thường, module chuyển sang **chế độ "Chuẩn bị"**: chép
  chính nó + `WinPEInstall.exe` ra gốc ổ được chọn để dùng sau.
- **Công nghệ**: Rust + NWG; tự phát hiện môi trường bằng
  `detect_winpe()` (khoá registry `MiniNT` / giá trị `PEBootType`);
  liệt kê volume bằng Win32 gốc (không WMI) và đối chiếu chéo bằng
  `diskpart list volume`; gắn ISO bằng **VirtDisk API gốc** qua
  `qsu_common::iso` (từ v0.7.19 — không còn cần PowerShell);
  chọn file bằng **trình duyệt file tích hợp** của module (hộp
  thoại file của Windows không mở được trong WinPE BHT/Anhdv);
  firmware bằng `sys::firmware_is_uefi()`.
- **Cơ chế tổ chức**:
  1. Mở module → tự nhận diện WinPE hay Windows thường. Trong WinPE:
     bỏ qua nâng quyền. Windows thường: nâng quyền + hiện chế độ
     Chuẩn bị.
  2. Chọn nguồn: file ISO (tự mount bằng VirtDisk API qua
     `qsu_common::iso` — từ v0.7.19; lỗi attach thì thông báo
     hướng dẫn mount sẵn rồi chọn thẳng file ảnh), hoặc file ảnh
     trực tiếp. Chọn bằng trình duyệt file tích hợp (liệt kê ổ
     đĩa, duyệt thư mục, lọc *.iso/*.wim/*.esd/*.swm, tên file
     đầy đủ) hoặc gõ thẳng đường dẫn vào ô. Đọc danh sách
     edition bằng DISM.
  3. Chọn ổ đích từ danh sách volume đã đối chiếu; các volume chứa
     chính .exe, chứa nguồn, hoặc là ổ ảo của ISO bị **loại mặc
     định** (bỏ loại trừ được bằng checkbox — chỉ với volume).
  4. Chọn đĩa đích (cho chế độ wipe): đĩa chứa .exe hoặc chứa nguồn
     bị **cấm tuyệt đối**, không có đường bỏ qua
     (`disk_block_reason()`).
  5. Tuỳ chọn format: mặc định **TẮT**; bật format thì bắt gõ serial
     của volume đích (dạng XXXX-XXXX). Ổ `X:` không bao giờ là đích.
  6. Bấm cài (`spawn_install()`): kiểm tra lại an toàn ở mức 2% →
     wipe bằng `disk::execute_wipe()` (4%) hoặc format (5%) → DISM
     apply (10%, ánh xạ % của DISM sang tối đa 80%) → inject driver
     (85%) → unattend (90%) → chép `custom` → `bcdboot` (95%) →
     100%. Nút "Khởi động lại" chỉ bật sau khi hoàn tất toàn bộ
     (`wpeutil reboot`, dự phòng `shutdown /r /t 5`).
- **Hàm chính**: `detect_winpe()`, `enumerate_volumes()`,
  `disk_block_reason()`, `volume_serial_of()`, `spawn_install()`,
  `build_unattend()` (bản sao độc lập, cùng quy tắc mật khẩu tuỳ
  chọn như wizard; từ v0.7.19 có thêm hai FirstLogonCommands dọn
  cờ đổi mật khẩu lần đầu), `browser_open()`/`browser_fill()`/
  `browser_activate()` (trình duyệt file tích hợp, v0.7.19),
  chốt boot theo phân vùng bằng `disk::finalize_boot()` dùng
  chung trong qsu_common (v0.7.19 — đã thay `find_efi_letter()`
  cũ), mount ISO bằng `qsu_common::iso::mount_iso()`,
  cấu trúc `InstallPlan` (chỉ mang `admin_user` +
  `admin_passwordless`, **không mang mật khẩu**).
- **An toàn riêng**: trước khi ghi, đọc lại serial volume tại chỗ
  (`volume_serial_of()`) để chống tráo ổ sau khi liệt kê; kế hoạch
  cài không chứa mật khẩu dạng rõ.
- **Checklist kiểm thử**:

  | Bước | Thao tác | Kết quả mong đợi |
  |---|---|---|
  | 1 | Chép module vào USB WinPE, boot máy test, chạy module | Mở thẳng giao diện cài (không hộp UAC), nhận diện đúng các volume |
  | 2 | Chọn nguồn là file ISO trong WinPE không có PowerShell (BHT/Anhdv) | Module tự mount ISO bằng VirtDisk và liệt kê được các phiên bản (trước v0.7.19: báo ngõ cụt PowerShell) |
  | 2b | Bấm "Chọn file..." trong WinPE | Trình duyệt file TÍCH HỢP mở ra (không phải hộp thoại Windows), duyệt được ổ/thư mục, thấy tên file đầy đủ, nhấp đúp file là nạp |
  | 2c | Nạp một nguồn LỖI (đường dẫn sai / ISO hỏng) rồi nạp lại nguồn đúng | Sau lần lỗi các nút bấm lại được ngay (trước v0.7.19 từng kẹt nút "Nạp nguồn", phải mở lại module); lần nạp lại chạy bình thường |
  | 2d | Cài theo phân vùng (UEFI) khi phân vùng EFI của đĩa đích KHÔNG có ký tự ổ | Bước bcdboot tự gán ký tự cho EFI trên CÙNG đĩa đích và qua được, kiểm chứng có `bootmgfw.efi` + BCD (trước v0.7.19: thất bại, phải chạy bcdboot tay) |
  | 2e | Boot đầu vào Windows mới sau ca cài có autounattend | AutoLogon vào thẳng desktop — KHÔNG dừng ở màn hình bắt đổi mật khẩu (từ v0.7.19) |
  | 3 | Thử chọn ổ chứa chính module làm đích | Ổ bị loại mặc định; bỏ loại trừ bằng checkbox thì chọn được (volume), nhưng đĩa chứa module vẫn bị cấm ở chế độ wipe |
  | 4 | Bật format, gõ sai serial volume | Bị chặn, không format |
  | 5 | Cài thử lên ổ phụ với format tắt | DISM chạy, có unattend, `bcdboot` xong, nút Khởi động lại bật |
  | 6 | Chạy module trên Windows thường | Hiện chế độ "Chuẩn bị": chép module + WinPEInstall.exe ra gốc ổ chọn |

- **Trạng thái kiểm thử thực tế**: **ĐÃ chạy trong WinPE thật**
  (2026-11-18, DESKTOP-704R89L, WinPE USB BHT/Anhdv): nạp nguồn
  `F:\sources\install.wim` của ISO gốc (11 phiên bản), cài theo
  phân vùng lên C: với format — DISM apply đạt 100%; bước
  bcdboot của bản cũ THẤT BẠI (phân vùng EFI không có ký tự ổ —
  đã sửa ở v0.7.19 bằng `disk::finalize_boot()`), kỹ thuật viên
  chốt boot bằng tay (`bcdboot C:\Windows /s S: /f UEFI`) và
  Windows gốc boot vào "Getting ready" → desktop (dừng một lần
  ở màn hình đổi mật khẩu — cũng đã sửa ở v0.7.19). Các lỗi
  chọn file/mount ISO/kẹt nút cùng buổi test đó đều là đầu vào
  của v0.7.19; các bản sửa v0.7.19 **chưa** được kiểm chứng lại
  trên máy thật — xem 4 ca test ưu tiên trong HUONG-DAN v0.7.19.

### 3.5. Sao lưu / Khôi phục Driver — `BackupRestoreDriver.exe`

- **Mục đích**: liệt kê toàn bộ driver đã cài, sao lưu từng driver
  (hoặc tất cả) ra thư mục, khôi phục từ thư mục sao lưu, xuất danh
  sách HTML.
- **Công nghệ**: liệt kê bằng **WMI gốc qua COM** (không qua
  PowerShell): `CoInitializeEx` (MTA) → `IWbemLocator` →
  `ROOT\CIMV2` → WQL `SELECT * FROM Win32_PnPSignedDriver WHERE DeviceName IS NOT NULL`;
  sao lưu/khôi phục bằng `pnputil`; mở HTML bằng `ShellExecuteW`.
- **Cơ chế tổ chức**:
  1. `enumerate_drivers()`: lấy DeviceName/Manufacturer/DeviceClass/
     DriverVersion/DriverDate/InfName; `category_of()` phân nhóm
     Graphics/Audio/Network/Storage/Input/Other; `cim_date()` đổi
     định dạng ngày CIM sang hiển thị.
  2. Sao lưu (`spawn_backup()`): với mỗi driver được chọn chạy
     `pnputil /export-driver <InfName> <thư mục đích>\<tên inf bỏ đuôi>`.
  3. Khôi phục (`spawn_restore()`): `collect_infs()` quét đệ quy các
     file `.inf` trong thư mục nguồn; chạy
     `pnputil /add-driver <inf> /install` — chấp nhận mã thoát
     0/259/3010 là thành công — rồi `pnputil /scan-devices`.
  4. Xuất HTML (`export_html()`): ghi `DanhSachDriver.html` cạnh
     .exe và mở bằng trình duyệt mặc định.
  5. Thư mục sao lưu mặc định (`default_backup_folder()`): ưu tiên
     thư mục `D:\backup_driver*` đã có; chưa có thì
     `D:\backup_driver_<yyyyMMdd>`; không có ổ D: thì Desktop.
- **Hàm chính**: `enumerate_drivers()`, `category_of()`,
  `cim_date()`, `spawn_backup()`, `spawn_restore()`,
  `collect_infs()`, `export_html()`, `default_backup_folder()`.
- **An toàn riêng**: chỉ xuất/khôi phục driver, không tự xoá driver
  khỏi hệ thống; log `DriverTool_Log.txt` cạnh .exe.
- **Checklist kiểm thử**:

  | Bước | Thao tác | Kết quả mong đợi |
  |---|---|---|
  | 1 | Mở module | Danh sách driver hiện đầy đủ, có nhóm/loại, phiên bản, ngày |
  | 2 | Sao lưu 1 driver | Thư mục con theo tên .inf được tạo, chứa đủ file driver |
  | 3 | Sao lưu tất cả | Mọi driver trong danh sách đều có thư mục tương ứng |
  | 4 | Khôi phục từ thư mục vừa sao lưu | pnputil báo cài lại được; không lỗi đỏ |
  | 5 | Bấm xuất HTML | File `DanhSachDriver.html` mở trong trình duyệt, nội dung khớp danh sách |

- **Trạng thái kiểm thử thực tế**: bản PowerShell tương đương đã
  chạy ở v0.1 (module tham chiếu, port từ source dịch ngược). Bản
  Rust: **chưa kiểm chứng trên máy thật**.

### 3.6. Tắt BitLocker — `TatBitlocker.exe`

- **Mục đích**: liệt kê các volume BitLocker và giải mã hoàn toàn
  (`manage-bde -off`) có theo dõi tiến độ tới khi `FullyDecrypted`.
- **Công nghệ**: đây là **ngoại lệ PowerShell có chủ đích** (ghi rõ
  trong mã nguồn): trạng thái đọc bằng script PowerShell
  `Get-BitLockerVolume` trả JSON (MountPoint/VolumeStatus/
  ProtectionStatus/EncryptionPercentage/CapacityGB) vì API gốc
  tương đương không sẵn có; thao tác tắt bằng `manage-bde.exe`.
- **Cơ chế tổ chức**:
  1. `query_volumes()` chạy script `BL_SCRIPT`, phân tích JSON.
  2. Người dùng chọn volume → hộp xác nhận → `spawn_decrypt()` chạy
     `manage-bde.exe -off <mount>`.
  3. Vòng poll: cứ 5 giây gọi lại `query_volumes()`, tối đa 720 lần
     (~1 giờ), cập nhật % đã giải mã; dừng khi trạng thái là
     FullyDecrypted.
- **Hàm chính**: `BL_SCRIPT` (hằng script), `query_volumes()`,
  `spawn_decrypt()`.
- **An toàn riêng**: chỉ tắt mã hoá theo yêu cầu rõ ràng; việc giải
  mã của Windows **tiếp tục ở nền kể cả khi đóng công cụ** (ghi rõ
  trong log và giao diện) — tester cần biết để không kết luận sai
  là "đóng tool = dừng giải mã". Log `Bitlocker_Log.txt`.
- **Checklist kiểm thử**:

  | Bước | Thao tác | Kết quả mong đợi |
  |---|---|---|
  | 1 | Mở module trên máy có 1 volume đang mã hoá | Volume hiện đúng trạng thái Encrypted + % |
  | 2 | Bấm tắt BitLocker, xác nhận | `manage-bde -off` chạy, % giải mã tăng dần trong giao diện |
  | 3 | Đóng module giữa chừng, mở lại sau | Volume vẫn đang/đã giải mã (tiến trình của Windows không bị ảnh hưởng) |
  | 4 | Mở module trên máy không có BitLocker | Danh sách rỗng/thông báo phù hợp, không lỗi |

- **Trạng thái kiểm thử thực tế**: **chưa kiểm chứng trên máy thật**.

### 3.7. Sao Lưu Wi-Fi — `SaoLuuWifi.exe`

- **Mục đích**: liệt kê hồ sơ Wi-Fi đã lưu, xuất hồ sơ (kèm khoá rõ)
  ra XML, nhập lại hồ sơ từ XML, xem mật khẩu đã lưu.
- **Công nghệ**: đọc hồ sơ **trực tiếp từ XML của Wlansvc**
  (`C:\ProgramData\Microsoft\Wlansvc\Profiles\Interfaces\*\*.xml`,
  lấy thẻ `<name>` — không phụ thuộc ngôn ngữ hệ thống); dự phòng
  bằng `netsh wlan show profiles` (phân tích sau dấu `: `). Mọi
  thao tác xuất/nhập qua `netsh`.
- **Cơ chế tổ chức**:
  1. `list_profiles_from_wlansvc()`; nếu rỗng →
     `list_profiles_netsh()`.
  2. Xuất (`spawn_export()`): `netsh wlan export profile name=... folder=... key=clear`
     cho từng hồ sơ đã chọn.
  3. Nhập (`spawn_import()`): `netsh wlan add profile filename=... user=all`.
  4. Xem khoá (`spawn_show_keys()`): xuất tạm vào
     `%TEMP%\QSU-WifiKeys-<pid>`, đọc thẻ `<keyMaterial>`, hiển thị,
     **xoá thư mục tạm ngay sau đó**.
- **Hàm chính**: `list_profiles_from_wlansvc()`,
  `list_profiles_netsh()`, `spawn_export()`, `spawn_import()`,
  `spawn_show_keys()`.
- **An toàn riêng**: mật khẩu Wi-Fi **không ghi vào log** (log chỉ
  ghi số lượng hồ sơ có khoá); thư mục tạm chứa khoá rõ bị xoá
  ngay. Log `WifiTool_Log.txt`.
- **Checklist kiểm thử**:

  | Bước | Thao tác | Kết quả mong đợi |
  |---|---|---|
  | 1 | Mở module trên máy đã lưu vài mạng Wi-Fi | Danh sách đúng tên các mạng đã lưu |
  | 2 | Xuất hồ sơ | File XML xuất hiện trong thư mục chọn, có `keyMaterial` |
  | 3 | Xoá một hồ sơ khỏi Windows rồi nhập lại từ XML vừa xuất | Hồ sơ xuất hiện lại, kết nối được |
  | 4 | Bấm xem mật khẩu | Mật khẩu hiện đúng; kiểm tra `%TEMP%` không còn thư mục `QSU-WifiKeys-*`; log không chứa mật khẩu |

- **Trạng thái kiểm thử thực tế**: **chưa kiểm chứng trên máy thật**.


### 3.8. Dọn Rác Máy Tính — `DonRacMayTinh.exe`

- **Mục đích**: quét và dọn 8 hạng mục rác quen thuộc, báo dung
  lượng giải phóng thật (đo trước/sau).
- **Công nghệ**: Win32/FS gốc; thùng rác qua `SHEmptyRecycleBinW`;
  dịch vụ wuauserv qua `qsu_common::svc`.
- **Cơ chế tổ chức** — 8 hạng mục quét (`scan_items()`, kiểu
  `CleanKind`):

  | # | Hạng mục | Mặc định tick | Ghi chú |
  |---|---|---|---|
  | 1 | Temp người dùng (`%TEMP%`) | Có | |
  | 2 | Temp Windows (`C:\Windows\Temp`) | Có | |
  | 3 | Thùng rác | Không | Dung lượng = tổng thư mục `$Recycle.Bin` trên các ổ cố định |
  | 4 | Cache Windows Update (`SoftwareDistribution\Download`) | Không | Kiểu `WuCache` riêng (xem an toàn) |
  | 5 | Cache thumbnail/icon (`thumbcache_*.db`, `iconcache_*.db` trong `...\Explorer`) | Có | Kiểu `FilteredFiles`, chỉ xoá file khớp mẫu |
  | 6 | Cache Chrome | Không | Quét các profile Default/Guest Profile/Profile * |
  | 7 | Cache Edge | Không | `Cache\Cache_Data`, `Code Cache`, `GPUCache` |
  | 8 | Delivery Optimization | Có (nếu thư mục tồn tại) | |

  Quy tắc tick sẵn: một hạng mục được tick khi
  `default_on && dung_lượng > 0` (sửa từ phát hiện T3 — trước đây
  tick cả hạng mục 0 byte). Bấm Dọn → hộp xác nhận liệt kê hạng mục
  + tổng dung lượng → `spawn_clean()` xoá → quét lại và báo dung
  lượng đã giải phóng = trước − sau.
- **Hàm chính**: `scan_items()`, `delete_dir_contents()`,
  `spawn_clean()`, `browser_cache_dirs()`.
- **An toàn riêng**:
  - Phân loại mục bằng `DirEntry::file_type()`: **liên kết (junction/
    symlink) bị xoá như một liên kết, không bao giờ đi xuyên qua**
    để xoá nội dung đích (sửa từ phát hiện N2 — lỗi nghiêm trọng
    của bản trước rà soát).
  - Thùng rác chỉ tính dung lượng là đã dọn khi `SHEmptyRecycleBinW`
    trả HRESULT 0 (phát hiện H3).
  - Hạng mục cache Windows Update: chỉ dừng/khởi động lại `wuauserv`
    qua SCM **nếu dịch vụ đang chạy từ trước**; không ép trạng thái.
  - Không bao giờ đụng tới Downloads/Documents của người dùng.
  - Log `DonRac_Log.txt`.
- **Checklist kiểm thử**:

  | Bước | Thao tác | Kết quả mong đợi |
  |---|---|---|
  | 1 | Tạo sẵn file rác trong `%TEMP%`, mở module | Hạng mục Temp hiện đúng dung lượng gần khớp, được tick sẵn |
  | 2 | Để một hạng mục 0 byte | Hạng mục đó **không** được tick sẵn |
  | 3 | Tạo junction trong `%TEMP%` trỏ ra thư mục ngoài có file quan trọng, chạy dọn Temp | Junction bị xoá, file ở đích **nguyên vẹn** |
  | 4 | Dọn thùng rác | Thùng rác rỗng; dung lượng báo giải phóng khớp phần đã dọn |
  | 5 | Dọn cache Windows Update khi wuauserv đang chạy / đang dừng | Dịch vụ trở lại đúng trạng thái ban đầu trong cả hai trường hợp |

- **Trạng thái kiểm thử thực tế**: **chưa kiểm chứng trên máy thật**.

### 3.9. Quản Lý Update Windows — `QuanLyUpdateWindows.exe`

- **Mục đích**: xem trạng thái các dịch vụ cập nhật; bật/tắt Windows
  Update; khôi phục (reset) các thành phần Windows Update theo quy
  trình chuẩn của Microsoft; mở trang cài đặt cập nhật.
- **Công nghệ**: dịch vụ qua `qsu_common::svc` (SCM gốc); chính sách
  `NoAutoUpdate` ghi bằng registry API gốc vào
  `HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU`;
  reset dùng `regsvr32 /s` cho danh sách DLL chuẩn.
- **Cơ chế tổ chức**:
  1. Hiển thị trạng thái wuauserv / UsoSvc / BITS (`svc::query()`).
  2. **Bật** (`action_enable()`): xoá giá trị `NoAutoUpdate` → đặt
     cả ba dịch vụ về `START_DEMAND` → khởi động chúng.
  3. **Tắt** (`action_disable()`): dừng cả ba → đặt `START_DISABLED`
     → ghi `NoAutoUpdate = 1`.
  4. **Khôi phục** (`action_reset()`): dừng [wuauserv, bits,
     cryptsvc, msiserver, UsoSvc] → `regsvr32 /s` lần lượt 34 DLL
     trong hằng `RESET_DLLS` → khởi động lại [cryptsvc → AUTO;
     bits/wuauserv/UsoSvc → DEMAND], **chỉ sửa kiểu khởi động nếu
     đang bị Disabled** (phát hiện T4); `msiserver` được dừng nhưng
     không khởi động lại (chủ đích, khớp quy trình gốc — phát hiện
     D6).
  5. Nút "Kiểm tra cập nhật ngay" mở `ms-settings:windowsupdate`.
- **Hàm chính**: `SERVICES` (hằng danh sách), `RESET_DLLS` (34 mục),
  `set_no_auto_update()`, `action_enable()`, `action_disable()`,
  `action_reset()`.
- **An toàn riêng**: mọi thay đổi dịch vụ/chính sách đều ghi log
  `UpdateTool_Log.txt`; không xoá thư mục SoftwareDistribution
  (việc đó thuộc DonRacMayTinh, có cơ chế riêng).
- **Checklist kiểm thử**:

  | Bước | Thao tác | Kết quả mong đợi |
  |---|---|---|
  | 1 | Mở module | Ba dịch vụ hiện đúng trạng thái/kiểu khởi động thật |
  | 2 | Bấm Tắt Windows Update | Ba dịch vụ dừng + Disabled; registry có `NoAutoUpdate = 1` |
  | 3 | Bấm Bật Windows Update | `NoAutoUpdate` biến mất; dịch vụ về Manual và chạy |
  | 4 | Chạy Khôi phục thành phần | Log đủ 34 dòng regsvr32; cryptsvc về Automatic; các dịch vụ còn lại về Manual |
  | 5 | Bấm Kiểm tra cập nhật ngay | Trang Settings Windows Update mở ra |

- **Trạng thái kiểm thử thực tế**: **chưa kiểm chứng trên máy thật**.

### 3.10. Phím Tắt BIOS — `PhimTatBIOS.exe`

- **Mục đích**: tra cứu nhanh phím vào BIOS Setup và Boot Menu theo
  hãng máy (bảng tĩnh 16 hãng: Dell F2/F12, HP F10/F9, Lenovo
  F2|F1/F12, Asus F2/F8, Acer F2/F12, MSI Del/F11, Gigabyte Del/F12,
  ASRock F2/F11, Apple Option, Surface Vol+/Vol−+Power, Toshiba
  F2/F12, Samsung F2/F10|Esc, Sony VAIO F2/F11, Fujitsu F2/F12,
  Intel NUC F2/F10, LG F2/F10).
- **Công nghệ**: Rust + NWG; dữ liệu là hằng `HOTKEYS` trong mã
  nguồn; tìm kiếm bỏ dấu (`unaccented()`).
- **Cơ chế tổ chức**: mở lên là dùng; gõ tên hãng vào ô tìm kiếm →
  lọc bảng ngay. Không thao tác hệ thống nào.
- **Hàm chính**: `HOTKEYS` (hằng bảng), `unaccented()`.
- **An toàn riêng**: module duy nhất **không nâng quyền Admin**,
  không ghi log — vì không có gì để ghi.
- **Checklist kiểm thử**:

  | Bước | Thao tác | Kết quả mong đợi |
  |---|---|---|
  | 1 | Mở module bằng user thường | Mở ngay, không hộp UAC |
  | 2 | Gõ "giga" | Chỉ còn dòng Gigabyte (Del / F12) |

- **Trạng thái kiểm thử thực tế**: **chưa kiểm chứng trên máy thật**
  (module chỉ hiển thị dữ liệu tĩnh; rủi ro thực tế gần như bằng 0).

### 3.11. Tuỳ Chỉnh Windows — `TuyChinhWindows.exe`

- **Mục đích**: áp dụng/hoàn tác **49 tinh chỉnh (tweak)** Windows
  theo catalogue, xem trạng thái thật của từng tweak, và gỡ **27
  ứng dụng Appx** rác cho người dùng hiện tại. Catalogue tham chiếu
  Sophia Script (giấy phép MIT, ghi công trong hộp About của module).
- **Công nghệ**: engine registry **gốc** bằng Win32 API (module con
  `reg.rs`: RegOpenKeyExW/RegCreateKeyExW/RegQueryValueExW/
  RegSetValueExW/RegDeleteValueW/RegDeleteTreeW — không qua
  `reg.exe`); dịch vụ qua `qsu_common::svc`; hai ngoại lệ công cụ
  ngoài có ghi chép trong mã nguồn: `schtasks` (dò trạng thái qua
  XML `/Query /XML` vì tên phần tử XML không bị bản địa hoá) và
  `powercfg` (không có API công khai cho gói nguồn; trích GUID bằng
  mẫu hex). Điểm khôi phục và Appx dùng PowerShell
  (`Checkpoint-Computer`, `Get/Remove-AppxPackage`) — cũng là ngoại
  lệ có ghi chép vì không có Win32 API công khai tương đương.
- **Cơ chế tổ chức**:
  1. Catalogue `TWEAKS` trong `tweaks.rs`: mỗi tweak gồm `category`,
     tên hiển thị, mô tả, `risk`, `probe` (cách dò trạng thái) và
     hai danh sách `apply`/`undo` gồm các `Op` nguyên tử
     (`Dword/Sz/DelValue/DefaultSz/DelTree/ServiceStart/TaskEnabled/UltimatePower`).
     Phân bổ 5 nhóm: Quyền riêng tư & Telemetry 9, Giao diện &
     Explorer 11, Hiệu năng 9, Hệ thống 11, Menu ngữ cảnh & Tính
     năng 9 — cộng 49. Mức rủi ro trong code có **2 mức**: `Safe`
     (An toàn) và `Caution` (Cần cân nhắc).
  2. Mở module → `engine::probe()` đánh giá từng tweak:
     `Applied / NotApplied / Unknown` (dò registry/dịch vụ/tác vụ/
     gói nguồn đang active).
  3. Người dùng tick các tweak → Áp dụng/Hoàn tác
     (`engine::run_ops()` chạy từng `Op`, lỗi từng op được ghi log
     và đếm, không dừng cả loạt). **Lần áp dụng đầu tiên của mỗi
     phiên**, module hỏi có tạo điểm khôi phục trước không; đồng ý
     thì `engine::create_restore_point()` chạy (đặt
     `SystemRestorePointCreationFrequency = 0` bằng registry gốc để
     tránh giới hạn 24 giờ, `Enable-ComputerRestore`, rồi
     `Checkpoint-Computer`); tạo điểm khôi phục thất bại chỉ ghi log
     và vẫn tiếp tục theo lựa chọn của người dùng. Có nút tạo điểm
     khôi phục riêng.
  4. Gói nguồn Ultimate Performance: GUID của gói do module tạo
     được lưu trong file trạng thái `TuyChinhWindows_state.txt`
     cạnh .exe để dò/hoàn tác chính xác; hoàn tác = chuyển về gói
     Balanced (`381b4222-f694-41f0-9685-ff5bb260df2e`).
  5. Tab Appx: `appx::list_installed()` chỉ liệt kê các gói thuộc
     danh sách tuyển chọn `CURATED` (27 gói: 19 An toàn + 8 Cần cân
     nhắc) **và** đang thật sự cài cho người dùng hiện tại; gỡ bằng
     `Remove-AppxPackage` cho **riêng người dùng hiện tại**, chủ
     đích **không** gỡ gói provisioned. Trước khi ghép lệnh, 
     `PackageFullName` bị kiểm tra chỉ gồm chữ/số/`.`/`_`/`-`.
- **Hàm chính**: `tweaks.rs`: `TWEAKS`, `Op`, `Probe`, `TweakState`;
  `engine.rs`: `run_op()`, `run_ops()`, `probe()`,
  `create_restore_point()`, `task_is_disabled()`, `op_ultimate()`;
  `reg.rs`: `get_dword/get_sz/set_dword/set_sz/set_default_sz/delete_value/delete_tree/key_exists`;
  `appx.rs`: `CURATED`, `list_installed()`, `remove()`.
- **An toàn riêng**: mọi tweak đều có đường hoàn tác đối xứng trong
  catalogue (riêng các op xoá giá trị/cây khoá: hoàn tác hiểu theo
  nghĩa "đưa về mặc định Windows" của catalogue, không phải khôi
  phục giá trị cũ của máy — tester cần lưu ý điểm này); điểm khôi
  phục được đề nghị trước lần áp đầu tiên; Appx gỡ rồi cài lại được
  từ Microsoft Store.
- **Checklist kiểm thử** (thứ tự an toàn theo HUONG-DAN v0.4):

  | Bước | Thao tác | Kết quả mong đợi |
  |---|---|---|
  | 1 | Mở module, bấm Kiểm tra trạng thái | Mỗi tweak hiện một trong ba trạng thái; không tweak nào báo sai rõ rệt so với máy |
  | 2 | Bấm nút tạo điểm khôi phục | Điểm khôi phục xuất hiện trong System Restore (rstrui) |
  | 3 | Áp dụng tweak "Hiện đuôi file" rồi Hoàn tác | Đuôi file hiện/ẩn đúng theo từng bước; trạng thái probe đổi tương ứng sau khi kiểm tra lại |
  | 4 | Mở tab Appx | Chỉ liệt kê gói thuộc danh sách tuyển chọn đang cài; gỡ thử 1 gói an toàn (vd Tips) — gói biến mất với user hiện tại, user khác trên máy vẫn còn |
  | 5 | Kiểm tra log `TuyChinh_Log.txt` | Mỗi op có một dòng kết quả; op lỗi có dòng `LỖI:` kèm mô tả |

- **Trạng thái kiểm thử thực tế**: **chưa kiểm chứng trên máy thật**.

---

## 4. Module cài Windows — phần sâu

Phần này là đặc tả hành vi chi tiết của cặp wizard/engine, viết từ
mã nguồn hiện tại. Đây là phần tester cần đối chiếu kỹ nhất.

### 4.1. Ánh xạ giai đoạn thiết kế → mã nguồn

Đặc tả `docs/logic_cai_dat_win_online.md` chia quá trình thành các
pha 0, 1, 2, 3A, 3B. Hiện thực trong `cai_dat_win/src/main.rs`:

| Pha | Việc | Hàm trong code |
|---|---|---|
| 0 | Quét tiền đề (preflight) | `spawn_preflight()` → `run_preflight()` |
| 1 | Chọn nguồn ISO (quét / chọn tay / tải online) | `scan_isos_on_drive()`, `spawn_download()` |
| 2 | Gắn ISO, đọc danh sách edition | `mount_iso()`, `load_editions()` |
| 3A | Cài đè (in-place) | `exec_main()` → `exec_inplace()` |
| 3B | Cài mới (staging WinPE) | `exec_main()` → `exec_clean()` → engine `WinPEInstall.exe` |

### 4.2. Pha 0 — Preflight (`run_preflight()`)

Quét đồng bộ các thông tin sau và đưa ra phán quyết
(`verdict_of()`):

- **Hệ điều hành hiện tại**: `sys::os_info()` (tên, build, EditionID).
- **Firmware**: `probe_firmware()` dùng **hai** API — `GetFirmwareType`
  là quyết định; mẹo biến môi trường firmware là dự phòng;
  `FirmwareProbe::detail()` ghi giá trị thô của cả hai vào log để
  chẩn đoán các ca mâu thuẫn.
- **Kiểu phân vùng của đĩa hệ thống**: `sys::disk_partition_style()`.
- **BitLocker**: `probe_bitlocker()` qua PowerShell
  `(Get-BitLockerVolume -MountPoint 'X:').VolumeStatus.ToString()` →
  một trong `Decrypted / Encrypted / InProgress` (gồm cả các trạng
  thái đang treo như EncryptionInProgress, DecryptionInProgress,
  Suspended...) / `Unknown`.
- **Ổ Data**: ổ cố định nào chứa file marker `.qsu_data_drive`;
  `default_data_letter()` ưu tiên ổ có marker nhưng **không bao giờ
  chọn ổ hệ điều hành** (sửa từ phát hiện T2).
- **Dung lượng trống** trên ổ OS và ổ Data (đối chiếu
  `minFreeGbInPlace` / `minFreeGbClean` trong settings).
- **Nguồn điện**: `sys::power_status()` — cảnh báo khi chạy pin và
  pin < 60%.
- **Khoá OEM**: `sys::oem_key_present()` (chỉ kiểm tra tồn tại).

Phán quyết `verdict_of()`:

- **Chặn cứng**: BitLocker đang `InProgress` (đang mã hoá/giải mã
  dở) — không cho tiến hành.
- **Cảnh báo (vẫn cho đi tiếp)**: pin yếu không cắm điện; không tìm
  thấy ổ Data; dung lượng trống thấp.

Ngay khi quét xong — **kể cả khi người dùng chưa bấm cài** —
`persist_scan_log()` ghi toàn bộ log phiên quét ra
`<Data>:\QSU-Logs\wizard-scan-<yyyyMMdd-HHmm>.log` (dự phòng: cạnh
.exe của wizard) để giữ lại bằng chứng chẩn đoán (từ v0.7.2).

### 4.3. Pha 1–2 — Nguồn ISO và danh sách phiên bản

Ba đường lấy ISO:

1. **Quét tự động**: `scan_isos_on_drive()` quét file `*.iso` ở gốc
   các ổ dữ liệu và một cấp thư mục con.
2. **Chọn tay**: hộp chọn file.
3. **Tải online**: nếu `catalogueUrl` có giá trị, wizard tải
   catalogue JSON (`http::get_bytes()`), hiển thị danh sách;
   `spawn_download()` tải bằng `http::download_file()` (WinHTTP,
   Range resume) vào `<Data>:\<downloadFolder>`. **Bản ghi catalogue
   thiếu SHA-256 sẽ bị từ chối tải**; tải xong băm lại bằng
   `sha256::hash_file()` — không khớp thì **xoá file** và báo lỗi
   (khi so khớp tiền tố băm, code so theo ký tự `chars().take(16)`
   chứ không cắt byte — sửa từ phát hiện T1).

Gắn ISO (từ v0.7.17 — BA LỚP): `mount_iso()` thử lần lượt
(1) dùng lại mount có sẵn: sổ đăng ký mount trong tiến trình còn
giữ ký tự của chính ISO đó, hoặc `Get-DiskImage` báo Attached ở
tầng hệ thống thì lấy ký tự qua `Get-Volume`; (2) PowerShell
`Mount-DiskImage` với `$ErrorActionPreference = 'Stop'` — mọi
lệnh trong đường này thu cả stdout lẫn stderr và lỗi thất bại
được gộp thành mã thoát + nguyên văn lỗi của cmdlet (các bản
trước chỉ dựa vào lệnh này và khi nó nuốt lỗi, hộp thoại hiện
"(output: )" rỗng — thực địa trên máy Windows lite); (3) dự
phòng VirtDisk API gốc: `OpenVirtualDisk` (storage-type ISO) +
`AttachVirtualDisk` đọc-chỉ-đọc gọi thẳng từ Rust (FFI khai báo
tay, winapi 0.3 không có module virtdisk), ký tự ổ dò bằng hiệu
tập volume `GetLogicalDrives` trước/sau attach (ký tự DRIVE_CDROM
mới, ưu tiên ký tự có `sources\install.*`, thăm dò ~10 giây).
Ký tự lấy từ output `Get-Volume` vẫn là **ký tự chữ cái đầu
tiên** (heuristic cũ — phát hiện mở D4). `dismount_iso()` gỡ
đối xứng theo cơ chế đã mount ghi trong sổ đăng ký:
Dismount-DiskImage cho mount PowerShell, DetachVirtualDisk +
đóng handle cho mount VirtDisk. Cả ba lớp cùng hỏng thì hộp
thoại liệt kê lỗi của từng cơ chế.

Đọc edition: `load_editions()` chạy DISM `/English /Get-WimInfo`
lên `install.wim/.esd` trong ISO, phân tích bằng `parse_kv()`;
`parse_build()` lấy thành phần thứ 3 của trường Version làm build.
Edition mặc định: khớp `EditionID` với Windows hiện tại trước, rồi
mới tới khớp theo tên. **Cài đè bị chặn** nếu build ISO < build hiện
tại (kiểm tra trong luồng bắt đầu thực thi, thông báo rõ số build
hai phía).

### 4.4. Pha 3A — Cài đè (`exec_inplace()`)

Các bước chung của `exec_main()` trước khi rẽ nhánh (áp dụng cho cả
3A và 3B): tạo/cập nhật marker ổ Data (1%) → kiểm tra BitLocker:
nếu ổ OS đang `Encrypted`, **treo** BitLocker bằng
`manage-bde -protectors -disable` (không đặt RebootCount — phát
hiện mở D5) (3%) → sao lưu toàn bộ driver bằng
`pnputil /export-driver * <Data>:\QSU-DriverBackup` (10%; lỗi bước
này chỉ cảnh báo, không dừng).

Riêng 3A:

1. Chép thư mục `custom` (nếu có) vào
   `<OS>:\ProgramData\QSU\custom` (16%).
2. Ghi mốc trạng thái: file `install-state.json` trong thư mục log
   + giá trị `InstallState` tại `HKLM\SOFTWARE\QSU` (24%).
3. Chạy `setup.exe` của ISO với tham số:
   `/auto upgrade /quiet /showoobe none /DynamicUpdate disable /Compat IgnoreWarning /MigrateDrivers all /Telemetry disable` (40%).
4. `tail_panther()`: theo dõi `setupact.log` của Panther, poll mỗi
   2 giây, tối đa 1800 vòng, chỉ bắt các dòng chứa ký tự `%` để cập
   nhật tiến độ hiển thị.

Từ bước 3, quá trình nâng cấp do chính Windows Setup điều khiển;
công cụ chỉ quan sát log.

### 4.5. Pha 3B — Cài mới: staging (`exec_clean()`)

Wizard chuẩn bị mọi thứ trên ổ Data rồi đăng ký khởi động một lần
vào WinPE. Các mốc phần trăm là cố định trong code:

| Mốc | Việc |
|---|---|
| 18% | Tạo thư mục staging `<Data>:\<stagingFolder>` — có cổng chặn: staging **không được trùng gốc ổ Data, không được trùng gốc ổ OS** (lớp phòng vệ 2 của phát hiện N3). Tìm `WinPEInstall.exe` ở `modules\install\` trước, dự phòng cạnh wizard |
| 22–52% | Chép cây `sources` của ISO vào staging, báo tiến độ theo byte; sau đó `clear_readonly_tree()` gỡ thuộc tính chỉ-đọc khỏi cây vừa chép |
| — | `stage_boot_sdi()`: tìm và chép `boot.sdi` theo chuỗi dự phòng ở mục 4.7 |
| — | Chép thư mục `custom` (nếu có) vào staging |
| 56% | Xử lý `winpe.wim`: ưu tiên `settings.winpeWimPath` nếu có, không thì lấy `sources\boot.wim` của ISO; thiếu file là **lỗi cứng dừng staging**. Gỡ chỉ-đọc (`clear_readonly_file()`) |
| — | `probe_winpe_wim()`: đọc `/Get-WimInfo`, chọn index WinPE theo quy tắc mục 4.7, ghi log toàn bộ thông tin các index |
| — | `mount_inject_winpe()`: DISM mount WIM → tiêm file khởi động + `WinPEInstall.exe` vào `System32` (mục 4.7) → unmount `/Commit`. Nếu mount lỗi (điển hình 0xC1510111 do WIM còn cờ chỉ-đọc): tự thử lại bằng đường **export** (`/Export-Image /Compress:Max /Bootable`) sang file mới, probe lại, mount lại, rồi đổi tên đè lên `winpe.wim` |
| 69% | `repack_winpe_single_image()` (từ v0.7.15, **vô điều kiện** mọi ca cài mới): xuất riêng index đã inject sang `winpe_single.wim` bằng `/Export-Image /Bootable /Compress:Max`, xoá bản hai-ảnh, đổi tên thành `winpe.wim` (mục 4.7). Export lỗi hoặc file đích rỗng → **dừng trước BCD** |
| 71% | Dòng log cố định ghi rõ **cổng kiểm chứng cờ bootable đã bị gỡ** (từ v0.7.11) — wizard không phán cờ boot bằng output DISM; từ v0.7.15 WIM không còn dùng nguyên trạng mà đã đóng gói lại một ảnh ở mốc 69% |
| 74% | Chép driver lưu trữ vào `staging\Drivers`: từ `settings.storageDriversPath`, hoặc thư mục `drivers` cạnh wizard nếu settings để trống |
| 78% | Ghi `autounattend.xml` (mục 4.9) vào staging |
| 84% | `register_bcd()` đăng ký BCD (mục 4.7), sau đó `verify_bcd()` đối chiếu chẩn đoán (chỉ ghi log) |
| 92% | Ghi `stage.json` (mục 4.8) |
| 100% | Dismount ISO, hoàn tất; người dùng khởi động lại để vào WinPE |

### 4.6. Ba chế độ phân vùng và cổng chặn theo đĩa

Chế độ được chọn ở Tab Tuỳ chọn cài đặt (`PartMode { Preserve, WipeDisk, Rebuild }`,
ghi vào `stage.json` dưới khoá `partitionMode`: `preserve` /
`wipeDisk` / `rebuild`).

**preserve** — giữ nguyên toàn bộ sơ đồ phân vùng. Engine chỉ format
phân vùng Windows (nhận diện bằng volume serial đã lưu trong
`stage.json`) rồi cài lên đó. Đây là chế độ mặc định.

**wipeDisk** — xoá **toàn bộ một đĩa vật lý** (đĩa chứa Windows hiện
tại) và chia lại từ đầu theo `disk::wipe_layout()`:

- Người dùng chọn đĩa đích trong danh sách; `disk_block_reason()`
  loại khỏi danh sách: đĩa chứa ổ Data (kiểm tra bằng cả ký tự ổ lẫn
  disk extent), đĩa không có serial, đĩa dung lượng 0, đĩa chỉ thấy
  qua nguồn dự phòng mà không kiểm chứng được, đĩa < 25 GB.
- Bấm tiến hành: hộp xác nhận bắt **gõ lại serial của đĩa đích**,
  khớp mới cho đi tiếp.
- Engine kiểm tra lại trong WinPE (mã thoát 10/11/12 — mục 4.11).

**rebuild** (từ v0.7.13) — cho máy **một đĩa**: chỉ xoá và chia lại
các phân vùng hệ thống đã chọn, giữ nguyên các phân vùng còn lại
(điển hình: ổ D: chứa dữ liệu trên cùng đĩa).

- Wizard hiển thị sơ đồ phân vùng hiện tại của đĩa chứa Windows
  (`build_disk_layout()`), phân loại từng phân vùng bằng
  `classify_partition()` (đối chiếu ký tự ổ + volume serial):
  phân vùng **Windows** (vai trò "cài mới"), phân vùng chứa
  **staging** ("được bảo vệ", khoá không cho chọn xoá), phân vùng
  **Data** ("giữ nguyên", khoá), các phân vùng hệ thống
  EFI/MSR/Recovery (**đánh dấu sẵn** để xoá), còn lại do người dùng
  tự quyết.
- Vùng rebuild = tổng các phân vùng đã chọn; engine chia lại trong
  vùng đó theo `disk::rebuild_layout()` (EFI/MSR/Recovery mới +
  Windows chiếm phần còn lại của vùng).
- Cổng chặn `disk_block_reason()` ở chế độ rebuild **nhẹ hơn** wipe:
  chỉ chặn khi serial rỗng, dung lượng 0, hoặc đĩa từ nguồn dự
  phòng — vì đĩa đích ở chế độ này *được phép* chứa ổ Data (Data
  nằm ngoài vùng rebuild và được bảo vệ bởi cơ chế khớp phân vùng
  của `execute_rebuild()`).
- Xác nhận cũng bắt gõ lại serial đĩa.

### 4.7. boot.sdi, chọn index WIM, tiêm mã khởi động, BCD

**Chuỗi dự phòng `boot.sdi`** (`stage_boot_sdi()`, từ v0.7.5) — thử
lần lượt, mỗi vị trí đều ghi log kèm nhãn biến thể:

1. `<ISO đã gắn>:\sources\boot.sdi`
2. `<ISO đã gắn>:\boot.sdi` (gốc ISO)
3/4. `%WINDIR%\Boot\DVD\EFI\boot.sdi` và `%WINDIR%\Boot\DVD\PCAT\boot.sdi`
   — **thứ tự xoay theo firmware**: máy UEFI thử EFI trước, Legacy
   thử PCAT trước
5. `<OS>:\Recovery\WindowsRE\boot.sdi` (phân vùng Recovery)

Chép xong có kiểm chứng file đích; nếu cả 5 vị trí đều không có,
staging dừng với thông báo **liệt kê mọi vị trí đã thử**. Không bao
giờ dùng `boot.wim` thay cho `boot.sdi`.

**Chọn index WinPE** (`probe_winpe_wim()`) — thứ tự ưu tiên trong
code hiện tại:

1. Trường **"Boot Index"** trong phần tóm tắt của DISM `/Get-WimInfo`
   (ISO gốc Microsoft có trường này → chọn index 2, tức ảnh Setup —
   đây là hành vi có chủ đích, xem mục 5.6 điểm 2).
2. Nếu header không có Boot Index: chọn index mà tên ảnh chứa
   "windows pe" và **không** chứa "setup".
3. Index 1.
4. Index đầu tiên trong danh sách.

**Tiêm mã khởi động** (`mount_inject_winpe()`): ghi vào WIM đã mount
ba file có nội dung từ `qsu_start_cmd()` — `qsu_start.cmd` và
`startnet.cmd` (giống hệt nhau) trong `Windows\System32`, cộng
`winpeshl.ini` với mục `[LaunchApps]` trỏ tới
`%SYSTEMDRIVE%\Windows\System32\qsu_start.cmd`; chép
`WinPEInstall.exe` vào `System32`. Nội dung script khởi động:
chạy `wpeinit` → quét các ổ C:–Z: tìm `\<stagingFolder>\WinPEInstall.exe`,
chạy nó với tham số là tên thư mục staging → không thấy thì chạy
bản trong `System32` → cuối cùng `wpeutil reboot`.

**Đóng gói lại một ảnh** (`repack_winpe_single_image()`, từ
v0.7.15): ngay sau khi tiêm xong và **trước mọi bước BCD**, wizard
xuất riêng index đã inject sang `winpe_single.wim`:

```
dism /English /Export-Image /SourceImageFile:<staging>\winpe.wim
     /SourceIndex:<index đã inject>
     /DestinationImageFile:<staging>\winpe_single.wim
     /Bootable /Compress:Max
```

rồi xoá `winpe.wim` hai-ảnh và đổi tên bản một-ảnh thành
`winpe.wim` (gỡ cờ chỉ-đọc sau đổi tên, log kích thước mới).
Nguyên nhân: `winpe.wim` sau tiêm vẫn mang **cả hai ảnh** của
`boot.wim` gốc và header của nó không chỉ định ảnh PE đã inject
là ảnh boot — ca thực địa 2026-11-17 (log
`install-20261117-1746.log`: probe chọn index 1 theo tên lúc
17:48:47, tiêm xong 17:48:59, BCD đăng ký 17:49:19) cho thấy
winload đã nạp ảnh **Windows Setup** thay vì ảnh PE, máy lên
thẳng Setup đòi media driver. WIM một ảnh mang cờ boot thì
winload chỉ có thể khởi động đúng engine — cùng nguyên tắc
đóng gói lại của tool gốc. Bước này áp dụng **vô điều kiện**
(ISO mod hay gốc, mọi index đã chọn) và là bước đóng gói thuần
tuý: **không** đọc/phán cờ bootable từ output DISM (cổng kiểm
chứng của v0.7.7 vẫn gỡ hẳn); chỉ khi chính lệnh export trả mã
khác 0, hoặc file đích không tồn tại/rỗng, wizard mới dừng —
dừng **trước** đăng ký BCD nên máy chưa có mục boot mới nào.

**Đăng ký BCD** (`register_bcd()`) — công thức cố định:

```
bcdedit /create /d "QSU WinPE Ramdisk" /device
  → ramdisksdidevice = partition=<ổ Data>:
  → ramdisksdipath   = \<stagingFolder>\boot.sdi
bcdedit /create /d "QSU WinPE" /application osloader
  → device / osdevice = ramdisk=[<ổ Data>:]\<stagingFolder>\winpe.wim,{device-guid}
  → path       = \windows\system32\boot\winload.efi (UEFI)
                 hoặc \windows\system32\winload.exe (Legacy)
  → systemroot = \windows
  → winpe yes, detecthal yes
bcdedit /bootsequence {loader-guid}
```

Điểm cốt lõi: chỉ dùng **`/bootsequence`** (khởi động một lần duy
nhất) — **không** đụng vào `/displayorder`, nên thứ tự boot thường
trực của máy không đổi và không có nguy cơ vòng lặp WinPE. GUID
được trích từ output `bcdedit` bằng mẫu hex. Sau khi đăng ký,
`verify_bcd()` dump `bcdedit /enum all` và đối chiếu từng phần tử,
đồng thời kiểm tra chéo `{fwbootmgr}` — tất cả **chỉ ghi log**,
không chặn, không tự sửa/xoá mục BCD.

### 4.8. `stage.json` — schema đầy đủ

Ghi bởi wizard (cuối `exec_clean()`), đọc bởi engine
(`stage::StageConfig::from_json()`). Khoá JSON → ý nghĩa → mặc định
khi thiếu (theo `stage.rs`):

| Khoá | Kiểu | Mặc định | Ý nghĩa |
|---|---|---|---|
| `version` | số | 1 | Phiên bản schema |
| `mode` | chuỗi | `clean` | Chế độ tổng thể |
| `imageFile` | chuỗi | — | Tên file ảnh (install.wim/.esd) trong staging |
| `imageSwm` | chuỗi | — | Mẫu file .swm đi kèm (nếu ảnh bị chia nhỏ) |
| `imageIndex` | số | — | Index edition sẽ cài |
| `imageName` | chuỗi | — | Tên edition (log/hiển thị) |
| `bootMode` | chuỗi | `UEFI` | `UEFI` hoặc `Legacy` — **thiếu khoá này engine mặc định hiểu là UEFI** (rủi ro đã ghi trong rà soát UEFI) |
| `partitionMode` | chuỗi | `preserve` | `preserve` / `wipeDisk` / `rebuild` |
| `targetDiskSerial` | chuỗi | — | Serial đĩa đích (wipeDisk/rebuild) |
| `rebuildPartitions` | mảng | rỗng | Chỉ dùng cho rebuild: mỗi mục `{ kind, sizeBytes, gptGuid, offsetBytes }` mô tả phân vùng sẽ xoá/chia lại |
| `osVolumeSerial` | số | — | Volume serial của phân vùng Windows (preserve) |
| `osVolumeLabel` | chuỗi | — | Nhãn phân vùng Windows |
| `dataVolumeSerial` | số | — | Volume serial của ổ Data (đối chiếu an toàn) |
| `bcdGuid` | chuỗi | — | GUID mục BCD vừa đăng ký |
| `computerName` | chuỗi | — | Tên máy ghi vào unattend |
| `adminUser` | chuỗi | — | Tên tài khoản quản trị tạo mới |
| `adminPassword` | chuỗi | — | **Wizard luôn ghi rỗng rồi XOÁ khoá này khỏi JSON** (mục 4.9) |
| `ultraviewerPassword` | chuỗi | — | Như trên — tàn dư schema, luôn bị xoá |
| `applyTweaks` | bool | false | Công tắc tweak của wizard — **hiện chưa có thành phần nào tiêu thụ** (phát hiện D1) |
| `apps` | mảng | rỗng | Danh sách app hậu cài — wizard luôn ghi rỗng và xoá khoá |
| `driverBackupRel` | chuỗi | `QSU-DriverBackup` | Tên thư mục backup driver — engine hiện **không đọc** trường này (xem mục 4.10) |
| `logsFolder` | chuỗi | từ settings | Tên thư mục log trên ổ Data |
| `stagingFolder` | chuỗi | từ settings | Tên thư mục staging |

Tương thích ngược: engine đọc được `stage.json` của các bản cũ hơn
(thiếu `partitionMode` → hiểu là `preserve`; thiếu
`rebuildPartitions` → rỗng).

### 4.9. Unattend — quy tắc (`build_unattend()`)

Wizard sinh `autounattend.xml` với hai pass `specialize` và
`oobeSystem`:

- Locale/múi giờ lấy từ `settings.json` (`locale`, `uiLanguage`,
  `inputLocale`, `timezone`).
- OOBE: `HideEULAPage`, `HideOEMRegistrationScreen`,
  `HideOnlineAccountScreens`, `HideWirelessSetupInOOBE` đều bật;
  `ProtectYourPC = 3`.
- Tạo một `LocalAccount` thuộc nhóm Administrators + `AutoLogon`
  bật với `LogonCount = 1`.
- **Mật khẩu là tuỳ chọn** (từ v0.7.2): người dùng để trống mật
  khẩu thì XML **không có phần tử `<Password>`** ở cả hai vị trí
  (LocalAccount và AutoLogon) — tài khoản được tạo không mật khẩu,
  người dùng cuối tự đặt sau. Có mật khẩu thì ghi dạng `PlainText`.
- `FirstLogonCommands` để trống (không có agent hậu cài — mục 5.5).
- Mọi giá trị nội suy đều qua `xml_escape()`.
- Phía đích: mật khẩu quản trị **không nằm trong `stage.json`**
  (wizard xoá khoá trước khi ghi — mục 4.8); unattend nằm trong
  staging và được engine chép vào bản Windows mới dưới **cả hai
  tên** `autounattend.xml` và `unattend.xml` trong
  `Windows\Panther`.

Module `CaiDatWinWinPE` có bản sao `build_unattend()` độc lập với
cùng các quy tắc trên (kể cả mật khẩu tuỳ chọn).

**Hồ sơ CORE cho ảnh rút gọn (từ v0.7.21, branch
`tiny11-core-profile`)**: wizard (nút bật/tắt "Ảnh rút gọn
(Tiny11 Core)" ở tab Tuỳ chọn) và module WinPE (ô tick tương ứng)
có tuỳ chọn hồ sơ triển khai tối thiểu cho ảnh rút gọn kiểu
Tiny11 Core — ảnh loại này boot vòng lỗi 0xEF khi cài bằng hồ sơ
chuẩn nhưng lên bình thường khi cài tay tối thiểu kiểu
WinNTSetup. Khi bật: unattend sinh ra **không có khối
`FirstLogonCommands`** (mọi phần còn lại giữ nguyên);
`stage.json` ghi thêm `imageProfile: "core"` (thiếu trường hiểu
là `"standard"`); sau khi bung ảnh, engine/module bỏ qua bước
chuẩn bị hậu cài và ghi `Windows\Setup\Scripts\SetupComplete.cmd`
chứa đúng một lệnh `net user <admin> /logonpasswordchg:no` —
Windows chạy file này bằng quyền SYSTEM ở cuối pha setup, trước
lần đăng nhập đầu, thay cho các lệnh dọn cờ ở
`FirstLogonCommands` của hồ sơ chuẩn; ảnh đã có sẵn
SetupComplete.cmd thì nối thêm dòng, không ghi đè. Cơ chế đầy
đủ: `docs/co_che_v0.7.21.md` mục 13.

### 4.10. Driver: sao lưu và inject — ranh giới thật

Cần phân biệt rõ hai đường driver khác nhau, vì tài liệu/tab Tài
liệu trong wizard từng mô tả gộp:

1. **Sao lưu driver của Windows hiện tại**: `pnputil /export-driver *`
   vào `<Data>:\QSU-DriverBackup` (bước 10% của `exec_main()`). Thư
   mục này là **bản sao lưu độc lập** để kỹ thuật viên dùng sau
   (khôi phục tay hoặc bằng module BackupRestoreDriver). Từ v0.7.14
   engine WinPE **có** inject từ thư mục này: sau khi inject
   `staging\Drivers`, engine đọc trường `driverBackupRel` trong
   `stage.json` và inject thêm từ `<Data>:\QSU-DriverBackup` bằng
   DISM `/Add-Driver /Recurse` (chỉ khi tên tương đối an toàn và
   thư mục không rỗng; các bản ≤ v0.7.13 không đọc trường này).
2. **Inject driver vào bản Windows mới**: engine inject từ
   `staging\Drivers` — tức **driver lưu trữ** (IRST/NVMe...) do
   người dùng chuẩn bị qua `settings.storageDriversPath` hoặc thư
   mục `drivers` cạnh wizard (mốc 74%) — và từ v0.7.14 thêm
   `QSU-DriverBackup` (driver của Windows hiện tại, xem mục 1).
   Engine chạy DISM `/Add-Driver /Recurse` offline sau khi bung
   ảnh; lỗi inject chỉ là cảnh báo, không dừng cài đặt. Module
   `CaiDatWinWinPE` cũng inject từ thư mục driver đi kèm nguồn
   theo cùng nguyên tắc (lỗi = cảnh báo).

### 4.11. Engine `WinPEInstall.exe` — luồng và mã thoát

Luồng chạy trong WinPE (hàm `run()` của `winpe_install/src/main.rs`):

1. Tham số dòng lệnh 1 = tên thư mục staging (mặc định `QSU-Stage`).
   `find_stage()` quét các ổ A:–Z: tìm thư mục staging chứa cả
   marker và `stage.json`; không thấy → **thoát 2**. Log mở tại
   `<Data>:\<logsFolder>\winpe-<yyyyMMdd-HHmm>.log`.
2. Đối chiếu volume serial của ổ Data với `stage.json` — lệch →
   **thoát 3**.
3. Chặn đĩa đích trùng đĩa Data / marker nằm trên đĩa đích →
   **thoát 5**. File ảnh không có trong staging → **thoát 6**.
4. Theo `partitionMode`:
   - **preserve**: `find_os_drive()` tìm phân vùng Windows **chỉ
     bằng volume serial** (serial = 0 hoặc không khớp → không tìm
     được, **không có dự phòng heuristic** — sửa từ phát hiện N1)
     → không thấy thì **thoát 4**. `format_os_partition()` format
     NTFS nhãn `Windows` bằng `format.com`, dự phòng diskpart →
     lỗi **thoát 7**.
   - **wipeDisk**: thiếu `targetDiskSerial` / không tìm thấy đĩa
     hoặc khớp mơ hồ → **thoát 10**; đĩa đích chứa ổ Data hoặc
     không xác định được đĩa Data → **thoát 11**;
     `disk::execute_wipe()` lỗi → **thoát 12**.
   - **rebuild**: thiếu serial, `rebuildPartitions` rỗng, không tìm
     thấy đĩa, ổ Data nằm trên đĩa đích nhưng không đọc được
     extent, hoặc không rõ đĩa Data → **thoát 13** (mọi trường hợp
     đều dừng **trước khi đụng vào đĩa**); `execute_rebuild()` lỗi
     → **thoát 13**.
   - **Tự gán ký tự ổ cho phân vùng boot (từ v0.7.16, cả wipeDisk
     và rebuild)**: sau khi diskpart chia xong, `execute_wipe()` /
     `execute_rebuild()` đối chiếu layout thật; phân vùng boot
     (EFI/System Reserved) khớp theo đĩa + loại phân vùng, và nếu
     nó thiếu ký tự ổ — hoặc liệt kê báo có mà truy cập thử không
     được — `disk::ensure_boot_letter()` TỰ gán (ưu tiên `S:`;
     script diskpart phụ `select partition`/`assign`, dự phòng
     `SetVolumeMountPointW` theo tên volume khớp (đĩa, offset);
     thử tối đa 2 lần) và chốt bằng probe thật `GetFileAttributesW`
     trên đường dẫn gốc — liệt kê volume hiện ký tự KHÔNG được
     tính là thành công. Chỉ dừng (thoát 12/13 như cũ) khi phân
     vùng boot vắng mặt hẳn trong layout mới hoặc gán + probe đều
     thất bại. Sửa ca thực địa `winpe-20261117-1811.log`: diskpart
     báo gán `S` thành công, exit 0, nhưng EFI vẫn không có ký tự
     và bản cũ huỷ toàn bộ ca cài ngay sau bước xoá đĩa.
5. Bung ảnh: DISM `/Apply-Image` (streaming, có `/SWMFile` nếu ảnh
   chia nhỏ) → lỗi **thoát 8**.
6. Inject driver từ `staging\Drivers`, rồi từ v0.7.14 thêm từ
   `<Data>:\QSU-DriverBackup` (mục 4.10; kết quả được bỏ qua — lỗi
   chỉ ghi log).
7. Chép unattend vào `Windows\Panther` dưới cả hai tên file; chép
   thư mục `custom` vào `ProgramData\QSU\custom` (chỉ sao chép).
8. Nếu staging có `PostInstall.exe` do người dùng tự đặt: chép vào
   `ProgramData\QSU` và đăng ký offline qua hive registry
   (`reg load HKLM\QSU_OFFLINE`, khoá RunOnce `QSU-PostInstall`,
   giá trị Winlogon `QSUFirstLogon = 1`); không có thì chỉ ghi log.
9. Chế độ wipe/rebuild: `disk::copy_winre()` khôi phục môi trường
   Recovery vào phân vùng Recovery mới (best-effort).
10. Tạo boot bằng `bcdboot`:
    - wipe/rebuild: `/s <phân vùng boot mới> /f UEFI|BIOS` theo
      `bootMode`.
    - preserve (từ v0.7.19): `disk::finalize_boot()` dùng chung —
      tìm phân vùng boot TRÊN CÙNG ĐĨA với phân vùng Windows
      theo loại phân vùng (GPT: EFI System theo GUID; MBR:
      System Reserved active, không có thì chính phân vùng
      Windows), thiếu ký tự ổ thì tự gán (`ensure_boot_letter`),
      `bcdboot /f UEFI|BIOS`, rồi kiểm chứng file boot thật
      (`bootmgfw.efi` + `BCD` / `bootmgr` + `Boot\BCD`); thử
      lại một lần; lỗi mang theo output của bcdboot. (Đường cũ
      `find_efi_letter()` — quét thư mục `\EFI` theo ký tự ổ +
      `mountvol /s` — đã bị xoá; đó là rủi ro R1, đã đóng.)
    - `bcdboot` lỗi → **thoát 9 và KHÔNG khởi động lại** (để người
      tại chỗ còn đọc được log/màn hình).
11. Thành công: `wpeutil reboot`; chỉ khi `wpeutil` lỗi mới dùng
    `shutdown /r /t 10` (tránh khởi động lại hai lần — phát hiện H4).

Bảng mã thoát tổng hợp:

| Mã | Ý nghĩa |
|---|---|
| 0 | Thành công (đang khởi động lại vào Windows mới) |
| 2 | Không tìm thấy thư mục staging / `stage.json` |
| 3 | Volume serial ổ Data không khớp `stage.json` |
| 4 | preserve: không xác định được phân vùng Windows theo serial |
| 5 | Đĩa đích trùng đĩa Data |
| 6 | File ảnh cài đặt không có trong staging |
| 7 | Format phân vùng Windows thất bại |
| 8 | DISM `/Apply-Image` thất bại |
| 9 | `bcdboot` thất bại (không tự khởi động lại) |
| 10 | wipeDisk: thiếu serial / không tìm thấy đĩa đích / khớp mơ hồ |
| 11 | wipeDisk: đĩa đích chứa ổ Data hoặc không rõ đĩa Data |
| 12 | wipeDisk: `execute_wipe()` lỗi |
| 13 | rebuild: mọi lỗi kiểm tra đầu vào hoặc `execute_rebuild()` |

### 4.12. File log — vị trí và tên

| File | Vị trí | Ai ghi | Khi nào |
|---|---|---|---|
| `wizard-scan-<yyyyMMdd-HHmm>.log` | `<Data>:\QSU-Logs\` (dự phòng: cạnh wizard) | Wizard | Ngay sau mỗi lần quét preflight, kể cả chưa cài |
| `install-<yyyyMMdd-HHmm>.log` | `<Data>:\QSU-Logs\` | Wizard | Toàn bộ phiên thực thi cài đặt (kèm 9 dòng `[Cơ chế]` giải thích từng bước từ v0.7.12) |
| `winpe-<yyyyMMdd-HHmm>.log` | `<Data>:\<logsFolder>\` | Engine WinPEInstall | Mỗi lần engine chạy trong WinPE; kịch bản diskpart được ghi nguyên văn vào đây |
| `winpe-manual-<yyyyMMdd-HHmm>.log` | Ổ chứa nguồn cài | CaiDatWinWinPE | Mỗi phiên cài tương tác |
| `CaiDatWinTuDong-panic.log` | Cạnh `CaiDatWinTuDong.exe` (+ bản sao trong `QSU-Logs` khi biết thư mục) | Wizard | Khi có panic bị bắt — "hộp đen" chẩn đoán |
| `install-state.json` | Thư mục log | Wizard (cài đè) | Mốc trạng thái trước khi chạy setup.exe |
| `DriverTool_Log.txt` / `Bitlocker_Log.txt` / `WifiTool_Log.txt` / `DonRac_Log.txt` / `UpdateTool_Log.txt` / `TuyChinh_Log.txt` | Cạnh .exe của từng module | Từng module v0.3/v0.4 | Mỗi phiên chạy |
| `TuyChinhWindows_state.txt` | Cạnh `TuyChinhWindows.exe` | TuyChinhWindows | Lưu GUID gói nguồn Ultimate Performance do module tạo |

---

## 5. Hạn chế & lỗi đã biết

### 5.1. D1 — Công tắc "áp tweak sau cài" trong wizard là no-op (chưa sửa)

Wizard có công tắc tuỳ chọn áp dụng tweak sau khi cài (giá trị khởi
tạo từ `settings.defaultApplyTweaks`), và giá trị này **được ghi
vào `stage.json`** ở khoá `applyTweaks`. Tuy nhiên **không có thành
phần nào trong toàn bộ mã nguồn tiêu thụ khoá này**: engine WinPE
không đọc nó, và không có agent hậu cài nào tồn tại để áp tweak sau
khi Windows mới boot. Hệ quả thực tế: bật hay tắt công tắc này
**không thay đổi hành vi cài đặt**. Đây là phát hiện mở D1 từ đợt
rà soát v0.4, giữ nguyên tới v0.7.13, chờ quyết định hướng xử lý
(bỏ công tắc khỏi giao diện, hay xây cơ chế tiêu thụ thật).

### 5.2. Lỗi hằng số kiểu ListView — checkbox có thể không hiện (đã sửa ở v0.7.14)

Phát hiện trong đợt rà soát v0.7.13, **đã sửa ở v0.7.14**: bốn crate
khai báo hằng số kiểu mở rộng ListView sai giá trị:

```rust
const LVS_EX_CHECKBOXES: LPARAM = 0x400;   // SAI
```

`0x400` thực ra là `LVS_EX_INFOTIP`; hằng đúng của
`LVS_EX_CHECKBOXES` là `0x4`. Hằng này được gửi qua message
`LVM_SETEXTENDEDLISTVIEWSTYLE` trong hàm `lv_enable_checkboxes()`
của từng module. Các file dính lỗi (đã kiểm chứng trực tiếp trong
mã nguồn):

| File | Dòng |
|---|---|
| `rust/tuy_chinh_windows/src/main.rs` | 169 |
| `rust/don_rac/src/main.rs` | 310 |
| `rust/backup_restore_driver/src/main.rs` | 356 |
| `rust/sao_luu_wifi/src/main.rs` | 175 |

Riêng wizard `CaiDatWinTuDong.exe` dùng đúng `0x4` (dòng 360) nên
không bị ảnh hưởng. Triệu chứng kỳ vọng trên máy thật: ở 4 module
trên, các ô checkbox trong danh sách (chọn tweak, chọn hạng mục
dọn rác, chọn driver, chọn hồ sơ Wi-Fi) **có thể không hiển thị
hoặc không hoạt động** — đây là ca kiểm thử ưu tiên cao vì nó chạm
vào thao tác chọn của người dùng (xem mục 6, ca T-01). Lưu ý tài
liệu nội bộ trước đây chỉ nêu 2 module (Tuỳ Chỉnh Windows, Dọn
Rác); rà soát mã nguồn cho thấy phạm vi thật là 4 — xem mục 5.6.

### 5.3. Các phát hiện đã sửa trong đợt rà soát v0.4 (để tester không báo lại)

Đợt rà soát toàn bộ v0.4 (`docs/kiem_tra_v0.4.md`) đã sửa 13 lỗi —
liệt kê tóm tắt để tester biết hành vi hiện tại là *có chủ đích*:

- **N1**: engine bỏ dự phòng heuristic khi tìm phân vùng Windows —
  chỉ khớp volume serial (mục 4.11).
- **N2**: DonRacMayTinh không đi xuyên junction khi xoá (mục 3.8).
- **N3**: hai lớp chặn `stagingFolder` nguy hiểm (settings + lúc
  tạo staging) (mục 2.6, 4.5).
- **T1**: so khớp tiền tố SHA-256 theo ký tự, không cắt byte (mục 4.3).
- **T2**: ổ Data mặc định không bao giờ là ổ OS (mục 4.2).
- **T3**: DonRacMayTinh không tick sẵn hạng mục 0 byte (mục 3.8).
- **T4**: reset Windows Update chỉ sửa kiểu khởi động khi đang
  Disabled (mục 3.9).
- **H1**: parser JSON tự strips BOM (mục 2.2).
- **H2**: buffer đọc sơ đồ đĩa tăng từ 64 B lên 64 KiB (mục 2.8).
- **H3**: dung lượng thùng rác chỉ tính khi lệnh xoá trả thành công
  (mục 3.8).
- **H4**: engine tránh khởi động lại hai lần sau khi cài xong
  (mục 4.11).
- **H5**: gói phát hành kèm `settings.example.json` trong cả thư
  mục `modules\`.
- **H6**: chú thích trong `stage.rs` được sửa cho khớp hành vi.

### 5.4. Các phát hiện mở D2–D7 (chưa sửa, theo `docs/kiem_tra_v0.4.md`)

| Mã | Nội dung | Ảnh hưởng thực tế |
|---|---|---|
| D1 | Công tắc tweak trong wizard không có nơi tiêu thụ | Xem mục 5.1 |
| D2 | Người dùng bấm **Cancel ở hộp UAC** thì tiến trình thoát lặng, không thông báo | Tester có thể tưởng chương trình "không mở được"; hành vi đúng hiện tại là thoát im lặng |
| D3 | Output XML của `schtasks /Query /XML` có rủi ro encoding UTF-16 trên một số bản Windows | Probe trạng thái tác vụ trong TuyChinhWindows có thể trả `Unknown` oan |
| D4 | `mount_iso()` lấy ký tự ổ là ký tự chữ cái đầu tiên trong output PowerShell | Heuristic; output bất thường → không gắn được ISO (fail-safe) chứ không gắn nhầm |
| D5 | Treo BitLocker bằng `manage-bde -protectors -disable` không đặt RebootCount | Về lý thuyết bộ bảo vệ có thể tự bật lại nếu máy khởi động lại nhiều lần trước khi cài |
| D6 | Reset Windows Update không khởi động lại `msiserver` | Chủ đích (khớp quy trình gốc của Microsoft) — không phải lỗi |
| D7 | Nhóm nhỏ: tải file gặp HTTP 416 thì xoá tải lại toàn bộ thay vì coi như đã đủ; việc xoá khoá mật khẩu khỏi `stage.json` chưa thành API dùng chung; `Logger` bỏ qua lỗi ghi file; 4 cảnh báo clippy dạng style | Ảnh hưởng nhẹ tới trải nghiệm/độ tin cậy log; không ảnh hưởng an toàn dữ liệu |

### 5.5. Những gì CHƯA TỪNG chạy trên máy thật & mốc còn thiếu

Tính đến v0.7.13 (2026-11-17), các thành phần sau **chưa kiểm chứng
trên máy thật** — mọi mô tả về chúng trong tài liệu này là từ mã
nguồn và rà soát tĩnh:

- **Một bản cài Windows trọn vẹn từ đầu tới cuối bằng chính bộ công
  cụ này** — *đây là mốc (milestone) còn thiếu của toàn dự án*.
  Lần tiến xa nhất: staging v0.7.6 hoàn tất 100%, BCD đăng ký xong,
  máy khởi động lại vào mục boot một lần nhưng WinPE không lên được
  (Boot Manager báo 0xc000000e với WIM của ISO đã tuỳ biến trên VM
  Legacy). Chưa lần nào engine chạy tới bước bung ảnh trên máy thật.
- Engine `WinPEInstall.exe`: cả ba chế độ preserve / wipeDisk /
  rebuild trong WinPE thật; toàn bộ bảng mã thoát 2–13 mới chỉ đúng
  ở mức mã nguồn (riêng các cổng chặn phía wizard của wipeDisk đã
  thấy hoạt động qua hộp xác nhận trên VM ở v0.7.2).
- Chế độ **rebuild** của v0.7.13 (gồm `execute_rebuild()` và mã
  thoát 13): chưa chạy lần nào ngoài biên dịch + rà soát.
- Module `CaiDatWinWinPE.exe`: ĐÃ chạy trong WinPE thật lần đầu
  2026-11-18 (xem trạng thái ở §3.4) — các bản sửa v0.7.19 rút
  ra từ buổi đó chưa được kiểm chứng lại.
- Cả 6 module v0.3 (BackupRestoreDriver bản Rust, TatBitlocker,
  SaoLuuWifi, DonRacMayTinh, QuanLyUpdateWindows, PhimTatBIOS) và
  `TuyChinhWindows.exe` (v0.4 → nay).
- Các bản v0.7.11, v0.7.12, v0.7.13 của wizard: chưa phát hành chạy
  thực địa lần nào (bản thực địa gần nhất là v0.7.9 trên
  DESKTOP-704R89L và v0.7.6 trên DESKTOP-J8LPB75).
- Rủi ro từ rà soát UEFI (`docs/kiem_tra_uefi_v0.7.8.md`):
  **R1 — ĐÃ ĐÓNG ở v0.7.19**: `find_efi_letter()` (ở cả engine
  và CaiDatWinWinPE) từng nhận diện phân vùng ESP bằng dấu hiệu
  có thư mục `\EFI` ở gốc ổ — không thấy EFI chưa có ký tự ổ và
  có thể chọn nhầm ESP của đĩa khác; ca thực địa 2026-11-18 đã
  xác nhận hậu quả (bcdboot thất bại sau apply 100%). Cả hai
  nơi nay dùng `disk::finalize_boot()` — đối chiếu theo đĩa +
  loại phân vùng. **R2** — phía WinPE,
  firmware được dò bằng một API duy nhất (`firmware_is_uefi()`),
  kém dự phòng hơn probe hai API của wizard. Kết luận của rà soát
  UEFI: đường UEFI đã viết đúng, không có lỗi chặn; Secure Boot
  tương thích vì mọi binary trong chuỗi boot đều do Microsoft ký,
  file WIM/SDI chỉ là dữ liệu, và các .exe của QSU chỉ chạy ở
  user-mode trong WinPE.

Ngoài ra: cơ chế **agent hậu cài đặt** (tự cài ứng dụng + đặt mật
khẩu truy cập từ xa ở lần đăng nhập đầu tiên) là **chủ đích không
xây dựng** — không phải tính năng thiếu. `stage.json` vì thế luôn
có `apps` rỗng và không chứa mật khẩu nào.

### 5.6. Điểm lệch giữa tài liệu cũ và mã nguồn hiện tại

Các điểm dưới đây được ghi lại nguyên trạng để tester không bị
nhầm — tài liệu này đi theo **mã nguồn**, và các tài liệu cũ chưa
được sửa lại:

1. **Phạm vi lỗi hằng số checkbox**: ghi chú trong mã nguồn wizard
   (`cai_dat_win/src/main.rs` khoảng dòng 357) và `BUILD_NOTES.md`
   mục v0.7.13 nói lỗi `0x400` chỉ có ở "module Tuỳ Chỉnh Windows
   và Dọn Rác". Kiểm chứng trực tiếp: **4 crate** dính lỗi, thêm
   `backup_restore_driver` và `sao_luu_wifi` (bảng ở mục 5.2).
2. **Thứ tự ưu tiên chọn index WinPE**: HUONG-DAN của v0.7.8 và
   `BUILD_NOTES.md` mục v0.7.8 mô tả cơ chế như thể chọn **theo
   tên ảnh** là chính. Mã nguồn hiện tại (`probe_winpe_wim()`) đặt
   **Boot Index của header lên trước** — chọn theo tên chỉ khi
   header thiếu Boot Index; `docs/kiem_tra_uefi_v0.7.8.md` §6 mô tả
   đúng thứ tự này. Hệ quả thực tế cần tester biết: với ISO gốc
   Microsoft, công cụ chọn **index 2 (ảnh Setup)** theo thiết kế.
3. **Công nghệ tải ISO**: đặc tả `logic_cai_dat_win_online.md` §3
   viết tải bằng BITS (`Start-BitsTransfer`). Hiện thực là WinHTTP
   (`qsu_common::http::download_file`, Range resume).
4. **Nhận diện phân vùng Windows trong WinPE**: đặc tả §6 nói
   engine nhận diện "bằng marker và nhãn ổ". Hiện thực là
   **serial-only**, dự phòng heuristic đã bị loại ở phát hiện N1.
5. **Agent hậu cài trong đặc tả**: §1/§7 của đặc tả mô tả agent
   chạy ở lần đăng nhập đầu tiên như một thành phần kiến trúc.
   Hiện thực: chủ đích không xây (mục 5.5); `FirstLogonCommands`
   trong unattend để trống; thư mục `custom` chép nguyên là cơ chế
   thay thế.
6. **Driver inject**: dòng log `[Cơ chế]` ở mốc 74% trong wizard và
   cách diễn đạt ở tab Tài liệu (`HUONG_DAN_TEXT`/`TAI_LIEU_TEXT`
   nhúng trong wizard, tương ứng `docs/co_che_v0.7.13.md` §4) dễ
   gây hiểu rằng driver đã sao lưu vào `QSU-DriverBackup` được
   engine nạp vào bản Windows mới. Mã nguồn ở các bản ≤ v0.7.13:
   engine **chỉ** inject từ `staging\Drivers` (driver lưu trữ);
   `QSU-DriverBackup` không được chép vào staging và trường
   `driverBackupRel` không được engine đọc (mục 4.10). **Đã sửa ở
   v0.7.14**: engine inject thêm từ `QSU-DriverBackup` trên ổ Data
   (đọc `driverBackupRel`, có kiểm tra tên tương đối an toàn).
7. **Giao diện**: ghi chú hiện thực hoá v1.1 trong đặc tả nói wizard
   dùng "theme tối như launcher". Từ v0.5 toàn bộ 9 GUI đã chuyển
   về giao diện Windows chuẩn sáng.
8. **`README.md` ở gốc dự án** vẫn mô tả kiến trúc PowerShell/PS2EXE
   của v0.1 — đã lỗi thời từ v0.2. Tài liệu gốc hiện hành là
   `BUILD_NOTES.md` và `HUONG-DAN.txt` trong gói phát hành (và tài
   liệu này).
9. **Số mức rủi ro của tweak**: `BUILD_NOTES.md` mục v0.4 mô tả
   catalogue tweak có "3 mức rủi ro"; enum `Risk` trong
   `tweaks.rs`/`appx.rs` chỉ có **2 mức** (`Safe`, `Caution`).
10. **`settings.example.json`** không chứa hai khoá `postInstallApps`
    và `ultraviewerPassword` dù mô hình `Settings` vẫn đọc chúng
    (tàn dư schema cũ; wizard không dùng khi ghi `stage.json` —
    mục 2.6). Không ảnh hưởng hành vi, nhưng dễ gây thắc mắc khi
    đối chiếu.
11. **`docs/plan_ban_nang_cao.md`** (đặc tả bản nâng cao dùng phân
    vùng boot riêng — **chưa xây dựng**, đã đóng băng) ở mục 11 còn
    ghi "bản đang dùng để test" là v0.7.12; bản hiện tại là
    v0.7.13. Các kịch bản kiểm thử của tài liệu đó chỉ áp dụng một
    phần cho bản hiện tại (đã lọc ở mục 6).

---

## 6. Ma trận kiểm thử tổng hợp

Gộp từ các `HUONG-DAN.txt` (v0.3 → v0.7.13), hai bản rà soát, và
phần áp dụng được của 12 kịch bản nghiệm thu trong
`docs/plan_ban_nang_cao.md` (kịch bản gốc viết cho bản nâng cao
chưa xây dựng; ở đây chỉ lấy các kịch bản vẫn đúng với cơ chế hiện
tại và ghi rõ nguồn). Thứ tự = ưu tiên giảm dần. Cột "Kỳ vọng" là
kết quả đúng theo mã nguồn hiện tại — sai lệch so với cột này là
lỗi cần báo, kèm log tương ứng (mục 4.12).

### Nhóm P0 — Cổng an toàn phá huỷ & mốc trọn vẹn (chạy trước tiên)

| # | Kịch bản | Thao tác tóm tắt | Kết quả mong đợi | Nguồn |
|---|---|---|---|---|
| T-01 | Checkbox ở 4 module cũ | Mở TuyChinhWindows, DonRacMayTinh, BackupRestoreDriver, SaoLuuWifi; kiểm tra ô tick trong danh sách có hiện và tick được không | Đã sửa ở v0.7.14 (hằng `0x400` → `0x4`, mục 5.2): kỳ vọng checkbox hiện và tick được ở cả 4 module — nếu còn module nào không hiện thì ghi nhận lại | v0.7.14 |
| T-02 | Gõ sai serial khi wipeDisk | Wizard → cài mới → wipeDisk → gõ serial không khớp ở hộp xác nhận | Bị chặn, không có thao tác đĩa nào xảy ra | HUONG-DAN v0.7 |
| T-03 | Máy 1 đĩa chọn wipeDisk | Máy chỉ có 1 đĩa chứa cả Windows và Data | Đĩa bị loại khỏi danh sách đích wipe (đĩa chứa ổ Data); không có đường nào wipe được đĩa này — phải dùng rebuild | HUONG-DAN v0.7 + plan §9 ca 5 |
| T-04 | stage.json cũ (thiếu khoá mới) | Đặt stage.json kiểu cũ (không `partitionMode`/`rebuildPartitions`) vào staging, boot WinPE | Engine hiểu là `preserve`, chạy như bản cũ | HUONG-DAN v0.7.13 + plan §9 ca 10 |
| T-05 | **Cài trọn vẹn preserve trên máy thật 2 đĩa (MỐC)** | Wizard cài mới preserve với ISO gốc Microsoft, Secure Boot BẬT; để máy tự reboot vào WinPE | WinPE boot lên, engine bung ảnh xong (log `winpe-*.log` không mã thoát lỗi), `bcdboot` xong, máy vào Windows mới; kiểm tra `bcdedit /enum` sau cài: **không còn mục QSU trong displayorder**, boot thường trực không đổi (không vòng lặp WinPE) | Milestone dự án + plan §9 ca 8 |
| T-06 | Tài khoản không mật khẩu | Cài mới với mật khẩu admin để trống | Windows mới có tài khoản admin không mật khẩu, đăng nhập được; `autounattend.xml` không có phần tử `<Password>`; `stage.json` không có khoá `adminPassword` | HUONG-DAN v0.7.2 + plan §9 ca 12 |
| T-07 | Máy VMD/RAID | Máy bật Intel VMD/RAID, đặt driver IRST vào thư mục `drivers` cạnh wizard (hoặc `storageDriversPath`) | Driver được chép vào `staging\Drivers` (log mốc 74%), engine inject offline, Windows mới boot lên nhận ổ | plan §9 ca 11 |

### Nhóm P1 — Chế độ rebuild (mới ở v0.7.13) & wipeDisk

| # | Kịch bản | Thao tác tóm tắt | Kết quả mong đợi | Nguồn |
|---|---|---|---|---|
| T-08 | Rebuild máy 1 đĩa GPT | Máy 1 đĩa có C: (Windows) + D: (dữ liệu); rebuild giữ nguyên tuỳ chọn mặc định | Sau cài: D: **nguyên vẹn từng file**; EFI/Recovery được tạo mới; Windows mới boot lên | HUONG-DAN v0.7.13 ca 1 |
| T-09 | Rebuild bỏ tick Recovery | Bỏ chọn phân vùng Recovery cũ ở bước phân vùng | Recovery cũ được **giữ nguyên** sau cài; các phân vùng đã chọn mới bị chia lại | HUONG-DAN v0.7.13 ca 2 |
| T-10 | Rebuild gõ sai serial | Gõ serial không khớp ở hộp xác nhận rebuild | Bị chặn ở wizard, không thao tác đĩa | HUONG-DAN v0.7.13 ca 3 |
| T-11 | Hồi quy preserve/wipe sau khi có rebuild | Chạy lại một ca preserve và một ca wipeDisk chuẩn | Hành vi không đổi so với v0.7.12 trở về trước | HUONG-DAN v0.7.13 ca 4 |
| T-12 | Wipe máy 2 đĩa GPT | Đĩa đích là đĩa chứa Windows, đĩa Data ở đĩa khác | Đĩa đích bị xoá sạch, chia lại EFI 260 MB + MSR 16 MB + Windows + Recovery 600 MB (nhãn Windows/Recovery đúng); đĩa Data không bị đụng | HUONG-DAN v0.7 |
| T-13 | Wipe máy Legacy/MBR | Máy Legacy, đĩa đích MBR | Bố cục MBR: System Reserved 100 MB (active) + Windows + Recovery 600 MB (`set id=27`) | HUONG-DAN v0.7 |
| T-14 | Serial đĩa đổi giữa chừng | Sau khi wizard ghi stage.json, thay ổ đĩa đích bằng đĩa khác rồi boot WinPE | Engine dừng với mã thoát 10 (không tìm thấy đĩa theo serial), đĩa mới không bị đụng | HUONG-DAN v0.7 |
| T-15 | Rút/cắm lại USB giữa chừng | Staging trên USB, rút cắm lại làm ký tự ổ thay đổi trước khi boot WinPE | Engine vẫn tìm thấy staging theo marker + serial, không phụ thuộc ký tự ổ | HUONG-DAN v0.7 |

### Nhóm P2 — Wizard: hồi quy các điểm đã từng lỗi

| # | Kịch bản | Thao tác tóm tắt | Kết quả mong đợi | Nguồn |
|---|---|---|---|---|
| T-16 | ISO mod không có `sources\boot.sdi` | Dùng ISO đã tuỳ biến thiếu boot.sdi trong sources | Chuỗi dự phòng tìm thấy boot.sdi ở vị trí khác (gốc ISO / Boot\DVD / Recovery), log ghi rõ vị trí đã dùng; chỉ dừng khi cả 5 vị trí đều thiếu, và thông báo liệt kê đủ 5 vị trí | v0.7.5 |
| T-17 | WIM còn cờ chỉ-đọc (0xC1510111) | ISO có boot.wim read-only sau khi chép | Wizard tự gỡ cờ; nếu mount vẫn lỗi, đường export dự phòng chạy và staging hoàn tất, lỗi hiện đọc được (không còn mã hex trơ) | v0.7.6 |
| T-18 | ISO mod không có Boot Index trong header | boot.wim của ISO mod (index 1 = "Microsoft Windows PE", index 2 = "Windows Setup") | Chọn index theo tên → index 1; log `/Get-WimInfo` đầy đủ các index | v0.7.8 |
| T-19 | ISO gốc Microsoft | boot.wim gốc (header có Boot Index) | Chọn index 2 theo Boot Index — hành vi có chủ đích (mục 5.6 điểm 2) | Rà soát UEFI §6 |
| T-20 | Cài đè bằng ISO cũ hơn | ISO có build thấp hơn Windows hiện tại, chọn cài đè | Bị chặn với thông báo nêu số build hai phía | Thực địa 2026-11-17 |
| T-21 | Bấm TIẾN HÀNH rồi xác nhận (ca "chết im lặng" cũ) | Chạy staging trên cửa sổ wizard ở vị trí bất kỳ, kể cả sát mép màn hình | Cửa sổ **không** biến mất; log chạy liên tục có các dòng `[step]`; nếu có lỗi nội bộ, xuất hiện `CaiDatWinTuDong-panic.log` thay vì tắt lặng | v0.7.3/v0.7.4 |
| T-22 | Log quét tồn tại độc lập | Chỉ mở wizard cho quét preflight xong rồi đóng, chưa cài | File `wizard-scan-*.log` đã có trên ổ Data với đầy đủ kết quả quét + dòng chẩn đoán `[Đĩa]` | v0.7.2 |
| T-23 | Cửa sổ trên màn hình nhỏ | Mở wizard ở độ phân giải thấp / thanh taskbar cao | Cửa sổ tự co theo work-area, kéo giãn được, không phần tử nào bị khuất; tối thiểu 900×640 | v0.7.9/v0.7.10 |

### Nhóm P3 — Module tương tác WinPE và các module tiện ích

| # | Kịch bản | Thao tác tóm tắt | Kết quả mong đợi | Nguồn |
|---|---|---|---|---|
| T-24 | CaiDatWinWinPE trọn gói | Theo 7 bước trong checklist mục 3.4 (USB WinPE → chọn nguồn → đích → cài → reboot) | Cài xong Windows khởi động được; log `winpe-manual-*.log` đầy đủ | HUONG-DAN v0.6 |
| T-25 | Thứ tự test an toàn 6 module v0.3 | Lần lượt: PhimTatBIOS → SaoLuuWifi (xuất thử 1 hồ sơ) → BackupRestoreDriver (sao lưu 1 driver) → DonRacMayTinh (quét trước, dọn sau) → QuanLyUpdateWindows (chỉ xem trạng thái) → TatBitlocker (chỉ trên máy có volume mã hoá dự phòng) | Từng module đúng checklist ở mục 3.5–3.10 | HUONG-DAN v0.3 |
| T-26 | TuyChinhWindows theo thứ tự an toàn | Kiểm tra trạng thái → tạo điểm khôi phục → áp + hoàn tác tweak "Hiện đuôi file" → mở tab Appx xem danh sách | Đúng checklist mục 3.11; log `TuyChinh_Log.txt` đủ dòng từng op | HUONG-DAN v0.4 |
| T-27 | Huỷ UAC | Mở bất kỳ module nào cần Admin, bấm Cancel ở hộp UAC | Tiến trình thoát lặng (hành vi hiện tại — phát hiện D2); ghi nhận để quyết định có sửa hay không | Rà soát v0.4 |
| T-28 | Launcher đủ 32 nút | Mở launcher, bấm thử các nút chưa có module | Đủ 32 nút; nút chưa có module báo "Chưa có module"; tìm kiếm không dấu lọc đúng | HUONG-DAN v0.2 |

---

## 7. Lịch sử phiên bản

Mỗi phiên bản một dòng, theo `BUILD_NOTES.md` và các gói phát hành
`drop_v*/`. Mọi bản đều là bản test nội bộ.

| Phiên bản | Ngày | Nội dung chính |
|---|---|---|
| v0.1 | 2026-11-16 | Bản mẫu PowerShell: launcher WinForms 32 nút + module BackupRestoreDriver.ps1 (port từ source dịch ngược); đã chạy thật trên IT-DJGQZ. |
| v0.2 | 2026-11-16 | Chuyển hẳn sang Rust: launcher + wizard `CaiDatWinTuDong` + engine `WinPEInstall` + thư viện `qsu_common`; agent hậu cài chủ đích không xây. |
| v0.3 | 2026-11-16 | Thêm 6 module Rust: BackupRestoreDriver, TatBitlocker, SaoLuuWifi, DonRacMayTinh, QuanLyUpdateWindows, PhimTatBIOS. |
| v0.4 | 2026-11-16 | Thêm TuyChinhWindows (49 tweak + 27 Appx, catalogue tham chiếu Sophia Script MIT); rà soát toàn bộ mã nguồn: sửa 13 lỗi (3 nghiêm trọng, 4 trung bình, 6 nhỏ), còn 6 phát hiện mở D1–D7. |
| v0.5 | 2026-11-17 | Bỏ giao diện tối tự vẽ ở cả 9 GUI, chuyển về giao diện Windows chuẩn sáng, font và nút lớn hơn — chỉ thay trình bày, không đổi logic. |
| v0.6 | 2026-11-17 | Thêm module tương tác `CaiDatWinWinPE` (chạy trong WinPE + chế độ Chuẩn bị trên Windows thường). |
| v0.7 | 2026-11-17 | Chế độ wipeDisk: xoá toàn bộ đĩa đích theo serial, chia lại GPT/MBR kèm WinRE; `qsu_common::disk` ra đời; engine thêm mã thoát 10/11/12. |
| v0.7.1 | 2026-11-17 | Sửa liệt kê đĩa trống trên máy thật: `enumerate_disks()` coi đĩa tồn tại khi mở được handle, quét 0..16, hợp nhất dự phòng Get-Disk/diskpart, thêm chẩn đoán `[Đĩa]`. |
| v0.7.2 | 2026-11-17 | Mật khẩu admin thành tuỳ chọn (bỏ phần tử `<Password>` khi để trống); log quét preflight được lưu ngay (`wizard-scan-*.log`) kể cả khi chưa cài. |
| v0.7.3 | 2026-11-17 | Chống "chết im lặng" (giả thuyết cửa sổ bị khuất): phản hồi tức thì, cửa sổ tự dịch vào vùng nhìn thấy, `catch_unwind` cho luồng staging, hộp đen panic. |
| v0.7.4 | 2026-11-17 | Tìm ra gốc lỗi thật của v0.7.3: NWG `Notice` dispatch đồng bộ gây tái nhập + `RefCell` double-borrow → abort; sửa bằng mượn ngắn/`try_borrow`/`catch_unwind` ở handler + mốc `[step]` — đã kiểm chứng thực địa. |
| v0.7.5 | 2026-11-17 | Chuỗi dự phòng `boot.sdi` 5 vị trí, thứ tự xoay theo firmware; thông báo lỗi liệt kê mọi vị trí đã thử. |
| v0.7.6 | 2026-11-17 | Sửa DISM 0xC1510111 (gỡ cờ chỉ-đọc trên file chép từ ISO) + đường export dự phòng khi mount lỗi; thông báo lỗi lệnh ngoài đọc được; staging hoàn tất trọn vẹn lần đầu và reboot vào mục boot một lần (WinPE chưa lên — 0xc000000e). |
| v0.7.7 | 2026-11-17 | Thêm cổng kiểm chứng cờ bootable của WIM (dừng an toàn trước BCD nếu WIM không boot được); probe firmware hai API + log giá trị thô; `verify_bcd()` chẩn đoán chỉ-ghi-log. |
| v0.7.8 | 2026-11-17 | Chọn index WinPE theo tên ảnh khi header thiếu Boot Index (bản này không phát hành riêng). |
| v0.7.9 | 2026-11-17 | Cửa sổ wizard thích ứng: cap theo work-area, khung kéo giãn/maximize, layout co giãn theo trọng số, tối thiểu 900×640. |
| v0.7.10 | 2026-11-17 | Wizard gom còn 2 tab, khối tiến độ/log/nút ghim dưới khung tab; khung thiết kế 1036×748 (bản này không phát hành riêng). |
| v0.7.11 | 2026-11-17 | **Gỡ hẳn cổng kiểm chứng cờ bootable** theo chỉ thị người dùng (cổng đã chặn oan 4/4 lần chạy thực địa, kể cả ISO gốc Microsoft) — dùng WinPE nguyên trạng của ISO. |
| v0.7.12 | 2026-11-17 | Thêm tab Hướng dẫn + Tài liệu (tổng 4 tab) và 9 dòng `[Cơ chế]` giải thích trong log cài đặt; logic cài đặt không đổi so với v0.7.11. |
| v0.7.13 | 2026-11-17 | Chế độ phân vùng **rebuild** cho máy 1 đĩa (giữ phân vùng dữ liệu, `stage.json` thêm `rebuildPartitions`, engine thêm mã thoát 13); rà soát phát hiện lỗi hằng số checkbox `0x400` ở các module cũ và lỗi driver sao lưu không tới được engine. |
| v0.7.14 | 2026-11-17 | Sửa hai lỗi trên: hằng số checkbox `0x400` → `0x4` ở 4 module (Tuỳ Chỉnh Windows, Dọn Rác, Backup/Restore Driver, Sao Lưu Wifi); engine inject thêm driver đã sao lưu từ `QSU-DriverBackup` trên ổ Data (đọc `driverBackupRel`, kiểm tra tên tương đối an toàn). |
| v0.7.15 | 2026-11-18 | **Đóng gói lại WinPE thành WIM một ảnh bootable** trong wizard (`repack_winpe_single_image()`, mốc 69%): sau khi inject, xuất riêng index đã inject bằng `/Export-Image /Bootable /Compress:Max` rồi thay `winpe.wim` — sửa ca thực địa 2026-11-17 winload nạp nhầm ảnh Windows Setup vì WIM hai-ảnh không có Boot Index chỉ định ảnh PE. Không khôi phục cổng kiểm chứng DISM-text đã gỡ ở v0.7.11. |
| v0.7.16 | 2026-11-18 | **Engine tự gán ký tự ổ cho phân vùng boot sau khi chia** (`disk::ensure_boot_letter()` trong `qsu_common`, dùng chung cho `execute_wipe()` và `execute_rebuild()`): sửa ca thực địa `winpe-20261117-1811.log` — bản repack v0.7.15 đã đưa được engine lên, diskpart wipe báo thành công mọi bước kể cả `assign letter=S`, nhưng liệt kê sau wipe thấy phân vùng EFI không có ký tự ổ và bước xác minh cũ huỷ toàn bộ ca cài ngay sau bước phá huỷ. Nay phân vùng boot thiếu ký tự thì engine tự gán (ưu tiên `S:`, diskpart phụ / `SetVolumeMountPointW`, thử lại 1 lần) và chốt bằng probe truy cập thật đường dẫn gốc; chỉ dừng khi phân vùng vắng mặt hẳn hoặc gán + probe đều thất bại. |
| v0.7.17 | 2026-11-18 | **Mount ISO ba lớp trong wizard** + **UX xác nhận serial**: sửa ca thực địa trên máy Windows lite — "Đọc ISO" với ISO gốc báo "Mount-DiskImage không trả ký tự ổ đĩa (output: )" rỗng vì đường mount cũ chỉ dựa vào một lệnh PowerShell và lỗi cmdlet bị nuốt. Nay `mount_iso()` dùng lại mount có sẵn → PowerShell với lỗi ép terminating và hiện nguyên văn → dự phòng VirtDisk API gốc (ký tự ổ dò bằng hiệu tập volume); `dismount_iso()` gỡ đối xứng theo sổ đăng ký mount. Vùng xác nhận serial (cả wipeDisk/rebuild): thêm dòng định danh đĩa đích đầy đủ ngay trên ô nhập + nhãn phản hồi khớp/chưa khớp trực tiếp theo từng ký tự (cùng `normalize_disk_serial` như chốt chặn lúc bấm TIẾN HÀNH) + danh sách đĩa xổ xuống rộng 780px để không cắt serial. Chỉ wizard thay đổi; engine giữ nguyên. |
| v0.7.18 | 2026-11-18 | **Hiển thị tiến trình trong WinPE**: sửa ca thực địa `winpe-20261117-1852.log` — engine chạy khoẻ (ensure_boot_letter tự gán T:, DISM bung ảnh tới 82%) nhưng bị tưởng là treo và bị tắt nguồn vì không có bề mặt hiển thị nào: cửa sổ không xuất hiện trong PE của ISO mod, console launcher trống vì engine chỉ ghi file log. Nay engine có hai bề mặt: (1) console của `qsu_start.cmd` là bề mặt CHÍNH — Logger gắn sink in mọi dòng log thẳng ra console, giai đoạn in dòng riêng, % bung ảnh cập nhật tại chỗ bằng dòng `\r` nhờ `cmd::run_streaming_progress()` mới (cắt output DISM ở cả `\r`/`\n`); `qsu_start.cmd` không đổi. (2) Màn hình tiến trình toàn màn hình tông xanh tự vẽ bằng GDI (theo màn hình mẫu của tool gốc, thương hiệu bản rebuild) là bề mặt PHỤ — mở trước mọi công việc trên luồng riêng, gồm bước hiện tại + số % lớn + thanh tiến trình + đồng hồ `mm:ss` chạy từng giây + đường dẫn log; % ngoài bước bung ảnh là tiến độ tổng thể theo giai đoạn, trong bước bung ảnh là % trực tiếp của DISM. Khi lỗi engine neo lại trong WinPE giữ lỗi trên cả hai bề mặt (mã thoát chuyển thành giá trị trả về của `run()`, logic cài đặt không đổi). Wizard chỉ đổi nội dung tab Tài liệu (mục 11 mới). |
| v0.7.19 | 2026-11-18 | **Chốt boot cùng đĩa cho đường preserve + module WinPE tự mount ISO / trình duyệt file tích hợp / chống kẹt nút + dọn cờ đổi mật khẩu**: sửa 4 lỗi từ buổi test thực địa cùng ngày trên DESKTOP-704R89L (WinPE USB BHT, cài ISO gốc theo phân vùng). (1) `disk::finalize_boot()` mới trong qsu_common, nối vào CaiDatWinWinPE (theo phân vùng) và engine preserve: tìm phân vùng boot TRÊN CÙNG ĐĨA với phân vùng Windows theo loại phân vùng (GPT: EFI System; MBR: System Reserved active), tự gán ký tự bằng `ensure_boot_letter`, bcdboot + kiểm chứng file boot thật, thử lại 1 lần, lỗi kèm output của bcdboot; xoá cả hai bản sao `find_efi_letter()` (đóng rủi ro R1 — ca thực địa: apply 100% nhưng bcdboot thất bại vì EFI của SSD không có ký tự ổ, phải chạy bcdboot tay). (2) Module WinPE mount ISO bằng VirtDisk API qua module mới `qsu_common::iso` — bỏ hẳn ngõ cụt "không có PowerShell/cmdlet Storage"; lỗi attach có hướng dẫn dự phòng mount sẵn + chọn thẳng `sources\install.*`. (3) Trình duyệt file TÍCH HỢP thay `nwg::FileDialog` (hộp thoại Windows không mở được trong WinPE BHT): liệt kê ổ đĩa, duyệt thư mục, lọc *.iso/*.wim/*.esd/*.swm, tên đầy đủ, hai chế độ file/thư mục; ô gõ đường dẫn giữ nguyên. Chống kẹt nút: luồng nền nạp nguồn/liệt kê bọc `catch_unwind` (busy luôn được nhả), hộp thoại trong `refresh()` trì hoãn sau `set_busy_ui()`. (4) `build_unattend()` ở cả wizard và module WinPE thêm hai FirstLogonCommands (`net user <Admin> /logonpasswordchg:no` + `Set-LocalUser -PasswordNeverExpires`) dọn cờ "phải đổi mật khẩu ở lần đăng nhập đầu" (pwdLastSet=0) — ca thực địa AutoLogon bị chặn ở màn hình đổi mật khẩu; áp dụng cả khi mật khẩu trống, kèm dòng `[Cơ chế]` ở cả hai nơi sinh. Wizard thêm mục 12 vào tab Tài liệu. |
| v0.7.20 | 2026-11-18 | **Chuẩn hoá giao diện toàn bộ (chỉ lớp trình bày — hành vi không đổi)**: thêm design system `qsu_common::ui` (sáu font chuẩn Segoe UI/Consolas dạng getter thay 10 bản sao chép tay, token lề 16/khe 8/header (16,8)+(16,40)/nút cao 32/ô nhập 28, `add_header`, `make_primary` cho một nút chính mỗi cửa sổ, hộp thoại chuẩn) và icon cửa sổ chung `qsu.ico` nhúng trong qsu_common cho mọi cửa sổ. Launcher: header về toạ độ chuẩn, lưới nút dùng một công thức cho cả dựng/lọc, icon đủ **32/32** nút (9 glyph gốc giữ toạ độ + 23 glyph mới cùng ngôn ngữ nét 2px mực thép xanh RGB(40,93,145), sinh bởi `packaging/draw-icons.py`). Các module gom font/header/icon; ô log kỹ thuật về Consolas; hàng nút hành động đồng đều 32; cột nội dung của Tắt BitLocker/Sao Lưu Wifi dịch lề 14→16. Ngoại lệ có ghi trong chuẩn: header wizard giữ (16,6)/(16,36) vì khung tab ở y=60; màn tiến trình WinPE toàn màn hình giữ nguyên thiết kế v0.7.18; engine WinPEInstall không sửa mã nguồn (build lại vì thư viện chung mở rộng). Chuẩn + rà soát từng cửa sổ: `docs/chuan_giao_dien.md`. |
| v0.7.21 | 2026-11-20 | **Hồ sơ CORE cho ảnh rút gọn (Tiny11 Core)** — trên branch `tiny11-core-profile` (main giữ v0.7.20 làm đường lui): tuỳ chọn triển khai tối thiểu cho ảnh rút gọn vốn boot vòng CRITICAL_PROCESS_DIED (0xEF) khi cài bằng hồ sơ chuẩn nhưng lên ngon khi cài tay kiểu WinNTSetup. Wizard thêm nút "Ảnh rút gọn (Tiny11 Core)" ở tab Tuỳ chọn; module WinPE thêm ô tick tương ứng. `build_unattend()` (cả hai bản sao, đồng bộ) thêm tham số hồ sơ: chuẩn = khối `FirstLogonCommands` giữ nguyên từng byte; CORE = bỏ hẳn khối đó. `stage.json` thêm trường `imageProfile` (core/standard, mặc định standard, tương thích ngược). Engine WinPEInstall + module WinPE: hồ sơ CORE bỏ qua bước chuẩn bị agent hậu cài, ghi `Windows\Setup\Scripts\SetupComplete.cmd` tối thiểu (`net user <admin> /logonpasswordchg:no`; nối thêm nếu ảnh đã có sẵn file) để dọn cờ đổi mật khẩu ở cuối pha setup, trước lần đăng nhập đầu. Tài liệu: `docs/co_che_v0.7.21.md` mục 13; tab Tài liệu của wizard đồng bộ từng chữ. |

---

*Hết tài liệu. Khi báo lỗi, luôn đính kèm: phiên bản gói, máy test
(UEFI/Legacy, số đĩa), chế độ cài, và trọn bộ log theo mục 4.12.*
