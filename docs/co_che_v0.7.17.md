# Tài liệu cơ chế — Cài mới qua WinPE (v0.7.17)

> Đây là bản sao dạng file của nội dung tab "Tài liệu" trong wizard
> Cài Đặt Win Tự Động (hằng `TAI_LIEU_TEXT` trong
> `rust/cai_dat_win/src/main.rs`). Sửa nội dung thì sửa cả hai nơi
> cho khớp. So với v0.7.12, bản này cộng CHẾ ĐỘ PHÂN VÙNG THỨ BA —
> "rebuild" (mục 7): chỉ xoá các phân vùng hệ thống đã chọn trên
> đĩa đích, chia lại layout mới trong vùng trống của chúng, giữ
> nguyên phân vùng data trên cùng đĩa (kể cả khi đĩa đích chính là
> đĩa chứa staging — ca máy 1 đĩa). Hai chế độ preserve/wipeDisk
> giữ nguyên hành vi. So với v0.7.13, bản này cộng bước ĐÓNG GÓI
> LẠI WINPE THÀNH WIM MỘT ẢNH BOOTABLE (mục 2, từ v0.7.15): sau
> khi inject, wizard xuất riêng ảnh PE đã inject thành WIM duy
> nhất một ảnh gắn cờ boot — khắc phục ca thực địa winload nạp
> nhầm ảnh Windows Setup thay vì engine của tool; và đồng bộ
> mục 4 với thay đổi inject driver từ QSU-DriverBackup của
> v0.7.14. So với v0.7.15, bản này cộng bước ENGINE TỰ GÁN KÝ
> TỰ Ổ CHO PHÂN VÙNG BOOT SAU KHI CHIA (mục 7, từ v0.7.16):
> khắc phục ca thực địa winpe-20261117-1811.log — diskpart
> báo gán ký tự S cho phân vùng EFI thành công mà EFI vẫn
> không có ký tự, engine cũ dừng cứng ngay sau bước xoá đĩa.
> So với v0.7.16, bản này cộng MOUNT ISO BA LỚP (mục 1, từ
> v0.7.17): khắc phục ca thực địa trên máy chạy Windows lite —
> "Đọc ISO" với ISO gốc báo "Mount-DiskImage không trả ký tự
> ổ đĩa (output: )" rỗng và huỷ đọc, vì đường mount cũ chỉ dựa
> vào một lệnh PowerShell và lỗi của cmdlet bị nuốt mất. Nay
> wizard dùng lại mount có sẵn, mount bằng PowerShell với lỗi
> hiện nguyên văn, và dự phòng bằng VirtDisk API gốc.

TÀI LIỆU CƠ CHẾ — CÀI MỚI QUA WINPE (cơ chế của bản build hiện tại)

Tài liệu này mô tả đúng những gì wizard thực thi khi chọn "Cài mới từ xa (qua WinPE)". Quá trình gồm hai giai đoạn: chuẩn bị (staging) chạy trong Windows hiện tại bằng wizard này, và cài thật chạy trong WinPE bằng engine WinPEInstall.exe.

1. STAGING TRÊN Ổ DATA
- Mọi thứ cần cho ca cài được chép vào thư mục staging (mặc định QSU-Stage) trên ổ Data: bộ nguồn sources, winpe.wim đã inject, boot.sdi, driver, autounattend.xml, thư mục custom và file stage.json.
- Ổ Data được đánh dấu bằng file marker ".qsu_data_drive" ở gốc ổ. Khi vào WinPE — nơi ký tự ổ được gán lại khác hẳn — engine tìm ổ Data bằng marker này, rồi đối chiếu serial phân vùng ghi trong stage.json.
- stage.json là "hợp đồng" giữa wizard và engine: phiên bản Windows + index, chế độ boot (UEFI/Legacy), chế độ phân vùng, serial phân vùng OS, serial đĩa đích, GUID mục BCD, tên máy, tài khoản. stage.json KHÔNG chứa mật khẩu (mật khẩu chỉ nằm trong autounattend.xml) và không chứa danh sách ứng dụng hậu cài.
- Nguồn sources được đọc từ ISO đã mount. Mount ISO có BA LỚP (từ v0.7.17): (1) ISO đã gắn sẵn (lần đọc trước, Explorer...) thì dùng lại ký tự ổ, đọc trạng thái qua Get-DiskImage; (2) mount bằng PowerShell Mount-DiskImage với lỗi ép thành lỗi dừng (terminating) — lỗi thật của cmdlet hiện nguyên văn trong hộp thoại thay vì thông báo rỗng như các bản trước; (3) nếu tầng PowerShell/cmdlet Storage không mount được (hay gặp trên các bản Windows lite bị cắt thành phần), wizard mount thẳng bằng VirtDisk API của Windows: mở ảnh ISO ở chế độ đọc-chỉ-đọc rồi dò ký tự ổ DVD mới xuất hiện sau khi gắn. Gỡ mount đối xứng theo đúng cơ chế đã mount: Dismount-DiskImage với mount PowerShell, DetachVirtualDisk với mount VirtDisk.

