# HaniWord の配布用のビルド (変換サーバー・IME 本体・起動用・候補窓・設定アプリ・llama.cpp)。
# 実行ファイルに、ビルドした PC のパス (C:\Users\<名前>\...) が残らないようにする:
#   Rust: --remap-path-prefix (ホーム → C:\b、cargo → C:\cargo、このリポジトリ → C:\haniword)
#   Swift: -file-prefix-map (DWARF のデバッグ情報は package.py が llvm-strip で落とす)
#   MSVC のリンカー: /PDBALTPATH:%_PDB% (デバッグ情報の欄には .pdb のファイル名だけ)
#   llama.cpp: ユーザーのフォルダの外 (-LlamaWork) に写してからビルドする (assert の __FILE__ が残るため)
# 使い方 (PowerShell):
#   .\scripts\release\build.ps1 -LlamaSource <llama.cpp の c35b667 を取り出したフォルダ>
#   llama.cpp には patches\llama.cpp-zenz.patch を当てる (当たっていなければ写したほうに当てる。元のフォルダは変えない)
# 要るもの: Visual Studio 2022 (C++)・Rust (x86_64 と i686-pc-windows-msvc)・Swift for Windows 6.4・protoc・Node.js・
#           CMake と Ninja・Vulkan SDK (環境変数 VULKAN_SDK)・Git
param(
    [Parameter(Mandatory = $true)][string]$LlamaSource,
    [string]$LlamaWork = "$env:SystemDrive\hwbuild\llama",
    [string]$TargetRoot = "",
    [switch]$SkipSwift,
    [switch]$SkipLlama
)
$ErrorActionPreference = "Stop"
$repo = (Resolve-Path "$PSScriptRoot\..\..").Path
$LlamaSource = (Resolve-Path $LlamaSource).Path
if (-not $TargetRoot) { $TargetRoot = "$repo\target" }
$log = "$TargetRoot\haniword_build_logs"
$null = New-Item -ItemType Directory -Path $log -Force

