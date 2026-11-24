"""2026-11-05 A (仕事 22 の 1): 英字・数字のすぐ後ろの区切り (・ / ／) を、左の並びの形と右の字の種類で分けて数える (英字の語のあとの / を半角にする線の材料)。
試験の記事 (記事 ID の 1 の位が 0) は外す (--test なら試験の記事だけ)。
  nice -n 19 python3 -I scan_slash_sides.py [--test] <出力.tsv> <Wikipedia の文 (ID<TAB>文) ...>
出力: 左の形<TAB>右の形<TAB>区切りの字<TAB>回数
  左の形: 英字 1 字 (大・小)・英字 2 字以上 (大文字だけ・小文字だけ・混ざる)・数字 (1〜4 けた)。左の並びの前がカタカナ・中黒なら「名前の中」を付ける
  右の形: 英字 1 字 (後ろが英字でない)・英字 2 字以上・カタカナ・漢字・かな・数字・記号・空白・端"""
import collections, re, sys

args = sys.argv[1:]
only_test = args[:1] == ["--test"]
if only_test: args = args[1:]
out, *paths = args
SEP = re.compile(r"(?<![A-Za-z0-9])([A-Za-z]+|[0-9]{1,4})([・/／])")


def kind(c):
    if not c: return "端"
    if "0" <= c <= "9": return "数字"
    if c.isascii() and c.isalpha(): return "英字"
    if "ぁ" <= c <= "ゖ": return "かな"
    if "ァ" <= c <= "ヺ" or c == "ー": return "カタカナ"
    if "一" <= c <= "鿿" or c in "々〆ヶ": return "漢字"
    if c.isspace(): return "空白"
    return "記号"


cnt = collections.Counter()
lines = skipped = 0
for path in paths:
    for line in open(path, encoding="utf-8", errors="replace"):
        aid, _, s = line.partition("\t")
        lines += 1
        if not aid.isdigit() or (int(aid) % 10 == 0) != only_test:
            skipped += 1
            continue
        for m in SEP.finditer(s):
            left = m.group(1)
            if left.isdigit():
                lk = "数字"
            elif len(left) == 1:
                lk = "英字1字" + ("大" if left.isupper() else "小")
            else:
                lk = "英字2字以上" + ("大" if left.isupper() else "小" if left.islower() else "混")
            before = s[m.start() - 1] if m.start() > 0 else ""
            if kind(before) == "カタカナ" or before in "・=＝":
                lk += "(名前の中)"
            rest = s[m.end():m.end() + 2]
            rk = kind(rest[:1])
            if rk == "英字":
                rk = "英字1字" if not (len(rest) > 1 and rest[1].isascii() and rest[1].isalpha()) else "英字2字以上"
            cnt[(lk, rk, m.group(2))] += 1
    print(path, lines, "外した文", skipped, file=sys.stderr, flush=True)
with open(out, "w", encoding="utf-8", newline="\n") as f:
    f.write(f"# 左の形<TAB>右の形<TAB>区切りの字<TAB>回数。{'試験の記事だけの' if only_test else '試験の記事を外した'} Wikipedia 日本語版の本文 (wikimedia/wikipedia 20231101.ja)。文 {lines - skipped}\n")
    for k, v in sorted(cnt.items(), key=lambda kv: -kv[1]):
        f.write("\t".join(k) + f"\t{v}\n")
print("文", lines, "外した文", skipped)
