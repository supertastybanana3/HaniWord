"""2026-11-07 C: 仕事 42。rare_kanji_plain.py (活用しない語) と rare_kanji_forms.py (活用形) の出力を合わせて、
extra_dictionaries/core_rare_kanji.tsv を書く (同じ表記・読み・品詞の行は先のものだけ。確かめ用の 7 列目は落とす)。
使い方: python3 -I rare_kanji_table.py <plain.tsv> <forms.tsv> <out core_rare_kanji.tsv>"""
import sys
seen, rows = set(), []
for p in sys.argv[1:3]:
    for l in open(p, encoding="utf-8"):
        if l.startswith("#") or not l.strip(): continue
        c = l.rstrip("\n").split("\t")[:6]
        if (c[0], c[1], c[2]) in seen: continue
        seen.add((c[0], c[1], c[2])); rows.append(c)
with open(sys.argv[3], "w", encoding="utf-8", newline="\n") as f:
    f.write("# 表記\t読み\tcid\tmid\t値\t出典\n")
    for r in rows: f.write("\t".join(r) + "\n")
print(len(rows), "rows")
