"""万病辞書 MANBYO (NAIST ソーシャル・コンピューティング研究室、CC BY 4.0) から、医学の分野辞書の語の候補を作る。
https://sociocom.naist.jp/manbyou-dic/ (2026-10-20 に確認: 「本成果物は クリエイティブ・コモンズ 表示 4.0 国際 ライセンス の下に提供されています」)
- 信頼度レベル S の行は ICD10 対応標準病名マスター (MEDIS) の病名と読みなので使わない (MEDIS は配布目的の利用に許諾の申請が要る)
- 標準病名・ICD コードの列 (MEDIS の標準病名マスター由来) も使わない。使うのは S 以外の行の「出現形」と「出現形よみ」(読みは下書き) だけ
- 頻度レベルが下位 5% (0-5%)・出現なし の行は使わない (カルテの書き方の揺れがほとんど)
出力: collect_egov.py と同じ列 (表記 / 0 / 0 / 出どころ manbyo / 出現形よみ / 例)
使い方: python3 manbyo_candidates.py <MANBYO_*.csv> <出力.tsv> [--holdout <測りの組の .py>]
"""
import argparse, csv, os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common
from collect_wikipedia import holdout_titles


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("csv")
    ap.add_argument("out")
    ap.add_argument("--holdout", default=os.environ.get("HANIWORD_HOLDOUT", ""))
    a = ap.parse_args()
    hold = holdout_titles(a.holdout)
    rows = list(csv.reader(open(a.csv, encoding="utf-8-sig")))
    head = rows[0]
    i_word, i_read, i_level, i_freq = head.index("出現形"), head.index("出現形よみ"), head.index("信頼度レベル"), head.index("頻度レベル")
    kept = {}
    for r in rows[1:]:
        if r[i_level] == "S" or r[i_freq] in ("0-5%", "出現なし"):
            continue
        word = r[i_word].strip()
        if word in hold or not (2 <= len(word) <= 16) or not common.KANJI.search(word) \
                or re.search(r"[0-9A-Za-z０-９Ａ-Ｚａ-ｚ,，、（）()・/／]", word) or not common.plausible(word):
            continue
        kept.setdefault(word, (common.clean_reading(r[i_read]) or "", r[i_freq]))
    with open(a.out, "w", encoding="utf-8") as out:
        for word, (reading, freq) in sorted(kept.items()):
            out.write(f"{word}\t0\t0\tmanbyo\t{reading}\t万病辞書 (頻度 {freq})\n")
    print("terms", len(kept), file=sys.stderr)


if __name__ == "__main__":
    main()
