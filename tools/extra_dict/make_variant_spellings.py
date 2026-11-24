"""代わりの字の成句の表 (variant_spellings.tsv) と、成句の辞書に足す行 (core_phrases_prefer.txt) を作る (2026-10-22)。
本来の字 (表外字) を意味の違う常用漢字で代わりに書く成句 (論を俟たない/論を待たない) で、変換サーバーは
変換の結果に代わりの字の書き方が句の読みのまま出たら本来の書き方に置き換え、候補の一覧では本来の書き方とかな書きを代わりの字の書き方より前に置く。
入力 variant_pairs.txt (読み・本来の書き方・代わりの字の書き方。2 つの判定がそろった組)。
活用形は make_core_phrases.py と同じ作り方で本来の書き方から作り、同じ所の字を替えて代わりの字・かな書きの形にする (機会を窺っ/機会を伺っ/機会をうかがっ)。
かな書きは、JMdict の同じ項目の書き方のうち、本来の書き方の漢字から表外字を除いた漢字だけを使うもの (論をまたない。JMdict の並び順で最初のもの)。
出力 variant_spellings.tsv: 読み (ひらがな)<TAB>代わりの字の書き方<TAB>本来の書き方<TAB>かな書き (無ければ空)
--current <conv.jsonl> (conv_probe.py で組の読みを今の変換にかけた結果) を付けると、成句の辞書に足す行 (読み<TAB>書き方<TAB>値。make_core_phrases.py --prefer) も書く:
  今の変換が代わりの字なら 本来の書き方 -11 だけ (かな書きも足すと AI がかな書きを選ぶことがある。かな書きはサーバーが候補の一覧に入れる) /
  今が JMdict のかな書きなら それ -11・本来の書き方 -11.5 /
  今がほかのかな書きなら 本来の書き方 -11.5 だけ / 今が本来の書き方・ほかの標準の書き方なら足さない (句を 1 つの文節にして、本来の書き方を候補に出すため)
組の表は , でつないで複数渡せる (variant_pairs.txt,misuse_spellings.txt。字の誤り 危機一発/危機一髪 も同じ仕組み)
使い方: HANIWORD_BUILD=<作業場所> <Sudachi のある Python> make_variant_spellings.py <作業の置き場 (azookey_dict.tsv・azk_pos.tsv・jmdict_siblings.json)>
        <variant_pairs.txt[,misuse_spellings.txt]> <variant_spellings.tsv> [--current <conv.jsonl> <core_phrases_prefer.txt>]"""
import json, os, re, subprocess, sys, tempfile
from common import hira, kata

args = sys.argv[1:]
current_path = prefer_out = None
if "--current" in args:
    i = args.index("--current"); current_path, prefer_out = args[i + 1], args[i + 2]; del args[i:i + 3]
work, pairs_path, out = args[:3]
KANJI = re.compile(r"[一-鿿々〆]")
siblings = json.load(open(os.path.join(work, "jmdict_siblings.json"), encoding="utf-8"))
pairs = []
for l in (line for path in pairs_path.split(",") for line in open(path, encoding="utf-8")):
    if not l.strip() or l.startswith("#"):
        continue
    reading, orig, sub = l.rstrip("\n").split("\t")[:3]
    diff = [k for k, (a, b) in enumerate(zip(orig, sub)) if a != b]
    assert len(orig) == len(sub) and len(diff) == 1, l
    d = diff[0]
    allowed = set(KANJI.findall(orig)) - {orig[d]}
    order = siblings.get(f"{reading}\t{orig}", [])   # JMdict の並び順
    # 本来の字の所をカタカナにした書き方 (ゲキを飛ばす) はかな書きにしない (本来の書き方に無いカタカナを使わない)
    kata_chars = lambda t: set(re.findall(r"[ァ-ヺー]", t))
    kana = [k for k in order if k not in (orig, sub, reading) and set(KANJI.findall(k)) <= allowed and kata_chars(k) <= kata_chars(orig)]
    pairs.append({"reading": reading, "orig": orig, "sub": sub, "d": d, "kana": kana[0] if kana else "",
                  "rank": order.index(orig) if orig in order else len(order)})


