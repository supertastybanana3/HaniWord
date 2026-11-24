"""2026-11-07 C: 仕事 44 (と 43 の (A))。常に効く表に入れる固有名詞の候補を 1 つの表にまとめる: 人 (人物・グループ・1 語の名前、rank.tsv)・
上場企業 (companies/company_table.tsv の候補)・閲覧数の上位の記事のうち (A) の作品・団体・出来事・地名 (classify_ab.py。人は rank.tsv の側で見る)。
数: 全文の目安の数 = 全文で名前の出た回数 (counts.txt) × 語の切れ目にそろう割合 (name_boundary.py、1 つの束で 30 文まで。抜いた文が 5 未満なら割合を使わず「?」)、
    閲覧数の上位 1,000 に入った月の数と直近 12 か月の月の数。
印 (線の外に置く理由。数は付けておく): 英字だけ (読みが無い)・1 字・表記がふつうの語と同じ (azooKey の辞書に同じ表記がある。全文の数は当てにならない)
使い方: python3 -I a_candidates.py <rank.tsv> <company_table.tsv> <ab.tsv> <titled.tsv> <counts.txt> <boundary.tsv> <top_articles.tsv> <azookey_dict.tsv> <out.tsv> [追加辞書 ...]
        [--horses horses.tsv --horse-counts horse_counts.txt --horse-boundary horse_boundary.tsv]"""
import re, sys
from collections import defaultdict
rank_p, comp_p, ab_p, titled_p, counts_p, bd_p, top_p, dic_p, out_p = sys.argv[1:10]
extra = [a for a in sys.argv[10:] if not a.startswith("--")]
def opt(n): return sys.argv[sys.argv.index(n) + 1] if n in sys.argv else None
horses_p, hcounts_p, hbd_p = opt("--horses"), opt("--horse-counts"), opt("--horse-boundary")
extra = [a for a in extra if a not in (horses_p, hcounts_p, hbd_p)]
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
def rows(p): return [l.rstrip("\n").split("\t") for l in open(p, encoding="utf-8") if not l.startswith("#") and l.strip()]
cnt = {}
for l in open(counts_p, encoding="utf-8"):
    n, _, name = l.rstrip("\n").partition(" "); cnt[name] = int(n)
bd = {r[0]: (int(r[2]), float(r[4])) for r in rows(bd_p)}
top = {r[0]: (int(r[1]), int(r[6])) for r in rows(top_p)}
if hcounts_p:
    for l in open(hcounts_p, encoding="utf-8"):
        n, _, name = l.rstrip("\n").partition(" "); cnt.setdefault(name, int(n))
if hbd_p:
    for r in rows(hbd_p): bd.setdefault(r[0], (int(r[2]), float(r[4])))
pairs, surfaces = set(), set()
for l in open(dic_p, encoding="utf-8"):
    c = l.rstrip("\n").split("\t"); pairs.add((c[0], c[1])); surfaces.add(c[1])
for p in extra:
    for c in rows(p):
        if len(c) > 1: pairs.add((kata(c[1]), c[0])); surfaces.add(c[0])
def est(name):
    raw = cnt.get(name, 0); s, ratio = bd.get(name, (0, 0.0))
    return (str(round(raw * ratio)) if s >= 5 else f"{raw}?"), raw
def flags(name, y):
    f = []
    if re.fullmatch(r"[A-Za-z0-9 .,&'・\-!?]+", name): f.append("英字だけ")
    if len(name) <= 1: f.append("1 字")
    if name in surfaces: f.append("表記がふつうの語と同じ")
    if re.search(r"[「」『』“”\"！!？?【】〈〉《》]", name): f.append("語ではない (記号を含む題)")  # 分野によらない決まり (まとめ役 12:53)。・ は名前に多いので外さない
    return "・".join(f)
# 人の記事の書き出しの形 (「よみ、1941年…」) に合っても人でない記事 (法令・戦い・かつての施設・競走馬) がある。は、のあとの説明に人の役の語が無いか、
# 人でないことを示す語があれば「人かどうか確かめられない」の印を付けて線の外に置く
ROLE = re.compile(r"(者|家|手|員|人|士|優|師|官|長|王|妃|皇|帝|将|侍|僧|主|子|女|男|スト|ター|ナー|ラー|ゲーマー|タレント|アイドル|モデル|歌手|俳優|女優|声優|選手|芸人|作家|監督|騎手|棋士|力士|レスラー|ボクサー|YouTuber|VTuber|アーティスト|ミュージシャン|シンガー|ダンサー|コメディアン|キャスター|アナウンサー|グループ|ユニット|バンド|コンビ|トリオ|デュオ|クリエイター|プロデューサー|ディレクター|デザイナー|パーソナリティ|DJ|ラッパー|大名|武将|天皇|将軍|公家|貴族|皇族|王族|宰相|カリフ|シャー)")
# 名前でない記事の題 (分野によらない決まり): 法令 (〜法・〜令・〜規則・〜条約・〜条例)・一覧・年や日付
NOT_NAME = re.compile(r"((法|令|規則|規程|条約|条例|憲章|協定)$|一覧|^\d{1,4}年|^\d{1,2}月\d{1,2}日|^(明治|大正|昭和|平成|令和)\d+年)")
NOT_PERSON = re.compile(r"(かつて存在した|に存在した国|法律|法令|勅令|政令|省令|条約|規則|戦い|戦争|競走馬|種牡馬|繁殖牝馬|映画館|撮影所|出版社|演芸場|劇場|メーカー|である法)")
out = []
for r in rows(rank_p):
    name, y, kind, title = r[2], r[3], r[5], r[8]
    e, raw = est(name); tm, tr = top.get(title, (0, 0))
    fl = flags(name, y)
    d = r[6][:40]
    # 2026-11-07 12:1x (2026-11-07 12:08 の決め): 分野ごとの決まりは作らず、どの記事にも同じ物差しを当てる。人の役の語が無い記事は、
    # 名前でない記事の題 (法令・一覧・年や日付) だけを外し、競走馬は「競走馬」、ほかは「その他の固有名詞」として同じ線と同じ確かめに乗せる
    if not ROLE.search(d) or NOT_PERSON.search(d):
        if NOT_NAME.search(name) or NOT_NAME.search(title) or re.search(r"(法律|法令|勅令|政令|省令|条約|規則)", d): fl = (fl + "・" if fl else "") + "名前でない記事の題"
        elif re.search(r"(競走馬|種牡馬|繁殖牝馬)", d): kind = "競走馬"
        else: kind = "その他の固有名詞"
    out.append((kind, name, y, e, raw, tm, tr, fl, title, r[6][:30]))
