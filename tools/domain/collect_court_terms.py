"""collect_courts.py で取った判決 (詳細ページの事件名・判示事項・裁判要旨と、全文の PDF の文字) から、語の候補を数える。
出典: 裁判所ウェブサイト 裁判例検索 (https://www.courts.go.jp/hanrei/)。判決は著作権の対象にならない (著作権法 13 条 3 号)。
使うのは語 (名詞の連なり) と、その語が出た判決の数だけで、文は配布物に入れない。
出力 (TSV): 表記 / 判決の数 / 回数 / 出どころ (detail = 事件名・判示事項・裁判要旨、text = 全文) / 例 (事件名)
使い方: python collect_court_terms.py <courts のフォルダ> <出力.tsv> [--jobs 8]
"""
import argparse, collections, gzip, json, multiprocessing, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common


def one_case(args):
    folder, rec = args
    d = rec.get("detail", {})
    found = collections.Counter()
    head = "\n".join(d.get(k, "") for k in ("事件名", "判示事項", "裁判要旨"))
    for surface, _r, n in common.chains(head):
        found[(surface, "detail")] += 1
    path = os.path.join(folder, "text", f"{rec['id']}.txt.gz")
    if rec.get("pdf") == "text" and os.path.exists(path):
        text = gzip.open(path, "rt", encoding="utf-8").read()
        # PDF の行の折り返しで語が切れないよう、改行をつなぐ (段落の区切りは。で切れる)
        text = text.replace("\r", "").replace("\n", "")
        for surface, _r, n in common.chains(text):
            found[(surface, "text")] += 1
    return d.get("事件名", ""), found


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("folder")
    ap.add_argument("out")
    ap.add_argument("--jobs", type=int, default=8)
    a = ap.parse_args()
    recs = [json.loads(line) for line in open(os.path.join(a.folder, "cases.jsonl"), encoding="utf-8")]
    recs = [r for r in recs if r.get("pdf") != "none"]
    cases, count, srcs, example = collections.Counter(), collections.Counter(), collections.defaultdict(set), {}
    with multiprocessing.Pool(a.jobs) as pool:
        for name, found in pool.imap_unordered(one_case, [(a.folder, r) for r in recs], chunksize=4):
            seen = set()
            for (surface, src), n in found.items():
                count[surface] += n
                srcs[surface].add(src)
                if surface not in seen:
                    seen.add(surface)
                    cases[surface] += 1
                    example.setdefault(surface, name)
    with open(a.out, "w", encoding="utf-8") as out:
        out.write(f"# 判決 {len(recs)} 件から。表記\t判決の数\t回数\t出どころ\t例の事件名\n")
        for surface, n in sorted(cases.items(), key=lambda kv: (-kv[1], -count[kv[0]], kv[0])):
            out.write(f"{surface}\t{n}\t{count[surface]}\t{','.join(sorted(srcs[surface]))}\t{example[surface]}\n")
    print("cases", len(recs), "terms", len(cases), file=sys.stderr)


if __name__ == "__main__":
    main()
