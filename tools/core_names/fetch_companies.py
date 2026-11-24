"""2026-11-07 C: 仕事 44 (上場企業の名前)。Wikidata (CC0) の MediaWiki の API で、日本の証券取引所 (東京 Q217475・名古屋 Q1188840・
札幌 Q11093491・福岡 Q5507703) に上場している (上場先 P414) 項目を集める。
1) 検索 list=search srsearch="haswbstatement:P414=<取引所>" (名前空間 0、500 件ずつ)  2) wbgetentities (50 件ずつ、日本語のラベル・
日本語版ウィキペディアの記事名・主張)。上場先の主張に銘柄コード (P249) の修飾子があるか、終わりの日 (P582) があるか (上場廃止)、
解散 (P576)・名前のカナ表記 (P1814) も残す。作法は wd_api.py (API:Etiquette)。
出力: <out.jsonl> と <out>.meta.txt (取った日時と検索の条件)
使い方: python3 -I fetch_companies.py <out.jsonl>"""
import json, sys, time
sys.path.insert(0, __import__("os").path.dirname(__file__))
import wd_api
EXCHANGES = {"Q217475": "東京", "Q1188840": "名古屋", "Q11093491": "札幌", "Q5507703": "福岡"}
out = sys.argv[1]
started = time.strftime("%Y-%m-%d %H:%M:%S %z")
qids, queries = {}, []
for q, name in EXCHANGES.items():
    query = f"haswbstatement:P414={q}"; queries.append(query); offset = 0
    while True:
        d = wd_api.call(action="query", list="search", srsearch=query, srnamespace="0", srlimit="500", sroffset=str(offset), srinfo="", srprop="")
        for h in d["query"]["search"]: qids.setdefault(h["title"], set()).add(q)
        if "continue" not in d: break
        offset = d["continue"]["sroffset"]
    print(name, len([k for k, v in qids.items() if q in v]), flush=True)
def val(c):
    dv = c.get("mainsnak", {}).get("datavalue", {}).get("value")
    return dv.get("id") if isinstance(dv, dict) and "id" in dv else dv
ids = sorted(qids)
with open(out, "w", encoding="utf-8") as f:
    for i in range(0, len(ids), 50):
        d = wd_api.call(action="wbgetentities", ids="|".join(ids[i:i + 50]), props="labels|sitelinks|claims", languages="ja", sitefilter="jawiki")
        for qid, e in d.get("entities", {}).items():
            cl = e.get("claims", {})
            listings = []
            for c in cl.get("P414", []):
                ex = val(c)
                if ex not in EXCHANGES: continue
                quals = c.get("qualifiers", {})
                tick = [q.get("datavalue", {}).get("value") for q in quals.get("P249", [])]
                ended = "P582" in quals
                listings.append({"exchange": EXCHANGES[ex], "ticker": [t for t in tick if t], "ended": ended, "rank": c.get("rank")})
            rec = {"qid": qid, "label": e.get("labels", {}).get("ja", {}).get("value", ""),
                   "title": e.get("sitelinks", {}).get("jawiki", {}).get("title", ""),
                   "listings": listings, "dissolved": "P576" in cl,
                   "kana": [val(c) for c in cl.get("P1814", [])], "instance_of": [val(c) for c in cl.get("P31", [])][:5]}
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        if i % 1000 == 0: print("entities", i, flush=True)
with open(out + ".meta.txt", "w", encoding="utf-8") as f:
    f.write(f"取った日時: {started} 〜 {time.strftime('%Y-%m-%d %H:%M:%S %z')}\n入口: {wd_api.API} (MediaWiki の API。query.wikidata.org/sparql は robots.txt が断るので使わない)\n"
            f"検索の条件 (list=search、名前空間 0): {' / '.join(queries)}\n項目の中身: wbgetentities props=labels|sitelinks|claims languages=ja sitefilter=jawiki\n項目の数: {len(ids)}\n")
print("done", len(ids))
