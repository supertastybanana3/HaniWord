"""2026-11-07 C: 仕事 43。冒頭の文が空だった記事の冒頭の節を、wikitext で 1 つずつ取り直す (作法は fetch_category.py と同じ)"""
import json, sys, time, urllib.error, urllib.parse, urllib.request
UA = "HaniWord-dict-tools/0.1 (https://github.com/supertastybanana3)"
API = "https://ja.wikipedia.org/w/api.php"
path = sys.argv[1]
rows = [json.loads(l) for l in open(path, encoding="utf-8")]
for r in rows:
    if r["intro"]:
        continue
    params = dict(action="parse", page=r["title"], prop="wikitext", section="0", format="json", formatversion="2", maxlag="5")
    req = urllib.request.Request(API + "?" + urllib.parse.urlencode(params), headers={"User-Agent": UA})
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            d = json.loads(resp.read().decode("utf-8"))
        r["intro"] = (d.get("parse", {}).get("wikitext") or "")[:1500]
        r["intro_kind"] = "wikitext"
    except urllib.error.HTTPError as e:
        print("HTTP", e.code, r["title"])
        if e.code == 429: time.sleep(60)
    time.sleep(2)
with open(path, "w", encoding="utf-8") as f:
    for r in rows: f.write(json.dumps(r, ensure_ascii=False) + "\n")
print(sum(1 for r in rows if not r["intro"]), "still empty")
