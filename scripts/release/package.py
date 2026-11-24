# HaniWord の配布物一式 (release\bin と release\files) を、入れてよいファイルの一覧からまっさらに作る。Windows の Python で動かす。
#   bin   → Program Files\HaniWord\Releases\<版>。IME 本体 (32/64)・起動用・候補窓・設定アプリと、IME 本体が読む表
#   files → %APPDATA%\HaniWord (インストーラーがふだんの権限で写す)。変換サーバー・辞書・llama.cpp・実行環境・ライセンス表示
# 中身は scripts\release\build.ps1 でビルドしたものと、このリポジトリのデータと、次の語の候補の表 (--next-words) と、同音の語の組み合わせの表 (--colloc) だけ。
# 最後に、入っていてはいけないもの (設定・学習・ログ・モデル・ビルドした PC のパス) と DLL の依存の欠けを確かめ、SHA256SUMS.txt を書く。
# 確かめが通ったときだけ release に置く (前の release は release.before_<日時> に残す)。
# 使い方: py -3 scripts\release\package.py --next-words <next_*.hwn と SHA256SUMS.txt のフォルダ> --colloc <colloc_choices.tsv と SHA256SUMS.txt のフォルダ>
#         [--llama-source <llama.cpp>] [--llama-work C:\hwbuild\llama] [--target-root <build.ps1 の -TargetRoot>]
#         [--forbid <ほかに入っていてはいけない語 (何回でも)>]
import argparse, datetime, glob, hashlib, json, os, re, shutil, struct, subprocess, sys, tempfile

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
ap = argparse.ArgumentParser()
ap.add_argument("--next-words", required=True)
ap.add_argument("--colloc", required=True)
ap.add_argument("--llama-source", required=True)
ap.add_argument("--llama-work", default=os.path.expandvars(r"%SystemDrive%\hwbuild\llama"))
ap.add_argument("--target-root", default=os.path.join(REPO, "target"))
ap.add_argument("--out", default=os.path.join(REPO, "release"))
ap.add_argument("--forbid", action="append", default=[])
args = ap.parse_args()

T64 = os.path.join(args.target_root, "target_haniword", "release")
TS64 = os.path.join(args.target_root, "target_haniword_static", "release")
TS32 = os.path.join(args.target_root, "target_haniword_static", "i686-pc-windows-msvc", "release")
SWIFT_OUT = os.path.join(REPO, r"server-swift\.build\out\Products\Release-windows-x86_64")
STORAGE = os.path.join(REPO, r"server-swift\LocalPackages\AzooKeyKanaKanjiConverter\Sources\KanaKanjiConverterModuleWithDefaultDictionary")
SWIFT_HOME = os.path.expandvars(r"%LOCALAPPDATA%\Programs\Swift")


def one(pattern):
    found = sorted(glob.glob(pattern))
    if len(found) != 1:
        sys.exit(f"expected exactly one match for {pattern}, found {found}")
    return found[0]


def vs_redist():
    vswhere = os.path.expandvars(r"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe")
    vs = subprocess.run([vswhere, "-latest", "-products", "*", "-property", "installationPath"], capture_output=True, text=True).stdout.strip()
    return one(os.path.join(vs, r"VC\Redist\MSVC\14.*\x64"))


stage = tempfile.mkdtemp(prefix="release.pending.", dir=os.path.dirname(args.out))
BIN = os.path.join(stage, "bin")
FILES = os.path.join(stage, "files")
LIC = os.path.join(FILES, "licenses")
os.makedirs(LIC)


def put(src, dst_dir, name=None):
    if not os.path.isfile(src):
        sys.exit(f"missing: {src}")
    os.makedirs(dst_dir, exist_ok=True)
    shutil.copy2(src, os.path.join(dst_dir, name or os.path.basename(src)))


def put_tree(src, dst):
    if not os.path.isdir(src):
        sys.exit(f"missing folder: {src}")
    shutil.copytree(src, dst)


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def read(path):
    for enc in ("utf-8", "latin-1"):
        try:
            return open(path, encoding=enc).read()
        except UnicodeDecodeError:
            continue
    return ""


runtime = one(SWIFT_HOME + r"\Runtimes\*\usr\bin")
toolchain = one(SWIFT_HOME + r"\Toolchains\*\usr\bin")
redist = vs_redist()
vcomp = os.path.join(redist, r"Microsoft.VC143.OpenMP\vcomp140.dll")

# ===== bin (Program Files) =====
put(TS64 + r"\azookey_windows.dll", BIN, "azookey.dll")
put(TS32 + r"\azookey_windows.dll", BIN, "azookey32.dll")
put(T64 + r"\launcher.exe", BIN)
put(T64 + r"\ui.exe", BIN)
put(T64 + r"\frontend.exe", BIN, "azookey-settings.exe")
# 2026-10-30: 言語の一覧 (Win+Space・言語バー) のアイコン。インストーラーが版ごとのフォルダの外 (Program Files\HaniWord\haniword.ico) に置き、
# IME 本体がそこを登録する (版ごとのフォルダの DLL を指すと、更新で前のフォルダを片付けたあと既定の字に戻った)
put(os.path.join(REPO, r"crates\client\res\azookey.ico"), BIN, "haniword.ico")
for name in ("vcruntime140.dll", "vcruntime140_1.dll"):  # 起動用・候補窓・設定アプリ用 (IME 本体は C の実行環境を中に持つ)
    put(os.path.join(redist, "Microsoft.VC143.CRT", name), BIN)
for name in ("english_words_scowl.txt", "english_terms.txt", "english_case.txt", "english_romaji_priority.txt", "english_boundary_words.txt", "english_suru_words.txt", "english_n_split.txt",
             "english_words_context.txt", "english_words_context.json", "english_context_curated.txt", "english_brands.txt", "english_products.txt", "english_names.txt", "japanese_names_romaji.txt", "initial_words.txt", "german_case.tsv",
             "ENGLISH_CONTEXT_COPYRIGHT.txt", "SCOWL_COPYRIGHT.txt", "UNICODE_LICENSE.txt", "IVS_NOTES.md"):
    put(os.path.join(REPO, name), BIN)
put_tree(os.path.join(REPO, "latin_words"), os.path.join(BIN, "latin_words"))
# 次の語の候補の表: そのフォルダの SHA256SUMS.txt と合うものだけ
sums = {}
for line in open(os.path.join(args.next_words, "SHA256SUMS.txt"), encoding="utf-8"):
    digest, name = line.split()
    sums[name.lstrip("*")] = digest
tables = sorted(n for n in os.listdir(args.next_words) if n.endswith(".hwn"))
if sorted(sums) != tables or len(tables) != 14:
    sys.exit(f"next words: SHA256SUMS.txt lists {sorted(sums)}, the folder has {tables}")
for name in tables:
    if sha256(os.path.join(args.next_words, name)) != sums[name]:
        sys.exit(f"next words: checksum mismatch {name}")
    put(os.path.join(args.next_words, name), os.path.join(BIN, "next_words"))

