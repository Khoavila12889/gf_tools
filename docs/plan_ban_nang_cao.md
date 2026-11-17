# PLAN — Bản nâng cao (kiến trúc phân vùng boot) cho module Cài Đặt Win

Tài liệu kế hoạch thực thi đầy đủ cho bản nâng cao của wizard cài Windows
trong UtilityRebuild, viết ngày 2026-11-17. Đây là **đặc tả để thực thi
sau** (bản build theo kiến trúc này hiện chưa thực hiện được); mọi thông
số dưới đây là giá trị chốt để cứ theo đó mà làm, không phải đoán lại.

Nền tảng: cơ chế đã chứng minh thực địa của tool gốc (đọc
`readme_co_che_tool_goc.md`) + các cơ chế an toàn của bản rebuild hiện
tại v0.7.11/v0.7.12 (đọc `co_che_v0.7.12.md` và `logic_cai_dat_win_online.md`).

---

## 1. Mục tiêu và nguyên tắc

**Mục tiêu:** thay cơ chế staging "thư mục trên ổ Data + ramdisk BCD"
bằng cơ chế "phân vùng boot riêng trên ổ hệ thống + WIM đóng gói lại",
để khâu boot vào WinPE đạt độ bền như tool gốc, đồng thời giữ toàn bộ
tính an toàn của bản rebuild.

**Nguyên tắc bất biến (kế thừa v0.7.11):**

1. Nhận diện ổ đích theo **serial** (volume serial cho phân vùng,
   serial đĩa vật lý cho wipe) — tuyệt đối không theo nhãn ổ, không
   theo ký tự ổ. Kiểm tra lại serial ngay trước thao tác phá huỷ;
   lệch serial = dừng cứng, không đụng gì.
2. Mục boot đăng ký dạng **dùng một lần** (`/bootsequence`) — WinPE
   không lên được thì lần boot kế tự về Windows cũ. Không bao giờ tạo
   entry mặc định cố định (bài học vòng lặp boot của tool gốc).
3. Đĩa chứa ổ Data/staging **không bao giờ** là đích xoá (engine kiểm
   tra lại độc lập, exit 11).
4. Mật khẩu tài khoản admin **tuỳ chọn** (để trống được — triển khai
   hàng loạt); mật khẩu không bao giờ vào log/stage.json.
5. Không license check, không telemetry, không Defender exclusion,
   không root CA, không xử lý credential UltraViewer. Thư mục `custom`
   chép nguyên trạng, không thực thi.

---

## 2. Kiến trúc tổng thể

```
[Windows đang chạy — Wizard]
  Phase 0  Tiền kiểm (firmware, BitLocker, nguồn điện, dung lượng)
  Phase 1  Sao lưu driver của máy
  Phase 2  Tạo phân vùng boot QSUBOOT (shrink ổ OS)
  Phase 3  Chép file boot từ ISO + chuẩn bị WinPE (extract/inject/export)
  Phase 4  Chép bộ nguồn cài sang ổ Data (QSU-Stage) + stage.json
  Phase 5  Ghi unattend + đăng ký BCD (boot một lần) + tổng kết
  → Reboot
[WinPE — Engine WinPEInstall]
  Phase 6  Định vị staging (marker + stage.json), đối chiếu serial
  Phase 7  Xử lý đĩa đích (preserve hoặc wipeDisk)
  Phase 8  Bung ảnh (DISM /Apply-Image) + inject driver offline
  Phase 9  Gắn unattend + bcdboot + đặt WinRE
  Phase 10 Dọn phân vùng QSUBOOT (preserve) + reboot vào Windows mới
```

Điểm rơi an toàn: mọi lỗi ở Phase 0–5 → huỷ staging, máy nguyên trạng.
Lỗi ở Phase 6 (trước format) → engine dừng, boot một lần đã tiêu thụ,
máy về Windows cũ. Chỉ từ Phase 7 (đã format/wipe) mới là điểm không
quay lại — và tới đó mọi đối chiếu serial đã xong.

---

## 3. Thông số cố định (constants)

