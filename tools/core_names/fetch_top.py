"""2026-11-07 C: 仕事 43・44 (流行語・固有名詞)。日本語版ウィキペディアの月ごとの閲覧数の上位 1,000 記事 (Wikimedia Analytics API の
pageviews/top、人の閲覧・全端末) を、記録のある 2015-07 から 2026-10 まで月 1 回ずつ取る。
決まり (2026-11-07 に開いて確かめた): access policy (User-Agent に連絡先・1 つの頼みが終わってから次を送る・データは CC0)、
wikimedia.org の robots.txt は www.wikimedia.org へ転送されて 404 (断りなし)。1 つずつ 2 秒おき、429 なら待つ。
出力 JSONL (1 行 1 か月): month, articles [[題, 閲覧, 順位], ...]
使い方: python3 -I fetch_top.py <out.jsonl>"""
import json, sys, time, urllib.error, urllib.parse, urllib.request
UA = "HaniWord-dict-tools/0.1 (https://github.com/supertastybanana3)"
BASE = "https://wikimedia.org/api/rest_v1/metrics/pageviews/top/ja.wikipedia/all-access/{}/{:02d}/all-days"
out = sys.argv[1]
done = {json.loads(l)["month"] for l in open(out, encoding="utf-8")} if __import__("os").path.exists(out) else set()
with open(out, "a", encoding="utf-8") as f:
    y, m = 2015, 7
    while (y, m) <= (2026, 10):
        key = f"{y}-{m:02d}"
        if key not in done:
            for _ in range(5):
                try:
                    with urllib.request.urlopen(urllib.request.Request(BASE.format(y, m), headers={"User-Agent": UA}), timeout=60) as r:
                        items = json.loads(r.read().decode("utf-8"))["items"][0]["articles"]
                    f.write(json.dumps({"month": key, "articles": [[a["article"], a["views"], a["rank"]] for a in items]}, ensure_ascii=False) + "\n"); f.flush()
                    break
                except urllib.error.HTTPError as e:
                    if e.code == 429: time.sleep(int(e.headers.get("Retry-After") or 60)); continue
                    if e.code == 404: print("no data", key); break
                    raise
            time.sleep(2)
        m += 1
        if m > 12: y, m = y + 1, 1
print("done", time.strftime("%Y-%m-%d %H:%M:%S %z"))
