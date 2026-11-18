param(
    [switch]$UpdatedLauncher,
    [switch]$UpdatedInstallModules,
    [string]$OutputName = 'drop_v0.7.19_single'
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$revision = '232f4db'
$stage = Join-Path $repo ('rust\target\single-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $stage | Out-Null
git -C $repo archive --format=zip --output="$stage\source.zip" $revision
if ($LASTEXITCODE) { throw 'git archive failed' }
Expand-Archive -LiteralPath "$stage\source.zip" -DestinationPath "$stage\source"
if ($UpdatedLauncher) {
    # Keep all installation modules pinned to the field-tested Git revision.
    Copy-Item -LiteralPath "$repo\rust\launcher\src" -Destination "$stage\source\rust\launcher" -Recurse -Force
    Copy-Item -LiteralPath "$repo\rust\launcher\assets" -Destination "$stage\source\rust\launcher" -Recurse -Force
    Copy-Item -LiteralPath "$repo\rust\launcher\Cargo.toml" -Destination "$stage\source\rust\launcher\Cargo.toml" -Force
}
if ($UpdatedInstallModules) {
    foreach ($crate in @('qsu_common','cai_dat_win','cai_dat_win_winpe','winpe_install')) {
        Copy-Item -LiteralPath "$repo\rust\$crate\src" -Destination "$stage\source\rust\$crate" -Recurse -Force
    }
}
# Git archive retains old timestamps; refresh inputs so Cargo cannot reuse a
# newer binary built from uncommitted working-tree sources in the shared target.
Get-ChildItem -LiteralPath "$stage\source\rust" -Recurse -File | ForEach-Object { $_.LastWriteTimeUtc = [DateTime]::UtcNow }
$previousFlags = $env:RUSTFLAGS
try {
    $env:RUSTFLAGS = '-C target-feature=+crt-static'
    cargo +stable-x86_64-pc-windows-msvc build --manifest-path "$stage\source\rust\Cargo.toml" --workspace --release --target x86_64-pc-windows-msvc --offline --target-dir "$stage\build"
    if ($LASTEXITCODE) { throw 'Workspace build failed' }
} finally { $env:RUSTFLAGS = $previousFlags }
$release = Join-Path $stage 'build\x86_64-pc-windows-msvc\release'
$entries = @('static PAYLOAD: &[(&str, &[u8])] = &[')
$binaries = @('launcher','BackupRestoreDriver','CaiDatWinTuDong','CaiDatWinWinPE','DonRacMayTinh','PhimTatBIOS','QuanLyUpdateWindows','SaoLuuWifi','TatBitlocker','TuyChinhWindows','WinPEInstall')
foreach ($name in $binaries) {
    $destination = if ($name -eq 'launcher') { 'launcher.exe' } elseif ($name -eq 'WinPEInstall') { 'modules/install/WinPEInstall.exe' } else { "modules/$name.exe" }
    $source = (Join-Path $release "$name.exe").Replace('\','/')
    $entries += "(`"$destination`", include_bytes!(r#`"$source`"#)),"
}
$entries += '];'
$entries | Set-Content -LiteralPath "$stage\payload.rs" -Encoding UTF8
Copy-Item -LiteralPath "$PSScriptRoot\single.rs" -Destination $stage
$output = Join-Path $repo $OutputName
New-Item -ItemType Directory -Path $output -Force | Out-Null
rustc +stable-x86_64-pc-windows-msvc --edition=2021 --target x86_64-pc-windows-msvc -C opt-level=2 -C target-feature=+crt-static "$stage\single.rs" -o "$output\UtilityRebuild.exe"
if ($LASTEXITCODE) { throw 'Single executable build failed' }
$verify = Join-Path $stage 'verify'
$process = Start-Process -FilePath "$output\UtilityRebuild.exe" -ArgumentList @('--extract-to', ('"' + $verify + '"')) -WindowStyle Hidden -PassThru -Wait
if ($process.ExitCode) { throw 'Extraction verification failed' }
foreach ($name in $binaries) {
    $relative = if ($name -eq 'launcher') { 'launcher.exe' } elseif ($name -eq 'WinPEInstall') { 'modules\install\WinPEInstall.exe' } else { "modules\$name.exe" }
    if ((Get-FileHash -LiteralPath "$verify\$relative").Hash -ne (Get-FileHash -LiteralPath "$release\$name.exe").Hash) { throw "Payload mismatch: $name" }
}
Get-Item -LiteralPath "$output\UtilityRebuild.exe" | Select-Object FullName,Length
Get-FileHash -LiteralPath "$output\UtilityRebuild.exe" -Algorithm SHA256