2. CHỌN ẢNH WINPE TRONG boot.wim
- boot.wim của ISO thường có 2 ảnh: "Microsoft Windows PE" và "Microsoft Windows Setup". Wizard chọn ảnh để mount và inject theo thứ tự: (1) "Boot Index" do header của WIM ghi sẵn — chính là ảnh mà bộ cài của Microsoft sẽ boot; (2) nếu header không mang Boot Index (hay gặp ở ISO mod đóng gói lại), chọn theo TÊN ảnh chứa "Windows PE" và không chứa "Setup".
- Ảnh được chọn được mount bằng DISM, inject các file khởi động của engine (qsu_start.cmd, startnet.cmd, winpeshl.ini) cùng WinPEInstall.exe, rồi unmount /Commit. Nếu mount bản gốc thất bại (WIM nén solid / dạng ESD), wizard xuất lại ảnh đó (Export-Image, nén Max) rồi mount bản xuất và thay tên thành winpe.wim — các bước sau không thay đổi.
- Ngay sau khi inject xong (từ v0.7.15, áp dụng cho MỌI ca cài mới): wizard ĐÓNG GÓI LẠI ảnh PE đã inject thành một WIM mới chỉ có đúng một ảnh — DISM /Export-Image /Bootable /Compress:Max xuất riêng index đã inject sang winpe_single.wim, xoá bản hai-ảnh, đổi tên bản một-ảnh thành winpe.wim. Lý do: winpe.wim sau inject vẫn mang cả hai ảnh của boot.wim gốc và header không chỉ định ảnh PE đã inject là ảnh boot, nên khi khởi động winload có thể nạp ảnh Windows Setup thay vì engine của tool (thực địa 2026-11-17: máy lên thẳng Windows Setup đòi driver). WIM một ảnh mang cờ boot thì winload chỉ có thể khởi động đúng engine — cùng nguyên tắc đóng gói lại WinPE của tool gốc. Đây là bước đóng gói thuần tuý: wizard vẫn KHÔNG đọc/phán cờ bootable từ output DISM (mục 9); chỉ khi chính lệnh export lỗi hoặc file đích rỗng thì dừng, và dừng TRƯỚC khi đăng ký BCD.

3. boot.sdi THEO FIRMWARE
- boot.sdi là file mồi để Boot Manager nạp WinPE từ ramdisk; biến thể của nó phải khớp kiểu firmware: UEFI dùng bản EFI, Legacy/BIOS dùng bản PCAT.
- Wizard lấy boot.sdi theo chuỗi dự phòng đã xếp theo firmware (UEFI thử bản EFI trước, Legacy thử bản PCAT trước); nguồn lần lượt là: sources\boot.sdi trong ISO -> \boot.sdi ở gốc ISO -> thư mục Boot\DVD của Windows đang chạy -> thư mục Recovery\WindowsRE của Windows đang chạy. Mọi vị trí đã thử đều được ghi vào log; không vị trí nào có file thì dừng với thông báo liệt kê đủ các vị trí.