# ===== files (%APPDATA%\HaniWord) =====
# Swift の変換サーバーは DWARF のデバッグ情報 (ビルドしたフォルダのパスが入る) を落として入れる
if subprocess.run([os.path.join(toolchain, "llvm-strip.exe"), "--strip-debug", os.path.join(SWIFT_OUT, "azookey-server.dll"),
                   "-o", os.path.join(FILES, "azookey-server.dll")]).returncode != 0:
    sys.exit("llvm-strip failed")
put(T64 + r"\azookey-server.exe", FILES)
for name in ("english_words.tsv", "english_words_extra.tsv", "japanese_vocabulary.json", "JAPANESE_VOCABULARY_NOTES.md",
             "extra_dictionary.tsv", "latin_kana_policy.tsv", "katakana_variants.tsv", "reading_suppress.tsv", "variant_spellings.tsv",
             "misreading_candidates.tsv", "misuse_notes.tsv", "value_adjustments.tsv", "homophone_phrases.tsv", "homophone_triggers.tsv",
             "homophone_words.tsv", "counter_counts.tsv", "rendaku_readings.tsv", "hyouki_groups.tsv", "casual_words.tsv", "SCOWL_COPYRIGHT.txt"):
    put(os.path.join(REPO, name), FILES)
# 2026-11-07 (仕事 (4)): 同音の語の組み合わせの表 colloc_choices.tsv (30MB ほどあるのでリポジトリには入れない。作り方は tools/colloc/README.md)。
# そのフォルダの SHA256SUMS.txt と合うものだけ。変換サーバーは行を二分探索で引くので、行の形と、鍵 (読み<TAB>文脈の種類<TAB>文脈) のバイトの順も確かめる
sums = {}
for line in open(os.path.join(args.colloc, "SHA256SUMS.txt"), encoding="utf-8"):
    digest, name = line.split()
    sums[name.lstrip("*")] = digest
colloc = os.path.join(args.colloc, "colloc_choices.tsv")
if sorted(sums) != ["colloc_choices.tsv"] or sha256(colloc) != sums["colloc_choices.tsv"]:
    sys.exit(f"colloc: SHA256SUMS.txt must list colloc_choices.tsv with its checksum ({sorted(sums)})")
prev_key, colloc_rows = b"", 0
with open(colloc, "rb") as f:
    for line in f:
        if line.startswith(b"#"):
            continue
        cols = line.rstrip(b"\n").split(b"\t")
        key = b"\t".join(cols[:3]) + b"\t"
        if len(cols) != 6 or not cols[4].isdigit() or b"\r" in line or key <= prev_key:
            sys.exit(f"colloc: bad or unsorted row {line[:200]!r}")
        prev_key, colloc_rows = key, colloc_rows + 1
if colloc_rows < 100_000:
    sys.exit(f"colloc: only {colloc_rows} rows")
put(colloc, FILES)
# 2026-10-22: 分野辞書とオフにできない追加辞書 (files\domain)。domain_dictionaries・extra_dictionaries から書き出して読み戻す。
# 2 つのライセンス文 (domain_dictionaries_LICENSE.txt・extra_dictionaries_LICENSE.txt) と出典の記事の一覧は licenses に置かれる
# 2026-10-23: 書き出すもとは git に入っていて変更の無いものだけ (作業ツリーの置き忘れのファイルを拾わない)。公開してよい名前は下の 2 つで決め打ち
# 2026-10-31: 数学 (math) を足す (JMdict・ウィキブックス・学習指導要領解説から。de96cb5)
PUBLISHED_DOMAINS = {"architecture", "biology", "chemistry", "earth", "geography", "history", "it", "law", "math", "medicine", "religion"}
# 2026-11-04: 感動詞・つなぎの言葉 (core_interjections) を足す
# 2026-11-07 (仕事 39): 役割語の一人称・二人称と語尾 (core_roles) を足す
# 2026-11-07 (仕事 42): 常用外の字の表記 (core_rare_kanji。嗾ける・揶揄う) を足す
# 2026-11-07 (仕事 44): よく知られた人・作品・団体と日本の上場企業の名前 (core_names) を足す
# 2026-11-11: ソフト・アプリ・サービスの製品名 (core_products。After Effects ← アフターエフェクト)
ALWAYS_DICTIONARIES = {"core_forms", "core_interjections", "core_misreadings", "core_names", "core_phrases", "core_products", "core_rare_kanji", "core_roles", "core_words"}
# 2026-11-06 (仕事 39): 方言の言い方の辞書 (dialect_dictionaries。設定で選んだ人だけ読む。domain_dialect_<名前>.*・dialects.txt)
PUBLISHED_DIALECTS = {"kansai"}
# 2026-11-07 (仕事 43): くだけた言葉の辞書 (slang_dictionaries。設定でオンにした人だけ読む。domain_slang_<名前>.*・slang.txt。internet = ネットスラング)
PUBLISHED_SLANG = {"internet"}
for folder in ("domain_dictionaries", "extra_dictionaries", "dialect_dictionaries", "slang_dictionaries"):
    tracked = {p for p in subprocess.run(["git", "-C", REPO, "ls-files", "-z", "--", folder], capture_output=True).stdout.decode("utf-8").split("\0") if p}
    present = {folder + "/" + n for n in os.listdir(os.path.join(REPO, folder)) if os.path.isfile(os.path.join(REPO, folder, n))}
    changed = subprocess.run(["git", "-C", REPO, "status", "--porcelain", "--", folder], capture_output=True).stdout.strip()
    if tracked != present or changed:
        sys.exit(f"{folder}: only committed, unchanged files may be published (untracked: {sorted(present - tracked)}, missing: {sorted(tracked - present)}, changed: {changed[:200]})")
if subprocess.run([sys.executable, os.path.join(REPO, r"tools\domain\build_domain.py"),
                   "--out", os.path.join(FILES, "domain"), "--licenses", LIC]).returncode != 0:
    sys.exit("build_domain.py failed")
# 2026-11-06: 変換サーバーの一式 (azookey-server.exe・azookey-server.dll・llama_*\*.dll) のどこからも読み込まれない DLL は入れない
# (PE の import 表と遅延 import 表をたどって数えた。scripts\release\pe_imports.py。外した一式で変換が同じことを 100 文で確かめた)
UNUSED_RUNTIME_DLLS = {"foundationnetworking.dll", "foundationxml.dll", "swiftdistributed.dll", "swiftregexbuilder.dll", "swiftremotemirror.dll",
                       "swiftruntime.dll", "swiftswiftononesupport.dll", "swift_builtin_float.dll", "swift_differentiation.dll", "swift_volatile.dll",
                       "concrt140.dll", "msvcp140_1.dll", "msvcp140_2.dll", "msvcp140_atomic_wait.dll", "msvcp140_codecvt_ids.dll",
                       "vccorlib140.dll", "vcruntime140_threads.dll"}
for path in sorted(glob.glob(runtime + r"\*.dll")):  # Swift と Visual C++ の実行環境 (Swift の再配布用のもの)
    if os.path.basename(path).lower() in UNUSED_RUNTIME_DLLS:
        continue
    put(path, FILES)
