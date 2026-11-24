param([switch]$ListOnly)
$ErrorActionPreference = "Stop"
# 2026-11-23: Windows のフォルダは決め打ちせず API から取る (setup_tasks.ps1 と同じ。Windows が C: 以外にある PC でも動くように)
$system = [Environment]::SystemDirectory
$env:PSModulePath = "$system\WindowsPowerShell\v1.0\Modules"
$env:PATH = "$system;$([IO.Path]::GetDirectoryName($system));$system\WindowsPowerShell\v1.0"
$app = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ApplicationData)) "HaniWord"
$bin = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)) "HaniWord"
function Test-OwnImePath([string]$Path, [string]$AppDirectory, [string]$BinDirectory) {
    if (-not $Path) { return $false }
    $full = [IO.Path]::GetFullPath($Path)
    if ([IO.Path]::GetFileName($full) -notin @("launcher.exe","ui.exe","azookey-server.exe")) { return $false }
    $directory = [IO.Path]::GetDirectoryName($full)
    return $directory -eq $AppDirectory -or $directory -eq $BinDirectory -or
        $directory.StartsWith("$BinDirectory\Releases\",[StringComparison]::OrdinalIgnoreCase)
}
function Own-Processes {
    @(Get-Process launcher,ui,azookey-server -ErrorAction SilentlyContinue | Where-Object {
        Test-OwnImePath $_.Path $app $bin
    })
}
if ($ListOnly) {
    Own-Processes | Select-Object Id,Path | ConvertTo-Json
    return
}
Own-Processes | Stop-Process -Force -ErrorAction SilentlyContinue
$deadline = (Get-Date).AddSeconds(10)
while ((Own-Processes).Count -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
if ((Own-Processes).Count) { throw "An installed IME process could not be stopped" }
