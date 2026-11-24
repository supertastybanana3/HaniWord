# Run by the uninstaller (elevated).
$ErrorActionPreference = "Continue"
# 2026-10-20 (Codex の再調査 M2): 管理者権限で動くので、PowerShell のモジュールは Windows のフォルダのものだけを読む
# (利用者のフォルダに同じ名前のモジュールを置かれても読み込まない)。場所は環境変数ではなく API から取る
$system = [Environment]::SystemDirectory
$env:PSModulePath = "$system\WindowsPowerShell\v1.0\Modules"
$env:PATH = "$system;$([IO.Path]::GetDirectoryName($system));$system\WindowsPowerShell\v1.0"
Import-Module "$system\WindowsPowerShell\v1.0\Modules\ScheduledTasks", "$system\WindowsPowerShell\v1.0\Modules\International"
Get-Process launcher, ui, azookey-server -ErrorAction SilentlyContinue | Where-Object {
    $directory = if ($_.Path) { [IO.Path]::GetDirectoryName($_.Path) } else { "" }
    $directory -eq "$env:APPDATA\HaniWord" -or $directory -eq "$env:ProgramFiles\HaniWord" -or
        $directory.StartsWith("$env:ProgramFiles\HaniWord\Releases\",[StringComparison]::OrdinalIgnoreCase)
} | Stop-Process -Force
Unregister-ScheduledTask -TaskName "HaniWord Startup" -Confirm:$false -ErrorAction SilentlyContinue
$list = Get-WinUserLanguageList
foreach ($l in $list) {
    foreach ($tip in @($l.InputMethodTips | Where-Object { $_ -like "*AB8A96CA-A49B-41F3-AECC-A2458BD4CD3D*" })) {
        [void]$l.InputMethodTips.Remove($tip)
    }
}
Set-WinUserLanguageList $list -Force -WarningAction SilentlyContinue