put(vcomp, FILES)
put_tree(STORAGE + r"\azooKey_dictionary_storage\Dictionary", os.path.join(FILES, "Dictionary"))
put_tree(STORAGE + r"\azooKey_emoji_dictionary_storage\EmojiDictionary", os.path.join(FILES, "EmojiDictionary"))
# 2026-11-10: cpu_avx2 (AVX2・FMA・F16C を使う CPU 版。launcher が CPU の命令を見て llama_cpu と選び分ける) を足した
for variant, names in (("vulkan", ("llama.dll", "ggml.dll", "ggml-base.dll", "ggml-cpu.dll", "ggml-vulkan.dll")),
                       ("cpu_avx2", ("llama.dll", "ggml.dll", "ggml-base.dll", "ggml-cpu.dll")),
                       ("cpu", ("llama.dll", "ggml.dll", "ggml-base.dll", "ggml-cpu.dll"))):
    out = os.path.join(FILES, "llama_" + variant)
    for name in names:
        put(os.path.join(args.llama_work, "build_" + variant, "bin", name), out)
    put(vcomp, out)

# ===== ライセンス表示 (files\licenses と THIRD_PARTY_NOTICES.txt) =====
notice = []


def section(title, body):
    notice.append("=" * 78 + "\n" + title + "\n" + "=" * 78 + "\n" + body.strip() + "\n\n")


def attach(name, path, title=None):
    """ライセンス文を licenses に写し、本文も THIRD_PARTY_NOTICES.txt に入れる"""
    if not os.path.isfile(path):
        sys.exit(f"license file missing: {path}")
    shutil.copyfile(path, os.path.join(LIC, name))
    section(title or name, read(path))


def license_files(d):
    if not os.path.isdir(d):
        return []
    return sorted(f for f in os.listdir(d) if re.match(r"(LICEN[CS]E|COPYING|NOTICE|COPYRIGHT|UNLICENSE)", f, re.I) and os.path.isfile(os.path.join(d, f)))


attach("HaniWord_LICENSE.txt", os.path.join(REPO, "LICENSE"), "HaniWord — MIT License")
upstream = subprocess.run(["git", "-C", REPO, "show", "65835aa:LICENSE"], capture_output=True)
if upstream.returncode != 0 or b"fkunn1326" not in upstream.stdout:
    sys.exit("could not read the azooKey-Windows license (commit 65835aa)")
open(os.path.join(LIC, "azooKey-Windows_LICENSE.txt"), "wb").write(upstream.stdout)
section("azooKey-Windows (fkunn1326) — MIT License。HaniWord はこれをもとにしています", upstream.stdout.decode("utf-8"))
conv = os.path.join(REPO, r"server-swift\LocalPackages\AzooKeyKanaKanjiConverter")
attach("AzooKeyKanaKanjiConverter_LICENSE.txt", os.path.join(conv, "LICENSE"),
       "AzooKeyKanaKanjiConverter (Miwa / Ensan) — MIT License。改造して同梱しています")
attach("azooKey_dictionary_storage_LICENSE.txt", os.path.join(REPO, r"server-swift\azooKey_dictionary_storage\LICENSE"),
       "azooKey の辞書 (Dictionary) — Apache License 2.0。変更していません")
attach("azooKey_emoji_dictionary_README.md", os.path.join(REPO, r"server-swift\azooKey_emoji_dictionary_storage\data\README.md"),
       "azooKey の絵文字辞書 (EmojiDictionary) — 出典ごとのライセンス (Mozc: BSD-3-Clause, Unicode CLDR: Unicode License, azooKey: MIT)")
attach("EfficientNGram_tokenizer_README.md", os.path.join(conv, r"Sources\EfficientNGram\tokenizer\README.md"),
       "EfficientNGram の tokenizer データ — CC BY-SA 4.0 (README の表示)")
attach("llama.cpp_LICENSE.txt", os.path.join(args.llama_source, "LICENSE"),
       "llama.cpp / ggml (ggml authors) — MIT License。patches\\llama.cpp-zenz.patch を当ててビルドして同梱しています")
sgemm = os.path.join(args.llama_source, r"ggml\src\ggml-cpu\llamafile\sgemm.cpp")
if os.path.isfile(sgemm):
    section("llama.cpp に含まれる llamafile sgemm (Mozilla Foundation) — MIT License (ファイル先頭の表示)", read(sgemm).split("*/")[0])
# 2026-11-06: llama.cpp の中の、ほかから持ってきたコードの表示 (配布元の LICENSE をそのまま installer\licenses に置いた)
attach("YaRN_LICENSE.txt", os.path.join(REPO, r"installer\licenses\YaRN_LICENSE.txt"),
       "llama.cpp (ggml-cpu.dll・ggml-vulkan.dll の rope_yarn) に含まれる YaRN (https://github.com/jquesnelle/yarn) の計算 — MIT License")
attach("ggllm.cpp_LICENSE.txt", os.path.join(REPO, r"installer\licenses\ggllm.cpp_LICENSE.txt"),
       "llama.cpp (llama.dll の src/llama-vocab.cpp) に含まれる ggllm.cpp (https://github.com/cmp-nct/ggllm.cpp) をもとにした部分 — MIT License")
section("llama.cpp に含まれる Unicode の表 (llama.dll の src/unicode-data.cpp)",
        "Unicode Character Database (https://www.unicode.org/Public/UCD/latest/ucd/) から作られた表です — Unicode License V3。全文は licenses/UNICODE_LICENSE.txt")
# ggml-vulkan.dll には Vulkan SDK 1.4.363.0 のヘッダー (vulkan.hpp。Apache-2.0 OR MIT) が組み込まれる。MIT を選ぶ
attach("Vulkan-Headers_LICENSE.md", os.path.join(REPO, r"installer\licenses\Vulkan-Headers_LICENSE.md"),
       "Vulkan-Headers (The Khronos Group Inc.、https://github.com/KhronosGroup/Vulkan-Headers の vulkan-sdk-1.4.363.0) — "
       "Apache-2.0 OR MIT のうち MIT で使います。llama_vulkan\\ggml-vulkan.dll に組み込まれるヘッダー (vulkan.hpp)")
attach("Vulkan-Headers_MIT.txt", os.path.join(REPO, r"installer\licenses\Vulkan-Headers_MIT.txt"), "Vulkan-Headers — MIT License (LICENSES/MIT.txt)")
section("tracing-chrome (Thoren Paulson) — MIT License",
        "crates/client/src/tracing_chrome.rs は tracing-chrome を改変したものです。\nCopyright (c) 2020 Thoren Paulson\n"
        "Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the \"Software\"), "
        "to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, "
        "and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:\n"
        "The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.\n"
        "THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, "
        "FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER "
        "LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.")
attach("Mozc_LICENSE.txt", os.path.join(REPO, r"installer\licenses\Mozc_LICENSE.txt"),
       "Mozc (Google) — BSD-3-Clause。候補窓の配色の値などを参考にしています")
# 2026-11-06: 入力モードの表示 (crates/ui の indicator.html・caret.rs) は CaretCue をもとにしている (ui.exe に入る)
attach("CaretCue_NOTICE.md", os.path.join(REPO, r"crates\ui\THIRD_PARTY_CaretCue.md"),
       "CaretCue (GoldAnd07) — Apache License 2.0。入力モードの表示とテキストカーソルの位置探しは、これをもとにしています")
