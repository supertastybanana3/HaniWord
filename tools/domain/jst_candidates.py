"""NBDC の「科学技術用語形態素解析辞書」(JST シソーラス 2015 年版の見出し語と同義語、CC BY-SA 4.0) から、分野の語の候補を作る。
クレジット: 科学技術用語形態素解析辞書 © バイオサイエンスデータベースセンター licensed under CC表示-継承4.0 国際
  (https://dbarchive.biosciencedbc.jp/jp/mecab/lic.html を 2026-10-20 に確認)
主題カテゴリーの記号 (例 LS = ライフサイエンス) の頭で選ぶ。漢字を含まない語・英字を含む語は除く。同義語 (V) の読みは見出し語の読みのことがあるので、
読みは下書きとしてだけ使う (辞書の読みは luna で付け直す)。
出力: collect_egov.py と同じ列 (表記 / 0 / 0 / 出どころ jst / 読み (見出し語のときだけ) / 例 (カテゴリー))
使い方: python3 jst_candidates.py <thesaurus_utf8.csv> <出力.tsv> --codes LS [--holdout <測りの組の .py>]
"""
import argparse, csv, os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common
from collect_wikipedia import holdout_titles


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("csv")
    ap.add_argument("out")
    ap.add_argument("--codes", nargs="+", required=True)
    ap.add_argument("--holdout", default=os.environ.get("HANIWORD_HOLDOUT", ""))
    a = ap.parse_args()
    hold = holdout_titles(a.holdout)
    rows = list(csv.reader(open(a.csv, encoding="utf-8")))[1:]
    best = {}
    for r in rows:
        surface, reading, flag, codes = r[0], r[11], r[16], r[17]
        if not any(c.startswith(tuple(a.codes)) for c in re.split(r"[ /]", codes)):
            continue
        if surface in hold or not (2 <= len(surface) <= 16) or not common.KANJI.search(surface) \
                or re.search(r"[0-9A-Za-z０-９Ａ-Ｚａ-ｚ,，、（）()・/／]", surface) or not common.plausible(surface):
            continue
        reading = common.clean_reading(reading) if flag == "C" else ""
        # 見出し語を優先
        if surface not in best or (flag == "C" and best[surface][1] != "C"):
            best[surface] = (reading or "", flag, codes)
    with open(a.out, "w", encoding="utf-8") as out:
        for surface, (reading, flag, codes) in sorted(best.items()):
            out.write(f"{surface}\t0\t0\tjst\t{reading}\tJST シソーラス {codes} ({'見出し語' if flag == 'C' else '同義語'})\n")
    print("terms", len(best), file=sys.stderr)


if __name__ == "__main__":
    main()
