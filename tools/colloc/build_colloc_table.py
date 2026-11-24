"""2026-11-06 (仕事 (4) 組み合わせの数で同音の字を選び直す): count_colloc.py の書き出しを足し合わせたものから、変換サーバーが読む表 colloc_choices.tsv を作る。
  python3 -I build_colloc_table.py <足し合わせた数.tsv> <out colloc_choices.tsv> [いくつ以上] [何倍以上] [どの語にも付く語の線]
足し合わせた数.tsv: 読み<TAB>文脈の種類<TAB>文脈<TAB>表記<TAB>回数 (同じ組は 1 行、この 4 つの欄の順に並べたもの。run_merge.sh)。数千万行になるので、並びのまま 2 回読む。
1 回目: 読みごとの表記の回数 (いちばん多い表記) と、文脈の語 (R1・R2 は後ろの語、L1・L2 は前の語) ごとに、組になる読みの数。
2 回目: 読み・文脈ごとに選ぶ (2026-11-07 の v2):
- 組になる読みが <どの語にも付く語の線> (初期値 10,000) を超える文脈の語は使わない。する の し (後ろ側 49,748)・こと・年・日 (前側 25,702)・なる・ある など。
  v1 では「後ろが し」が回数の多いサ変の名詞 (精算 → 生産)、「日|に」がウィキペディアの「○日に公開」(航海 → 公開) に引っぱっていた。
  残る語の例: 東京 (8,356〜9,165)・操作 (2,896〜3,182)・薬・買う・浸る。外した語は <out>.generic.tsv に書く
- いちばん多い表記の回数が <いくつ以上> (初期値 10) の文脈を表に入れる。その回数の 1/<何倍以上> (初期値 10) より多く出る別の表記は「近い表記」として行に書き、
  変換サーバーはその表記を替えない (効く 83・利く 15 の 利く。どちらも正しい)。近い表記でない語 (聞く 1・出ていない語) は、いちばん多い表記に替えてよい
  (v1 は 2 番目の表記の 10 倍以上のときだけ表に入れていたので、近い正しい表記があると、外したい語 (聞く) も替えられなかった)
- その読みでいちばん多い表記を選び、ほかの表記が 1 つも出ていない文脈は入れない (変換もたいていその表記を選ぶ。表を小さくするため)。
  文章に表記が 1 つしか出ない読みも入れない
表の行: 読み<TAB>文脈の種類<TAB>文脈<TAB>選ぶ表記<TAB>その回数<TAB>近い表記 (表記:回数 を , でつなぐ。無ければ空)。
行は 読み<TAB>文脈の種類<TAB>文脈 の UTF-8 のバイトの順に並べる (変換サーバーは読み込まずに二分探索で引く。colloc.swift)"""
import collections, sys

src, out = sys.argv[1:3]
minimum = int(sys.argv[3]) if len(sys.argv) > 3 else 10
ratio = float(sys.argv[4]) if len(sys.argv) > 4 else 10.0
generic_line = int(sys.argv[5]) if len(sys.argv) > 5 else 10_000


def rows():
    for line in open(src, encoding="utf-8"):
        f = line.rstrip("\n").split("\t")
        if len(f) == 5 and f[4].isdigit():
            yield f[0], f[1], f[2], f[3], int(f[4])


def side_word(kind, ctx):
    return ("R", ctx.split("|")[-1]) if kind.startswith("R") else ("L", ctx.split("|")[0])


total = collections.defaultdict(collections.Counter)
fan = collections.Counter()
prev = None
for reading, kind, ctx, surface, n in rows():
    total[reading][surface] += n
    if (reading, kind, ctx) != prev:
        prev = (reading, kind, ctx)
        fan[side_word(kind, ctx)] += 1
dominant = {r: c.most_common(1)[0][0] for r, c in total.items() if len(c) >= 2}
del total
generic = {k for k, v in fan.items() if v > generic_line}
with open(out + ".generic.tsv", "w", encoding="utf-8", newline="\n") as g:
    g.write(f"# 組になる読みが {generic_line} を超えるので文脈にしない語 (側<TAB>語<TAB>読みの数)\n")
    for (side, word), v in sorted(((k, fan[k]) for k in generic), key=lambda kv: -kv[1]):
        g.write(f"{side}\t{word}\t{v}\n")
del fan

kept = collections.Counter()
groups = 0
table_rows = []
with open(out, "w", encoding="utf-8", newline="\n") as f:
    f.write(f"# 同音の読みの語と前後の語の組の回数で選ぶ表記 (日本語版ウィキペディアと青空文庫。tools/colloc の build_colloc_table.py v2: {minimum} 回以上、"
            f"近い表記は 1/{ratio:g} より多いもの、組になる読みが {generic_line} を超える文脈の語は使わない)\n")

    def emit(key, counts):
        global groups
        groups += 1
        reading, kind, ctx = key
        if reading not in dominant or side_word(kind, ctx) in generic:
            return
        (word, n), *_ = counts.most_common(1)
        if n < minimum or (word == dominant[reading] and len(counts) == 1):
            return
        close = [(s, c) for s, c in counts.items() if s != word and c * ratio > n]
        table_rows.append((f"{reading}\t{kind}\t{ctx}\t".encode("utf-8"), f"{reading}\t{kind}\t{ctx}\t{word}\t{n}\t{','.join(f'{s}:{c}' for s, c in sorted(close))}\n"))
        kept[kind] += 1

    key, counts = None, collections.Counter()
    for reading, kind, ctx, surface, n in rows():
        k = (reading, kind, ctx)
        if k != key:
            if key is not None:
                emit(key, counts)
            key, counts = k, collections.Counter()
        counts[surface] += n
    if key is not None:
        emit(key, counts)
    table_rows.sort(key=lambda r: r[0])
    for _, line in table_rows:
        f.write(line)
print(groups, "組", sum(kept.values()), "行", dict(kept), "外した文脈の語", len(generic))
