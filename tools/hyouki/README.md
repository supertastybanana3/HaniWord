# 表記ゆれの組の表の作り方 (tools/hyouki)

変換サーバー (`server-swift/Sources/azookey-server/hyouki.swift`) が読む `hyouki_groups.tsv` (リポジトリの頭) を作る道具です。
同じ読み・同じ品詞の番号で意味の同じ書き分け (かなと漢字・送り仮名のあるなし・交ぜ書き・々) の組と、形ごとのコーパスの回数を持ちます。
出典とライセンスは `extra_dictionaries/LICENSE.txt` の「表記ゆれの組の表」の節にあります (表は CC BY-SA 4.0)。

## 手順

1. `<Sudachi の python> -I build_groups.py <azookey_dict.tsv> <リポジトリ> <JMdict_e.gz> <Mozc の公開辞書 (dictionary_oss)> <通則 7 の語の表> <置き場>`
   - azooKey の辞書と追加辞書 (core_forms・core_words・core_phrases) の行のうち、読み・左右の品詞の番号が同じで表記だけ違う行を集め、
     同じ語の書き分けと確かめられたものを組にする (JMdict の同じ項目・Sudachi の正規化表記が同じ・漢字の骨格が同じ)。漢字の違う組 (炒る／煎る) は入れない
   - 種類 (形式名詞・補助動詞・名詞のかな書き …) は Mozc の公開辞書の品詞の名前から。無ければ Sudachi の品詞
   - 通則 7 の語の表は、文化庁「送り仮名の付け方」(内閣告示) の通則 7 の語 (受付・場合・番組 など) を 1 行 1 語にしたもの。
     元の表の印「通則7」に使うだけで、配る表 (下の 4.) ではこの印を外す
   - 出力: groups.tsv (組)・targets.tsv (数える (読み, 形))
2. `<Sudachi の python> -I count_forms.py targets.tsv <出力.tsv> <コーパス名> <コーパス (ID<TAB>文) ...> --jobs N` (重いので夜に)
   - Wikipedia 日本語版 (wikimedia/wikipedia 20231101.ja の本文) と青空文庫 (aozorahack/aozorabunko_text の本文) で、形ごとの回数を数える
3. `python3 -I finish_table.py groups.tsv <置き場> <回数.tsv> ... --dict <azookey_dict.tsv> --repo <リポジトリ>`
   - 組に回数と印 (使い分け・かなは別の語と共有・通則 7・JMdict の形の印 など) を付けた元の表 hyouki_groups.tsv (74,132 組・16.6MB) を作る
4. `python3 -I shrink_table.py <元の hyouki_groups.tsv> <ja_freq.tsv> hyouki_groups.tsv --rank 50000 --min-count 100`
   - 配る表 (19,569 組・3.5MB): 組の形のどれかが wordfreq の日本語の頻度の順位 50,000 以内で、Wikipedia と青空文庫の回数の合計が 100 以上の組。
     ja_freq.tsv は `tools/kanji_info/wordfreq_ja.py` で作る
   - 列の位置は変えない (hyouki.swift は列の番号で読む)。読まない列 (ゆれ・コーパスでいちばん多い形・辞書の値など) は空にし、
     印は「かなは別の語と共有 (…)」と「使い分け: …」だけ残す

## 列 (配る表)

読み・左の番号・右の番号・形 (| 区切り)・種類・(空)・印・Wikipedia の回数 (形の順)・青空文庫の回数 (形の順)・(空 5 つ)・JMdict の項目の番号・(空)
