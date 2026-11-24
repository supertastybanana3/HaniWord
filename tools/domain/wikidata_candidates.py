"""collect_wikidata.py の出力から、歴史 (history。日本史と世界史をまとめた 1 つの分野) の語の候補を作る。
日本史と世界史に分けないのは、国の無い項目 (元号など) が日本か中国か決められず、振り分けを誤りやすいため。
片仮名だけの語 (打てば片仮名になる)・漢字を含まない語・括弧つきの語・写経で測る記事 (--holdout の ARTICLE_SETS) の題名は除く。
出力: <出力の頭>.tsv (collect_egov.py と同じ列: 表記 / 0 / 0 / 出どころ / 読み (名前のカナ表記 P1814) / 例)、
      <出力の頭>_sources.tsv (表記<TAB>Wikidata:<Q 番号>、記事があれば 表記<TAB>Wikipedia:<記事名> も)
使い方: python3 wikidata_candidates.py <出力の頭> <collect_wikidata の出力.jsonl>... [--holdout <測りの組の .py>]
"""
import argparse, json, os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common
from collect_wikipedia import holdout_titles


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("inputs", nargs="+")
    ap.add_argument("--holdout", default=os.environ.get("HANIWORD_HOLDOUT", ""))
    ap.add_argument("--min-links", type=int, default=2, help="記事のある言語版の数がこれより少ない項目は除く (日本語版だけ = 1 はあまり知られていない)")
    a = ap.parse_args()
    hold = holdout_titles(a.holdout)
    seen = {}
    for path in a.inputs:
        for line in open(path, encoding="utf-8"):
            it = json.loads(line)
            if it["qid"] in seen:
                if it.get("kana"):
                    seen[it["qid"]]["kana"] = it["kana"]
            else:
                seen[it["qid"]] = it
    stats = {"kept": 0, "katakana_or_shape": 0, "holdout": 0, "few_links": 0}
    written = set()
    with open(f"{a.out}.tsv", "w", encoding="utf-8") as out, open(f"{a.out}_sources.tsv", "w", encoding="utf-8") as sources:
        for it in seen.values():
            term = it["label"].strip()
            if it["title"] in hold or term in hold:
                stats["holdout"] += 1
                continue
            if it.get("links", 0) < a.min_links:
                stats["few_links"] += 1
                continue
            if not (2 <= len(term) <= 20) or not common.KANJI.search(term) or re.search(r"[0-9A-Za-z（）()「」,、]", term):
                stats["katakana_or_shape"] += 1
                continue
            stats["kept"] += 1
            # 名前のカナ表記が表記と合わないことがある (武帝 → かん ぶてい) ので、読みは下書きとしてだけ使う
            kana = common.clean_reading(it.get("kana", "")) or ""
            if term not in written:
                out.write(f"{term}\t0\t0\twikidata\t{kana}\tWikidata {it['qid']} {it['kind']}" + (f" / Wikipedia: {it['title']}" if it["title"] else "") + "\n")
                written.add(term)
            sources.write(f"{term}\tWikidata:{it['qid']}\n")
            if it["title"]:
                sources.write(f"{term}\tWikipedia:{it['title']}\n")
    print(stats, "terms", len(written), file=sys.stderr)


if __name__ == "__main__":
    main()
