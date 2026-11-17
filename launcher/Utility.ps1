#Requires -Version 5.1
<#
.SYNOPSIS
    Utility Rebuild - Cửa sổ chính (launcher).

.DESCRIPTION
    Hiển thị lưới 32 nút chức năng tiếng Việt và ô tìm kiếm lọc nút
    (không phân biệt dấu tiếng Việt). Mỗi nút tương ứng một module
    độc lập trong thư mục ..\modules. Khi bấm nút, module được chạy
    bằng quyền Administrator (Start-Process -Verb RunAs); nếu module
    chưa được xây dựng, chương trình hiện thông báo thay vì báo lỗi.

    Đây là mã nguồn viết mới hoàn toàn (clean-room). Launcher không
    chứa cơ chế kiểm tra bản quyền, không gửi dữ liệu đi đâu, không
    tự ý thay đổi cấu hình bảo mật của máy.
#>

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
[System.Windows.Forms.Application]::EnableVisualStyles()

# ---------------------------------------------------------------------------
# Danh sách 32 chức năng: Nhãn hiển thị -> file module trong thư mục modules\
# Thứ tự giữ đúng thứ tự lưới nút của công cụ gốc.
# ---------------------------------------------------------------------------
$script:ModuleList = @(
    @{ Label = 'Kho Driver IRST';          File = 'KhoDriverIRST.ps1' }
    @{ Label = 'Kho Windows';              File = 'KhoWindows.ps1' }
    @{ Label = 'Backup License MS';        File = 'BackupLicenseMS.ps1' }
    @{ Label = 'Backup/Restore Driver';    File = 'BackupRestoreDriver.ps1' }
    @{ Label = 'Tắt Bitlocker';            File = 'TatBitlocker.ps1' }
    @{ Label = 'Tạo Boot WinPE';           File = 'TaoBootWinPE.ps1' }
    @{ Label = 'Cài Đặt Win Tự Động';      File = 'CaiDatWinTuDong.ps1' }
    @{ Label = 'Cài Đặt Win=WinPE';        File = 'CaiDatWinWinPE.ps1' }
    @{ Label = 'Cài Đặt Office Online';    File = 'CaiDatOfficeOnline.ps1' }
    @{ Label = 'Cài Đặt Office OFFLINE';   File = 'CaiDatOfficeOffline.ps1' }
    @{ Label = 'Kích Hoạt Win/Office';     File = 'KichHoatWinOffice.ps1' }
    @{ Label = 'Cài Đặt Phần Mềm';         File = 'CaiDatPhanMem.ps1' }
    @{ Label = 'Sao Lưu Dữ Liệu';          File = 'SaoLuuDuLieu.ps1' }
    @{ Label = 'Sao Lưu/Quản lý Wifi';     File = 'SaoLuuWifi.ps1' }
    @{ Label = 'Sao Lưu Chrome-Edge';      File = 'SaoLuuChromeEdge.ps1' }
    @{ Label = 'Backup/Restore Zalo';      File = 'BackupRestoreZalo.ps1' }
    @{ Label = 'Check Cấu Hình Máy Tính';  File = 'CheckCauHinh.ps1' }
    @{ Label = 'Check / Test';             File = 'CheckTest.ps1' }
    @{ Label = 'Phím Tắt BIOS';            File = 'PhimTatBIOS.ps1' }
    @{ Label = 'Phân Vùng Ổ Cứng';         File = 'PhanVungOCung.ps1' }
    @{ Label = 'Tuỳ Chỉnh Windows';        File = 'TuyChinhWindows.ps1' }
    @{ Label = 'Tuỳ Chỉnh Office';         File = 'TuyChinhOffice.ps1' }
    @{ Label = 'Windows Security';         File = 'WindowsSecurity.ps1' }
    @{ Label = 'Quản Lý Update Windows';   File = 'QuanLyUpdateWindows.ps1' }
    @{ Label = 'Dọn Rác Máy Tính';         File = 'DonRacMayTinh.ps1' }
    @{ Label = 'Gỡ Bỏ Phần Mềm';           File = 'GoBoPhanMem.ps1' }
    @{ Label = 'Fix Lỗi Máy In';           File = 'FixLoiMayIn.ps1' }
    @{ Label = 'Quản Lý Máy In';           File = 'QuanLyMayIn.ps1' }
    @{ Label = 'Fix Share Mạng LAN';       File = 'FixShareMangLAN.ps1' }
    @{ Label = 'Quản lý File/Folder';      File = 'QuanLyFileFolder.ps1' }
    @{ Label = 'Chuyển Windows OS';        File = 'ChuyenWindowsOS.ps1' }
    @{ Label = 'Multi Zalo PC';            File = 'MultiZaloPC.ps1' }
)

