"""2026-11-02: 難読語の一覧 (homophone_words.tsv) を作る。変換サーバーの homophone_words.swift が読む。
  python3 -I build_words.py <universe.tsv> <counts.json> <out.tsv> [--learn-max-ratio 1.0] [--list-min-count 30]
universe.tsv: 読み<TAB>語<TAB>勝つ語<TAB>…。azooKey の辞書で読みだけ打つと同じ読みの別の名詞 (勝つ語) に負け、
  JMdict に同じ読みで載る漢字の名詞 (2〜6 字)。勝ち負けは azooKey の辞書の品詞と値で決めたもの
counts.json: {語: 回数}。日本語版ウィキペディア (20231101) の全部の記事で、漢字の並びがその語だけの所を数えたもの
出力の 学習・一覧 の印 (0 か 1):
  学習: 語の回数 <= 勝つ語の回数 × learn-max-ratio。AI (zenz) がもともと文脈で選べる、よく出る語 (機能・以下) は覚えない
  一覧: 語の回数 >= list-min-count。Space の一覧の 2 番目に上げる。ほとんど出ない語 (曖々) は上げない
どちらも 0 の行も残す (変換サーバーが、同じ読みの語の仲間を知るために読む)"""
import json, sys

a = sys.argv[1:]
opts = {"--learn-max-ratio": 1.0, "--list-min-count": 30}
for k in list(opts):
    if k in a:
        i = a.index(k); opts[k] = float(a[i + 1]); del a[i:i + 2]
upath, cpath, out = a
counts = json.load(open(cpath, encoding="utf-8"))
c = lambda w: counts.get(w) or 0
rows, seen = [], set()
for line in open(upath, encoding="utf-8"):
    if line.startswith("#") or not line.strip():
        continue
    r, w, l = line.rstrip("\n").split("\t")[:3]
    if not r or not w or not l or w == l or (r, w, l) in seen:
        continue
    seen.add((r, w, l))
    learn = c(w) <= c(l) * opts["--learn-max-ratio"]
    listed = c(w) >= opts["--list-min-count"]
    rows.append(f"{r}\t{w}\t{l}\t{int(learn)}\t{int(listed)}")
with open(out, "w", encoding="utf-8", newline="\n") as f:
    f.write("# 難読語の一覧 (読み<TAB>語<TAB>勝つ語<TAB>学習<TAB>一覧)。作り方と印の決まりは tools/homophones/build_words.py。"
            "出典は extra_dictionaries/LICENSE.txt\n")
    f.write("\n".join(rows) + "\n")
print(len(rows), "rows", sum(r.endswith(("1\t0", "1\t1")) for r in rows), "learn", sum(r.endswith("\t1") for r in rows), "list")
