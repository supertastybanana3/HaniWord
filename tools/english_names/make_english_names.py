"""2026-11-04 (仕事 19 の 2): 英語の固有名詞の表 (english_names.txt) を、Wikidata の項目の英語のラベルの候補 (name_candidates.tsv) から作る。
IME 本体は、区切りの判定の語には使わず、すでに英字の語になった所の字の形 (einstein → Einstein) と、Shift で打った英字の守り
(日本の名前の表と同じ所。Einstein + の) だけに使う (auto_english の english_names)。
候補の表は、Wikidata (CC0 1.0) の人の姓・地名・組織などの項目のうち、言語版 (サイトリンク) が 20 以上で英語のラベルがラテン文字 1 語のもの
(打つ形・英語の表記・言語版の数・種類・…・小文字が普通の語・ローマ字で読める・今の大文字の表 の列)。
  python3 make_english_names.py <name_candidates.tsv> <出力> [--sitelinks 80]
選び方: 言語版が N 以上・英字だけ・4 字以上・小文字の形が普通の英単語 (SCOWL) でない・IME の判定でローマ字として読めない・今の表記直しの表に無い・
英語の表記から字の上の点を外すと打つ形になる語。2026-11-04 の測りで、言語版 50 以上 (13,897 語) は区切りに入れたとき日本語を壊し、
Shift の守りでも英字 1 字＋かな の組を少し落としたので、80 以上 (3,499 語) にした"""
import argparse, unicodedata

p = argparse.ArgumentParser()
p.add_argument("table")
p.add_argument("out")
p.add_argument("--sitelinks", type=int, default=80)
a = p.parse_args()
fold = lambda s: "".join(c for c in unicodedata.normalize("NFKD", s) if not unicodedata.combining(c))
names = []
for line in open(a.table, encoding="utf-8"):
    if line.startswith("#"): continue
    r = line.rstrip("\n").split("\t")
    typed, label, sitelinks, common, readable, in_case = r[0], r[1], int(r[2]), r[6], r[7], r[8]
    if sitelinks < a.sitelinks or not (typed.isascii() and typed.isalpha()) or len(typed) < 4 or common or readable or in_case: continue
    if fold(label).lower() != typed: continue
    names.append(fold(label))
with open(a.out, "w", encoding="utf-8", newline="\n") as f:
    f.write("# HaniWord の英語の固有名詞の表 (英字の語の字の形と、Shift で打った英字の守りに使う。区切りの判定には使わない)。1 行 1 語、大文字の位置は英語の表記どおり (字の上の点は外す)\n")
    f.write(f"# 出典: Wikidata (https://www.wikidata.org/、CC0 1.0) の項目の英語のラベル。言語版 (サイトリンク) が {a.sitelinks} 以上の人の姓・地名・組織など (tools/english_names/make_english_names.py)\n")
    f.write("# 選び方: 英字だけ・4 字以上・IME の判定でローマ字として読めない・普通の英単語 (SCOWL) と表記直しの表に無い\n")
    f.write("\n".join(names) + "\n")
print(len(names), "names")
