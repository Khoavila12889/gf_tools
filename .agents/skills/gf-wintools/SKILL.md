---
name: gf-wintools
description: Phát triển, sửa lỗi, kiểm thử và đóng gói Golden Farm Windows Tools (gf_tools, wintools, UtilityRebuild). Dùng cho workspace Rust, launcher, các module Windows, wizard cài Windows, engine WinPE, driver, BitLocker, WiFi, dọn rác, Update và tweaks; hoặc khi xử lý build GNU/MSVC và xuất EXE của dự án.
---

# Golden Farm Windows Tools

## Đọc nguồn trước khi sửa

1. Tìm repo chứa README.md và rust/Cargo.toml; vị trí ban đầu là E:\wintools\gf_tools. Đọc AGENTS.md áp dụng và git status --short; giữ nguyên thay đổi sẵn có.
2. Đọc README.md, mục liên quan trong BUILD_NOTES.md và mã module. Dùng docs/README_FULL.md để tra luồng, cấu trúc gói và kiểm thử. Tài liệu có thể khác phiên bản: README ghi v0.7.18, README_FULL ghi v0.7.13 trong lần khảo sát; không lấy tên gói drop làm bằng chứng phiên bản đang chạy.
3. Với cài Windows, đọc docs/logic_cai_dat_win_online.md và docs/co_che_v*.md phù hợp. Đối chiếu mã và log; phân biệt đã viết, đã build và đã kiểm chứng máy thật.
4. Tìm bằng rg; nếu thiếu, dùng Get-ChildItem/Select-String trong phạm vi nguồn, tránh target và thư mục drop.

## Chọn đúng mã nguồn

| Phạm vi | Đường dẫn từ gốc repo |
|---|---|
| Launcher, nút, tìm kiếm, icon | rust/launcher/ |
| Wizard Windows | rust/cai_dat_win/ |
| Engine staging WinPE | rust/winpe_install/ |
| Cài tương tác WinPE | rust/cai_dat_win_winpe/ |
| Đĩa, phân vùng, chốt boot | rust/qsu_common/src/disk.rs |
| Hợp đồng staging/cấu hình | rust/qsu_common/src/stage.rs, settings.rs |
| Mount ISO, lệnh, log, hệ thống | rust/qsu_common/src/iso.rs, cmd.rs, log.rs, sys.rs |
| Driver, BitLocker, WiFi | rust/backup_restore_driver/, tat_bitlocker/, sao_luu_wifi/ |
| Dọn rác, Update, BIOS, tweaks | rust/don_rac/, quan_ly_update/, phim_tat_bios/, tuy_chinh_windows/ |
| Gói một EXE | packaging/build-single.ps1, packaging/single.rs |

## Quy tắc triển khai

- Giữ mỗi module là EXE Rust native độc lập, UI tiếng Việt và control Windows gốc. Tác vụ dài chạy worker thread, cập nhật qua NWG Notice; khoá nút khi chạy và mở lại cả khi lỗi.
- Đọc luồng nâng quyền để tránh UAC lặp. BIOS là tra cứu không cần admin; WinPE có ngữ cảnh riêng.
- Dùng qsu_common cho logic chung. Khi đổi stage.json, sửa cả producer/consumer và kiểm tra mặc định, tương thích dữ liệu cũ.
- Ưu tiên Win32/COM, dữ liệu có cấu trúc; tránh parse output bản địa hoá. Không thêm phụ thuộc PowerShell vào WinPE. Đọc các ngoại lệ PowerShell hiện có trước khi sửa.
- Khi thêm module, cập nhật workspace, binary, launcher mapping, log, đóng gói và tài liệu. Nút chưa triển khai phải báo rõ, không giả thành công.
- Giữ PowerShell cũ trong launcher/ và modules/ làm tham chiếu; không quay lại PS2EXE.
- Giữ chính sách dự án: không thêm telemetry, root CA, ngoại lệ Defender, cổng token hay KMS bên thứ ba; kích hoạt chỉ bằng key hợp lệ. Không sao chép tài nguyên độc quyền.

