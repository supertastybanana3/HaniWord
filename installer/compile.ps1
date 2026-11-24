# HaniWord のインストーラー (haniword-setup-<版>.exe) を作る。scripts\release\package.py で ..\release を作ってから動かす。
# 中身が SHA256SUMS.txt のとおりか・数えていないファイルが無いか・インストーラーに埋め込む停止用のコマンドが
# stop_ime_for_update.ps1 と同じかを確かめてから、Inno Setup 6 (ISCC.exe、署名を確かめる) でコンパイルする。
# 使い方: .\installer\compile.ps1 [-Version 0.1.1] [-Build <数字>] [-Out <出力先>]
# 2026-10-29: -Build はインストール先のフォルダの名前 (Program Files\HaniWord\Releases\<版>-<ビルド>) に使う。既定はコンパイルした時刻。
# ビルドごとに別のフォルダに入れるので、更新で使用中の IME 本体 (azookey.dll) を入れ替えずに済み、再起動が要らない
param([string]$Version = "0.1.1", [string]$Build = (Get-Date -Format "yyyyMMddHHmm"), [string]$Out = "")
$ErrorActionPreference = "Stop"
if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$') { throw "Invalid version" }
if ($Build -notmatch '^[0-9]{1,14}$') { throw "Invalid build" }
# 2026-10-29: 版の番号は crates\shared\src\lib.rs の APP_VERSION (新しい版の知らせが GitHub の最新のリリースと比べる・設定画面に出す) と同じにする
$sharedSource = Get-Content "$PSScriptRoot\..\crates\shared\src\lib.rs" -Raw -Encoding utf8
if ($sharedSource -notmatch 'pub const APP_VERSION: &str = "([0-9]+\.[0-9]+\.[0-9]+)";' -or $matches[1] -ne $Version) {
    throw "Version $Version differs from APP_VERSION in crates\shared\src\lib.rs"
}
$installer = $PSScriptRoot
$release = (Resolve-Path "$installer\..\release").Path
if (-not $Out) { $Out = "$installer\..\release_installer" }
$null = New-Item -ItemType Directory -Path $Out -Force

$lines = @(Get-Content "$release\SHA256SUMS.txt" -Encoding utf8)
foreach ($line in $lines) {
    if ($line -notmatch '^([a-f0-9]{64})  (.+)$') { throw "Invalid checksum line: $line" }
    $hash = $matches[1]; $relative = $matches[2]
    if ($relative -match '[:\\\x00-\x1f]' -or $relative.Split('/') -contains '..' -or $relative.Split('/') -contains '') { throw "Invalid path: $relative" }
    if ((Get-FileHash -LiteralPath (Join-Path $release $relative)).Hash.ToLowerInvariant() -ne $hash) { throw "Changed since packaging: $relative" }
}
if (@(Get-ChildItem $release -Recurse -File).Count -ne $lines.Count + 1) { throw "A file in release is not in SHA256SUMS.txt" }

# 更新のときに古い版を止めるコマンドは、管理者権限で動くので base64 にしてインストーラーに埋め込む
$stopBody = Get-Content "$installer\stop_ime_for_update.ps1" -Raw -Encoding utf8
$expectedStop = '#define StopCommandEncoded "' + [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($stopBody)) + '"'
if ([IO.File]::ReadAllText("$installer\generated_stop_command.iss").Trim() -cne $expectedStop) {
    [IO.File]::WriteAllText("$installer\generated_stop_command.iss", $expectedStop + "`r`n")
    "generated_stop_command.iss was updated from stop_ime_for_update.ps1"
}

$compiler = "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
if (-not (Test-Path $compiler)) { $compiler = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" }
if ((Get-AuthenticodeSignature $compiler).Status -ne "Valid") { throw "Inno Setup compiler signature is not valid: $compiler" }
$name = "haniword-setup-$Version"
& $compiler "/DVerifiedSecurityPayload=1" "/DMyAppVersion=$Version" "/DMyAppBuild=$Build" "/O$Out" "/F$name" "$installer\haniword.iss"
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
$exe = "$Out\$name.exe"
$sha = (Get-FileHash $exe).Hash.ToLowerInvariant()
"$sha  $name.exe" | Set-Content "$Out\$name.exe.sha256" -Encoding ascii
"$exe`n$sha  $((Get-Item $exe).Length) bytes`ninstalls into Releases\$Version-$Build"
