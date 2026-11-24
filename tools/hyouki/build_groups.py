"""2026-11-02: 表記ゆれの組 (かなと漢字・送り仮名のあるなし) の候補を azooKey の辞書から作る (段 1。重い数えは段 2 の count_forms.py)。
azooKey の辞書と追加辞書の行のうち、読み・左右の品詞の番号が同じで表記だけ違う行を集め、同じ語の書き分けと確かめられたものを組にする。
  同じ語の確かめ: JMdict の同じ項目に載る (活用した形は、後ろのかなを除いた語幹で見る)・Sudachi の正規化表記が同じ・漢字の骨格が同じ
  ゆれ: かな (かなだけの形がある)・送り仮名 (漢字の骨格が同じ)・交ぜ書き (片方の漢字の一部がかな)・々 (様々/様様)。漢字の違う組 (炒る/煎る) は入れない
  種類: Mozc の公開辞書の品詞 (名詞,非自立 → 形式名詞、動詞,非自立 → 補助動詞 など)。無ければ Sudachi の品詞
使い方 (Sudachi の入った python で):
  python -I build_groups.py <azookey_dict.tsv> <リポジトリ> <JMdict_e.gz> <mozc_oss> <通則7 の表> <出力の置き場>
出力: groups.tsv (組の表。回数の列は段 2 で足す)、targets.tsv (数える (読み, 形) と、組の語の正規化表記)、build_log.txt"""
import collections, gzip, os, re, sys, xml.etree.ElementTree as ET
from sudachipy import dictionary, tokenizer

azd, repo, jmdict_path, mozc_dir, tsusoku7_path, out = sys.argv[1:7]
os.makedirs(out, exist_ok=True)
log = open(os.path.join(out, "build_log.txt"), "w", encoding="utf-8")
def note(*a):
    print(*a); print(*a, file=log)

hira = lambda s: "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" and c != "ヵ" and c != "ヶ" else c for c in s)
HIRA = re.compile(r"[ぁ-ゖゝゞー]")
KANJI = re.compile(r"[々〆ヶ㐀-䶿一-鿿豈-﫿]")
WORD_OK = re.compile(r"^[ぁ-ゖゝゞー々〆ヶ㐀-䶿一-鿿豈-﫿]+$")
NAME_IDS = set(range(1289, 1294))  # 人名一般・姓・名・組織・地名


def expand_odoriji(w):
    o = []
    for c in w:
        o.append(o[-1] if c == "々" and o else c)
    return "".join(o)


def skeleton(w):
    return "".join(c for c in expand_odoriji(w) if not HIRA.match(c))


def is_subseq(a, b):
    it = iter(b)
    return all(c in it for c in a)


def common_kana_suffix(w, r):
    n = 0
    while n < min(len(w), len(r)) and w[-1 - n] == r[-1 - n] and HIRA.match(w[-1 - n]):
        n += 1
    return n


# --- azooKey の辞書と追加辞書の行 ---
cells = collections.defaultdict(dict)  # (読み (ひらがな), 左, 右) -> 表記 -> (値, 出どころ)
def add_row(r, w, lc, rc, v, src):
    if not WORD_OK.match(w) or lc in NAME_IDS or rc in NAME_IDS:
        return
    k = (hira(r), lc, rc)
    if w not in cells[k] or v > cells[k][w][0]:
        cells[k][w] = (v, src)

for l in open(azd, encoding="utf-8"):
    p = l.rstrip("\n").split("\t")
    if len(p) == 6:
        add_row(p[0], p[1], int(p[2]), int(p[3]), float(p[5]), "azooKey")
for name in ("core_forms", "core_words", "core_phrases"):
    for l in open(os.path.join(repo, "extra_dictionaries", name + ".tsv"), encoding="utf-8"):
        if l.startswith("#"):
            continue
        p = l.rstrip("\n").split("\t")
        if len(p) >= 5:
            add_row(p[1], p[0], int(p[2]), int(p[2]), float(p[4]), name)
multi = {k: d for k, d in cells.items() if len(d) >= 2}
note("組の候補の升 (読み・左・右が同じで表記が 2 つ以上):", len(multi), "/ 升の数", len(cells))

# --- JMdict: 書き方 (漢字) と読みの組 -> 項目。活用した形のため、後ろのかなを外した語幹でも引けるようにする ---
inf_map = {"ateji": "当て字", "irregular kanji": "まれな字", "irregular okurigana": "まれな送り仮名", "out-dated kanji": "古い字",
           "rarely used kanji": "まれな形", "search-only kanji": "検索用の形", "word containing irregular kana": "まれなかな"}