# 2026-11-06 (仕事 35): 入力モードの表示の「カーソルのそばに小さく」(crates/ui の indicator.html の #compact・indicator.rs) は Mozc の表示を移したもの
attach("Mozc_indicator_NOTICE.md", os.path.join(REPO, r"crates\ui\THIRD_PARTY_Mozc_indicator.md"),
       "Mozc (Google) の入力モードの表示 — BSD-3-Clause。入力モードの表示「カーソルのそばに小さく」は、これを移したものです")
# 2026-11-06 (仕事 37): 外観で「足りない字を補わない」ときに □ を出すフォント (ui.exe と設定アプリに入る)
attach("AdobeNotDef_NOTICE.md", os.path.join(REPO, r"crates\ui\THIRD_PARTY_AdobeNotDef.md"),
       "Adobe NotDef (Adobe) — SIL Open Font License 1.1。外観の設定で足りない字を □ で出すときに、候補の窓と設定アプリの見本で使います")
# 2026-11-06: 候補窓 (ui.exe) と設定アプリ (azookey-settings.exe) は WebView2 の読み込み部品 (WebView2LoaderStatic.lib、webview2-com-sys が
# msvc で静的にリンクする) を中に持つ。NuGet の Microsoft.Web.WebView2 1.0.3650.58 の LICENSE.txt・NOTICE.txt をそのまま置いた
attach("WebView2_LICENSE.txt", os.path.join(REPO, r"installer\licenses\WebView2_LICENSE.txt"),
       "Microsoft Edge WebView2 SDK (Microsoft.Web.WebView2 1.0.3650.58 の WebView2LoaderStatic.lib) — ui.exe と azookey-settings.exe に組み込まれています")
attach("WebView2_NOTICE.txt", os.path.join(REPO, r"installer\licenses\WebView2_NOTICE.txt"), "Microsoft Edge WebView2 SDK — NOTICE.txt")
section("データの出典", """
- english_words.tsv: JMdict (Electronic Dictionary Research and Development Group, CC BY-SA 4.0) の訳語・語源と、
  CMUdict (Carnegie Mellon University, BSD 2 条項型) の発音から作成 (このファイル自体も CC BY-SA 4.0)。
  https://www.edrdg.org/edrdg/licence.html
- english_words_scowl.txt, english_romaji_priority.txt, english_n_split.txt: SCOWL (Kevin Atkinson ほか) から作成。全文は SCOWL_COPYRIGHT.txt。
  english_romaji_priority.txt と english_n_split.txt の選別に JMdict の読みを使用。
- english_boundary_words.txt (語の切れ目なら英語にする英単語)・english_suru_words.txt (サ変の形のすぐ前なら英字にまとめる英単語): SCOWL の語
  (english_suru_words.txt は、HaniWord の開発者が書いた english_terms.txt の語と git の動詞の名前 7 つも) のうち、ローマ字として読んだかなが
  azooKey の辞書で日本語の語にならないものを選んで作成 (tools/english_lists/make_boundary_words.py・make_suru_words.py)。全文は SCOWL_COPYRIGHT.txt。
- english_brands.txt: Wikidata (https://www.wikidata.org/、CC0 1.0) の企業・ブランド・製品・ソフトウェア・ウェブサービスなどの項目のラベルと別名から作成 (tools/domain/collect_brands.py)。
- english_products.txt: 日本語版ウィキペディア (CC BY-SA 4.0) の、ソフトウェア・アプリケーション・ウェブサービスのカテゴリの記事の題と転送の題から作成 (tools/core_products)。
  語ごとの大文字・小文字は english_case.txt・english_brands.txt にそろえた。このファイルは CC BY-SA 4.0。
- english_names.txt: Wikidata (https://www.wikidata.org/、CC0 1.0) の人の姓・地名・組織などの項目の英語のラベルから作成 (tools/english_names/make_english_names.py)。
- german_case.tsv (ドイツ語の名詞の頭の大文字。英語の文のつづりの直しが読む): FineWeb-2 (HuggingFaceFW/fineweb-2 の deu_Latn、ODC-By 1.0。
  Common Crawl の利用規約にも従う) の文の途中の語が大文字で始まった回数の割合から作成 (tools/latin_spell/german_case)。文そのものは入らない。
  語は latin_words/de.txt (wordfreq、CC BY-SA 4.0) の語だけ。このファイルは CC BY-SA 4.0。
- initial_words.txt (英字 1 字＋語 (Jリーグ・B級・X線) を小文字で打った綴りと字の形): 日本語版ウィキペディアの本文 (wikimedia/wikipedia 20231101.ja。2023-11-01 のダンプで、
  元は CC BY-SA 3.0 / GFDL、https://huggingface.co/datasets/wikimedia/wikipedia) での語の形と回数 (文そのものは入らない) と、
  SudachiDict (Apache License 2.0) の読みから作成 (tools/initials/scan_initials.py・initials_words.py・make_initial_words.py)。
  このファイルは、CC BY-SA 3.0 の 4(b) (同じ許諾要素の後の版) により CC BY-SA 4.0。
- japanese_names_romaji.txt: Mozc の公開辞書 (https://github.com/google/mozc の src/data/dictionary_oss、commit f183d4d3a7ba139d7cb2252c05f608e9f8266aef) の
  人名 (名・姓・人名一般) の読みと、Wikidata (https://www.wikidata.org/、CC0 1.0) の日本の人物の名前のカナ (P1814) をローマ字にしたもの
  (tools/names/build_names_v2.py・select_names.py)。許諾は licenses\\extra_dictionaries_LICENSE.txt の「Mozc の公開辞書のファイルの許諾」。
- IME 本体 (azookey.dll) に組み込んだ 2 つの表 (crates/client/src/engine/after_latin.txt・japanese_heads.txt。英単語 + 来にくい かな (powers + たげ) を
  英単語 2 つ (power + stage) に分けるかを決める): 日本語版ウィキペディアの本文 (wikimedia/wikipedia 20231101.ja。2023-11-01 のダンプで、元は CC BY-SA 3.0 / GFDL、
  https://huggingface.co/datasets/wikimedia/wikipedia) を UniDic (unidic-lite) の読みで数えた回数 (英字の語のすぐ後ろの読みの頭と、語の頭の読み)。
  回数だけで本文は含みません。表は、CC BY-SA 3.0 の 4(b) (同じ許諾要素の後の版) により CC BY-SA 4.0 (https://creativecommons.org/licenses/by-sa/4.0/) で配布します。作り方は tools/after_latin にあります。
- 長押しの候補の並び (アクセント付きの文字の頻度): 英語版ウィキペディア「Letter frequency」の表 (CC BY-SA 4.0、2026-10-10 取得) の数値と、
  Xiao ほか "MES-P: an Emotional Tonal Speech Dataset in Mandarin Chinese" (arXiv:1808.10095) の表 V の数値を使用。
- 変換の AI モデル (zenz-v3.2-small, Miwa-Keita) はこのソフトに同梱せず、利用者が Hugging Face から取得します (取る版 c67e03e のモデルカードの表示は Apache-2.0、今のモデルカードの表示は CC BY-SA 4.0。HaniWord は厳しいほうの CC BY-SA 4.0 として扱います。
  取る版のモデルカード: https://huggingface.co/Miwa-Keita/zenz-v3.2-small-gguf/blob/c67e03e07d215c869f591b274c1631170d3e11fe/README.md
  今のモデルカード: https://huggingface.co/Miwa-Keita/zenz-v3.2-small-gguf/blob/main/README.md)。
""")
section("分野辞書 (domain\\domain_<名前>.*、11 分野) — 材料ごとの条件 (多くは CC BY-SA 4.0)。"
        "licenses\\domain_dictionaries_LICENSE.txt と、出典の記事の一覧 licenses\\domain_<名前>_wikipedia.txt・_wikibooks.txt",
        read(os.path.join(LIC, "domain_dictionaries_LICENSE.txt")))
