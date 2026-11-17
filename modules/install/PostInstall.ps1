#Requires -Version 5.1
<#
.SYNOPSIS
    Agent hậu cài — chạy một lần ở lần đăng nhập đầu tiên sau khi cài Windows.

.DESCRIPTION
    File này được module CaiDatWinTuDong.ps1 cài sẵn vào bản Windows mới
    tại C:\ProgramData\QSU\PostInstall.ps1 và đăng ký chạy qua khóa
    RunOnce (HKLM\...\RunOnce, mục "QSU-PostInstall"). Nhiệm vụ:

      1. Cài driver còn thiếu từ bản sao lưu driver trước khi cài
         (pnputil /add-driver từng file *.inf + /scan-devices).
      2. Nếu kỹ thuật viên có chọn, áp profile của module Tuỳ Chỉnh
         Windows (nếu module đó đã được build và nằm trong thư mục
         staging); chưa có thì ghi log bỏ qua.
      3. Dọn dẹp: xóa thư mục staging trên ổ Data, xóa boot entry tạm
         (BCD) của WinPE staging nếu còn, xóa bản sao unattend.xml trong
         Panther (file này chứa mật khẩu đăng nhập lần đầu).
      4. Viết báo cáo hoàn tất cạnh file log trên ổ Data và tự xóa
         chính agent (thư mục C:\ProgramData\QSU).

    Mật khẩu không bao giờ được ghi vào log. Agent không chứa cơ chế
    kiểm tra bản quyền, không gửi dữ liệu đi đâu.
#>

# ---------------------------------------------------------------------------
# Tự nâng quyền Administrator nếu cần (RunOnce chạy bằng token người dùng;
# tài khoản đăng nhập lần đầu là admin nhưng có thể bị UAC giới hạn).
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

$script:AgentDir = $PSScriptRoot
if (-not $script:AgentDir) { $script:AgentDir = 'C:\ProgramData\QSU' }
$script:LogFile  = $null
$script:Config   = $null

# ---------------------------------------------------------------------------
# Tìm ổ Data bằng file marker .qsu_data_drive (không dựa vào ký tự ổ đĩa
# vì ký tự có thể khác so với lúc chạy wizard).
# ---------------------------------------------------------------------------
function Find-DataDrive {
    foreach ($drv in [System.IO.DriveInfo]::GetDrives()) {
        if ($drv.DriveType -ne 'Fixed') { continue }
        $root = $drv.RootDirectory.FullName
        if (Test-Path -LiteralPath (Join-Path $root '.qsu_data_drive')) {
            return $root
        }
    }
    return $null
}

# ---------------------------------------------------------------------------
# Ghi log: vào file log chính trên ổ Data (tên file lấy từ config).
# ---------------------------------------------------------------------------
function Write-AgentLog {
    param([Parameter(Mandatory)][string]$Message)
    $line = '[{0}] [Hậu cài] {1}' -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $Message
    Write-Host $line
    if ($script:LogFile) {
        try { Add-Content -LiteralPath $script:LogFile -Value $line -Encoding UTF8 -ErrorAction Stop } catch { }
    }
}

# ---------------------------------------------------------------------------
# Nạp cấu hình cài đặt: ưu tiên bản sao cạnh agent (ProgramData\QSU),
# không có thì tìm trong thư mục staging trên ổ Data.
# ---------------------------------------------------------------------------
function Read-InstallConfig {
    param([string]$DataRoot)
    $candidates = @()
    $candidates += (Join-Path $script:AgentDir 'install-config.json')
    if ($DataRoot) {
        $candidates += (Join-Path $DataRoot 'QSU-Stage\install-config.json')
    }
    foreach ($path in $candidates) {
        if (Test-Path -LiteralPath $path) {
            try {
                return (Get-Content -LiteralPath $path -Raw -Encoding UTF8 | ConvertFrom-Json)
            } catch { }
        }
    }
    return $null
}

# ===========================================================================
# CHẠY CHÍNH
# ===========================================================================
$dataRoot = Find-DataDrive
$script:Config = Read-InstallConfig -DataRoot $dataRoot

$logsFolderName = 'QSU-Logs'
if ($script:Config -and $script:Config.LogsFolder) { $logsFolderName = [string]$script:Config.LogsFolder }

if ($dataRoot) {
    $logDir = Join-Path $dataRoot $logsFolderName
    if (-not (Test-Path -LiteralPath $logDir)) {
        New-Item -ItemType Directory -Path $logDir -Force | Out-Null
    }
    if ($script:Config -and $script:Config.LogFileName) {
        $script:LogFile = Join-Path $logDir ([string]$script:Config.LogFileName)
    } else {
        $script:LogFile = Join-Path $logDir ('postinstall-{0}.log' -f (Get-Date -Format 'yyyyMMdd-HHmm'))
    }
}

