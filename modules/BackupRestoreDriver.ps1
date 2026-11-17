#Requires -Version 5.1
<#
.SYNOPSIS
    Backup/Restore Driver - Sao lưu và khôi phục driver thiết bị.

.DESCRIPTION
    Liệt kê toàn bộ driver bên thứ ba trên máy bằng WMI
    (Win32_PnPSignedDriver), cho phép lọc theo nhóm thiết bị
    (Graphics / Audio / Network / Storage / Input / Other), chỉ hiện
    driver không phải của Microsoft, và tìm kiếm theo tên.

      - Backup : với mỗi driver được chọn, chạy
                 pnputil /export-driver <oemXX.inf> <thư mục đích>\<tên INF>
      - Restore: tìm đệ quy mọi file *.inf trong thư mục đã chọn và chạy
                 pnputil /add-driver <inf> /install cho từng file
                 (mã thoát 0, 259, 3010 đều coi là thành công),
                 cuối cùng chạy pnputil /scan-devices.
      - Xuất HTML: ghi danh sách driver ra DanhSachDriver.html cạnh
                 script rồi mở lên.

    Mọi thao tác đều ghi vào DriverTool_Log.txt cạnh script.
    Module cần quyền Administrator (sẽ tự xin nâng quyền nếu chưa có).

    Đây là bản port viết mới (clean-room) theo hành vi đã kiểm chứng
    của module gốc; không chứa cơ chế khóa bản quyền của bản gốc.
#>

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
[System.Windows.Forms.Application]::EnableVisualStyles()

# ---------------------------------------------------------------------------
# Tự nâng quyền Administrator nếu đang chạy thường
# ---------------------------------------------------------------------------
$currentIdentity  = [Security.Principal.WindowsIdentity]::GetCurrent()
$currentPrincipal = New-Object Security.Principal.WindowsPrincipal($currentIdentity)
if (-not $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    try {
        Start-Process -FilePath 'powershell.exe' -Verb RunAs `
            -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`""
    } catch { }
    exit
}

# ---------------------------------------------------------------------------
# Hằng số / trạng thái
# ---------------------------------------------------------------------------
$script:ScriptDir = $PSScriptRoot
$script:LogFile   = Join-Path $script:ScriptDir 'DriverTool_Log.txt'
$script:AllDrivers = @()          # toàn bộ driver đã quét
$script:Checked    = @{}          # InfName -> $true/$false (giữ khi lọc lại)
$script:Rebuilding = $false       # cờ chặn sự kiện ItemChecked khi nạp lại list

# pnputil: tiến trình 32-bit trên Windows 64-bit phải dùng bản SysNative
if ([Environment]::Is64BitOperatingSystem -and -not [Environment]::Is64BitProcess) {
    $script:PnpUtil = Join-Path $env:WINDIR 'SysNative\pnputil.exe'
} else {
    $script:PnpUtil = Join-Path $env:WINDIR 'System32\pnputil.exe'
}

# ---------------------------------------------------------------------------
# Ghi log ra DriverTool_Log.txt cạnh script
# ---------------------------------------------------------------------------
function Write-DriverLog {
    param([Parameter(Mandatory)][string]$Message)
    $line = '[{0}] {1}' -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $Message
    try {
        Add-Content -LiteralPath $script:LogFile -Value $line -Encoding UTF8 -ErrorAction Stop
    } catch { }
}

# ---------------------------------------------------------------------------
# Phân nhóm thiết bị từ DeviceClass (GUID lớp cài đặt của Windows)
# Win32_PnPSignedDriver.DeviceClass trả về GUID dạng "{4d36e...}".
# ---------------------------------------------------------------------------
function Get-DriverCategory {
    param([AllowEmptyString()][string]$DeviceClass)
    $guid = ($DeviceClass -replace '[{}]', '').Trim().ToLower()
    switch ($guid) {
        '4d36e968-e325-11ce-bfc1-08002be10318' { return 'Graphics' }  # Display
        '4d36e96c-e325-11ce-bfc1-08002be10318' { return 'Audio' }     # MEDIA
        '4d36e972-e325-11ce-bfc1-08002be10318' { return 'Network' }   # Net
        '4d36e97b-e325-11ce-bfc1-08002be10318' { return 'Storage' }   # SCSIAdapter
        '4d36e96a-e325-11ce-bfc1-08002be10318' { return 'Storage' }   # HDC (IDE/ATA)
        '4d36e967-e325-11ce-bfc1-08002be10318' { return 'Storage' }   # DiskDrive
        '4d36e96b-e325-11ce-bfc1-08002be10318' { return 'Input' }     # Keyboard
        '4d36e96f-e325-11ce-bfc1-08002be10318' { return 'Input' }     # Mouse
        '745a17a0-74d3-11d0-b6fe-00a0c90f57da' { return 'Input' }     # HIDClass
        default { return 'Other' }
    }
}

