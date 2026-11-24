"""2026-11-07 C: 仕事 44。名前 (人・会社) ごとに、手元の Wikipedia の文の 1 つの束で、名前を含む文のうち名前の頭と終わりが語の切れ目
(Sudachi の A 単位) にそろう割合を出す (テイン が プロテイン に、KANA が 他の語に当たるような数を除くため)。その名前の記事の文は除く。
1) grep -onF で束の中の一致 (行番号:名前) を取り、名前ごとに 50 行まで抜く (行番号で決まる順で、多い名前は間をあけて)  2) その行だけを読み、Sudachi で切れ目を見る
出力 TSV: 名前<TAB>束の中の一致の文の数 (その記事の文を除く)<TAB>抜いた文<TAB>切れ目にそろった文<TAB>割合
使い方: python -I name_boundary.py <names.tsv (名前<TAB>その記事の番号)> <束.tsv> <out.tsv> [抜く数 (既定 50)]"""
import subprocess, sys, tempfile, os
from collections import defaultdict
from sudachipy import dictionary, tokenizer
names_p, shard, out = sys.argv[1:4]
K = int(sys.argv[4]) if len(sys.argv) > 4 else 50
own = {}
for l in open(names_p, encoding="utf-8"):
    if l.startswith("#") or not l.strip(): continue
    c = l.rstrip("\n").split("\t"); own.setdefault(c[0], set()).add(c[1] if len(c) > 1 else "")
with tempfile.NamedTemporaryFile("w", encoding="utf-8", suffix=".txt", delete=False) as tf:
    tf.write("\n".join(n for n in own if n) + "\n"); pat = tf.name
hits = defaultdict(list)
p = subprocess.Popen(["grep", "-onF", "-f", pat, shard], stdout=subprocess.PIPE, env=dict(os.environ, LC_ALL="C.UTF-8"))
for raw in p.stdout:
    ln, _, name = raw.decode("utf-8", "replace").rstrip("\n").partition(":")
    if name in own: hits[name].append(int(ln))
p.wait(); os.unlink(pat)
want = defaultdict(list)  # 行番号 → 名前
total = {}
for name, lns in hits.items():
    lns = sorted(set(lns)); total[name] = lns
    step = max(1, len(lns) // K)
    for ln in lns[::step][:K]: want[ln].append(name)
tok = dictionary.Dictionary(dict="full").create(); MODE = tokenizer.Tokenizer.SplitMode.A
stat = defaultdict(lambda: [0, 0, 0])  # 一致 (記事の外)、抜いた、そろった
own_lines = defaultdict(int)
with open(shard, encoding="utf-8") as f:
    for i, line in enumerate(f, start=1):
        if i not in want: continue
        pid, _, text = line.rstrip("\n").partition("\t")
        for name in want[i]:
            if pid in own[name]: own_lines[name] += 1; continue
            st = stat[name]; st[1] += 1
            a = text.find(name)
            if a < 0: continue
            bounds = set()
            for t in tok.tokenize(text[:2000], MODE): bounds.add(t.begin()); bounds.add(t.end())
            if a in bounds and a + len(name) in bounds: st[2] += 1
with open(out, "w", encoding="utf-8") as fo:
    fo.write("# 名前\t束の中の一致の文の数\t抜いた文\t切れ目にそろった文\t割合\n")
    for name in own:
        n = len(total.get(name, [])); st = stat[name]
        fo.write(f"{name}\t{n}\t{st[1]}\t{st[2]}\t{(st[2] / st[1]) if st[1] else 0:.2f}\n")
print("names", len(own), "with hits", len(hits))
