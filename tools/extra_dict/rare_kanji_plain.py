"""2026-11-07 C: 仕事 42。JMdict にあって azooKey の辞書に同じ読みで無い常用外の字の表記 (rare_kanji_find.py の出力) に、品詞の番号と値を付ける。
活用しない語 (名詞・副詞・感動詞・接続詞など) だけ。動詞・形容詞は rare_kanji_forms.py。
- 足さない語: 追加辞書にある・語義の印 (X・vulg・derog・arch) がある・同じ項目で前に並ぶ表記 (よく使う書き方) が辞書にある
  (纏わり付く があるときの 纏り付く。AI が後ろの表記を 1 番目に選ぶことがある)・読みにかな以外の字 (・) がある
- 手本: azooKey の辞書の同じ読みの語のうち、JMdict の品詞に合う品詞の番号 (名詞,サ変接続 など。下の ACCEPT の順に探す) を持つ語。
  名詞,一般 は 1285 を先に。単漢字の一覧の語 (-30)、語の少ない品詞の番号 (から の 297 など 100 語未満) は手本にしない
- 値: 手本と同じ読み・同じ品詞の番号の語 (追加辞書の語も入れる) のうち、いちばん低い値より MARGIN 低く (uk の印の語は MARGIN_UK)、
  -13 より上には置かない。同じ読み・同じ品詞の番号で値の高い語が必ずあるので、辞書だけの変換の 1 番目は変わらず、今ある候補も押し下げない
- 手本の無い語 (同じ読みの語が辞書に無い・品詞の合う語が無い) は足さない (理由を残す)
出力: <out.tsv> (core_*.tsv と同じ列: 表記・読み (カタカナ)・cid・mid・値・出典) と <skip.tsv>
使い方: python3 -I rare_kanji_plain.py <missing.tsv> <azookey_dict.tsv> <azk_pos.tsv> <out.tsv> <skip.tsv> [除く表 (表記<TAB>読み) ...]"""
import sys
from collections import defaultdict
missing, dic, posf, out, skip = sys.argv[1:6]
extra = sys.argv[6:]
MARGIN, MARGIN_UK = 0.5, 1.5
label = {int(l.split("\t")[0]): l.rstrip("\n").split("\t")[2] for l in open(posf, encoding="utf-8")}
# 語の少ない品詞の番号 (から・まで の 297 など、10 語ほどの特別な番号) は手本にしない
pos_count = {int(l.split("\t")[0]): int(l.split("\t")[1]) for l in open(posf, encoding="utf-8")}
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
by_reading = defaultdict(list)
for l in open(dic, encoding="utf-8"):
    r, w, lc, rc, mid, v = l.rstrip("\n").split("\t")
    by_reading[r].append((w, int(lc), int(rc), int(mid), float(v)))
have = {(kata(c[1]), c[0]) for p in extra for c in (l.rstrip("\n").split("\t") for l in open(p, encoding="utf-8")) if len(c) > 1 and not c[0].startswith("#")}
core_low = defaultdict(list)  # (読み, 品詞の番号) → 追加辞書の値 (今ある追加辞書の語も押し下げない)
for p in extra:
    for l in open(p, encoding="utf-8"):
        c = l.rstrip("\n").split("\t")
        if len(c) > 4 and not c[0].startswith("#") and c[2].isdigit(): core_low[(kata(c[1]), int(c[2]))].append(float(c[4]))
