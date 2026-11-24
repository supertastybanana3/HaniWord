"""2026-11-07 (仕事 40): 注意の注記の「含む」の鍵が、ふつうの語の中に入っている形 (非人 → 非人為的・非人道的) を、手元の Wikipedia の文から数える。
鍵が漢字だけなら、鍵の前後に続く漢字を含めた続き (非人為) を、鍵がカタカナを含むなら前後のカタカナの続きを数える。
出力 TSV: 鍵<TAB>出た数<TAB>鍵そのままの数<TAB>続きの上位 (続き:数 を | 区切り)
使い方: python3 -I scan_exclusions.py <rows.tsv (make_table の出力)> <out.tsv> <文の束.tsv> [読む行の上限]
"""
import collections, re, sys

rows, out, corpus = sys.argv[1:4]
limit = int(sys.argv[4]) if len(sys.argv) > 4 else 0
keys = sorted({l.split("\t")[2] for l in open(rows, encoding="utf-8") if l.split("\t")[1] == "含む"}, key=len, reverse=True)
pat = re.compile("|".join(map(re.escape, keys)))
KANJI = r"[々㐀-䶿一-鿿]"
KATA = r"[ァ-ヺー]"
total, exact, runs = collections.Counter(), collections.Counter(), collections.defaultdict(collections.Counter)
n = 0
with open(corpus, encoding="utf-8") as f:
    for line in f:
        n += 1
        if limit and n > limit:
            break
        text = line.split("\t", 1)[-1]
        for m in pat.finditer(text):
            k = m.group(0)
            cls = KATA if re.search(KATA, k) else KANJI
            a, b = m.start(), m.end()
            while a > 0 and re.match(cls, text[a - 1]):
                a -= 1
            while b < len(text) and re.match(cls, text[b]):
                b += 1
            run = text[a:b]
            total[k] += 1
            if run == k:
                exact[k] += 1
            else:
                runs[k][run] += 1
with open(out, "w", encoding="utf-8") as f:
    f.write(f"# 読んだ行 {n}。鍵<TAB>出た数<TAB>鍵そのままの数<TAB>続きの上位\n")
    for k in sorted(total, key=lambda k: -total[k]):
        top = "|".join(f"{r}:{c}" for r, c in runs[k].most_common(30))
        f.write(f"{k}\t{total[k]}\t{exact[k]}\t{top}\n")
print(n, "lines", len(total), "keys found")