section("オフにできない追加辞書 (domain\\domain_core_*.*) と、代わりの字の成句・よくある誤用・難読語の同音語の表 "
        "(variant_spellings.tsv・misreading_candidates.tsv・misuse_notes.tsv・homophone_phrases.tsv・homophone_triggers.tsv・homophone_words.tsv・counter_counts.tsv・rendaku_readings.tsv・hyouki_groups.tsv・colloc_choices.tsv・casual_words.tsv) — 材料ごとの条件。"
        "licenses\\extra_dictionaries_LICENSE.txt",
        read(os.path.join(LIC, "extra_dictionaries_LICENSE.txt")))
# 2026-11-07 (仕事 40): 注意の注記 (misuse_notes.tsv の 差別・言い換え の行) の参考文献の一覧 (注記の [n]、SIST 02 の Web の形・Internet Archive の写しつき) と、
# 語を選んだ NG 語の一覧の許諾の文 (installer\licenses\caution_lists を licenses\caution_list_<名前> に平らに写す。
# 配布元の LICENSE そのまま。Nextremer のものだけは LICENSE のファイルが無いので、README の記載と MIT の本文)
attach("caution_references.txt", os.path.join(REPO, "extra_dictionaries", "caution_references.txt"),
       "注意の注記の参考文献 (misuse_notes.tsv の 差別・言い換え の行の注記の番号 [n])")
for _name in sorted(os.listdir(os.path.join(REPO, r"installer\licenses\caution_lists"))):
    attach("caution_list_" + _name, os.path.join(REPO, r"installer\licenses\caution_lists", _name),
           "注意の注記の語を選んだ NG 語の一覧 — " + _name.replace("_LICENSE.txt", "").replace("_", "/", 1))
section("方言の言い方の辞書 (domain\\domain_dialect_<名前>.*。設定の「方言の語尾」で選んだ人だけが使う) — CC BY-SA 4.0。"
        "licenses\\dialect_dictionaries_LICENSE.txt",
        read(os.path.join(LIC, "dialect_dictionaries_LICENSE.txt")))
section("くだけた言葉の辞書 (domain\\domain_slang_<名前>.*。設定の「ネットスラング」をオンにした人だけが使う) — CC BY-SA 4.0。"
        "licenses\\slang_dictionaries_LICENSE.txt",
        read(os.path.join(LIC, "slang_dictionaries_LICENSE.txt")))
section("次の語の候補の表 (next_words/next_*.hwn)", """
英字を打っているときに次の語を出すための表 (前の語と次の語の組ごとの点数) です。次の公開データで語の並びを数えて作りました。
文章そのものは入っていません。表は CC BY-SA 4.0 (https://creativecommons.org/licenses/by-sa/4.0/) で配布します。作り方は tools/next_words にあります。

- 英語: FineWeb (Hugging Face, HuggingFaceFW/fineweb の sample-10BT) — ODC-By 1.0。Common Crawl の利用規約にも従います。
  https://huggingface.co/datasets/HuggingFaceFW/fineweb
- 英語以外の 13 言語: FineWeb-2 (Hugging Face, HuggingFaceFW/fineweb-2 の各言語の train) — ODC-By 1.0。Common Crawl の利用規約にも従います。
  https://huggingface.co/datasets/HuggingFaceFW/fineweb-2
- 全言語: Tatoeba の例文 (https://tatoeba.org) — CC BY 2.0 FR (一部 CC0)。
  https://downloads.tatoeba.org/exports/per_language/
- 候補にする語の一覧: 英語は english_words_scowl.txt ほか (SCOWL)、英語以外は latin_words (wordfreq 3.1.1, Robyn Speer, CC BY-SA 4.0)。
- 変更: 小文字にそろえて隣り合う 2 語を数え、FineWeb / FineWeb-2 を 0.7・Tatoeba を 0.3 の重みで混ぜ、
  前の語を見ない順位と上位 3 つが変わる組だけを残しました。
""")
attach("CMUdict_LICENSE.txt", os.path.join(REPO, r"installer\licenses\CMUdict_LICENSE.txt"), "CMUdict (Carnegie Mellon University) — english_words.tsv の作成に使用")
attach("SCOWL_COPYRIGHT.txt", os.path.join(REPO, "SCOWL_COPYRIGHT.txt"), "SCOWL")
attach("ENGLISH_CONTEXT_COPYRIGHT.txt", os.path.join(REPO, "ENGLISH_CONTEXT_COPYRIGHT.txt"), "SCOWLv2 English context supplement")
attach("UNICODE_LICENSE.txt", os.path.join(REPO, "UNICODE_LICENSE.txt"), "Unicode IVD and Unihan data")
attach("IVS_NOTES.md", os.path.join(REPO, "IVS_NOTES.md"), "Registered ideograph variants: sources and modifications")
attach("JAPANESE_VOCABULARY_NOTES.md", os.path.join(REPO, "JAPANESE_VOCABULARY_NOTES.md"), "Curated public Japanese vocabulary")
for name in ("NOTICE.txt", "UPSTREAM.txt", "WORD_FREQ_LICENSE.txt"):
    attach(f"LatinWords_{name}", os.path.join(REPO, "latin_words", name), f"Selected-language wordfreq dictionaries: {name}")

# Rust のライブラリ (Windows 向けに実際に組み込まれる通常の依存)
meta = subprocess.run(["cargo", "metadata", "--filter-platform", "x86_64-pc-windows-msvc", "--format-version", "1", "--locked"],
                      cwd=REPO, capture_output=True, text=True, encoding="utf-8")
if meta.returncode != 0:
    sys.exit("cargo metadata failed: " + meta.stderr[-500:])
m = json.loads(meta.stdout)
pk = {p["id"]: p for p in m["packages"]}
ws = set(m["workspace_members"])
nodes = {n["id"]: n for n in m["resolve"]["nodes"]}
seen, stack = set(), list(ws)
while stack:
    i = stack.pop()
    if i in seen:
        continue
    seen.add(i)
    for d in nodes[i]["deps"]:
        if any(k["kind"] in (None, "normal") for k in d["dep_kinds"]):
            stack.append(d["pkg"])