# Thư mục modules\ nằm cạnh thư mục launcher\
$script:RootDir    = Split-Path -Parent $PSScriptRoot
$script:ModulesDir = Join-Path $script:RootDir 'modules'

# ---------------------------------------------------------------------------
# Hàm tiện ích
# ---------------------------------------------------------------------------

# Bỏ dấu tiếng Việt để tìm kiếm không phân biệt dấu ("tuy chinh" ~ "Tuỳ Chỉnh")
function ConvertTo-Unaccented {
    param([AllowEmptyString()][string]$Text)
    if ([string]::IsNullOrEmpty($Text)) { return '' }
    $normalized = $Text.Normalize([System.Text.NormalizationForm]::FormD)
    $sb = New-Object System.Text.StringBuilder
    foreach ($ch in $normalized.ToCharArray()) {
        $cat = [System.Globalization.CharUnicodeInfo]::GetUnicodeCategory($ch)
        if ($cat -ne [System.Globalization.UnicodeCategory]::NonSpacingMark) {
            [void]$sb.Append($ch)
        }
    }
    # 'đ'/'Đ' không tách dấu qua FormD nên xử lý riêng
    return ($sb.ToString().Replace('đ', 'd').Replace('Đ', 'D'))
}

# Chuỗi thông tin hệ điều hành hiển thị ở header, ví dụ:
# "Windows 10 Pro 22H2 (64-bit)"
function Get-OsCaptionText {
    $caption = ''
    try {
        $os = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop
        $caption = $os.Caption -replace '^Microsoft\s+', ''
    } catch {
        $caption = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' `
                    -ErrorAction SilentlyContinue).ProductName
    }
    $ver = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' `
            -ErrorAction SilentlyContinue).DisplayVersion
    if (-not $ver) {
        $ver = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' `
                -ErrorAction SilentlyContinue).ReleaseId
    }
    $bits = if ([Environment]::Is64BitOperatingSystem) { '64-bit' } else { '32-bit' }
    return ("{0} {1} ({2})" -f $caption, $ver, $bits).Trim()
}

