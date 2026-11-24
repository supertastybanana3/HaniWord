"""追加辞書の表 (extra_dictionaries/core_words.tsv など: 表記<TAB>読み<TAB>…) の読みを、ほかの読みと照らして、どれとも合わない行を書き出す
(2026-11-06。core_words の 造山帯 ツクリヤマタイ の件で作った。書き出した行は目で見て、core_words_reading_fixes.txt に直しを書く)。
照らす読み: JMdict (表記が同じ項目の読み)・Sudachi (1 語ならその読み、2 語以上なら読みをつないだもの)・変換器の辞書 (同じ表記のほかの行の読み)。
人名・地名の別の読み (豊島 テシマ) や分野の読み (小体 ショウタイ) も出てくるので、直すかどうかは目で決める。
使い方: <Sudachi のある Python> check_core_readings.py <表.tsv> <JMdict_e.gz> <変換器の辞書の全件.tsv> <出力.tsv>"""
import collections, gzip, os, re, sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "domain"))
import common
from sudachipy import SplitMode


def main():
    table, jm_path, dict_path, out = sys.argv[1:5]
    rows = [l.rstrip("\n").split("\t") for l in open(table, encoding="utf-8") if not l.startswith("#") and l.strip()]
    terms = {r[0] for r in rows}
    jm = collections.defaultdict(set)
    text = gzip.open(jm_path, "rt", encoding="utf-8").read()
    for entry in re.findall(r"<entry>(.*?)</entry>", text, re.S):
        kebs = re.findall(r"<keb>(.*?)</keb>", entry)
        if not set(kebs) & terms:
            continue
        for rele in re.findall(r"<r_ele>(.*?)</r_ele>", entry, re.S):
            reb = re.search(r"<reb>(.*?)</reb>", rele).group(1)
            restr = re.findall(r"<re_restr>(.*?)</re_restr>", rele)
            for k in kebs:
                if k in terms and (not restr or k in restr):
                    jm[k].add(common.to_katakana(reb))
    base = collections.defaultdict(set)
    for line in open(dict_path, encoding="utf-8"):
        c = line.split("\t", 2)
        if c[0] in terms:
            base[c[0]].add(c[1])
    n = collections.Counter()
    with open(out, "w", encoding="utf-8") as f:
        f.write("# 表記\t表の読み\tJMdict\tSudachi\t変換器の辞書 (ほかの行)\t扱い\n")
        for term, reading, *_ in rows:
            ms = common.tokenizer().tokenize(term, SplitMode.C)
            sud = common.to_katakana("".join(m.reading_form() for m in ms))
            others = base[term] - {reading}
            if reading in jm[term] or reading == sud or reading in others:
                n["合う"] += 1
                continue
            kind = "JMdict と合わない" if jm[term] else ("Sudachi (1 語) と合わない" if len(ms) == 1 else "Sudachi (つないだ読み) と合わない")
            n[kind] += 1
            f.write(f"{term}\t{reading}\t{','.join(sorted(jm[term]))}\t{sud}{'' if len(ms) == 1 else ' (つないだ)'}\t{','.join(sorted(others))}\t{kind}\n")
    print(dict(n))


if __name__ == "__main__":
    main()
