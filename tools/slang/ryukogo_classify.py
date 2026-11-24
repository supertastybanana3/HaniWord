"""2026-11-07 C: 仕事 43 (流行語)。受賞語に区分と数を付ける (ネットミームと同じ決まり)。
区分: 辞書にある (azooKey の辞書か追加辞書に同じ表記と読み。読みが無ければ同じ表記) / 英字だけ / 語ではない (句読点・! を含む・15 字以上・空白を含む) /
      型 (○○ を含む) / 人名だけ (語の記事か行き先がカテゴリ「存命人物」「〜年生」「〜年没」の記事、か受賞者の名前と同じ) / 読みが無い / 候補
数: (2) 記事 (転送でない) の直近 12 か月の月の平均と、いちばん多かった月。記事の無い語・転送だけの語は (1) 手元の全文の目安の数 (語の切れ目にそろう割合を掛けた数)
使い方: python3 -I classify.py <awards.tsv> <words.tsv> <articles.jsonl> <categories.jsonl> <boundary.tsv> <azookey_dict.tsv> <out.tsv> [追加辞書 ...]"""
import json, re, sys
aw_p, wd_p, ar_p, cat_p, bd_p, dic_p, out_p = sys.argv[1:8]
extra = sys.argv[8:]
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
def rows(p): return [l.rstrip("\n").split("\t") for l in open(p, encoding="utf-8") if not l.startswith("#")]
years, who = {}, {}
for r in rows(aw_p):
    years.setdefault(r[2], []).append(f"{r[0]} {r[1]}"); who.setdefault(r[2], set()).update(x for x in r[5].split("|") if x)
arts = {json.loads(l)["word"]: json.loads(l) for l in open(ar_p, encoding="utf-8")}
cats = {json.loads(l)["title"]: json.loads(l)["categories"] for l in open(cat_p, encoding="utf-8")}
est = {r[0]: (int(r[6]), int(r[3])) for r in rows(bd_p)}
pairs, surfaces = set(), set()
for l in open(dic_p, encoding="utf-8"):
    c = l.rstrip("\n").split("\t"); pairs.add((c[0], c[1])); surfaces.add(c[1])
for p in extra:
    for c in rows(p):
        if len(c) > 1: pairs.add((kata(c[1]), c[0])); surfaces.add(c[0])
RULES = [("A 人物", r"^(存命人物|\d+年生|\d+年没)$"), ("A 作品", r"(テレビドラマ|テレビアニメ|アニメ映画|映画|漫画|小説|特撮|舞台作品|楽曲|シングル|アルバム|ゲーム|番組|ライトノベル|ミュージカル)"),
         ("A 団体", r"(音楽グループ|の企業$|の会社$|球団$|チーム$|団体$|政党$|ユニット$|バンド$|アイドルグループ|クラブ$)"), ("A 出来事", r"(事件|事故|災害|地震|台風|選挙|戦争|大会|オリンピック|ワールドカップ|紛争)"),
         ("A 地名と施設", r"(の市$|の町$|の村$|の区$|都道府県|の国$|駅$|空港|山$|川$|島$|城$|寺院|神社|道路|鉄道路線|施設|建築物)")]
def ab(t):
    for name, pat in RULES:
        h = [x for x in cats.get(t, []) if re.search(pat, x)]
        if h: return name + " (" + h[0] + ")"
    return "B"
def person(t): return any(c == "存命人物" or re.fullmatch(r"\d+年(生|没)", c) for c in cats.get(t, []))
with open(out_p, "w", encoding="utf-8") as f:
    f.write("# 語\t読み\t読みの出どころ\t記事\t直近 12 か月の月の平均\tいちばん多かった月\tその月の閲覧\t全文の目安の数 (記事が無いか転送のとき)\t区分\t受賞 (年 賞)\t版\t(A)/(B) (語の記事のカテゴリ)\n")
    for w, y, src, kind, title, revid, avg, peak, peakv in rows(wd_p):
        a = arts.get(w, {}); tgt = (a.get("redirect_to") or a.get("title") or "").split("#")[0]
        e = est.get(w, (0, 0))
        e_s = "" if kind == "記事" else (str(e[0]) if e[1] >= 5 else f"{e[0]}?")
        if (y and (kata(y), w) in pairs) or (not y and w in surfaces): k = "辞書にある"
        elif re.fullmatch(r"[A-Za-z0-9 .,&'!?\-#]+", w): k = "英字だけ"
        elif re.search(r"[、。!！?？ 　]", w) or len(w) >= 15: k = "語ではない"
        elif "○" in w: k = "型"
        elif tgt and person(tgt): k = "人名だけ"
        elif w in who.get(w, set()): k = "受賞者の名前と同じ"
        elif not y: k = "読みが無い"
        else: k = "候補"
        f.write("\t".join([w, y, src, kind, avg, peak, peakv, e_s, k, "・".join(years.get(w, [])), str(revid), ab(tgt) if tgt else "B"]) + "\n")
