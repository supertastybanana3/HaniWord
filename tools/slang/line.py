"""2026-11-07 C: 仕事 43。まれの線の案: 月の平均の閲覧 PV 以上 か 目安の数 ME 以上 なら入れる。線からの近さ = max(閲覧/PV, 目安の数/ME)。
かなだけの 3 字までの語 (りょ・みかか) は、全文の数が別の語の一部を拾うので目安の数を使わない (閲覧だけ)。
使い方: python3 -I line.py <proposal.tsv> <out.tsv> [PV ME]"""
import re, sys
src, out = sys.argv[1:3]
PV, ME = (int(sys.argv[3]), int(sys.argv[4])) if len(sys.argv) > 4 else (300, 20)
rows = [l.rstrip("\n").split("\t") for l in open(src, encoding="utf-8") if not l.startswith("#")]
sc = []
for r in rows:
    if r[10] != "(a) の候補": continue
    e = None if re.fullmatch(r"[ぁ-ゖァ-ヺー]{1,3}", r[1]) else int(r[7])
    sc.append((max(int(r[5]) / PV, (e or 0) / ME), r, e))
sc.sort(key=lambda x: -x[0])
with open(out, "w", encoding="utf-8") as f:
    f.write(f"# 線: 月の平均の閲覧 {PV} 以上 か 目安の数 {ME} 以上\n# 入れる\t線からの近さ\t語\t読み\t月の平均の閲覧\t目安の数 (使わない は空)\t注意の表\n")
    for s, r, e in sc:
        f.write("\t".join(["入れる" if s >= 1 else "外す", f"{s:.2f}", r[1], r[2], r[5], "" if e is None else str(e), r[9]]) + "\n")
print(sum(1 for s, _, _ in sc if s >= 1), "/", len(sc))
