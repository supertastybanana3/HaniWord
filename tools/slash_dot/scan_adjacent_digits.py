"""2026-11-05 A (仕事 22 の 2、まとめ役 12:37): 1 けたの数どうしの区切り (・ / ／) を、隣り合う数 (n と n+1: 4・5千人・7・8割) かどうかと、すぐ後ろの種類で数える。
試験の記事 (記事 ID の 1 の位が 0) は外す (--test なら試験の記事だけ)。種類は follow_kinds.py と同じ (語の表・数の位・助数詞・漢字 1 字・そのほか) に、かな・記号・端
  nice -n 19 python3 -I scan_adjacent_digits.py [--test] <出力.tsv> <語の表 follow_*.txt> <counter_counts.tsv> <Wikipedia の文 ...>
出力: 隣り合う (はい・いいえ)<TAB>後ろの種類<TAB>区切り<TAB>回数"""
import collections, re, sys

args = sys.argv[1:]
only_test = args[:1] == ["--test"]
if only_test: args = args[1:]
out, table_path, counters_path, *paths = args
words = [l.split("\t")[0] for l in open(table_path, encoding="utf-8") if not l.startswith(("#", "-"))]
counters = sorted({l.split("\t")[0] for l in open(counters_path, encoding="utf-8") if not l.startswith("#") and l.strip()}, key=len, reverse=True)
PAT = re.compile(r"(?<![0-9０-９.．,，/／・])([0-9])([・/／])([0-9])(?![0-9０-９.．,，/／・])")
RUN = re.compile(r"[一-鿿々〆ヶァ-ヺー]{1,4}")


def kind(rest):
    r = RUN.match(rest)
    if not r:
        c = rest[:1]
        return "端" if not c else "かな" if "ぁ" <= c <= "ゖ" else "記号ほか"
    key = r.group(0)
    if any(key.startswith(w) for w in words): return "語の表"
    if key[0] in "百千万億兆": return "数の位"
    if any(key.startswith(c) for c in counters): return "助数詞"
    if len(key) == 1 and "一" <= key <= "鿿": return "漢字1字"
    return "そのほか"


cnt = collections.Counter()
lines = skipped = 0
for path in paths:
    for line in open(path, encoding="utf-8", errors="replace"):
        aid, _, s = line.partition("\t")
        lines += 1
        if not aid.isdigit() or (int(aid) % 10 == 0) != only_test:
            skipped += 1
            continue
        if "・" not in s and "/" not in s and "／" not in s: continue
        for m in PAT.finditer(s):
            adjacent = "はい" if int(m.group(3)) == int(m.group(1)) + 1 else "いいえ"
            cnt[(adjacent, kind(s[m.end():m.end() + 4]), "・" if m.group(2) == "・" else "/")] += 1
    print(path, lines, "外した文", skipped, file=sys.stderr, flush=True)
with open(out, "w", encoding="utf-8", newline="\n") as f:
    f.write(f"# 隣り合う 1 けたの数か<TAB>後ろの種類<TAB>区切り<TAB>回数。{'試験の記事だけの' if only_test else '試験の記事を外した'} Wikipedia 日本語版の本文 (wikimedia/wikipedia 20231101.ja)。文 {lines - skipped}\n")
    for k, v in sorted(cnt.items(), key=lambda kv: -kv[1]):
        f.write("\t".join(k) + f"\t{v}\n")
print("文", lines, "外した文", skipped)