def kana_form(p, form):
    """本来の書き方の形 (form) を、かな書きの形にする (本来の書き方とかな書きの違う所を差し替える)"""
    o, k = p["orig"], p["kana"]
    if not k:
        return ""
    pre = os.path.commonprefix([o, k])
    suf = os.path.commonprefix([o[len(pre):][::-1], k[len(pre):][::-1]])[::-1]
    mid_o, mid_k = o[len(pre):len(o) - len(suf)], k[len(pre):len(k) - len(suf)]
    return pre + mid_k + form[len(pre) + len(mid_o):] if form.startswith(pre + mid_o) else ""


# 本来の書き方の活用形を make_core_phrases.py で作る (足す行と同じ作り方)
here = os.path.dirname(os.path.abspath(__file__))
with tempfile.TemporaryDirectory() as tmp:
    prefer, empty, forms_out = (os.path.join(tmp, n) for n in ("prefer.tsv", "empty.jsonl", "forms.tsv"))
    open(prefer, "w", encoding="utf-8").writelines(f"{p['reading']}\t{p['orig']}\t-11\n" for p in pairs)
    open(empty, "w", encoding="utf-8").close()
    subprocess.run([sys.executable, os.path.join(here, "make_core_phrases.py"), work, empty, forms_out, "--prefer", prefer], check=True, stdout=subprocess.DEVNULL)
    forms = [l.rstrip("\n").split("\t")[:2] for l in open(forms_out, encoding="utf-8") if l.strip() and not l.startswith("#")]
rows = {}
for p in pairs:
    head = p["orig"][:p["d"] + 1]
    # 本来の書き方が azooKey にもうある組 (危機一髪) は make_core_phrases.py が形を作らないので、基本の形だけ入れる
    own = [(s, r) for s, r in forms if s.startswith(head)] or [(p["orig"], kata(p["reading"]))]
    for surface, reading in own:
        if not surface.startswith(head):
            continue
        sub = surface[:p["d"]] + p["sub"][p["d"]] + surface[p["d"] + 1:]
        # 同じ代わりの字の書き方に本来の書き方が 2 つあるとき (チャンスを窺う・チャンスを覗う) は、JMdict で先に並ぶほう
        key = (hira(reading), sub)
        if key not in rows or p["rank"] < rows[key][2]:
            rows[key] = (surface, kana_form(p, surface), p["rank"])
with open(out, "w", encoding="utf-8", newline="\n") as f:
    f.write("# 代わりの字の成句 (変換サーバーが読む。tools/extra_dict/make_variant_spellings.py が extra_dictionaries/variant_pairs.txt から作る)。\n"
            "# 読み<TAB>代わりの字の書き方<TAB>本来の書き方<TAB>かな書き。変換の結果に代わりの字の書き方が句の読みのまま出たら本来の書き方にし、\n"
            "# 候補の一覧では本来の書き方とかな書きを代わりの字の書き方より前に置く。書き方は JMdict (CC BY-SA 4.0、extra_dictionaries/LICENSE.txt) から\n")
    for (reading, sub), (orig, kana, _) in sorted(rows.items()):
        f.write(f"{reading}\t{sub}\t{orig}\t{kana}\n")
print("組", len(pairs), "行 (活用形を含む)", len(rows))

if current_path:
    cur = {json.loads(l)["reading"]: json.loads(l)["full"] for l in open(current_path, encoding="utf-8")}
    out_rows = []
    by_reading = {}
    for p in pairs:
        by_reading.setdefault(p["reading"], []).append(p)
    for reading, ps in by_reading.items():
        subs = {p["sub"] for p in ps}
        p0 = min(ps, key=lambda p: p["rank"])   # JMdict で先に並ぶ本来の書き方
        o, k, now = p0["orig"], p0["kana"], cur.get(reading)
        if now is None:
            continue
        if now in subs:
            act = [(o, -11.0)]
        elif k and now == k:
            act = [(k, -11.0), (o, -11.5)]
        elif not KANJI.search(now) and now != o:
            act = [(o, -11.5)]
        else:
            act = []
        out_rows += [(reading, s, v) for s, v in act]
    with open(prefer_out, "w", encoding="utf-8", newline="\n") as f:
        f.write("# 成句の辞書に足す行 (make_core_phrases.py --prefer。make_variant_spellings.py --current が作る)。読み<TAB>書き方<TAB>値\n")
        f.writelines(f"{r}\t{s}\t{v}\n" for r, s, v in out_rows)
    print("成句の辞書に足す行", len(out_rows))
