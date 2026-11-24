"""count_word_heads.py の出力から、IME 本体が読む表 (crates/client/src/engine/japanese_heads.txt) を作る (2026-11-04、仕事 11)。
使い方: python make_heads.py <heads.json> <japanese_heads.txt> [min=41]
  3 音の頭のうち min 回以上のものだけを、かなの順で書く (IME 本体は組み込んだ文字のまま二分探索で引く。無い並びは語の頭としてまれとみる)。
  語の頭の線 (40 回を超えたら分けない) を変えるなら min も合わせる"""
import json, sys

src, dst = sys.argv[1], sys.argv[2]
least = next((int(a[4:]) for a in sys.argv[3:] if a.startswith("min=")), 41)
c = json.load(open(src, encoding="utf-8"))
rows = sorted((k, n) for k, n in c["first3"].items() if n >= least)
with open(dst, "w", encoding="utf-8", newline="\n") as f:
    f.write("# HaniWord: 日本語の文の語の頭 (形態素の頭から 3 音) の回数。英単語のすぐ後ろに助詞なしで続けた和文 (patch 来てから) を、英単語 2 つに分けないために使う\n")
    f.write("# 出典: 日本語版ウィキペディアの本文 (wikimedia/wikipedia 20231101.ja の 3 万記事) を UniDic (unidic-lite) の読みで数えたもの (tools/after_latin)。回数だけで本文は含まない\n")
    f.write(f"# 語の頭 {c['heads']}・記事 {c['articles']}。{least} 回以上の並びだけ\n")
    for k, n in rows:
        f.write(f"{k}\t{n}\n")
print(f"3 音 {len(rows)} (全部 {len(c['first3'])})・語の頭 {c['heads']:,}・記事 {c['articles']:,}")