stem_index = collections.defaultdict(set)  # (語幹, 読みの語幹) -> 項目番号
form_info = {}  # (書き方, 読み) -> 書き方の印
entry_uk = set()
entry_head = {}
reading_entries = collections.defaultdict(set)
with gzip.open(jmdict_path) as f:
    for _, el in ET.iterparse(f):
        if el.tag != "entry":
            continue
        seq = el.findtext("ent_seq")
        kebs = [(k.findtext("keb"), [i.text or "" for i in k.findall("ke_inf")]) for k in el.findall("k_ele")]
        rebs = [(r.findtext("reb"), [x.text for x in r.findall("re_restr")], r.find("re_nokanji") is not None) for r in el.findall("r_ele")]
        if any("usually written using kana" in (m.text or "") for s in el.findall("sense") for m in s.findall("misc")):
            entry_uk.add(seq)
        entry_head[seq] = (kebs[0][0] if kebs else rebs[0][0])
        for reb, restr, nok in rebs:
            reading_entries[reb].add(seq)
            if nok:
                continue
            for keb, inf in kebs:
                if restr and keb not in restr:
                    continue
                marks = [v for k, v in inf_map.items() for i in inf if k in i]
                if marks:
                    form_info[(keb, reb)] = "・".join(sorted(set(marks)))
                n = common_kana_suffix(keb, reb)
                for k in range(n + 1):
                    stem_index[(keb[:len(keb) - k], reb[:len(reb) - k])].add(seq)
        el.clear()
note("JMdict の項目:", len(entry_head), "語幹の引き:", len(stem_index))


def jm_entries(w, r):
    n = common_kana_suffix(w, r)
    s = (w[:len(w) - n], r[:len(r) - n])
    return stem_index.get(s, set()) if s[0] else set()


# --- Mozc の公開辞書の品詞 ---
iddef = {}
for l in open(os.path.join(mozc_dir, "id.def"), encoding="utf-8"):
    i, name = l.rstrip("\n").split(" ", 1); iddef[int(i)] = name
mozc_pos = collections.defaultdict(set)
need = {(r, w) for (r, _, _), d in multi.items() for w in d}
for fn in sorted(os.listdir(mozc_dir)):
    if fn.startswith("dictionary0"):
        for l in open(os.path.join(mozc_dir, fn), encoding="utf-8"):
            p = l.rstrip("\n").split("\t")
            if len(p) >= 5 and (p[0], p[4]) in need:
                mozc_pos[(p[0], p[4])].add(iddef.get(int(p[1]), "?"))

# --- Sudachi: 1 語だけを解析した正規化表記と品詞 ---
tok = dictionary.Dictionary(dict="core").create()
MODE = tokenizer.Tokenizer.SplitMode.C
sud = {}
for w in sorted({w for d in multi.values() for w in d}):
    ms = tok.tokenize(w, MODE)
    sud[w] = (tuple(m.normalized_form() for m in ms), tuple(m.part_of_speech()[:2] for m in ms), "".join(m.reading_form() for m in ms))


def sudachi_same(a, b):
    return sud[a][0] == sud[b][0]


# --- 通則7 の語 ---
tsusoku7 = {}
for l in open(tsusoku7_path, encoding="utf-8"):
    if not l.startswith("#") and l.strip():
        w, kind = l.rstrip("\n").split("\t")
        if len(w) >= 2:
            tsusoku7[w] = "《" in kind  # True: 複合語の中で送らない (申込《書》)

# --- 今ある表 (重なりの確かめ) ---
existing = collections.defaultdict(set)
for l in open(os.path.join(repo, "variant_spellings.tsv"), encoding="utf-8"):
    if not l.startswith("#"):
        p = l.rstrip("\n").split("\t")
        for w in p[1:4]:
            if w: existing[w].add("variant_spellings")
for l in open(os.path.join(repo, "katakana_variants.tsv"), encoding="utf-8"):
    if not l.startswith("#"):
        p = l.rstrip("\n").split("\t")
        if len(p) >= 2: existing[p[1]].add("katakana_variants")
for l in open(os.path.join(repo, "extra_dictionaries", "core_misreadings.tsv"), encoding="utf-8"):
    if not l.startswith("#"):
        existing[l.split("\t")[0]].add("core_misreadings")
for l in open(os.path.join(repo, "extra_dictionaries", "misuse_spellings.txt"), encoding="utf-8"):
    if not l.startswith("#"):
        p = l.rstrip("\n").split("\t")
        for w in p[1:3]:
            if w: existing[w].add("misuse_spellings")

