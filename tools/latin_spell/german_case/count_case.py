"""2026-11-05 C: 仕事 24 のドイツ語の名詞の頭の大文字 (まとめ役 14:25)。FineWeb-2 のドイツ語の文 (次の語の候補の表を作った元) から、文の途中の語について数える:
  語ごと: 小文字の語 → 大文字で始まる回数・全部の回数 (全部大文字の語は数えない)
  前の語ごと: 前の語 (小文字) → 次に来た「大文字と小文字の両方で書かれる語」(大文字の割合 5〜95%) が大文字だった回数・全部の回数
文の途中: 行の頭と . ! ? : のあとの語は数えない (前の語としては使う)。
使い方: python3 -I count_case.py <fw2_train.txt> <読むバイト数> <出力のフォルダ>"""
import collections, re, sys
src, limit, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
TOKEN = re.compile(r"[^\W\d_]+|[.!?:;,]")
cap = collections.Counter(); tot = collections.Counter()
lines = []
read = 0
with open(src, encoding="utf-8", errors="replace") as f:
    for line in f:
        read += len(line)
        if read > limit:
            break
        lines.append(line)
# 1 回目: 語ごとの大文字の割合
for line in lines:
    start = True
    for m in TOKEN.finditer(line):
        t = m.group(0)
        if t in ".!?:":
            start = True; continue
        if t in ";,":
            continue
        if not start and not (len(t) > 1 and t.isupper()):
            w = t.lower(); tot[w] += 1
            if t[0].isupper(): cap[w] += 1
        start = False
share = {w: cap[w] / n for w, n in tot.items() if n >= 20}
ambiguous = {w for w, s in share.items() if 0.05 <= s <= 0.95}
# 2 回目: 前の語ごとに、次の両方の書き方がある語が大文字だった割合
acap = collections.Counter(); atot = collections.Counter()
pcap = collections.Counter(); ptot = collections.Counter()
for line in lines:
    start = True; prev = None
    for m in TOKEN.finditer(line):
        t = m.group(0)
        if t in ".!?:":
            start = True; prev = None; continue
        if t in ";,":
            prev = None; continue
        w = t.lower()
        if not start and prev is not None and w in ambiguous and not (len(t) > 1 and t.isupper()):
            atot[prev] += 1
            ptot[(prev, w)] += 1
            if t[0].isupper(): acap[prev] += 1; pcap[(prev, w)] += 1
        start = False; prev = w
glob = sum(acap.values()) / max(1, sum(atot.values()))
with open(f"{out}/word_share.tsv", "w", encoding="utf-8") as f:
    for w, s in sorted(share.items()):
        if s >= 0.05:
            f.write(f"{w}\t{s:.4f}\t{tot[w]}\n")
with open(f"{out}/after_rate.tsv", "w", encoding="utf-8") as f:
    f.write(f"#global\t{glob:.4f}\t{sum(atot.values())}\n")
    for p, n in sorted(atot.items(), key=lambda x: -x[1]):
        if n >= 200:
            f.write(f"{p}\t{acap[p] / n:.4f}\t{n}\n")
with open(f"{out}/pair_share.tsv", "w", encoding="utf-8") as f:
    for (p, w), n in sorted(ptot.items()):
        if n >= 10:
            f.write(f"{p} {w}\t{pcap[(p, w)] / n:.4f}\t{n}\n")
print("組", sum(1 for n in ptot.values() if n >= 10))
print("読んだ行", len(lines), "語", len(share), "両方の語", len(ambiguous), "前の語", sum(1 for n in atot.values() if n >= 200), "全体の大文字の割合", round(glob, 4))
