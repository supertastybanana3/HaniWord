"""情報処理学会 情報入試委員会「情報科全教科書用語リスト 第2版」(CC BY-SA 4.0。配布元のページと表の「解説」シートで 2026-10-21 に確認:
https://sites.google.com/a.ipsj.or.jp/ipsjjn/resources/wordlist-v2) から、情報の分野の語の候補を作る。
高校の情報科の全教科書の索引の語。同じ語の書き方は「10進法|十進法」のように | で区切ってあるので、漢字を含むものをそれぞれ候補にする。
「率」(教科書のうち載っているものの割合) を 100 倍して文書の数の列に入れる (よく出る語を先に並べるため)。
出力: collect_egov.py と同じ列 (表記 / 率×100 / 0 / 出どころ ipsj / 読み (無し) / 例 (カテゴリ))
使い方: python3 ipsj_candidates.py <wordlist.xlsx> <出力.tsv> [--holdout <測りの組の .py>]
"""
import argparse, os, sys

import openpyxl

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common
from collect_wikipedia import holdout_titles


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("xlsx")
    ap.add_argument("out")
    ap.add_argument("--holdout", default=os.environ.get("HANIWORD_HOLDOUT", ""))
    a = ap.parse_args()
    hold = holdout_titles(a.holdout)
    ws = openpyxl.load_workbook(a.xlsx, read_only=True).worksheets[0]
    rows = list(ws.iter_rows(values_only=True))
    head = list(rows[0])
    i_term, i_cat, i_rate = head.index("用語"), head.index("カテゴリ"), head.index("率3.1")
    best = {}
    for r in rows[1:]:
        if not r[i_term]:
            continue
        try:
            rate = int(round(float(r[i_rate] or 0) * 100))
        except ValueError:
            rate = 0
        for t in str(r[i_term]).split("|"):
            t = common.normalize_term(t.strip())
            if not t or t in hold or not (2 <= len(t) <= 16) or not common.KANJI.search(t):
                continue
            if t not in best or rate > best[t][0]:
                best[t] = (rate, r[i_cat] or "")
    with open(a.out, "w", encoding="utf-8") as out:
        for t, (rate, cat) in sorted(best.items(), key=lambda x: (-x[1][0], x[0])):
            out.write(f"{t}\t{rate}\t0\tipsj\t\t情報科全教科書用語リスト: {cat}\n")
    print("candidates", len(best))


if __name__ == "__main__":
    main()
