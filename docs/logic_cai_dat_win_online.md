# Logic module Cài Đặt Windows Online / Từ Xa (bản build mới)

Phiên bản logic: v1 — 2026-11-16
Module ưu tiên số 1 của người dùng. Tài liệu này là đặc tả logic để triển khai,
tổng hợp từ: giao diện module gốc (ảnh chụp "Cài Đặt Windows Tự Động AIO"),
bản đồ chức năng (maps/module_map.md, maps/deep_cai_dat_win.md) và kỹ thuật
cài Windows chuẩn của Microsoft. Các điểm chưa đối chiếu được với ruột gốc
(đang bị Enigma mã hóa) được đánh dấu **[VERIFY]** — sẽ kiểm chứng bằng
memory dump ở giai đoạn sau và chỉnh lại tài liệu này.

## 0. Mục tiêu và kịch bản sử dụng

- Kỹ thuật viên điều khiển máy đích **từ xa** (RDP / S-remote / TeamViewer),
  mở module trên **Windows đang chạy** của máy đích.
- Sau khi bấm **TIẾN HÀNH CÀI ĐẶT**, toàn bộ quá trình phải **không cần
  thao tác thêm**: máy tự reboot nhiều lần và hoàn tất tới màn hình desktop.
- Ổ chứa dữ liệu người dùng (ổ Data) **tuyệt đối không bị đụng vào** ở cả
  hai chế độ cài.
- Mọi giai đoạn đều ghi log có timestamp; kỹ thuật viên kết nối lại sau
  bất kỳ lần reboot nào cũng xem được máy đang ở bước nào.

## 1. Kiến trúc 3 phần

1. **Wizard (giao diện):** quét hệ thống → chọn nguồn ISO → chọn phiên bản
   → chọn chế độ cài → xác nhận → bắt đầu. Sau khi bắt đầu, wizard chuyển
   vai trò thành màn hình theo dõi (đọc log của engine).
2. **Engine cài đặt:** hai nhánh — Cài đè (setup.exe) và Cài mới từ xa
   (staging WinPE + DISM). Xem §4, §5.
3. **Hậu cài (first-logon agent):** một script được cài sẵn vào bản Windows
   mới, tự chạy ở lần đăng nhập đầu tiên: khôi phục driver, áp profile
   tuỳ chỉnh, dọn dẹp, viết báo cáo hoàn tất. Xem §7.

## 2. Giai đoạn 0 — Kiểm tra tiền đề (chặn nếu không đạt)

Chạy ngay khi module mở, hiển thị dạng danh sách đạt/không đạt như bản gốc:

| Hạng mục | Cách kiểm tra | Điều kiện qua |
|---|---|---|
| Quyền quản trị | WindowsPrincipal IsInRole(Administrator) | Bắt buộc, thiếu thì tự xin nâng quyền |
| Nguồn điện | Get-CimInstance Win32_Battery | Laptop phải cắm sạc (hoặc pin ≥ 60% và có cảnh báo) |
| OS hiện tại | RtlGetVersion / registry ProductName, EditionID, CurrentBuild | Xác định edition + build để đối chiếu ISO |
| Boot Mode | Kiểm tra thư mục firmware / bcdedit /enum | UEFI hoặc Legacy; ghi nhận kèm kiểu đĩa GPT/MBR (Get-Disk PartitionStyle) |
| BitLocker ổ OS | manage-bde -status C: | Phải "Fully Decrypted" hoặc sẽ tự suspend ở bước chuẩn bị; đang mã hóa dở dang → CHẶN |
| Dung lượng trống ổ OS | Get-PSDrive | ≥ 25 GB cho cài đè (setup + Windows.old); ≥ 12 GB cho cài mới |
| Ổ Data | Các phân vùng không chứa OS | Chọn ổ Data = phân vùng dữ liệu lớn nhất; đây là nơi tìm ISO và ghi log/báo cáo |
| Đánh dấu an toàn ổ Data | Tạo/đọc file marker `<Data>:\.qsu_data_drive` | Chỉ những ổ có marker hoặc được người dùng xác nhận mới được coi là ổ Data |

Kết quả quét hiển thị đúng bố cục bản gốc: Ổ Cài Windows (OS), Ổ Chứa Dữ
Liệu (Data), Boot Mode, Trạng Thái BitLocker — kèm dòng kết luận an toàn.

## 3. Giai đoạn 1 — Nguồn ISO

Hai nguồn, ưu tiên local trước:

