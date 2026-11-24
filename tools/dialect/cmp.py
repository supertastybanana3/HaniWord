"""2026-11-06 (仕事 39): 前と後の結果 (jsonl) を文ごとに比べ、語尾が直った・崩れた文を AI あり・辞書だけ別に出す。
使い方: python3 cmp.py <src.txt> <前.jsonl> <後.jsonl>"""
import json, re, sys
def load(src):
    out = {}
    for line in open(src, encoding="utf-8"):
        if not line.strip() or line.startswith("#"): continue
        c = [x.strip() for x in re.split(r"\s*/\s*", line.strip())] + ["", ""]
        out[c[0]] = (c[1], [k for k in c[2].split(",") if k], [x for x in c[3].split("|") if x])
    return out
def kok(t, keys): return all(any(x in t for x in k.split("|")) for k in keys)
cases = load(sys.argv[1])
L = lambda p: {json.loads(l)["reading"]: json.loads(l) for l in open(p, encoding="utf-8")}
b, a = L(sys.argv[2]), L(sys.argv[3])
for m in ("full", "dict"):
    good, bad, other = [], [], []
    for k in b:
        if k not in cases or k not in a: continue
        keys = cases[k][1]
        x, y = kok(b[k][m] or "", keys), kok(a[k][m] or "", keys)
        if x != y: (good if y else bad).append((b[k][m], a[k][m]))
        elif b[k][m] != a[k][m]: other.append((b[k][m], a[k][m]))
    print(f"== {'AI あり' if m == 'full' else '辞書だけ'}: 直った {len(good)}・崩れた {len(bad)}・語尾は同じで字が変わった {len(other)}")
    for t, (x, y) in [("良", g) for g in good] + [("悪", g) for g in bad] + [("  ", g) for g in other]:
        print(f"  {t} {x} → {y}")
