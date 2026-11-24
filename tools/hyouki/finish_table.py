"""2026-11-02: 段 1 の組 (groups.tsv) に、段 2 の回数 (count_forms.py の出力) を足して、表記ゆれの組の表を作る (段 3)。
回数は (読み, 形) ごと (品詞の番号では分けない)。漢字を含む形はその読みの回数を全部数える。
かなだけの形は、文の中の解析で同じ語 (正規化表記がその組の漢字の形と同じ) と分かったものだけ数える。
かなが自分自身を正規化表記にした回数 (Sudachi は こと を 事 と別の語にする) は、同じ升でいちばんよく出る漢字の形の組にだけ入れる。
2026-11-03: ただし、種類が形式名詞・補助動詞・副詞・接続詞・助詞・助動詞などの組だけ (名詞・動詞・形容詞のかな書きの組 の｜野・から｜殻 に、助詞の の・から を数えないため)。
2026-11-04: --dict <azookey_dict.tsv> --repo <リポジトリ> を渡すと、かなの形が同じ読みのほかの品詞の番号の辞書の行 (品詞の違う語・追加辞書の言い回し) にも
あれば「かなは別の語と共有」の印を付ける (A の頼み: および 1285 の名詞のかな書きの組は、接続詞 および 555・556 と同じかななのに印が無かった)。
括弧には、ほかの番号の組の漢字の形と番号を入れる (そこで組になっていなければ「かなだけ」)。
  python3 -I finish_table.py <groups.tsv> <出力の置き場> <回数.tsv> ... [--dict <azookey_dict.tsv> --repo <リポジトリ>]
出力: hyouki_groups.tsv (全部の組)、hyouki_groups_frequent.tsv (よく出る組: 2 つ以上の形が出て、合わせて 100 回以上)、stats.txt"""
import collections, os, sys

args = sys.argv[1:]
opt = {}
for k in ("--dict", "--repo"):
    if k in args:
        i = args.index(k); opt[k] = args[i + 1]; del args[i:i + 2]
groups_path, out, *count_paths = args
cnt = collections.defaultdict(collections.Counter)  # (読み, 形) -> {(コーパス, 正規化表記): 回数}
corpora = []
for p in count_paths:
    for l in open(p, encoding="utf-8"):
        r, w, norm, corpus, v, nd = l.rstrip("\n").split("\t")
        cnt[(r, w)][(corpus, norm)] += int(v)
        if corpus not in corpora:
            corpora.append(corpus)


def total(r, w, corpus, norms=None):
    return sum(v for (c, n), v in cnt[(r, w)].items() if c == corpus and (norms is None or n in norms))


rows = []
header = None
for l in open(groups_path, encoding="utf-8"):
    if l.startswith("#"):
        header = l; continue
    p = l.rstrip("\n").split("\t")
    rows.append(p)

# --- 同じ読みのほかの品詞の番号の行 (build_groups.py と同じ行の選び方: 人名・地名などの番号と、かな・漢字以外の字を含む表記は除く) ---
other_ids = collections.defaultdict(lambda: collections.defaultdict(set))  # 読み (ひらがな) -> かなの形 -> {(左, 右)}
if "--dict" in opt:
    import re
    hira_r = lambda s: "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" and c not in "ヵヶ" else c for c in s)
    KANA_ONLY = re.compile(r"^[ぁ-ゖゝゞー]+$")
    NAME_IDS = set(range(1289, 1294))
    def add_id(r, w, lc, rc):
        if KANA_ONLY.match(w) and lc not in NAME_IDS and rc not in NAME_IDS:
            other_ids[hira_r(r)][w].add((lc, rc))
    for l in open(opt["--dict"], encoding="utf-8"):
        p = l.rstrip("\n").split("\t")
        if len(p) == 6:
            add_id(p[0], p[1], int(p[2]), int(p[3]))
    for name in ("core_forms", "core_words", "core_phrases"):
        for l in open(os.path.join(opt["--repo"], "extra_dictionaries", name + ".tsv"), encoding="utf-8"):
            if l.startswith("#"):
                continue
            p = l.rstrip("\n").split("\t")
            if len(p) >= 5:
                add_id(p[1], p[0], int(p[2]), int(p[2]))
