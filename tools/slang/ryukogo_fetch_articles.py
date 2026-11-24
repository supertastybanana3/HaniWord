"""2026-11-07 C: 仕事 43 (流行語)。受賞語ごとに、同じ名前の記事があるか (転送か)、記事の版の番号、冒頭 (読みの括弧を見るため)、
転送の記事の並べ替えの鍵 (DEFAULTSORT) を MediaWiki の API で取る (API:Etiquette: 連絡先つきの名乗り・1 つずつ・1 秒あける・maxlag・429 は待つ)。
出力 JSONL: word, exists, redirect_to, pageid, revid, timestamp, intro (冒頭の文、記事のとき), defaultsort (転送のとき)
使い方: python3 -I fetch_articles.py <awards.tsv> <out.jsonl>"""
import json, re, sys, time, urllib.error, urllib.parse, urllib.request
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
words = sorted({l.rstrip("\n").split("\t")[2] for l in open(sys.argv[1], encoding="utf-8") if not l.startswith("#")})
words = [w for w in words if w and "[" not in w and "|" not in w and len(w) <= 80]
out = {}
for i in range(0, len(words), 20):
    batch = words[i:i + 20]
    d = call(action="query", titles="|".join(batch), prop="info|revisions|extracts", rvprop="ids|timestamp|content", rvslots="main",
             rvsection="0", exintro="1", explaintext="1", exlimit="20")
    norm = {n["from"]: n["to"] for n in d["query"].get("normalized", [])}
    pages = {p["title"]: p for p in d["query"]["pages"]}
    for w in batch:
        t = norm.get(w, w); p = pages.get(t, {})
        rec = {"word": w, "exists": "missing" not in p and "invalid" not in p and bool(p)}
        if rec["exists"]:
            rv = (p.get("revisions") or [{}])[0]
            content = rv.get("slots", {}).get("main", {}).get("content", "")
            rec.update(pageid=p.get("pageid"), revid=rv.get("revid"), timestamp=rv.get("timestamp"), title=p.get("title"))
            m = re.match(r"#(?:REDIRECT|転送)\s*\[\[([^\]]+)\]\]", content, re.I)
            if m:
                rec["redirect_to"] = m.group(1)
                ds = re.search(r"\{\{(?:DEFAULTSORT|デフォルトソート):([^}]+)\}\}", content)
                rec["defaultsort"] = ds.group(1).strip() if ds else ""
            else:
                rec["intro"] = (p.get("extract") or "")[:400]
        out[w] = rec
    print(i + len(batch), "/", len(words), flush=True)
with open(sys.argv[2], "w", encoding="utf-8") as f:
    for w in words: f.write(json.dumps(out[w], ensure_ascii=False) + "\n")
print("fetched_at", time.strftime("%Y-%m-%d %H:%M:%S %z"))