# --- 品詞の分け方 ---
ORDER = [("名詞,非自立", "形式名詞"), ("動詞,非自立", "補助動詞"), ("形容詞,非自立", "補助形容詞"), ("接続詞", "接続詞"), ("副詞", "副詞"),
         ("連体詞", "連体詞"), ("感動詞", "感動詞"), ("助動詞", "助動詞"), ("助詞", "助詞"), ("接頭詞", "接頭辞"), ("名詞,接尾", "接尾辞"),
         ("名詞,代名詞", "代名詞"), ("名詞,形容動詞語幹", "形容動詞"), ("形容詞", "形容詞"), ("動詞", "動詞"), ("名詞", "名詞")]
SUD = {"名詞": "名詞", "動詞": "動詞", "形容詞": "形容詞", "形状詞": "形容動詞", "副詞": "副詞", "接続詞": "接続詞", "連体詞": "連体詞",
       "感動詞": "感動詞", "助動詞": "助動詞", "助詞": "助詞", "接頭辞": "接頭辞", "接尾辞": "接尾辞", "代名詞": "代名詞"}


# 升の左の番号ごとの品詞の大分類 (名詞・動詞 など): その番号の語の Mozc の品詞でいちばん多いもの (6 割以上のときだけ)
cat_count = collections.defaultdict(collections.Counter)
for (r, lc, rc), d in multi.items():
    for w in d:
        for c in {n.split(",")[0] for n in mozc_pos.get((r, w), ())}:
            cat_count[lc][c] += 1
cell_cat = {}
for lc, c in cat_count.items():
    top, k = c.most_common(1)[0]
    if k >= 0.6 * sum(c.values()):
        cell_cat[lc] = top


def pos_class(reading, forms, lc, kana_same):
    """kana_same: かなだけの形が Sudachi で組の漢字の形と同じ語のとき True (かなの形の品詞も使う)"""
    use = [w for w in forms if KANJI.search(w) or kana_same]
    names = set().union(*(mozc_pos.get((reading, w), set()) for w in use))
    if lc in cell_cat and any(n.startswith(cell_cat[lc]) for n in names):
        names = {n for n in names if n.startswith(cell_cat[lc])}
    for key, cls in ORDER:
        if any(n.startswith(key) for n in names):
            return cls, "Mozc"
    for w in use:
        p = sud[w][1]
        if p:
            return SUD.get(p[0][0], "名詞"), "Sudachi"
    return "名詞", "なし"


