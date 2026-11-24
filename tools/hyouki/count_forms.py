"""2026-11-02: 表記ゆれの組の形ごとの回数を、Wikipedia・青空文庫の文で数える (段 2。重いので夜に流す)。
Sudachi (C 単位) で文を区切り、続く 1〜4 個の単位をつないだ表記と読みが targets.tsv の (読み, 形) に当たったら数える
(語の途中 (ことば の こと) や、読みの違う使い方 (行った いった と おこなった) は数えない)。
1 つ目の単位の正規化表記ごとに分けて数える (かなの いる が 居る か 要る かを、文の中の解析で分けるため)。
  python -I count_forms.py <targets.tsv> <出力.tsv> <コーパス名> <コーパス (ID<TAB>文) ...> [--every N] [--max-lines N] [--jobs N]
  --every N: N 行に 1 行だけ使う (Wikipedia を間引く)。--max-lines: ファイルごとの行の上限 (試し用)
出力: 読み<TAB>形<TAB>正規化表記<TAB>コーパス名<TAB>回数<TAB>出た文書の数"""
import collections, multiprocessing, os, sys

args = sys.argv[1:]
opt = {}
for k in ("--every", "--max-lines", "--jobs"):
    if k in args:
        i = args.index(k); opt[k] = int(args[i + 1]); del args[i:i + 2]
targets_path, out_path, corpus_name, *corpora = args
EVERY, MAXL, JOBS = opt.get("--every", 1), opt.get("--max-lines", 0), opt.get("--jobs", 4)
hira = lambda s: "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)

targets = set()
first_chars = set()
for l in open(targets_path, encoding="utf-8"):
    r, w = l.rstrip("\n").split("\t")
    targets.add((r, w)); first_chars.add(w[0])


def chunks():
    for path in corpora:
        n = 0
        buf = []
        with open(path, encoding="utf-8", errors="replace") as f:
            for i, l in enumerate(f):
                if i % EVERY:
                    continue
                n += 1
                if MAXL and n > MAXL:
                    break
                buf.append(l)
                if len(buf) >= 5000:
                    yield buf; buf = []
        if buf:
            yield buf


tok = None
def work(lines):
    global tok
    if tok is None:
        from sudachipy import dictionary, tokenizer
        tok = (dictionary.Dictionary(dict="core").create(), tokenizer.Tokenizer.SplitMode.C)
    t, mode = tok
    cnt = collections.Counter(); docs = collections.defaultdict(set)
    chars = 0
    for l in lines:
        p = l.rstrip("\n").split("\t", 1)
        if len(p) != 2 or not p[1]:
            continue
        doc, s = p
        if len(s) > 2000:
            s = s[:2000]
        chars += len(s)
        try:
            ms = t.tokenize(s, mode)
        except Exception:
            continue
        toks = [(m.surface(), m.reading_form(), m.normalized_form()) for m in ms]
        for i, (surf, _, norm) in enumerate(toks):
            if not surf or surf[0] not in first_chars:
                continue
            w = ""; r = ""
            for j in range(i, min(i + 4, len(toks))):
                w += toks[j][0]; r += hira(toks[j][1])
                if (r, w) in targets:
                    k = (r, w, norm); cnt[k] += 1; docs[k].add(doc)
    return cnt, {k: len(v) for k, v in docs.items()}, chars


if __name__ == "__main__":
    total = collections.Counter(); ndoc = collections.Counter(); chars = 0
    with multiprocessing.Pool(JOBS) as pool:
        for i, (c, d, n) in enumerate(pool.imap_unordered(work, chunks(), chunksize=1)):
            total.update(c); ndoc.update(d); chars += n
            if i % 200 == 0:
                print(f"{corpus_name} 塊 {i} 字 {chars:,}", flush=True)
    with open(out_path, "w", encoding="utf-8") as f:
        for (r, w, norm), v in sorted(total.items()):
            f.write(f"{r}\t{w}\t{norm}\t{corpus_name}\t{v}\t{ndoc[(r, w, norm)]}\n")
    print(f"{corpus_name} 終わり 字 {chars:,} 組 {len(total)}", flush=True)