seen_titles = {o[8] for o in out}
# 競走馬の記事 (horses/horses.tsv、書き出しの説明に 競走馬・種牡馬・繁殖牝馬): 人の記事の形で拾えなかった馬も同じ山に入れる。読みはかなの名前のかな
if horses_p:
    for h in rows(horses_p):
        aid, title, name = h[0], h[1], h[2]
        if title in seen_titles or not re.fullmatch(r"[ぁ-ゖァ-ヺー]+", name): continue
        y = hira(name); e, raw = est(name); tm, tr = top.get(title, (0, 0))
        out.append(("競走馬", name, y, e, raw, tm, tr, flags(name, y), title, h[4][:30]))
for c in rows(comp_p):
    if c[10]: continue
    e, raw = est(c[1]); tm, tr = top.get(c[0], (0, 0))
    out.append(("会社", c[1], c[4], e, raw, tm, tr, flags(c[1], c[4]), c[0], (c[2] + " " if c[2] else "") + "銘柄 " + c[12]))
intro = {r[1]: r for r in rows(titled_p)}
# 書き出しが「『VIVANT』（ヴィヴァン）は2023年…」のように、括弧のあとが「は、」でない記事は extract_names.py の形に合わず名前と括弧が空になる。
# そのときは残した書き出し (は、のあとの欄に 160 字) を、『』「」を許し「は」のあとの読点を問わない形で読み直す (2026-11-07、物差しの試し問題 VIVANT)
for _t, _r in intro.items():
    if not _r[2] and len(_r) > 4:
        _m = re.match(r"^[「『]?([^（()「」『』\n]{1,60}?)[」』]?（([^）]{0,120})）(?:[^は\n]{0,20})?は", _r[4])
        if _m: _r[2], _r[3] = _m.group(1).strip(), _m.group(2)
for r in rows(ab_p):
    t, k = r[0], r[1]
    if not k.startswith("A ") or k == "A 人物": continue
    if re.search(r"(の登場人物|のエピソード|の楽曲|シリーズの作品)", t): continue  # 一覧のような記事
    it = intro.get(t)
    name = re.sub(r"\s*\([^)]*\)$", "", t).strip()
    y = ""
    if it and it[3] and it[2]:  # 括弧の読みは書き出しの名前の読みなので、表記もその名前に
        first = re.split(r"[、,，]", it[3])[0].strip().replace("・", "").replace(" ", "").replace("　", "")
        if re.fullmatch(r"[ぁ-ゖァ-ヺー]+", first):
            # 表記は書き出しの名前。日本語の名前の空白 (姓 名) は除き、英字どうしの間の空白 (ONE OK ROCK) は残す。英字の記事名が書き出しの名前の頭と同じなら記事名 (Google LLC → Google)
            nm = re.sub(r"[『』「」]", "", it[2]).replace("　", " ").strip()
            nm = re.sub(r"(?<=[^A-Za-z0-9.&'!?\-]) | (?=[^A-Za-z0-9.&'!?\-])", "", nm)
            if re.fullmatch(r"[A-Za-z0-9 .,&'!?\-]+", name) and nm.replace(" ", "").startswith(name.replace(" ", "")): nm = name
            y, name = hira(first), nm
    if re.fullmatch(r"[ぁ-ゖァ-ヺー]+", name): y = hira(name)  # かなだけの表記は表記のかなを読みに (括弧の読みが外国の発音のとき: ペレストロイカ（ぴりすとろーいか）)
    if not y or (kata(y), name) in pairs: continue
    e, raw = est(name)
    fl = flags(name, y).replace("英字だけ", "") .strip("・")
    out.append((k.replace("A ", ""), name, y, e, raw, int(r[3]), int(r[4]), fl, t, r[2][:30]))
with open(out_p, "w", encoding="utf-8") as f:
    f.write("# 種類\t表記\t読み\t全文の目安の数\t全文の数 (そのまま)\t上位の月の数\t直近 12 か月の上位の月\t印\t記事\tメモ\n")
    for o in out: f.write("\t".join(map(str, o)) + "\n")
print(len(out))