4. DRIVER
- Trước khi staging, wizard sao lưu toàn bộ driver của Windows hiện tại (pnputil /export-driver) vào thư mục QSU-DriverBackup trên ổ Data.
- Driver lưu trữ (IRST/NVMe...) từ thư mục drivers cạnh wizard (hoặc đường dẫn khai trong settings) được chép vào staging\Drivers. Trong WinPE, sau khi bung ảnh Windows xong, engine inject offline cả hai nguồn vào bản Windows mới bằng DISM /Add-Driver /Recurse — staging\Drivers trước, rồi QSU-DriverBackup (tên thư mục đọc từ stage.json, chỉ inject khi tên tương đối an toàn và thư mục không rỗng; từ v0.7.14, các bản trước chỉ inject staging\Drivers nên driver của chính máy đó không được nạp lại) — máy chạy RAID/VMD vẫn nhận ổ ngay lần boot đầu tiên.

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
- TỰ GÁN KÝ TỰ Ổ CHO PHÂN VÙNG BOOT SAU KHI CHIA (từ v0.7.16 — áp dụng cho cả WIPE DISK và REBUILD): sau khi diskpart chia xong, engine đối chiếu lại layout thật trên đĩa đích. Phân vùng boot (EFI/System Reserved) có thể có mặt nhưng KHÔNG có ký tự ổ dùng được — thực địa đã gặp: diskpart báo "đã gán ký tự thành công" mà phân vùng EFI vẫn không có ký tự, và các bản trước dừng cứng ngay tại đó dù đĩa đã bị xoá trắng (máy mất cả Windows cũ lẫn đường vào Windows mới). Từ v0.7.16 engine TỰ CHỮA: tự gán ký tự cho phân vùng boot (ưu tiên S như script chia đĩa đã định; gán bằng script diskpart phụ, dự phòng bằng gán điểm mount trực tiếp lên volume), rồi kiểm chứng bằng cách TRUY CẬP THẬT đường dẫn gốc của ký tự đó — truy cập được mới tính là có ký tự, không tin riêng phần liệt kê. Chỉ khi phân vùng boot vắng mặt hẳn trong layout mới, hoặc tự gán + truy cập thử đều thất bại sau khi thử lại, engine mới dừng. Ký tự đã chốt được các bước sau (bung ảnh, bcdboot, chép WinRE) dùng tiếp như bình thường.

8. THƯ MỤC CUSTOM
- Thư mục "custom" đặt cạnh wizard được chép nguyên trạng vào staging, rồi vào ProgramData\QSU\custom của Windows mới. Wizard và engine chỉ CHÉP, không bao giờ thực thi nội dung trong đó — kỹ thuật viên tự chạy bộ cài / script của mình sau khi Windows mới đã lên.

9. KHÔNG KIỂM TRA CỜ BOOTABLE (đã gỡ ở v0.7.11)
- Các bản trước từng có một cổng kiểm chứng cờ bootable của winpe.wim: đọc chữ trong output DISM /Get-WimInfo, nếu phán "chưa đạt" thì xuất lại với /Bootable rồi kiểm chứng lại, vẫn "chưa đạt" thì chặn đứng trước khi đăng ký BCD. Thực địa cho thấy cổng này kết luận sai 4/4 lần chạy — chặn cả ISO Microsoft gốc — trong khi các tool khác vẫn cài bình thường từ chính các ISO đó.
- Theo yêu cầu của người dùng, từ v0.7.11 cổng này bị gỡ hẳn và KHÔNG quay lại: wizard không đọc/phán cờ bootable ở bất kỳ đâu. Từ v0.7.15, thay cho việc dùng WinPE nguyên trạng, wizard đóng gói lại ảnh PE đã inject thành WIM một ảnh bootable (mục 2) — đó là bước đóng gói cơ học để winload không nạp nhầm ảnh Setup, không kèm bất kỳ phán quyết hay kiểm chứng nào. Tính an toàn được đảm bảo bằng mục boot một lần (mục 5) và đối chiếu serial (mục 6), thay vì bằng một cổng phán quyết đã chứng minh là phán sai.

10. LOG
- Toàn bộ quá trình được ghi vào file install-<thời gian>.log trong thư mục QSU-Logs trên ổ Data, đồng thời hiện ở ô log của wizard. Các dòng bắt đầu bằng [Cơ chế] giải thích ngắn gọn công nghệ đang áp dụng tại bước đó.
