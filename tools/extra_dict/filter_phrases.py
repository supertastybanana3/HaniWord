"""成句の候補を、辞書だけの変換 (conv_probe.py --dict-only の結果) で先に絞る (2026-10-22、M3.5 の B)。
辞書だけで正しく出る・記号を含む・辞書だけの変換との違いが表記ゆれや数字の書き方だけ (写経の集計 probe/e2e/analyze.py の kind) の組を除き、残りを AI の変換で確かめる
使い方: HANIWORD_BUILD=<作業場所> <Sudachi のある Python> filter_phrases.py <jm_dict.jsonl> <jm_full_cases.tsv>"""
import collections, json, re, sys
from common import analyze as load_analyze
analyze = load_analyze()
src, out = sys.argv[1:3]
why, keep = collections.Counter(), []
for l in open(src, encoding="utf-8"):
    r = json.loads(l)
    e, d = r["expected"], r["dict"]
    if d == e: why["辞書だけで正しい"] += 1; continue
    if re.search(r"[・、。,.!?！？〜～…]", e): why["記号を含む"] += 1; continue
    if analyze.kind(e, d) in ("表記ゆれ", "数字や記号"): why["違いが表記ゆれ・数字だけ"] += 1; continue
    keep.append(r); why["AI で確かめる"] += 1
open(out, "w", encoding="utf-8").writelines(f"{r['reading']}\t{r['expected']}\n" for r in keep)
print(dict(why))
