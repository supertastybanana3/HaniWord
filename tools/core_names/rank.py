"""2026-11-07 C: 仕事 44。拾い出した人物 (extract_names.py の出力) から、azooKey の辞書 (と追加辞書・分野辞書) に同じ読みと表記がある語を外し、
名前が手元の Wikipedia の文に出た回数 (grep の数え - 自分の記事の中の回数) で並べる。姓だけ・名だけは入れない (表記はフルネームか 1 語の名前・グループ名)。
使い方: python3 rank.py <persons.tsv> <azookey_dict.tsv> <grep の数え (回数 名前)> <out.tsv> [追加の表 (表記<TAB>読み) ...]"""
import sys
from collections import defaultdict
src, dic, counts, out = sys.argv[1:5]
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
have = set()
for line in open(dic, encoding="utf-8"):
    c = line.split("\t")
    if len(c) >= 2: have.add((c[0], c[1]))
for path in sys.argv[5:]:
    for line in open(path, encoding="utf-8"):
        c = line.rstrip("\n").split("\t")
        if not line.startswith("#") and len(c) >= 2: have.add((c[1], c[0]))
cnt = {}
for line in open(counts, encoding="utf-8"):
    p = line.strip().split(" ", 1)
    if len(p) == 2: cnt[p[1]] = int(p[0])
rows, skipped = [], 0
seen = set()
for line in open(src, encoding="utf-8"):
    if line.startswith("#"): continue
    aid, title, surface, yomi, split, kind, desc, own = line.rstrip("\n").split("\t")
    if (kata(yomi), surface) in have:
        skipped += 1; continue
    if (surface, yomi) in seen: continue
    seen.add((surface, yomi))
    n = max(0, cnt.get(surface, 0) - int(own))
    rows.append((n, surface, yomi, split, kind, desc, aid, title))
rows.sort(key=lambda r: -r[0])
with open(out, "w", encoding="utf-8") as f:
    f.write("# 順\tほかの記事の文に出た回数\t表記\t読み\t姓名の区切り\t種類\tは、のあと\t記事の番号\t題\n")
    for i, r in enumerate(rows, 1): f.write(f"{i}\t" + "\t".join(map(str, r)) + "\n")
print(len(rows), "rows,", skipped, "already in the dictionaries")
