"""2026-11-07 C: 仕事 43 (まとめ役 09:39・09:48)。ネットミーム (Category:日本のインターネットスラング)・流行語大賞の受賞語・閲覧数の上位の記事の (B) を、
同じ線の案で並べる。線: 記事のある語は直近 12 か月の月の平均の閲覧数 PV 以上、記事の無い語・転送だけの語は全文の目安の数 ME 以上
(ネットミームは今までどおり どちらかが越えれば)。線からの近さ = max(閲覧/PV, 目安の数/ME)。
使い方: python3 -I proposal_all.py <netslang proposal.tsv> <ryukogo classified.tsv> <toppv b_candidates.tsv> <toppv b_views.tsv> <out.tsv> [PV ME]"""
import re, sys
ns_p, ry_p, bc_p, bv_p, out_p = sys.argv[1:6]
PV, ME = (int(sys.argv[6]), int(sys.argv[7])) if len(sys.argv) > 7 else (300, 20)
def rows(p): return [l.rstrip("\n").split("\t") for l in open(p, encoding="utf-8") if not l.startswith("#") and l.strip()]
items = []
for r in rows(ns_p):
    if r[10] != "(a) の候補": continue
    e = None if re.fullmatch(r"[ぁ-ゖァ-ヺー]{1,3}", r[1]) else int(r[7])
    items.append(("ネットミーム", r[1], r[2], int(r[5]), "", e, "", r[9]))
for r in rows(ry_p):
    if r[8] != "候補" or not r[11].startswith("B"): continue  # (A) 固有名詞 (語の記事のカテゴリ) は仕事 44 の表の側
    pv = int(r[4]) if r[3] == "記事" and r[4] else None
    e = None
    if r[3] != "記事" and r[7]: e = int(r[7].rstrip("?"))
    items.append(("流行語大賞", r[0], r[1], pv, (r[5] + " " + r[6]) if r[5] else "", e, r[9], ""))
views = {r[0]: (int(r[1]), r[2], r[3]) for r in rows(bv_p)}
for r in rows(bc_p):
    if r[9] or r[3] != "B ふつうの語": continue
    v = views.get(r[0])
    items.append(("閲覧数の上位", r[1], r[2], v[0] if v else None, (v[1] + " " + v[2]) if v else "", None, f"上位 {r[5]} か月 (直近 12 か月で {r[6]})", r[4]))
def score(it):
    pv, e = it[3], it[5]
    return max((pv or 0) / PV, (e or 0) / ME)
items.sort(key=lambda it: -score(it))
with open(out_p, "w", encoding="utf-8") as f:
    f.write(f"# 線: 直近 12 か月の月の平均の閲覧 {PV} 以上 か 全文の目安の数 {ME} 以上 (記事の無い語・転送だけの語)\n")
    f.write("# 入れる\t線からの近さ\t出どころ\t語\t読み\t月の平均の閲覧\tいちばん多かった月\t全文の目安の数\tメモ (受賞・上位の月)\tカテゴリ・注意の表\n")
    for it in items:
        s = score(it)
        f.write("\t".join(map(str, ["入れる" if s >= 1 else "外す", f"{s:.2f}", it[0], it[1], it[2], "" if it[3] is None else it[3], it[4], "" if it[5] is None else it[5], it[6], it[7]])) + "\n")
from collections import Counter
print(Counter((it[0], "入れる" if score(it) >= 1 else "外す") for it in items))