# JMdict の品詞 → 受け入れる azooKey の品詞 (名前の頭)。前にあるものほど先に手本にする
NOUN = ["名詞,一般", "名詞,サ変接続", "名詞,形容動詞語幹", "名詞,副詞可能"]
ACCEPT = {
    "n": NOUN, "adj-no": NOUN, "n-adv": ["名詞,副詞可能"] + NOUN, "n-t": ["名詞,副詞可能"] + NOUN,
    "vs": ["名詞,サ変接続"], "adj-na": ["名詞,形容動詞語幹"], "adj-t": ["名詞,形容動詞語幹", "名詞,一般"],
    "adv": ["副詞"], "adv-to": ["副詞"], "int": ["感動詞"], "conj": ["接続詞"], "pn": ["名詞,代名詞"],
    "ctr": ["名詞,接尾,助数詞"], "n-suf": ["名詞,接尾"], "suf": ["名詞,接尾"], "pref": ["接頭詞"], "num": ["名詞,数"],
}
CONJ = lambda p: p.startswith("v") and p not in ("vs",) or p == "adj-i" or p == "adj-ix"
rows, skipped = [], []
for l in open(missing, encoding="utf-8"):
    if l.startswith("#"): continue
    yomi, surf, pos, uk, order, _top, flags, earlier = (l.rstrip("\n").split("\t") + ["", "", ""])[:8]
    ps = pos.split("|")
    r = kata(yomi)
    if (r, surf) in have: skipped.append((surf, yomi, pos, "追加辞書にある")); continue
    if not __import__("re").fullmatch(r"[ァ-ヺー]+", r): skipped.append((surf, yomi, pos, "読みにかな以外の字 (・ など)")); continue
    if earlier: skipped.append((surf, yomi, pos, f"同じ項目で前に並ぶ表記が辞書にある ({earlier})")); continue  # AI が後ろの表記を 1 番目に選ぶことがある (纏り付け)
    if flags: skipped.append((surf, yomi, pos, f"語義の印 {flags} (きわどい・卑語・さげすむ・どの語義も古語)")); continue
    if all(CONJ(p) or p == "exp" for p in ps):
        skipped.append((surf, yomi, pos, "動詞・形容詞・句 (rare_kanji_forms.py か、足さない)")); continue
    wants = []
    for p in ps:
        if p == "vs" and "名詞,サ変接続" not in wants: wants.insert(0, "名詞,サ変接続")
        for a in ACCEPT.get(p, []):
            if a not in wants: wants.append(a)
    cands = [c for c in by_reading.get(r, []) if c[4] > -29.0]  # 単漢字の一覧の語 (-30、櫪) は手本にしない
    tpl = None
    for a in wants:
        hit = [c for c in cands if c[1] == c[2] and label.get(c[1], "").startswith(a) and pos_count.get(c[1], 0) >= 100]
        if hit:
            # 名詞,一般 の名前の番号は 1285・1288・1289・1297 とあり、1289 には 米津玄師 などの名前も入っている。1285 があれば 1285 を手本に
            cid = 1285 if any(c[1] == 1285 for c in hit) else max(hit, key=lambda c: c[4])[1]
            same = [c for c in cands if c[1] == cid and c[2] == cid]
            low = min(same, key=lambda c: c[4])
            if core_low[(r, cid)] and min(core_low[(r, cid)]) < low[4]: low = ("追加辞書の語", cid, cid, low[3], min(core_low[(r, cid)]))
            tpl = (cid, low); break
    if not tpl:
        why = "同じ読みの語が辞書に無い" if not cands else "同じ読みに品詞の合う語が無い (" + ",".join(sorted({label.get(c[1], "?").split(",")[0] for c in cands})[:3]) + ")"
        skipped.append((surf, yomi, pos, why)); continue
    cid, low = tpl
    value = round(min(low[4] - (MARGIN_UK if uk == "1" else MARGIN), -13.0), 3)  # ふつうの語 (-13 より上) より上には置かない
    if value > -5.0: skipped.append((surf, yomi, pos, f"値が高すぎる (手本 {low[0]} {low[4]:.2f})")); continue
    rows.append((surf, r, str(cid), str(low[3]), str(value), f"JMdict ({pos}{'・uk' if uk == '1' else ''})・値は同じ読み・品詞の {low[0]} ({low[4]:.2f}) より下", label.get(cid, "")))
with open(out, "w", encoding="utf-8") as f:
    f.write("# 表記\t読み\tcid\tmid\t値\t出典\t品詞の名前 (確かめ用)\n")
    for x in rows: f.write("\t".join(x) + "\n")
with open(skip, "w", encoding="utf-8") as f:
    f.write("# 表記\t読み\t品詞\t足さない理由\n")
    for x in skipped: f.write("\t".join(x) + "\n")
from collections import Counter
print("足す", len(rows), "足さない", len(skipped))
for k, n in Counter(x[3].split(" (")[0] for x in skipped).most_common(): print(" ", n, k)
print(Counter(x[6].split(",")[0] + "," + x[6].split(",")[1] for x in rows).most_common(8))
