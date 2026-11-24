"""2026-11-07 C: 仕事 43。まれの線の案を作る。語ごとに区分を付け、(a) の候補を「閲覧の順と目安の数の順のよいほう」で並べる。
区分: 辞書にある (azooKey の辞書に同じ読み・表記がある。補った読みが辞書の読みと同じものも) / 型 (○○ を含む。(b) で別に) /
      語ではない (句読点・! を含む・15 字以上・〜用語・辞典・数字の並び) / 読みが無い / (a) の候補
使い方: python3 -I make_proposal.py <sheet.tsv> <boundary.tsv> <yomi_fill.tsv> <caution_notes.txt> <out.tsv>"""
import re, sys
sheet_p, bd_p, yf_p, cn_p, out = sys.argv[1:6]
def rd(p): return [l.rstrip("\n").split("\t") for l in open(p, encoding="utf-8") if not l.startswith("#")]
sheet = rd(sheet_p); bd = {r[0]: r for r in rd(bd_p)}; yf = {r[0]: r for r in rd(yf_p)}
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
cn = rd(cn_p)
def caution(w):
    for c in cn:
        if c[1] == "含む" and c[2] in w: return c[0] + ":" + c[2]
        if c[1] == "読み" and c[2].split(":", 1)[1] == w: return c[0] + ":" + c[2]
    return ""
rows = []
for s in sheet:
    w, y, form, pv, dic = s[0], s[1], s[2], int(s[4]), s[6]
    src = "記事の括弧"
    if not y and w in yf and yf[w][1]: y, src = yf[w][1], yf[w][2] + " (DEFAULTSORT と合う)"
    if not y: src = ""
    b = bd[w]; n_s = int(b[3]); est = int(b[6]) if n_s >= 5 else int(b[1]); est_note = "" if n_s >= 5 else "全文の数のまま (抜いた文が 5 未満)"
    other = dic.split("|", 1)[1].split(",") if "|" in dic else []
    if dic and not dic.startswith("|"): kind = "辞書にある"
    elif (y and kata(y) in other) or (not y and other): kind = "辞書にある"  # 読みが無くても、辞書に同じ表記があれば (電凸 デントツ)
    elif "○○" in w: kind = "型"
    elif re.search(r"[、。!！?？ 　]", w) or len(w) >= 15 or re.search(r"(用語|辞典)$", w) or re.fullmatch(r"[\d\-]+", w): kind = "語ではない"
    elif not y: kind = "読みが無い"
    else: kind = "(a) の候補"
    rows.append(dict(w=w, y=y, src=src, form=form, pv=pv, est=est, est_note=est_note, dic=dic, kind=kind, caution=caution(w)))
cand = [r for r in rows if r["kind"] == "(a) の候補"]
for key in ("pv", "est"):
    for i, r in enumerate(sorted(cand, key=lambda r: -r[key])): r["rank_" + key] = i + 1
for r in cand: r["rank"] = min(r["rank_pv"], r["rank_est"])
cand.sort(key=lambda r: (r["rank"], -(r["pv"] + r["est"])))
with open(out, "w", encoding="utf-8") as f:
    f.write("# 順 (閲覧の順と目安の数の順のよいほう)\t語\t読み\t読みの出どころ\t形\t月の平均の閲覧\t閲覧の順\t目安の数 (手元の Wikipedia の全文、語の切れ目にそろう文)\t目安の数の順\t注意の表\t区分\tnote\n")
    for i, r in enumerate(cand):
        f.write("\t".join(map(str, [i + 1, r["w"], r["y"], r["src"], r["form"], r["pv"], r["rank_pv"], r["est"], r["rank_est"], r["caution"], r["kind"], r["est_note"]])) + "\n")
    for r in rows:
        if r["kind"] != "(a) の候補":
            f.write("\t".join(map(str, ["", r["w"], r["y"], r["src"], r["form"], r["pv"], "", r["est"], "", r["caution"], r["kind"], r["dic"]])) + "\n")
from collections import Counter
print(Counter(r["kind"] for r in rows))
