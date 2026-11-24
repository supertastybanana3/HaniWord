"""2026-11-07 C: 仕事 43 (B)。候補の記事ごとの月ごとの閲覧数 (人の閲覧・全端末、2015-07〜2026-10) を Wikimedia の Analytics API で取り、
直近 12 か月 (2025-11〜2026-10) の月の平均と、いちばん多かった月を出す。1 つずつ 2 秒おき、429 なら待つ (access policy)。書いた題から続ける。
使い方: python3 -I fetch_views.py <b_candidates.tsv (1 列目が題、10 列目が空の行だけ)> <out.tsv>"""
import json, os, sys, time, urllib.error, urllib.parse, urllib.request
UA = "HaniWord-dict-tools/0.1 (https://github.com/supertastybanana3)"
BASE = "https://wikimedia.org/api/rest_v1/metrics/pageviews/per-article/ja.wikipedia/all-access/user/{}/monthly/2015070100/2026103100"
src, out = sys.argv[1:3]
titles = [c[0] for c in (l.rstrip("\n").split("\t") for l in open(src, encoding="utf-8") if not l.startswith("#")) if len(c) > 9 and not c[9]]
done = {l.split("\t")[0] for l in open(out, encoding="utf-8")} if os.path.exists(out) else set()
with open(out, "a", encoding="utf-8") as f:
    for t in titles:
        if t in done: continue
        url = BASE.format(urllib.parse.quote(t.replace(" ", "_"), safe=""))
        v = None
        for _ in range(4):
            try:
                with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": UA}), timeout=60) as r:
                    v = [(i["timestamp"][:6], i["views"]) for i in json.loads(r.read().decode("utf-8")).get("items", [])]
                break
            except urllib.error.HTTPError as e:
                if e.code == 404: v = []; break
                if e.code == 429: time.sleep(60); continue
                raise
        time.sleep(2)
        last = [n for ym, n in (v or []) if "202511" <= ym <= "202610"]
        avg = round(sum(last) / len(last)) if last else 0
        pk = max(v, key=lambda x: x[1]) if v else ("", 0)
        f.write(f"{t}\t{avg}\t{pk[0][:4]}-{pk[0][4:]}\t{pk[1]}\n"); f.flush()
print("done", time.strftime("%Y-%m-%d %H:%M:%S %z"))
