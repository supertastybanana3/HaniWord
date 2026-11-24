"""集めた候補と「今の辞書での変換」の結果から、luna に読みを付けさせる語を選ぶ。

捨てる語:
  ok         仮の読みで変換したら、AI ありでも辞書だけでも、その語がそのまま出た (今の辞書で足りている)。
             2026-11-06: 前は AI ありだけを見ていて、辞書だけでは外れる語 (体循環 → 帯循環) を捨てていた。ライブ変換の短い入力は辞書だけで
             変換し、文の中では AI も外す (B の目隠しの試しの dev)。辞書だけの結果が無い試し (convert.py --no-dict) は AI ありだけで見る
  homophone  変換器の辞書に同じ表記が既にあり、変換すると同じ読みの別の 1 語が出る (施行→思考)。分野辞書で重くすると一般の変換を壊し、
             軽く足しても辞書に既にある語なので変わらない
  short      1 字の語 (漢字 1 字は分野辞書に入れない)
  weak       その分野の語らしさの手がかりが足りない (法令の数・判決の数・出どころ。--min-* で決める)
出力: luna の入力 (表記<TAB>例) と、全部の語の扱い (select_log.tsv: 表記・扱い・仮の読み・変換の結果)
使い方: python3 select_terms.py --candidates <collect_egov などの TSV> --probe <convert.py の出力> --draft <draft_readings の出力>
          --dict <変換器の辞書の全件> [--courts <collect_court_terms の出力>] --out <luna の入力.tsv> --log <select_log.tsv>
"""
import argparse, collections, json, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common

STRONG = {"title", "abbrev", "ruby", "caption", "defined", "wiki", "detail", "jst", "wikidata", "manbyo", "jmdict", "ipsj", "gsi"}  # 出どころだけで分野の語と言えるもの


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--candidates", nargs="+", required=True)
    ap.add_argument("--probe", nargs="+", required=True)
    ap.add_argument("--draft", nargs="+", required=True)
    ap.add_argument("--dict", required=True)
    ap.add_argument("--courts")
    ap.add_argument("--min-laws", type=int, default=3)
    ap.add_argument("--min-cases", type=int, default=2)
    ap.add_argument("--laws-alone", type=int, default=30, help="判決に出なくても、これだけの法令に出れば残す")
    ap.add_argument("--out", required=True)
    ap.add_argument("--log", required=True)
    a = ap.parse_args()

    by_reading = collections.defaultdict(set)
    in_dict = set()
    for line in open(a.dict, encoding="utf-8"):
        c = line.rstrip("\n").split("\t")
        by_reading[c[1]].add(c[0])
        in_dict.add(c[0])
    cand = {}
    for path in a.candidates:
        for line in open(path, encoding="utf-8"):
            if line.startswith("#") or not line.strip():
                continue
            c = line.rstrip("\n").split("\t")
            term, laws, srcs, example = c[0], int(c[1]), set(c[3].split(",")), c[5] if len(c) > 5 else ""
            old = cand.get(term)
            cand[term] = (max(laws, old[0]) if old else laws, srcs | (old[1] if old else set()), (old[2] if old else "") or example)
    cases = collections.Counter()
    court_src = collections.defaultdict(set)
    if a.courts:
        for line in open(a.courts, encoding="utf-8"):
            if line.startswith("#"):
                continue
            c = line.rstrip("\n").split("\t")
            cases[c[0]] = int(c[1])
            court_src[c[0]] = set(c[3].split(","))
    draft = {}
    for path in a.draft:
        for line in open(path, encoding="utf-8"):
            r, t, s = line.rstrip("\n").split("\t")
            draft[t] = r
    probe = {}
    for path in a.probe:
        if not os.path.exists(path):  # 試験がまだ始まっていない分
            continue
        for line in open(path, encoding="utf-8"):
            try:
                r = json.loads(line)
            except ValueError:  # 書きかけの最後の行
                continue
            probe[r["expected"]] = r
    kept = 0
    reasons = collections.Counter()
    with open(a.out, "w", encoding="utf-8") as out, open(a.log, "w", encoding="utf-8") as log:
        log.write("# 表記\t扱い\t仮の読み\t今の変換\t法令の数\t判決の数\n")
        for term in sorted(cand, key=lambda t: (-cand[t][0], t)):
            laws, srcs, example = cand[term]
            p = probe.get(term)
            if p is None:
                reason = "unprobed"
            elif p["ok"] and p.get("dict", term) == term:
                reason = "ok"
            elif len(term) < 2:
                reason = "short"
            elif term in in_dict and p["full"] in by_reading[common.to_katakana(p["reading"])]:
                reason = "homophone"
            elif not (srcs | court_src[term]) & STRONG and cases[term] < a.min_cases and laws < a.laws_alone:
                reason = "weak"
            elif laws < a.min_laws and cases[term] < a.min_cases and not (srcs | court_src[term]) & STRONG:
                reason = "weak"
            else:
                reason = "luna"
            reasons[reason] += 1
            log.write(f"{term}\t{reason}\t{draft.get(term, '')}\t{p['full'] if p else ''}\t{laws}\t{cases[term]}\n")
            if reason == "luna":
                out.write(f"{term}\t{example}\n")
                kept += 1
    print(dict(reasons), "luna", kept, file=sys.stderr)


if __name__ == "__main__":
    main()