crates = sorted((pk[i] for i in seen if i not in ws), key=lambda p: (p["name"], p["version"]))
lines, mpl, missing = [], [], []
for p in crates:
    d = os.path.dirname(p["manifest_path"])
    files = license_files(d)
    # 2026-11-06: crate にライセンスのファイルが入っていないものは、公開したときのコミット (.cargo_vcs_info.json) の配布元から取った文
    # (installer\licenses\rust\<名前-版>、取り方は installer\licenses\rust\README.md)
    if not files:
        extra = os.path.join(REPO, "installer", "licenses", "rust", f"{p['name']}-{p['version']}")
        if os.path.isdir(extra):
            d, files = extra, sorted(os.listdir(extra))
    lic = p.get("license") or "(see files)"
    lines.append(f"{p['name']} {p['version']} — {lic} {p.get('repository') or ''}")
    if "MPL" in lic:
        mpl.append(f"{p['name']} {p['version']}: https://crates.io/crates/{p['name']}/{p['version']}")
    if files:
        sub = os.path.join(LIC, "rust", f"{p['name']}-{p['version']}")
        os.makedirs(sub, exist_ok=True)
        for f in files:
            shutil.copyfile(os.path.join(d, f), os.path.join(sub, f))
    else:
        missing.append(f"{p['name']} {p['version']}")
section(f"Rust のライブラリ ({len(crates)} 個)。各ライセンス文は licenses/rust/<名前-版>/ にあります",
        "\n".join(lines) + ("\n\nライセンス文のファイルが同梱されていなかったもの (ライセンスは上の表示のとおり):\n" + "\n".join(missing) if missing else ""))
if mpl:
    section("MPL-2.0 のライブラリのソースの入手先 (変更せずに組み込んでいます)", "\n".join(mpl))
for name, cand in (("Apache-2.0.txt", "LICENSE-APACHE"), ("MIT.txt", "LICENSE-MIT")):
    for p in crates:
        f = os.path.join(os.path.dirname(p["manifest_path"]), cand)
        if os.path.isfile(f) and (name != "Apache-2.0.txt" or "Apache License" in read(f)):
            shutil.copyfile(f, os.path.join(LIC, name))
            break

# Swift のライブラリと実行環境
pins = json.load(open(os.path.join(REPO, r"server-swift\Package.resolved"), encoding="utf-8"))["pins"]
sw = []
for pin in pins:
    d = os.path.join(REPO, r"server-swift\.build\checkouts", pin["location"].rstrip("/").split("/")[-1].replace(".git", ""))
    files = license_files(d)
    sub = os.path.join(LIC, "swift", pin["identity"])
    os.makedirs(sub, exist_ok=True)
    for f in files:
        shutil.copyfile(os.path.join(d, f), os.path.join(sub, f))
    sw.append(f"{pin['identity']} {pin['state'].get('version') or pin['state'].get('revision', '')[:12]} {pin['location']} ({', '.join(files) or 'ライセンス文なし'})")
section("Swift のライブラリ。ライセンス文は licenses/swift/<名前>/ にあります", "\n".join(sw))
# 2026-11-23: 例外の文は Apache 2.0 の本文 (licenses/Apache-2.0.txt) には無いので、Swift の LICENSE.txt を別に写して例外の文も載せる
_swift_lic = os.path.join(REPO, r"installer\licenses\Swift_LICENSE.txt")
if not os.path.isfile(_swift_lic):
    sys.exit(f"license file missing: {_swift_lic}")
shutil.copyfile(_swift_lic, os.path.join(LIC, "Swift_LICENSE.txt"))
_rle = read(_swift_lic)
_rle = _rle[_rle.index("## Runtime Library Exception"):].strip()
section("Swift の実行環境 (swift*.dll, Foundation*.dll, dispatch.dll, BlocksRuntime.dll, _FoundationICU.dll)",
        "Swift (Apple Inc. and the Swift project authors) — Apache License 2.0 with Runtime Library Exception。\n"
        "全文 (Apache License 2.0 の本文と、下の Runtime Library Exception の文) は licenses/Swift_LICENSE.txt。組み立てに使った Swift 6.4.0 の\n"
        "swiftlang/swift のタグ swift-6.4.0-RELEASE の LICENSE.txt そのままです。同じタグの swift-corelibs-foundation・swift-foundation (Foundation*.dll)、\n"
        "swift-corelibs-libdispatch (dispatch.dll・swiftDispatch.dll・BlocksRuntime.dll)、swift-foundation-icu (_FoundationICU.dll)、\n"
        "swift-experimental-string-processing (swift_RegexParser.dll・swift_StringProcessing.dll) の LICENSE も同じ許諾の文です。\n\n"
        + _rle + "\n\n"
        "https://www.swift.org/legal/license.html\n"
        "_FoundationICU.dll は ICU (Unicode, Inc.) を含みます — Unicode License V3。https://github.com/unicode-org/icu/blob/main/LICENSE")
section("Microsoft Visual C++ ランタイム (msvcp140.dll, vcruntime140*.dll, vcomp140.dll)",
        "Microsoft が再配布を認めているファイルです (Visual Studio の Distributable Code)。https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files")

# 設定アプリ (JavaScript) の本番の依存
fe = os.path.join(REPO, "frontend")
npm = subprocess.run("npm ls --omit=dev --all --json", cwd=fe, capture_output=True, text=True, encoding="utf-8", shell=True).stdout
tree = json.loads(npm or "{}")
js = {}


def walk(deps):
    for name, info in (deps or {}).items():
        # 入っていない任意の依存 (他の OS 用の esbuild など) は除く
        if not info.get("version") or info.get("missing") or not os.path.isdir(os.path.join(fe, "node_modules", name)):
            continue
        if (name, info["version"]) in js:
            continue
        js[(name, info["version"])] = True
        walk(info.get("dependencies"))


walk(tree.get("dependencies"))
jl, jmissing = [], []
for name, ver in sorted(js):
    d = os.path.join(fe, "node_modules", name)
    try:
        lic = json.load(open(os.path.join(d, "package.json"), encoding="utf-8")).get("license") or ""
    except (OSError, ValueError):
        lic = ""
    files = license_files(d)
    # 2026-11-06: パッケージにライセンスのファイルが入っていないもの (@radix-ui の各パッケージ・react-remove-scroll-bar) は、
    # 配布元から取った文 (installer\licenses\javascript\<名前> か <@の付く組の名前>、取り方は installer\licenses\README.md)
    if not files:
        for key in (name.replace("/", "__"), name.split("/")[0] if name.startswith("@") else None):
            extra = os.path.join(REPO, "installer", "licenses", "javascript", key) if key else ""
            if key and os.path.isdir(extra):
                d, files = extra, sorted(os.listdir(extra))
                break
    if not files:
        jmissing.append(f"{name} {ver}")
    if files:
        sub = os.path.join(LIC, "javascript", name.replace("/", "__"))
        os.makedirs(sub, exist_ok=True)
        for f in files:
            shutil.copyfile(os.path.join(d, f), os.path.join(sub, f))
    jl.append(f"{name} {ver} — {lic}")
if not jl:
    sys.exit("no JavaScript dependency notices found; refusing an incomplete release")
section(f"設定アプリの JavaScript のライブラリ ({len(jl)} 個)。ライセンス文は licenses/javascript/<名前>/ にあります",
        "\n".join(jl) + ("\n\nライセンス文のファイルが同梱されていなかったもの (ライセンスは上の表示のとおり):\n" + "\n".join(jmissing) if jmissing else ""))
