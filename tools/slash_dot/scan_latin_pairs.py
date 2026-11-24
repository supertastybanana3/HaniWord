"""2026-11-05 A (仕事 22 の 1・2、まとめ役 12:28): 英字の語のすぐ後ろの区切り (・ / ／) を、左の語の形ごとと、英字の語どうしの組ごとに数える。
試験の記事 (記事 ID の 1 の位が 0) は外す (--test なら試験の記事だけ)。
  nice -n 19 python3 -I scan_latin_pairs.py [--test] <出力のフォルダ> <Wikipedia の文 (ID<TAB>文) ...>
出力:
  latin_shapes.tsv  左の形<TAB>右の字の種類<TAB>区切り<TAB>回数
    左の形: 小文字1字・小文字の語・大文字1字・大文字の並び (CT・DVD)・頭だけ大文字 (Youtube)・混ざる (NetBSD・iPhone)。左の語の前がカタカナ・中黒なら「(名前の中)」
  latin_pairs.tsv   左の語<TAB>右の語<TAB>区切り<TAB>回数 (英字の語どうし。並び A/B/C は隣り合う組ごと)"""
import collections, os, re, sys

args = sys.argv[1:]
only_test = args[:1] == ["--test"]
if only_test: args = args[1:]
out_dir, *paths = args
os.makedirs(out_dir, exist_ok=True)
SEP = re.compile(r"(?<![A-Za-z0-9])([A-Za-z]+)(?=([・/／]))")
PAIR = re.compile(r"(?<![A-Za-z0-9])(?=([A-Za-z]+)([・/／])([A-Za-z]+)(?![A-Za-z]))")


def kind(c):
    if not c: return "端"
    if "0" <= c <= "9": return "数字"
    if c.isascii() and c.isalpha(): return "英字"
    if "ぁ" <= c <= "ゖ": return "かな"
    if "ァ" <= c <= "ヺ" or c == "ー": return "カタカナ"
    if "一" <= c <= "鿿" or c in "々〆ヶ": return "漢字"
    if c.isspace(): return "空白"
    return "記号"


def shape(w):
    if len(w) == 1: return "大文字1字" if w.isupper() else "小文字1字"
    if w.islower(): return "小文字の語"
    if w.isupper(): return "大文字の並び"
    if w[0].isupper() and w[1:].islower(): return "頭だけ大文字"
    return "混ざる"


shapes = collections.Counter(); pairs = collections.Counter()
lines = skipped = 0
for path in paths:
    for line in open(path, encoding="utf-8", errors="replace"):
        aid, _, s = line.partition("\t")
        lines += 1
        if not aid.isdigit() or (int(aid) % 10 == 0) != only_test:
            skipped += 1
            continue
        if "・" not in s and "/" not in s and "／" not in s: continue
        for m in SEP.finditer(s):
            w, sep = m.group(1), m.group(2)
            before = s[m.start() - 1] if m.start() > 0 else ""
            k = shape(w) + ("(名前の中)" if kind(before) == "カタカナ" or before in "・=＝" else "")
            shapes[(k, kind(s[m.end() + 1:m.end() + 2]), sep)] += 1
        for m in PAIR.finditer(s):
            pairs[(m.group(1), m.group(3), m.group(2))] += 1
    print(path, lines, "外した文", skipped, file=sys.stderr, flush=True)
label = "試験の記事だけの" if only_test else "試験の記事を外した"
for name, header, c in (("latin_shapes.tsv", "左の形<TAB>右の字の種類<TAB>区切り<TAB>回数", shapes),
                        ("latin_pairs.tsv", "左の語<TAB>右の語<TAB>区切り<TAB>回数", pairs)):
    with open(os.path.join(out_dir, name), "w", encoding="utf-8", newline="\n") as f:
        f.write(f"# {header}。{label} Wikipedia 日本語版の本文 (wikimedia/wikipedia 20231101.ja)。文 {lines - skipped}\n")
        for k, v in sorted(c.items(), key=lambda kv: -kv[1]):
            f.write("\t".join(k) + f"\t{v}\n")
print("文", lines, "外した文", skipped)
