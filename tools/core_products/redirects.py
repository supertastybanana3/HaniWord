"""2026-11-11 C: 製品名の表の材料 2。jawiki の転送の表 (jawiki-latest-redirect.sql.gz) から、集めた記事へ向かう転送の題を取る
(英字の別名 After Effects・Premiere Pro と、カタカナの題 ダビンチリゾルブ。転送のページの題は page.tsv から引く)。
出力 TSV: 記事の題<TAB>転送の題 (| 区切り)
使い方: python3 -I redirects.py <jawiki-latest-redirect.sql.gz> <jawiki の tsv の置き場> <collect_articles.py の出力> <out.tsv>"""
import gzip, re, sys, collections
dump, W, arts_p, out_p = sys.argv[1:5]
arts = {l.split("\t")[0].replace(" ", "_") for l in open(arts_p, encoding="utf-8")}
title_of = {}
for l in open(f"{W}/page.tsv", encoding="utf-8"):
    c = l.rstrip("\n").split("\t")
    if c[1] == "0" and c[3] == "1": title_of[int(c[0])] = c[2]
S = r"'((?:[^'\\]|\\.)*)'"
pat = re.compile(r"\((\d+),0," + S)
out = collections.defaultdict(set)
with gzip.open(dump, "rt", encoding="utf-8", errors="replace") as f:
    for line in f:
        if not (line.startswith("(") or line.startswith("INSERT INTO")): continue
        for m in pat.finditer(line):
            tgt = re.sub(r"\\(.)", r"\1", m.group(2))
            if tgt in arts and int(m.group(1)) in title_of: out[tgt].add(title_of[int(m.group(1))])
with open(out_p, "w", encoding="utf-8", newline="\n") as f:
    for t, rs in sorted(out.items()): f.write(t.replace("_", " ") + "\t" + "|".join(sorted(x.replace("_", " ") for x in rs)) + "\n")
print("転送のある記事", len(out))
