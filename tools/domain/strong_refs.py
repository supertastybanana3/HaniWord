"""読みの手がかり (参考の読み) を、人が付けた辞書の読みから足す。reference_readings.py の出力と同じ形 (表記<TAB>読み<TAB>出どころ)。
Codex の枠を節約するため、ここで読みがわかる語は luna の 2 回目・Web で確かめる回に回さない (luna_readings.py todo_lite)。
  - Mozc の公開辞書 (data_ext/mozc_oss/dictionary*.txt。表記が完全に一致する項目の読み)
  - 候補の TSV の 5 列目の読みのうち、人が付けたもの: JMdict (jmdict)・Wikidata の名前のカナ表記 (wikidata)・地名集日本 (gsi)・
    e-Gov の法令の読み (egov の見出し・定義語)・JST シソーラスの見出し語 (jst。同義語の読みは見出し語のものなので使わない → jst_candidates.py が見出し語にだけ読みを書く)
使い方: python3 strong_refs.py --terms <束の terms.tsv> --out <refs_strong.tsv> [--mozc <mozc_oss のフォルダ>] [--candidates <候補.tsv>...]
  --mozc を省くと、環境変数 HANIWORD_BUILD (作業場所) の data_ext/mozc_oss を読む
"""
import argparse, glob, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common

TRUSTED = {"jmdict", "wikidata", "gsi", "jst", "title", "abbrev", "ruby", "defined"}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--terms", required=True)
    ap.add_argument("--out", required=True)
    build = os.environ.get("HANIWORD_BUILD")
    ap.add_argument("--mozc", default=os.path.join(build, "data_ext", "mozc_oss") if build else None, required=not build)
    ap.add_argument("--candidates", nargs="*", default=[])
    a = ap.parse_args()
    terms = {l.split("\t", 1)[0].rstrip("\n") for l in open(a.terms, encoding="utf-8") if l.strip()}
    refs = set()
    for path in sorted(glob.glob(os.path.join(a.mozc, "dictionary*.txt"))):
        for line in open(path, encoding="utf-8"):
            c = line.rstrip("\n").split("\t")
            if len(c) >= 5 and c[4] in terms:
                r = common.clean_reading(c[0])
                if r:
                    refs.add((c[4], r, "mozc"))
    for path in a.candidates:
        for line in open(path, encoding="utf-8"):
            c = line.rstrip("\n").split("\t")
            if len(c) < 5 or not c[4]:
                continue
            t = common.normalize_term(c[0]) or c[0]
            if t in terms and set(c[3].split(",")) & TRUSTED:
                r = common.clean_reading(c[4])
                if r:
                    refs.add((t, r, "cand:" + "+".join(sorted(set(c[3].split(",")) & TRUSTED))))
    with open(a.out, "w", encoding="utf-8") as out:
        out.writelines(f"{t}\t{r}\t{s}\n" for t, r, s in sorted(refs))
    print("refs", len(refs), "terms with refs", len({t for t, _, _ in refs}), "of", len(terms))


if __name__ == "__main__":
    main()
