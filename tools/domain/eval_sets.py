"""分野辞書の効果を測る: 測りの文の組 (リポジトリの外の .tsv) を convert.py の入力に直す・2 つの結果を比べる。

  cases   <文の組.tsv> <出力.tsv> : 列 (ローマ字 / 記事 / 文 / 読み (ひらがな) / ...) → 読み<TAB>正解<TAB>文脈。
                                     文脈は同じ記事の直前の文 (写経では同じ記事の文を消さずに続けて打つので)
  compare <前.jsonl> <後.jsonl>   : 一致した文の数・文字の誤り率 (編集距離 / 正解の字数)・良くなった文・悪くなった文
"""
import json, sys


def cmd_cases(src, out):
    prev_article, prev = None, ""
    n = 0
    with open(out, "w", encoding="utf-8") as f:
        for line in open(src, encoding="utf-8"):
            c = line.rstrip("\n").split("\t")
            if len(c) < 4 or line.startswith("#"):
                continue
            article, sentence, reading = c[1], c[2], c[3]
            context = prev if article == prev_article else ""
            f.write(f"{reading}\t{sentence}\t{context}\n")
            prev_article, prev = article, sentence
            n += 1
    print("cases", n)


def edit_distance(a, b):
    prev = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        cur = [i]
        for j, y in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (x != y)))
        prev = cur
    return prev[-1]


def load(path):
    return [json.loads(line) for line in open(path, encoding="utf-8")]


def summary(rows, key="full"):
    ok = sum(r[key] == r["expected"] for r in rows)
    err = sum(edit_distance(r[key], r["expected"]) for r in rows)
    chars = sum(len(r["expected"]) for r in rows)
    return ok, len(rows), err / max(chars, 1)


def cmd_compare(a, b, key="full"):
    ra, rb = load(a), load(b)
    if [r["reading"] for r in ra] != [r["reading"] for r in rb]:
        sys.exit("the two results are not for the same cases")
    for name, rows in (("before", ra), ("after", rb)):
        ok, n, cer = summary(rows, key)
        print(f"{name}: {ok}/{n} ({ok / n:.1%}) char error {cer:.2%}")
    better = [(x, y) for x, y in zip(ra, rb) if x[key] != y[key] and y[key] == y["expected"]]
    worse = [(x, y) for x, y in zip(ra, rb) if x[key] != y[key] and x[key] == x["expected"]]
    changed = [(x, y) for x, y in zip(ra, rb) if x[key] != y[key] and (x, y) not in better and (x, y) not in worse]
    dist = lambda r: edit_distance(r[key], r["expected"])
    closer = sum(dist(y) < dist(x) for x, y in changed)
    print(f"better {len(better)} / worse {len(worse)} / other changes {len(changed)} (closer {closer}, farther {sum(dist(y) > dist(x) for x, y in changed)})")
    for label, items in (("WORSE", worse), ("BETTER", better), ("CHANGED", changed)):
        for x, y in items:
            print(f"{label}\t{x['expected']}\n\tbefore: {x[key]}\n\tafter : {y[key]}")


if __name__ == "__main__":
    if sys.argv[1] == "cases":
        cmd_cases(sys.argv[2], sys.argv[3])
    elif sys.argv[1] == "compare":
        cmd_compare(sys.argv[2], sys.argv[3], *(sys.argv[4:5]))