| Thông số | Giá trị | Ghi chú |
|---|---|---|
| Dung lượng phân vùng boot | **3584 MB** (3.5 GB) | Đủ cho boot.wim (~700 MB) + file boot + $WinPEDriver$; tool gốc dùng 1500 MB nhưng không có dự phòng driver |
| Nhãn phân vùng boot | `QSUBOOT` | Engine dọn dẹp tìm theo nhãn này (chỉ để DỌN, không để nhận diện đích) |
| Định dạng phân vùng boot | **FAT32** | Đọc được bởi cả UEFI và Legacy boot files của ISO |
| Ký tự ổ ưu tiên | `W:` | Bận thì lấy ký tự trống bất kỳ và ghi vào stage.json; không giả định cứng |
| Điều kiện shrink | querymax ≥ 3584 MB | Không đạt → fallback (mục 7) |
| Hibernate | Tắt trước khi shrink (`powercfg /h off`) | Tránh hiberfil.sys chặn shrink; bật lại hay không do Windows mới tự quản |
| Thư mục nguồn trên ổ Data | `<Data>:\QSU-Stage` | Giữ nguyên như v0.7.11 |
| Marker ổ Data | `<Data>:\.qsu_data_drive` | Giữ nguyên |
| Thư mục log | `<Data>:\QSU-Logs\install-<yyyyMMdd-HHmm>.log` | Giữ nguyên |
| Timeout chờ DISM mount | Không đặt cứng; theo tiến trình DISM | Log % tiến trình như hiện tại |

---

## 4. Phase 2–3: Phân vùng boot và WinPE

### 4.1 Tạo QSUBOOT (diskpart, script được log nguyên văn)

```
select volume <OS>
shrink desired=3584 minimum=3584
create partition primary size=3584
format fs=fat32 quick label="QSUBOOT"
assign letter=W        (nếu W bận: assign không tham số, đọc lại ký tự thật)
```

Kiểm chứng sau tạo: liệt kê lại volume, xác nhận có volume nhãn QSUBOOT
đúng dung lượng ±50 MB và có ký tự. Sai lệch → coi là lỗi Phase 2 →
fallback.

### 4.2 Cây file trên QSUBOOT

```
<PB>:\
  bootmgr
  bootmgr.efi                 (nếu ISO có)
  boot\
    bcd  (của ISO — chỉ để tham chiếu, BCD hệ thống mới là nơi đăng ký)
    boot.sdi                    ← xem thứ tự ưu tiên dưới
    ... (toàn bộ cây \boot của ISO)
  efi\                          (toàn bộ cây \efi của ISO)
  boot.wim                      ← WinPE đã đóng gói lại (mục 4.3)
  $WinPEDriver$\                ← driver staging cho Windows mới (mục 4.4)
```

Thứ tự ưu tiên boot.sdi (ghi log từng vị trí đã thử):
1. `<ISO>:\boot\boot.sdi`
2. `<ISO>:\sources\boot.sdi`
3. Chuỗi dự phòng hiện tại của v0.7.5 (theo firmware: UEFI thử
   `%WINDIR%\Boot\DVD\EFI\boot.sdi` trước PCAT; Legacy ngược lại;
   cuối cùng `<OS>:\Recovery\WindowsRE\boot.sdi`).

### 4.3 WinPE đóng gói lại (điểm cốt lõi của bản nâng cao)

1. Chọn ảnh PE bằng probe hiện tại: **Boot Index trong header WIM
   trước; không có thì theo tên** ("Windows PE", không lấy "Setup").
2. Mount ảnh đã chọn vào thư mục mount tạm (xoá cờ read-only trên cây
   sources trước khi mount — bài học lỗi 0xC1510111 của v0.7.6).
3. Inject như hiện tại: `WinPEInstall.exe`, startnet.cmd/winpeshl.ini
   trỏ vào engine, driver đã sao lưu ở Phase 1 (đặc biệt driver storage).
4. Unmount `/Commit`.
5. **Export một ảnh duy nhất**: 
   `DISM /Export-Image /SourceImageFile:<staging>\winpe.wim /SourceIndex:<idx đã chọn> /DestinationImageFile:<PB>:\boot.wim /Bootable /Compress:Max`
   → boot.wim trên QSUBOOT chỉ có đúng 1 ảnh và ảnh đó là ảnh boot.
   **Không có bước kiểm tra/xác minh cờ bootable nào** (chỉ đạo của
   người dùng sau sự cố cổng kiểm tra v0.7.7–v0.7.11).

### 4.4 Driver cho Windows mới

