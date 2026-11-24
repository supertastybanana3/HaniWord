"""2026-11-07 C: 仕事 43。全文の数え (wiki_all.tsv) は字の並びの一致なので、長い語の一部 (スピる が スピルバーグ に) を数えてしまう。
1 つの束で、語を含む文を語ごとに 300 文まで抜き、Sudachi (A 単位) の語の切れ目に語の頭と終わりがそろう割合を出す。
目安の数 = 全文の数 × その割合。その語の記事 (とリダイレクトの行き先) の文は除く。
使い方: python -I boundary_ratio.py <table.tsv> <src/articles.jsonl> <wiki_all.tsv> <out.tsv> <束>"""
import json, random, re, sys
from collections import Counter
from sudachipy import dictionary, tokenizer
table, arts, wiki, out, shard = sys.argv[1:6]
rows = [l.rstrip("\n").split("\t") for l in open(table, encoding="utf-8") if not l.startswith("#")]
A = [json.loads(l) for l in open(arts, encoding="utf-8")]
own = {r[0]: {str(a.get("pageid")), str(a.get("target_pageid"))} - {"None"} for a, r in zip(A, rows)}
allc = {}
for l in open(wiki, encoding="utf-8"):
    if not l.startswith("#"):
        w, n = l.rstrip("\n").split("\t"); allc[w] = int(n)
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
forms = {}
for r in rows:
    w = r[0]; fs = {w}
    if re.fullmatch(r"[ぁ-ゖァ-ヺー]+", w): fs |= {hira(w), kata(w)}
    for f in fs:
        if len(f) >= 2: forms.setdefault(f, set()).add(w)
pat = re.compile("|".join(map(re.escape, sorted(forms, key=len, reverse=True))))
K = 300; rnd = random.Random(1107)
raw = Counter(); samp = {}
with open(shard, encoding="utf-8") as f:
    for line in f:
        pid, _, text = line.partition("\t")
        text = text.rstrip("\n")
        seen = {}
        for m in pat.finditer(text):
            for w in forms[m.group()]:
                seen.setdefault(w, (m.start(), m.end()))
        for w, span in seen.items():
            if pid in own[w]: continue
            raw[w] += 1
            s = samp.setdefault(w, [])
            if len(s) < K: s.append((text, span))
            else:
                j = rnd.randrange(raw[w])
                if j < K: s[j] = (text, span)
tok = dictionary.Dictionary(dict="full").create(); MODE = tokenizer.Tokenizer.SplitMode.A
with open(out, "w", encoding="utf-8") as fo:
    fo.write("# 語\t全文の数\tこの束の数\t抜いた文\t切れ目にそろった文\t割合\t目安の数\t例 (そろわなかった文の語のまわり)\n")
    for r in rows:
        w = r[0]; s = samp.get(w, []); ok = 0; bad = ""
        for text, (a, b) in s:
            bounds = set()
            for t in tok.tokenize(text[:2000], MODE):
                bounds.add(t.begin()); bounds.add(t.end())
            if a in bounds and b in bounds: ok += 1
            elif not bad: bad = text[max(0, a - 6):b + 6]
        ratio = ok / len(s) if s else 0.0
        fo.write("\t".join([w, str(allc.get(w, 0)), str(raw[w]), str(len(s)), str(ok), f"{ratio:.2f}", str(round(allc.get(w, 0) * ratio)), bad]) + "\n")
print("done", len(rows))
