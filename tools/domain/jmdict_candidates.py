"""JMdict (EDRDG、CC BY-SA 4.0。https://www.edrdg.org/edrdg/licence.html を 2026-10-20 に確認) の分野の印 (<field>) が付いた語から、
分野の語の候補を作る。印の例: chem (化学)・math (数学)・physics (物理)・comp (情報)・geom (図形)・astron (天文)・geol (地学)。
漢字を含む表記だけ。まれな書き方・不規則な書き方の印 (iK・oK・rK・sK・ik) が付いた表記と読みは使わない。
JMdict の読みは下書きとしてだけ使う (辞書の読みは luna で付け直す)。
出力: collect_egov.py と同じ列 (表記 / 0 / 0 / 出どころ jmdict / 読み / 例 (印と英語の訳))
使い方: python3 jmdict_candidates.py <JMdict_e.gz> <出力.tsv> --fields chem [--holdout <測りの組の .py>]
"""
import argparse, gzip, os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common
from collect_wikipedia import holdout_titles

RARE = re.compile(r"&(iK|oK|rK|sK|ik|ok|rk|sk);")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("jmdict")
    ap.add_argument("out")
    ap.add_argument("--fields", nargs="+", required=True)
    ap.add_argument("--holdout", default=os.environ.get("HANIWORD_HOLDOUT", ""))
    a = ap.parse_args()
    hold = holdout_titles(a.holdout)
    data = gzip.open(a.jmdict, "rt", encoding="utf-8").read()
    seen = set()
    n = 0
    with open(a.out, "w", encoding="utf-8") as out:
        for entry in data.split("<entry>")[1:]:
            fields = set(re.findall(r"<field>&([a-zA-Z-]+);</field>", entry))
            hit = sorted(fields & set(a.fields))
            if not hit:
                continue
            kebs = [common.normalize_term(k) for k, rest in re.findall(r"<k_ele>\s*<keb>([^<]+)</keb>(.*?)</k_ele>", entry, re.S)
                    if not RARE.search(rest)]
            rebs = [r for r, rest in re.findall(r"<r_ele>\s*<reb>([^<]+)</reb>(.*?)</r_ele>", entry, re.S)
                    if not RARE.search(rest) and "<re_nokanji/>" not in rest]
            reading = common.clean_reading(rebs[0]) if rebs else None
            gloss = "; ".join(re.findall(r"<gloss>([^<]+)</gloss>", entry)[:3])
            for k in kebs:
                if not k or k in seen or k in hold or not (2 <= len(k) <= 16) or not common.KANJI.search(k):
                    continue
                seen.add(k)
                out.write(f"{k}\t0\t0\tjmdict\t{reading or ''}\tJMdict {'/'.join(hit)}: {gloss}\n")
                n += 1
    print("candidates", n)


if __name__ == "__main__":
    main()