1. **ISO có sẵn trên máy:** quét `*.iso` ở thư mục gốc và thư mục con cấp 1
   của mọi ổ Data; hiển thị tên file + dung lượng; người dùng chọn 1 file
   (hoặc nút "Chọn ISO" để duyệt tay). Đây là hành vi đã thấy ở bản gốc.
2. **Tải từ kho online của người dùng** (thay kho của tác giả): một file
   catalogue JSON đặt trên hạ tầng của người dùng, định dạng:

   ```json
   { "images": [ { "name": "Windows 11 Pro 24H2 x64",
                   "edition": "Professional",
                   "url": "https://<server-cua-nguoi-dung>/iso/win11pro.iso",
                   "size": 5800000000,
                   "sha256": "<64 hex>" } ] }
   ```

   Tải bằng BITS (Start-BitsTransfer) để có resume khi mất mạng; **kiểm
   SHA-256 bắt buộc** sau khi tải, sai hash thì xoá và báo lỗi. URL
   catalogue cấu hình được trong file settings của module — không hard-code
   server của bất kỳ ai.

Sau khi có ISO: mount bằng `Mount-DiskImage` (API có sẵn từ Windows 8,
không cần cài thêm ImDisk như bản gốc — kết quả tương đương: một ký tự ổ
đĩa chứa bộ cài). **[VERIFY]** bản gốc dùng ImDisk; bản mới dùng API hệ
thống cho sạch, hành vi người dùng nhìn thấy không đổi.

## 4. Giai đoạn 2 — Chọn phiên bản (edition)

- Tìm `sources\install.wim` / `install.esd` / `install.swm` trong ISO đã mount.
- `dism /Get-WimInfo /WimFile:<path>` liệt kê mọi index: tên, edition,
  build, kiến trúc, ngôn ngữ.
- **Đề xuất mặc định:** index có edition trùng edition đang chạy
  (Pro ↔ Professional...) để giữ nguyên kích hoạt digital license — đúng
  dòng gợi ý đã thấy trên giao diện bản gốc ("Đề xuất chọn…").
- Kiểm tra tương thích:
  - Cài đè giữ ứng dụng: yêu cầu build ISO ≥ build hiện tại và cùng ngôn
    ngữ nền; không đạt → vô hiệu hoá chế độ cài đè, chỉ cho cài mới.
  - Đọc OEM key từ BIOS (OA3 MSDM) nếu có; hiển thị "Key OEM: Có/Không"
    như bản gốc. Không có key và không có digital license → cảnh báo
    Windows sau cài sẽ chưa kích hoạt (vẫn cho tiếp tục).

## 5. Giai đoạn 3A — Cài đè trên Windows hiện tại (giữ dữ liệu + ứng dụng)

Cơ chế chuẩn của Microsoft, chạy `setup.exe` từ ISO đã mount:

```
setup.exe /auto upgrade /quiet /showoobe none /DynamicUpdate disable
          /Compat IgnoreWarning /MigrateDrivers all /Telemetry disable
```

Logic đi kèm:

- Trước khi chạy: suspend BitLocker (`manage-bde -protectors -disable C:`)
  nếu đang bật; tự gọi module Backup Driver sao lưu driver hiện tại vào
  ổ Data (phòng khi bản mới thiếu driver).
- Trạng thái "đang cài đè" được ghi vào registry + file trạng thái trên
  ổ Data để wizard mở lại sau reboot vẫn hiển thị đúng giai đoạn.
- Theo dõi tiến trình qua log setup tại
  `$WINDOWS.~BT\Sources\Panther\setupact.log` (setup tự ghi % theo giai
  đoạn); wizard chỉ đọc và hiển thị, không can thiệp.
- Kết quả: Windows mới giữ nguyên ứng dụng, dữ liệu, tài khoản; bản cũ
  nằm trong `Windows.old` (tự xoá sau 10 ngày, rollback được trong
  thời gian đó — đây là lưới an toàn của chế độ này).
- **[VERIFY]** bản gốc có thể dùng chính setup.exe hoặc một engine riêng
  (WinNTSetup kiểu tuỳ biến); tham số chính xác chờ dump đối chiếu. Kết
  quả đầu ra của hai cách là tương đương với người dùng cuối.

## 6. Giai đoạn 3B — Cài mới từ xa, không cần USB (điểm "từ xa" cốt lõi)

Dành cho máy không có người tại chỗ. Thay cặp module gốc "Tạo Boot
WinPE + Cài Win=WinPE" bằng kỹ thuật staging:

