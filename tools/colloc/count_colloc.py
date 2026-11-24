"""2026-11-06 (仕事 (4) 組み合わせの数で同音の字を選び直す): 文の集まりで、同音の読みの語と、そのすぐ前後の語の組を数える。
  python3 count_colloc.py <同音の読み.txt> <out.tsv> <何番目> <何本に分ける> <文の tsv>...
文の tsv: 記事 ID<TAB>文。<何番目>/<何本に分ける> で、行を分けて並べて流す (行番号 % 本数 == 番目 の行だけ)。
Sudachi (full 辞書、SplitMode.B) で語に分け、読み (reading_form) が同音の読みで、表記に漢字を含む語について、次の 4 つの文脈で数える:
  R1 すぐ後ろの語 (内容語: 名詞・動詞・形容詞・形状詞) の表記           (効く|薬)
  R2 すぐ後ろが格助詞 (を に が で と へ から より) で、その後ろの内容語   (感傷|に|浸る・機械|を|操作)
  L1 すぐ前の内容語                                                     (よく|効く は副詞なので数えない。新しい|機械)
  L2 前が格助詞で、その前の内容語                                       (薬|が|効く)
動詞・形容詞は活用した表記のまま (効く・効い)。記号・数字・空白の語は文脈にしない。
<out> は書き出しの頭の名前: 200 万文ごとに、それまでの数を <out>.<番号>.tsv (読み<TAB>文脈の種類<TAB>文脈<TAB>表記<TAB>回数) に書いて空にする
(メモリを抑えるため。足し合わせは run_merge.sh)。
途中で止めたときは、済んだ書き出しの数 K から続けられる: 環境変数 COLLOC_FIRST_SPILL=K と COLLOC_SKIP=K×200 万 で、済んだ文を語に分けずに飛ばし、
書き出しの番号を K から付ける (飛ばす文の数え方は数える文と同じ。語に分けられなかった文のぶんだけ、境目が数文ずれることがある)"""
import collections, os, re, sys

from sudachipy import Dictionary, SplitMode

readings_path, out, part, parts, *paths = sys.argv[1:]
part, parts = int(part), int(parts)
homophones = set(open(readings_path, encoding="utf-8").read().split())
KANJI = re.compile(r"[一-鿿㐀-䶿々]")
CONTENT = {"名詞", "動詞", "形容詞", "形状詞"}
CASE = {"を", "に", "が", "で", "と", "へ", "から", "より"}
tok = Dictionary(dict="full").create()
counts = collections.Counter()


def content(t):
    pos = t.part_of_speech()
    return pos[0] in CONTENT and pos[1] not in ("数詞",) and not re.fullmatch(r"[\W\d_a-zA-Z０-９]+", t.surface())


def case(t):
    pos = t.part_of_speech()
    return pos[0] == "助詞" and pos[1] == "格助詞" and t.surface() in CASE


n = 0
spill = int(os.environ.get("COLLOC_FIRST_SPILL", "0"))
skip = int(os.environ.get("COLLOC_SKIP", "0"))


def flush():
    global spill, counts
    with open(f"{out}.{spill:03d}.tsv", "w", encoding="utf-8", newline="\n") as f:
        for (reading, kind, ctx, surface), v in counts.items():
            f.write(f"{reading}\t{kind}\t{ctx}\t{surface}\t{v}\n")
    print(n, "spill", spill, len(counts), flush=True)
    spill += 1
    counts = collections.Counter()


for path in paths:
    for lineno, line in enumerate(open(path, encoding="utf-8")):
        if lineno % parts != part:
            continue
        text = line.rstrip("\n").partition("\t")[2]
        if not text or len(text) > 2000:
            continue
        if skip:
            skip -= 1
            n += 1
            continue
        try:
            ts = list(tok.tokenize(text, SplitMode.B))
        except Exception:
            continue
        for i, t in enumerate(ts):
            surface = t.surface()
            if not KANJI.search(surface):
                continue
            reading = t.reading_form()
            if reading not in homophones:
                continue
            if i + 1 < len(ts):
                a = ts[i + 1]
                if content(a):
                    counts[(reading, "R1", a.surface(), surface)] += 1
                elif case(a) and i + 2 < len(ts) and content(ts[i + 2]):
                    counts[(reading, "R2", a.surface() + "|" + ts[i + 2].surface(), surface)] += 1
            if i >= 1:
                b = ts[i - 1]
                if content(b):
                    counts[(reading, "L1", b.surface(), surface)] += 1
                elif case(b) and i >= 2 and content(ts[i - 2]):
                    counts[(reading, "L2", ts[i - 2].surface() + "|" + b.surface(), surface)] += 1
        n += 1
        if n % 2_000_000 == 0:
            flush()
flush()
print("done", n, flush=True)
