# Kiểm tra đường boot UEFI — Utility Rebuild v0.7.8

Câu hỏi: đem tool sang một PC UEFI thật, liệu có chạy ổn không?
Phạm vi: đọc CODE (không chỉ docs) của `cai_dat_win` (wizard),
`winpe_install` (engine), `cai_dat_win_winpe` (module tương tác),
`qsu_common`. Chỉ audit — không sửa code trong lượt này.

## 1. Dò firmware trong wizard — **OK**

- `probe_firmware()` tại `cai_dat_win/src/main.rs:421-455` dò bằng HAI API
  độc lập: `GetFirmwareType` (API có tài liệu từ Windows 8; 1=BIOS, 2=UEFI)
  là phán quyết chính — `Some(2) => UEFI`, `Some(1) => Legacy`. Chỉ khi
  GetFirmwareType không kết luận (Unknown/lỗi API) mới rơi về trick
  `GetFirmwareEnvironmentVariableW` (Legacy chỉ khi lỗi đúng
  ERROR_INVALID_FUNCTION=1). Giá trị thô của cả hai API được ghi vào log
  quét, log cài đặt và log tại register_bcd (`FirmwareProbe::detail()`,
  main.rs:456-475).
- Header wizard hiện "Boot Mode: UEFI (GPT|MBR)" từ `pf.uefi` + kiểu phân
  vùng đĩa 0 đọc bằng IOCTL (main.rs:3456-3466 và 3577-3587) — hai tín
  hiệu độc lập, không suy ra lẫn nhau.
- Tại `register_bcd` còn đối chiếu tín hiệu thứ ba: `bcdedit /enum
  {fwbootmgr}` (chỉ tồn tại trên UEFI) — log-only, lệch thì ghi CẢNH BÁO
  (main.rs:1631-1667).
- Cách một PC UEFI thật bị nhận nhầm Legacy: gần như không có trên phần
  cứng thật — GetFirmwareType dứt khoát trên Windows 8+. Máy UEFI nhưng
  boot qua CSM/Legacy thì bị báo Legacy — đó là hành vi ĐÚNG, vì chế độ
  boot thực tế mới quyết định winload/SDI. Đường dự phòng (cả hai API đều
  không kết luận) thiên về UEFI trừ khi lỗi đúng INVALID_FUNCTION — chỉ
  là lý thuyết trên phần cứng thật.

## 2. Chuỗi boot.sdi trên UEFI — **OK**

- `stage_boot_sdi()` (main.rs:1901-2009): thứ tự thử = ISO `sources\boot.sdi`
  → gốc ISO `\boot.sdi` → **theo firmware**: UEFI thì
  `%WINDIR%\Boot\DVD\EFI\boot.sdi` TRƯỚC, rồi mới `PCAT\boot.sdi`
  (main.rs:1911-1915); Legacy thì ngược lại → cuối cùng
  `<OS>:\Recovery\WindowsRE\boot.sdi`. Mọi vị trí thử đều ghi log kèm
  nhãn biến thể; không đâu có thì dừng với thông báo liệt kê đủ các vị trí.

## 3. Đăng ký BCD trên UEFI — **OK**

- `register_bcd()` (main.rs:1613-1703): loader `path` =
  `\windows\system32\boot\winload.efi` khi `plan.uefi`, ngược lại
  `\windows\system32\winload.exe` (main.rs:1683-1687). Device options:
  `ramdisksdidevice partition=<Data>:`, `ramdisksdipath
  \<staging>\boot.sdi`; loader device/osdevice =
  `ramdisk=[<Data>:]\<staging>\winpe.wim,{guid}`; `systemroot \windows`,
  `winpe yes`, `detecthal yes`, kích hoạt bằng `/bootsequence` (một lần,
  không đụng displayorder). Không có giả định phân vùng active/MBR nào;
  object `{bootmgr}` mà bcdedit trừu tượng hoá dùng chung cho cả hai
  firmware — không có lỗi bootmgr/bootmgfw ở tầng này.
- Wizard KHÔNG ghi gì vào ESP trong lúc staging: không mountvol, không
  gán ký tự ESP trong `cai_dat_win` (grep toàn crate — mountvol chỉ xuất
  hiện ở hai binary chạy trong WinPE). Mọi thay đổi boot đi qua bcdedit,
  tức qua BCD store hệ thống do Windows quản lý.
- Sau đăng ký có `verify_bcd()` dump `/enum all` + đối chiếu từng phần
  tử vào log (chẩn đoán, không chặn) — main.rs:1713-1880.

## 4. Engine winpe_install trong WinPE boot UEFI — **OK, 1 RISK thấp**

