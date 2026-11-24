"""2026-11-07 C: 仕事 43。表の語ごとに、閲覧数 (月の平均)・手元の Wikipedia の全文で含む文の数・azooKey の辞書にあるか、をまとめる。
使い方: python3 -I make_sheet.py <table.tsv> <pageviews.tsv (fetch_pageviews.py の出力)> <wiki_all.tsv> <azookey_dict.tsv> <out.tsv>"""
import re, sys
table, pvr, wiki, dic, out = sys.argv[1:6]
rows = [l.rstrip("\n").split("\t") for l in open(table, encoding="utf-8") if not l.startswith("#")]
# 閲覧数: 記事の題から括弧の注 (「 (ネット用語)」) を除いた語ごとの月の平均 (リダイレクトはリダイレクトの題での閲覧)。多い順の順位も付ける
avg = {}
for l in open(pvr, encoding="utf-8"):
    if l.startswith("#"): continue
    c = l.rstrip("\n").split("\t")
    avg[re.sub(r"\s*\([^)]*\)$", "", c[0])] = int(c[3] or 0)
pv = {w: (i + 1, v) for i, (w, v) in enumerate(sorted(avg.items(), key=lambda x: -x[1]))}
wc = {}
for l in open(wiki, encoding="utf-8"):
    if l.startswith("#"): continue
    w, n = l.rstrip("\n").split("\t"); wc[w] = int(n)
want = {r[0] for r in rows}
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
pairs, surf = {}, {}
for l in open(dic, encoding="utf-8"):
    c = l.rstrip("\n").split("\t")
    if len(c) < 6: continue
    y, w = c[0], c[1]
    if w in want:
        surf.setdefault(w, set()).add(y)
        pairs[(y, w)] = max(pairs.get((y, w), -99.0), float(c[5]))
def kind(w):
    if re.fullmatch(r"[ぁ-ゖー]+", w): return "ひらがな"
    if re.fullmatch(r"[ァ-ヺー]+", w): return "カタカナ"
    if re.fullmatch(r"[ぁ-ゖァ-ヺー]+", w): return "かなの混ぜ"
    return "漢字などを含む"
with open(out, "w", encoding="utf-8") as f:
    f.write("# 語\t読み\t形\t閲覧の順\t月の平均の閲覧\t全文で含む文の数\t辞書 (同じ読み・表記の値|ほかの読み)\tnote\n")
    for r in rows:
        w, y = r[0], r[1]
        rk, v = pv.get(w, (0, 0))
        kv = kata(y)
        same = pairs.get((kv, w))
        d = f"{same:.2f}" if same is not None else ""
        other = sorted(surf.get(w, set()) - {kv})
        if other: d += "|" + ",".join(other[:3])
        f.write("\t".join([w, y, kind(w), str(rk), str(v), str(wc.get(w, 0)), d, r[6] if len(r) > 6 else ""]) + "\n")