1. **Chuẩn bị WinPE:** dùng WinPE dựng sẵn của người dùng (file WIM) hoặc
   tách từ chính ISO; copy vào thư mục staging trên ổ Data
   (`<Data>:\QSU-Stage\`).
2. **Chuẩn bị bộ nguồn cài:** giải nén `sources\` của ISO vào staging,
   kèm file `autounattend.xml` sinh ở §7 và thư mục driver IRST/NVMe.
3. **Đăng ký boot một lần vào WinPE** bằng bcdedit: tạo ramdisk + loader
   entry trỏ tới WIM staging, đặt làm boot sequence một lần
   (`bcdedit /bootsequence {id}`). Lỗi ở bước này = máy vẫn boot Windows
   cũ bình thường (an toàn).
4. **Reboot.** Trong WinPE, script tự chạy (winpeshl.ini / startnet.cmd):
   - Xác định ổ OS bằng marker và nhãn ổ — **không dựa vào ký tự ổ đĩa**
     (trong WinPE ký tự thay đổi); ổ Data có marker `.qsu_data_drive`
     bị loại khỏi mọi thao tác format.
   - Format phân vùng OS (giữ nguyên cấu trúc phân vùng EFI/MSR/
     Recovery hiện có; chỉ format phân vùng Windows).
   - `dism /Apply-Image /ImageFile:install.wim /Index:<đã chọn> /ApplyDir:<OS>:\`
   - Inject driver lưu trữ offline:
     `dism /Image:<OS>:\ /Add-Driver /Driver:<staging>\Drivers /Recurse`
     (đây là khâu làm SSD NVMe/IRST hiện hình — tương ứng module "Kho
     Driver IRST" của bộ gốc).
   - Copy `autounattend.xml` vào `<OS>:\Windows\Panther\` và agent hậu
     cài vào `<OS>:\ProgramData\QSU\`.
   - `bcdboot <OS>:\Windows /s <EFI>: /f UEFI` (hoặc ALL theo Boot Mode).
   - Ghi log kết quả vào ổ Data, reboot.
5. Máy boot vào Windows mới → OOBE chạy hoàn toàn bằng unattend → tới
   desktop → agent hậu cài chạy.

Đường USB vẫn hỗ trợ như bản gốc: nếu kỹ thuật viên có mặt, module Tạo
Boot WinPE ghi cùng bộ staging này ra USB và WinPE trên USB dùng chung
script cài ở trên (một engine, hai đường vào).

## 6b. Chế độ "Cài mới hoàn toàn" — wipeDisk: xoá toàn bộ ổ đĩa, chia lại từ đầu (v0.7)

Bổ sung cho §6 theo yêu cầu của người dùng (2026-11-17): đường cài mới
ở §6 **giữ nguyên cấu trúc phân vùng** (chỉ format phân vùng Windows).
Chế độ mới `wipeDisk` coi đích là **toàn bộ một ổ đĩa vật lý**: xoá sạch
mọi phân vùng trên đĩa đó rồi chia lại theo chuẩn, dành cho máy cần làm
mới hoàn toàn. Đây là tuỳ chọn — chế độ giữ phân vùng (`preserve`) vẫn
là **mặc định**.

### Hợp đồng stage.json (mở rộng, tương thích ngược)

| Khoá | Trị | Ý nghĩa |
|---|---|---|
| `partitionMode` | `"preserve"` \| `"wipeDisk"` | Thiếu khoá → engine hiểu là `"preserve"` (stage cũ vẫn chạy nguyên vẹn) |
| `targetDiskSerial` | serial phần cứng của đĩa đích | Chỉ có ý nghĩa khi `partitionMode = "wipeDisk"`; wizard KHÔNG strip thêm khoá nào khác — cam kết "stage.json không chứa mật khẩu/apps" giữ nguyên |

### Bố cục phân vùng sau khi chia lại

UEFI/GPT: **EFI 260MB FAT32** (nhãn `System`) → **MSR 16MB** →
**Windows NTFS** (phần còn lại trừ 600MB) → **Recovery 600MB NTFS**
(nhãn `Recovery`, `set id="de94bba4-06d1-4d40-a16a-bfd50179d6ac"` +
`gpt attributes=0x8000000000000001`).

BIOS/MBR: **System Reserved 100MB NTFS active** → **Windows NTFS**
(còn lại trừ 600MB) → **Recovery 600MB NTFS** (`set id=27`).

Sau khi apply image: copy `Windows\System32\Recovery\Winre.wim` (và
`ReAgent.xml` nếu có) từ image vừa giải vào
`<Recovery>:\Recovery\WindowsRE\`, rồi chạy `reagentc /setreimage`
trỏ vào đó (best-effort, lỗi chỉ ghi log). `bcdboot` trỏ vào **phân
vùng boot MỚI** (`/s <EFI mới>:` với UEFI, `/s <System Reserved mới>:`
với BIOS) — không dùng phân vùng EFI cũ. Mọi bước còn lại (apply
image, inject driver offline, autounattend + AutoLogon, chép thư mục
`custom`, log) giữ nguyên trình tự §6.

Thực thi bằng **một script diskpart sinh tự động** (ghi nguyên văn vào
log trước khi chạy), dùng chung cho cả hai đường vào: (a) engine
`WinPEInstall.exe` khi stage.json mang `partitionMode = "wipeDisk"`,
(b) module tương tác `CaiDatWinWinPE.exe` khi người dùng chọn dòng
đích "Toàn bộ ổ đĩa". Mã nguồn chung nằm ở `qsu_common::disk`
(`execute_wipe`, `wipe_script`, `copy_winre`).

### Bốn chốt an toàn cứng (không có ngoại lệ)

1. **Đĩa chứa ổ staging/Data không bao giờ là đích wipe.** Wizard:
   đĩa chứa ổ Data đã chọn bị từ chối ngay khi bấm Bắt đầu. Module
   WinPE: đĩa chứa ổ của chính công cụ hoặc ổ chứa nguồn cài đặt bị
   đánh dấu BỊ CẤM trong danh sách — ô ghi đè "ổ bị loại trừ" **không**
   áp dụng cho đích đĩa. Engine kiểm tra lại tại thời điểm thực thi:
   số đĩa vật lý của ổ Data (qua `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS`)
   trùng đĩa đích → dừng (mã thoát 11); không xác định được số đĩa của
   ổ Data → cũng dừng (thà dừng oan còn hơn wipe nhầm).
2. **Bắt buộc gõ serial của đĩa đích.** Cả wizard (mục chọn đĩa ở chế
   độ Cài mới) lẫn module WinPE (hàng xác nhận serial dùng chung với
   đường format) đều yêu cầu nhập đúng serial phần cứng của đĩa đích;
   UI đồng thời liệt kê **mọi phân vùng trên đĩa đó** (ký tự ổ / nhãn /
   dung lượng / loại) kèm cảnh báo "toàn bộ dữ liệu trên các phân vùng
   này sẽ bị xoá". So khớp serial sau chuẩn hoá (bỏ ký tự không phải
   chữ/số, viết hoa) để dung sai cách ghi của nhà sản xuất.
3. **Nhận diện đĩa bằng serial, không bằng số thứ tự hay ký tự ổ.**
   Liệt kê đĩa vật lý qua IOCTL (`IOCTL_STORAGE_QUERY_PROPERTY` lấy
   serial + model, `IOCTL_DISK_GET_DRIVE_LAYOUT_EX` lấy bảng phân
   vùng); khớp serial phải ra **đúng một** đĩa — không thấy hoặc
   trùng nhiều đĩa → dừng (mã thoát 10).
4. **Đối chiếu lần cuối ngay trước diskpart.** `execute_wipe` truy
   vấn lại serial thật của đúng số đĩa sắp chạy script và so với
   serial kỳ vọng; lệch → hard stop, chưa đụng gì vào đĩa. Wizard
   cũng chặn sẵn: đĩa đích nhỏ hơn 25GB, serial không đọc được, hoặc
   serial gõ tay không khớp đều bị từ chối trước khi staging.

Ký tự ổ của các phân vùng mới được xác định bằng cách liệt kê lại sau
khi diskpart chạy xong (khớp theo nhãn `Windows`/`System`/`Recovery`
+ loại phân vùng trên đúng đĩa đích), không giả định trước.

## 7. File unattend và hậu cài

`autounattend.xml` sinh tự động từ lựa chọn trên wizard:

- Ngôn ngữ/bàn phím: vi-VN mặc định (đổi được), múi giờ SE Asia Standard Time.
- Bỏ qua toàn bộ màn hình OOBE: EULA, tạo tài khoản online, các trang
  privacy (đặt về tắt), nhập key (dùng key generic theo edition khi cần;
  kích hoạt dựa vào digital license/OEM key — **không có cơ chế kích
  hoạt trái phép**).
- Tài khoản quản trị đầu tiên: tên do kỹ thuật viên nhập trên wizard;
  mật khẩu **tuỳ chọn** (quyết định v0.7.2 — logic triển khai hàng
  loạt của người dùng): để trống cả hai ô = tài khoản **không mật
  khẩu**, người dùng cuối tự đặt mật khẩu sau khi cài. Khi không có
  mật khẩu, unattend **bỏ hẳn** phần tử `<Password>` ở cả
  `LocalAccount` lẫn `AutoLogon` (đúng cấu trúc bản sinh Schneegans;
  AutoLogon không mật khẩu là cấu hình Windows Setup hỗ trợ cho
  đăng nhập nội bộ — không sinh `<Value>` rỗng). Chỉ chặn khi hai ô
  lệch nhau (một ô trống/một ô có, hoặc nhập lại sai). Mật khẩu khi
  có vẫn **không ghi vào log**; log chỉ ghi dòng trạng thái "Tài
  khoản admin: không đặt mật khẩu (theo lựa chọn)" khi để trống.
- Tên máy: giữ tên cũ (đọc trước khi cài) hoặc sinh theo mẫu cấu hình.

Agent hậu cài (chạy một lần ở first logon, tự huỷ sau khi xong):

1. Cài driver còn thiếu từ bản backup ở giai đoạn chuẩn bị
   (`pnputil /add-driver *.inf /subdirs /install` + `/scan-devices`).
2. Áp profile của module Tuỳ Chỉnh Windows nếu kỹ thuật viên đã chọn.
3. Xoá thư mục staging (trừ log), gỡ boot entry tạm nếu còn.
4. Viết báo cáo hoàn tất (phiên bản đã cài, edition, thời gian, lỗi nếu
   có) cạnh file log trên ổ Data — kỹ thuật viên mở ra xem là biết máy
   đã xong, không cần gọi hỏi người dùng máy.

## 8. An toàn, log, huỷ bỏ

- Log toàn trình: `<Data>:\QSU-Logs\install-<yyyyMMdd-HHmm>.log`.
- Trước điểm reboot đầu tiên: huỷ bỏ bất cứ lúc nào cũng an toàn tuyệt
  đối (chưa có thay đổi không đảo ngược nào ngoài suspend BitLocker —
  tự bật lại khi huỷ).
- Sau điểm reboot vào setup/WinPE: không huỷ được; wizard cảnh báo rõ
  điều này trong hộp xác nhận cuối, kèm tóm tắt: ISO đã chọn, edition,
  chế độ cài, ổ OS sẽ bị tác động, ổ Data được bảo vệ.
- Chống format nhầm: thao tác format chỉ chấp nhận phân vùng (a) chứa
  Windows hiện tại (đối chiếu trước khi reboot) hoặc (b) đã được đánh
  dấu OS trong staging; ổ có marker Data bị chặn ở tầng code, không
  chỉ ở tầng giao diện.

## 9. Danh sách [VERIFY] — chờ memory dump để đối chiếu bản gốc

1. Engine cài đè của bản gốc: setup.exe (và tham số) hay engine riêng.
2. Nội dung autounattend.xml của bản gốc (nếu nó có sinh).
3. Cách bản gốc đưa driver IRST vào quá trình cài.
4. Cơ chế boot vào WinPE của cặp module gốc (BCD trực tiếp hay qua
   phân vùng Recovery).
5. Catalogue ISO của "Kho Windows" (địa chỉ + danh sách bản Win) —
   phần này bản mới thay bằng kho của người dùng bất kể kết quả đối
   chiếu, chỉ tham khảo định dạng.
6. Tham số launcher truyền cho module (nếu có).

## 10. Quyết định mặc định (người dùng có thể đổi)

- Kho ISO online: catalogue JSON trên hạ tầng của người dùng; URL cấu
  hình được. Mặc định wizard ưu tiên ISO local có sẵn trên ổ Data.
- Mount ISO bằng API Windows có sẵn thay cho ImDisk.
- Cài mới từ xa dùng staging WinPE trên ổ Data + boot một lần bằng BCD,
  không cần USB và không cần người tại chỗ.
- Profile hậu cài mặc định: chỉ khôi phục driver + báo cáo; áp tuỳ
  chỉnh Windows là tuỳ chọn tích riêng.


---

## Implementation notes v1.1 (2026-11-16) — bản Rust đã hiện thực

Đã hiện thực đầy đủ đặc tả v1 bằng Rust (workspace `rust/`): wizard
`cai_dat_win` → `CaiDatWinTuDong.exe` (GUI native-windows-gui, theme tối như
launcher), engine WinPE `winpe_install` → `WinPEInstall.exe`, thư viện dùng
chung `qsu_common`. Build release sạch (0 lỗi, 0 cảnh báo); cả ba exe là
PE32+ x86-64. Chưa chạy thử trên Windows thật — xem mục "Cần test" cuối phần này.

### Quyết định thiết kế

- **Mount ISO: dùng PowerShell `Mount-DiskImage` thay vì VirtDisk API.**
  VirtDisk (OpenVirtualDisk/AttachVirtualDisk) đòi hỏi chuỗi quyền truy cập +
  xử lý vòng đời handle phức tạp và dễ rò rỉ mount khi tiến trình chết;
  Mount-DiskImage là đường chính thức của Windows, tự dọn khi dismount và cho
  ngay ký tự ổ qua `Get-Volume`. Đánh đổi: phụ thuộc PowerShell 5.1 (có sẵn
  trên mọi Windows 10/11) và tốn ~1–2 giây mỗi lần mount — chấp nhận được.
  Wizard tái sử dụng mount cũ nếu cùng ISO, dismount khi đổi ISO hoặc khi đọc lỗi.
- **BitLocker: đọc qua `Get-BitLockerVolume` (enum `VolumeStatus`)** thay vì
  parse text của `manage-bde -status` — tên enum không phụ thuộc ngôn ngữ hiển
  thị nên chạy đúng cả trên Windows tiếng Việt. Suspend vẫn dùng
  `manage-bde -protectors -disable` như đặc tả.
- **DISM luôn gọi kèm `/English`** để việc parse `Key : Value` không vỡ trên
  máy Windows ngôn ngữ khác. Edition mặc định: khớp `EditionID` trước, dự
  phòng khớp token tên (Pro/Home/Enterprise). Build lấy từ trường `Version`
  dạng `10.0.xxxxx.y` (phần tử thứ 3).
- **WinPE boot index**: đọc từ trường `Boot Index` của DISM trên boot.wim,
  dự phòng index 2 (chuẩn của Microsoft) — tránh giả định cứng.
- **Kích hoạt WinPE**: tiêm đồng thời ba cơ chế vào ảnh WinPE đã mount —
  `winpeshl.ini` ([LaunchApps] → qsu_start.cmd), `startnet.cmd` (cùng nội
  dung, dự phòng), và bản sao `WinPEInstall.exe` trong System32 (đường dự
  phòng cuối). `qsu_start.cmd` tự quét các ký tự ổ C:–Z: tìm staging theo tên
  thư mục cấu hình, không phụ thuộc ký tự ổ trong WinPE.
- **stage.json rút gọn có chủ đích**: wizard ghi stage.json rồi **loại bỏ**
  các khoá `apps`, `ultraviewerPassword` và `adminPassword` khỏi file đã ghi.
  Mật khẩu tài khoản admin chỉ tồn tại trong `autounattend.xml` (nơi Windows
  Setup cần nó) và trong bộ nhớ tiến trình wizard — không vào log, không vào
  stage.json. Đây là khác biệt có chủ đích so với struct `StageConfig` trong
  thư viện (struct vẫn giữ các trường để tương thích, wizard không điền).
- **BCD**: phân tích GUID từ output của `bcdedit` bằng mẫu hex
  `{8-4-4-4-12}` — không phụ thuộc nhãn ngôn ngữ ("device"/"path" dịch khác
  nhau theo locale). `/bootsequence` đảm bảo chỉ boot WinPE đúng một lần.
- **In-place**: sau khi chạy setup.exe, wizard tail `setupact.log` của
  Panther 2 giây/lần và hiển thị mọi dòng chứa ký tự `%` — kỹ thuật viên thấy
  tiến độ thật thay vì màn hình đứng yên.

### Loại trừ có chủ đích: agent hậu cài (post-install)

**Không xây dựng** thành phần agent chạy ở lần đăng nhập đầu tiên để cài ứng
dụng âm thầm / cấu hình truy cập từ xa. Lý do: cài đặt unattended + tự động
đăng nhập bằng mật khẩu lưu sẵn là bề mặt rủi ro cao và nằm ngoài nhu cầu đã
xác nhận của bản v1. Hệ quả trong mã nguồn:

- Thư mục `rust/post_install/` chỉ còn `README.md` giải thích quyết định này;
  crate đã bị loại khỏi workspace.
- `autounattend.xml` do wizard sinh **không chứa** lệnh cài ứng dụng nào
  (FirstLogonCommands để trống). AutoLogon cho tài khoản admin do kỹ thuật
  viên nhập vẫn giữ — đó là một phần của luồng cài đặt §7, mật khẩu lấy từ ô
  nhập của wizard, không ghi log.
- Thay thế bằng **thư mục `custom`**: nếu cạnh file exe của wizard có thư mục
  tên `custom`, toàn bộ nội dung của nó được sao **nguyên trạng** vào thư mục
  staging (đường clean) hoặc thẳng vào `<OS>:\ProgramData\QSU\custom` (đường
  in-place); engine WinPE cũng sao tiếp `staging\custom` vào ảnh mới tại
  `ProgramData\QSU\custom`. Wizard/engine **không diễn giải, không thực thi**
  nội dung này — kỹ thuật viên tự đặt script/công cụ của mình vào đó và tự
  chịu trách nhiệm chạy chúng sau khi Windows mới lên.
- `WinPEInstall.exe` vẫn hỗ trợ móc nối tuỳ chọn: nếu staging có sẵn
  `PostInstall.exe` do người dùng tự cung cấp, engine sao nó vào
  `ProgramData\QSU` và đăng ký RunOnce như trước; nếu không có, engine bỏ qua
  và ghi log rõ ràng "không có agent hậu cài".

### Tối ưu đã áp dụng

- **Quét ISO song song**: mỗi ổ đĩa một luồng riêng; kết quả hợp nhất về UI
  qua cơ chế Notice (không khoá giao diện).
- **Mọi thao tác dài đều ở luồng nền** (preflight, quét ISO, tải catalogue,
  tải ISO, đọc edition, thực thi) — UI chỉ nhận kết quả qua `Shared` +
  `nwg::Notice`.
- **Tải ISO**: WinHTTP có resume bằng Range (server trả 200 thay vì 206 thì
  tự tải lại từ đầu); **kiểm SHA-256 streaming** theo khối 1MB (không nạp cả
  file ISO vài GB vào RAM); bản ghi catalogue thiếu SHA-256 bị **từ chối tải**
  (không có ngoại lệ).
- **Sao chép staging theo byte** với callback tiến độ thật (không phải ước
  lượng), DISM mount/apply đều stream log ra giao diện.
- **SHA-256 thuần Rust** (FIPS 180-4, có test vector chuẩn) — không phụ thuộc
  thêm crate ngoài; dùng chung cho cả kiểm ISO tải về.

### Cần test trên Windows thật (chưa kiểm chứng được ở môi trường build)

1. Toàn bộ luồng GUI wizard (chỉ mới biên dịch, chưa chạy): owner-draw dark
   theme, các toggle chế độ, hộp thoại chọn file.
2. Mount-DiskImage + DISM /Get-WimInfo trên ISO thật (cả install.wim/.esd/.swm).
3. Đường in-place trọn vẹn: suspend BitLocker, backup driver, setup.exe chạy
   nâng cấp, tail Panther.
4. Đường clean trọn vẹn: BCD ramdisk boot vào WinPE, WinPEInstall format đúng
   phân vùng theo serial, apply image, bcdboot UEFI + Legacy, reboot về
   Windows mới và unattend được Setup tiêu thụ (locale vi-VN, tài khoản,
   AutoLogon).
5. Test vector SHA-256/JSON của `qsu_common` (test unit biên dịch cho target
   Windows nên chưa chạy được trên máy build Linux).

### Implementation notes v0.7 (2026-11-17) — chế độ wipeDisk (§6b) đã hiện thực

- **`qsu_common::disk`** (mới): liệt kê đĩa vật lý + bảng phân vùng thuần
  IOCTL (serial/model qua `IOCTL_STORAGE_QUERY_PROPERTY`, layout qua
  `IOCTL_DISK_GET_DRIVE_LAYOUT_EX`, đối chiếu ký tự ổ/nhãn/serial-volume
  qua FindFirstVolume), `find_disk_by_serial` (khớp sau chuẩn hoá, yêu
  cầu duy nhất), `wipe_layout`/`wipe_script` sinh script diskpart theo
  đúng bố cục §6b, `execute_wipe` (đối chiếu serial lần cuối → chạy
  diskpart → liệt kê lại lấy ký tự ổ mới theo nhãn+loại), `copy_winre`.
- **Wizard**: khối chọn chế độ phân vùng + chọn đĩa vật lý chỉ hiện ở
  chế độ Cài mới; chặn cứng đĩa chứa ổ Data, đĩa < 25GB, serial gõ tay
  không khớp; hộp xác nhận cuối ghi rõ chế độ + liệt kê mọi phân vùng
  sẽ bị xoá; stage.json ghi `partitionMode`/`targetDiskSerial`.
- **Engine `WinPEInstall.exe`**: đọc hai khoá mới (thiếu → `preserve`);
  mã thoát mới 10 (serial đích không có/trùng), 11 (đĩa đích chứa ổ
  Data hoặc không xác định được đĩa của ổ Data), 12 (diskpart wipe
  lỗi). Đường `preserve` giữ nguyên hành vi cũ, kể cả chốt serial-only
  của bản audit v0.4.
- **Module `CaiDatWinWinPE.exe`**: danh sách đích có thêm dòng "Toàn bộ
  ổ đĩa" cho từng đĩa vật lý; đĩa chứa ổ công cụ/nguồn bị cấm tuyệt
  đối (không qua ô ghi đè); xác nhận bằng serial của ĐĨA (dùng chung
  hàng xác nhận với đường format); tầng thực thi liệt kê lại theo
  serial và kiểm số đĩa của các ổ cấm trước khi chạy diskpart.

### Implementation notes v0.7.2 (2026-11-17) — mật khẩu tuỳ chọn + log quét bền

- **Mật khẩu admin thành TUỲ CHỌN** ở cả wizard và module WinPE
  (theo logic triển khai hàng loạt của người dùng: cài nhiều máy,
  tài khoản admin tạo ra được phép không mật khẩu, người dùng cuối
  tự đặt sau):
  - Validation: chặn **chỉ khi** hai ô mật khẩu lệch nhau (một ô
    trống/một ô có, hoặc hai ô khác nhau) — thông báo mới nói rõ
    "để trống cả hai ô nếu không muốn đặt mật khẩu". Cả hai ô trống
    = hợp lệ; cả hai ô giống nhau = như cũ.
  - `build_unattend` (cả hai crate): mật khẩu trống →
    `LocalAccount` sinh **không có** phần tử `<Password>` và
    `AutoLogon` sinh **không có** phần tử `<Password>` (không sinh
    `<Value>` rỗng) — đối chiếu cấu trúc bản sinh Schneegans:
    AutoLogon không mật khẩu là cấu hình được Setup hỗ trợ. Có mật
    khẩu → khối `<Password><Value>…<PlainText>true` như cũ.
  - Hộp xác nhận cuối của wizard (cả hai biến thể tóm tắt) và hộp
    xác nhận của module WinPE đều thêm dòng trạng thái:
    "Tài khoản \<tên\>: KHÔNG mật khẩu — người dùng tự đặt mật khẩu
    sau khi cài" (hoặc "có đặt mật khẩu (không hiển thị)"). Module
    WinPE bỏ hộp hỏi riêng "mật khẩu đang để TRỐNG… Tiếp tục?" —
    trạng thái nay nằm ở hộp xác nhận cuối như wizard.
  - Log cài đặt (cả hai luồng) ghi "Tài khoản admin: không đặt mật
    khẩu (theo lựa chọn)" khi để trống; giá trị mật khẩu vẫn không
    bao giờ vào log. `stage.json` không đổi: trường mật khẩu vẫn bị
    strip như cũ — không mật khẩu thì càng không có gì để strip.
  - Nhãn ô mật khẩu ở cả hai giao diện đổi thành "Mật khẩu (để
    trống = không đặt mật khẩu):" (bố cục hàng nhập được nới lại:
    ô nhập hẹp hơn, ghi chú rút gọn "(Chỉ cài mới. Không ghi vào
    log.)" / "(Không ghi vào log.)").
  - Module WinPE: `InstallPlan` thêm `admin_user` +
    `admin_passwordless` (chỉ trạng thái, không mang mật khẩu).
- **Log quét bền (`wizard-scan-*.log`)**: chẩn đoán [Đĩa] của
  v0.7.1 trước đây chỉ tới file log khi bắt đầu cài — mà ca liệt kê
  đĩa lỗi lại không bắt đầu cài được nên bằng chứng bị kẹt. Nay
  `persist_scan_log()` chạy ở cuối **mỗi** lần quét tiền đề (kể cả
  Quét lại): nội dung gồm tóm tắt tiền đề (OS/boot/BitLocker/nguồn
  điện/kết luận), danh sách ổ cố định + marker, mọi đĩa vật lý
  (summary + phân vùng) và **toàn bộ** dòng chẩn đoán [Đĩa] của cả
  hai đường liệt kê. Ghi ra `<Data>:\<logsFolder>\wizard-scan-<yyyyMMdd-HHmm>.log`
  (tạo thư mục nếu chưa có); ổ Data chưa chọn được hoặc ghi lỗi →
  ghi cạnh file exe wizard (`wizard-scan-<...>.log`). Đường dẫn
  file được đẩy vào log panel ngay khi ghi xong (ca 0 đĩa có câu
  chữ riêng, yêu cầu gửi file đó để chẩn đoán).
