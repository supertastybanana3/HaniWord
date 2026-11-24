"""2026-11-07 C: 仕事 44 (競走馬、まとめ役 11:41)。GI・JpnI に出走した馬の名前を 4 つの型に分ける。
読み: 名前がかなだけならそのかな (ひらがなにする)。ほかは括弧の頭のかな (無ければ読みが無い)
(a) azooKey の辞書か追加辞書に同じ表記と読みがある → 足さない
(b) 読みが、辞書のほかの語 (句ではない 1 語) の読みとちょうど同じ (キセキ = 奇跡・軌跡) → 足すと 1 番目を替えるおそれ
(c) 名前が、辞書にある 2 つ以上の語の続きに分かれる (Sudachi の C 単位で 2 語以上、どの語も辞書の語で未知語でない。ライス＋シャワー) → 句として打った人の変換を替えるおそれ
(d) どれでもない (トウカイテイオー) → 入れてよい
出力 TSV: 名前<TAB>読み<TAB>型<TAB>型の根拠 (同じ読みの語・分かれ方)<TAB>GI 出走<TAB>GI 勝ち<TAB>JpnI 出走<TAB>JpnI 勝ち<TAB>勝った競走<TAB>題
使い方: python -I classify_names.py <horses.tsv> <records.tsv> <azookey_dict.tsv> <out.tsv> [追加辞書 ...]"""
import re, sys
from collections import defaultdict
from sudachipy import dictionary, tokenizer
hz_p, rc_p, dic_p, out_p = sys.argv[1:5]
extra = sys.argv[5:]
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
def rows(p): return [l.rstrip("\n").split("\t") for l in open(p, encoding="utf-8") if not l.startswith("#") and l.strip()]
pairs, by_r = set(), defaultdict(list)
for l in open(dic_p, encoding="utf-8"):
    c = l.rstrip("\n").split("\t"); pairs.add((c[0], c[1]))
    if float(c[5]) > -29.0: by_r[c[0]].append((float(c[5]), c[1]))
for p in extra:
    for c in rows(p):
        if len(c) > 1: pairs.add((kata(c[1]), c[0]))
rec = {r[0]: r for r in rows(rc_p)}
tok = dictionary.Dictionary(dict="full").create(); MODE = tokenizer.Tokenizer.SplitMode.C
with open(out_p, "w", encoding="utf-8") as f:
    f.write("# 名前\t読み\t型\t根拠\tGI 出走\tGI 勝ち\tJpnI 出走\tJpnI 勝ち\t勝った競走\t題\n")
    for h in rows(hz_p):
        title, name, inner = h[1], h[2], h[3]
        r = rec.get(title)
        if not r or (int(r[1]) == 0 and int(r[3]) == 0): continue
        if re.fullmatch(r"[ぁ-ゖァ-ヺー]+", name): y = hira(name)
        else:
            m = re.match(r"\s*([ぁ-ゖァ-ヺー・ ]+)[、,]", inner); y = hira(m.group(1).replace("・", "").replace(" ", "")) if m else ""
        R = kata(y)
        if not y: k, why = "読みが無い", ""
        elif (R, name) in pairs: k, why = "(a) 辞書にある", ""
        else:
            others = sorted((v, w) for v, w in by_r.get(R, []) if w != name)
            toks = list(tok.tokenize(name, MODE))
            if others: k, why = "(b) 読みがふつうの語と同じ", "・".join(w for _, w in sorted(others, reverse=True)[:3])
            elif len(toks) >= 2 and not any(t.is_oov() for t in toks) and all(len(t.surface()) >= 2 for t in toks):
                k, why = "(c) ふつうの語の続き", "＋".join(t.surface() for t in toks)
            else: k, why = "(d) 重ならない", ""
        f.write("\t".join([name, y, k, why, r[1], r[2], r[3], r[4], r[6], title]) + "\n")
