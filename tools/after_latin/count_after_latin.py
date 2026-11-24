"""英字の語のすぐ後ろ (空白なし) に来る日本語の読みの頭を、日本語の Wikipedia の本文で数える (2026-11-03、仕事 11)。
IME 本体の区切りで、英単語のすぐ後ろに来にくい かな (powers + たげ) で始まる割り方を下げるのに使う。
使い方: python count_after_latin.py <out.json> <parquet>... [limit=記事の数]
  要るもの: pyarrow・fugashi (unidic-lite)。読みは UniDic の仮名形 (は = ハ、を = ヲ。打つ かな と同じ) をひらがなにしたもの
  出力: {"articles", "occurrences", "first1": {かな 1 音: 回数}, "first2": {2 音: 回数}, "first3": {3 音: 回数},
         "lower2": {小文字だけの英字の語のあとの 2 音: 回数}, "words": {英字の語の数}}"""
import collections, json, re, sys
import pyarrow.parquet as pq
import fugashi

out_path = sys.argv[1]
paths = [a for a in sys.argv[2:] if not a.startswith("limit=")]
limit = next((int(a[6:]) for a in sys.argv[2:] if a.startswith("limit=")), None)
JA = "ぁ-ゖァ-ヺー一-鿿々"
PAT = re.compile(rf"(?<![A-Za-z0-9_\-'.])([A-Za-z]{{2,}})([{JA}][{JA}、。]{{0,11}})")
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


first1, first2, first3, lower2 = collections.Counter(), collections.Counter(), collections.Counter(), collections.Counter()
words = collections.Counter()
articles = occurrences = 0
for path in paths:
    for batch in pq.ParquetFile(path).iter_batches(batch_size=500, columns=["text"]):
        for text in batch.column(0).to_pylist():
            articles += 1
            for m in PAT.finditer(text):
                word, tail = m.group(1), m.group(2)
                kana, at = "", 0
                ok = True
                for w in tagger(word + tail):
                    start, at = at, at + len(w.surface)
                    if at <= len(word):
                        continue
                    if start < len(word):
                        ok = False  # 英字の語と日本語が 1 つの語になった
                        break
                    if w.feature.kana is None or w.feature.kana == "*":
                        break
                    kana += hira(w.feature.kana)
                    if len(morae(kana)) >= 3:
                        break
                if not ok or not kana:
                    continue
                mo = morae(kana)
                occurrences += 1
                words[word.lower()] += 1
                first1[mo[0]] += 1
                if len(mo) >= 2:
                    first2[mo[0] + mo[1]] += 1
                    if word.islower():
                        lower2[mo[0] + mo[1]] += 1
                if len(mo) >= 3:
                    first3[mo[0] + mo[1] + mo[2]] += 1
            if limit and articles >= limit:
                break
        if limit and articles >= limit:
            break
    if limit and articles >= limit:
        break
json.dump({"articles": articles, "occurrences": occurrences, "first1": dict(first1.most_common()), "first2": dict(first2.most_common()), "first3": dict(first3.most_common()),
           "lower2": dict(lower2.most_common()), "words": dict(words.most_common(20000))}, open(out_path, "w", encoding="utf-8"), ensure_ascii=False)
print(f"articles {articles} occurrences {occurrences} keys2 {len(first2)}")
