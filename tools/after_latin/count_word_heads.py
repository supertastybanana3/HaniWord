"""日本語の文の語の頭 (形態素の頭から 2 音・3 音) の回数を、日本語の Wikipedia の本文で数える (2026-11-04、仕事 11)。
英単語のすぐ後ろのローマ字が、日本語の語の頭としてよく出る並び (来て・見て・して・だれ) かどうかを見るのに使う。
使い方: python count_word_heads.py <out.json> <parquet>... [limit=記事の数]
  要るもの: pyarrow・fugashi (unidic-lite)。読みは UniDic の仮名形 (は = ハ) をひらがなにしたもの。行ごとに区切って解析する
  出力: {"articles", "heads", "first2": {2 音: 回数}, "first3": {3 音: 回数}}"""
import collections, json, sys
import pyarrow.parquet as pq
import fugashi

out_path = sys.argv[1]
paths = [a for a in sys.argv[2:] if not a.startswith("limit=")]
limit = next((int(a[6:]) for a in sys.argv[2:] if a.startswith("limit=")), None)
SMALL = set("ぁぃぅぇぉゃゅょゎ")
tagger = fugashi.Tagger()


def hira(s):
    return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)


def morae(kana):
    out = []
    for c in kana:
        if c in SMALL and out:
            out[-1] += c
        else:
            out.append(c)
    return out


first2, first3 = collections.Counter(), collections.Counter()
articles = heads = 0
done = False
for path in paths:
    for batch in pq.ParquetFile(path).iter_batches(batch_size=200, columns=["text"]):
        for text in batch.column(0).to_pylist():
            articles += 1
            for line in text.split("\n"):
                if not line.strip():
                    continue
                kanas = [hira(w.feature.kana) if w.feature.kana not in (None, "*") else None for w in tagger(line)]
                for i, k in enumerate(kanas):
                    if not k:
                        continue
                    joined = ""
                    for x in kanas[i:i + 3]:
                        if not x:
                            break
                        joined += x
                        if len(morae(joined)) >= 3:
                            break
                    mo = morae(joined)
                    if len(mo) >= 2:
                        heads += 1
                        first2[mo[0] + mo[1]] += 1
                    if len(mo) >= 3:
                        first3[mo[0] + mo[1] + mo[2]] += 1
            if limit and articles >= limit:
                done = True
                break
        if done:
            break
    if done:
        break
json.dump({"articles": articles, "heads": heads, "first2": dict(first2.most_common()), "first3": dict(first3.most_common())},
          open(out_path, "w", encoding="utf-8"), ensure_ascii=False)
print(f"articles {articles} heads {heads} keys2 {len(first2)} keys3 {len(first3)}")
