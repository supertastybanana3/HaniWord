"""(2026-10-22、M3.5 の C: 使われている抜けの語) pick_used.py の候補 (used.tsv) と、今のサーバーでの変換 (used_conv.jsonl) から、
オフにできない追加辞書の行 (C の形: 表記・読み・cid・mid・値・出典) を作る。
残すのは: Mozc の項目で語の終わり (右の品詞) が名詞・表記がかなを含まない (漢字・カタカナだけ。助詞をはさむ句は成句の受け持ち)・
今の変換 (AI) で正しく出ない・今の変換との違いが表記ゆれや数字の書き方だけでない (写経の集計 probe/e2e/analyze.py の kind。数字の書き方は規則では変えない)。
品詞の番号 (cid)・意味の番号 (mid) は、両方の辞書にある語で、Mozc の右の品詞ごとに一番多い azooKey の右の番号 (追加辞書は左右 1 つの番号なので、後ろにつながる側に合わせる)。
値は、両方にある語の Mozc のコストと azooKey の値の当てはめ (品詞ごと) から 1 下げ、-10 より上にしない。
記事で 2 つに分け (文の数がそろうように)、片方の記事に出た語だけの辞書 (core_wordsa / core_wordsb) と、その反対側の文の組も作る (選んだ文と別の文で測るため)
出力の置き場には used.tsv (pick_used.py) と used_conv.jsonl (conv_probe.py か tools/domain/convert.py で、used.tsv の 読み・表記 を変換したもの) を置く
使い方: HANIWORD_BUILD=<作業場所> <Sudachi のある Python> make_core_words.py <作業の置き場 (azookey_dict.tsv のある所)> <出力の置き場>"""
import collections, glob, json, os, sys
from common import B, MOZC as M, analyze as load_analyze
work, out = sys.argv[1], sys.argv[2]
analyze = load_analyze()
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
iddef = {int(l.split(" ", 1)[0]): l.rstrip("\n").split(" ", 1)[1] for l in open(f"{M}/id.def", encoding="utf-8")}
mozc = {}
for path in sorted(glob.glob(f"{M}/dictionary0*.txt")):
    for l in open(path, encoding="utf-8"):
        p = l.rstrip("\n").split("\t")
        if len(p) >= 5:
            k = (kata(p[0]), p[4])
            if k not in mozc or int(p[3]) < mozc[k][2]: mozc[k] = (int(p[1]), int(p[2]), int(p[3]))
cid_of, mids, fit_pts = collections.defaultdict(collections.Counter), collections.defaultdict(collections.Counter), collections.defaultdict(list)
for l in open(os.path.join(work, "azookey_dict.tsv"), encoding="utf-8"):
    r, w, lc, rc, mid, v = l.rstrip("\n").split("\t")
    if (r, w) in mozc:
        rid = mozc[(r, w)][1]
        cid_of[rid][int(rc)] += 1; mids[(rid, int(rc))][int(mid)] += 1
        fit_pts[iddef.get(rid, "?").split(",")[0]].append((mozc[(r, w)][2], float(v)))
def fit(xs):
    n = len(xs); mx = sum(c for c, _ in xs) / n; my = sum(v for _, v in xs) / n
    a = sum((c - mx) * (v - my) for c, v in xs) / sum((c - mx) ** 2 for c, _ in xs)
    return a, my - a * mx
noun_a, noun_b = fit(fit_pts["名詞"])
conv = {(analyze.hira(json.loads(l)["reading"]), json.loads(l)["expected"]): json.loads(l) for l in open(os.path.join(out, "used_conv.jsonl"), encoding="utf-8")}
rows, why = [], collections.Counter()
for l in open(os.path.join(out, "used.tsv"), encoding="utf-8"):
    ruby, word, lid, rid, cost, pos, arts = l.rstrip("\n").split("\t")
    lid, rid, cost = int(lid), int(rid), int(cost)
    c = conv[(analyze.hira(ruby), word)]
    if iddef.get(rid, "").split(",")[0] != "名詞": why["終わりが名詞でない"] += 1; continue
    if any("ぁ" <= ch <= "ゖ" for ch in word): why["かなを含む"] += 1; continue
    if c["ok"]: why["今の変換で正しい"] += 1; continue
    if analyze.kind(word, c["full"]) in ("表記ゆれ", "数字や記号"): why["違いが表記ゆれ・数字だけ"] += 1; continue
    if rid not in cid_of: why["品詞の番号が分からない"] += 1; continue
    cid = cid_of[rid].most_common(1)[0][0]
    mid = mids[(rid, cid)].most_common(1)[0][0]
    value = max(-40.0, min(-10.0, noun_a * cost + noun_b - 1))
    rows.append((word, ruby, cid, mid, round(value, 3), "Mozc 公開辞書 (IPAdic)", {a.split(":")[0] for a in arts.split(";")}, c["full"]))
    why["残す"] += 1
print(dict(why), f"名詞の値 ≈ {noun_a * 1000:.3f}/1000*cost + {noun_b:.2f} - 1")
# 記事を 2 つに分ける (文の数がそろうように、文の多い記事から少ないほうへ)
groups = {}
for i in range(1, 5):
    for l in open(f"{B}/probe/e2e/results/night_installer-full800_part{i}.txt", encoding="utf-8-sig"):
        p = l.rstrip("\n").split("\t")
        if len(p) > 2: groups.setdefault(p[1], p[2])
cases = [l for l in open(f"{B}/data_ext/domain/eval/wiki_luna_full.cases.tsv", encoding="utf-8") if l.strip() and not l.startswith("#")]
size = collections.Counter(groups[l.split("\t")[1]] for l in cases)
fold, n = {}, {"a": 0, "b": 0}
for art, k in size.most_common():
    f = min(n, key=n.get); fold[art] = f; n[f] += k
print("文の数", n)
# 2026-11-06: 読みの直し (extra_dictionaries/core_words_reading_fixes.txt。check_core_readings.py で出して目で見て決めたもの) を当てる
fixes = {}
for l in open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "extra_dictionaries", "core_words_reading_fixes.txt"), encoding="utf-8"):
    p = l.rstrip("\n").split("\t")
    if not l.startswith("#") and len(p) >= 3:
        fixes[(p[0], p[1])] = p[2]
rows = [(r[0], fixes.get((r[0], r[1]), r[1])) + tuple(r[2:]) for r in rows if fixes.get((r[0], r[1]), r[1]) != ""]
def write(name, items):
    with open(os.path.join(out, name), "w", encoding="utf-8") as f:
        f.write("# 表記\t読み\tcid\tmid\t値\t出典\n" + "".join(f"{w}\t{r}\t{c}\t{m}\t{v}\t{s}\n" for w, r, c, m, v, s, *_ in sorted(items)))
write("core_words.tsv", rows)
for f in "ab":
    write(f"core_words{f}.tsv", [r for r in rows if any(fold.get(a) == f for a in r[6])])
    with open(os.path.join(out, f"cases_{f}.tsv"), "w", encoding="utf-8") as o:
        o.writelines(l for l in cases if fold[groups[l.split("\t")[1]]] == f)
print("辞書", len(rows), "a", sum(any(fold.get(a) == "a" for a in r[6]) for r in rows), "b", sum(any(fold.get(a) == "b" for a in r[6]) for r in rows))
for r in rows[::15][:25]: print(" ", r[0], r[1], r[2], r[3], r[4], "今", r[7])
