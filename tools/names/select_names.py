"""2026-11-05: よくある日本の名前のローマ字の表 (japanese_names_romaji.txt) を、build_names_v2.py の names_romaji_v2.tsv から選ぶ。
Mozc の公開辞書のコストが N 以下 (よく出る名前) か、Wikidata の日本の人物の人数が M 以上の綴り。全部 (135,665 綴り) を入れると、
Mozc の姓にしか無いまれな綴り (akara・igata・irui・atou) が Shift で打った英字 1 字＋かな (A から・I 型) を名前とみて英字のまま残したので絞る
(2026-11-04 の Shift の英字の組の測りで、コスト 6,000・人数 10 は、表に無い名前の割れを全部入れたときと同じだけ減らし、英字 1 字＋かな の減りは半分)。
  python3 select_names.py <names_romaji_v2.tsv> <出力> [--cost 6000] [--people 10]"""
import argparse

p = argparse.ArgumentParser()
p.add_argument("table")
p.add_argument("out")
p.add_argument("--cost", type=int, default=6000)
p.add_argument("--people", type=int, default=10)
a = p.parse_args()
names = []
for line in open(a.table, encoding="utf-8"):
    if line.startswith("#"): continue
    # 綴り 出どころ 読み Wikidata の人数 Mozc のコスト 英単語 日本語の語
    r = line.rstrip("\n").split("\t")
    people = int(r[3]) if r[3] else 0
    cost = int(r[4]) if r[4] else None
    if (cost is not None and cost <= a.cost) or people >= a.people:
        names.append(r[0])
with open(a.out, "w", encoding="utf-8", newline="\n") as f:
    f.write("# HaniWord: よくある日本の名前のローマ字 (1 行 1 つ、小文字)。Shift で打ち始めた英字の名前を割らないために IME 本体が読む。\n")
    f.write("# 出典: Mozc の公開辞書 (dictionary_oss) の人名の読みと、Wikidata (CC0 1.0) の日本の人物の名前のカナ (tools/names/build_names_v2.py・select_names.py)。"
            "許諾は extra_dictionaries/LICENSE.txt\n")
    for name in sorted(set(names)):
        f.write(name + "\n")
print(len(set(names)), "names")
