"""2026-11-07 C: 仕事 43。決まった線 (proposal_all.py の「入れる」) の語と頼まれた語から、辞書に入れる語の表を作る (2026-11-07 11:38 の決め)。
- 読み: 記事の冒頭の括弧に並ぶかなの読みを全部 (嫌儲 けんもう・けんちょ・いやもう・いやんもう)。括弧の読みが無ければ前に決めた読み
- かなの形: かなだけの語は、題の形を 1 番目、括弧のもう一方の形 (ひらがな・カタカナ。同じ読み) を 2 番目 (ぬるぽ・ヌルポ)
- 閲覧数の上位の (B) は、カテゴリが インターネット・ミーム／流行語／ネットスラング／俗語 のものだけ。ほかは控えの表 (常に効く表の抜けの候補) へ
出力: <words.tsv> (表記<TAB>読み (| 区切り)<TAB>2 番目の形 (| 区切り)<TAB>出どころ<TAB>出典の記事と版)、<held.tsv> (控え)
使い方: python3 -I prepare_words.py <proposal_all.tsv> <netslang table.tsv> <ryukogo classified.tsv> <ryukogo articles.jsonl> <ryukogo meta (article.meta.json)>
        <toppv b_candidates.tsv> <titled.tsv> <requested.tsv> <out words.tsv> <out held.tsv> [読みの無い受賞語で足すもの]"""
import json, re, sys
(prop_p, ns_p, ry_p, ry_art, ry_meta, bc_p, titled_p, req_p, out_p, held_p) = sys.argv[1:11]
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
def rows(p): return [l.rstrip("\n").split("\t") for l in open(p, encoding="utf-8") if not l.startswith("#") and l.strip()]
KANA = re.compile(r"[ぁ-ゖァ-ヺー]+")
def paren_readings(word, text):
    """冒頭の「語（…）」の括弧の中の、かなだけの読みを全部 (等・通称 などの前まで)"""
    m = re.search(r"[「『]?" + re.escape(word) + r"[」』]?\s*[（(]([^）)]{1,80})[）)]", (text or "")[:len(word) + 40])
    if not m: return []
    out = []
    for seg in re.split(r"[、,，/／]", m.group(1)):
        seg = re.sub(r"(等|など|とも|ほか)$", "", seg.strip()).replace("・", "").replace(" ", "")
        if KANA.fullmatch(seg): out.append(seg)
        elif seg: break
    return out
ns = {r[0]: r for r in rows(ns_p)}
ry_arts = {json.loads(l)["word"]: json.loads(l) for l in open(ry_art, encoding="utf-8")}
ry_rev = json.load(open(ry_meta, encoding="utf-8"))
ry = {r[0]: r for r in rows(ry_p)}
bc = {r[1]: r for r in rows(bc_p)}
titled = {r[1]: r for r in rows(titled_p)}
kept_rev = json.load(open(__import__("os").path.join(__import__("os").path.dirname(bc_p), "kept_revisions.json"), encoding="utf-8")) if __import__("os").path.exists(__import__("os").path.join(__import__("os").path.dirname(bc_p), "kept_revisions.json")) else {}
SLANG_CAT = re.compile(r"(ミーム|流行語|ネットスラング|俗語)")
words, held = [], []
for r in rows(prop_p):
    if r[0] != "入れる": continue
    src, w, y0 = r[2], r[3], r[4]
    if src == "ネットミーム":
        text = ns.get(w, [""] * 4)[2]; oid = ns.get(w, [""] * 4)[3]; cite = f"Wikipedia「{w}」(版 {oid}) の見出し"
    elif src == "流行語大賞":
        a = ry_arts.get(w, {}); text = a.get("intro", "")
        cite = f"Wikipedia「新語・流行語大賞」(版 {ry_rev['revid']}) の受賞語" + (f"・「{a['title']}」(版 {a.get('revid')})" if a.get("exists") and not a.get("redirect_to") else "")
    else:
        b = bc.get(w)
        if not b or not SLANG_CAT.search(b[4]): held.append((w, y0, b[4] if b else "", "閲覧数の上位 (カテゴリがスラングでない)")); continue
        it = titled.get(b[0]); text = (it[2] + "（" + it[3] + "）") if it else ""
        rv = kept_rev.get(b[0], ("?", ""))
        cite = f"Wikipedia「{b[0]}」(版 {rv[0]}) の見出し (閲覧数の上位の記事)"
    ys = [hira(y) for y in paren_readings(w, text)] or [y0]
    ys = list(dict.fromkeys(y for y in ys if y))
    seconds = []
    if KANA.fullmatch(w):
        y = hira(w); ys = [y] + [x for x in ys if x != y]  # かなだけの語は表記のかなが 1 番目の読み
        for f in paren_readings(w, text):
            if f != w and hira(f) == y: seconds.append(f)  # 括弧のもう一方の形 (ヌルポ)
    if any(x[0] == w for x in words): continue  # 2 つの出どころにある語 (きつねダンス: 流行語大賞と閲覧数の上位) は先のほうだけ
    words.append((w, "|".join(ys), "|".join(dict.fromkeys(seconds)), src, cite))
# 読みの無い受賞語で足すもの (まとめ役 22:52。表記・読み・出どころ・出典)
if len(sys.argv) > 11:
    for r in rows(sys.argv[11]):
        if not any(x[0] == r[0] for x in words): words.append((r[0], r[1], "", r[2], r[3]))
for r in rows(req_p):
    words.append((r[0], r[1], "", "頼まれた語", f"HaniWord の開発者が足した語 ({r[3][:10]})"))
with open(out_p, "w", encoding="utf-8") as f:
    f.write("# 表記\t読み (| 区切り、1 番目が主)\t2 番目の形 (| 区切り)\t出どころ\t出典\n")
    for x in words: f.write("\t".join(x) + "\n")
with open(held_p, "w", encoding="utf-8") as f:
    f.write("# 控え: 閲覧数の上位の記事で、カテゴリがスラングでないふつうの言葉 (常に効く表の抜けの候補。2026-11-07 11:38 の決め)\n# 語\t読み\tカテゴリ\t理由\n")
    for x in held: f.write("\t".join(x) + "\n")
print(len(words), "語", len(held), "控え", sum(1 for x in words if "|" in x[1]), "読みが複数", sum(1 for x in words if x[2]), "2 番目の形あり")
