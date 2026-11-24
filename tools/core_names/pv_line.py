"""2026-11-07 C: 仕事 44 の物差しの直し (まとめ役 12:24)。候補 (a_candidates.tsv) の記事ごとに、月ごとの閲覧数のダンプ (pageviews/months/*.tsv、
人の閲覧、日本語版) の月の平均を付け、線の案ごとに入る件数と試し問題の通る数を出す。記事の題は空白を _ にして引く。同じ表記と読みの行は、その記事の閲覧数の合計。
印 (人かどうか確かめられない → 今は 名前でない記事の題・表記がふつうの語と同じ・1 字) のある行は線の外のまま。上場企業は線によらず入れる (11:38 の決め) ので別に数える。
使い方: python3 -I pv_line.py <a_candidates.tsv> <months dir> <horses names_types.tsv> <out.tsv> [線 ...]"""
import glob, os, re, sys
from collections import Counter, defaultdict
ac_p, mdir, nt_p, out_p = sys.argv[1:5]
lines = [int(x) for x in sys.argv[5:]] or [100, 300, 1000]
months = sorted(glob.glob(os.path.join(mdir, "2*.tsv")))
tot = defaultdict(int)
for m in months:
    for l in open(m, encoding="utf-8"):
        t, _, n = l.rstrip("\n").rpartition("\t")
        if n.isdigit(): tot[t] += int(n)
avg = {t: v / len(months) for t, v in tot.items()}
def rows(p): return [l.rstrip("\n").split("\t") for l in open(p, encoding="utf-8") if not l.startswith("#") and l.strip()]
ac = rows(ac_p)
def pv1(title): return avg.get(title.replace(" ", "_"), 0.0)
# 同じ表記と読みの行 (同じ名前の別の人) は 1 行になるので、閲覧数はその表記と読みの記事の合計 (まとめ役 12:53)
group = defaultdict(float)
for r in ac: group[(r[1], r[2])] += pv1(r[8])
pv_of_row = {id(r): group[(r[1], r[2])] for r in ac}
title_rows = defaultdict(list)
for r in ac: title_rows[r[8]].append(r)
def pv(title): return max((pv_of_row[id(r)] for r in title_rows.get(title, [])), default=pv1(title))
with open(out_p, "w", encoding="utf-8") as f:
    f.write(f"# 月の閲覧数のダンプ {len(months)} か月 ({', '.join(os.path.basename(m)[:6] for m in months)}) の月の平均\n# 種類\t表記\t読み\t月の平均の閲覧\t上位の月の数\t全文の目安の数\t印\t記事\n")
    for r in sorted(ac, key=lambda r: -pv(r[8])):
        f.write("\t".join([r[0], r[1], r[2], f"{pv(r[8]):.0f}", r[5], r[3], r[7], r[8]]) + "\n")
print(f"月の数 {len(months)}、記事 {len(avg)}")
clean = [r for r in ac if not r[7] and r[0] != "会社"]
print("候補 (印なし、会社を除く)", len(clean), "会社 (線によらない)", sum(1 for r in ac if r[0] == "会社" and not r[7]))
nt = rows(nt_p); gi_win = [r for r in nt if int(r[5]) > 0 and not r[2].startswith("(a)")]
title_of = {r[1]: r[8] for r in ac}
for L in lines:
    inc = [r for r in clean if pv(r[8]) >= L]
    g = sum(1 for r in gi_win if r[0] in title_of and pv(title_of[r[0]]) >= L)
    probes = {w: (pv(title_of[w]) >= L if w in title_of else None) for w in ("VIVANT", "トウカイテイオー")}
    print(f"線 月 {L} 以上: {len(inc)}", dict(Counter(r[0] for r in inc).most_common()), f"| GI 勝ち馬 (辞書に無い {len(gi_win)}) のうち {g}", "| 試し:", probes)
