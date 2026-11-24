# Run by the installer (elevated). Registers the logon task, adds the keyboards, starts the IME server.
# $App: %APPDATA%\HaniWord (server, dictionaries, settings). $Bin: Program Files\HaniWord (launcher, candidate window:
# they run elevated for a moment at logon, so they must live where the user cannot modify them)
param([Parameter(Mandatory=$true)][string]$App, [Parameter(Mandatory=$true)][string]$Bin, [string]$JapaneseAddedMarker = "")
$ErrorActionPreference = "Stop"
# 2026-10-20 (Codex の再調査 M2): 管理者権限で動くので、PowerShell のモジュールは Windows のフォルダのものだけを読む
# (利用者のフォルダに同じ名前のモジュールを置かれても読み込まない)。場所は環境変数ではなく API から取る
$system = [Environment]::SystemDirectory
$env:PSModulePath = "$system\WindowsPowerShell\v1.0\Modules"
$env:PATH = "$system;$([IO.Path]::GetDirectoryName($system));$system\WindowsPowerShell\v1.0"
Import-Module "$system\WindowsPowerShell\v1.0\Modules\ScheduledTasks", "$system\WindowsPowerShell\v1.0\Modules\International"
$protectedRoot = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)) "HaniWord"
$Bin = [IO.Path]::GetFullPath($Bin).TrimEnd('\')
if ($Bin -ne $protectedRoot -and [IO.Path]::GetDirectoryName($Bin) -ne "$protectedRoot\Releases") { throw "Launcher must be installed in protected Program Files" }
foreach ($directory in @($protectedRoot,$Bin,[IO.Path]::GetDirectoryName($Bin))) {
    if ((Get-Item -LiteralPath $directory).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Linked binary directory rejected" }
}
$user = "$env:USERDOMAIN\$env:USERNAME"

# 2026-10-29: 今のフォルダの user-files (変換サーバー・辞書・モデルの控え) に、前のフォルダの user-files と同じものが入ったか
# (前のものの直下の名前が全部あり、空でない)。入っていなければ前のものは消さない
function Test-SamePayload([string]$Old, [string]$New) {
    if (-not (Test-Path -LiteralPath $New -PathType Container)) { return $false }
    if ((Get-Item -LiteralPath $New -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { return $false }
    $names = @(Get-ChildItem -LiteralPath $New -Force | ForEach-Object { $_.Name })
    if (-not @(Get-ChildItem -LiteralPath $New -Recurse -File -Force).Count) { return $false }
    foreach ($item in @(Get-ChildItem -LiteralPath $Old -Force)) {
        if ($names -notcontains $item.Name) { return $false }
    }
    return $true
}

# 2026-10-29: 前の版・前のビルドのフォルダ (Releases\<版>-<ビルド>) を片付ける。消すのは $Releases の直下のフォルダだけで、
# つなぎ (junction・symlink) を含むものは触らない。user-files は今のフォルダに同じものが入っていれば消し、入っていなければフォルダごと残す。
# azookey.dll・azookey32.dll が消せたら (どのアプリも読み込んでいなければ) フォルダごと消す。消せなければ残し、次に入れるときにまた試す
# (読み込んでいるアプリは、同じフォルダの表 (英単語・next_words) を後から読む)
function Remove-OldReleases([Parameter(Mandatory=$true)][string]$Releases, [Parameter(Mandatory=$true)][string]$Current) {
    $Releases = [IO.Path]::GetFullPath($Releases).TrimEnd('\')
    $Current = [IO.Path]::GetFullPath($Current).TrimEnd('\')
    foreach ($old in @(Get-ChildItem -LiteralPath $Releases -Directory -Force)) {
        $path = [IO.Path]::GetFullPath($old.FullName).TrimEnd('\')
        if ($path -ieq $Current -or [IO.Path]::GetDirectoryName($path) -ine $Releases) { continue }
        if (($old.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
            @(Get-ChildItem -LiteralPath $path -Recurse -Force -Attributes ReparsePoint).Count) {
            Write-Output "Kept a folder with links: $path"
            continue
        }
        $payload = Join-Path $path "user-files"
        if (Test-Path -LiteralPath $payload) {
            if (-not (Test-SamePayload $payload (Join-Path $Current "user-files"))) {
                Write-Output "Kept a folder whose payload is not in the current one: $path"
                continue
            }
            Remove-Item -LiteralPath $payload -Recurse -Force -ErrorAction SilentlyContinue
        }
        $unused = $true
        foreach ($name in @("azookey.dll", "azookey32.dll")) {
            $dll = Join-Path $path $name
            if (Test-Path -LiteralPath $dll) {
                try { Remove-Item -LiteralPath $dll -Force -ErrorAction Stop } catch { $unused = $false }
            }
        }
        if ($unused) { Remove-Item -LiteralPath $path -Recurse -Force -ErrorAction SilentlyContinue }
        else { Write-Output "Kept a folder still in use: $path" }
    }
}

Set-Content -LiteralPath (Join-Path $protectedRoot "launch.vbs") -Encoding ASCII -Value @(
    'Set objShell = CreateObject("WScript.Shell")',
    ('objShell.CurrentDirectory = "' + $Bin + '"'),
    ('objShell.Run """' + (Join-Path $Bin "launcher.exe") + '""", 0, False')
)
# Private-data ACL migration is performed unelevated by the installer first.

$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit 0 -MultipleInstances IgnoreNew
$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest
$action = New-ScheduledTaskAction -Execute (Join-Path $system "wscript.exe") -Argument ('"' + (Join-Path $protectedRoot "launch.vbs") + '"') -WorkingDirectory $Bin
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $user
Register-ScheduledTask -TaskName "HaniWord Startup" -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null

$list = Get-WinUserLanguageList
$ja = $list | Where-Object { $_.LanguageTag -like "ja*" } | Select-Object -First 1
$japaneseAdded = $false
if (-not $ja) {
    $japaneseAdded = $true
    $list.Add("ja-JP")
    $ja = $list | Where-Object { $_.LanguageTag -like "ja*" } | Select-Object -First 1
}
# 2026-10-20: HaniWord registers only the live-conversion profile
foreach ($tip in @("0411:{AB8A96CA-A49B-41F3-AECC-A2458BD4CD3D}{FBF851F6-E974-4D5C-937C-1B2988A9B1EC}")) {
    if (-not ($ja.InputMethodTips -contains $tip)) { $ja.InputMethodTips.Add($tip) }
}
Set-WinUserLanguageList $list -Force -WarningAction SilentlyContinue
# 2026-11-08 (仕事 53): 日本語を新しく足したとき (もとから無かった人。英語版の Windows など) だけ、インストーラーが US 配列の案内を出す印を置く。
# 印の置き場はインストーラーの一時フォルダの ja_added.txt だけ (ほかの名前・無いフォルダには書かない)
if ($japaneseAdded -and $JapaneseAddedMarker) {
    if ([IO.Path]::GetFileName($JapaneseAddedMarker) -eq "ja_added.txt" -and (Test-Path -LiteralPath ([IO.Path]::GetDirectoryName($JapaneseAddedMarker)) -PathType Container)) {
        [IO.File]::WriteAllText($JapaneseAddedMarker, "1")
    }
}
Start-ScheduledTask -TaskName "HaniWord Startup"

# 片付けに失敗しても、IME の登録は終わっているので止めない
try { Remove-OldReleases -Releases "$protectedRoot\Releases" -Current $Bin } catch { Write-Output "Old releases were not removed: $_" }
