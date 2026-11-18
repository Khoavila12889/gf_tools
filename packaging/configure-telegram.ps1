param([string]$ChatId)
$ErrorActionPreference='Stop'
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
if(!(New-Object Security.Principal.WindowsPrincipal($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Open elevated PowerShell first'}
if(!$ChatId){$ChatId=Read-Host 'Telegram chat_id (personal chat or group)'}
if($ChatId -notmatch '^-?\d+$'){throw 'Invalid numeric chat_id'}
$secret=Read-Host 'Telegram bot token (hidden)' -AsSecureString
$pointer=[Runtime.InteropServices.Marshal]::SecureStringToBSTR($secret)
try {
    $token=[Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer)
    if($token -notmatch '^\d+:[A-Za-z0-9_-]+$'){throw 'Invalid token format'}
    $directory=Join-Path $env:ProgramData 'QSU'
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
    $path=Join-Path $directory 'Telegram.json'
    if(!(Test-Path -LiteralPath $path)){New-Item -ItemType File -Path $path | Out-Null}
    & icacls.exe $path /inheritance:r /grant:r '*S-1-5-18:F' '*S-1-5-32-544:F' | Out-Null
    if($LASTEXITCODE){throw 'Cannot protect credential file'}
    @{bot_token=$token;chat_id=$ChatId} | ConvertTo-Json | Set-Content -LiteralPath $path -Encoding UTF8
    Write-Output 'Configuration saved for future installations. No Telegram message sent.'
} finally {[Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer);$token=$null;$secret.Dispose()}
