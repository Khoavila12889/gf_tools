# Utility Rebuild — Bộ công cụ hỗ trợ kỹ thuật Windows

Bản dựng lại **độc lập (clean-room)** của một bộ tiện ích IT gồm 32 chức năng,
viết hoàn toàn mới bằng PowerShell. Mục tiêu là **tương đương về chức năng**
với công cụ gốc mà người dùng đang sử dụng hợp pháp, không sao chép mã nguồn,
tài nguyên đồ họa hay văn bản của bản gốc (ngoại trừ các nhãn nút chức năng —
vốn thuần túy mô tả tính năng).

## Kiến trúc

- **Ngôn ngữ/môi trường:** Windows PowerShell 5.1 + Windows Forms
  (chạy trên mọi Windows 10/11, không cần cài thêm runtime).
- **Cấu trúc thư mục:**

  ```
  utility_rebuild/
  ├── README.md
  ├── BUILD_NOTES.md          ← trạng thái từng module
  ├── launcher/
  │   └── Utility.ps1         ← cửa sổ chính: lưới 32 nút + ô tìm kiếm
  └── modules/
      ├── BackupRestoreDriver.ps1   ← module đã hoàn thiện (module mẫu)
      └── ...                        ← mỗi chức năng một file .ps1 riêng
  ```

- **Launcher** (`launcher/Utility.ps1`) hiển thị lưới 4 cột × 8 hàng gồm 32
  nút chức năng tiếng Việt, kèm ô tìm kiếm lọc nút (không phân biệt dấu).
  Khi bấm nút, launcher chạy file `modules/<TênModule>.ps1` tương ứng bằng
  `Start-Process -Verb RunAs` (nâng quyền Administrator) vì hầu hết module
  đều thao tác hệ thống. Module nào chưa được xây dựng sẽ hiện thông báo
  "chưa có trong bản rebuild" thay vì lỗi.
- **Mỗi module là một script độc lập**: tự kiểm tra quyền admin (tự nâng
  quyền nếu cần), tự ghi log, không phụ thuộc vào launcher — có thể chạy
  riêng lẻ bằng cách chuột phải → *Run with PowerShell*.

## Đóng gói .exe (thực hiện sau, trên máy Windows)

Các file `.ps1` sẽ được đóng gói thành `.exe` bằng **PS2EXE** trên máy
Windows khi bản rebuild hoàn thiện:

```powershell
Install-Module ps2exe
Invoke-PS2EXE -InputFile .\launcher\Utility.ps1 -OutputFile .\Utility.exe -RequireAdmin
```

Giai đoạn hiện tại chỉ gồm mã nguồn `.ps1` để dễ rà soát và đối chiếu.

## Chính sách trung thực chức năng (fidelity policy)

- Mỗi module được xây dựng để **hành xử tương đương** bản gốc: cùng thao
  tác hệ thống (lệnh, khóa registry, dịch vụ…), cùng quy ước đặt tên thư
  mục/file xuất ra, cùng mã thoát được coi là thành công.
- Module `BackupRestoreDriver` là **module tham chiếu**: nó được port từ
  source đã dịch ngược hoàn chỉnh và kiểm chứng CRC, nên đạt mức tương
  đương cao nhất hiện có.
- Các module còn lại đang chờ khôi phục source (xem `BUILD_NOTES.md`);
  hành vi của chúng sẽ được **đối chiếu lại với memory dump** của bản gốc
  trước khi được coi là hoàn thiện.
- **Cố ý KHÔNG tái tạo** các hành vi sau của bản gốc, vì chúng không thuộc
  chức năng phục vụ người dùng:
  - Cổng kiểm tra key bản quyền / kích hoạt online và biến môi trường
    "token" mà các module gốc dùng để tự khóa.
  - Gửi telemetry (thông tin máy/người dùng) về server của tác giả.
  - Tự cài chứng thư gốc (root CA) của tác giả vào máy.
  - Tự thêm ngoại lệ Windows Defender cho chính chương trình.
  - Tự cập nhật từ kênh phát hành của tác giả.
- Riêng module **Kích Hoạt Win/Office**: bản rebuild chỉ hỗ trợ kích hoạt
  hợp lệ (nhập key chính hãng, xem trạng thái bản quyền) — không triển khai
  cơ chế kích hoạt qua KMS bên thứ ba.

## Cách chạy thử

```powershell
# Từ thư mục utility_rebuild (PowerShell thường cũng được,
# launcher sẽ tự nâng quyền cho từng module khi bấm nút):
powershell -ExecutionPolicy Bypass -File .\launcher\Utility.ps1
```

> Lưu ý: các file `.ps1` chứa tiếng Việt được lưu dạng **UTF-8 with BOM**
> để PowerShell 5.1 đọc đúng dấu. Nếu chỉnh sửa bằng trình soạn thảo khác,
> hãy giữ nguyên định dạng này.
