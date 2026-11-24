"""2026-11-06 A (仕事 10c): Wikipedia の数学の記事で、名詞 (漢字かカタカナの並び) のすぐ後ろに英字 1 字 (変数) が来て、その後ろが区切り
(・ / 、 , 。 ) 空白 助詞 = など) の所を数える。名詞ごとに、英字 1 字が続いた回数と、名詞が出た回数の合計。
使い方: python3 count_noun_letter.py <texts.jsonl> <out.tsv>   (出力: 名詞<TAB>英字が続いた回数<TAB>名詞の回数<TAB>例の英字)"""
import json, re, sys, collections
src, out = sys.argv[1], sys.argv[2]
NOUN = r"([一-鿿々]{1,6}|[ァ-ヺー]{2,10})"
BOUND = r"(?=[・/／、，,。．.\s)）=＝<>＜＞]|[のをがとはにでもへや]|$)"
pat = re.compile(NOUN + r"([A-Za-zＡ-Ｚａ-ｚ])" + BOUND)
follow = collections.Counter(); letters = collections.defaultdict(collections.Counter)
for line in open(src, encoding="utf-8"):
    text = json.loads(line).get("text", "")
    for m in pat.finditer(text):
        noun = m.group(1)
        # 漢字の並びは、後ろから 1〜4 字の語として数える (三角形ABC の 三角形・角形・形 …)。読みは後で辞書で引く
        if re.fullmatch(r"[一-鿿々]+", noun):
            for k in range(1, min(4, len(noun)) + 1):
                follow[noun[-k:]] += 1; letters[noun[-k:]][m.group(2)] += 1
        else:
            follow[noun] += 1; letters[noun][m.group(2)] += 1
# 名詞の回数 (分母): 数えた名詞が本文に出た回数
total = collections.Counter()
keys = [k for k, v in follow.items() if v >= 10]
big = re.compile("|".join(sorted(map(re.escape, keys), key=len, reverse=True))) if keys else None
for line in open(src, encoding="utf-8"):
    text = json.loads(line).get("text", "")
    for k in keys:
        pass
    if big:
        for m in big.finditer(text):
            total[m.group(0)] += 1
with open(out, "w", encoding="utf-8") as f:
    for k in sorted(keys, key=lambda k: -follow[k]):
        f.write(f"{k}\t{follow[k]}\t{total[k]}\t{''.join(l for l, _ in letters[k].most_common(5))}\n")
print(len(keys), "nouns")