# Chạy một module với quyền Administrator; báo rõ nếu module chưa được build
function Start-UtilityModule {
    param([Parameter(Mandatory)][hashtable]$Module)

    $path = Join-Path $script:ModulesDir $Module.File
    if (-not (Test-Path -LiteralPath $path)) {
        [System.Windows.Forms.MessageBox]::Show(
            "Module '$($Module.Label)' chưa được xây dựng trong bản rebuild này.`r`n`r`nFile module dự kiến: modules\$($Module.File)",
            'Chưa có module',
            [System.Windows.Forms.MessageBoxButtons]::OK,
            [System.Windows.Forms.MessageBoxIcon]::Information) | Out-Null
        return
    }
    try {
        $args = "-NoProfile -ExecutionPolicy Bypass -File `"$path`""
        Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList $args
    } catch {
        [System.Windows.Forms.MessageBox]::Show(
            "Không chạy được module '$($Module.Label)'.`r`n$($_.Exception.Message)",
            'Lỗi',
            [System.Windows.Forms.MessageBoxButtons]::OK,
            [System.Windows.Forms.MessageBoxIcon]::Error) | Out-Null
    }
}

# ---------------------------------------------------------------------------
# Giao diện chính
# ---------------------------------------------------------------------------
$colorBack    = [System.Drawing.Color]::FromArgb(32, 32, 36)   # nền tối
$colorHeader  = [System.Drawing.Color]::FromArgb(24, 24, 28)
$colorButton  = [System.Drawing.Color]::FromArgb(52, 56, 66)   # nút chức năng
$colorAccent  = [System.Drawing.Color]::FromArgb(0, 120, 215)  # điểm nhấn
$colorText    = [System.Drawing.Color]::White

$form = New-Object System.Windows.Forms.Form
$form.Text            = 'Utility Rebuild'
$form.Size            = New-Object System.Drawing.Size(920, 690)
$form.MinimumSize     = New-Object System.Drawing.Size(760, 560)
$form.StartPosition   = 'CenterScreen'
$form.BackColor       = $colorBack
$form.Font            = New-Object System.Drawing.Font('Segoe UI', 10)

# --- Header: tiêu đề + thông tin máy ---
$header = New-Object System.Windows.Forms.Panel
$header.Dock      = 'Top'
$header.Height    = 78
$header.BackColor = $colorHeader
$form.Controls.Add($header)

$lblTitle = New-Object System.Windows.Forms.Label
$lblTitle.Text      = 'Utility Rebuild'
$lblTitle.Font      = New-Object System.Drawing.Font('Segoe UI', 17, [System.Drawing.FontStyle]::Bold)
$lblTitle.ForeColor = $colorText
$lblTitle.AutoSize  = $true
$lblTitle.Location  = New-Object System.Drawing.Point(18, 10)
$header.Controls.Add($lblTitle)

$lblSub = New-Object System.Windows.Forms.Label
$lblSub.Text      = 'Bộ công cụ hỗ trợ cài đặt Windows - Software'
$lblSub.Font      = New-Object System.Drawing.Font('Segoe UI', 10)
$lblSub.ForeColor = [System.Drawing.Color]::Gainsboro
$lblSub.AutoSize  = $true
$lblSub.Location  = New-Object System.Drawing.Point(20, 46)
$header.Controls.Add($lblSub)

$lblSys = New-Object System.Windows.Forms.Label
$lblSys.Text      = "System OS : $(Get-OsCaptionText)`r`nPC-Name : $env:COMPUTERNAME   |   User: $env:USERNAME"
$lblSys.Font      = New-Object System.Drawing.Font('Segoe UI', 9.5)
$lblSys.ForeColor = $colorText
$lblSys.AutoSize  = $true
$lblSys.Anchor    = 'Top,Right'
$lblSys.Location  = New-Object System.Drawing.Point(560, 14)
$header.Controls.Add($lblSys)

# --- Thanh tìm kiếm + nút phụ ở cuối cửa sổ ---
$footer = New-Object System.Windows.Forms.Panel
$footer.Dock      = 'Bottom'
$footer.Height    = 52
$footer.BackColor = $colorHeader
$form.Controls.Add($footer)

$lblSearch = New-Object System.Windows.Forms.Label
$lblSearch.Text      = 'Tìm kiếm :'
$lblSearch.Font      = New-Object System.Drawing.Font('Segoe UI', 10.5, [System.Drawing.FontStyle]::Bold)
$lblSearch.ForeColor = $colorText
$lblSearch.AutoSize  = $true
$lblSearch.Location  = New-Object System.Drawing.Point(14, 15)
$footer.Controls.Add($lblSearch)

$txtSearch = New-Object System.Windows.Forms.TextBox
$txtSearch.Font     = New-Object System.Drawing.Font('Segoe UI', 11)
$txtSearch.Location = New-Object System.Drawing.Point(100, 12)
$txtSearch.Size     = New-Object System.Drawing.Size(240, 28)
$footer.Controls.Add($txtSearch)

$btnClear = New-Object System.Windows.Forms.Button
$btnClear.Text      = 'Xoá'
$btnClear.Font      = New-Object System.Drawing.Font('Segoe UI', 10, [System.Drawing.FontStyle]::Bold)
$btnClear.ForeColor = $colorText
$btnClear.BackColor = $colorButton
$btnClear.FlatStyle = 'Flat'
$btnClear.Location  = New-Object System.Drawing.Point(350, 11)
$btnClear.Size      = New-Object System.Drawing.Size(70, 30)
$footer.Controls.Add($btnClear)

$btnAbout = New-Object System.Windows.Forms.Button
$btnAbout.Text      = 'Giới thiệu'
$btnAbout.Font      = New-Object System.Drawing.Font('Segoe UI', 10, [System.Drawing.FontStyle]::Bold)
$btnAbout.ForeColor = $colorText
$btnAbout.BackColor = $colorButton
$btnAbout.FlatStyle = 'Flat'
$btnAbout.Anchor    = 'Top,Right'
$btnAbout.Location  = New-Object System.Drawing.Point(786, 11)
$btnAbout.Size      = New-Object System.Drawing.Size(110, 30)
$footer.Controls.Add($btnAbout)

# --- Lưới nút chức năng: 4 cột x 8 hàng ---
$grid = New-Object System.Windows.Forms.TableLayoutPanel
$grid.Dock        = 'Fill'
$grid.ColumnCount = 4
$grid.RowCount    = 8
$grid.Padding     = New-Object System.Windows.Forms.Padding(14)
for ($c = 0; $c -lt 4; $c++) {
    [void]$grid.ColumnStyles.Add((New-Object System.Windows.Forms.ColumnStyle('Percent', 25)))
}
for ($r = 0; $r -lt 8; $r++) {
    [void]$grid.RowStyles.Add((New-Object System.Windows.Forms.RowStyle('Percent', 12.5)))
}
$form.Controls.Add($grid)
$grid.BringToFront()

$script:Buttons = @()
foreach ($module in $script:ModuleList) {
    $btn = New-Object System.Windows.Forms.Button
    $btn.Text      = $module.Label
    $btn.Tag       = $module
    $btn.Dock      = 'Fill'
    $btn.Margin    = New-Object System.Windows.Forms.Padding(6)
    $btn.Font      = New-Object System.Drawing.Font('Segoe UI', 10, [System.Drawing.FontStyle]::Bold)
    $btn.ForeColor = $colorText
    $btn.BackColor = $colorButton
    $btn.FlatStyle = 'Flat'
    $btn.FlatAppearance.BorderSize = 0
    $btn.Cursor    = 'Hand'
    $btn.Add_Click({
        param($sender, $e)
        Start-UtilityModule -Module $sender.Tag
    })
    $grid.Controls.Add($btn)
    $script:Buttons += $btn
}

# --- Lọc nút theo ô tìm kiếm (không phân biệt dấu) ---
function Apply-SearchFilter {
    $query = ConvertTo-Unaccented $txtSearch.Text.Trim()
    foreach ($btn in $script:Buttons) {
        if ($query -eq '') {
            $btn.Visible = $true
        } else {
            $label = ConvertTo-Unaccented $btn.Tag.Label
            $btn.Visible = $label.ToLower().Contains($query.ToLower())
        }
    }
}
$txtSearch.Add_TextChanged({ Apply-SearchFilter })
$btnClear.Add_Click({
    $txtSearch.Clear()
    $txtSearch.Focus()
})
$btnAbout.Add_Click({
    [System.Windows.Forms.MessageBox]::Show(
        "Utility Rebuild`r`nBộ công cụ hỗ trợ cài đặt Windows - Software.`r`n`r`nBản dựng lại độc lập (clean-room) bằng PowerShell,`r`ntương đương chức năng với công cụ gốc.",
        'Giới thiệu',
        [System.Windows.Forms.MessageBoxButtons]::OK,
        [System.Windows.Forms.MessageBoxIcon]::Information) | Out-Null
})

$txtSearch.Focus()
[void]$form.ShowDialog()