# Bỏ dấu tiếng Việt (dùng cho ô tìm kiếm)
function ConvertTo-Unaccented {
    param([AllowEmptyString()][string]$Text)
    if ([string]::IsNullOrEmpty($Text)) { return '' }
    $normalized = $Text.Normalize([System.Text.NormalizationForm]::FormD)
    $sb = New-Object System.Text.StringBuilder
    foreach ($ch in $normalized.ToCharArray()) {
        if ([System.Globalization.CharUnicodeInfo]::GetUnicodeCategory($ch) -ne
            [System.Globalization.UnicodeCategory]::NonSpacingMark) {
            [void]$sb.Append($ch)
        }
    }
    return ($sb.ToString().Replace('đ', 'd').Replace('Đ', 'D'))
}

# ---------------------------------------------------------------------------
# Thư mục backup mặc định:
#   - Có ổ D:  -> thư mục backup_driver* sẵn có trên D:\ (nếu tìm thấy),
#                 nếu không: D:\backup_driver_yyyyMMdd
#   - Không có D: -> Desktop\backup_driver_yyyyMMdd
# ---------------------------------------------------------------------------
function Get-DefaultBackupFolder {
    $stamp = Get-Date -Format 'yyyyMMdd'
    if (Test-Path 'D:\') {
        $existing = Get-ChildItem -Path 'D:\' -Directory -Filter 'backup_driver*' `
                        -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($existing) { return $existing.FullName }
        return "D:\backup_driver_$stamp"
    }
    $desktop = [Environment]::GetFolderPath('Desktop')
    return (Join-Path $desktop "backup_driver_$stamp")
}

# ---------------------------------------------------------------------------
# Quét driver bằng WMI: SELECT * FROM Win32_PnPSignedDriver
# WHERE DeviceName IS NOT NULL
# ---------------------------------------------------------------------------
function Get-DriverList {
    $query = 'SELECT * FROM Win32_PnPSignedDriver WHERE DeviceName IS NOT NULL'
    $rows = Get-CimInstance -Query $query -ErrorAction Stop
    $list = @()
    foreach ($row in $rows) {
        $dateText = ''
        if ($row.DriverDate) {
            try { $dateText = ([datetime]$row.DriverDate).ToString('dd/MM/yyyy') } catch { }
        }
        $list += [pscustomobject]@{
            DeviceName    = [string]$row.DeviceName
            Manufacturer  = [string]$row.Manufacturer
            Category      = Get-DriverCategory ([string]$row.DeviceClass)
            DriverVersion = [string]$row.DriverVersion
            DriverDate    = $dateText
            InfName       = [string]$row.InfName
        }
    }
    return ($list | Sort-Object Category, DeviceName)
}

# ===========================================================================
# GIAO DIỆN
# ===========================================================================
$form = New-Object System.Windows.Forms.Form
$form.Text          = 'Backup/Restore Driver'
$form.Size          = New-Object System.Drawing.Size(950, 600)
$form.StartPosition = 'CenterScreen'
$form.Font          = New-Object System.Drawing.Font('Segoe UI', 9.5)

# --- Hàng lọc theo nhóm thiết bị ---
$grpFilter = New-Object System.Windows.Forms.GroupBox
$grpFilter.Text     = 'Nhóm thiết bị'
$grpFilter.Location = New-Object System.Drawing.Point(10, 8)
$grpFilter.Size     = New-Object System.Drawing.Size(560, 48)
$form.Controls.Add($grpFilter)

$script:CategoryBoxes = @{}
$categories = @('Graphics', 'Audio', 'Network', 'Storage', 'Input', 'Other')
$x = 12
foreach ($cat in $categories) {
    $chk = New-Object System.Windows.Forms.CheckBox
    $chk.Text     = $cat
    $chk.Checked  = $true
    $chk.AutoSize = $true
    $chk.Location = New-Object System.Drawing.Point($x, 19)
    $chk.Add_CheckedChanged({ Refresh-DriverList })
    $grpFilter.Controls.Add($chk)
    $script:CategoryBoxes[$cat] = $chk
    $x += 88
}

