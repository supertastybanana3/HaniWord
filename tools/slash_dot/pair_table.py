"""2026-11-05 A (仕事 22 の 2、まとめ役 12:28): 英字の語どうしの決まった組 (TCP/IP・I/O・A/B) の表を、試験でない記事の数 (latin_pairs.tsv) から作り、試験の記事で確かめる。
左の語が小文字の語・小文字 1 字の組は入れない (IME 本体が打った時点で / にする)。組ごとに / (／ を含む) と ・ を数え、合計 min 以上・/ の割合 ratio 以上の組を選ぶ
  python3 -I pair_table.py <dev の latin_pairs.tsv> <test の latin_pairs.tsv> <min> <ratio> [--write 出力]"""
import collections, sys

dev_path, test_path, MIN, RATIO = sys.argv[1], sys.argv[2], int(sys.argv[3]), float(sys.argv[4])
out_path = sys.argv[sys.argv.index("--write") + 1] if "--write" in sys.argv else None


def load(path):
    c = collections.defaultdict(lambda: [0, 0])
    for l in open(path, encoding="utf-8"):
        if l.startswith("#"): continue
        a, b, sep, n = l.rstrip("\n").split("\t")
        c[(a, b)][1 if sep in "/／" else 0] += int(n)
    return c


dev, test = load(dev_path), load(test_path)
typed_slash = lambda a: a.islower()  # 小文字の語・小文字 1 字は IME 本体が / にする
chosen = {k for k, (d, s) in dev.items() if not typed_slash(k[0]) and d + s >= MIN and s / (d + s) >= RATIO}
for name, c in (("dev", dev), ("test", test)):
    good = sum(c[k][1] for k in chosen if k in c); bad = sum(c[k][0] for k in chosen if k in c)
    dots = sum(d for k, (d, s) in c.items() if not typed_slash(k[0]))
    print(f"{name}: 組 {len(chosen)}  / に直す 良 {good}・悪 {bad} (・ の {bad / max(dots, 1):.4f})")
if out_path:
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write(f"# 英字の語どうしの区切りを / にする組 (合計 {MIN} 以上・/ の割合 {RATIO} 以上。左が小文字の語は除く)。左<TAB>右<TAB>/ の回数<TAB>・ の回数\n")
        f.write("# 出典: Wikipedia 日本語版 (wikimedia/wikipedia 20231101.ja。試験の記事を外した本文) の回数 (tools/slash_dot/scan_latin_pairs.py・pair_table.py)\n")
        for k in sorted(chosen, key=lambda k: -sum(dev[k])):
            d, s = dev[k]; f.write(f"{k[0]}\t{k[1]}\t{s}\t{d}\n")
