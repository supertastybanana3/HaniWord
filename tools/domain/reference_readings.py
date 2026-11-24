"""luna の読みと照らし合わせる「よそで確かめられる読み」を集める (表記<TAB>読み (カタカナ)<TAB>出どころ)。
配布物には入れない (照らし合わせにだけ使う)。
  - 日本語版ウィキペディアの記事の冒頭の括弧の読み (collect_wikipedia.py の出力)
  - mozcdic-ut-jawiki (ウィキペディアの題名と読みの辞書。1 列目が読み・5 列目が表記)
  - e-Gov の法令名の読み (collect_egov.py の出力の 5 列目)
使い方: python3 reference_readings.py --terms <表記の一覧 (1 列目)> --out <refs.tsv> [--wiki x.jsonl ...] [--jawiki mozcdic-ut-jawiki.txt.bz2] [--egov egov_terms.tsv]
"""
import argparse, bz2, json, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--terms", nargs="+", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--wiki", nargs="*", default=[])
    ap.add_argument("--jawiki")
    ap.add_argument("--egov")
    a = ap.parse_args()
    terms = set()
    for path in a.terms:
        for line in open(path, encoding="utf-8"):
            if line.strip() and not line.startswith("#"):
                terms.add(line.split("\t", 1)[0].rstrip("\n"))
    refs = set()
    for path in a.wiki:
        for line in open(path, encoding="utf-8"):
            r = json.loads(line)
            reading = common.clean_reading(r.get("reading", ""))
            if r["term"] in terms and reading:
                refs.add((r["term"], reading, "wikipedia"))
    if a.jawiki:
        for line in bz2.open(a.jawiki, "rt", encoding="utf-8"):
            c = line.rstrip("\n").split("\t")
            if len(c) >= 5 and c[4] in terms:
                reading = common.clean_reading(c[0])
                if reading:
                    refs.add((c[4], reading, "jawiki"))
    if a.egov:
        for line in open(a.egov, encoding="utf-8"):
            c = line.rstrip("\n").split("\t")
            if len(c) > 4 and c[0] in terms and c[4] and "title" in c[3].split(","):
                reading = common.clean_reading(c[4])
                if reading:
                    refs.add((c[0], reading, "egov"))
    with open(a.out, "w", encoding="utf-8") as out:
        for t, r, s in sorted(refs):
            out.write(f"{t}\t{r}\t{s}\n")
    print("refs", len(refs), "terms with refs", len({t for t, _, _ in refs}), "of", len(terms), file=sys.stderr)


if __name__ == "__main__":
    main()
