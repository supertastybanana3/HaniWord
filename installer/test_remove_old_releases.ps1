# 2026-10-29: setup_tasks.ps1 の前の版のフォルダの片付け (Remove-OldReleases・Test-SamePayload) の試験。
# setup_tasks.ps1 は実行しない (IME の登録・タスクには触らない)。2 つの関数だけを読み出し、%TEMP% に作った偽の Releases で試す。
# 使い方 (管理者でなくてよい): powershell -NoProfile -ExecutionPolicy Bypass -File installer\test_remove_old_releases.ps1
$ErrorActionPreference = "Stop"
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot "setup_tasks.ps1"), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw "setup_tasks.ps1 does not parse: $errors" }
foreach ($name in @("Test-SamePayload", "Remove-OldReleases")) {
    $function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $function) { throw "$name is missing in setup_tasks.ps1" }
    . ([scriptblock]::Create($function.Extent.Text))
}

$root = Join-Path ([IO.Path]::GetTempPath()) "haniword_releases_test_$PID"
$releases = Join-Path $root "HaniWord\Releases"
$failures = @()
function Check([bool]$Condition, [string]$What) { if (-not $Condition) { $script:failures += $What } }
function Add-File([string]$Path) {
    $null = New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName($Path))
    [IO.File]::WriteAllText($Path, "test")
}
function Add-Release([string]$Name, [string[]]$Payload = @("azookey-server.dll", "Dictionary\a.louds", "zenz-v3.2-small.gguf")) {
    $folder = Join-Path $releases $Name
    foreach ($file in @("azookey.dll", "azookey32.dll", "english_terms.txt", "next_words\table.hwn")) { Add-File (Join-Path $folder $file) }
    foreach ($file in $Payload) { Add-File (Join-Path $folder "user-files\$file") }
    return $folder
}

try {
    $current = Add-Release "0.1.1-202610300000"
    $legacy = Add-Release "0.1.0"                                   # 前の名前 (ビルドなし)。どのアプリも読み込んでいない
    $inUse = Add-Release "0.1.0-202610291200"                       # 開いているアプリが DLL を読み込んでいる
    $missing = Add-Release "0.1.0-202610291100" @("azookey-server.dll", "Dictionary\a.louds", "zenz-v3.2-small.gguf", "old_only.tsv")
    $linked = Add-Release "0.1.0-202610291000"
    $outside = Join-Path $root "outside"
    Add-File (Join-Path $outside "keep.txt")
    $null = New-Item -ItemType Junction -Path (Join-Path $linked "link") -Target $outside
    Add-File (Join-Path $releases "notes.txt")                      # フォルダでないものは触らない
    Add-File (Join-Path $root "HaniWord\launch.vbs")                # Releases の外は触らない

    $lock = [IO.File]::Open((Join-Path $inUse "azookey32.dll"), "Open", "Read", "Read")
    try {
        $log = @(Remove-OldReleases -Releases $releases -Current $current)
    } finally { $lock.Dispose() }

    Check (-not (Test-Path -LiteralPath $legacy)) "an unused old folder is removed"
    Check (Test-Path -LiteralPath (Join-Path $current "azookey.dll")) "the current folder is kept"
    Check (@(Get-ChildItem -LiteralPath $current -Recurse -File -ErrorAction SilentlyContinue).Count -eq 7) "nothing in the current folder is removed"
    Check (-not (Test-Path -LiteralPath (Join-Path $inUse "user-files"))) "the payload copy of a folder in use is removed"
    Check (Test-Path -LiteralPath (Join-Path $inUse "azookey32.dll")) "a DLL in use is kept"
    Check (Test-Path -LiteralPath (Join-Path $inUse "english_terms.txt")) "tables next to a DLL in use are kept"
    Check (Test-Path -LiteralPath (Join-Path $inUse "next_words\table.hwn")) "next_words next to a DLL in use are kept"
    Check (Test-Path -LiteralPath (Join-Path $missing "user-files\old_only.tsv")) "a payload not in the current folder is kept"
    Check (Test-Path -LiteralPath (Join-Path $missing "azookey.dll")) "a folder whose payload is kept keeps its DLLs"
    Check (Test-Path -LiteralPath (Join-Path $linked "azookey.dll")) "a folder with a link is not touched"
    Check (Test-Path -LiteralPath (Join-Path $linked "user-files\zenz-v3.2-small.gguf")) "a folder with a link keeps its payload"
    Check (Test-Path -LiteralPath (Join-Path $outside "keep.txt")) "the target of a link is not touched"
    Check (Test-Path -LiteralPath (Join-Path $releases "notes.txt")) "files in Releases are not touched"
    Check (Test-Path -LiteralPath (Join-Path $root "HaniWord\launch.vbs")) "files outside Releases are not touched"
    Check (($log -join "`n") -match "still in use: .*0\.1\.0-202610291200") "a folder in use is reported"

    # 次に入れたとき (DLL がもう読み込まれていない) は、残っていたフォルダも消える
    $null = @(Remove-OldReleases -Releases $releases -Current $current)
    Check (-not (Test-Path -LiteralPath $inUse)) "a folder kept for a DLL in use is removed next time"

    # 今のフォルダの user-files が空・無いときは、前のものを消さない
    $empty = Add-Release "0.1.2-202610310000" @()
    $previous = Add-Release "0.1.1-202610300500"
    $null = @(Remove-OldReleases -Releases $releases -Current $empty)
    Check (Test-Path -LiteralPath (Join-Path $previous "user-files\azookey-server.dll")) "nothing is removed when the current payload is empty"
} finally {
    $link = Join-Path $root "HaniWord\Releases\0.1.0-202610291000\link"
    if (Test-Path -LiteralPath $link) { (Get-Item -LiteralPath $link -Force).Delete() }
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}

if ($failures.Count) { $failures | ForEach-Object { "FAILED: $_" }; exit 1 }
"ok: Remove-OldReleases"