- Engine KHÔNG tự dò firmware: nó tin `stage.json` —
  `let uefi = cfg.boot_mode.eq_ignore_ascii_case("UEFI")`
  (winpe_install/src/main.rs:208). Trường `bootMode` do wizard ghi từ
  `plan.uefi` (cai_dat_win main.rs:2645). Hệ quả: phán quyết sai của
  wizard sẽ truyền thẳng xuống layout + bcdboot; bù lại wizard dò bằng
  hai API + đối chiếu {fwbootmgr}. Mặc định khi stage.json thiếu trường
  này là "UEFI" (qsu_common/src/stage.rs:110) — wizard luôn ghi trường
  này nên chỉ file stage chép tay mới dính.
- wipeDisk GPT: `disk::wipe_script(..., uefi=true)` sinh đúng chuẩn —
  `convert gpt` → EFI 260MB FAT32 nhãn "System" (assign S) → MSR 16MB →
  Windows NTFS (assign W) → Recovery 600MB NTFS (assign R) +
  `set id="de94bba4-06d1-4d40-a16a-bfd50179d6ac"` +
  `gpt attributes=0x8000000000000001` (qsu_common/src/disk.rs:1079-1097,
  1104-1130). `execute_wipe` đối chiếu lại serial ngay trước diskpart và
  xác minh phân vùng mới bằng liệt kê thật (kind theo GUID + nhãn +
  dung lượng), không tin ký tự tạm (disk.rs:1167-1275). WinRE:
  `copy_winre` chép Winre.wim + ReAgent.xml vào
  `<R>:\Recovery\WindowsRE` rồi `reagentc /setreimage /path ... /target
  <W>:\Windows` ở chế độ offline, best-effort (disk.rs:1281-1334).
- bcdboot: wipe → `bcdboot <W>:\Windows /s <boot_letter>: /f UEFI`
  trỏ vào ESP MỚI vừa chia (winpe_install main.rs, khối bcdboot);
  preserve → `/s <ESP> /f UEFI`. Cả hai chế độ đều có `/f` tường minh.
- **RISK (preserve trên UEFI):** `find_efi_letter()` (winpe_install
  main.rs:111-131) nhận diện ESP theo dấu hiệu đơn giản: phân vùng có ký tự nào chứa
  thư mục `\EFI` ở gốc là lấy; không có mới `mountvol <S|Z|R>: /s`. Nó
  không kiểm tra `\EFI\Microsoft\Boot\bootmgfw.efi` và không kiểm tra
  ESP đó thuộc đĩa đích. Trên máy nhiều đĩa/nhiều ESP hoặc ổ dữ liệu có
  thư mục EFI ở gốc, có thể chọn nhầm ESP — bcdboot vẫn "thành công"
  nhưng ghi vào ESP mà firmware không boot từ đó. Máy một đĩa (đa số
  máy thật) không dính.

## 5. Module tương tác cai_dat_win_winpe trên UEFI — **OK, cùng RISK**

- Firmware dò TRỰC TIẾP trong WinPE bằng `sys::firmware_is_uefi()` của
  qsu_common (cai_dat_win_winpe main.rs:1492; plan.uefi tại 2461-2476).
  Đây là trick MỘT API cũ (GetFirmwareEnvironmentVariableW —
  qsu_common/src/sys.rs:207-224), không phải bộ dò hai API của wizard.
  Trong WinPE boot bằng UEFI trick này trả đúng; vẫn là **RISK thấp**
  vì thiếu lớp đối chiếu và giá trị thô như wizard.
- Wipe dùng chung `disk::execute_wipe(..., plan.uefi)` (main.rs:1128).
  bcdboot: wipe → `/s <boot> /f UEFI|BIOS` theo plan (main.rs:1279-1280);
  preserve + UEFI → `find_efi_letter()` (bản sao cùng cách nhận diện như engine,
  main.rs:692-715) + `/f UEFI` (main.rs:1283-1287). Cùng RISK chọn ESP
  như mục 4.

## 6. Chọn ảnh WIM (v0.7.8) với ISO Microsoft GỐC — **OK, có một điểm cần biết**

- Thứ tự ưu tiên THẬT trong `probe_winpe_wim()` (main.rs:899-990):
  (1) "Boot Index" do DISM báo trong header WIM; (2) chọn theo TÊN —
  ảnh chứa "windows pe" và không chứa "setup"; (3) index 1; (4) index
  đầu tiên.
- Hệ quả trên ISO gốc: header boot.wim chuẩn mang sẵn **Boot Index = 2**
  nên nhánh (1) thắng — tool chọn **index 2 ("Microsoft Windows Setup")**,
  KHÔNG phải index 1. Chọn theo tên chỉ kích hoạt khi header mất Boot
  Index (ca ISO mod thực địa). Đây là hành vi an toàn: index 2 chính là
  ảnh boot mặc định của đĩa cài Microsoft.
