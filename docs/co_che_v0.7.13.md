# Tài liệu cơ chế — Cài mới qua WinPE (v0.7.13)

> Đây là bản sao dạng file của nội dung tab "Tài liệu" trong wizard
> Cài Đặt Win Tự Động (hằng `TAI_LIEU_TEXT` trong
> `rust/cai_dat_win/src/main.rs`). Sửa nội dung thì sửa cả hai nơi
> cho khớp. So với v0.7.12, bản này cộng CHẾ ĐỘ PHÂN VÙNG THỨ BA —
> "rebuild" (mục 7): chỉ xoá các phân vùng hệ thống đã chọn trên
> đĩa đích, chia lại layout mới trong vùng trống của chúng, giữ
> nguyên phân vùng data trên cùng đĩa (kể cả khi đĩa đích chính là
> đĩa chứa staging — ca máy 1 đĩa). Hai chế độ preserve/wipeDisk
> giữ nguyên hành vi.

TÀI LIỆU CƠ CHẾ — CÀI MỚI QUA WINPE (cơ chế của bản build hiện tại)

Tài liệu này mô tả đúng những gì wizard thực thi khi chọn "Cài mới từ xa (qua WinPE)". Quá trình gồm hai giai đoạn: chuẩn bị (staging) chạy trong Windows hiện tại bằng wizard này, và cài thật chạy trong WinPE bằng engine WinPEInstall.exe.

1. STAGING TRÊN Ổ DATA
- Mọi thứ cần cho ca cài được chép vào thư mục staging (mặc định QSU-Stage) trên ổ Data: bộ nguồn sources, winpe.wim đã inject, boot.sdi, driver, autounattend.xml, thư mục custom và file stage.json.
- Ổ Data được đánh dấu bằng file marker ".qsu_data_drive" ở gốc ổ. Khi vào WinPE — nơi ký tự ổ được gán lại khác hẳn — engine tìm ổ Data bằng marker này, rồi đối chiếu serial phân vùng ghi trong stage.json.
- stage.json là "hợp đồng" giữa wizard và engine: phiên bản Windows + index, chế độ boot (UEFI/Legacy), chế độ phân vùng, serial phân vùng OS, serial đĩa đích, GUID mục BCD, tên máy, tài khoản. stage.json KHÔNG chứa mật khẩu (mật khẩu chỉ nằm trong autounattend.xml) và không chứa danh sách ứng dụng hậu cài.

2. CHỌN ẢNH WINPE TRONG boot.wim
- boot.wim của ISO thường có 2 ảnh: "Microsoft Windows PE" và "Microsoft Windows Setup". Wizard chọn ảnh để mount và inject theo thứ tự: (1) "Boot Index" do header của WIM ghi sẵn — chính là ảnh mà bộ cài của Microsoft sẽ boot; (2) nếu header không mang Boot Index (hay gặp ở ISO mod đóng gói lại), chọn theo TÊN ảnh chứa "Windows PE" và không chứa "Setup".
- Ảnh được chọn được mount bằng DISM, inject các file khởi động của engine (qsu_start.cmd, startnet.cmd, winpeshl.ini) cùng WinPEInstall.exe, rồi unmount /Commit. Nếu mount bản gốc thất bại (WIM nén solid / dạng ESD), wizard xuất lại ảnh đó (Export-Image, nén Max) rồi mount bản xuất và thay tên thành winpe.wim — các bước sau không thay đổi.

3. boot.sdi THEO FIRMWARE
- boot.sdi là file mồi để Boot Manager nạp WinPE từ ramdisk; biến thể của nó phải khớp kiểu firmware: UEFI dùng bản EFI, Legacy/BIOS dùng bản PCAT.
- Wizard lấy boot.sdi theo chuỗi dự phòng đã xếp theo firmware (UEFI thử bản EFI trước, Legacy thử bản PCAT trước); nguồn lần lượt là: sources\boot.sdi trong ISO -> \boot.sdi ở gốc ISO -> thư mục Boot\DVD của Windows đang chạy -> thư mục Recovery\WindowsRE của Windows đang chạy. Mọi vị trí đã thử đều được ghi vào log; không vị trí nào có file thì dừng với thông báo liệt kê đủ các vị trí.

4. DRIVER
- Trước khi staging, wizard sao lưu toàn bộ driver của Windows hiện tại (pnputil /export-driver) vào thư mục QSU-DriverBackup trên ổ Data.
- Driver lưu trữ (IRST/NVMe...) từ thư mục drivers cạnh wizard (hoặc đường dẫn khai trong settings) được chép vào staging\Drivers. Trong WinPE, sau khi bung ảnh Windows xong, engine inject offline các driver này vào bản Windows mới bằng DISM /Add-Driver /Recurse — máy chạy RAID/VMD vẫn nhận ổ ngay lần boot đầu tiên.

5. MỤC BOOT MỘT LẦN (/bootsequence)
- Wizard đăng ký vào BCD một cặp entry ramdisk (device options + loader) trỏ tới winpe.wim và boot.sdi trong staging, đặt đường dẫn loader theo firmware (winload.efi cho UEFI, winload.exe cho Legacy), rồi kích hoạt bằng /bootsequence — chỉ hiệu lực cho lần khởi động KẾ TIẾP, đúng một lần.
- Nhờ vậy, nếu WinPE không lên được vì bất kỳ lý do gì, lần khởi động sau đó máy tự quay về Windows cũ: hệ thống chưa bị thay đổi gì và không bao giờ xảy ra tình trạng lặp boot vào WinPE.