- Chép toàn bộ driver đã sao lưu (Phase 1) + gói driver storage bổ
  sung (nếu người dùng đặt cạnh wizard) vào `<PB>:\$WinPEDriver$\`
  theo cấu trúc thư mục con từng gói.
- Engine ở Phase 8 vẫn inject offline bằng DISM `/Add-Driver /Recurse`
  từ staging trên ổ Data (nguồn chính); `$WinPEDriver$` là lớp dự
  phòng theo quy ước của Windows Setup.

---

## 5. Phase 5: BCD (thông số đầy đủ)

Hai đối tượng, đăng ký vào **BCD hệ thống**, theo đúng công thức
ramdisk đang dùng ở v0.7.11 — chỉ đổi vị trí trỏ từ ổ Data sang QSUBOOT:

**Ramdisk options** `{<guid-device>}`:
```
device                  ramdisksdidevice=partition=<PB>:
                        ramdisksdipath=\boot\boot.sdi
```

**OS loader** `{<guid-loader>}`:
```
device                  ramdisk=[<PB>:]\boot.wim,{<guid-device>}
osdevice                ramdisk=[<PB>:]\boot.wim,{<guid-device>}
path                    UEFI:   \windows\system32\boot\winload.efi
                        Legacy: \windows\system32\winload.exe