# --- Ô tìm kiếm + tuỳ chọn chỉ driver không phải Microsoft ---
$chkNonMs = New-Object System.Windows.Forms.CheckBox
$chkNonMs.Text     = 'Chỉ driver không phải Microsoft'
$chkNonMs.Checked  = $true
$chkNonMs.AutoSize = $true
$chkNonMs.Location = New-Object System.Drawing.Point(584, 24)
$chkNonMs.Add_CheckedChanged({ Refresh-DriverList })
$form.Controls.Add($chkNonMs)

$lblSearch = New-Object System.Windows.Forms.Label
$lblSearch.Text     = 'Tìm kiếm:'
$lblSearch.AutoSize = $true
$lblSearch.Location = New-Object System.Drawing.Point(10, 66)
$form.Controls.Add($lblSearch)

$txtSearch = New-Object System.Windows.Forms.TextBox
$txtSearch.Location = New-Object System.Drawing.Point(76, 62)
$txtSearch.Size     = New-Object System.Drawing.Size(300, 24)
$txtSearch.Add_TextChanged({ Refresh-DriverList })
$form.Controls.Add($txtSearch)

$lblCount = New-Object System.Windows.Forms.Label
$lblCount.AutoSize  = $true
$lblCount.Location  = New-Object System.Drawing.Point(390, 66)
$form.Controls.Add($lblCount)

# --- Danh sách driver (ListView có checkbox) ---
$list = New-Object System.Windows.Forms.ListView
$list.Location    = New-Object System.Drawing.Point(10, 94)
$list.Size        = New-Object System.Drawing.Size(914, 356)
$list.Anchor      = 'Top,Bottom,Left,Right'
$list.View        = 'Details'
$list.CheckBoxes  = $true
$list.FullRowSelect = $true
$list.GridLines   = $true
[void]$list.Columns.Add('Thiết bị',      330)
[void]$list.Columns.Add('Hãng sản xuất', 150)
[void]$list.Columns.Add('Nhóm',           75)
[void]$list.Columns.Add('Phiên bản',     115)
[void]$list.Columns.Add('Ngày',           85)
[void]$list.Columns.Add('File INF',      140)
$list.Add_ItemChecked({
    param($sender, $e)
    if ($script:Rebuilding) { return }
    if ($e.Item -and $e.Item.Tag) {
        $script:Checked[$e.Item.Tag.InfName] = $e.Item.Checked
    }
})
$form.Controls.Add($list)

# --- Hàng chọn thư mục đích ---
$lblFolder = New-Object System.Windows.Forms.Label
$lblFolder.Text     = 'Thư mục backup:'
$lblFolder.AutoSize = $true
$lblFolder.Location = New-Object System.Drawing.Point(10, 462)
$lblFolder.Anchor   = 'Bottom,Left'
$form.Controls.Add($lblFolder)

$txtFolder = New-Object System.Windows.Forms.TextBox
$txtFolder.Location = New-Object System.Drawing.Point(116, 458)
$txtFolder.Size     = New-Object System.Drawing.Size(660, 24)
$txtFolder.Anchor   = 'Bottom,Left,Right'
$form.Controls.Add($txtFolder)

$btnBrowse = New-Object System.Windows.Forms.Button
$btnBrowse.Text     = 'Chọn...'
$btnBrowse.Location = New-Object System.Drawing.Point(784, 457)
$btnBrowse.Size     = New-Object System.Drawing.Size(70, 26)
$btnBrowse.Anchor   = 'Bottom,Right'
$form.Controls.Add($btnBrowse)

$btnRescan2 = New-Object System.Windows.Forms.Button
$btnRescan2.Text     = 'Quét lại'
$btnRescan2.Location = New-Object System.Drawing.Point(860, 457)
$btnRescan2.Size     = New-Object System.Drawing.Size(64, 26)
$btnRescan2.Anchor   = 'Bottom,Right'
$form.Controls.Add($btnRescan2)

# --- Hàng nút thao tác ---
$btnBackup = New-Object System.Windows.Forms.Button
$btnBackup.Text     = 'Backup'
$btnBackup.Font     = New-Object System.Drawing.Font('Segoe UI', 10, [System.Drawing.FontStyle]::Bold)
$btnBackup.Location = New-Object System.Drawing.Point(116, 494)
$btnBackup.Size     = New-Object System.Drawing.Size(130, 34)
$btnBackup.Anchor   = 'Bottom,Left'
$form.Controls.Add($btnBackup)

