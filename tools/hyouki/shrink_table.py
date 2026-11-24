"""2026-11-04: 表記ゆれの組の表 (out/hyouki_groups.tsv、16.6MB) を配る大きさ (2〜3MB) に絞る。配る表 (リポジトリの頭の hyouki_groups.tsv) は --rank 50000 --min-count 100 (19,569 組・3.5MB)。
- 行: 組の形のどれかが wordfreq 3.1.1 の日本語の頻度の順位 --rank 以内で、Wikipedia と青空文庫の回数の合計が --min-count 以上の組だけ
- 列: 位置は変えない (変換サーバーの hyouki.swift は列の番号で読む)。読まない列 (5 ゆれ・9〜13・15) は空にする。
  印 (6) は「かなは別の語と共有 (…)」と「使い分け: …」だけ残す (hyouki.swift が見るのはこの 2 つと種類 (4))
  python3 -I shrink_table.py <hyouki_groups.tsv> <ja_freq.tsv (tools/kanji_info/wordfreq_ja.py の出力)> <出力> [--rank 20000] [--min-count 0]"""
import sys
args = sys.argv[1:]
opt = {"--rank": 20000, "--min-count": 0}
for k in list(opt):
    if k in args:
        i = args.index(k); opt[k] = int(args[i + 1]); del args[i:i + 2]
src, freq, out = args
def split_marks(m):
    """印を「；」で分ける (括弧の中の「；」では分けない。かなは別の語と共有 (ほかの番号: …；同じ番号: …))"""
    parts, depth, cur = [], 0, ""
    for c in m:
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
        if c == "；" and depth == 0:
            parts.append(cur); cur = ""
        else:
            cur += c
    return parts + [cur] if cur else parts

rank = {}
for n, l in enumerate(open(freq, encoding="utf-8"), 1):
    rank.setdefault(l.split("\t")[0], n)
kept = total = 0
with open(out, "w", encoding="utf-8", newline="\n") as f:
    f.write(f"# 表記ゆれの組 (同じ読み・同じ品詞の番号で、意味の同じ書き分け)。組の形のどれかが wordfreq の日本語の頻度の順位 {opt['--rank']} 以内"
            + (f"・回数の合計 {opt['--min-count']} 以上" if opt["--min-count"] else "") + " の組だけ。出典は extra_dictionaries/LICENSE.txt\n")
    f.write("# 読み<TAB>左<TAB>右<TAB>形 (| 区切り)<TAB>種類<TAB>(空)<TAB>印<TAB>Wikipedia の回数 (形の順)<TAB>青空文庫の回数 (形の順)<TAB>(空 5 つ)<TAB>JMdict の項目<TAB>(空)\n")
    for l in open(src, encoding="utf-8"):
        if l.startswith("#"): continue
        p = l.rstrip("\n").split("\t"); total += 1
        forms = p[3].split("|")
        if not any(rank.get(w, 10 ** 9) <= opt["--rank"] for w in forms): continue
        if sum(int(x or 0) for x in p[7].split("|") + p[8].split("|")) < opt["--min-count"]: continue
        for i in (5, 9, 10, 11, 12, 13, 15):
            if i < len(p): p[i] = ""
        p[6] = "；".join(m for m in split_marks(p[6]) if m.startswith("かなは別の語と共有") or m.startswith("使い分け"))
        f.write("\t".join(p) + "\n"); kept += 1
print("組", total, "→", kept)
