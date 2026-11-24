"""collect_wikipedia.py の出力 (記事の題名と冒頭の読み) から、分野辞書の語の候補を選ぶ。
残すのは、ほかの材料 (--also の TSV の 1 列目。法令・判決から集めた語) にも出てくる題名か、--cats に挙げたカテゴリの直下の記事の題名。
人物の記事 (冒頭の括弧に生年がある。--keep-persons で残す)・個別の事件の記事 (--drop-cases のとき)・写経で測る記事 (--holdout) は除く。
出力 (TSV): collect_egov.py と同じ列 (表記 / 0 / 0 / 出どころ wiki / 冒頭の読み / 例 (記事名))。draft_readings.py にそのまま渡せる
  さらに --sources に「表記<TAB>Wikipedia:<記事名>」を書く (出典の一覧に使う)
使い方: python3 wiki_candidates.py <pages.jsonl> <出力.tsv> --also <TSV>... [--cats 日本の法律 法律用語] [--sources <sources.tsv>]
"""
import argparse, json, os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("pages")
    ap.add_argument("out")
    ap.add_argument("--also", nargs="*", default=[])
    ap.add_argument("--cats", nargs="*", default=[])
    ap.add_argument("--cat-pattern", help="記事のカテゴリの名前がこの正規表現に合えば残す (法律なら 法|訴訟|裁判|司法|権利)")
    ap.add_argument("--sources")
    ap.add_argument("--keep-persons", action="store_true", help="人物の記事も残す (歴史)")
    ap.add_argument("--drop-cases", action="store_true", help="個別の事件・判決の記事を除く (法律)")
    ap.add_argument("--holdout", default=os.environ.get("HANIWORD_HOLDOUT", ""))
    a = ap.parse_args()
    from collect_wikipedia import holdout_titles
    hold = holdout_titles(a.holdout)
    also = set()
    for path in a.also:
        for line in open(path, encoding="utf-8"):
            if not line.startswith("#"):
                also.add(line.split("\t", 1)[0].rstrip("\n"))
    kept, skipped = [], {"person": 0, "case": 0, "shape": 0, "unrelated": 0}
    for line in open(a.pages, encoding="utf-8"):
        p = json.loads(line)
        term, lead = p["term"], p.get("lead", "")
        if not (2 <= len(term) <= 20) or not common.KANJI.search(term) or re.search(r"[0-9A-Za-z（）()「」・]", term):
            skipped["shape"] += 1
            continue
        if p["title"] in hold or term in hold:  # 写経で測る記事 (収集のあとで B が足した記事も除く)
            skipped["holdout"] = skipped.get("holdout", 0) + 1
            continue
        if not a.keep_persons and re.search(r"[（(][^）)]*\d{3,4}年", lead[:120]):
            skipped["person"] += 1
            continue
        if a.drop_cases and term.endswith(("事件", "判決", "訴訟事件")):
            skipped["case"] += 1
            continue
        if term not in also and not set(p.get("cats", [])) & set(a.cats) \
                and not (a.cat_pattern and any(re.search(a.cat_pattern, c) for c in p.get("cats", []))):
            skipped["unrelated"] += 1
            continue
        kept.append((term, common.clean_reading(p.get("reading", "")) or "", p["title"]))
    with open(a.out, "w", encoding="utf-8") as out:
        for term, reading, title in kept:
            out.write(f"{term}\t0\t0\twiki\t{reading}\tWikipedia: {title}\n")
    if a.sources:
        with open(a.sources, "w", encoding="utf-8") as out:
            for term, _, title in kept:
                out.write(f"{term}\tWikipedia:{title}\n")
    print("kept", len(kept), skipped, file=sys.stderr)


if __name__ == "__main__":
    main()