$btnRestore = New-Object System.Windows.Forms.Button
$btnRestore.Text     = 'Restore'
$btnRestore.Font     = New-Object System.Drawing.Font('Segoe UI', 10, [System.Drawing.FontStyle]::Bold)
$btnRestore.Location = New-Object System.Drawing.Point(256, 494)
$btnRestore.Size     = New-Object System.Drawing.Size(130, 34)
$btnRestore.Anchor   = 'Bottom,Left'
$form.Controls.Add($btnRestore)

$btnHtml = New-Object System.Windows.Forms.Button
$btnHtml.Text     = 'Xuất HTML'
$btnHtml.Location = New-Object System.Drawing.Point(396, 494)
$btnHtml.Size     = New-Object System.Drawing.Size(110, 34)
$btnHtml.Anchor   = 'Bottom,Left'
$form.Controls.Add($btnHtml)

$btnAll = New-Object System.Windows.Forms.Button
$btnAll.Text     = 'Chọn tất cả'
$btnAll.Location = New-Object System.Drawing.Point(516, 494)
$btnAll.Size     = New-Object System.Drawing.Size(100, 34)
$btnAll.Anchor   = 'Bottom,Left'
$form.Controls.Add($btnAll)

$btnNone = New-Object System.Windows.Forms.Button
$btnNone.Text     = 'Bỏ chọn'
$btnNone.Location = New-Object System.Drawing.Point(626, 494)
$btnNone.Size     = New-Object System.Drawing.Size(90, 34)
$btnNone.Anchor   = 'Bottom,Left'
$form.Controls.Add($btnNone)

$lblStatus = New-Object System.Windows.Forms.Label
$lblStatus.AutoSize = $true
$lblStatus.Location = New-Object System.Drawing.Point(10, 538)
$lblStatus.Anchor   = 'Bottom,Left'
$form.Controls.Add($lblStatus)

# ===========================================================================
# LOGIC
# ===========================================================================

# Nạp lại ListView theo bộ lọc hiện tại, giữ trạng thái tick theo InfName
function Refresh-DriverList {
    if (-not $list) { return }
    $script:Rebuilding = $true
    $list.BeginUpdate()
    $list.Items.Clear()

    $query = ConvertTo-Unaccented $txtSearch.Text.Trim()
    $shown = 0
    foreach ($drv in $script:AllDrivers) {
        if (-not $script:CategoryBoxes[$drv.Category].Checked) { continue }
        if ($chkNonMs.Checked -and $drv.Manufacturer -match 'Microsoft') { continue }
        if ($query -ne '') {
            $hay = ConvertTo-Unaccented ("{0} {1} {2}" -f $drv.DeviceName, $drv.Manufacturer, $drv.InfName)
            if (-not $hay.ToLower().Contains($query.ToLower())) { continue }
        }
        $item = New-Object System.Windows.Forms.ListViewItem($drv.DeviceName)
        [void]$item.SubItems.Add($drv.Manufacturer)
        [void]$item.SubItems.Add($drv.Category)
        [void]$item.SubItems.Add($drv.DriverVersion)
        [void]$item.SubItems.Add($drv.DriverDate)
        [void]$item.SubItems.Add($drv.InfName)
        $item.Tag = $drv
        if ($script:Checked.ContainsKey($drv.InfName)) {
            $item.Checked = [bool]$script:Checked[$drv.InfName]
        }
        [void]$list.Items.Add($item)
        $shown++
    }
    $list.EndUpdate()
    $script:Rebuilding = $false
    $lblCount.Text = "Hiển thị: $shown / $($script:AllDrivers.Count) driver"
}

function Set-Status {
    param([string]$Text)
    $lblStatus.Text = $Text
    $form.Refresh()
    [System.Windows.Forms.Application]::DoEvents()
}

# Quét lại toàn bộ driver từ WMI
function Invoke-DriverScan {
    Set-Status 'Đang quét driver...'
    try {
        $script:AllDrivers = @(Get-DriverList)
        Write-DriverLog "Đã quét $($script:AllDrivers.Count) driver."
    } catch {
        Write-DriverLog "Lỗi quét driver: $($_.Exception.Message)"
        [System.Windows.Forms.MessageBox]::Show(
            "Không quét được danh sách driver.`r`n$($_.Exception.Message)",
            'Lỗi', 'OK', 'Error') | Out-Null
    }
    Refresh-DriverList
    Set-Status 'Sẵn sàng.'
}

