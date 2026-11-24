"""2026-11-06 (仕事 (4) 組み合わせの数で同音の字を選び直す): 変換の辞書で、同じ読みに漢字を含む語が 2 つ以上ある読み (同音の読み) を書き出す。
  python3 -I homophone_readings.py <辞書の置き場の Dictionary/louds> <out.txt>
読みはカタカナ (辞書の読みのまま)。語は漢字を 1 字以上含むもの (かな書き・英字の語は数えない)。品詞は問わない (同じ読みの 名詞・動詞 も数える)"""
import glob, os, re, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "domain"))
from louds import read_txt  # noqa: E402

louds, out = sys.argv[1:3]
KANJI = re.compile(r"[一-鿿㐀-䶿々]")
words = {}
for path in glob.glob(os.path.join(louds, "*.loudstxt3")):
    for ruby, items in read_txt(path):
        for w, *_ in items:
            if KANJI.search(w):
                words.setdefault(ruby, set()).add(w)
readings = sorted(r for r, ws in words.items() if len(ws) >= 2 and re.fullmatch(r"[ァ-ヺー]+", r))
with open(out, "w", encoding="utf-8", newline="\n") as f:
    for r in readings:
        f.write(r + "\n")
print(len(words), "読み", len(readings), "同音の読み")
