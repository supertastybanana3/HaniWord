"""count_after_latin.py の出力から、IME 本体が読む表 (crates/client/src/engine/after_latin.txt) を作る (2026-11-03、仕事 11)。
使い方: python make_table.py <full.json> <after_latin.txt> [min=4]
  2 音・3 音の頭のうち min 回以上のものだけを、1 つの並び (かなの順) で書く。IME 本体は組み込んだ文字のまま二分探索で引き、
  無い並びは 0 回 (来にくい) とみる。来にくいの線 (3 回以下) を変えるなら min も合わせる"""
import json, sys

src, dst = sys.argv[1], sys.argv[2]
least = next((int(a[4:]) for a in sys.argv[3:] if a.startswith("min=")), 4)
c = json.load(open(src, encoding="utf-8"))
rows2 = [(k, n) for k, n in c["first2"].items() if n >= least]
rows3 = [(k, n) for k, n in c["first3"].items() if n >= least]
with open(dst, "w", encoding="utf-8", newline="\n") as f:
    f.write("# HaniWord: 英字の語のすぐ後ろ (空白なし) に来る読みの頭 (2 音・3 音) の回数。英単語 + 来にくい かな (powers + たげ) を英単語 2 つ (power + stage) に分けるかを決めるのに使う\n")
    f.write("# 出典: 日本語版ウィキペディアの本文 (wikimedia/wikipedia 20231101.ja) を UniDic (unidic-lite) の読みで数えたもの (tools/after_latin)。回数だけで本文は含まない\n")
    f.write(f"# 数えた所 {c['occurrences']}・記事 {c['articles']}。{least} 回以上の並びだけ\n")
    for k, n in sorted(rows2 + rows3):
        f.write(f"{k}\t{n}\n")
print(f"2 音 {len(rows2)} (全部 {len(c['first2'])})・3 音 {len(rows3)} (全部 {len(c['first3'])})・数えた所 {c['occurrences']:,}・記事 {c['articles']:,}")
