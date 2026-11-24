"""2026-11-07 C: 仕事 43。カテゴリの記事ごとの直近 12 か月 (2025-11〜2026-10) の月ごとの閲覧数 (人の閲覧、全端末) を、Wikimedia REST API の
pageviews で取る。決まり: User-Agent の決まり (連絡先つきの名乗り)・API usage guidelines (並列で頼まない・高い頻度で頼まない・止める指示に従う)。
www.wikimedia.org の robots.txt は 404 (断りなし、2026-11-07 に確かめた)。1 つずつ 2 秒おき、429 なら 60 秒待つ。リダイレクトの語は行き先の記事の数も取る。
使い方: python3 -I fetch_pageviews.py <src/articles.jsonl> <out.tsv>"""
import json, sys, time, urllib.error, urllib.parse, urllib.request
UA = "HaniWord-dict-tools/0.1 (https://github.com/supertastybanana3)"
BASE = "https://wikimedia.org/api/rest_v1/metrics/pageviews/per-article/ja.wikipedia/all-access/user/{}/monthly/2025110100/2026103100"
def views(title):
    url = BASE.format(urllib.parse.quote(title.replace(" ", "_"), safe=""))
    for attempt in range(3):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": UA}), timeout=60) as r:
                items = json.loads(r.read().decode("utf-8")).get("items", [])
            time.sleep(2)
            return [i["views"] for i in items]
        except urllib.error.HTTPError as e:
            time.sleep(2)
            if e.code == 404: return []   # その期間に閲覧の記録が無い
            if e.code == 429: time.sleep(60); continue
            raise
    return None
import re
rows = [json.loads(l) for l in open(sys.argv[1], encoding="utf-8")]
with open(sys.argv[2], "w", encoding="utf-8") as f:
    f.write("# 記事の題\t12 か月の合計\t記録のある月の数\t月の平均\tリダイレクトの行き先\t行き先の 12 か月の合計\t行き先の月の平均\n")
    for r in rows:
        v = views(r["title"])
        tgt, tv = "", None
        m = re.match(r"#(?:REDIRECT|転送)\s*\[\[([^\]#]+)", r.get("intro") or "", re.I)
        if m:
            tgt = m.group(1).strip(); tv = views(tgt)
        def fmt(x): return ("", "", "") if x is None else (str(sum(x)), str(len(x)), str(round(sum(x) / len(x))) if x else "0")
        a = fmt(v); b = fmt(tv) if tgt else ("", "", "")
        f.write(f"{r['title']}\t{a[0]}\t{a[1]}\t{a[2]}\t{tgt}\t{b[0]}\t{b[2]}\n"); f.flush()
print(len(rows), "articles")
