"""2026-11-05 C: ドイツ語の名詞の頭の大文字の表 (german_case.tsv、IME 本体の隣に置く) を、count_case.py の出力から作る。
語は latin_words/de.txt (次の語の候補の表と同じ語) にあるものだけ。
  #global<TAB>割合       両方の書き方がある語が、文の途中で大文字だった割合
  w<TAB>語<TAB>割合      語が文の途中で大文字で始まった割合 (5% 以上の語だけ)
  a<TAB>前の語<TAB>割合  前の語のあとで、両方の書き方がある語が大文字だった割合 (200 回以上の前の語)
  p<TAB>前の語 語<TAB>割合<TAB>回数  その組で大文字だった割合 (10 回以上で、語の割合と 0.25 以上違う組だけ)
使い方: python3 -I make_table.py <count_case.py の出力のフォルダ> <latin_words/de.txt> <出力 german_case.tsv>"""
import sys
src, vocab_path, out = sys.argv[1:4]
vocab = {l.strip() for l in open(vocab_path, encoding="utf-8") if l.strip() and not l.startswith("#")}
word = {}
for l in open(f"{src}/word_share.tsv", encoding="utf-8"):
    w, s, n = l.rstrip("\n").split("\t")
    if w in vocab:
        word[w] = float(s)
lines = []
for l in open(f"{src}/after_rate.tsv", encoding="utf-8"):
    p, s, n = l.rstrip("\n").split("\t")
    if p == "#global":
        lines.insert(0, f"#global\t{float(s):.3f}")
    else:
        lines.append(f"a\t{p}\t{float(s):.3f}")
for w, s in sorted(word.items()):
    lines.append(f"w\t{w}\t{s:.3f}")
pairs = 0
for l in open(f"{src}/pair_share.tsv", encoding="utf-8"):
    key, s, n = l.rstrip("\n").split("\t")
    p, w = key.split(" ")
    if w in word and abs(float(s) - word[w]) >= 0.25:
        lines.append(f"p\t{p} {w}\t{float(s):.3f}\t{n}")
        pairs += 1
header = ("# HaniWord のドイツ語の名詞の頭の大文字の表 (つづりの指摘・自動修正 latin_spell.rs が読む)。FineWeb-2 (deu_Latn、ODC-By 1.0) の文の途中の語の、"
          "大文字で始まった回数の割合から作成 (tools/latin_spell/german_case)。文そのものは入っていない\n")
open(out, "w", encoding="utf-8").write(header + "\n".join(lines) + "\n")
print("語", len(word), "組", pairs, "前の語", sum(1 for l in lines if l.startswith("a\t")))