Write-AgentLog '=== Agent hậu cài bắt đầu ==='
if (-not $dataRoot) {
    Write-AgentLog 'CẢNH BÁO: không tìm thấy ổ Data (thiếu marker .qsu_data_drive). Một số bước sẽ bị bỏ qua.'
}
if (-not $script:Config) {
    Write-AgentLog 'CẢNH BÁO: không đọc được install-config.json. Tiếp tục với giá trị mặc định.'
}

$mode = 'Clean'
if ($script:Config -and $script:Config.Mode) { $mode = [string]$script:Config.Mode }
Write-AgentLog "Chế độ cài đặt: $mode"

$driverNote  = 'Không có bản sao lưu driver.'
$tweaksNote  = 'Không được chọn.'

# ---------------------------------------------------------------------------
# 1) Cài driver từ bản sao lưu trước khi cài
# ---------------------------------------------------------------------------
$backupRel = $null
if ($script:Config -and $script:Config.DriverBackupRelative) {
    $backupRel = [string]$script:Config.DriverBackupRelative
}
if ($dataRoot -and $backupRel) {
    $backupDir = Join-Path $dataRoot $backupRel
    if (Test-Path -LiteralPath $backupDir) {
        # pnputil: tiến trình 32-bit trên Windows 64-bit phải dùng bản SysNative
        if ([Environment]::Is64BitOperatingSystem -and -not [Environment]::Is64BitProcess) {
            $pnpUtil = Join-Path $env:WINDIR 'SysNative\pnputil.exe'
        } else {
            $pnpUtil = Join-Path $env:WINDIR 'System32\pnputil.exe'
        }
        $infs = @(Get-ChildItem -LiteralPath $backupDir -Recurse -Filter '*.inf' `
                    -File -ErrorAction SilentlyContinue)
        Write-AgentLog "Cài driver từ bản sao lưu: $($infs.Count) file .inf trong $backupDir"
        $driverOk = 0; $driverFail = 0
        foreach ($inf in $infs) {
            $output = & $pnpUtil /add-driver $inf.FullName /install 2>&1 | Out-String
            $code = $LASTEXITCODE
            # 0 = thành công, 259 = không cần thao tác, 3010 = cần khởi động lại
            if ($code -in @(0, 259, 3010)) {
                $driverOk++
            } else {
                $driverFail++
                Write-AgentLog "LỖI driver $($inf.Name) (exit $code): $($output.Trim())"
            }
        }
        & $pnpUtil /scan-devices 2>&1 | Out-Null
        $driverNote = "Đã cài $driverOk driver, lỗi $driverFail."
        Write-AgentLog "Cài driver xong: $driverNote"
    } else {
        $driverNote = "Không tìm thấy thư mục sao lưu driver: $backupDir"
        Write-AgentLog $driverNote
    }
}

# ---------------------------------------------------------------------------
# 2) Áp profile Tuỳ Chỉnh Windows (nếu được chọn và module đã có)
# ---------------------------------------------------------------------------
$applyTweaks = $false
if ($script:Config -and $script:Config.ApplyTweaks) { $applyTweaks = [bool]$script:Config.ApplyTweaks }
if ($applyTweaks) {
    $tweakScript = $null
    if ($dataRoot -and $script:Config -and $script:Config.StagingRelative) {
        $cand = Join-Path $dataRoot ("{0}\TuyChinhWindows.ps1" -f [string]$script:Config.StagingRelative)
        if (Test-Path -LiteralPath $cand) { $tweakScript = $cand }
    }
    if ($tweakScript) {
        try {
            Write-AgentLog "Áp profile Tuỳ Chỉnh Windows: $tweakScript"
            & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $tweakScript
            $tweaksNote = 'Đã chạy module Tuỳ Chỉnh Windows.'
        } catch {
            $tweaksNote = "Lỗi khi chạy Tuỳ Chỉnh Windows: $($_.Exception.Message)"
            Write-AgentLog $tweaksNote
        }
    } else {
        $tweaksNote = 'Module Tuỳ Chỉnh Windows chưa được xây dựng — bỏ qua bước áp profile.'
        Write-AgentLog $tweaksNote
    }
}

# ---------------------------------------------------------------------------
# 3) Dọn dẹp
# ---------------------------------------------------------------------------
# 3a. Xóa marker đích OS còn sót lại trên ổ hệ thống (nếu có).
$targetMarker = Join-Path $env:SystemDrive '.qsu_target_os'
if (Test-Path -LiteralPath $targetMarker) {
    Remove-Item -LiteralPath $targetMarker -Force -ErrorAction SilentlyContinue
}

# 3b. Xóa bản sao unattend trong Panther — file này chứa mật khẩu tài
#     khoản admin lần đầu ở dạng rõ, phải xóa sau khi cài xong.
foreach ($pantherCopy in @(
    (Join-Path $env:WINDIR 'Panther\unattend.xml'),
    (Join-Path $env:WINDIR 'Panther\Unattend\unattend.xml')
)) {
    if (Test-Path -LiteralPath $pantherCopy) {
        Remove-Item -LiteralPath $pantherCopy -Force -ErrorAction SilentlyContinue
        Write-AgentLog "Đã xóa bản sao unattend: $pantherCopy"
    }
}

# 3c. Xóa boot entry tạm của WinPE staging (BCD) nếu còn sót
if ($script:Config) {
    foreach ($guidProp in @('BootLoaderGuid', 'RamdiskGuid')) {
        $guid = $script:Config.$guidProp
        if ($guid) {
            try {
                & bcdedit.exe /delete $guid /f 2>&1 | Out-Null
                Write-AgentLog "Đã xóa boot entry tạm $guidProp ($guid)."
            } catch { }
        }
    }
}

# 3d. Xóa thư mục staging trên ổ Data (log nằm ở thư mục riêng QSU-Logs
#     nên không bị ảnh hưởng)
if ($dataRoot -and $script:Config -and $script:Config.StagingRelative) {
    $stagingDir = Join-Path $dataRoot ([string]$script:Config.StagingRelative)
    if (Test-Path -LiteralPath $stagingDir) {
        try {
            Remove-Item -LiteralPath $stagingDir -Recurse -Force -ErrorAction Stop
            Write-AgentLog "Đã xóa thư mục staging: $stagingDir"
        } catch {
            Write-AgentLog "Không xóa hết được staging $stagingDir : $($_.Exception.Message)"
        }
    }
}

# 3e. Gỡ mục RunOnce của chính agent (Windows thường tự xóa sau khi chạy,
#     nhưng nếu có lần nâng quyền lại thì mục này có thể còn)
try {
    Remove-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce' `
        -Name 'QSU-PostInstall' -ErrorAction SilentlyContinue
} catch { }

# ---------------------------------------------------------------------------
# 4) Báo cáo hoàn tất cạnh file log trên ổ Data
# ---------------------------------------------------------------------------
$osReg = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' `
            -ErrorAction SilentlyContinue
$startedAt   = ''; if ($script:Config -and $script:Config.StartedAt)   { $startedAt   = [string]$script:Config.StartedAt }
$imageName   = ''; if ($script:Config -and $script:Config.ImageName)   { $imageName   = [string]$script:Config.ImageName }
$editionName = ''; if ($script:Config -and $script:Config.EditionName) { $editionName = [string]$script:Config.EditionName }

$reportLines = @(
    '=== BÁO CÁO HOÀN TẤT CÀI ĐẶT WINDOWS ==='
    "Máy tính        : $env:COMPUTERNAME"
    "Windows đã cài  : $($osReg.ProductName) $($osReg.DisplayVersion) (build $($osReg.CurrentBuildNumber))"
    "Edition         : $($osReg.EditionID)"
    "Chế độ cài      : $mode"
    "Ảnh cài đặt     : $imageName"
    "Phiên bản chọn  : $editionName"
    "Bắt đầu lúc     : $startedAt"
    "Hoàn tất lúc    : $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
    "Driver          : $driverNote"
    "Tuỳ chỉnh Win   : $tweaksNote"
    "Log chi tiết    : $script:LogFile"
)
if ($dataRoot) {
    $reportPath = Join-Path (Join-Path $dataRoot $logsFolderName) `
        ('install-report-{0}.txt' -f (Get-Date -Format 'yyyyMMdd-HHmm'))
    try {
        Set-Content -LiteralPath $reportPath -Value $reportLines -Encoding UTF8 -ErrorAction Stop
        Write-AgentLog "Đã viết báo cáo hoàn tất: $reportPath"
    } catch {
        Write-AgentLog "Không viết được báo cáo: $($_.Exception.Message)"
    }
}
Write-AgentLog '=== Agent hậu cài kết thúc ==='

# ---------------------------------------------------------------------------
# 5) Tự xóa agent: xóa thư mục C:\ProgramData\QSU bằng một tiến trình nền
#    (script đang chạy nằm trong chính thư mục này nên phải xóa trễ).
# ---------------------------------------------------------------------------
$cleanupAgent = $true
if ($script:Config -and $null -ne $script:Config.CleanupAgent) {
    $cleanupAgent = [bool]$script:Config.CleanupAgent
}
if ($cleanupAgent -and $script:AgentDir -like '*\ProgramData\QSU') {
    try {
        $cmd = 'ping -n 4 127.0.0.1 >nul & rd /s /q "{0}"' -f $script:AgentDir
        Start-Process -FilePath 'cmd.exe' -ArgumentList "/c $cmd" -WindowStyle Hidden
    } catch { }
}