- Kiểm chứng bootable trên ISO gốc: `check_wim_bootable()` thấy header
  Boot Index = 2 → Some(true) → **không chạy export /Bootable**, đi
  thẳng (main.rs:2028-2080, 2119-2135).
- Inject (startnet.cmd / winpeshl.ini / qsu_start.cmd / WinPEInstall.exe)
  vào index 2 chạy được như vào index 1: cả hai đều là ảnh WindowsPE,
  winpeshl.ini/startnet.cmd được WinPE đọc từ System32 của ảnh được
  boot bất kể index nào — và boot index 2 là đúng cách đĩa Microsoft
  boot. Khi nhánh tên chọn index 1 (WinPE thuần) thì cũng đủ cho engine:
  engine chỉ cần DISM + các lệnh chuẩn trong PE và file của chính nó,
  không cần thành phần Setup nào.
- Không còn chỗ nào hard-code ngữ nghĩa index 2: đường export dự phòng
  dò lại index trên file đã xuất (main.rs:2550), kiểm chứng sau chuẩn
  hoá dùng index 1 vì file xuất chỉ còn một index (main.rs:2195).

## 7. Secure Boot — **OK**

- Chuỗi boot chỉ gồm binary Microsoft ký số: firmware nạp bootmgfw.efi
  từ ESP (tool không thay file này), boot manager nạp winload.efi từ
  trong WIM — file này giữ nguyên byte gốc của Microsoft (DISM
  mount/commit chỉ thêm file của QSU, không sửa winload.efi). File WIM
  và boot.sdi là DỮ LIỆU, vốn không được ký và boot manager cũng không
  kiểm chữ ký của chúng — đây là cơ chế ramdisk WinPE chuẩn.
- Các exe QSU (WinPEInstall.exe, CaiDatWinWinPE.exe) là binary không ký
  nhưng chỉ chạy ở user-mode BÊN TRONG WinPE sau khi chuỗi boot đã xong
  — ngoài phạm vi của Secure Boot. Không inject driver kernel lạ lúc
  boot.
- Toàn workspace không có `testsigning` / `nointegritychecks`
  (grep cả 4 crate: 0 kết quả). Mục BCD tự tạo bằng bcdedit từ phiên
  admin là thao tác được phép khi Secure Boot bật; `/bootsequence` là
  boot một lần, không sửa thứ tự boot vĩnh viễn.
- Điểm không chứng minh được bằng code: winload.efi trong một ISO MOD
  có còn nguyên bản Microsoft hay không — với ISO gốc thì chắc chắn có.

## 8. Giả định Legacy-only còn sót — **RISK thấp, đã liệt kê**

- `find_efi_letter()` ở cả engine lẫn module tương tác (mục 4/5): chỉ
  dựa vào thư mục `\EFI`, không xác minh bootmgfw.efi/đĩa sở hữu.
- Dò firmware một API trong hai binary phía WinPE (mục 5).
- `stage.json` thiếu `bootMode` → mặc định "UEFI" (stage.rs:110).
- Ngược lại, các điểm từng lo đều sạch: phân tích phân vùng GPT bằng
  IOCTL_DISK_GET_DRIVE_LAYOUT_EX + GUID loại chuẩn (EFI
  C12A7328-F81F-11D2-BA4B-00A0C93EC93B, MSR E3C9E316-..., Recovery
  DE94BBA4-... — disk.rs:61-65, 386-391); script diskpart rẽ nhánh
  GPT/MBR rõ ràng, `active` chỉ ở nhánh MBR; reagentc dùng dạng offline
  `/target`; nhận diện ổ OS/ Data trong WinPE theo serial volume, không
  theo ký tự.

## Chưa chứng minh được nếu không chạy trên phần cứng UEFI thật

1. GetFirmwareType + đối chiếu {fwbootmgr} trên firmware OEM cụ thể
   (mới kiểm chứng trên VM Legacy).
2. Secure Boot của máy thật chấp nhận mục ramdisk tự tạo trọn gói
   (chính sách dbx/firmware từng hãng có thể khác nhau).
3. Boot thực tế vào ảnh đã inject — cả nhánh index 2 (ISO gốc) lẫn
   index 1 đã chuẩn hoá /Bootable (ISO mod) — trên UEFI + Secure Boot.
4. `find_efi_letter()` trên máy nhiều đĩa/nhiều ESP.
5. bcdboot /f UEFI ghi mục NVRAM và thứ tự boot firmware sau reboot
   trên máy thật (một số firmware cần kiểm chứng riêng).
