"""2026-11-07 C: 仕事 43。ウィキペディア「Category:日本のインターネットスラング」の記事 (と下位のカテゴリの記事) の一覧・版の番号・冒頭の文を、
MediaWiki の API で取る (API:Etiquette: 連絡先つきの名乗り・1 つずつ・間を空ける・429 と maxlag なら待つ)。
使い方: python3 -I fetch_category.py <out_dir>"""
import json, sys, time, urllib.error, urllib.parse, urllib.request
UA = "HaniWord-dict-tools/0.1 (https://github.com/supertastybanana3)"
API = "https://ja.wikipedia.org/w/api.php"
out = sys.argv[1]
def call(params):
    params = dict(params, format="json", formatversion="2", maxlag="5")
    for attempt in range(5):
        req = urllib.request.Request(API + "?" + urllib.parse.urlencode(params), headers={"User-Agent": UA})
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                d = json.loads(r.read().decode("utf-8"))
            if d.get("error", {}).get("code") == "maxlag":
                time.sleep(10); continue
            time.sleep(2)
            return d
        except urllib.error.HTTPError as e:
            if e.code == 429:
                time.sleep(60); continue
            raise
    raise SystemExit("API が答えない")
def members(cat):
    pages, cats, cont = [], [], {}
    while True:
        d = call(dict(action="query", list="categorymembers", cmtitle=cat, cmlimit="500", cmtype="page|subcat", **cont))
        for m in d["query"]["categorymembers"]:
            (cats if m["ns"] == 14 else pages).append(m["title"]) if m["ns"] in (0, 14) else None
        if "continue" not in d: return pages, cats
        cont = {"cmcontinue": d["continue"]["cmcontinue"]}
root = "Category:日本のインターネットスラング"
pages, subcats = members(root)
seen = {root}
by_cat = {t: root for t in pages}
for c in subcats:
    if c in seen: continue
    seen.add(c)
    ps, _ = members(c)  # 下位のカテゴリは 1 段だけ
    for t in ps: by_cat.setdefault(t, c)
titles = list(by_cat)
rows = []
for i in range(0, len(titles), 20):
    batch = titles[i:i + 20]
    d = call(dict(action="query", prop="revisions|extracts", titles="|".join(batch), rvprop="ids|timestamp", exintro="1", explaintext="1", exlimit="20"))
    for p in d["query"]["pages"]:
        rev = (p.get("revisions") or [{}])[0]
        rows.append({"title": p["title"], "category": by_cat.get(p["title"], root), "oldid": rev.get("revid"), "timestamp": rev.get("timestamp"),
                     "intro": (p.get("extract") or "")[:1200]})
with open(f"{out}/articles.jsonl", "w", encoding="utf-8") as f:
    for r in rows: f.write(json.dumps(r, ensure_ascii=False) + "\n")
print(len(titles), "articles", len(subcats), "subcats")
