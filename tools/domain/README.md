# 分野辞書の作り方 (tools/domain)

分野辞書は、専門の文章でよく使う語 (表記と読み) を、標準の辞書より控えめな重みで変換サーバーに足すものです。
配布物では `%APPDATA%\HaniWord\domain\` に `domain_<名前>.louds` ほかと `domains.txt` を置き、`domains.txt` に並ぶ分野は初期値でオン、
設定 `input.domains_off` (設定アプリの「専門用語の辞書」) に入れた分野だけ使いません。

## データ

- `domain_dictionaries/<名前>.tsv` — 分野辞書の中身 (リポジトリに入れるのはこれだけ)。列: 表記 / 読み (カタカナ) / 品詞の番号 / 意味の番号 / 値 / 出典
- `build_domain.py --out <置き場所>` — 上の TSV から変換サーバーが読む形と `domains.txt` を書き出す (配布物を作るときに呼ぶ)
- `extra_dictionaries/core_<名前>.tsv` — オフにできない追加辞書 (辞書の抜け M3.5: 活用形 core_forms・成句 core_phrases・語 core_words)。列は分野辞書と同じ。
  `build_domain.py` が同じ形 (`domain_core_<名前>.louds` ほか) で書き、名前を `always.txt` に並べる。変換サーバーは `always.txt` の辞書を
  `input.domains_off` に関係なく読む (試験用のビルドでは環境変数 `AZK_ALWAYS_CORE_<名前>=0` で 1 つずつ外せる)。
  ライセンス文は `extra_dictionaries/LICENSE.txt` (辞書の隣に `LICENSE_extra.txt`、`--licenses` の置き場所に `extra_dictionaries_LICENSE.txt`)

## 手順 (どの分野も同じ)

1. **語を集める** — 分野ごとの `collect_*.py`。名詞の連なりは Sudachi で切り出す (`common.chains`。人名・地名・数・指示語は除く)。
   集めた語ごとに、出てきた文書の数と出どころ (法令の見出し・定義語・判決の判示事項など) を記録する。
   効果を測る記事 (リポジトリの外の測りの組) は使わない (`collect_wikipedia.py --holdout`)。
2. **今の辞書で既に正しく変換できる語を捨てる** — `draft_readings.py` で仮の読み (e-Gov の読み > 変換器の辞書の読み > Sudachi の読み) を付け、
   `convert.py` で変換サーバーの DLL に渡す。その語が **AI ありでも辞書だけでも** そのまま出れば捨てる。仮の読みは捨てる判定にだけ使い、辞書には入れない。
   (2026-11-06 まで AI ありだけを見ていて、辞書だけでは外れる語 (体循環 → 帯循環) を捨てていた。ライブ変換の短い入力は辞書だけで変換し、文の中では AI も外す)
3. **luna に回す語を選ぶ** — `select_terms.py`。同じ表記が辞書に既にあり、変換すると同じ読みの一般の語が出るもの (施行→思考) は捨てる
   (重くすると一般の変換を壊し、軽く足しても変わらない)。分野の語らしさの手がかり (文書の数・出どころ) が足りない語も捨てる。
4. **読みを付ける** — `luna_readings.py`。GPT-6 luna に束の組み方を変えて 2 回 (a・b) 付けさせ、その分野の用語かも判定させる。
   2 回の読みが食い違う語・自信なしの語・ウィキペディアなどの読み (`reference_readings.py`) と合わない語は、3 回目 (c) で Web 検索
   (日本語版ウィキペディアの冒頭の読みを優先) をさせて決める。形態素解析の読みは使わない。
5. **重みを決めて辞書にする** — `assemble_domain.sh` (`make_domain_tsv.py` → `calibrate.py`)。表記はそろえてから使う (`common.normalize_term`:
   全角英数は半角、ハイフンの類は `-`、括弧・読点入りは入れない)。今の変換結果が意味の通らない切れ端 (胃尿地・即時広告) なら分野の語が勝つ重み、
   自然な日本語 (献血・各遺伝子) ならそれより下 (候補には出るが 1 位は今のまま)。作ったあと全部の語を変換し直して確かめ、外れた語だけ重みを動かす。
   「今の変換」は、AI ありで出なければ AI ありの結果、AI ありでは出て辞書だけで外れれば辞書だけの結果 (`make_domain_tsv.py` の `current_of`。2026-11-06 まで
   AI ありで出れば入れなかった)。AI ありでは出て辞書だけの結果も自然な別の語 (かがくしんか → 科学進化) なら入れない
   - 交ぜ書きと漢字の書き方が同じ読みで並ぶ語 (側わん/側弯・木ぐい/木杭) は、`prefer_kanji.py` で漢字を上・交ぜ書きを 2 番目にする (2026-10-31。
     医学・建築・化学・歴史と、生物・宗教の一部。地学 (花こう岩・かんらん石) と数学 (べき根) は教科書の書き方が交ぜ書きなので当てない)
6. **同音語の優遇のデータ** (変換サーバーの DomainBoost。文脈に分野の目印の語があれば、同じ読みの分野の語を候補に足して少し加点する)
   - `<名前>_signals.txt` — 目印の語。`signal_candidates.py` (分野の記事の冒頭に多く出る語) などで候補を作り、`luna_signals.py` で
     「その分野に特有で一般の文章にはあまり出ない語」だけを 6.1-sol に選ばせる
   - `<名前>_boost.tsv` — 変換器の辞書に既にある分野の語 (表記<TAB>読み)。3. で捨てた「同じ読みの一般の語が出る語」を luna に回して分野の語と判定されたものと、目印の語。
     `make_boost.py` が書く。同じ読みで漢字の一部をかなにした書き方 (タンパク質) のほうがよく使われる語 (蛋白質) は優遇しない
   - 2026-10-31: 医学も同じ手順で作った (同音で捨てた 657 語を `pipeline_readings_lite.sh` で判定)。束の決まりは `rules/<名前>.txt` に
     「この束だけ」の行 (一般の変換の語と意味が違えば入れる・かな書き (げり/下痢) ならその分野でふつう使う書き方か・人名などの固有名詞は入れない) を足したもの。
     測った文で悪くした語は `<名前>_exclude.txt` に書けば優遇から外れる (目印には残る)
   - `build_domain.py` が分野辞書の語と合わせて `domain_<名前>_boost.tsv`・`domain_<名前>_signals.txt` を書く
7. **効果を測る** — `eval_sets.py cases` で測りの文の組 (リポジトリの外) を読み→変換の形に直し、`run_eval.sh` で分野辞書のあり・なしを変換して
   `eval_sets.py compare` で比べる。分野の文で上がり、一般の文 (確認用 207 文・全 3,675 文) で下がらないこと。
   **組の 3 列目 (文脈) は明示したときだけ使う** (2026-11-06): `convert.py` は `--context` を付けたときだけ 3 列目を左の文脈として渡す。
   `run_eval.sh` と `extra_dict/run_always_eval.sh` は、組の名前が `*.cases.tsv` (`eval_sets.py cases` が作る文の組。前の文を 3 列目に持つ) なら付け、
   `EVAL_CONTEXT=1` で付ける・`EVAL_CONTEXT=0` で付けない。語の試しの塊 (`draft_readings.py` の 読み<TAB>表記<TAB>出どころ) の 3 列目は文脈ではない。
   それまでは 3 列目をいつも文脈に渡していて、語の試しは egov・sudachi などの出どころの字を前に置いて変換していた
8. **英字の略語の表** (2026-11-01) — `abbrev.py`。`<名前>_abbrev.txt` (1 行 1 語、表記そのまま) は、よく打つ分野に選んだ人の IME 本体が
   英字の判定の語の表に足す (mRNA・NSAIDs・DBMS を英字のまま)。分野の記事の冒頭と JMdict の英字の見出しから拾い (いちばん多く出る分野の半分以上・
   5 分野以上に広く出る一般の略語 (JR・DNA) は除く)、6.1-sol で分野の略語かを判定し、小文字でローマ字として読める語 (TCA・NATO・GUI) と
   今の英字の判定の表にある語を除く。拾った記事は `<名前>_abbrev_sources.tsv` に書き、`build_domain.py --licenses` が記事の一覧に入れる

## 分野辞書のライセンスと出典

分野辞書のファイル (domain_dictionaries/*.tsv と、そこから作る domain/domain_*.*) は、プログラム (MIT) とは別に **CC BY-SA 4.0** で配布する
(ウィキペディアの題名と NBDC の辞書を素材に含むため、継承の条件がかかる)。ライセンス文と出典は `domain_dictionaries/LICENSE.txt`。
`build_domain.py` が辞書の隣に `LICENSE.txt` として、`--licenses` の置き場所に `domain_dictionaries_LICENSE.txt` と
ウィキペディアの記事の一覧 `domain_<名前>_wikipedia.txt` を書く。

| 材料 | 配布元のページ (2026-10-20 に開いて確認) | ライセンス | 出典の書き方・加工の記載 | 使い方 |
|---|---|---|---|---|
| e-Gov 法令検索の法令データ (法律) | https://laws.e-gov.go.jp/terms/ (利用規約。画面は JavaScript なので assets/terms-*.js の文言で確認)、https://www.digital.go.jp/resources/open_data/public_data_license_v1.0 | 公共データ利用規約 第 1.0 版 (PDL1.0)。CC BY 4.0 と互換 (規約の本文) | 「出典：e-Gov法令検索（URL）、PDL1.0（規約の URL）」。編集・加工したときは、そのことと加工した主体も書く | 法令の本文から語・見出し・定義語・法令名と読みを抜き出す (加工) |
| 裁判所ウェブサイトの裁判例 (法律) | https://www.courts.go.jp/outline/index.html (このサイトについて)、robots.txt | 判決は著作権法 13 条 3 号で権利の目的とならない (e-Gov の著作権法の本文で確認)。サイトの内容は無断改変不可とある | 語を選ぶ手がかりにだけ使い、文は配布しない | どの語を入れるかの判断 (判決の数) |
| 日本語版ウィキペディア (法律・生物・歴史・医学) | https://ja.wikipedia.org/wiki/Wikipedia:ウィキペディアを二次利用する | CC BY-SA 4.0 (と GFDL) | 記事の URL で表示 (記事の一覧のファイル)。改変 (読みを付け直した) を書く | 記事の題名と冒頭の読み |
| 科学技術用語形態素解析辞書 (NBDC、JST シソーラス 2015) (生物・医学) | https://dbarchive.biosciencedbc.jp/jp/mecab/lic.html | CC BY-SA 4.0 (翻案物も CC BY-SA 4.0 以降で配布) | クレジット「科学技術用語形態素解析辞書 © バイオサイエンスデータベースセンター licensed under CC表示-継承4.0 国際」を必ず表示 | 主題カテゴリーで語を選ぶ。同義語の読みは使わない |
| Wikidata (歴史・医学) | https://www.wikidata.org/wiki/Wikidata:Licensing | CC0 | 表示の義務なし (出典には書く) | 人物・出来事・病気・薬などの日本語のラベルと名前のカナ表記 (P1814) |
| JMdict (EDRDG) (情報・化学・地学・建築・仏教神道) | https://www.edrdg.org/edrdg/licence.html (2026-10-22 に開いて確認)、https://www.edrdg.org/jmdict/j_jmdict.html | CC BY-SA 4.0 | 文書・配布元のページで、使ったことと出典を示し、ライセンス文の写しかその場所へのリンクを付ける。**最新の版から定期的に更新する手順が要る** (4 節 "there must be a procedure for regular updating of the data from the most recent versions available"。古いままはライセンス違反) → 下の「JMdict の版を新しくする」 | 分野の印 (comp・chem・geol・astron・min・met・geogr・archit・Buddh・Shinto) の付いた語を候補にする (`jmdict_candidates.py`)。読みは luna で付け直す |
| 万病辞書 MANBYO_202106 (NAIST ソーシャル・コンピューティング研究室) (医学) | https://sociocom.naist.jp/manbyou-dic/ | CC BY 4.0 (ページの表示「本成果物は クリエイティブ・コモンズ 表示 4.0 国際 ライセンス の下に提供されています」) | 著作者・題名・URL・ライセンスを表示し、改変を書く | 信頼度 S の行 (ICD10 対応標準病名マスター＝MEDIS 由来。MEDIS は配布目的の利用に許諾の申請が要る) と標準病名・ICD コードの列は使わない。頻度が下位 5% の行も使わない |

分野ごとの道具: 法律 `collect_egov.py`・`collect_courts.py`・`collect_court_terms.py`・`collect_wikipedia.py`、
生物 `jst_candidates.py`・`collect_wikipedia.py`、歴史 `collect_wikidata.py`・`wikidata_candidates.py`・`collect_wikipedia.py`、
医学 `jst_candidates.py`・`manbyo_candidates.py`・`collect_wikidata.py`・`collect_wikipedia.py`、
情報・化学・地学・建築・仏教神道 `jmdict_candidates.py` (ほかに `jst_candidates.py`・`collect_wikipedia.py` など)。

## JMdict の版を新しくする

JMdict のライセンス (https://www.edrdg.org/edrdg/licence.html の 4 節) は、データを入れたソフトに、最新の版から定期的に更新する手順があることを求めています。
HaniWord の決まり (2026-10-22 に決め、2026-11-23 に続けられる形に改めた): **JMdict_e.gz は、開発者が配布のページ (https://www.edrdg.org/jmdict/j_jmdict.html) から手で取ります (道具では取りません)。HaniWord の版を出す前に、表に使った JMdict の作成日を確かめ、3 か月より古ければ、新しい JMdict_e.gz を取って作り直してから出します。** 版を出さない間は作り直しません。
オフにできない追加辞書の成句 (core_phrases、JMdict の表現・慣用句) も同じ決まりで、手順は tools/extra_dict/README.md の「JMdict の版を新しくする」。

1. 新しい JMdict_e.gz (https://www.edrdg.org/jmdict/j_jmdict.html) を取り、版 (ファイルの頭の `JMdict created: <日付>`) を控える
2. JMdict の語を使う分野ごとに `jmdict_candidates.py <JMdict_e.gz> <出力.tsv> --fields <分野の印> --holdout <測りの組の記事の一覧>`
   で候補を作り直す (分野の印は上の表)
3. 前の版の候補と比べる
   - **増えた語**: 手順の 2〜4 (今の辞書で試す → `select_terms.py` → 読み付け。Codex の枠を節約するときは `pipeline_readings_lite.sh`) に回す
   - **消えた語**: `<名前>.tsv` の 6 列目 (出典) が JMdict だけの語なら、その行を除く。ほかの出典 (ウィキペディア・NBDC など) もあれば残す
4. 増えた語の束を足して `assemble_domain.sh` で組み上げ直す (同じ語の判定・ほかの読みの確かめ・値の調整もし直す)。変換サーバーの版が変わったときも、
   今の変換で正しく出る語を除き直すために組み上げ直す
5. 手順の 7 で、前の版と比べて分野の文・一般の文が下がらないことを確かめる
6. `domain_dictionaries/LICENSE.txt` の「JMdict の版と更新」の版を書き換える