6. NHẬN DIỆN Ổ ĐÍCH BẰNG SERIAL
- Cả wizard lẫn engine đều KHÔNG nhận diện ổ đĩa theo ký tự ổ hay nhãn ổ (ký tự thay đổi trong WinPE, nhãn có thể bị đổi hoặc trùng). Căn cứ duy nhất là serial: serial của phân vùng (volume serial) và serial phần cứng của đĩa vật lý (đọc bằng IOCTL, không phân tích output chữ của lệnh nào).
- Wizard ghi serial phân vùng OS và serial đĩa đích vào stage.json. Trong WinPE, ngay trước bước format/xoá, engine đối chiếu lại serial thật: không khớp là dừng hẳn, không đụng vào bất kỳ ổ nào. Ở chế độ WIPE DISK, đĩa chứa ổ Data/staging bị cấm tuyệt đối làm đĩa đích — engine kiểm tra lại độc lập, không tin hoàn toàn vào wizard. Ở chế độ REBUILD (mục 7), đĩa chứa staging ĐƯỢC làm đích nhưng phân vùng staging tự nó được bảo vệ bằng đối chiếu vị trí vật lý (extent) — engine chứng minh nó không nằm trong danh sách xoá trước khi động vào đĩa.

7. BA CHẾ ĐỘ PHÂN VÙNG
- PRESERVE (mặc định): chỉ format phân vùng Windows cũ rồi bung ảnh lên đó; mọi phân vùng khác (dữ liệu, EFI, Recovery hiện có) được giữ nguyên và tái sử dụng.
- WIPE DISK: xoá toàn bộ đĩa đích và chia lại theo chuẩn Microsoft. UEFI/GPT: EFI 260 MB FAT32 -> MSR 16 MB -> Windows NTFS (phần còn lại) -> Recovery 600 MB NTFS (đặt đúng GUID và thuộc tính phân vùng WinRE, chép Winre.wim vào và đăng ký bằng reagentc). Legacy/MBR: System Reserved 100 MB (đặt active) -> Windows -> Recovery 600 MB (id 27). Chế độ này chỉ chạy khi kỹ thuật viên đã gõ đúng serial của đĩa ở wizard để xác nhận xoá.
- REBUILD (mới ở v0.7.13): chỉ xoá CÁC PHÂN VÙNG HỆ THỐNG ĐÃ CHỌN trên đĩa đích — EFI, MSR, Windows cũ, Recovery cũ — rồi chia lại layout mới (cùng layout như WIPE DISK) trong đúng VÙNG TRỐNG của chúng; mọi phân vùng data trên cùng đĩa được giữ nguyên tuyệt đối. Sinh ra cho máy chỉ có 1 ổ đĩa (Windows cũ và data chung đĩa, đĩa chứa staging cũng chính là đĩa đích). Ở wizard, danh sách phân vùng của đĩa đích hiện dạng ô tick: phân vùng hệ thống tick sẵn (bỏ tick được, trừ phân vùng Windows cũ là bắt buộc), phân vùng data và phân vùng chứa bộ cài không tick được; xác nhận cũng phải gõ đúng serial đĩa như WIPE DISK. stage.json ghi thêm mảng rebuildPartitions: từng phân vùng sẽ xoá kèm loại, kích thước, offset và GUID duy nhất của phân vùng (đĩa GPT). Trong WinPE, engine đối chiếu lại TỪNG phân vùng bằng GUID (GPT) hoặc offset + kích thước (MBR, dung sai 1 MB), kiểm tra phân vùng chứa staging không nằm trong danh sách xoá, kiểm tra các phân vùng cần xoá tạo thành MỘT vùng liên tục và vùng đó đủ lớn cho layout mới (tối thiểu: boot + Recovery + Windows 10 GB) — TẤT CẢ trước khi xoá bất cứ thứ gì; lệch bất kỳ điểm nào là dừng cứng với exit code 13, không đụng vào đĩa. Khi chia lại, các phân vùng mới được ghim đúng offset vào vùng vừa trống nên không bao giờ lấn sang phân vùng data kề bên; sau khi chia, engine xác minh lại cả phân vùng mới lẫn các phân vùng được giữ (còn nguyên vị trí và kích thước).

8. THƯ MỤC CUSTOM
- Thư mục "custom" đặt cạnh wizard được chép nguyên trạng vào staging, rồi vào ProgramData\QSU\custom của Windows mới. Wizard và engine chỉ CHÉP, không bao giờ thực thi nội dung trong đó — kỹ thuật viên tự chạy bộ cài / script của mình sau khi Windows mới đã lên.

9. KHÔNG KIỂM TRA CỜ BOOTABLE (đã gỡ ở v0.7.11)
- Các bản trước từng có một cổng kiểm chứng cờ bootable của winpe.wim: đọc chữ trong output DISM /Get-WimInfo, nếu phán "chưa đạt" thì xuất lại với /Bootable rồi kiểm chứng lại, vẫn "chưa đạt" thì chặn đứng trước khi đăng ký BCD. Thực địa cho thấy cổng này kết luận sai 4/4 lần chạy — chặn cả ISO Microsoft gốc — trong khi các tool khác vẫn cài bình thường từ chính các ISO đó.
- Theo yêu cầu của người dùng, từ v0.7.11 cổng này bị gỡ hẳn: wizard dùng WinPE nguyên trạng của ISO. Tính an toàn được đảm bảo bằng mục boot một lần (mục 5) và đối chiếu serial (mục 6), thay vì bằng một cổng phán quyết đã chứng minh là phán sai.

10. LOG
- Toàn bộ quá trình được ghi vào file install-<thời gian>.log trong thư mục QSU-Logs trên ổ Data, đồng thời hiện ở ô log của wizard. Các dòng bắt đầu bằng [Cơ chế] giải thích ngắn gọn công nghệ đang áp dụng tại bước đó.
