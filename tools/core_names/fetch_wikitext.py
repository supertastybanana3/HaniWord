"""2026-11-07 C: 仕事 44 (競走馬)。競走馬の記事の今の版の wikitext を MediaWiki の API で 50 件ずつ取る (戦績の表を読むため。手元の全文は表が落ちている)。
API:Etiquette: 連絡先つきの名乗り・1 つずつ・1 秒あける・maxlag=5・429 と maxlag なら待つ。書いた題から続ける。出力 JSONL: title, revid, timestamp, wikitext
使い方: python3 -I fetch_wikitext.py <horses.tsv> <out.jsonl>"""
import json, os, sys, time, urllib.error, urllib.parse, urllib.request
UA = "HaniWord-dict-tools/0.1 (https://github.com/supertastybanana3)"
API = "https://ja.wikipedia.org/w/api.php"
def call(**p):
    p.update(format="json", formatversion="2", maxlag="5")
    data = urllib.parse.urlencode(p).encode()
    for _ in range(8):
        try:
            with urllib.request.urlopen(urllib.request.Request(API, data=data, headers={"User-Agent": UA}), timeout=120) as r:
                d = json.loads(r.read().decode("utf-8"))
            if d.get("error", {}).get("code") == "maxlag": time.sleep(10); continue
            time.sleep(1.0); return d
        except urllib.error.HTTPError as e:
            if e.code == 429: time.sleep(int(e.headers.get("Retry-After") or 60)); continue
            raise
    raise RuntimeError("retries")
src, out = sys.argv[1:3]
titles = [l.rstrip("\n").split("\t")[1] for l in open(src, encoding="utf-8") if not l.startswith("#")]
done = {json.loads(l)["title"] for l in open(out, encoding="utf-8")} if os.path.exists(out) else set()
todo = [t for t in titles if t not in done]
with open(out, "a", encoding="utf-8") as f:
    for i in range(0, len(todo), 50):
        batch = todo[i:i + 50]
        d = call(action="query", titles="|".join(batch), prop="revisions", rvprop="ids|timestamp|content", rvslots="main", redirects="1")
        q = d.get("query", {})
        back = {}
        for n in q.get("normalized", []): back[n["to"]] = n["from"]
        for r in q.get("redirects", []): back[r["to"]] = back.get(r["from"], r["from"])
        for p in q.get("pages", []):
            rv = (p.get("revisions") or [{}])[0]
            f.write(json.dumps({"title": back.get(p["title"], p["title"]), "page": p["title"], "revid": rv.get("revid"), "timestamp": rv.get("timestamp"),
                                "wikitext": rv.get("slots", {}).get("main", {}).get("content", "")}, ensure_ascii=False) + "\n")
        f.flush()
print("done", time.strftime("%Y-%m-%d %H:%M:%S %z"))
