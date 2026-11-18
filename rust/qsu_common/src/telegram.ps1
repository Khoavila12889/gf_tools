# Loaded by PostInstall.ps1. The token exists only in a protected local config.
function Send-SetupNotification($Results) {
    $configFile=Join-Path $script:Root 'Telegram.json'
    if(!(Test-Path -LiteralPath $configFile)){Write-SetupLog 'Telegram not configured';return $true}
    try {
        $config=Get-Content -LiteralPath $configFile -Raw | ConvertFrom-Json
        if($config.bot_token -notmatch '^\d+:[A-Za-z0-9_-]+$' -or [string]$config.chat_id -notmatch '^-?\d+$') {
            Write-SetupLog 'Telegram configuration invalid (token or chat_id)';return $true
        }
    } catch {Write-SetupLog 'Telegram configuration unreadable';return $true}
    $outbox=Join-Path $script:Root 'Telegram-outbox.json'
    $allOk=@($Results | Where-Object {$_.status -ne 'installed'}).Count -eq 0
    $phase=if($allOk){'SETUP_COMPLETED'}else{'SETUP_INCOMPLETE'}
    $event=$null
    if(Test-Path -LiteralPath $outbox){try{$event=Get-Content -LiteralPath $outbox -Raw | ConvertFrom-Json}catch{}}
    if(!$event -or $event.phase -ne $phase) {
        $os=Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction SilentlyContinue
        $lines=@('Utility Rebuild',('Machine: '+$env:COMPUTERNAME),('Windows build: '+$os.CurrentBuild),('Status: '+$phase))
        foreach($id in @('UltraViewer','Chrome','7-Zip')) {
            $row=@($Results | Where-Object {$_.app -eq $id})
            $status=if($row.Count -eq 1 -and $row[0].status -eq 'installed'){'Installed'}else{'Failed - see local log'}
            $lines+=($id+': '+$status)
        }
        $event=[pscustomobject]@{id=[Guid]::NewGuid().ToString();phase=$phase;text=($lines -join "`n");status='pending';attempts=0;next_utc='';message_id=$null}
        $event.text+="`nEvent: "+$event.id
        $event | ConvertTo-Json | Set-Content -LiteralPath $outbox -Encoding UTF8
    }
    if($event.status -in @('delivered','configuration_error')){return $true}
    if($event.attempts -ge 12){Write-SetupLog 'Telegram retry limit reached';return $true}
    if($event.next_utc -and [DateTime]::Parse($event.next_utc).ToUniversalTime() -gt [DateTime]::UtcNow){return $false}
    $event.attempts++
    $response=$null
    try {
        $body=@{chat_id=[string]$config.chat_id;text=$event.text;disable_web_page_preview=$true} | ConvertTo-Json -Compress
        $response=Invoke-RestMethod -Uri ('https://api.telegram.org/bot'+$config.bot_token+'/sendMessage') -Method Post -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes($body)) -TimeoutSec 30 -ErrorAction Stop
    } catch {
        # Never log the exception/URL: Bot API URLs contain the credential.
        try {$response=$_.ErrorDetails.Message | ConvertFrom-Json}catch{}
    }
    if($response -and $response.ok -eq $true -and $response.result.message_id) {
        $event.status='delivered';$event.message_id=$response.result.message_id
        Write-SetupLog ('Telegram delivered, message_id='+$event.message_id)
    } elseif($response -and $response.error_code -in @(400,401,403,404)) {
        $event.status='configuration_error'
        Write-SetupLog 'Telegram token/chat configuration rejected; no blind retries'
    } else {
        $delay=[Math]::Min(3600,60*[Math]::Pow(2,[Math]::Min(6,$event.attempts)))
        if($response -and $response.parameters.retry_after){$delay=[Math]::Max($delay,[int]$response.parameters.retry_after)}
        $event.next_utc=[DateTime]::UtcNow.AddSeconds($delay).ToString('o')
        Write-SetupLog 'Telegram delivery pending; retry scheduled'
    }
    $event | ConvertTo-Json | Set-Content -LiteralPath $outbox -Encoding UTF8
    return $event.status -ne 'pending'
}
