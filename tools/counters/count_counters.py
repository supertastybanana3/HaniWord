"""2026-11-02: 助数詞として実際に使われる回数を数える (変換サーバーの counters.swift が、一覧に足す助数詞を選び、並べるのに使う)。
  python3 -I count_counters.py <助数詞の一覧> <out.tsv> <文の tsv>...
助数詞の一覧: 1 行 1 語 (表記)。# で始まる行は説明。タブのある行は 3 欄目の「二＋表記」から 二 を外して表記にする (counters_list.tsv の形)
文の tsv: 記事 ID<TAB>文 (日本語版ウィキペディアの文に分けたもの)
数える所: 数字 (0-9・０-９) か漢数字のすぐ後ろにその表記が来て、その後ろに漢字が続かない所 (二挺・10輌編成 の 輌 は数える。輌 の後ろの 編 は漢字でないので可)。
  長い表記を先に見る (二十人 は 人、二人前 は 人前 が一覧にあれば 人前)
out.tsv: 表記<TAB>回数 (一覧の順)"""
import re, sys
lst, out, *paths = sys.argv[1:]
words = []
for line in open(lst, encoding="utf-8"):
    line = line.rstrip("\n")
    if not line or line.startswith("#"):
        continue
    w = line.split("\t")[2][1:] if "\t" in line else line
    if w and w not in words:
        words.append(w)
NUM = "0-9０-９〇一二三四五六七八九十百千万億兆"
KANJI = "々〆ヵヶ㐀-䶿一-鿿豈-﫿"
pattern = re.compile(f"(?<=[{NUM}])(?:{'|'.join(re.escape(w) for w in sorted(words, key=len, reverse=True))})(?![{KANJI}])")
counts = dict.fromkeys(words, 0)
for p in paths:
    for line in open(p, encoding="utf-8"):
        for m in pattern.finditer(line.partition("\t")[2]):
            counts[m.group()] += 1
    print(p, flush=True)
with open(out, "w", encoding="utf-8", newline="\n") as f:
    for w in words:
        f.write(f"{w}\t{counts[w]}\n")
