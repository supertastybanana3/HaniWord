"""(2026-10-22、M3.5) 基準と追加辞書ありの変換 (convert.py の JSONL) を、同じ文どうしで比べる。外れの型は 写経の集計 probe/e2e/analyze.py の kind
使い方: HANIWORD_BUILD=<作業場所> <Sudachi のある Python> cmp.py <基準.jsonl> <追加あり.jsonl> [-v]"""
import collections, json, sys
from common import analyze as load_analyze
analyze = load_analyze()
base = {json.loads(l)["reading"]: json.loads(l) for l in open(sys.argv[1], encoding="utf-8")}
new = [json.loads(l) for l in open(sys.argv[2], encoding="utf-8")]
rows = [(base[r["reading"]], r) for r in new if r["reading"] in base]
ok0, ok1 = sum(b["ok"] for b, _ in rows), sum(n["ok"] for _, n in rows)
e0 = sum(analyze.lev(b["expected"], b["full"]) for b, _ in rows); e1 = sum(analyze.lev(n["expected"], n["full"]) for _, n in rows)
better = [(b, n) for b, n in rows if analyze.lev(n["expected"], n["full"]) < analyze.lev(b["expected"], b["full"])]
worse = [(b, n) for b, n in rows if analyze.lev(n["expected"], n["full"]) > analyze.lev(b["expected"], b["full"])]
print(f"{len(rows)} 文: 一致 {ok0}→{ok1}・文字の誤り {e0}→{e1}・良 {len(better)}・悪 {len(worse)}・変わった {sum(b['full'] != n['full'] for b, n in rows)}")
if "-v" in sys.argv:
    for tag, xs in (("良", better), ("悪", worse)):
        for b, n in xs:
            print(f"  {tag} 正解 {n['expected']}\n     前 {b['full']}\n     後 {n['full']}")