# Visual Studio の C++ の環境 (vcvars64) を読み込む
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw "Visual Studio with the C++ tools was not found" }
cmd /c "`"$vs\VC\Auxiliary\Build\vcvars64.bat`" >nul && set" | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path "env:$($matches[1])" -Value $matches[2] }
}

function Step([string]$Name, [scriptblock]$Body) {
    $ErrorActionPreference = "Continue"
    & $Body 2>&1 | Out-File "$log\$Name.txt" -Encoding utf8
    $code = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    Get-Content "$log\$Name.txt" | Select-String "Finished|Build complete|error\[|error:|FAILED" | Select-Object -Last 3 | ForEach-Object { "$Name : $_" }
    if ($code -ne 0) { throw "$Name failed; see $log\$Name.txt" }
}

$home_ = $env:USERPROFILE
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { "$home_\.cargo" }

# --- llama.cpp (Vulkan 版と、AVX2 を使う CPU 版と、AVX を使わない CPU 版) ---
# 2026-11-10: CPU 版に AVX2・FMA・F16C を使う版 (cpu_avx2) を足した。launcher が CPU の命令を見て選ぶ (無い CPU では AVX を使わない版。Vulkan 版の CPU の部分も AVX2 なので選ばない)
if (-not $SkipLlama) {
    if (-not $env:VULKAN_SDK) { $env:VULKAN_SDK = [Environment]::GetEnvironmentVariable("VULKAN_SDK", "Machine") }
    if (-not $env:VULKAN_SDK) { throw "VULKAN_SDK is not set" }
    $env:Path = "$env:VULKAN_SDK\Bin;" + $env:Path
    if ($LlamaWork.StartsWith($home_, [StringComparison]::OrdinalIgnoreCase)) { throw "LlamaWork must be outside the user profile" }
    $null = New-Item -ItemType Directory -Path $LlamaWork -Force
    robocopy $LlamaSource $LlamaWork /MIR /XD build build_* .git /NFL /NDL /NJH /NJS /NP | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "copy failed ($LASTEXITCODE)" }
    Set-Location $LlamaWork
    $patch = "$repo\patches\llama.cpp-zenz.patch"
    git apply --ignore-whitespace --reverse --check $patch 2>$null
    if ($LASTEXITCODE -ne 0) {
        git apply --ignore-whitespace $patch
        if ($LASTEXITCODE -ne 0) { throw "patches\llama.cpp-zenz.patch does not apply to $LlamaSource (expected llama.cpp c35b667)" }
    }
    $common = @("-G", "Ninja", "-DCMAKE_BUILD_TYPE=Release", "-DBUILD_SHARED_LIBS=ON", "-DLLAMA_CURL=OFF", "-DLLAMA_BUILD_TESTS=OFF",
        "-DLLAMA_BUILD_SERVER=OFF", "-DLLAMA_BUILD_EXAMPLES=OFF", "-DLLAMA_BUILD_TOOLS=OFF", "-DLLAMA_BUILD_APP=OFF", "-DLLAMA_BUILD_COMMON=OFF",
        "-DLLAMA_BUILD_COMMIT=c35b667", "-DGGML_NATIVE=OFF", "-DCMAKE_SHARED_LINKER_FLAGS=/PDBALTPATH:%_PDB%")
    foreach ($variant in @("vulkan", "cpu_avx2", "cpu")) {
        $flags = if ($variant -eq "vulkan") { @("-DGGML_VULKAN=ON", "-DGGML_AVX=ON", "-DGGML_AVX2=ON", "-DGGML_FMA=ON", "-DGGML_F16C=ON") }
                 elseif ($variant -eq "cpu_avx2") { @("-DGGML_VULKAN=OFF", "-DGGML_AVX=ON", "-DGGML_AVX2=ON", "-DGGML_FMA=ON", "-DGGML_F16C=ON", "-DGGML_AVX512=OFF", "-DGGML_BMI2=OFF") }
                 else { @("-DGGML_VULKAN=OFF", "-DGGML_AVX=OFF", "-DGGML_AVX2=OFF", "-DGGML_AVX512=OFF", "-DGGML_FMA=OFF", "-DGGML_F16C=OFF", "-DGGML_BMI2=OFF") }
        Step "llama_${variant}_cmake" { cmake -S . -B "build_$variant" @common @flags }
        Step "llama_$variant" { cmake --build "build_$variant" --config Release }
    }
    Set-Location $repo
}
# 変換サーバーは llama.dll を読み込む (リンクには Vulkan 版の llama.lib を使う。CPU 版も同じ関数を出す)
Copy-Item "$LlamaWork\build_vulkan\src\llama.lib" "$repo\server-swift\llama.lib" -Force

# --- 変換サーバー (Swift) ---
# 変換器に同梱する辞書は、サブモジュール (server-swift\azooKey_*_storage) と同じもの。新しく取り出したときは写す
$withDictionary = "$repo\server-swift\LocalPackages\AzooKeyKanaKanjiConverter\Sources\KanaKanjiConverterModuleWithDefaultDictionary"
foreach ($storage in @("azooKey_dictionary_storage", "azooKey_emoji_dictionary_storage")) {
    if (-not (Test-Path "$withDictionary\$storage")) {
        robocopy "$repo\server-swift\$storage" "$withDictionary\$storage" /E /XD .git /NFL /NDL /NJH /NJS /NP | Out-Null
        if ($LASTEXITCODE -ge 8) { throw "copy of $storage failed ($LASTEXITCODE)" }
    }
}
if (-not $SkipSwift) {
    Set-Location "$repo\server-swift"
    Step "swift" { swift build -c release -Xswiftc -file-prefix-map -Xswiftc "$home_=C:\b" -Xcc "-ffile-prefix-map=$home_=C:\b" -Xlinker "/PDBALTPATH:%_PDB%" }
}
Set-Location $repo
Copy-Item server-swift\.build\out\Products\Release-windows-x86_64\azookey-server.lib . -Force
$null = New-Item -ItemType Directory -Path crates\server\target -Force
Copy-Item azookey-server.lib crates\server\target\ -Force

# --- 設定アプリの画面 (frontend/dist に入り、frontend.exe に埋め込まれる) ---
Set-Location "$repo\frontend"
Step "npm" { if (-not (Test-Path node_modules)) { npm ci }; npm run build }
Set-Location $repo

# --- Rust (後に書いた置き換えほど優先されるので、広いものから順に) ---
$remap = "--remap-path-prefix=$home_=C:\b --remap-path-prefix=$cargoHome=C:\cargo --remap-path-prefix=$repo=C:\haniword -C link-arg=/PDBALTPATH:%_PDB%"
$env:CARGO_TARGET_DIR = "$TargetRoot\target_haniword"
$env:RUSTFLAGS = $remap
Step "rust64" { cargo build --release -p azookey-server -p launcher -p ui -p frontend --features frontend/custom-protocol }
# IME 本体は C の実行環境を中に組み込む (32 ビット版と 64 ビット版を同じフォルダに置くので、隣の vcruntime140.dll に頼らない)
$env:CARGO_TARGET_DIR = "$TargetRoot\target_haniword_static"
$env:RUSTFLAGS = "-C target-feature=+crt-static $remap"
Step "client64" { cargo build --release -p azookey-windows }
Step "client32" { cargo build --release -p azookey-windows --target=i686-pc-windows-msvc }
Remove-Item env:RUSTFLAGS, env:CARGO_TARGET_DIR

"DONE"