open(os.path.join(FILES, "THIRD_PARTY_NOTICES.txt"), "w", encoding="utf-8").write(
    "HaniWord は azooKey-Windows をもとにした日本語入力です。以下のソフトウェア・データを含みます。\n\n" + "".join(notice))

# ===== 確かめる =====
problems = []
everything = []
for top in (BIN, FILES):
    for dirpath, dirs, names in os.walk(top):
        everything += [os.path.join(dirpath, name) for name in names]

# 1. 設定・学習・ログ・モデル・TeX 版のファイル
FORBIDDEN_NAMES = {"settings.json", "phrases.tsv", "romaji_style_stats.json", "backend_failed.txt", "value_overrides.tsv",
                   "tex_commands.txt", "latex_workshop_license.txt", "debug_log_path.txt", "install_choices.json", "homophone_declined.txt",
                   "homophone_learned.txt", "hyouki_learned.txt", "user_dictionary.tsv"}
# 2026-11-04: 単語登録 (user_dictionary.tsv) と、設定アプリで書き出した表 (HaniWord_単語登録*.tsv) は個人のデータ
FORBIDDEN_PREFIXES = ("english_terms_personal", "english_learned", "learning_ng", "debug_log", "old_", "compat_log", "haniword_単語登録")
FORBIDDEN_DIRS = {"memory", "shared", ".git", "personal_domains", "client", "webview_test_profile"}
for path in everything:
    rel = os.path.relpath(path, stage)
    parts = {p.lower() for p in rel.split(os.sep)[:-1]}
    name = os.path.basename(path).lower()
    if parts & FORBIDDEN_DIRS or name in FORBIDDEN_NAMES or name.startswith(FORBIDDEN_PREFIXES) \
            or os.path.splitext(name)[1] in {".log", ".gguf", ".pdb", ".bak"}:
        problems.append(f"forbidden file: {rel}")

# 2026-10-22: 分野辞書の置き場所 (files\domain) には、domains.txt・always.txt (と dialects.txt・slang.txt) に並ぶ名前の書き出しだけを入れる。
# リポジトリに無い個人用の分野辞書と、ほかの場所の domain_* は入れない (licenses の出典の一覧は除く)
DOMAIN = os.path.join(FILES, "domain")
listed = set()
for list_name in ("domains.txt", "always.txt"):
    for line in open(os.path.join(DOMAIN, list_name), encoding="utf-8"):
        if line.strip() and not line.startswith("#"):
            listed.add(line.strip())
if listed & {"bio", "world_history"} or not all(re.fullmatch(r"[a-z0-9_]{1,32}", n) and not n[-1].isdigit() for n in listed):
    problems.append(f"unexpected domain names: {sorted(listed)}")
# 書き出した一覧・決め打ちの一覧・設定画面の分野の一覧がそろっているか (名前は数字で終わらない: domain_<名前><番号>.loudstxt3 とぶつからないように)
written = {}
for list_name in ("domains.txt", "always.txt"):
    written[list_name] = {l.strip() for l in open(os.path.join(DOMAIN, list_name), encoding="utf-8") if l.strip() and not l.startswith("#")}
settings_page = read(os.path.join(REPO, r"frontend\src\pages\general.tsx"))
block = re.search(r"const DOMAINS[^=]*=\s*\[(.*?)\n\];", settings_page, re.S)
settings_domains = set(re.findall(r'\["([a-z0-9_]+)",', block.group(1))) if block else set()
if written["domains.txt"] != PUBLISHED_DOMAINS or written["always.txt"] != ALWAYS_DICTIONARIES or settings_domains != PUBLISHED_DOMAINS:
    problems.append(f"domain lists differ: domains.txt {sorted(written['domains.txt'])}, always.txt {sorted(written['always.txt'])}, "
                    f"settings {sorted(settings_domains)}, expected {sorted(PUBLISHED_DOMAINS)} / {sorted(ALWAYS_DICTIONARIES)}")
# 2026-11-06 (仕事 39): 方言の一覧 (dialects.txt)・決め打ちの一覧・設定画面の方言の一覧がそろっているか。名前は分野・追加辞書とぶつからない
dialects_written = {l.strip() for l in open(os.path.join(DOMAIN, "dialects.txt"), encoding="utf-8") if l.strip() and not l.startswith("#")}
block = re.search(r"const DIALECTS[^=]*=\s*\[(.*?)\n\];", settings_page, re.S)
settings_dialects = set(re.findall(r'\["([a-z0-9_]+)",', block.group(1))) if block else set()
if dialects_written != PUBLISHED_DIALECTS or settings_dialects != PUBLISHED_DIALECTS or dialects_written & listed \
        or not all(re.fullmatch(r"[a-z0-9_]{1,32}", n) and not n[-1].isdigit() for n in dialects_written):
    problems.append(f"dialect lists differ: dialects.txt {sorted(dialects_written)}, settings {sorted(settings_dialects)}, expected {sorted(PUBLISHED_DIALECTS)}")
listed |= {"dialect_" + n for n in dialects_written}
# 2026-11-07 (仕事 43): くだけた言葉の一覧 (slang.txt)・決め打ちの一覧・設定画面の切り替えがそろっているか。名前はほかの一覧とぶつからない
slang_written = {l.strip() for l in open(os.path.join(DOMAIN, "slang.txt"), encoding="utf-8") if l.strip() and not l.startswith("#")}
if slang_written != PUBLISHED_SLANG or "i.internet_slang = " not in settings_page or slang_written & (listed | dialects_written) \
        or not all(re.fullmatch(r"[a-z0-9_]{1,32}", n) and not n[-1].isdigit() for n in slang_written):
    problems.append(f"slang lists differ: slang.txt {sorted(slang_written)}, expected {sorted(PUBLISHED_SLANG)} (and the settings switch input.internet_slang)")
listed |= {"slang_" + n for n in slang_written}
# 2026-11-02: よく打つ分野の英字の略語の表 (domain_<名前>_abbrev.txt、6f4c316) も入れてよい
domain_file = re.compile(r"domain_(%s)(\d+\.loudstxt3|\.louds|\.loudschars2|_boost\.tsv|_signals\.txt|_abbrev\.txt)" % "|".join(map(re.escape, sorted(listed))))
for path in everything:
    rel = os.path.relpath(path, stage)
    folder = os.path.dirname(path)
    name = os.path.basename(path)
    if folder == DOMAIN:
        if name not in {"domains.txt", "always.txt", "dialects.txt", "slang.txt", "LICENSE.txt", "LICENSE_extra.txt", "LICENSE_dialect.txt", "LICENSE_slang.txt"} \
                and not domain_file.fullmatch(name):
            problems.append(f"unexpected file in domain: {rel}")
    elif folder.startswith(DOMAIN + os.sep):
        problems.append(f"unexpected folder in domain: {rel}")
    elif name.lower().startswith("domain_") and not (folder == LIC and re.fullmatch(r"domain_(dictionaries_LICENSE|[a-z0-9_]+_wiki(pedia|books))\.txt", name)):
        problems.append(f"forbidden file: {rel}")


