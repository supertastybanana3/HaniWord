"""2026-11-07 C: 仕事 42。JMdict の動詞・形容詞で常用外の字を含む表記 (嗾ける・揶揄う) について、azooKey の辞書にある同じ読みのかなの語
(けしかける) の活用形 (けしかけ・けしかけて の けしかけ・けしかけれ・けしかけよ …。表記がかなで読みと同じ、同じ活用の型、左右の品詞が同じ) を写して、
漢字の形 (嗾け・嗾けれ・嗾けよ) を作る。漢字の基本形が辞書に無ければ基本形も足す。
- 表記と読みの最後の 1 字が同じかなの語だけ (嗾ける / けしかける)。語幹 = 最後の 1 字を除いたもの (嗾け / けしかけ)
- 足さない語: 語義の印 (X・vulg・derog) がある・どの語義も古語か廃語・片仮名を含む表記 (ブチ撒ける)・
  同じ項目で前に並ぶ表記 (よく使う書き方) が辞書にある (纏わり付く があるときの 纏り付く)
- 写す形は、その活用の型の かなの動詞・形容詞の 3 割以上にある「語幹のあとのかな・形の品詞の番号」の組だけ (かむ の か に かかる の かかっ を付けない)。
  2 字までの読みの形 (坐し まし・糶よ せよ) は足さない
- 値: 写したかなの形の値 + (漢字の基本形 − かなの基本形。漢字の基本形が辞書に無ければ 0)・かなの形の値・同じ読み・同じ品詞の語
  (追加辞書の語も入れる) のいちばん低い値のうち低いものから、さらに MARGIN 低く (uk の印の語は MARGIN_UK)、-13 より上には置かない。
  どの形も同じ読み・同じ品詞のかなの形より下なので、辞書だけの変換の 1 番目は変わらず、今ある候補も押し下げない
出力: <out.tsv> (core_*.tsv と同じ列) と、語ごとの数 <words.tsv>
使い方: python3 -I rare_kanji_forms.py <JMdict_e.gz> <kanjidic2.xml.gz> <azookey_dict.tsv> <azk_pos.tsv> <out.tsv> <words.tsv> [除く表 ...]"""
import gzip, re, sys
from collections import defaultdict
jm, kd, dic, posf, out, words_out = sys.argv[1:7]
extra = sys.argv[7:]
MARGIN, MARGIN_UK = 0.5, 1.5
x = gzip.open(kd, "rt", encoding="utf-8").read()
joyo = {re.search(r"<literal>(.)</literal>", c).group(1) for c in re.findall(r"<character>(.*?)</character>", x, re.S)
        if (g := re.search(r"<grade>(\d+)</grade>", c)) and 1 <= int(g.group(1)) <= 8}
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
is_kanji = lambda c: "一" <= c <= "鿿" or "㐀" <= c <= "䶿" or c == "々"
label = {int(l.split("\t")[0]): l.rstrip("\n").split("\t")[2] for l in open(posf, encoding="utf-8")}
ctype = lambda lc: (label.get(lc, "").split(",") + [""] * 7)[4]
is_base = lambda lc: ("動詞" in label.get(lc, "") or "形容詞" in label.get(lc, "")) and "基本形" in label.get(lc, "")
by_reading = defaultdict(list); have = set()
for l in open(dic, encoding="utf-8"):
    r, w, lc, rc, mid, v = l.rstrip("\n").split("\t")
    by_reading[r].append((r, w, int(lc), int(rc), int(mid), float(v))); have.add((r, w))
core_low = defaultdict(list)  # (読み, 品詞の番号) → 追加辞書の値。今ある追加辞書の語 (core_forms の 纏わり付け -21.8) も押し下げないように
for p in extra:
    for l in open(p, encoding="utf-8"):
        c = l.rstrip("\n").split("\t")
        if len(c) > 1 and not c[0].startswith("#"):
            have.add((kata(c[1]), c[0]))
            if len(c) > 4 and c[2].isdigit(): core_low[(kata(c[1]), int(c[2]))].append(float(c[4]))
prefix_index = defaultdict(list)  # (読みの頭, 残りの字数) → 読み
for r in by_reading:
    for n in range(0, 3):
        if len(r) > n: prefix_index[(r[:len(r) - n], n)].append(r)
# JMdict の動詞・形容詞の (読み, 表記, uk)
CONJ = lambda p: (p.startswith("v") and p not in ("vs", "vi", "vt", "vs-c")) or p in ("adj-i", "adj-ix")
pairs = {}; skipped_earlier = []
xml = gzip.open(jm, "rt", encoding="utf-8").read()
for e in re.findall(r"<entry>(.*?)</entry>", xml, re.S):
    pos = set(re.findall(r"<pos>&([^;]+);</pos>", e))
    if not any(CONJ(p) for p in pos): continue
    uk = "&uk;" in e
    if any(f"&{f};" in e for f in ("X", "vulg", "derog")): continue  # きわどい・卑語・さげすむ の印の語は足さない
    senses = re.findall(r"<sense>(.*?)</sense>", e, re.S)
    if senses and all("&arch;" in x or "&obs;" in x for x in senses): continue  # どの語義も古語・廃語の語は足さない
    kebs = [re.search(r"<keb>([^<]+)</keb>", k).group(1) for k in re.findall(r"<k_ele>(.*?)</k_ele>", e, re.S)
            if not set(re.findall(r"<ke_inf>&([^;]+);</ke_inf>", k)) & {"sK", "oK", "iK"}]
    for r_ele in re.findall(r"<r_ele>(.*?)</r_ele>", e, re.S):
        if "<re_nokanji/>" in r_ele: continue
        reb = hira(re.search(r"<reb>([^<]+)</reb>", r_ele).group(1))
        restr = re.findall(r"<re_restr>([^<]+)</re_restr>", r_ele)
        for i, keb in enumerate(kebs):
            if restr and keb not in restr: continue
            # 同じ項目で前に並ぶ表記 (よく使う書き方) が辞書にあれば、後ろの表記は足さない (纏わり付く があるときの 纏り付く。AI が後ろの表記を 1 番目に選ぶことがある)
            if any((kata(reb), k0) in have for k0 in kebs[:i] if not restr or k0 in restr): skipped_earlier.append((keb, reb, kebs[:i])); continue
            if not any(is_kanji(c) and c not in joyo for c in keb): continue
            if any("ァ" <= c <= "ヺ" for c in keb): continue  # 片仮名を含む表記 (ブチ撒ける、rK) は足さない
            if len(keb) < 2 or keb[-1] != reb[-1] or not ("ぁ" <= keb[-1] <= "ゖ"): continue
            pairs[(reb, keb)] = pairs.get((reb, keb), False) or uk
