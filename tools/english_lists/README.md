# 英字の判定の表を作る道具

IME 本体 (crates/client/src/engine/auto_english.rs) が読む英単語の表のうち、ここの道具で作るもの。

| 道具 | 作る表 | 中身 |
|---|---|---|
| `make_boundary_words.py` | `english_boundary_words.txt` | ローマ字として読めるが、かなが日本語の語にならない英単語 (see = せえ)。語の切れ目に収まっているときだけ英字にする |
| `make_suru_words.py` | `english_suru_words.txt` | サ変の形 (する・した・して …) のすぐ前なら英字にまとめる英単語 (release = れぇあせ・commit + する = こっみつる)。2026-11-06 |
| `make_case_tilde.py` | `english_case.txt` の ~ の行 | 普通の英単語でもある名前 (windows・zoom) を、和文にすき間なくつながっているときだけその表記にする行 |

## english_suru_words.txt の元の語

- 英単語の表: `english_words_scowl.txt` (SCOWL 2020.12.07) と `english_terms.txt`
- 足す語 `suru_extra_words.txt`: 上の 2 つに無く、「〜する」と使う git の動詞 (cherrypick・fixup・init・rebase・stash・submodule・worktree)。
  この表の元にだけ使い、英字の判定の語の表 (どこでも英字にする語) には入れない (rebase = ればせ は、あれば＋せ… の頭に当たるため)。
  出典は、git 2.43.0 の次の出力 (2026-11-06 に WSL の git で開いた):
  - `git help -a` の「Main Porcelain Commands」の命令名 (cherry-pick・init・rebase・stash・submodule・worktree。cherry-pick は - を外して打つ形 cherrypick にした)
  - `git commit -h` の選択肢 `--fixup`
  - 同じ一覧にある amend・squash (`git commit -h` の `--amend`・`--squash`) は SCOWL にあるので足していない

## 作り方 (english_suru_words.txt)

```
python3 -I make_suru_words.py inputs <KanaWordsProbe の入力> suru_extra_words.txt english_words_scowl.txt english_terms.txt
# 変換サーバーの試験 KanaWordsProbe を、AZK_KANA_WORDS_IN にその入力、AZK_TEST_DICTIONARY_STAGE に辞書の置き場を渡して流す
python3 -I make_suru_words.py pick <KanaWordsProbe の入力> <KanaWordsProbe の出力> english_suru_words.txt
```

日本語の語とみなす条件は `make_boundary_words.py` とほぼ同じ。違いは「1 位の並びに助詞が入る」を、1 位の点数が -25 以上のときだけ日本語の印とみること（1 位がでたらめな並び（delete = で|ェ|て -35.1）まで日本語とみると、て で終わる読みの英単語が入らなかった）。2026-11-06 の表は 2,216 語（辞書は変換サーバーの配布物と同じ Dictionary）。