# 2. ビルドした PC のアカウント名 (と --forbid の語) が中身に無いか。UTF-8 と UTF-16 の両方で、前に英字が続くもの (ほかの英単語の一部) は除く
def ascii_word(word, utf16):
    sep = b"\x00" if utf16 else b""
    body = sep.join(re.escape(bytes([c])) for c in word.encode("ascii"))
    return re.compile(b"(?<![A-Za-z]" + sep + b")" + body, re.I)


words = [os.path.basename(os.path.expandvars("%USERPROFILE%"))] + args.forbid
patterns = []
for word in words:
    if word.isascii():
        patterns += [(word, ascii_word(word, False)), (word, ascii_word(word, True))]
    else:
        patterns += [(word, re.compile(re.escape(word.encode("utf-8")))), (word, re.compile(re.escape(word.encode("utf-16-le"))))]
for path in everything:
    data = open(path, "rb").read()
    for word, pattern in patterns:
        hit = pattern.search(data)
        if hit:
            problems.append(f"'{word}' in {os.path.relpath(path, stage)} at {hit.start()}: {data[max(0, hit.start() - 40): hit.end() + 40]!r}")
            break


# 3. DLL の依存: 再配布する DLL が同じフォルダ (llama_* なら files も) にあるか・IME 本体は C の実行環境を持つか・
#    この配布物でビルドしたものにデバッグ情報の区画が残っていないか・変換サーバーが llama.dll から使う関数がどちらの版にもあるか
def pe_info(path):
    data = open(path, "rb").read()
    if data[:2] != b"MZ":
        return None
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    machine, nsec = struct.unpack_from("<HH", data, pe + 4)
    opt_size = struct.unpack_from("<H", data, pe + 20)[0]
    opt = pe + 24
    pe32 = struct.unpack_from("<H", data, opt)[0] == 0x10B
    dd = opt + (96 if pe32 else 112)
    exp_rva = struct.unpack_from("<I", data, dd)[0]
    imp_rva = struct.unpack_from("<I", data, dd + 8)[0]
    secs = []
    for i in range(nsec):
        s = pe + 24 + opt_size + 40 * i
        vsize, va, rsize, raw = struct.unpack_from("<IIII", data, s + 8)
        secs.append((data[s:s + 8].rstrip(b"\0").decode("latin-1"), va, max(vsize, rsize), raw))

    def off(rva):
        for _, va, size, raw in secs:
            if va <= rva < va + size:
                return rva - va + raw
        raise ValueError(f"rva {rva:#x} outside the sections of {path}")

    def cstr(rva):
        q = off(rva)
        return data[q:data.index(b"\0", q)].decode("latin-1")

    imports = {}
    if imp_rva:
        p = off(imp_rva)
        while struct.unpack_from("<I", data, p + 12)[0]:
            lookup, name_rva = struct.unpack_from("<I", data, p)[0], struct.unpack_from("<I", data, p + 12)[0]
            funcs = []
            q = off(lookup or struct.unpack_from("<I", data, p + 16)[0])
            width, flag = (4, 1 << 31) if pe32 else (8, 1 << 63)
            while True:
                entry = struct.unpack_from("<I" if pe32 else "<Q", data, q)[0]
                if not entry:
                    break
                if not entry & flag:
                    funcs.append(cstr((entry & 0x7FFFFFFF) + 2))
                q += width
            imports[cstr(name_rva)] = funcs
            p += 20
    exports = set()
    if exp_rva:
        e = off(exp_rva)
        count, names = struct.unpack_from("<I", data, e + 24)[0], struct.unpack_from("<I", data, e + 32)[0]
        exports = {cstr(struct.unpack_from("<I", data, off(names) + 4 * i)[0]) for i in range(count)}
    return {"machine": machine, "imports": imports, "exports": exports, "sections": [s[0] for s in secs]}


REDIST = re.compile(r"swift|foundation|_foundation|llama|ggml|vcruntime|msvcp|vcomp|dispatch|blocks|concrt|vccorlib", re.I)
BUILT = {"azookey.dll", "azookey32.dll", "launcher.exe", "ui.exe", "azookey-settings.exe", "azookey-server.dll", "azookey-server.exe",
         "llama.dll", "ggml.dll", "ggml-base.dll", "ggml-cpu.dll", "ggml-vulkan.dll"}
infos = {}
for path in everything:
    if path.lower().endswith((".dll", ".exe")):
        infos[path] = pe_info(path)
for path, info in infos.items():
    rel = os.path.relpath(path, stage)
    if info is None:
        problems.append(f"not a PE file: {rel}")
        continue
    folder = os.path.dirname(path)
    name = os.path.basename(path).lower()
    here = {n.lower() for n in os.listdir(folder)}
    if name in {"azookey.dll", "azookey32.dll"}:
        if any(re.match(r"vcruntime|msvcp|ucrtbase|api-ms-win-crt", d, re.I) for d in info["imports"]):
            problems.append(f"IME DLL needs the C runtime: {rel}")
        if (name == "azookey32.dll") != (info["machine"] == 0x14C):
            problems.append(f"wrong machine for {rel}: {info['machine']:#x}")
    if os.path.basename(folder) in ("llama_cpu", "llama_cpu_avx2") and any(re.search(r"vulkan|cuda", d, re.I) for d in info["imports"]):
        problems.append(f"the CPU llama needs a GPU loader: {rel}")
    if name in BUILT and any(s.startswith(".debug") for s in info["sections"]):
        problems.append(f"debug sections left in {rel}")
    for dep in info["imports"]:
        if not REDIST.match(dep) or dep.lower() in here:
            continue
        if os.path.dirname(folder) == FILES and os.path.basename(folder).startswith("llama_") and dep.lower() in {n.lower() for n in os.listdir(FILES)}:
            continue
        if folder == FILES and re.match(r"llama|ggml", dep, re.I):
            for backend in ("llama_vulkan", "llama_cpu_avx2", "llama_cpu"):
                lib = os.path.join(FILES, backend, dep.lower())
                if not os.path.isfile(lib):
                    problems.append(f"missing dependency: {rel} -> {backend}\\{dep}")
                    continue
                lacking = set(info["imports"][dep]) - infos[lib]["exports"]
                if lacking:
                    problems.append(f"{backend}\\{dep} lacks {sorted(lacking)[:5]} used by {rel}")
            continue
        problems.append(f"missing dependency: {rel} -> {dep}")

if problems:
    print("\n".join(problems))
    sys.exit(f"{len(problems)} problems; the release was not written (left in {stage})")

# ===== SHA256SUMS.txt と置き換え =====
lines = [f"{sha256(p)}  {os.path.relpath(p, stage).replace(os.sep, '/')}" for p in sorted(everything)]
open(os.path.join(stage, "SHA256SUMS.txt"), "w", encoding="utf-8", newline="\n").write("\n".join(lines) + "\n")
if os.path.exists(args.out):
    os.rename(args.out, args.out + ".before_" + datetime.datetime.now().strftime("%Y%m%d_%H%M%S"))
os.rename(stage, args.out)
size = sum(os.path.getsize(os.path.join(args.out, l.split("  ", 1)[1])) for l in lines)
print(f"ok: {args.out}: {len(lines)} files, {size / 1e6:.1f} MB "
      f"(rust {len(crates)}, missing license files {len(missing)}, swift {len(sw)}, js {len(jl)})")
