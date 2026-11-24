"""2026-11-04 C: 仕事 19 (2) の表。scan_initials.py の続き (英字 1 字＋漢字・カタカナ 12 字まで) を Sudachi (C 単位) で切り、
英字＋最初の語 (B級グルメ → B級、Jリーグ初出場 → Jリーグ) ごとに回数を合わせる。大文字と小文字は同じ語として合わせ、字の形ごとの回数と、
いちばん多い形とその割合を出す (Jリーグ・X線・Tシャツ は大文字、iモード・eスポーツ は小文字。割れている語も残す)。
  nice -n 19 <Sudachi の python> -I initials_words.py <runs.tsv> <出力 initials_words.tsv> [--min 2] [--dict <azookey_dict.tsv>]
出力の列: 多い形の語<TAB>英字 (小文字)<TAB>語の部分<TAB>語の部分の読み (ひらがな、Sudachi で英字ごと切ったときの読み。続きごとに違えば回数の多いもの)<TAB>合計<TAB>大文字の回数<TAB>小文字の回数<TAB>多い形 (大/小)<TAB>多い形の割合
          <TAB>記事の数 (続きごとの 1000 までの数の合計)<TAB>続きの例 (多い順に 3 つ、回数つき)
          <TAB>辞書の読み (--dict の azooKey の辞書でその語に付いている読み、ひらがな、| 区切り。型 は かた|がた。Sudachi の読みは文の中で揺れるので)"""
import collections, sys
args = sys.argv[1:]
mn = 2
if "--min" in args:
    i = args.index("--min"); mn = int(args[i + 1]); del args[i:i + 2]
dict_path = None
if "--dict" in args:
    i = args.index("--dict"); dict_path = args[i + 1]; del args[i:i + 2]
src, out = args
from sudachipy import dictionary, tokenizer
tok = dictionary.Dictionary(dict="core").create(); MODE = tokenizer.Tokenizer.SplitMode.C
hira = lambda s: "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
runs = []
for l in open(src, encoding="utf-8"):
    if l.startswith("#"):
        continue
    c, run, v, a = l.rstrip("\n").split("\t")
    runs.append((c, run, int(v), int(a)))
first = {}
# 英字も付けて切り、英字のあとの語とその読みを取る (A型 の 型 は がた。続きだけを切ると かた になる)。
# Sudachi が英字ごと 1 語にするとき (Tシャツ) は、続きだけを切った最初の語とその読み
for c0, run in {(r[0], r[1]) for r in runs}:
    ms = list(tok.tokenize(c0 + run, MODE))
    if len(ms) >= 2 and ms[0].surface() == c0:
        first[(c0, run)] = (ms[1].surface(), hira(ms[1].reading_form()))
    else:
        ms = list(tok.tokenize(run, MODE))
        first[(c0, run)] = (ms[0].surface(), hira(ms[0].reading_form())) if ms else (run, hira(run))
agg = collections.defaultdict(lambda: {"up": 0, "low": 0, "arts": 0, "ex": collections.Counter(), "readings": collections.Counter()})
for c, run, v, a in runs:
    w, rd = first[(c, run)]
    g = agg[(c.lower(), w)]
    g["up" if c.isupper() else "low"] += v
    g["arts"] += a
    g["ex"][c + run] += v
    g["readings"][rd] += v
dict_readings = collections.defaultdict(set)
if dict_path:
    words = {w for (_, w) in agg}
    for l in open(dict_path, encoding="utf-8"):
        p = l.rstrip("\n").split("\t")
        if len(p) == 6 and p[1] in words:
            dict_readings[p[1]].add(hira(p[0]))
rows = []
for (c, w), g in agg.items():
    total = g["up"] + g["low"]
    if total < mn:
        continue
    major = "大" if g["up"] >= g["low"] else "小"
    form = (c.upper() if major == "大" else c) + w
    share = max(g["up"], g["low"]) / total
    ex = "・".join(f"{k} {n}" for k, n in g["ex"].most_common(3))
    rows.append((total, [form, c, w, g["readings"].most_common(1)[0][0], str(total), str(g["up"]), str(g["low"]), major, f"{share:.3f}", str(g["arts"]), ex, "|".join(sorted(dict_readings.get(w, ())))]))
rows.sort(key=lambda r: -r[0])
with open(out, "w", encoding="utf-8", newline="\n") as f:
    f.write("# 2026-11-04 C 仕事 19: 英字 1 字＋漢字・カタカナの語 (Wikipedia 日本語版の本文、試験の記事 (記事 ID の 1 の位が 0) は除く)。scan_initials.py → initials_words.py\n")
    f.write("# 多い形の語\t英字\t語の部分\t読み (Sudachi)\t合計\t大文字\t小文字\t多い形\t多い形の割合\t記事の数\t続きの例\t辞書の読み\n")
    for _, r in rows:
        f.write("\t".join(r) + "\n")
print("続き", len(runs), "語", len(agg), f"合計 {mn} 回以上の語", len(rows))
