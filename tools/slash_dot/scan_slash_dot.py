"""2026-11-05 A (仕事 22・23): かなの入力中の / と . の打ち分け (「・」か「/」か、「。」か「.」か) と、全角の数字のあとの記号の材料を、Wikipedia の本文で数える。
試験の記事 (記事 ID の 1 の位が 0。scan_initials.py と同じ) は外す。
  nice -n 19 python3 -I scan_slash_dot.py [--test] <出力のフォルダ> <Wikipedia の文 (ID<TAB>文) ...>
  --test: 試験の記事だけを数える (表の線を確かめる用)
出力 (どれも 回数の多い順。文そのものは書かない):
  digit_pair_follow.tsv   数字＋(・か / か ／)＋数字 のすぐ後ろの語 (漢字・カタカナの並び 4 字まで、ひらがな 1 字、記号の種類) ごとの、区切りの字ごとの回数
  digit_pair_before.tsv   同じ組の、すぐ前の字 (第・約・年 …) ごとの回数 (参考)
  sides.tsv               区切りの字の左右の字の種類 (数字・英字・かな・漢字・カタカナ・記号) の組ごとの、・ / ／ の回数 (仕事 22 の 1 と 3 の材料)
  period.tsv              。 と . の左右の字の種類の組ごとの回数、と 。か . のあとが小文字の拡張子の語になる所の、前の字の種類ごとの回数
  fullwidth.tsv           全角の数字のあとの ／ / ． . ， , の回数と、半角の数字のあとの同じもの (仕事 23 の材料)"""
import collections, os, re, sys

args = sys.argv[1:]
only_test = args[:1] == ["--test"]
if only_test: args = args[1:]
out_dir, *paths = args
os.makedirs(out_dir, exist_ok=True)


def kind(c):
    if not c: return "端"
    if "0" <= c <= "9": return "数字"
    if "０" <= c <= "９": return "全角数字"
    if c.isascii() and c.isalpha(): return "英字"
    if "Ａ" <= c <= "Ｚ" or "ａ" <= c <= "ｚ": return "全角英字"
    if "ぁ" <= c <= "ゖ": return "かな"
    if "ァ" <= c <= "ヺ" or c == "ー": return "カタカナ"
    if "一" <= c <= "鿿" or c in "々〆ヶ": return "漢字"
    if c.isspace(): return "空白"
    return "記号"


DIGIT = "0-9０-９"
# 数字の並び＋区切り＋数字の並び。前後に数字・小数点が続かない所 (1/2/3 の頭の組は数えるが、3 つ目の組は前が数字なので数えない)
PAIR = re.compile(rf"(?<![{DIGIT}.．,，/／・])([{DIGIT}]{{1,4}})([・/／])([{DIGIT}]{{1,4}})(?![{DIGIT}.．,，])")
RUN = re.compile(r"[一-鿿々〆ヶァ-ヺー]{1,4}")
EXT = ["pdf", "docx", "doc", "xlsx", "xls", "pptx", "ppt", "png", "jpg", "jpeg", "gif", "txt", "md", "csv", "zip", "exe", "mp3", "mp4",
       "html", "htm", "json", "py", "rs", "js", "ts", "cpp", "wav", "mov", "svg", "xml", "tex", "dll", "bat", "ps1", "jar", "apk", "iso", "bmp", "tiff", "heic", "webp"]
EXT_RE = re.compile(r"([。.．])(" + "|".join(sorted(EXT, key=len, reverse=True)) + r")(?![A-Za-z0-9])")
SEP = re.compile(r"[・/／]")
DOT = re.compile(r"[。.．]")
FULL = re.compile(r"([０-９0-9])([／/．.，,])([０-９0-9]?)")

follow = collections.Counter(); before = collections.Counter(); sides = collections.Counter(); period = collections.Counter()
ext_before = collections.Counter(); full = collections.Counter()
lines = skipped = 0
for path in paths:
    for line in open(path, encoding="utf-8", errors="replace"):
        aid, _, s = line.partition("\t")
        lines += 1
        if not aid.isdigit() or (int(aid) % 10 == 0) != only_test:
            skipped += 1
            continue
        s = s.rstrip("\n")
        for m in PAIR.finditer(s):
            sep = m.group(2)
            rest = s[m.end():]
            r = RUN.match(rest)
            if r: key = r.group(0)
            elif rest and "ぁ" <= rest[0] <= "ゖ": key = "かな:" + rest[0]
            else: key = "種類:" + kind(rest[:1])
            follow[(key, sep)] += 1
            before[(kind(s[m.start() - 1:m.start()]) if m.start() == 0 or kind(s[m.start() - 1]) != "漢字" else s[m.start() - 1], sep)] += 1
        for m in SEP.finditer(s):
            i = m.start()
            sides[(kind(s[i - 1] if i else ""), kind(s[i + 1] if i + 1 < len(s) else ""), m.group(0))] += 1
        for m in DOT.finditer(s):
            i = m.start()
            period[(kind(s[i - 1] if i else ""), kind(s[i + 1] if i + 1 < len(s) else ""), m.group(0))] += 1
        for m in EXT_RE.finditer(s):
            i = m.start()
            ext_before[(kind(s[i - 1] if i else ""), m.group(1), m.group(2))] += 1
        for m in FULL.finditer(s):
            full[(kind(m.group(1)), m.group(2), kind(m.group(3)))] += 1
    print(path, lines, "試験の記事で外した文", skipped, file=sys.stderr, flush=True)


def write(name, header, counter):
    with open(os.path.join(out_dir, name), "w", encoding="utf-8", newline="\n") as f:
        f.write(f"# {header}。{'試験の記事 (記事 ID の 1 の位が 0) だけの' if only_test else '試験の記事 (記事 ID の 1 の位が 0) を外した'} Wikipedia 日本語版の本文 (wikimedia/wikipedia 20231101.ja)。文 {lines - skipped}\n")
        for k, v in sorted(counter.items(), key=lambda kv: -kv[1]):
            f.write("\t".join(k) + f"\t{v}\n")


write("digit_pair_follow.tsv", "数字＋区切り＋数字のすぐ後ろ<TAB>区切りの字<TAB>回数", follow)
write("digit_pair_before.tsv", "数字＋区切り＋数字のすぐ前 (漢字はその字、ほかは字の種類)<TAB>区切りの字<TAB>回数", before)
write("sides.tsv", "左の字の種類<TAB>右の字の種類<TAB>区切りの字 (・ / ／)<TAB>回数", sides)
write("period.tsv", "左の字の種類<TAB>右の字の種類<TAB>字 (。 . ．)<TAB>回数", period)
write("ext_before.tsv", "前の字の種類<TAB>字 (。 . ．)<TAB>小文字の拡張子の語<TAB>回数", ext_before)
write("fullwidth.tsv", "数字の種類<TAB>記号<TAB>後ろの字の種類<TAB>回数", full)
print("文", lines, "外した文", skipped)
