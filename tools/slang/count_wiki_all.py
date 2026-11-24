"""2026-11-07 C: 仕事 43。手元の Wikipedia の文 (sents/*.tsv、記事の番号<TAB>文) 全部で、表の語を含む文の数を数える。その語の記事 (とリダイレクトの行き先) の文は除く。
かなだけの語はひらがな・カタカナの両方の形を数える (2 字以上)。使い方: python3 -I count_wiki_all.py <table.tsv> <src/articles.jsonl> <out.tsv> <sents の束 ...>"""
import json, re, sys
from collections import Counter
table, arts, out = sys.argv[1:4]
shards = sys.argv[4:]
rows = [l.rstrip("\n").split("\t") for l in open(table, encoding="utf-8") if not l.startswith("#")]
A = [json.loads(l) for l in open(arts, encoding="utf-8")]
own = {}
for a, r in zip(A, rows):
    own[r[0]] = {str(a.get("pageid")), str(a.get("target_pageid"))} - {"None"}
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
forms = {}
for r in rows:
    w = r[0]; fs = {w}
    if re.fullmatch(r"[ぁ-ゖァ-ヺー]+", w): fs |= {hira(w), kata(w)}
    for f in fs:
        if len(f) >= 2: forms.setdefault(f, set()).add(w)
pat = re.compile("|".join(map(re.escape, sorted(forms, key=len, reverse=True))))
cnt = Counter()
for sh in shards:
    with open(sh, encoding="utf-8") as f:
        for line in f:
            pid, _, text = line.partition("\t")
            hit = set()
            for m in pat.findall(text): hit |= forms[m]
            for w in hit:
                if pid not in own[w]: cnt[w] += 1
with open(out, "w", encoding="utf-8") as fo:
    fo.write("# 語<TAB>Wikipedia の文 (全部の束、その語の記事の文を除く) で語を含む文の数\n")
    for r in rows: fo.write(f"{r[0]}\t{cnt[r[0]]}\n")
print(len(rows), "words")