systemroot              \windows
winpe                   yes
detecthal               yes
nx                      OptIn
```

**Boot manager**: `bcdedit /bootsequence {<guid-loader>}` (một lần),
`displayorder` và `default` **giữ nguyên** của Windows cũ. Sau đăng ký:
dump `bcdedit /enum all` vào log + đối chiếu từng phần tử (log-only,
không chặn).

Firmware xác định bằng probe hai API hiện tại (`GetFirmwareType` +
`GetFirmwareEnvironmentVariableW`), log giá trị thô.

---

## 6. stage.json — schema mở rộng

Giữ nguyên mọi trường hiện tại, thêm:

| Trường | Kiểu | Ý nghĩa |
|---|---|---|
| `bootMode` | `"partition"` \| `"dataRamdisk"` | Cơ chế staging đã dùng (partition = bản nâng cao; dataRamdisk = fallback v0.7.11) |
| `bootPartitionLetter` | string \| null | Ký tự QSUBOOT lúc staging (chỉ thông tin; engine tìm lại theo nhãn khi dọn) |
| `bootPartitionSizeMB` | number | 3584 |
| `winpeSourceIndex` | number | Index ảnh PE đã chọn trong boot.wim gốc |
| `winpeImageName` | string | Tên ảnh PE (log/chẩn đoán) |

Engine đọc được cả stage.json cũ (không có các trường này → hiểu là
`dataRamdisk`, hành vi như v0.7.11).

---

## 7. Fallback về cơ chế v0.7.11

Kích hoạt khi, **trước thời điểm đăng ký BCD**, bất kỳ bước nào của
Phase 2–3 thất bại: shrink không đủ chỗ, tạo/format phân vùng lỗi,
chép file boot lỗi, export WinPE lỗi.

Hành vi: log lý do + dòng `[Cơ chế]`, dọn phần QSUBOOT đã tạo dở (xoá
phân vùng, trả dung lượng — việc trả dung lượng bằng extend ổ OS là
best-effort, log-only nếu lỗi), rồi chạy nguyên quy trình staging
v0.7.11 (QSU-Stage + ramdisk từ ổ Data). `bootMode` trong stage.json
ghi `dataRamdisk`. Người dùng không phải làm gì thêm.

---

## 8. Phase 6–10: Engine trong WinPE (thay đổi so với hiện tại)

Giữ nguyên toàn bộ engine hiện tại (định vị staging theo marker +
stage.json, đối chiếu serial, exit codes: 10 = không thấy/không duy
nhất đĩa theo serial, 11 = đĩa đích trùng đĩa Data, 12 = lỗi wipe).
Thay đổi duy nhất:

**Phase 10 — dọn QSUBOOT (chỉ khi `bootMode="partition"`):**
- Preserve mode: sau bcdboot thành công, tìm volume nhãn `QSUBOOT`
  trên đĩa đích, xoá phân vùng, `extend` phân vùng Windows mới vào
  khoảng trống (nó nằm ngay sau phân vùng Windows theo cấu trúc
  shrink). Lỗi bất kỳ bước nào → chỉ log cảnh báo, không fail ca cài.
- Wipe mode: đĩa đích đã được tạo lại từ đầu — QSUBOOT không còn;
  engine kiểm tra sự vắng mặt và bỏ qua lặng lẽ.
- Không dọn khi `bootMode="dataRamdisk"`.

**Layout đĩa khi wipe (giữ nguyên v0.7, nhắc lại để đủ bộ thông số):**
- GPT: EFI 260 MB FAT32 → MSR 16 MB → Windows NTFS (phần còn lại −
  600 MB) → Recovery 600 MB NTFS (`set id="de94bba4-06d1-4d40-a16a-bfd50179d6ac"`,
  `gpt attributes=0x8000000000000001`); chép Winre.wim + ReAgent.xml
  vào `<R>:\Recovery\WindowsRE`, `reagentc /setreimage` best-effort.
- MBR: System Reserved 100 MB (active) → Windows → Recovery 600 MB
  (`set id=27`).
- bcdboot với `/f UEFI` hoặc `/f BIOS` tường minh theo firmware.

---

## 9. Ma trận kiểm thử nghiệm thu

| # | Kịch bản | Kết quả mong đợi |
|---|---|---|
| 1 | UEFI + 2 đĩa + preserve + ISO stock | Cài trọn vẹn; QSUBOOT được dọn; Windows mới nhận đủ driver |
| 2 | UEFI + 2 đĩa + wipe + ISO stock | Đĩa đích tạo lại đúng layout; đĩa Data nguyên vẹn |
| 3 | UEFI + ISO mod (header mất Boot Index) | Probe chọn theo tên; export 1 ảnh; PE boot lên (ca đã gãy ở v0.7.6–v0.7.9) |
| 4 | Legacy/MBR + preserve | winload.exe trong BCD; layout MBR đúng |
| 5 | 1 đĩa duy nhất + wipe | Wizard cấm từ UI; engine exit 11 nếu ép bằng stage.json tay |
| 6 | Shrink không đủ 3584 MB | Fallback dataRamdisk, cài vẫn chạy, log rõ lý do |
| 7 | Ngắt điện giữa Phase 3 | Boot lại vào Windows cũ bình thường (chưa đăng ký BCD) |
| 8 | WinPE không boot được (ISO lỗi) | Boot kế tiếp về Windows cũ — **không lặp boot** |
| 9 | Sai serial ở UI | Không cho bắt đầu |
| 10 | stage.json cũ (v0.7.11) + engine mới | Chạy như dataRamdisk, không lỗi |
| 11 | Máy Intel VMD/RAID | PE thấy ổ nhờ driver inject; nếu không thấy ổ đích → dừng an toàn, không format nhầm |
| 12 | Tài khoản không mật khẩu | Windows mới tạo đúng tài khoản trống mật khẩu theo unattend |

Tiêu chí đạt: 12/12 trên máy thật (không tính qua mô phỏng), mỗi ca
có log `install-*.log` đầy đủ kèm các dòng `[Cơ chế]`.

---

## 10. Rủi ro và giảm thiểu

| Rủi ro | Giảm thiểu |
|---|---|
| Shrink làm hỏng phân vùng OS (hiếm, mất điện giữa chừng) | Shrink là thao tác NTFS hỗ trợ chính thức; yêu cầu cắm sạc ở Phase 0; log script diskpart nguyên văn |
| Hai phân vùng EFI (máy từng cài nhiều Windows) | BCD đăng ký vào BCD **hệ thống** qua bcdedit mặc định (đúng ESP đang boot), không tự chọn ESP |
| QSUBOOT còn sót sau cài lỗi giữa chừng trong PE | Nhãn QSUBOOT dễ nhận diện để dọn tay; wizard lần chạy sau phát hiện QSUBOOT cũ và đề xuất dọn trước |
| ISO mod thiếu \boot hoặc \efi | Fallback boot.sdi + nếu thiếu cả cây boot → fallback cơ chế v0.7.11 (mục 7) |
| Extend trả dung lượng thất bại | Chỉ là 3.5 GB nằm lại dạng phân vùng trống; log rõ, dọn tay được bằng Disk Management |

---

## 11. Trạng thái

- v0.7.12 (cơ chế v0.7.11 + tab Hướng dẫn/Tài liệu + log [Cơ chế]):
  bản đang dùng để test.
- Bản nâng cao theo plan này: **chưa build** — chờ thực thi khi được
  yêu cầu lại. Mọi thông số đã chốt ở mục 3–8; khi làm, đối chiếu
  nghiệm thu theo mục 9.