$btnBrowse.Add_Click({
    $dlg = New-Object System.Windows.Forms.FolderBrowserDialog
    $dlg.Description = 'Chọn thư mục backup driver'
    if (Test-Path -LiteralPath $txtFolder.Text) { $dlg.SelectedPath = $txtFolder.Text }
    if ($dlg.ShowDialog() -eq 'OK') { $txtFolder.Text = $dlg.SelectedPath }
})

$btnRescan2.Add_Click({ Invoke-DriverScan })

$btnAll.Add_Click({
    foreach ($item in $list.Items) { $item.Checked = $true }
})
$btnNone.Add_Click({
    foreach ($item in $list.Items) { $item.Checked = $false }
})

# --- Backup: pnputil /export-driver cho từng driver đã tick ---
$btnBackup.Add_Click({
    $selected = @($script:AllDrivers | Where-Object {
        $script:Checked.ContainsKey($_.InfName) -and $script:Checked[$_.InfName]
    })
    if ($selected.Count -eq 0) {
        [System.Windows.Forms.MessageBox]::Show(
            'Chưa chọn driver nào. Hãy tick vào các driver cần backup.',
            'Backup Driver', 'OK', 'Information') | Out-Null
        return
    }
    $destRoot = $txtFolder.Text.Trim()
    if ($destRoot -eq '') {
        [System.Windows.Forms.MessageBox]::Show(
            'Chưa chọn thư mục đích.', 'Backup Driver', 'OK', 'Information') | Out-Null
        return
    }
    if (-not (Test-Path -LiteralPath $destRoot)) {
        New-Item -ItemType Directory -Path $destRoot -Force | Out-Null
    }

    Write-DriverLog "=== BACKUP: $($selected.Count) driver -> $destRoot"
    $ok = 0; $fail = 0
    foreach ($drv in $selected) {
        # Mỗi driver xuất vào thư mục con đặt theo tên file INF (bỏ đuôi .inf)
        $subName = [System.IO.Path]::GetFileNameWithoutExtension($drv.InfName)
        $dest = Join-Path $destRoot $subName
        if (-not (Test-Path -LiteralPath $dest)) {
            New-Item -ItemType Directory -Path $dest -Force | Out-Null
        }
        Set-Status "Đang backup: $($drv.DeviceName) ($($drv.InfName))..."
        $output = & $script:PnpUtil /export-driver $drv.InfName $dest 2>&1 | Out-String
        $code = $LASTEXITCODE
        if ($code -eq 0) {
            $ok++
            Write-DriverLog "OK   $($drv.InfName) -> $dest"
        } else {
            $fail++
            Write-DriverLog "FAIL $($drv.InfName) (exit $code): $($output.Trim())"
        }
    }
    Write-DriverLog "=== BACKUP xong: thành công $ok, lỗi $fail"
    Set-Status "Backup xong: thành công $ok, lỗi $fail."
    [System.Windows.Forms.MessageBox]::Show(
        "Backup hoàn tất.`r`nThành công: $ok`r`nLỗi: $fail`r`n`r`nChi tiết xem DriverTool_Log.txt",
        'Backup Driver', 'OK', 'Information') | Out-Null
})