# 活用の表: 基本形の品詞の番号ごとに、かなの動詞・形容詞 (語幹が 2 字以上) に付く (語幹のあとのかな, 形の品詞の番号) を数え、
# その品詞の語の 3 割以上にある組だけを正しい活用形とみなす (かむ の か + かっ (かかる の形) のような、別の語の形を拾わないように)
def kana_forms(kb):
    ks = hira(kb[0])[:-1]
    out = []
    for e in (e for n in range(0, 3) for r2 in prefix_index.get((kata(ks), n), []) for e in by_reading[r2]):
        sfx = e[1][len(ks):]
        if e[1] != hira(e[0]) or not e[1].startswith(ks) or ctype(e[2]) != ctype(kb[2]) or e[2] != e[3]: continue
        if not all("ぁ" <= c <= "ゖ" for c in sfx): continue
        out.append((sfx, e))
    return out
para_n = defaultdict(int); para_c = defaultdict(lambda: defaultdict(int))
for r, es in by_reading.items():
    for kb in es:
        if kb[1] == hira(r) and is_base(kb[2]) and kb[2] == kb[3] and len(r) >= 3:
            para_n[kb[2]] += 1
            for sfx, lc in {(sfx, e[2]) for sfx, e in kana_forms(kb)}: para_c[kb[2]][(sfx, lc)] += 1
allowed = {L: {k for k, n in c.items() if n >= 0.3 * para_n[L]} for L, c in para_c.items() if para_n[L] >= 5}
rows = {}; per_word = defaultdict(lambda: [0, 0, "", ""])
for (reb, keb), uk in sorted(pairs.items()):
    R = kata(reb)
    kana_bases = [e for e in by_reading[R] if e[1] == reb and is_base(e[2]) and e[2] == e[3]]
    if not kana_bases: per_word[keb][3] = "かなの基本形が辞書に無い"; continue
    margin = MARGIN_UK if uk else MARGIN
    for kb in kana_bases:
        kanji_base = [e for e in by_reading[R] if e[1] == keb and e[2] == kb[2]]
        offset = (max(e[5] for e in kanji_base) - kb[5]) if kanji_base else 0.0
        ws, ks = keb[:-1], reb[:-1]
        # 同じ語幹のかなの形 (読みが語幹 + 0〜2 字、表記が読みのひらがなと同じ、同じ活用の型、左右の品詞が同じ) のうち、活用の表にある組だけ
        ok = allowed.get(kb[2])
        if not ok: per_word[keb][3] = "活用の表が無い品詞"; continue
        for sfx, e in kana_forms(kb):
            if (sfx, e[2]) not in ok: continue
            nw = ws + sfx
            if (e[0], nw) in have: continue
            # 値: かなの形から写した値と、同じ読み・同じ品詞の語のいちばん低い値 (単漢字の -30 は除く) の低いほうより下 (今ある候補を押し下げない)
            low = min([x[5] for x in by_reading[e[0]] if x[2] == e[2] and x[3] == e[3] and x[5] > -29.0] + core_low[(e[0], e[2])] + [e[5]])
            value = round(min(min(e[5] + offset, e[5], low) - margin, -13.0), 3)  # ふつうの語 (-13 より上) より上には置かない
            if len(e[0]) <= 2: continue  # 2 字までの読みの形 (坐し まし・糶よ せよ) は、よく打つ読みに混ざるので足さない
            k = (nw, e[0], e[2])
            if k not in rows or value > rows[k][3]:
                rows[k] = (nw, e[0], e[2], value, e[4], keb, reb, uk)
        per_word[keb][2] = reb
for k, r in rows.items(): per_word[r[5]][0] += 1
with open(out, "w", encoding="utf-8") as f:
    f.write("# 表記\t読み\tcid\tmid\t値\t出典\n")
    for nw, r, cid, v, mid, keb, reb, uk in sorted(rows.values(), key=lambda x: (x[5], x[1])):
        f.write(f"{nw}\t{r}\t{cid}\t{mid}\t{v}\tJMdict ({keb}{'・uk' if uk else ''})・{reb} の活用形を写した形\n")
with open(words_out, "w", encoding="utf-8") as f:
    f.write("# 表記\t読み\t足す形の数\t足さない理由\n")
    for keb, (n, _, reb, why) in sorted(per_word.items()):
        f.write(f"{keb}\t{reb}\t{n}\t{why}\n")
print("語", len(pairs), "足す形", len(rows), "形を足す語", sum(1 for v in per_word.values() if v[0]))
print("前に並ぶ表記が辞書にあるので足さない語", len({(k, r) for k, r, _ in skipped_earlier}), [k for k, _, _ in skipped_earlier[:12]])
