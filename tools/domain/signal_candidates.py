"""目印の語 (luna_signals.py) の候補を、分野のウィキペディアの記事 (collect_wikipedia.py の出力) から選ぶ。
記事の冒頭に出る名詞 (Sudachi の長い単位 1 語、漢字を含む 2 字以上) を、出てきた記事の数で数え、--min-docs 以上のものを多い順に書く。
使い方: python3 signal_candidates.py <記事.jsonl>... --out <候補.txt> [--min-docs 5]
"""
import argparse, collections, json, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("pages", nargs="+")
    ap.add_argument("--out", required=True)
    ap.add_argument("--min-docs", type=int, default=5)
    a = ap.parse_args()
    docs = collections.Counter()
    n = 0
    for path in a.pages:
        for line in open(path, encoding="utf-8"):
            try:
                page = json.loads(line)
            except ValueError:
                continue
            n += 1
            docs.update({s for s, _, _ in common.chains(page.get("lead", ""), max_tokens=1)})
    words = [w for w, c in docs.most_common() if c >= a.min_docs]
    with open(a.out, "w", encoding="utf-8") as f:
        f.writelines(w + "\n" for w in words)
    print(f"pages {n}, words {len(docs)}, candidates {len(words)}")


if __name__ == "__main__":
    main()
