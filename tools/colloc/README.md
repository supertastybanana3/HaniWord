# 同音の語の組み合わせの表の作り方 (tools/colloc)

変換サーバー (`server-swift/Sources/azookey-server/colloc.swift`) が読む `colloc_choices.tsv` を作る道具です。
同じ読みの語 (機械・機会、効く・聞く) を、すぐ前後の語 (薬が効く・機械を操作) との組の回数で選び直すのに使います。
出典とライセンスは `extra_dictionaries/LICENSE.txt` の「同音の語の組み合わせの表」の節にあります (表は CC BY-SA 4.0)。

表は 30MB ほどあるので、リポジトリには入れていません。配布物に入れるときは、`colloc_choices.tsv` と、その SHA-256 を書いた
`SHA256SUMS.txt` を 1 つのフォルダに置き、`scripts/release/package.py --colloc <そのフォルダ>` に渡します
(package.py は、SHA-256 と、行の形・鍵の並びを確かめてから `files` に写します)。

## 手順

1. `python3 -I homophone_readings.py <変換サーバーの一式の Dictionary/louds> homophone_readings.txt`
   - azooKey の辞書で、同じ読みに漢字を含む語が 2 つ以上ある読み (カタカナ) を書き出す
2. `<Sudachi の python> count_colloc.py homophone_readings.txt <WORK>/part<i> <i> <本数> <文の tsv>...` を、i = 0〜本数-1 で並べて流す (重いので夜に)
   - 文の tsv は 1 行 1 文 (記事 ID<TAB>文)。2026-11-07 の表は次の 2 つの全部 (6,980 万文) で数えた
     - Wikipedia 日本語版 (Hugging Face の wikimedia/wikipedia 20231101.ja の parquet 15 個) の本文を、。！？ と改行で切ったもの
     - 青空文庫 (aozorahack/aozorabunko_text の 2023-03-22 の版) の、新字新仮名で著作権の切れた作品の本文から、ルビと注記を外して同じように切ったもの
   - SudachiPy (SudachiDict の full、SplitMode.B) で語に分け、同音の読みの語と、すぐ後ろ (R1)・後ろの格助詞の次 (R2)・すぐ前 (L1)・前の格助詞の前 (L2) の内容語の組を数える
   - 200 万文ごとに `<WORK>/part<i>.NNN.tsv` に書き出す (途中で止めたら、ファイルの頭の説明のとおり続きから流せる)
3. 書き出しを足し合わせる: `LC_ALL=C sort -t$'\t' -k1,4` で並べ、同じ 4 つの欄 (読み・文脈の種類・文脈・表記) の回数を足して `merged.tsv` にする
4. `python3 -I build_colloc_table.py merged.tsv colloc_choices.tsv 10 10 10000`
   - 組になる読みが 10,000 を超える文脈の語 (し・こと・年・日 など) は使わない。いちばん多い表記が 10 回以上の文脈を入れ、
     その 1/10 より多いほかの表記は「近い表記」として行に書く (変換サーバーは近い表記を替えない)
   - 行は 読み<TAB>文脈の種類<TAB>文脈 の UTF-8 のバイトの順 (変換サーバーは読み込まずに二分探索で引く)
   - 2026-11-07 の表: 933,267 行・31.6MB (SHA-256 041bd75b2b6674df9c08dd1edf3f836391c410e2e396648ed2138257b084125f)

## 行の形

読み (カタカナ)・文脈の種類 (R1・R2・L1・L2)・文脈 (R2・L2 は 格助詞|語 か 語|格助詞)・選ぶ表記・その回数・近い表記 (表記:回数 を , でつなぐ。無ければ空)。
頭の `#` の行は説明。

## 測り (2026-11-07、一般の文 3,675・目隠しの分野別の文 1,302)

変換サーバー v2.3 (数詞と接尾の語は文脈にしない、1 字の語は辞書だけの変換でだけ替える) で、
辞書だけの変換は 一般 1,315 → 1,395 文 (良 80・悪 0)、目隠し 473 → 517 文 (良 44・悪 0)、AI ありの変換は 一般 2,275 → 2,283 文 (良 10・悪 2)。
