param([switch]$LibraryOnly)
$ErrorActionPreference = 'Stop'
if(Test-Path -LiteralPath (Join-Path $PSScriptRoot 'Telegram.ps1')){. (Join-Path $PSScriptRoot 'Telegram.ps1')}
# The source checkout uses a lowercase filename; staged Windows files use PascalCase.
elseif(Test-Path -LiteralPath (Join-Path $PSScriptRoot 'telegram.ps1')){. (Join-Path $PSScriptRoot 'telegram.ps1')}

function Get-AppCatalogue {
    @(
        @{ Id='UltraViewer'; File='UltraViewer.exe'; Url='https://www.ultraviewer.net/en/UltraViewer_setup_6.6_en.exe'; Publisher='(DucFabulous|Duc Fabulous|UltraViewer)'; Hash=''; Paths=@('UltraViewer\UltraViewer_Desktop.exe','UltraViewer\UltraViewer.exe'); Args=@('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/SP-') },
        @{ Id='Chrome'; File='Chrome.msi'; Url='https://dl.google.com/dl/chrome/install/googlechromestandaloneenterprise64.msi'; Publisher='Google (LLC|Inc)'; Hash=''; Paths=@('Google\Chrome\Application\chrome.exe'); Args=@() },
        @{ Id='7-Zip'; File='7zip.exe'; Url='https://github.com/ip7z/7zip/releases/download/26.06/7z2606-x64.exe'; Publisher=''; Hash='9B697F65ECC2D9B0152F5EE96D72DDE522A7827690349D9307935DACF40F42EC'; Paths=@('7-Zip\7z.exe'); Args=@('/S') }
    )
}
function Write-SetupLog([string]$Message) {
    Add-Content -LiteralPath $script:Log -Value ('{0:u} {1}' -f [DateTime]::UtcNow,$Message) -Encoding UTF8
}
function Find-InstalledApp($App) {
    foreach($root in @($env:ProgramW6432,$env:ProgramFiles,${env:ProgramFiles(x86)})) {
        if(!$root){continue}
        foreach($relative in $App.Paths) {
            $candidate=Join-Path $root $relative
            if(Test-Path -LiteralPath $candidate -PathType Leaf){return $candidate}
        }
    }
    return $null
}
function Assert-Installer($App,[string]$Path) {
    if($App.Hash) {
        if((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ne $App.Hash){throw 'SHA256 mismatch'}
    } else {
        $signature=Get-AuthenticodeSignature -LiteralPath $Path
        if($signature.Status -ne 'Valid' -or !$signature.SignerCertificate -or $signature.SignerCertificate.Subject -notmatch $App.Publisher) {
            throw "Invalid installer signature/publisher for $($App.Id)"
        }
    }
}
function Invoke-Installer([string]$Program,[string[]]$Arguments,[string]$PendingFile) {
    $process=Start-Process -FilePath $Program -ArgumentList $Arguments -WindowStyle Hidden -PassThru
    Set-Content -LiteralPath $PendingFile -Value $process.Id
    if(!$process.WaitForExit(900000)) {
        # Do not kill an active MSI transaction, reboot, or start a second copy.
        throw "Installer still running after 15 minutes (PID $($process.Id)); inspect before retry"
    }
    $process.Refresh()
    Remove-Item -LiteralPath $PendingFile -Force
    return $process.ExitCode
}
function Install-App($App) {
    $installed=Find-InstalledApp $App
    if($installed){Write-SetupLog "$($App.Id): already installed at $installed";return}
    $pending=Join-Path $script:Cache ($App.Id+'.running')
    if(Test-Path -LiteralPath $pending) {
        $processId=[int](Get-Content -LiteralPath $pending -Raw)
        if(Get-Process -Id $processId -ErrorAction SilentlyContinue){throw "Previous installer PID $processId is still running"}
        Remove-Item -LiteralPath $pending -Force
    }
    $target=Join-Path $script:Cache $App.File
    $part=Join-Path $script:Cache ('download-'+$App.File)
    $lastError=''
    for($attempt=1;$attempt -le 3;$attempt++) {
        try {
            Write-SetupLog "$($App.Id): downloading attempt $attempt"
            Invoke-WebRequest -UseBasicParsing -Uri $App.Url -OutFile $part -TimeoutSec 180
            Assert-Installer $App $part
            Move-Item -LiteralPath $part -Destination $target -Force
            $lastError='';break
        } catch { $lastError=$_.Exception.Message;Write-SetupLog "$($App.Id): $lastError";if($attempt -lt 3){Start-Sleep -Seconds 20} }
    }
    if($lastError){throw $lastError}
    $arguments=$App.Args
    $program=$target
    if($App.Id -eq 'Chrome') {
        $program=Join-Path $env:SystemRoot 'System32\msiexec.exe'
        $arguments=@('/i',('"'+$target+'"'),'/qn','/norestart','/L*v',('"'+(Join-Path $script:Root 'Chrome-install.log')+'"'))
    } elseif($App.Id -eq 'UltraViewer') {
        $arguments+=('/LOG="'+(Join-Path $script:Root 'UltraViewer-install.log')+'"')
    }
    $code=Invoke-Installer $program $arguments $pending
    if($code -notin @(0,3010)){throw "Installer exit code $code"}
    if(!(Find-InstalledApp $App)){throw 'Installer returned success but application executable is missing'}
    Write-SetupLog "$($App.Id): installed, exit code $code"
}
function Install-AppBatch {
    $results=@()
    foreach($app in Get-AppCatalogue) {
        try {Install-App $app;$results+=@{app=$app.Id;status='installed'}}
        catch {$results+=@{app=$app.Id;status='failed';error=$_.Exception.Message};Write-SetupLog "$($app.Id): FAILED: $($_.Exception.Message)"}
    }
    return $results
}
function Invoke-PostInstall {
    if(![Environment]::Is64BitOperatingSystem){throw 'Only Windows x64 is supported'}
    $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
    if(!(New-Object Security.Principal.WindowsPrincipal($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Administrator privileges required'}
    $script:Root=Join-Path $env:ProgramData 'QSU'
    $script:Cache=Join-Path $script:Root 'Installers'
    New-Item -ItemType Directory -Path $script:Cache -Force | Out-Null
    $script:Log=Join-Path $script:Root 'PostInstall.log'
    $mutex=New-Object Threading.Mutex($false,'Global\QSU-PostInstall-Apps')
    $owns=$false
    try {
        try {$owns=$mutex.WaitOne(0)}catch [Threading.AbandonedMutexException]{$owns=$true}
        if(!$owns){return}
        Write-SetupLog 'Starting automatic application setup: UltraViewer, Chrome, 7-Zip'
        # FirstLogonCommands is elevated. Retry under this same admin account,
        # only while logged on, every five minutes for one hour and at next logon.
        # A persistent attempt cap prevents an indefinitely repeating task.
        $taskName='QSU-PostInstall-Apps'
        $attemptFile=Join-Path $script:Root 'PostInstall-attempts.txt'
        $attempt=0
        if(Test-Path -LiteralPath $attemptFile){$attempt=[int](Get-Content -LiteralPath $attemptFile -Raw)}
        if($attempt -ge 12){
            Write-SetupLog 'Application retry limit reached; only notification delivery may continue'
            $finished=$true
            try {
                $previous=Get-Content -LiteralPath (Join-Path $script:Root 'PostInstall-status.json') -Raw | ConvertFrom-Json
                $finished=Send-SetupNotification @($previous)
            } catch {Write-SetupLog 'Could not deliver final status; inspect local configuration'}
            if($finished){Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue}
            return
        }
        Set-Content -LiteralPath $attemptFile -Value ($attempt+1)
        try {
            $action=New-ScheduledTaskAction -Execute "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -Argument ('-NoProfile -ExecutionPolicy Bypass -File "'+$PSCommandPath+'"')
            $trigger=@((New-ScheduledTaskTrigger -AtLogOn -User $identity.Name),(New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(5) -RepetitionInterval (New-TimeSpan -Minutes 5) -RepetitionDuration (New-TimeSpan -Hours 1)))
            $principal=New-ScheduledTaskPrincipal -UserId $identity.Name -LogonType Interactive -RunLevel Highest
            $settings=New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Hours 2)
            Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
        } catch {Write-SetupLog "Could not register retry task: $($_.Exception.Message)"}
        [Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12
        $results=@(Install-AppBatch)
        $results | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $script:Root 'PostInstall-status.json') -Encoding UTF8
        if(@($results | Where-Object {$_.status -eq 'failed'}).Count -eq 0) {
            $notified=$false
            try {$notified=Send-SetupNotification $results}catch{Write-SetupLog 'Telegram worker failed; application setup remains complete'}
            if($notified){Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue}
            Write-SetupLog 'All three applications installed. No forced restart.'
        } else {
            Write-SetupLog 'Some applications failed; retry pending. Other completed installations are preserved.'
            if($attempt -ge 11){try {$null=Send-SetupNotification $results}catch{Write-SetupLog 'Telegram final failure notification pending'}}
        }
    } finally {if($owns){$mutex.ReleaseMutex()};$mutex.Dispose()}
}
if(!$LibraryOnly){Invoke-PostInstall}
