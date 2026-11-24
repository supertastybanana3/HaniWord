"""2026-11-07 (仕事 40): scan_exclusions.py の出力 (鍵を含む長い続きと回数) から、鍵が語の切れ目をまたいでいる続き (非人為的 = 非/人為/的 の 非人、
本土方言 = 本土/方言 の 土方) を除く語にする。切れ目は Sudachi (full 辞書) の短い単位 (A)。鍵の内側に切れ目が無い続き (支那人 = 支那/人、1 語の 支那竹) は除かない。
出力 TSV: 鍵<TAB>除く語 (| 区切り、回数の多い順)。使い方: python3 straddle_exclusions.py <scan.tsv> <out.tsv> [最小の回数 (既定 2)]"""
import sys, warnings
warnings.filterwarnings("ignore")
from sudachipy import dictionary, tokenizer
t = dictionary.Dictionary(dict="full").create()
A = tokenizer.Tokenizer.SplitMode.A
scan, out = sys.argv[1:3]
minimum = int(sys.argv[3]) if len(sys.argv) > 3 else 2
res = {}
for line in open(scan, encoding="utf-8"):
    if line.startswith("#"): continue
    key, total, exact, top = (line.rstrip("\n").split("\t") + [""])[:4]
    ex = []
    for item in [x for x in top.split("|") if x]:
        run, n = item.rsplit(":", 1)
        if int(n) < minimum: continue
        bounds, pos = {0}, 0
        for m in t.tokenize(run, A):
            pos += len(m.surface()); bounds.add(pos)
        # 鍵の内側 (両端を除く) に語の切れ目がある = 鍵が 2 つの語にまたがる (非/人為 の 非人)。1 語の中に鍵がある (支那竹) ときは除かない
        i = run.find(key)
        straddle = i >= 0
        while i >= 0:
            if not any(i < b < i + len(key) for b in bounds):
                straddle = False; break
            i = run.find(key, i + 1)
        if straddle: ex.append(run)
    if ex: res[key] = ex
with open(out, "w", encoding="utf-8") as f:
    f.write("# 鍵が語の切れ目をまたぐ続き (Sudachi の短い単位)。除く語にする\n")
    for k, ex in res.items(): f.write(f"{k}\t{'|'.join(ex)}\n")
print(len(res), "keys with exclusions")
