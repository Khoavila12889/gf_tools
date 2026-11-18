$ErrorActionPreference='Stop'
. (Join-Path (Split-Path $PSScriptRoot -Parent) 'rust\qsu_common\src\telegram.ps1')
$script:Root=Join-Path $env:TEMP ('qsu-telegram-test-'+[Guid]::NewGuid())
New-Item -ItemType Directory -Path $script:Root | Out-Null
$script:logs=@();$script:calls=0
function Write-SetupLog($Message){$script:logs+=$Message}
function Get-ItemProperty {return @{CurrentBuild='test'}}
function Assert($condition,$message){if(!$condition){throw $message}}
$fakeToken='123456:fake_test_credential'
@{bot_token=$fakeToken;chat_id='123456'} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $script:Root 'Telegram.json')
$results=@(@{app='UltraViewer';status='installed'},@{app='Chrome';status='installed'},@{app='7-Zip';status='installed'})
$outbox=Join-Path $script:Root 'Telegram-outbox.json'
try {
    function Invoke-RestMethod {
        param($Uri,$Method,$ContentType,$Body,$TimeoutSec,$ErrorAction)
        $script:calls++
        $script:body=[Text.Encoding]::UTF8.GetString($Body)
        return @{ok=$true;result=@{message_id=42}}
    }
    Assert (Send-SetupNotification $results) 'Successful delivery not acknowledged'
    Assert (Send-SetupNotification $results) 'Saved delivery not recognized'
    Assert ($script:calls -eq 1) 'Duplicate message after successful response'
    Assert (!$script:body.Contains($fakeToken)) 'Token leaked into message payload'
    $saved=Get-Content -LiteralPath $outbox -Raw | ConvertFrom-Json
    Assert ($saved.status -eq 'delivered' -and $saved.message_id -eq 42) 'Receipt not persisted'
    Remove-Item -LiteralPath $outbox
    function Invoke-RestMethod {throw 'Network error with https://api.telegram.org/bot123456:fake_test_credential/sendMessage'}
    Assert (!(Send-SetupNotification $results)) 'Network failure incorrectly marked delivered'
    Assert (!(($script:logs -join ' ').Contains($fakeToken))) 'Credential leaked through exception logging'
    $saved=Get-Content -LiteralPath $outbox -Raw | ConvertFrom-Json
    Assert ($saved.status -eq 'pending' -and $saved.next_utc) 'Network retry not persisted'
    Remove-Item -LiteralPath $outbox
    function Invoke-RestMethod {return @{ok=$false;error_code=429;parameters=@{retry_after=1200}}}
    $null=Send-SetupNotification $results
    $saved=Get-Content -LiteralPath $outbox -Raw | ConvertFrom-Json
    Assert (([DateTime]::Parse($saved.next_utc)-[DateTime]::UtcNow).TotalSeconds -gt 1190) 'Telegram retry_after ignored'
    Remove-Item -LiteralPath $outbox
    function Invoke-RestMethod {return @{ok=$false;error_code=403}}
    Assert (Send-SetupNotification $results) 'Invalid destination should stop blind retries'
    $saved=Get-Content -LiteralPath $outbox -Raw | ConvertFrom-Json
    Assert ($saved.status -eq 'configuration_error') 'Invalid destination not recorded'
    Write-Output 'PASS: receipt/dedup, transport failure, token redaction, rate limit, invalid chat. No network calls.'
} finally {
    Get-ChildItem -LiteralPath $script:Root -File | Remove-Item -Force
    Remove-Item -LiteralPath $script:Root
}
