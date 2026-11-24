"""2026-11-06 (仕事 36): 漢数字＋助数詞の書き方を数える (変換サーバーの counter_numerals.swift が、算用数字を漢数字に直すときの形を決めるため)。
  python3 -I count_kanji_numerals.py <counter_counts.tsv> <いくつ以上の助数詞> <out.tsv> <文の tsv>...
助数詞: counter_counts.tsv (表記<TAB>回数) で、回数が指定の数以上のもの。
文の tsv: 記事 ID<TAB>文 (日本語版ウィキペディア・青空文庫の文に分けたもの)。
数える所: 数字・漢数字の並び (前に数字・漢数字が無い所から) のすぐ後ろに助数詞が来て、その後ろに漢字が続かない所 (count_counters.py と同じ)。
並びの形を分ける:
  arabic   算用数字 (0-9・０-９) だけ
  place    位取りの漢数字 (十・百・千・万・億・兆 を含む: 二十三・千九百八十・一万)。1〜9 の 1 字 (一・二) もここ
  digits   桁を並べた漢数字 (2 字以上で 〇一〜九 だけ: 一九八〇・二〇二四・二三)
  zero     〇 か 零 の 1 字
  other    交ぜた形 (3万・二十3)
桁 (数の大きさ) は、arabic・place・digits で数えた値の桁数 (digits の 二三 は 2 桁と数える。二、三人 の意味のものも入る)。
out.tsv: 助数詞<TAB>形<TAB>桁<TAB>回数"""
import collections, re, sys

table, minimum, out, *paths = sys.argv[1:]
minimum = int(minimum)
counters = []
for line in open(table, encoding="utf-8"):
    if line.startswith("#") or "\t" not in line:
        continue
    word, count = line.rstrip("\n").split("\t")[:2]
    if int(count) >= minimum:
        counters.append(word)
KANJI_DIGIT = "〇一二三四五六七八九"
PLACE = "十百千万億兆"
NUM = "0-9０-９" + KANJI_DIGIT + PLACE + "零"
KANJI = "々〆ヵヶ㐀-䶿一-鿿豈-﫿"
pattern = re.compile(f"(?<![{NUM}])([{NUM}]+)({'|'.join(re.escape(w) for w in sorted(counters, key=len, reverse=True))})(?![{KANJI}])")
SMALL = {"十": 10, "百": 100, "千": 1000}
LARGE = {"万": 10 ** 4, "億": 10 ** 8, "兆": 10 ** 12}


def place_value(s):
    """位取りの漢数字の値 (読めなければ None)"""
    total, group, digit = 0, 0, None
    for c in s:
        if c in KANJI_DIGIT:
            if digit is not None:
                return None
            digit = KANJI_DIGIT.index(c)
        elif c in SMALL:
            group += (1 if digit is None else digit) * SMALL[c]
            digit = None
        elif c in LARGE:
            group += digit or 0
            if group == 0:
                return None
            total += group * LARGE[c]
            group, digit = 0, None
        else:
            return None
    return total + group + (digit or 0)


def shape(s):
    if re.fullmatch(r"[0-9０-９]+", s):
        return "arabic", len(s)
    if s in ("〇", "零"):
        return "zero", 1
    if re.fullmatch(f"[{KANJI_DIGIT}]+", s):
        return ("place", 1) if len(s) == 1 else ("digits", len(s))
    if re.fullmatch(f"[{KANJI_DIGIT}{PLACE}]+", s):
        v = place_value(s)
        return ("place", len(str(v))) if v else ("other", 0)
    return "other", 0


counts = collections.Counter()
for p in paths:
    for line in open(p, encoding="utf-8"):
        for m in pattern.finditer(line.partition("\t")[2]):
            form, size = shape(m.group(1))
            counts[(m.group(2), form, min(size, 9))] += 1
    print(p, flush=True)
with open(out, "w", encoding="utf-8", newline="\n") as f:
    for (counter, form, size), n in sorted(counts.items(), key=lambda kv: (counters.index(kv[0][0]), kv[0][1], kv[0][2])):
        f.write(f"{counter}\t{form}\t{size}\t{n}\n")
