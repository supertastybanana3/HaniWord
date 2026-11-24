"""2026-11-07 C: 仕事 44。固有名詞の候補 (a_candidates.tsv) の線の案: 閲覧数の上位 1,000 に入った月の数 TM 以上 か 全文の目安の数 ME 以上。
線からの近さ = max(上位の月/TM, 目安の数/ME)。印 (英字だけ・1 字・表記がふつうの語と同じ・人かどうか確かめられない) のある行は線の外。
使い方: python3 -I a_line.py <a_candidates.tsv> <out.tsv> [TM ME]"""
import sys
src, out = sys.argv[1:3]
TM, ME = (int(sys.argv[3]), int(sys.argv[4])) if len(sys.argv) > 4 else (1, 300)
rows = [l.rstrip("\n").split("\t") for l in open(src, encoding="utf-8") if not l.startswith("#")]
def e(r): return int(r[3]) if not r[3].endswith("?") else 0
sc = sorted(((max(int(r[5]) / TM, e(r) / ME), r) for r in rows if not r[7]), key=lambda x: -x[0])
with open(out, "w", encoding="utf-8") as f:
    f.write(f"# 線: 閲覧数の上位 1,000 に入った月 {TM} 以上 か 全文の目安の数 {ME} 以上\n# 入れる\t線からの近さ\t種類\t表記\t読み\t全文の目安の数\t上位の月の数\t直近 12 か月の上位の月\t記事\tメモ\n")
    for s, r in sc:
        f.write("\t".join(["入れる" if s >= 1 else "外す", f"{s:.2f}", r[0], r[1], r[2], r[3], r[5], r[6], r[8], r[9]]) + "\n")
n = sum(1 for s, _ in sc if s >= 1); print(n, "/", len(sc))
for s, r in sc[n - 10:n + 10]: print(("入れる" if s >= 1 else "外す"), f"{s:.2f}", r[0], r[1], r[2], "目安", r[3], "上位の月", r[5], r[9][:18])