# 組の表で、(読み, 左, 右) ごとに、かなの形 -> その組の漢字の形
group_kanji = collections.defaultdict(lambda: collections.defaultdict(set))
for p in rows:
    fs = p[3].split("|")
    for k in fs:
        if all("ぁ" <= c <= "ゖ" or c in "ゝゞー" for c in k):
            group_kanji[(p[0], p[1], p[2])][k] |= {w for w in fs if w != k}

def shared_elsewhere(r, lc, rc, forms):
    """かなの形が、同じ読みのほかの番号の行にもあれば、その番号の組の漢字の形と番号 (無ければ空)"""
    parts = []
    for k in forms:
        if not all("ぁ" <= c <= "ゖ" or c in "ゝゞー" for c in k):
            continue
        for (l2, r2) in sorted(other_ids[r].get(k, ())):
            if (str(l2), str(r2)) == (lc, rc):
                continue
            kanji = sorted(group_kanji[(r, str(l2), str(r2))].get(k, ()))
            parts.append(("・".join(kanji) if kanji else "かなだけ") + f" {l2}" + ("" if l2 == r2 else f"/{r2}"))
    return parts

# 2026-11-04: 形式名詞で始まる言い回し (こともある|事もある・ところで|所で) は、種類を形式名詞にして使い分けの印を付ける (A の頼み: 種類の向きは
# 同じ種類の中で効くので、名詞のかな書きに入っていると、名詞を漢字で書く人の向きが こともある → 事もある にも効いた)。
# かなの形が 形式名詞＋残り (残りはひらがなで始まる)、ほかの形がどれも その漢字＋同じ残り のときだけ (物語 ものがたり は残りが違うので入らない)。
# 左の品詞の番号は、表で形式名詞の言い回しに付いている 1310 (こと・もの・わけ・ほう)・1312 (よう)・1313 (うち・ため・とき・ところ・たび) だけ
# (形容詞 事々しい ことごとしき・物狂おしい・動詞 時めく・副詞 程なく・接続詞 故に・名詞 方位 などを入れないため)
KEISHIKI_LC = {"1310", "1312", "1313"}
KEISHIKI = {"こと": ("事",), "もの": ("物",), "ところ": ("所", "処"), "とき": ("時",), "ため": ("為",), "はず": ("筈",), "わけ": ("訳",),
            "うち": ("内", "中"), "ほう": ("方",), "まま": ("儘", "侭"), "よう": ("様",), "とおり": ("通り",), "ほど": ("程",), "くせ": ("癖",),
            "ゆえ": ("故",), "たび": ("度",), "つもり": ("積もり", "積り")}
KEISHIKI_MARK = "使い分け: 形式名詞はかな・実質の意味は漢字"

def keishiki_phrase(forms):
    """形式名詞で始まる言い回しの組なら、その形式名詞 (無ければ None)"""
    kana = [w for w in forms if all("ぁ" <= c <= "ゖ" or c in "ゝゞー" for c in w)]
    if len(kana) != 1:
        return None
    k = kana[0]
    for head, kanjis in KEISHIKI.items():
        rest = k[len(head):]
        if k.startswith(head) and rest and "ぁ" <= rest[0] <= "ゖ":
            if all(w == k or any(w == kj + rest for kj in kanjis) for w in forms):
                return head
    return None

SELF_OK = {"形式名詞", "補助動詞", "補助形容詞", "副詞", "接続詞", "連体詞", "代名詞", "感動詞", "助詞", "助動詞", "接頭辞", "接尾辞"}
KANJI = lambda w: any(not ("ぁ" <= c <= "ゖ" or c in "ゝゞー") for c in w)
# 升ごとに、かなの形を持つ組のうちでいちばんよく出る漢字の形の組 (かなの自分自身の回数をそこへ入れる)
best_in_cell = {}
for i, p in enumerate(rows):
    r, lc, rc, forms = p[0], p[1], p[2], p[3].split("|")
    if forms[0] == r:
        k = sum(total(r, w, c) for w in forms if KANJI(w) for c in corpora)
        cell = (r, lc, rc)
        if cell not in best_in_cell or k > best_in_cell[cell][1]:
            best_in_cell[cell] = (i, k)

