# データのライセンス

HaniWord のプログラム (ソースコード) は [LICENSE](LICENSE) の MIT License です。
このリポジトリの辞書・単語表などのデータは MIT ではなく、下の表ごとのライセンスです (多くは CC BY-SA 4.0)。
CC BY-SA 4.0 の表を使うとき・作り直して配るときは、表示と継承 (同じライセンスで配る) の条件に従ってください。

配布物 (インストーラー) に入るソフトウェア・データのライセンスと出典の全文は、インストール後の `%APPDATA%\HaniWord` にある
`THIRD_PARTY_NOTICES.txt` と `licenses` フォルダにあります (`scripts/release/package.py` が、下のファイルから組み立てます)。
各ファイルの頭の行にも、出典と作り方を書いています。

## フォルダごと

| フォルダ | 中身 | ライセンス (詳しくは) |
|---|---|---|
| `domain_dictionaries/` | 専門用語の辞書 (11 分野) | CC BY-SA 4.0 (`domain_dictionaries/LICENSE.txt`。材料の出典もここ) |
| `extra_dictionaries/` | オフにできない追加の辞書・注意の注記の参考文献 | 材料ごとの条件 (多くは CC BY-SA 4.0。一部は Mozc の公開辞書による IPAdic の許諾)。`extra_dictionaries/LICENSE.txt` |
| `dialect_dictionaries/` | 方言の言い方の辞書 (設定で選んだ人だけが使う) | CC BY-SA 4.0 (`dialect_dictionaries/LICENSE.txt`) |
| `slang_dictionaries/` | くだけた言葉の辞書 (設定でオンにした人だけが使う) | CC BY-SA 4.0 (`slang_dictionaries/LICENSE.txt`) |
| `latin_words/` | 英語以外の単語表 (wordfreq 3.1.1 から) | CC BY-SA 4.0 (`latin_words/NOTICE.txt`・`WORD_FREQ_LICENSE.txt`) |
| `crates/client/data/` | 異体字の表 (`ivs_sequences.txt`・`ideograph_variants.txt`) | Unicode IVD・Unihan から作成。Unicode License V3 (`UNICODE_LICENSE.txt`) |
| `installer/licenses/` | 配布物に入るほかのソフトウェア・データのライセンスの文 | 各ファイルのとおり |

## リポジトリの根の表

### CC BY-SA 4.0 (または材料ごとの条件)

| ファイル | 中身 | 出典・条件 |
|---|---|---|
| `english_words.tsv` | カタカナ語の読み → 英字の表記 | JMdict (CC BY-SA 4.0) の訳語・語源と CMUdict (BSD 2 条項型、`installer/licenses/CMUdict_LICENSE.txt`) の発音から作成。このファイルは CC BY-SA 4.0 |
| `english_products.txt` | ソフト・アプリ・サービスの製品名の英字の表記 | 日本語版ウィキペディアの記事の題から作成。CC BY-SA 4.0 |
| `german_case.tsv` | ドイツ語の名詞の頭の大文字 | FineWeb-2 (ODC-By 1.0) で数えた割合。語は `latin_words/de.txt` (wordfreq) の語だけ。CC BY-SA 4.0 |
| `initial_words.txt` | 英字 1 字＋語 (Jリーグ・B級・X線) | 日本語版ウィキペディアの本文 (2023-11-01 のダンプ) での形と回数 (本文は入らない) と SudachiDict (Apache License 2.0) の読みから作成。CC BY-SA 4.0 |
| `casual_words.tsv`・`counter_counts.tsv`・`homophone_phrases.tsv`・`homophone_triggers.tsv`・`homophone_words.tsv`・`hyouki_groups.tsv`・`misreading_candidates.tsv`・`misuse_notes.tsv`・`rendaku_readings.tsv`・`variant_spellings.tsv` | くだけた語・助数詞の回数・同音語・表記ゆれ・読み違い・よくある誤用・連濁・代わりの字の表 | 材料ごとの条件 (多くは CC BY-SA 4.0)。`extra_dictionaries/LICENSE.txt` |
| `japanese_names_romaji.txt` | 日本の人名のローマ字 | Mozc の公開辞書 (IPAdic の許諾) と Wikidata (CC0 1.0)。`extra_dictionaries/LICENSE.txt` の「Mozc の公開辞書のファイルの許諾」 |
| `crates/client/src/engine/after_latin.txt`・`japanese_heads.txt` | 英単語＋かな を英単語 2 つに分けるかの表 (IME 本体に組み込む) | 日本語版ウィキペディアの本文 (2023-11-01 のダンプ) を UniDic (unidic-lite) の読みで数えた回数 (本文は入らない)。CC BY-SA 4.0 |

ウィキペディア日本語版の 2023-11-01 のダンプ (Hugging Face の wikimedia/wikipedia の 20231101.ja) から作った表 (上の `initial_words.txt`・
`after_latin.txt`・`japanese_heads.txt`、`counter_counts.tsv`・`homophone_triggers.tsv`、同音の語の組み合わせの表 `colloc_choices.tsv` (リポジトリには入れていない) など。
どれを使ったかは各ファイルの頭の行と `tools/` の README) は、CC BY-SA 4.0 (元は CC BY-SA 3.0 / GFDL のダンプ) です。
CC BY-SA 3.0 の 4(b) (https://creativecommons.org/licenses/by-sa/3.0/legalcode) は、翻案を同じ許諾要素 (表示・継承) の後の版のライセンスで配ることを認めています。

### そのほかの条件

| ファイル | 中身 | 出典・条件 |
|---|---|---|
| `english_words_scowl.txt`・`english_romaji_priority.txt`・`english_n_split.txt`・`english_boundary_words.txt`・`english_suru_words.txt` | 英単語の判別に使う語の表 | SCOWL (Kevin Atkinson ほか) から作成。全文は `SCOWL_COPYRIGHT.txt`。`english_romaji_priority.txt`・`english_n_split.txt` の選別に JMdict の読みを使用。`english_suru_words.txt` には `english_terms.txt` の語も入る |
| `english_words_context.txt`・`english_words_context.json` | 英語の文脈・大文字の補い | SCOWLv2 / English Speller Database。全文は `ENGLISH_CONTEXT_COPYRIGHT.txt` |
| `english_brands.txt`・`english_names.txt` | 社名・ブランド、人・地名・組織の英語の名前 | Wikidata (CC0 1.0) |
| `tex_commands.txt` | TeX のコマンドの一覧 | LaTeX Workshop (MIT License) の一覧からコマンドの名前を抜き出したもの。全文は `LATEX_WORKSHOP_LICENSE.txt` |
| `japanese_vocabulary.json` | 手で集めた綴りと読み | CC0 1.0。`JAPANESE_VOCABULARY_NOTES.md` |

### HaniWord の開発で作った表 (プログラムと同じ MIT License)

`english_terms.txt`・`english_context_curated.txt`・`english_words_extra.tsv`・`extra_dictionary.tsv`・`reading_suppress.tsv`・`value_adjustments.tsv`・`settings.json`

### 出典の書き足しが要る表

次の表は、頭の行に出典が書かれていません。次の版で書き足します。

- `english_case.txt` (英単語の大文字・小文字の表記)
- `katakana_variants.tsv` (カタカナの語と同じ読みの漢字の語。変換の辞書の語と重みから作成)
- `latin_kana_policy.tsv` (かなで打ったときの、辞書の英字の語の扱い。変換の辞書の語から作成)

変換の辞書の語のライセンスは、azooKey の辞書 (Apache License 2.0) と `extra_dictionaries/LICENSE.txt` を見てください。
