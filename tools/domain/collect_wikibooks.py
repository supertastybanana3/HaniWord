"""日本語版ウィキブックス (CC BY-SA 4.0。https://ja.wikibooks.org の siteinfo の rightsinfo で 2026-10-21 に確認) の高校の教科書から、
分野の語の候補 (名詞の連なり) を集める。教科書の題名の頭 (例 高等学校化学) で始まるページの本文を取り、common.chains で語を切り出す。
MediaWiki API は 1 つずつ・1 秒おきに送り、User-Agent に連絡先を書く (API:Etiquette)。
出力: collect_egov.py と同じ列 (表記 / 出てきたページの数 / 出てきた回数 / 出どころ wikibooks / 下書きの読み (Sudachi) / 例 (ページ名))
使い方: python3 collect_wikibooks.py <出力.tsv> --prefixes 高等学校化学 [--min-pages 2]
"""
import argparse, collections, json, os, sys, time, urllib.parse, urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common
from collect_wikipedia import UA

API = "https://ja.wikibooks.org/w/api.php"


def api(**q):
    q.update(format="json", maxlag="5")
    for k in range(6):
        time.sleep(1.0)
        try:
            req = urllib.request.Request(API + "?" + urllib.parse.urlencode(q), headers={"User-Agent": UA})
            return json.load(urllib.request.urlopen(req, timeout=60))
        except Exception as e:
            print("retry", k, e, file=sys.stderr, flush=True)
            time.sleep(10 * (k + 1))
    raise RuntimeError("API failed")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--prefixes", nargs="+", required=True)
    ap.add_argument("--min-pages", type=int, default=2, help="これより少ないページにしか出ない語は除く (1 ページだけの語は説明の言い回しが多い)")
    a = ap.parse_args()
    titles = []
    for prefix in a.prefixes:
        cont = {}
        while True:
            d = api(action="query", list="allpages", apprefix=prefix, aplimit="500", apfilterredir="nonredirects", **cont)
            titles += [p["title"] for p in d["query"]["allpages"]]
            if "continue" not in d:
                break
            cont = {"apcontinue": d["continue"]["apcontinue"]}
    print("pages", len(titles), file=sys.stderr, flush=True)
    pages = collections.Counter()
    count = collections.Counter()
    reading = {}
    where = {}
    for t in titles:
        d = api(action="query", prop="extracts", explaintext="1", titles=t)
        text = "".join(p.get("extract", "") for p in d["query"]["pages"].values())
        seen = set()
        for surface, r, _ in common.chains(text):
            count[surface] += 1
            reading.setdefault(surface, r)
            where.setdefault(surface, t)
            seen.add(surface)
        pages.update(seen)
    n = 0
    with open(a.out, "w", encoding="utf-8") as out:
        for w, c in pages.most_common():
            if c < a.min_pages:
                continue
            out.write(f"{w}\t{c}\t{count[w]}\twikibooks\t{common.clean_reading(reading[w]) or ''}\tウィキブックス: {where[w]}\n")
            n += 1
    print("candidates", n, file=sys.stderr)


if __name__ == "__main__":
    main()
