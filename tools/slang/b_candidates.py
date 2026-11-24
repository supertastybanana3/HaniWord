"""2026-11-07 C: 仕事 43 (B)。閲覧数の上位の記事のうち「B ふつうの語」と「決まらない」の題から、ネットスラングの表の候補を作る。
読み: 書き出しの括弧の頭のかな (手元の全文、extract2.py の titled.tsv)。かなだけの題は表記のかな。英字の題は括弧の読みがあればその読みで英字の表記
外す: azooKey の辞書か追加辞書に同じ表記と読み・読みが無い・語ではない (句読点・空白・15 字以上)・人の記事 (カテゴリ)
使い方: python3 -I b_candidates.py <ab.tsv> <titled.tsv> <azookey_dict.tsv> <out.tsv> [追加辞書 ...]"""
import re, sys
ab_p, titled_p, dic_p, out_p = sys.argv[1:5]
extra = sys.argv[5:]
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
def rows(p): return [l.rstrip("\n").split("\t") for l in open(p, encoding="utf-8") if not l.startswith("#") and l.strip()]
intro = {r[1]: r for r in rows(titled_p)}
# 書き出しが「『VIVANT』（ヴィヴァン）は2023年…」のように、括弧のあとが「は、」でない記事は extract_names.py の形に合わず名前と括弧が空になる。
# そのときは残した書き出し (は、のあとの欄に 160 字) を、『』「」を許し「は」のあとの読点を問わない形で読み直す (2026-11-07、物差しの試し問題 VIVANT)
for _t, _r in intro.items():
    if not _r[2] and len(_r) > 4:
        _m = re.match(r"^[「『]?([^（()「」『』\n]{1,60}?)[」』]?（([^）]{0,120})）(?:[^は\n]{0,20})?は", _r[4])
        if _m: _r[2], _r[3] = _m.group(1).strip(), _m.group(2)
pairs = set()
for l in open(dic_p, encoding="utf-8"):
    c = l.rstrip("\n").split("\t"); pairs.add((c[0], c[1]))
for p in extra:
    for c in rows(p):
        if len(c) > 1: pairs.add((kata(c[1]), c[0]))
from collections import Counter
cnt = Counter()
with open(out_p, "w", encoding="utf-8") as f:
    f.write("# 題\t表記\t読み\t区分 (分け方)\t当たったカテゴリ\t上位の月の数\t直近 12 か月の上位の月\tいちばん多かった月\tその月の閲覧\t外す理由\n")
    for r in rows(ab_p):
        if r[1] not in ("B ふつうの語", "決まらない"): continue
        t = r[0]; name = re.sub(r"\s*\([^)]*\)$", "", t).strip()
        it = intro.get(t); y = ""
        # 括弧の読みは書き出しの名前 (括弧の前) の読みなので、表記もその名前にする (ソビエト連邦 の記事の ソビエト社会主義共和国連邦)
        if it and it[3] and it[2]:
            first = re.split(r"[、,，]", it[3])[0].strip().replace("・", "").replace(" ", "").replace("　", "")
            if re.fullmatch(r"[ぁ-ゖァ-ヺー]+", first): y, name = hira(first), re.sub(r"[ 　『』「」]", "", it[2])
        if re.fullmatch(r"[ぁ-ゖァ-ヺー]+", name): y = hira(name)  # かなだけの表記は表記のかなを読みに (括弧の読みが外国の発音のとき: ペレストロイカ（ぴりすとろーいか）)
        why = ""
        if re.search(r"[、。!！?？ 　]", name) or len(name) >= 15: why = "語ではない"
        elif not it: why = "手元の全文に記事が無い (読みが取れない)"
        elif not y: why = "読みが無い"
        elif (kata(y), name) in pairs: why = "辞書に同じ表記と読みがある"
        cnt[why or "候補"] += 1
        f.write("\t".join([t, name, y, r[1], r[2], r[3], r[4], r[5], r[6], why]) + "\n")
print(cnt)