## Bảo vệ dữ liệu và chẩn đoán WinPE

- Phân biệt preserve (format Windows đích), wipeDisk (xoá cả đĩa, cấm đĩa chứa staging/nguồn), rebuild (chỉ thay phân vùng hệ thống đã chọn, có thể cùng đĩa staging nhưng phải giữ DATA).
- Dùng serial đĩa, volume serial và marker .qsu_data_drive. Không tin ký tự ổ hoặc disk number giữ nguyên qua reboot. Đối chiếu lại định danh và layout/GUID/offset/size cần thiết ngay trước ghi; không xác định được thì dừng.
- Giữ xác nhận gõ serial và phạm vi xoá trong UI. Không chạy format, diskpart clean, sửa BCD hay reboot máy host chỉ để thử thay đổi mã.
- Khi lỗi mount ISO, kiểm tra mount có sẵn, lỗi PowerShell đầy đủ và VirtDisk fallback. Khi PE không chạy engine, kiểm tra ảnh đã inject, export WIM một ảnh bootable và BCD bootsequence một lần.
- Khi DISM đạt 100% rồi lỗi, kiểm tra boot partition CÙNG ĐĨA Windows, disk::finalize_boot(), mount letter truy cập thật, exit code/output bcdboot và file boot. Không coi bung ảnh xong là cài thành công.
- Giữ console dự phòng, tiến trình DISM và màn hình lỗi đứng lại kèm đường dẫn log. Tìm log cạnh EXE hoặc QSU-Logs trên ổ dữ liệu; lấy đúng lượt lỗi và phiên bản gói.
- Không ghi mật khẩu/WiFi key/product key vào log. Giữ loại trường mật khẩu khỏi stage.json; che dữ liệu nhạy cảm trong autounattend.xml khi báo cáo.
- Chỉ sao chép custom, không tự chạy nội dung. Không tự xây agent hậu cài; đọc kỹ hook PostInstall hiện có nếu yêu cầu đụng tới.

## Build và bàn giao

1. Đọc rust/.cargo/config.toml, manifests và script đóng gói. Xác nhận toolchain/target/linker thực tế. Cấu hình khảo sát mặc định GNU với linker Linux; trên Windows không chạy mặc định một cách mù quáng.
2. Với GNU đầy đủ, chạy cargo build --release --target x86_64-pc-windows-gnu trong rust/. Với MSVC đã cài, chỉ định --target x86_64-pc-windows-msvc. Nếu cần CRT tĩnh, dùng -C target-feature=+crt-static và khôi phục biến môi trường sau build.
3. Check/build crate bị ảnh hưởng; build toàn workspace khi sửa qsu_common hoặc phát hành. Chạy test phù hợp, kiểm tra logic định danh/phân vùng/stage JSON khi thay đổi. Thử thao tác phá huỷ chỉ trên VM/máy test được chỉ định.
4. Đọc packaging/build-single.ps1 trước khi dùng: bản khảo sát pin revision 232f4db; -UpdatedLauncher chỉ chép thay launcher, KHÔNG lấy sửa đổi module khác. Xác nhận nguồn gói chứa đúng bản sửa trước phát hành; không lặng lẽ xuất binary cũ.
5. Kiểm tra đủ EXE và modules/install/WinPEInstall.exe. Với một EXE, giải nén bằng --extract-to rồi so SHA-256 payload. Đừng coi kiểm tra đóng gói là test chức năng.
6. Thử UI/DPI nếu sửa layout; thử chế độ UEFI/GPT hoặc Legacy/MBR và preserve/wipeDisk/rebuild bị ảnh hưởng trên máy test. Báo rõ ca chưa chạy.
7. Cập nhật tài liệu thay đổi hành vi; đồng bộ TAI_LIEU_TEXT của wizard với docs/co_che_v*.md tương ứng khi chỉnh nội dung hướng dẫn.
8. Bàn giao bằng tiếng Việt: sửa gì, kiểm tra đã chạy, gói tạo ra nếu có và giới hạn chưa kiểm chứng. Không tuyên bố sửa thành công trên máy thật chỉ vì build qua.
