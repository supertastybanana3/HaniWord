"""2026-11-07 C: 仕事 43・44。閲覧数の上位の記事 (top_articles.tsv) の、隠しでないカテゴリと、転送かどうかを MediaWiki の API で取る
((A) 固有名詞と (B) ふつうの語の分け方に使う)。API:Etiquette: 連絡先つきの名乗り・1 つずつ・1 秒あける・maxlag=5・429 と maxlag なら待つ。
50 題ずつ prop=categories (clshow=!hidden、cllimit=max、続きは clcontinue)。書いた題から続ける。
出力 JSONL: title, exists, redirect_to, categories
使い方: python3 -I fetch_categories.py <top_articles.tsv> <out.jsonl>"""
import json, os, sys, time, urllib.error, urllib.parse, urllib.request
UA = "HaniWord-dict-tools/0.1 (https://github.com/supertastybanana3)"
API = "https://ja.wikipedia.org/w/api.php"
def call(**p):
    p.update(format="json", formatversion="2", maxlag="5")
    url = API + "?" + urllib.parse.urlencode(p)
    for _ in range(8):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": UA}), timeout=60) as r:
                d = json.loads(r.read().decode("utf-8"))
            if d.get("error", {}).get("code") == "maxlag": time.sleep(10); continue
            time.sleep(1.0); return d
        except urllib.error.HTTPError as e:
            if e.code == 429: time.sleep(int(e.headers.get("Retry-After") or 60)); continue
            raise
    raise RuntimeError(url)
src, out = sys.argv[1:3]
titles = [l.rstrip("\n").split("\t", 1)[0] for l in open(src, encoding="utf-8") if not l.startswith("#") and l.strip()]
done = {json.loads(l)["title"] for l in open(out, encoding="utf-8")} if os.path.exists(out) else set()
todo = [t for t in titles if t not in done]
with open(out, "a", encoding="utf-8") as f:
    for i in range(0, len(todo), 50):
        batch = todo[i:i + 50]
        cats, redirect, exists = {}, {}, {}
        params = dict(action="query", titles="|".join(batch), prop="categories", clshow="!hidden", cllimit="max", redirects="1")
        while True:
            d = call(**params)
            q = d.get("query", {})
            norm = {n["from"]: n["to"] for n in q.get("normalized", [])}
            for r in q.get("redirects", []): redirect[r["from"]] = r["to"]
            for p in q.get("pages", []):
                exists[p["title"]] = "missing" not in p
                cats.setdefault(p["title"], []).extend(c["title"].split(":", 1)[1] for c in p.get("categories", []))
            if "continue" not in d: break
            params.update(d["continue"])
        for t in batch:
            t2 = norm.get(t, t); t3 = redirect.get(t2, t2)
            f.write(json.dumps({"title": t, "exists": exists.get(t3, False), "redirect_to": t3 if t3 != t2 else "", "categories": sorted(set(cats.get(t3, [])))}, ensure_ascii=False) + "\n")
        f.flush()
        if (i // 50) % 40 == 0: print(i + len(batch), "/", len(todo), time.strftime("%H:%M"), flush=True)
print("done", time.strftime("%Y-%m-%d %H:%M:%S %z"))
