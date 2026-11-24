"""「今の辞書で既に正しく変換できる語」を捨てる試験 (convert.py) のための、仮の読みを付ける。
仮の読みは捨てる判定にだけ使い、辞書に入れる読みにはしない (辞書の読みは luna_readings.py で付ける)。
仮の読みの出どころ: e-Gov の法令名の読み > 変換器の辞書の読み (同じ表記のいちばん強い項目) > Sudachi の読み。
仮の読みが誤っていれば変換は正しく出ないので、語は捨てられずに残る (luna で読みを付け直してもう一度試す)。
使い方: python draft_readings.py <候補.tsv (1 列目が表記、5 列目が e-Gov の読み)> <変換器の辞書の全件.tsv> <出力.tsv> [--min-laws N]
出力 (TSV): 仮の読み<TAB>表記<TAB>読みの出どころ  (convert.py の入力にそのまま使える)
"""
import argparse, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("candidates")
    ap.add_argument("dict_all")
    ap.add_argument("out")
    a = ap.parse_args()
    best = {}
    for line in open(a.dict_all, encoding="utf-8"):
        c = line.rstrip("\n").split("\t")
        if len(c) >= 6:
            v = float(c[5])
            if c[0] not in best or v > best[c[0]][1]:
                best[c[0]] = (c[1], v)
    from sudachipy import SplitMode
    n = 0
    with open(a.out, "w", encoding="utf-8") as out:
        for line in open(a.candidates, encoding="utf-8"):
            if line.startswith("#") or not line.strip():
                continue
            c = line.rstrip("\n").split("\t")
            term, kana = c[0], (c[4] if len(c) > 4 else "")
            r, src = common.clean_reading(kana), "egov"
            if not r and term in best:
                r, src = best[term][0], "dict"
            if not r:
                r, src = common.clean_reading("".join(m.reading_form() for m in common.tokenizer().tokenize(term, SplitMode.C))), "sudachi"
            if r:
                out.write(f"{r}\t{term}\t{src}\n")
                n += 1
    print("written", n, file=sys.stderr)


if __name__ == "__main__":
    main()
