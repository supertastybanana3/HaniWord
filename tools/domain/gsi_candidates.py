"""国土地理院「地名集日本」(2021 年更新。公共データ利用規約 第 1.0 版 (PDL1.0)。https://www.gsi.go.jp/kihonjohochousa/gazetteer.html と
https://www.gsi.go.jp/kikakuchousei/kikakuchousei40182.html を 2026-10-20 に確認) の PDF から、地理の分野の語の候補を作る。
pdftotext -layout で表を文字にし、「番号 / 格子 / 漢字 / かな / ローマ字 / 緯度 / 経度 / 種類」の行を読む。海底地形は除く (ふつうは打たない)。
かなは下書きの読みにする (辞書の読みは luna で付け直す)。
出力: collect_egov.py と同じ列 (表記 / 0 / 0 / 出どころ gsi / 読み / 例 (種類))
使い方: python3 gsi_candidates.py <地名集日本.pdf> <出力.tsv> [--holdout <測りの組の .py>]
"""
import argparse, os, re, subprocess, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common
from collect_wikipedia import holdout_titles

ROW = re.compile(r"^\s*\d+\s+(?:\d{4}\s+)?(\S+)\s+([ぁ-ゖー]+)\s+.+?\s+\d+°\d+'\s+\d+°\d+'\s+(.+?)\s*$")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("pdf")
    ap.add_argument("out")
    ap.add_argument("--holdout", default=os.environ.get("HANIWORD_HOLDOUT", ""))
    a = ap.parse_args()
    hold = holdout_titles(a.holdout)
    text = subprocess.run(["pdftotext", "-layout", a.pdf, "-"], capture_output=True, text=True, errors="replace").stdout
    seen = {}
    kinds = {}
    for line in text.splitlines():
        m = ROW.match(line)
        if not m:
            continue
        term, kana, kind = common.normalize_term(m.group(1)), m.group(2), m.group(3)
        kinds[kind] = kinds.get(kind, 0) + 1
        if not term or kind == "Undersea feature" or term in hold or not common.KANJI.search(term) or term in seen:
            continue
        seen[term] = (common.to_katakana(kana), kind)
    with open(a.out, "w", encoding="utf-8") as out:
        for t, (r, kind) in seen.items():
            out.write(f"{t}\t0\t0\tgsi\t{r}\t地名集日本: {kind}\n")
    print("rows by kind", kinds)
    print("candidates", len(seen))


if __name__ == "__main__":
    main()
