# 次の語の候補の表 (next_<言語>.hwn) の作り方

英字を打っているときに、前の語から続きそうな語を出すための表です。IME 本体の隣の `next_words\` に置き、読み取り専用で写像して使います
(`crates/client/src/engine/next_word.rs`)。表はインストーラーに入っています。ここにあるのは表を作り直すための道具です。

## 元データ

| 元 | 使い方 | ライセンス |
|---|---|---|
| [FineWeb](https://huggingface.co/datasets/HuggingFaceFW/fineweb) sample-10BT の `000_00000.parquet` | 英語の学習用 | ODC-By 1.0 (Common Crawl の利用規約にも従う) |
| [FineWeb-2](https://huggingface.co/datasets/HuggingFaceFW/fineweb-2) の `<言語>_Latn/train/000_00000.parquet` の先頭約 2 億語 | 英語以外の学習用 | ODC-By 1.0 (同上) |
| [Tatoeba](https://tatoeba.org) の言語ごとの例文 (`downloads.tatoeba.org/exports/per_language/`) | 学習用 95%・試験用 5% | CC BY 2.0 FR (一部 CC0) |
| 候補にする語: 英語は `english_words_scowl.txt` ほか、英語以外は `latin_words/` | 語の一覧 | SCOWL / CC BY-SA 4.0 (wordfreq) |

できた表は CC BY-SA 4.0 で配布しています。表には語の組と点数だけが入り、文章そのものは入っていません。

## 手順 (1 言語ぶん)

```
# 1. 元データを取る (ISO3 = fra deu spa ita por pol ces tur dan swe nob isl fin)
mkdir -p WORK/src/fra
python3 tools/next_words/fetch_fw2.py https://huggingface.co/datasets/HuggingFaceFW/fineweb-2/resolve/main/data/fra_Latn/train/000_00000.parquet WORK/src/fra/fw2_train.txt 200000000
python3 tools/next_words/fetch_fw2.py https://huggingface.co/datasets/HuggingFaceFW/fineweb-2/resolve/main/data/fra_Latn/test/000_00000.parquet WORK/src/fra/fw2_test.txt 999999999999
curl -L -o WORK/src/fra/tatoeba.tsv.bz2 https://downloads.tatoeba.org/exports/per_language/fra/fra_sentences.tsv.bz2

# 2. 表を作る (WORK/hwn/next_fr.hwn と、試験の当たり率 WORK/work/fra/eval.tsv)
tools/next_words/make_table.sh WORK fra
```

`fetch_fw2.py` は parquet の text 列だけを HTTP の範囲指定で読みます (pyarrow が要ります)。`make_table.sh` の中身:

1. `ngcount2` (Rust): 小文字にそろえ、空白だけで隔たった隣り合う 2 語を数える (句読点・数字・改行で切る)。トルコ語は I → ı・İ → i
2. `tatoeba_split2.py`: Tatoeba を学習用と試験用に分け、試験用から学習用と同じ文・1 語違いの文を除く。極端に多い決まった人名は区切りにする
3. `build_table2.py`: P(次の語 | 前の語) を、それぞれ単語の頻度へ k=20 でならしてから FineWeb-2 0.7・Tatoeba 0.3 で混ぜる
4. `prune_smart.py`: 前の語ごと・打った文字 0〜2 文字ごとに、上位 3 つが「前の語を見ない順位」と違うときだけ、その組を残す
5. `hwn.py write`: IME が読む形 (`hwn.py` の先頭にファイルの形の説明) に書く
6. `eval_next.py`: 4 文字以上の語が上位 3 つに入る割合を、FineWeb-2 の試験用ファイルと Tatoeba の試験用の文で測る

英語の表は、同じ手順の英語専用の前の版で作りました (FineWeb 0.7・Tatoeba 0.3、語の一覧は `english_words_scowl.txt`・`english_words_context.txt`・`english_terms.txt`)。

できた表を配布物に入れるときは、`next_*.hwn` 14 個と、それぞれの SHA-256 を書いた `SHA256SUMS.txt` を 1 つのフォルダに置き、
`scripts/release/package.py --next-words <そのフォルダ>` に渡します。
