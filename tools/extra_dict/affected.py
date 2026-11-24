"""(2026-10-22、M3.5) 追加辞書の語の読みを含む文だけを選ぶ (読みを含まない文は、語を足しても変換が変わらない)
使い方: python3 affected.py <out.tsv> <cases.tsv ...> -- <辞書の TSV ...>"""
import sys
args = sys.argv[1:]; out = args[0]; k = args.index("--")
cases, dicts = args[1:k], args[k + 1:]
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
readings = {hira(l.split("\t")[1]) for d in dicts for l in open(d, encoding="utf-8") if l.strip() and not l.startswith("#")}
# 読みの長さごとに集合を引く (文の読みの部分文字列を全部見る)
lengths = sorted({len(r) for r in readings})
n = 0
with open(out, "w", encoding="utf-8") as f:
    for path in cases:
        for line in open(path, encoding="utf-8"):
            if not line.strip() or line.startswith("#"): continue
            r = hira(line.split("\t")[0])
            if any(r[i:i + L] in readings for L in lengths for i in range(len(r) - L + 1)):
                f.write(line); n += 1
print(out, n)