stats = collections.Counter(); kind_n = collections.Counter(); kind_freq = collections.Counter()
out_rows = []
for i, p in enumerate(rows):
    r, lc, rc, forms, values, yure, kind, marks, why, jm, uk, norm, src, cls_src = p
    forms = forms.split("|")
    head = keishiki_phrase(forms) if kind != "形式名詞" and lc in KEISHIKI_LC else None
    if head:
        kind = "形式名詞"
        if KEISHIKI_MARK not in marks:
            marks = KEISHIKI_MARK + ("；" + marks if marks else "")
        stats["形式名詞で始まる言い回しの組"] += 1
    norms = {n.split("+")[0] for n in norm.split("|")} if norm else set()  # 1 つ目の単位の正規化表記
    for w in forms:
        if KANJI(w):
            norms |= {n for (c, n) in cnt[(r, w)]}
    per = {}
    note = []
    for c in corpora:
        vals = []
        for w in forms:
            if KANJI(w):
                vals.append(total(r, w, c))
            else:
                ok = set(norms)
                if best_in_cell.get((r, lc, rc), (None,))[0] == i and kind in SELF_OK:
                    ok.add(w)
                vals.append(total(r, w, c, ok))
        per[c] = vals
    sums = [sum(per[c][j] for c in corpora) for j in range(len(forms))]
    top = forms[max(range(len(forms)), key=lambda j: sums[j])] if any(sums) else ""
    vals_f = [float(v) for v in values.split("|")]
    dict_top = forms[max(range(len(forms)), key=lambda j: vals_f[j])]
    freq = sum(1 for s in sums if s > 0) >= 2 and sum(sums) >= 100
    # 2026-11-03: かなが 1 字の名詞・動詞などの組 (い｜居・め｜目) は、かなの回数がほかの語 (している の い) とまざるので、よく出る組に入れない
    if any(len(w) == 1 and not KANJI(w) for w in forms) and kind not in SELF_OK:
        marks = (marks + "；" if marks else "") + "かなが 1 字 (ほかの語とまざる)"
        freq = False
        stats["かなが 1 字の組"] += 1
    # 2026-11-04: 同じ読みのほかの品詞の番号の行にも同じかなの形があれば、かなは別の語と共有
    elsewhere = shared_elsewhere(r, lc, rc, forms) if other_ids else []
    if elsewhere:
        label = "ほかの番号: " + "、".join(elsewhere[:6]) + ("…" if len(elsewhere) > 6 else "")
        if "かなは別の語と共有 (" in marks:
            marks = marks.replace("かなは別の語と共有 (", "かなは別の語と共有 (" + label + "；同じ番号: ", 1)
        else:
            marks = (marks + "；" if marks else "") + "かなは別の語と共有 (" + label + ")"
        stats["かなをほかの番号の行と共有する組"] += 1
    kind_n[kind] += 1
    if freq:
        kind_freq[kind] += 1
    out_rows.append((freq, [r, lc, rc, "|".join(forms), kind, yure, marks] + ["|".join(map(str, per[c])) for c in corpora]
                     + [top, dict_top, values, why, src, jm, uk]))
    stats["組"] += 1
    stats["よく出る組"] += freq

head = ("# 2026-11-02 表記ゆれの組 (同じ読み・同じ品詞の番号で、意味の同じ書き分け)。data_ext/hyouki/README.md を見る\n"
        "# 読み<TAB>左<TAB>右<TAB>形 (| 区切り)<TAB>種類<TAB>ゆれ<TAB>印<TAB>" + "<TAB>".join(f"{c} の回数 (形の順)" for c in corpora)
        + "<TAB>コーパスでいちばん多い形<TAB>今の辞書でいちばん上の形<TAB>辞書の値 (形の順)<TAB>同じ語の確かめ<TAB>辞書の出どころ<TAB>JMdict の項目<TAB>JMdict のかな書きの印\n")
for name, keep in (("hyouki_groups.tsv", lambda f: True), ("hyouki_groups_frequent.tsv", lambda f: f)):
    with open(os.path.join(out, name), "w", encoding="utf-8") as f:
        f.write(head)
        for fr, row in out_rows:
            if keep(fr):
                f.write("\t".join(row) + "\n")
with open(os.path.join(out, "stats.txt"), "w", encoding="utf-8") as f:
    for k, v in stats.items():
        f.write(f"{k}\t{v}\n")
    f.write("# 種類<TAB>組<TAB>よく出る組\n")
    for k, v in kind_n.most_common():
        f.write(f"{k}\t{v}\t{kind_freq[k]}\n")
print(dict(stats))
