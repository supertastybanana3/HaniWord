"""2026-11-05 A (仕事 22 の 2): 数字＋/＋数字 のすぐ後ろの語で / を ・ にする表を、試験でない記事の数 (digit_pair_follow.tsv) から作り、試験の記事の数で確かめる。
語は 漢字・カタカナの並び (4 字まで) の頭 (1〜4 字。それ自身が並びの全体として出てくるもの)。頭ごとに ・ と / (／ を含む) の回数を合わせ、合計 min 以上・・ の割合 ratio 以上の頭を選ぶ。
選んだ頭の後ろに長い頭で割合が 0.5 未満のもの (合計 min 以上) があれば、除く語として持つ (いちばん長い頭で決める)
  python3 -I follow_table.py <dev の digit_pair_follow.tsv> <test の digit_pair_follow.tsv> <min> <ratio> [--write 出力]"""
import collections, sys

dev_path, test_path, MIN, RATIO = sys.argv[1], sys.argv[2], int(sys.argv[3]), float(sys.argv[4])
out_path = sys.argv[sys.argv.index("--write") + 1] if "--write" in sys.argv else None


def runs(path):
    c = collections.defaultdict(lambda: [0, 0])
    for l in open(path, encoding="utf-8"):
        if l.startswith("#"): continue
        k, sep, n = l.rstrip("\n").split("\t")
        c[k][0 if sep == "・" else 1] += int(n)
    return c


dev, test = runs(dev_path), runs(test_path)
pre = collections.defaultdict(lambda: [0, 0])
for k, (d, s) in dev.items():
    if ":" in k: continue
    for L in range(1, len(k) + 1):
        pre[k[:L]][0] += d; pre[k[:L]][1] += s
# 頭はそれ自身が語として出てくるもの (並びの全体として 10 回以上、か 4 字で切れた並び)。合併号 の 合・丁目 の 丁 のような 1 字の頭は選ばない (合計 も拾うので)
whole = {k for k, (d, s) in dev.items() if ":" not in k and (d + s >= 10 or len(k) == 4)}
chosen = {p for p, (d, s) in pre.items() if p in whole and d + s >= MIN and d / (d + s) >= RATIO}
# いちばん短い頭だけ残す (長い頭は同じ決め)。除く語: 選んだ頭で始まり、割合 0.5 未満・合計 MIN 以上の長い頭
minimal = {p for p in chosen if not any(p[:L] in chosen for L in range(1, len(p)))}
exclude = {p for p, (d, s) in pre.items() if d + s >= MIN and d / (d + s) < 0.5 and any(p.startswith(q) and p != q for q in minimal)}


def decide(k):
    if ":" in k: return False
    best = None
    for L in range(len(k), 0, -1):
        if k[:L] in exclude: return False
        if k[:L] in minimal: return True
    return False


def score(c):
    good = bad = miss = 0
    for k, (d, s) in c.items():
        if decide(k): good += d; bad += s
        else: miss += d
    return good, bad, miss


for name, c in (("dev", dev), ("test", test)):
    g, b, m = score(c)
    total_slash = sum(s for d, s in c.values())
    print(f"{name}: 語 {len(minimal)}・除く {len(exclude)}  ・ に直す 良 {g}・悪 {b} (/ の {b / max(total_slash, 1):.4f})・直さない ・ {m}")
if out_path:
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write(f"# 数字＋/＋数字 のすぐ後ろが この語で始まれば / を ・ に (合計 {MIN} 以上・・ の割合 {RATIO} 以上)。- で始まる行は除く語。\n")
        f.write("# 出典: Wikipedia 日本語版 (wikimedia/wikipedia 20231101.ja。試験の記事を外した本文) の回数 (tools/slash_dot/scan_slash_dot.py・follow_table.py)\n")
        for p in sorted(minimal, key=lambda p: -sum(pre[p])):
            d, s = pre[p]; f.write(f"{p}\t{d}\t{s}\n")
        for p in sorted(exclude):
            d, s = pre[p]; f.write(f"-{p}\t{d}\t{s}\n")
