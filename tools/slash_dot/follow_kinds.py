"""2026-11-05 A (仕事 22 の 2、まとめ役 12:37): 数字＋/＋数字 のすぐ後ろを、語ではなく種類でまとめて数える (語の表 follow_*.txt に入らない所だけ)。
種類: 数の位 (百・千・万・億・兆 で始まる)・助数詞 (counter_counts.tsv の表記で始まる。長い表記を先に)・漢字 1 字 (後ろにかなか記号・端が続く漢字 1 字)・そのほか。
種類ごとに ・ と / を数え、合計 100 以上・・ の割合 0.9 以上の種類を ・ にする候補にする。試験でない記事で選び、試験の記事で確かめる
  python3 -I follow_kinds.py <dev の digit_pair_follow.tsv> <test の digit_pair_follow.tsv> <語の表 follow_*.txt> <counter_counts.tsv>"""
import collections, sys

dev_path, test_path, table_path, counters_path = sys.argv[1:5]
words = [l.split("\t")[0] for l in open(table_path, encoding="utf-8") if not l.startswith(("#", "-"))]
counters = sorted({l.split("\t")[0] for l in open(counters_path, encoding="utf-8") if not l.startswith("#") and l.strip()}, key=len, reverse=True)
PLACES = "百千万億兆"


def kind(key):
    if ":" in key: return None
    if any(key.startswith(w) for w in words): return "語の表"
    if key[0] in PLACES: return "数の位"
    c = next((c for c in counters if key.startswith(c)), None)
    if c: return "助数詞"
    if len(key) == 1 and "一" <= key <= "鿿": return "漢字1字"
    return "そのほか"


def load(path):
    c = collections.defaultdict(lambda: [0, 0]); ex = collections.defaultdict(collections.Counter)
    for l in open(path, encoding="utf-8"):
        if l.startswith("#"): continue
        k, sep, n = l.rstrip("\n").split("\t")
        kd = kind(k)
        if kd is None: continue
        c[kd][0 if sep == "・" else 1] += int(n)
        ex[kd][k] += int(n)
    return c, ex


for name, path in (("dev", dev_path), ("test", test_path)):
    c, ex = load(path)
    print(f"== {name}")
    for kd, (d, s) in sorted(c.items(), key=lambda kv: -sum(kv[1])):
        t = d + s
        print(f"{kd}\t・ {d}\t/ {s}\t・ の割合 {d / t:.3f}\t例 {' '.join(k for k, _ in ex[kd].most_common(8))}")