# --- 組を作る ---
groups = []
stats = collections.Counter()
for (r, lc, rc), d in sorted(multi.items()):
    words = sorted(d)
    kana = [w for w in words if w == r]
    kanji = [w for w in words if KANJI.search(w)]
    ent = {w: jm_entries(w, r) for w in kanji}
    # 漢字を含む形どうし: 骨格が同じ (送り仮名・々) か、片方の骨格がもう片方に含まれる (交ぜ書き) で、同じ語と確かめられたもの
    parent = {w: w for w in kanji}
    def find(x):
        while parent[x] != x:
            parent[x] = parent[parent[x]]; x = parent[x]
        return x
    rel = {}
    for i, a in enumerate(kanji):
        for b in kanji[i + 1:]:
            sa, sb = skeleton(a), skeleton(b)
            shared = bool(ent[a] & ent[b])
            if sa == sb:
                kind = "々" if expand_odoriji(a) == expand_odoriji(b) else "送り仮名"
                why = "JMdict" if shared else ("Sudachi" if sudachi_same(a, b) else "骨格")
            elif is_subseq(sa, sb) or is_subseq(sb, sa):
                kind = "交ぜ書き"
                why = "JMdict" if shared else ("Sudachi" if sudachi_same(a, b) else None)
            else:
                stats["漢字の違う形の対 (入れない)"] += 1
                continue
            if why is None:
                stats["交ぜ書きの形で同じ語と確かめられない対 (入れない)"] += 1
                continue
            rel[(a, b)] = (kind, why)
            parent[find(a)] = find(b)
    comps = collections.defaultdict(list)
    for w in kanji:
        comps[find(w)].append(w)
    # つながった形のうち、漢字のいちばん多い骨格 (ほかの骨格に含まれない骨格) ごとに組を分ける。湯のみ を橋にして 湯呑み と 湯飲み (字の違い) が 1 つにならないように
    split = []
    for c in comps.values():
        sks = {skeleton(w) for w in c}
        tops = [m for m in sks if not any(m != o and is_subseq(m, o) for o in sks)]
        for m in sorted(tops):
            split.append(sorted(w for w in c if is_subseq(skeleton(w), m)))
        if len(tops) >= 2:
            stats["字の違う骨格で分けた組"] += 1
    comps = split
    # かなだけの形: 同じ語と確かめられた漢字の形の組に付ける
    attach = collections.defaultdict(list)
    if kana:
        h = kana[0]
        for ci, c in enumerate(comps):
            whys = ["JMdict" for w in c if ent[w]] + ["Sudachi" for w in c if sudachi_same(w, h)]
            if whys:
                attach[ci] = sorted(set(whys))
    for ci, c in enumerate(comps):
        forms = sorted(c)
        kinds = sorted({rel[(a, b)][0] for (a, b) in rel if a in c and b in c})
        whys = {rel[(a, b)][1] for (a, b) in rel if a in c and b in c}
        if ci in attach:
            forms = [kana[0]] + forms
            kinds = ["かな"] + kinds
            whys |= set(attach[ci])
        if len(forms) < 2:
            continue
        kanji_norms = {sud[w][0][0] for w in forms if KANJI.search(w)}
        kana_same = bool(kana) and kana[0] in forms and sud[kana[0]][0][0] in kanji_norms
        cls, cls_src = pos_class(r, forms, lc, kana_same)
        if "かな" in kinds:
            kind_label = cls if cls in ("形式名詞", "補助動詞", "補助形容詞", "接続詞", "副詞", "連体詞", "感動詞", "助動詞", "助詞", "接頭辞", "接尾辞", "代名詞") else cls + "のかな書き"
        elif "交ぜ書き" in kinds:
            kind_label = cls + "の交ぜ書き"
        elif kinds == ["々"]:
            kind_label = "々の書き方"
        else:
            kind_label = cls + "の送り仮名"
        marks = []
        if cls in ("形式名詞", "補助動詞", "補助形容詞"):
            marks.append("使い分け: " + cls + "はかな・実質の意味は漢字")
        if "かな" in kinds and kana_same and len(attach) >= 2:
            marks.append("かなの形は Sudachi ではこの語")
        if "かな" in kinds and len(attach) >= 2:
            others = [sorted(comps[cj])[0] for cj in attach if cj != ci]
            marks.append("かなは別の語と共有 (" + "・".join(others[:5]) + ("…" if len(others) > 5 else "") + ")")
        t7 = [w for w in forms if w in tsusoku7]
        if t7 and any(skeleton(w) == skeleton(t7[0]) and w != t7[0] for w in forms):
            marks.append(("通則7: 複合語では送らない (" if tsusoku7[t7[0]] else "通則7: 慣用で送らない名詞 (") + t7[0] + ")")
        rare = [f"{w}={form_info[(w, r)]}" for w in forms if (w, r) in form_info]
        if rare:
            marks.append("JMdict の形の印: " + "・".join(rare))
        ov = sorted({s for w in forms for s in existing.get(w, ())})
        if ov:
            marks.append("今ある表にもある: " + "・".join(ov))
        seqs = sorted(set().union(*(ent.get(w, set()) for w in forms)), key=int)
        uk = any(s in entry_uk for s in seqs)
        groups.append(dict(reading=r, lc=lc, rc=rc, forms=forms, values=[d[w][0] for w in forms], src=sorted({d[w][1] for w in forms}),
                           yure=kinds, kind=kind_label, cls_src=cls_src, marks=marks, why=sorted(whys), jm=seqs[:5], uk=uk,
                           norm=sorted({"+".join(sud[w][0]) for w in forms if KANJI.search(w)})))
        stats["組"] += 1
        stats["種類 " + kind_label] += 1

with open(os.path.join(out, "groups.tsv"), "w", encoding="utf-8") as f:
    f.write("# 2026-11-02 表記ゆれの組 (段 1: build_groups.py)。読み<TAB>左<TAB>右<TAB>形 (| 区切り。辞書の値の順でなく字の順)<TAB>辞書の値 (形の順)<TAB>ゆれ<TAB>種類<TAB>印<TAB>同じ語の確かめ<TAB>JMdict の項目<TAB>JMdict のかな書きの印 (uk)<TAB>Sudachi の正規化表記<TAB>辞書の出どころ<TAB>種類の決め方\n")
    for g in groups:
        f.write("\t".join([g["reading"], str(g["lc"]), str(g["rc"]), "|".join(g["forms"]), "|".join(f"{v:.3f}" for v in g["values"]),
                           "・".join(g["yure"]), g["kind"], "；".join(g["marks"]), "・".join(g["why"]), ",".join(g["jm"]),
                           "uk" if g["uk"] else "", "|".join(g["norm"]), "・".join(g["src"]), g["cls_src"]]) + "\n")
with open(os.path.join(out, "targets.tsv"), "w", encoding="utf-8") as f:
    for r, w in sorted({(g["reading"], w) for g in groups for w in g["forms"]}):
        f.write(f"{r}\t{w}\n")
for k, v in sorted(stats.items(), key=lambda x: -x[1]):
    note(k, v)
