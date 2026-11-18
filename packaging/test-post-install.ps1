$ErrorActionPreference='Stop'
$worker=Join-Path (Split-Path $PSScriptRoot -Parent) 'rust\qsu_common\src\post_install.ps1'
$tokens=$null;$errors=$null
[System.Management.Automation.Language.Parser]::ParseFile($worker,[ref]$tokens,[ref]$errors) | Out-Null
if($errors.Count){throw ($errors | Out-String)}
. $worker -LibraryOnly
function Assert($condition,$message){if(!$condition){throw $message}}
$apps=@(Get-AppCatalogue)
foreach($fixture in @(@{Index=0;Name='qsu-ultraviewer.exe'},@{Index=2;Name='qsu-7z2606-x64.exe'})) {
    $download=Join-Path $env:TEMP $fixture.Name
    if(Test-Path -LiteralPath $download){Assert-Installer $apps[$fixture.Index] $download; Write-Output "Verified official installer: $($apps[$fixture.Index].Id)"}
}
Assert ($apps.Count -eq 3) 'Expected three apps'
Assert ($apps[0].Id -eq 'UltraViewer') 'Remote support must be attempted first'
$fake=Join-Path $env:TEMP ('qsu-invalid-installer-'+[Guid]::NewGuid()+'.exe')
Set-Content -LiteralPath $fake -Value 'not an installer'
try {
    foreach($app in $apps) {
        $rejected=$false
        try {Assert-Installer $app $fake}catch{$rejected=$true}
        Assert $rejected "Invalid installer accepted for $($app.Id)"
    }
} finally {Remove-Item -LiteralPath $fake -Force}
# No installation or download occurs in these tests.
function Write-SetupLog($Message){}
$script:attempted=@()
function Install-App($App){$script:attempted+=$App.Id;if($App.Id -eq 'UltraViewer'){throw 'simulated offline failure'}}
$result=@(Install-AppBatch)
Assert (($script:attempted -join ',') -eq 'UltraViewer,Chrome,7-Zip') 'One app failure prevented later attempts'
Assert ($result[0].status -eq 'failed') 'False success for failed install'
Assert ($result[1].status -eq 'installed' -and $result[2].status -eq 'installed') 'Independent app results lost'
. $worker -LibraryOnly
function Find-InstalledApp($App){return 'C:\Program Files\test.exe'}
function Write-SetupLog($Message){}
function Invoke-WebRequest {throw 'Download should not run for an installed app'}
function Invoke-Installer {throw 'Installer should not run for an installed app'}
Install-App $apps[0]
Write-Output 'PASS: parser, reject corrupt installers, isolate app failures, skip installed applications. No software installed.'
