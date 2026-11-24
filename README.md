# HaniWord

Windows 用の日本語入力 (IME) です。[azooKey-Windows](https://github.com/fkunn1326/azooKey-Windows) をもとに、ライブ変換を中心に作り込んでいます。
azooKey・azooKey-Windows の公式の版ではありません。不具合や質問は HaniWord の [Issues](https://github.com/supertastybanana3/HaniWord/issues) へお願いします (元の azooKey・azooKey-Windows の開発者には問い合わせないでください)。
AI (ニューラルかな漢字変換のモデル [zenz-v3.2-small](https://huggingface.co/Miwa-Keita/zenz-v3.2-small-gguf)) が、前後の文脈を読んで変換します。変換エンジンは [AzooKeyKanaKanjiConverter](https://github.com/azooKey/AzooKeyKanaKanjiConverter) です。

**HaniWord は、打った文字を PC の外に送りません。** HaniWord の変換・学習・設定は PC の中で行います。通信するのは、インストール中に変換のモデルを取得するときと、1 日 1 回 GitHub で新しい版が出ていないかを確かめるときの 2 つだけです。どちらも、打った文字・学習・設定は送りません (版の確かめは、「HaniWord の設定」の「全般」→「バージョンと更新プログラム」→「新しい版を知らせる」で切れます)。候補の一部に Windows の仕組み (Windows.Data.Text) を使っています。アプリがパスワード欄と知らせた欄では日本語入力を使わず、非公開の欄 (ブラウザの InPrivate など) と知らせた欄で打ったものは学習しません。どちらも、アプリが欄の種類を IME に知らせているときだけです (知らせないアプリの欄は、ふつうの欄と同じに扱います。Chrome のパスワード欄のように非公開の印だけが付いた欄では、日本語入力は使えますが学習はしません)。

> [!WARNING]
> 開発中のソフトです。署名もしていません。安全性や不具合のないことは保証できません。使うときは自己責任でお願いします。

## できること

- **AI による変換**: 同じ読みの語が多くても、文の流れから合うものを選びます。AI は PC の中で動き、GPU があれば GPU で速く変換します。Ctrl + 変換 (または Ctrl + J) で辞書だけの変換に切り替えられます
- **ライブ変換**: 入力しながら自動で変換します。Space で候補の一覧、Enter で確定
- **重いときは AI をオフに**: ほかのアプリの負荷などで AI の変換がふだんよりずっと遅くなったら、自動で辞書だけの変換にし、速さが戻ったら AI に戻します。オフの間は候補の窓の下に「負荷がかかっているためAIがオフになっています」と出ます (初期値はオフで、設定でオンにできます)
- **英語と日本語の自動の打ち分け**: `google`・`windows` のような英単語は、ローマ字にせず英字のまま入れます。`github`・`pdf` は `GitHub`・`PDF` に直します。大文字で始まる語は、その語だけ英字のまま入れます (`Googledekensaku` → Googleで検索)。設定で、大文字を打ったら確定まで英字にもできます (`NHKnonyu-su` → NHKのニュース)
- **次の語の候補**: 英単語を打っている途中に、前の語から続きそうな語を出し、Tab で補います。英数 (A) のときもカーソルの近くに出します (端末・コードエディタ・パスワード欄では出しません)。初期値はオフで、「HaniWord の設定」の「全般」→「英単語の候補」でオンにします
- **アクセント付きの文字**: フランス語・ドイツ語・スペイン語・イタリア語・ポルトガル語・ポーランド語・チェコ語・トルコ語・北欧の言語・ピンインから選んだものについて、キーの長押しで é ñ ü ā などを選べます。選んだ言語 (ピンインを除く) の単語は日本語と見分けて英字のまま入れ、次の語の候補も出します。言語はインストールの「設定を選んでから使う」か設定アプリの「外国語の入力」で選びます (選ばなければ英語だけ)。インストールのあとで設定アプリから選んだ言語は、長押しはすぐ使えますが、その言語の次の語の候補・つづりの直しの表は入っていません。表は、インストーラーをもう一度実行すると入ります (設定で選んである言語の表を入れます)
- **記号の全角・半角**: 全部全角・全部半角・！？～だけ全角、または記号ごとに選べます。¥ キーで入る字 (¥ か \\) も選べます (「ろ」キーはいつも \\)
- **数式・化学式の入力**: `x^2` → x²、`H_2O` → H₂O、`H2O` → H₂O、`アルファ` → α、小数点・桁区切り (`2.0`・`1,000`)
- **Windows の候補も一覧に**: Space の候補の一覧には、Windows に入っている日本語の変換の仕組み (Windows.Data.Text の TextConversionGenerator) の候補も足します。日本語の予測候補 (初期値はオフ) をオンにすると、Windows の予測の仕組み (TextPredictionGenerator) の候補も使います。どちらも Windows の仕組みを使います
- **そのほか**: `zh` `zj` `zk` `zl` で ← ↓ ↑ →、端末の入力待ちでは英字のまま、行頭の `/` `$` `@` はアプリへそのまま、確定した変換の学習 (PC の中だけ)、異体字の切り替え、GPU (Vulkan) での変換と CPU への切り替え

設定はスタートメニューの「HaniWord の設定」で変えられます。

変換の正しさを、ほかの IME と同じ条件で測った数は [変換の正しさ (参考の数)](#変換の正しさ-参考の数) にあります。

## インストール

[Releases](https://github.com/supertastybanana3/HaniWord/releases) から `haniword-setup-<版>.exe` を取って実行してください。

- Windows 10 / 11 (64 ビット)
- インストール中に、変換の AI モデル (約 74MB) を Hugging Face の作者の公開場所から取得し、SHA-256 で確かめます。インターネットにつないでおいてください
- 署名がないので、最初に「Windows によって PC が保護されました」と出ることがあります。出た場合は、「詳細情報」を押すと出る「実行」で進めてください
- 「おすすめの設定ですぐに使う」を選べば、設定のページを飛ばしてすぐに使えます。記号の全角・半角や外国語の入力を選ぶときは「設定を選んでから使う」を選びます
- 専門分野のページは、どちらを選んでも出ます。よく打つ分野 (法律・医学・数学など 11 分野) にチェックを入れると、その分野の話でない文でも用語が出やすくなります。選ばずに次へ進んでもかまいません
- 入れたら Win + Space で「HaniWord」を選び、半角/全角 キー (US 配列なら Alt + \`) で「あ」にして打ちます

### はじめの 10 分

1. Win + Space で「HaniWord」を選び、半角/全角 キー (US 配列なら Alt + \`) で「あ」にします
2. `kyouhaiitennkidesune` と打ちます。打っている間に「今日はいい天気ですね」と変換されていきます
   (入れた直後と PC を起動した直後は、最初の変換に数秒かかります)
3. Enter で確定します
4. 変換を選び直すときは Space で候補の一覧を出し、↑ ↓ で選んで Enter で確定します
5. Esc を 1 回押すと読み (ひらがな) に戻り、もう 1 回押すと打った分を消します
6. `kyouhagoogledekensaku` と打つと、google は英字のまま入ります。
   英字になった所を かな にしたいときは、無変換キー (または F6 でひらがな、F7 でカタカナ) で戻せます (ローマ字として読める綴りのときだけ)
7. 設定を変えたら、試すアプリ (メモ帳など) を一度閉じて開き直してください。
   開いたままのアプリには、効かない設定があります

### キーの一覧 (HaniWord だけのもの)

| キー | すること | US 配列など 変換・無変換 キーの無いキーボード |
|---|---|---|
| 半角/全角 | あ／A の切り替え | Alt + \` (1 の左のキー)。設定で Ctrl + Space も選べます (初期値オフ) |
| Ctrl + 変換 | AI の変換 ↔ 辞書だけの変換 (入力中・変換中) | Ctrl + J |
| Ctrl + 無変換 | 表記の切り替え H₂O ↔ H2O (入力中・変換中) | Ctrl + B |
| Ctrl + Shift + 変換 | 異体字を回す (変換中の 1 字の漢字) | Ctrl + Shift + J |
| Esc | 変換の表示を読み (ひらがな) に戻す。もう 1 回で打った分を消す | 同じ |
| 無変換 | 英字になった所を かな に戻す (ローマ字として読める綴りのときだけ) | F6 でひらがな、F7 でカタカナ (JIS 配列でも同じ) |
| Ctrl + Delete | 候補の一覧で選んでいる候補が学習した語なら、その学習を忘れる (候補の窓に「Ctrl+Delete で忘れる」と出ている候補。辞書の語は消えません) | 同じ |

- 右の列のキーは、JIS 配列でも同じように使えます。Ctrl + J・Ctrl + B は、入力中・変換中で読みがあるときだけ HaniWord が受け、ほかのときはアプリへそのまま渡します
- Alt + \` は、キーの位置 (1 の左のキー) で決めています。JIS 配列の Alt + @ は、今までどおりアプリへ渡します
- キーを割り当て直す道具で、IME オン (VK_IME_ON) のキーを押すと「あ」、IME オフ (VK_IME_OFF) のキーを押すと「A」になります (左右の Alt などに割り当てる人向け)

### 制限

- 署名がないため、「スマート アプリ コントロール」がオンの PC ではブロックされます。利用のために Windows の保護機能を無効にしないでください
- ユーザー アカウント制御 (UAC) をオフにした PC と、組み込みの Administrator アカウントではインストールできません。管理者ではないアカウントに、別の管理者のパスワードを入れてインストールすることもできません (HaniWord を使う管理者のアカウントで入れてください)
- GPU (Vulkan) があると速く変換できます。GPU で動かないときは CPU に切り替えますが、遅くなります。最初の変換だけ数秒かかります
- GPU (Vulkan) で動かすときは、変換サーバーのメモリが約 250MB で、CPU で動かすとき (約 100MB) より多くなります。少なくしたいときは、「HaniWord の設定」の「Zenzai」→「バックエンド」で「CPU (非推奨)」に切り替えられます (変換は遅くなります)
- 登録するのはライブ変換の入力方式だけです (Space で変換する方式は入っていません)
- US 配列 (英語キーボード) を使うときは、Windows の言語の設定で、日本語の「ハードウェア キーボード レイアウト」を「英語キーボード (101/102 キー)」に変えてください (変えたあとは再起動が要ります。HaniWord はこの設定を変えません)。英語版の Windows などで、インストールのときに日本語を新しく足したときは、インストーラーが同じことを案内し、言語の設定を開けます。あ／A の切り替えと 変換・無変換 の代わりのキーは、上の「キーの一覧」のとおりです
- 不具合を調べるための診断ログは、`%APPDATA%\HaniWord\debug_log_path.txt` (書き出すファイルのパスを書いたもの) を置いたときだけ書きます (ふだんは書きません)。有効にすると、打った文字と窓の題がそのファイルに残ります。書き出すファイルは `%APPDATA%\HaniWord` の中だけに置けます。人に渡す前に中身を確かめ、要らなくなったら debug_log_path.txt とログのファイルを消してください

### アンインストール

設定 → アプリ → インストールされているアプリ →「HaniWord」。

アンインストールしても、あなたのフォルダ `%APPDATA%\HaniWord` の中は消えません (管理者の権限で動くアンインストーラーに、利用者のフォルダの中を消させないため)。残るものは次のとおりです。

- 学習データと設定
- 変換サーバーと辞書、変換の AI モデル (約 74MB)
- 学習を消したときの控え (`memory_backup_<日時>` のフォルダ。「HaniWord の設定」で学習を消す前に、消すものを写したもの)
- 壊れていた学習のファイル (`memory_corrupt_<日時>` のフォルダ。停電などで途中で切れた学習を読めなかったときに、別に退けたもの。学習は空から始め直します)

要らなければ、アンインストールのあとで、エクスプローラーのアドレス欄に `%APPDATA%` と入れて開き、`HaniWord` フォルダを消してください。消すと元に戻せません。
単語登録の「書き出す」で書いたファイル (ドキュメントのフォルダ) など、自分で別の場所に保存したものは、このフォルダの外にあるので残ります。要らなければ別に消してください。

## 変換の正しさ (参考の数)

正しい読みをまとめて与え、変換の 1 番目の候補が正しい文と同じになった文の割合です (2026 年 11 月に測りました)。
かっこの中は、送りがなや かな と漢字の書き分けなど、表記の違いだけの文も正しいとしたときの割合です。
使い方 (ローマ字で打ちながらの変換、区切りの直し、学習) で数は変わります。目安として見てください。
表の「元の azooKey」の行は、同じ AI のモデルを使ったときの比べのもとです。

### 新しい文 1,088 文

HaniWord の辞書や設定を決めるときに一度も使っていない文です。
2026 年に公開された文 (ウィキニュース・ウィキブックス・消費者庁の報道発表) と、AI に場面を渡して書かせた文 (くだけた会話・メール・学校・暮らし・英字まじり) です。

| | 全部 (1,088 文) | 英字・数字を含まない文 (756 文) | 下の Mozc と同じ文 (745 文) |
|---|---|---|---|
| HaniWord 0.1.0 (AI あり) | 73.4% (86.3%) | 74.1% (89.7%) | 74.5% (90.2%) |
| HaniWord 0.1.0 (AI なし) | 47.9% (62.2%) | 49.7% (67.1%) | 50.3% (67.9%) |
| 元の azooKey (変換エンジン、上限 3) | 72.2% (84.9%) | 73.0% (88.8%) | 73.6% (89.5%) |
| 元の azooKey (変換エンジン、上限なし) | 75.4% (88.3%) | 74.1% (89.9%) | 74.6% (90.6%) |
| Microsoft IME | 61.3% (70.2%) | 63.4% (74.9%) | 63.9% (75.6%) |
| Google 日本語入力 | ― | ― | 60.9% (76.0%) |
| OSS Mozc | ― | ― | 58.7% (72.8%) |

場面ごと (全部の文、厳しい一致):

| | くだけた会話 (200) | メール (150) | 学校 (149) | 暮らし (99) | 英字まじり (99) | ウィキニュース (127) | ウィキブックス (127) | 消費者庁 (137) |
|---|---|---|---|---|---|---|---|---|
| HaniWord 0.1.0 (AI あり) | 84.0% | 84.7% | 75.8% | 62.6% | 87.9% | 64.6% | 65.4% | 56.2% |
| HaniWord 0.1.0 (AI なし) | 56.0% | 69.3% | 48.3% | 37.4% | 66.7% | 37.0% | 32.3% | 30.7% |
| Microsoft IME | 74.0% | 79.3% | 69.1% | 56.6% | 81.8% | 53.5% | 43.3% | 27.0% |

Google 日本語入力と OSS Mozc は、読みに英字・数字・記号を含む文を流していない (745 文だけ) ので、場面ごとの表には入れていません。

### 参考: Wikipedia の文 3,675 文

HaniWord の辞書や設定を決めるのに使った文なので、参考の数です。百科事典の長い文が多く、どの IME も上の新しい文より正しい割合は低く出ます。

| | 全部 (3,675 文) | 英字・数字を含まない文 (2,694 文) |
|---|---|---|
| HaniWord 0.1.0 (AI あり) | 64.7% (75.0%) | 63.0% (74.3%) |
| HaniWord 0.1.0 (AI なし) | 39.9% (48.3%) | 39.2% (48.5%) |
| 元の azooKey (変換エンジン、上限 3) | 62.7% (72.2%) | 60.5% (70.7%) |
| 元の azooKey (変換エンジン、上限なし) | 64.5% (74.3%) | 62.2% (72.8%) |
| Microsoft IME | 44.7% (52.0%) | 42.3% (50.2%) |
| OSS Mozc | ― | 37.0% (45.7%) |

### 測り方

- 正しいとするのは、1 番目の候補が正しい文と字まで同じときです (全角と半角、前後の空白はそろえます)。
  かっこの中は、送りがなの違い (行う／行なう)、かな と漢字の書き分け (すべて／全て、言う／いう。どちらの向きも)、漢数字が算用数字で出た所 (二つ → 2つ)、化学式などの下付き・上付きの数字 (CO2 → CO₂、m2 → m²) だけが違う文も正しいとしました。
  漢字の語が かな で出た所と、カタカナの語が漢字で出た所は、形態素解析 (Sudachi) の辞書で同じ語の書き分けとされる組だけです (語彙 → ごい、カキ → 夏季 のような所は正しいとしていません)。どの IME にも同じ数え方を当てます。
- どの組も 1 文ずつ変換しました (前の文を文脈として渡していません)。
- 新しい文 1,088 文は、HaniWord の辞書・規則・重みを決めるときに見た文とは別の文です。調整には一度も使っていません。
- HaniWord の「AI なし」は、AI ありの設定のまま、入力中の仮表示に使う辞書だけの変換の値です。設定で AI を切ったときの Space の変換とは、違うことがあります。
- ほかの IME と並べた数は、どの IME も同じ条件で測りました。学習も利用者の辞書も無い状態で、読み全体を 1 回変換したときの 1 番目を取り、確定はしていません。
  Google 日本語入力と OSS Mozc は、読みをローマ字で打って測るので、読みに英字・数字・記号を含む文は打てません。そのため、どの IME も比べられるよう、それらを含まない 745 文の列を設けました。
- 読みは、2 つの形態素解析 (Sudachi・UniDic) の読みが合ったものを使い、割れた所は、日付・曜日・英字などの決まった形は機械的にそろえ、残りは AI が前後の文を見て決めました。決めきれない文 (9 文) は外しました。書き手 (AI) には読みを付けさせていません。
- HaniWord の AI ありは、GPU (Vulkan) で測りました。同じ版で同じ文を 2 回流したとき、結果が変わった文はありませんでした (5,607 文で 0。PC の負荷が違っても同じ)。ほかの PC・GPU では確かめていません。
- Microsoft IME は、Windows に付いているものです (2026-11-22 に測りました)。1 番目の候補は IFELanguage という仕組みで取りました (画面の候補の 1 番目と同じとは限りません)。クラウドの候補を切り、学習も利用者の辞書も無い、まっさらな利用者で測りました。
- Google 日本語入力は、製品の版です (2026-11-22 に測りました)。学習も利用者の辞書も無い、まっさらな利用者で、自前の入力欄に読みをローマ字で 1 字ずつ打ち、Space を 1 回押したときの変換を取りました (確定はしていません)。読みに英字・数字・記号を含む文は流していません (下の OSS Mozc と同じ 745 文)。
- 測った PC: Intel の CPU と内蔵 GPU のノート PC で測りました。ほかの PC・GPU で同じ数になるかは確かめていません (速さは PC によって変わります)。
- OSS Mozc (2.28、2026-11-23 に測りました) は、製品の Google 日本語入力とは辞書が違います。学習の無い状態で、読みをローマ字に直して打ち、Space を 1 回押したときの変換を取りました。読みに英字・数字・記号を含む文は流していません。
- 元の azooKey は、azooKey-Windows が使っていた変換エンジン [AzooKeyKanaKanjiConverter](https://github.com/azooKey/AzooKeyKanaKanjiConverter) の版 7d5dd99 を、付属のコマンドで直接動かして測りました (azooKey-Windows のアプリそのものではありません)。AI のモデルは HaniWord と同じ zenz-v3.2-small、辞書はエンジンの既定のもの (azooKey_dictionary_storage の版 b05798b) です。「上限」は、AI が変換を見直す回数の上限で、HaniWord の初期値は 3 です。
- 新しい文 1,088 文そのものは、次の版を測るために公開していません。そのため、外の人がこの数を数え直すことはできません。

## ビルド

要るもの: Visual Studio 2022 (C++)・[Rust](https://www.rust-lang.org/tools/install) (`x86_64-pc-windows-msvc` と `i686-pc-windows-msvc`)・[Swift for Windows](https://www.swift.org/install/windows/) 6.4・[protoc](https://protobuf.dev/installation/)・[Node.js](https://nodejs.org/)・CMake と Ninja・[Vulkan SDK](https://vulkan.lunarg.com/)・Python 3・[Inno Setup 6](https://jrsoftware.org/isinfo.php)

```powershell
git clone --recursive https://github.com/supertastybanana3/HaniWord
git clone https://github.com/ggml-org/llama.cpp; git -C llama.cpp checkout c35b667
cd HaniWord
.\scripts\release\build.ps1 -LlamaSource ..\llama.cpp        # llama.cpp・変換サーバー・IME 本体・設定アプリ
py -3 scripts\release\package.py --llama-source ..\llama.cpp --next-words <次の語の表のフォルダ> --colloc <組み合わせの表のフォルダ>   # release\ に配布物一式
.\installer\compile.ps1                                       # release_installer\haniword-setup-0.1.0.exe
```

- llama.cpp には `patches/llama.cpp-zenz.patch` (zenz のトークナイザーの扱い) を当てます。`build.ps1` が写したほうに当てます
- `package.py` の `--llama-source`・`--next-words`・`--colloc` はどれも要ります
- 次の語の候補の表 (`next_*.hwn` 14 個) は大きいので、リポジトリには入れていません。インストーラーから取り出すか、`tools/next_words` の手順で作ってください
- 同音の語の組み合わせの表 (`colloc_choices.tsv`、約 30MB) も同じく、リポジトリには入れていません。インストーラーから取り出すか、`tools/colloc` の手順で作ってください
- 2 つの表は、それぞれのフォルダに、その表だけの SHA-256 を書いた `SHA256SUMS.txt` を置きます (`package.py` が中身と照らし合わせます。配布物の全体の `SHA256SUMS.txt` は使えません)。PowerShell では、表のフォルダで次のように作れます (`colloc` のフォルダでは `next_*.hwn` を `colloc_choices.tsv` に)
  ```powershell
  Get-ChildItem next_*.hwn | ForEach-Object { "$((Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower())  $($_.Name)" } | Set-Content -Encoding ascii SHA256SUMS.txt
  ```
- `package.py` は、もとの azooKey-Windows の LICENSE を履歴のコミット 65835aa から読みます。`--depth` を付けずに clone してください
- 開発中に IME を入れ替えるときは、仮想マシンか専用の PC を勧めます。IME が落ちると入力中のアプリが固まることがあります

## 開発について

- 個人が作っています。開発には AI のコーディング支援 (Anthropic の Claude) を使っています
- サポートはありません。Issues は読みますが、返事や直すことは約束できません。不具合の報告は Issues の雛形に沿って書いてください
- 新しい版が出たら、Windows の通知で知らせます (1 日 1 回 GitHub で確かめます)。入れるのは自動ではなく、自分で新しいインストーラーを実行します。新しい版は Releases に置きます。
  知らせが要らなければ、「HaniWord の設定」の「全般」→「バージョンと更新プログラム」→「新しい版を知らせる」をオフにします (`%APPDATA%\HaniWord\settings.json` の `notify_updates`)
- 変換サーバーが使うメモリは、GPU (Vulkan) のときで約 250MB、CPU のときで約 100MB です (2026 年 11 月の実測。タスク マネージャーの「メモリ」の列の数)

## ライセンス

HaniWord のプログラム (ソースコード) は [MIT License](LICENSE) です。**辞書・単語表などのデータは MIT ではなく、表ごとのライセンス (多くは CC BY-SA 4.0) です。** リポジトリの中のどの表が何かは [DATA_LICENSES.md](DATA_LICENSES.md) にまとめています。
配布物に含まれるソフトウェア・データのライセンスと出典は、インストール後の `%APPDATA%\HaniWord` にある `THIRD_PARTY_NOTICES.txt` と `licenses` フォルダにあります。おもなもの:

- [azooKey-Windows](https://github.com/fkunn1326/azooKey-Windows) (fkunn1326) — MIT License
- [AzooKeyKanaKanjiConverter](https://github.com/azooKey/AzooKeyKanaKanjiConverter) (Miwa / Ensan) — MIT License
- [azooKey の辞書](https://github.com/ensan-hcl/azooKey_dictionary_storage) — Apache License 2.0
- 専門用語の辞書 (11 分野、CC BY-SA 4.0): e-Gov 法令検索・国土地理院「地名集日本」(公共データ利用規約 第 1.0 版)、NBDC の科学技術用語辞書・情報処理学会の用語リスト・ウィキペディア・ウィキブックス (CC BY-SA 4.0)、万病辞書 (CC BY 4.0)、Wikidata (CC0)、[JMdict](https://www.edrdg.org/edrdg/licence.html) (CC BY-SA 4.0) などから作成
- 辞書の抜けを補う追加の辞書: [Mozc](https://github.com/google/mozc) の公開辞書 (IPAdic の許諾)、JMdict・KANJIDIC2 (CC BY-SA 4.0)、ウィキペディア日本語版 (名前・製品名・役割語。名前は 2023-11-01 のダンプから、下の注)、Wikidata (CC0。上場企業の一覧)、[RealPersonaChat](https://github.com/nu-dialogue/real-persona-chat)・ウィクショナリー日本語版 (CC BY-SA 4.0。くだけた一言) から作成
- 方言の言い方の辞書・ネットスラングの辞書 (設定でオンにした人だけが使う。CC BY-SA 4.0): ウィキペディア日本語版の記事から作成
- 注意の注記の表 (設定でオンにした人だけが使う。CC BY-SA 4.0): ウィキペディア日本語版の記事 (CC BY-SA 4.0)・ライセンスの書かれた NG 語の一覧 (MIT License・Apache License 2.0)・放送や新聞などの記事と手引き (参考文献の一覧は配布物の licenses\caution_references.txt) が挙げている語を選び、注記の文は HaniWord の開発者が書いたもの
- よくある誤用の表 (CC BY-SA 4.0): 文化庁「国語に関する世論調査」・文化審議会国語分科会「「異字同訓」の漢字の使い分け例」(文部科学省ウェブサイト利用規約) とウィキペディア「日本語の誤用」から作成
- [zenz-v3.2-small](https://huggingface.co/Miwa-Keita/zenz-v3.2-small-gguf) (Miwa-Keita) — インストール時に取得 (同梱していません)。インストーラーが取る版 (c67e03e) の[モデルカード](https://huggingface.co/Miwa-Keita/zenz-v3.2-small-gguf/blob/c67e03e07d215c869f591b274c1631170d3e11fe/README.md)は Apache-2.0、[今のモデルカード](https://huggingface.co/Miwa-Keita/zenz-v3.2-small-gguf/blob/main/README.md)は CC BY-SA 4.0 と表示しています。HaniWord は、厳しいほうの CC BY-SA 4.0 として扱います
- [llama.cpp](https://github.com/ggml-org/llama.cpp) — MIT License
- 英単語の表: [JMdict](https://www.edrdg.org/edrdg/licence.html) (CC BY-SA 4.0)・CMUdict・[SCOWL](https://github.com/en-wl/wordlist)
- 英字の表記の表: 社名・ブランドと、人・地名・組織の英語の名前は Wikidata (CC0)、ソフト・アプリ・サービスの製品名はウィキペディア日本語版の記事の題 (CC BY-SA 4.0) から作成
- 英語以外の単語表: [wordfreq](https://github.com/rspeer/wordfreq) (CC BY-SA 4.0)
- 同音の語の組み合わせの表 (CC BY-SA 4.0): ウィキペディア日本語版 (2023-11-01 のダンプ。元は CC BY-SA 3.0 / GFDL、下の注) と[青空文庫](https://www.aozora.gr.jp/)で、同じ読みの語と前後の語の組を数えて作成 (語の切り方は [SudachiDict](https://github.com/WorksApplications/SudachiDict)、Apache License 2.0)
- 次の語の候補の表 (CC BY-SA 4.0): [FineWeb](https://huggingface.co/datasets/HuggingFaceFW/fineweb)・[FineWeb-2](https://huggingface.co/datasets/HuggingFaceFW/fineweb-2) (ODC-By 1.0) と [Tatoeba](https://tatoeba.org) (CC BY 2.0 FR) から作成

ウィキペディア日本語版の 2023-11-01 のダンプ (Hugging Face の [wikimedia/wikipedia](https://huggingface.co/datasets/wikimedia/wikipedia) の 20231101.ja) から作った表があります (同音の語の組み合わせの表・追加の辞書の人の名前など。どの表かは [DATA_LICENSES.md](DATA_LICENSES.md) と各ファイルの頭の行)。このデータは CC BY-SA 3.0 / GFDL です。[CC BY-SA 3.0](https://creativecommons.org/licenses/by-sa/3.0/legalcode) の 4(b) は、翻案を同じ許諾要素 (表示・継承) の後の版のライセンスで配ることを認めているので、これらの表は CC BY-SA 4.0 で配布します (元は CC BY-SA 3.0 / GFDL のダンプ)。

azooKey-Windows とその参考資料 ([Mozc](https://github.com/google/mozc)・[khiin-rs](https://github.com/OMAMA-Taioan/khiin-rs)・[ajemi](https://github.com/dec32/ajemi)・[Windows-classic-samples](https://github.com/microsoft/Windows-classic-samples) ほか) に感謝します。