# --- Restore: pnputil /add-driver cho mọi *.inf đệ quy + /scan-devices ---
$btnRestore.Add_Click({
    $srcRoot = $txtFolder.Text.Trim()
    if ($srcRoot -eq '' -or -not (Test-Path -LiteralPath $srcRoot)) {
        [System.Windows.Forms.MessageBox]::Show(
            'Thư mục backup không tồn tại. Hãy chọn thư mục chứa driver đã backup.',
            'Restore Driver', 'OK', 'Information') | Out-Null
        return
    }
    $infs = @(Get-ChildItem -LiteralPath $srcRoot -Recurse -Filter '*.inf' `
                -File -ErrorAction SilentlyContinue)
    if ($infs.Count -eq 0) {
        [System.Windows.Forms.MessageBox]::Show(
            'Không tìm thấy file .inf nào trong thư mục đã chọn.',
            'Restore Driver', 'OK', 'Information') | Out-Null
        return
    }
    $confirm = [System.Windows.Forms.MessageBox]::Show(
        "Tìm thấy $($infs.Count) file .inf. Cài đặt toàn bộ vào hệ thống?",
        'Restore Driver', 'YesNo', 'Question')
    if ($confirm -ne 'Yes') { return }

    Write-DriverLog "=== RESTORE: $($infs.Count) file .inf từ $srcRoot"
    $ok = 0; $fail = 0
    foreach ($inf in $infs) {
        Set-Status "Đang cài: $($inf.FullName)..."
        $output = & $script:PnpUtil /add-driver $inf.FullName /install 2>&1 | Out-String
        $code = $LASTEXITCODE
        # 0 = thành công, 259 = không cần thao tác, 3010 = cần khởi động lại
        if ($code -in @(0, 259, 3010)) {
            $ok++
            Write-DriverLog "OK   $($inf.FullName) (exit $code)"
        } else {
            $fail++
            Write-DriverLog "FAIL $($inf.FullName) (exit $code): $($output.Trim())"
        }
    }
    Set-Status 'Đang quét lại thiết bị (scan-devices)...'
    & $script:PnpUtil /scan-devices 2>&1 | Out-Null
    Write-DriverLog "=== RESTORE xong: thành công $ok, lỗi $fail; đã chạy /scan-devices"
    Set-Status "Restore xong: thành công $ok, lỗi $fail."
    [System.Windows.Forms.MessageBox]::Show(
        "Restore hoàn tất.`r`nThành công: $ok`r`nLỗi: $fail`r`n`r`nChi tiết xem DriverTool_Log.txt",
        'Restore Driver', 'OK', 'Information') | Out-Null
})

# --- Xuất HTML: ghi DanhSachDriver.html cạnh script rồi mở lên ---
$btnHtml.Add_Click({
    $rows = New-Object System.Text.StringBuilder
    $i = 0
    foreach ($item in $list.Items) {
        $d = $item.Tag
        $i++
        $enc = {
            param($s) [System.Net.WebUtility]::HtmlEncode([string]$s)
        }
        [void]$rows.AppendLine(
            '<tr><td>{0}</td><td>{1}</td><td>{2}</td><td>{3}</td><td>{4}</td><td>{5}</td><td>{6}</td></tr>' -f `
            $i, (& $enc $d.DeviceName), (& $enc $d.Manufacturer), (& $enc $d.Category), `
            (& $enc $d.DriverVersion), (& $enc $d.DriverDate), (& $enc $d.InfName))
    }
    $html = @"
<!DOCTYPE html>
<html lang="vi">
<head>
<meta charset="utf-8">
<title>Danh sách Driver</title>
<style>
  body { font-family: 'Segoe UI', Arial, sans-serif; margin: 24px; }
  table { border-collapse: collapse; width: 100%; }
  th, td { border: 1px solid #999; padding: 6px 8px; font-size: 13px; text-align: left; }
  th { background: #e8e8e8; }
</style>
</head>
<body>
<h2>Danh sách Driver</h2>
<p>Máy: $env:COMPUTERNAME - Xuất lúc: $(Get-Date -Format 'dd/MM/yyyy HH:mm:ss')</p>
<table>
<tr><th>STT</th><th>Thiết bị</th><th>Hãng sản xuất</th><th>Nhóm</th><th>Phiên bản</th><th>Ngày</th><th>File INF</th></tr>
$rows
</table>
</body>
</html>
"@
    $htmlPath = Join-Path $script:ScriptDir 'DanhSachDriver.html'
    try {
        Set-Content -LiteralPath $htmlPath -Value $html -Encoding UTF8 -ErrorAction Stop
        Write-DriverLog "Đã xuất HTML: $htmlPath ($i driver)"
        Start-Process -FilePath $htmlPath
    } catch {
        [System.Windows.Forms.MessageBox]::Show(
            "Không ghi được file HTML.`r`n$($_.Exception.Message)",
            'Xuất HTML', 'OK', 'Error') | Out-Null
    }
})

# ---------------------------------------------------------------------------
# Khởi động: điền thư mục mặc định và tự quét driver ngay khi mở
# ---------------------------------------------------------------------------
$txtFolder.Text = Get-DefaultBackupFolder
Write-DriverLog '--- Khoi dong Backup/Restore Driver ---'
$form.Add_Shown({ Invoke-DriverScan })
[void]$form.ShowDialog()
