"""2026-11-07 C: 仕事 43。ネットスラングの辞書 (slang_dictionaries/internet.tsv) を作る。
入力: 入れる語の表 (prepare_words.py の出力: 表記・読み (| 区切り、1 番目が主)・2 番目の形 (| 区切り)・出どころ・出典)・azooKey の辞書 (動詞の活用形の手本)
- 品詞: ふつうは名詞 (1285)。〜る で終わる動詞 (アサヒる) は、ググる の活用形 (語幹のあとの れ・ら・り・る・っ・ん・ろ・りゃ。語幹だけの形は除く) を写す。
  〜られる (ギュられる) は 〜ら (五段の未然形) だけを足し、られる・られた は変換器が組み立てる
- 読み: 表の読みを全部 (2026-11-07 11:38 の決め: 読みが複数ある語は全部入れる)。2 番目の形 (ヌルポ) は 1 番目の形より少し下
- 値: 読みが 4 字以上は VALUE_LONG、3 字までは VALUE_SHORT (よく打つ短い読みに混ざらないように)。動詞の形は、ググる の形と基本形の差を足す
使い方: python3 -I make_dict.py <words.tsv> <azookey_dict.tsv> <out.tsv>"""
import re, sys
words_p, dic_p, out_p = sys.argv[1:4]
VALUE_LONG, VALUE_SHORT, SECOND = -12.0, -14.0, 0.5
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
def rows(p): return [l.rstrip("\n").split("\t") for l in open(p, encoding="utf-8") if not l.startswith("#") and l.strip()]
# 2026-11-22: 入れない語の表 (この道具と同じ置き場の exclude.tsv。決まりは表の頭と README の「入れない語」)。この語と活用形は足さない
from pathlib import Path
EXCLUDE = {r[0] for r in rows(Path(__file__).with_name("exclude.tsv"))}
# ググる の活用形 (読み・表記・品詞)。語幹 ググ のあとのかなと品詞の番号を写す
tmpl = [(r[1][2:], int(r[2]), float(r[5])) for r in rows(dic_p)
        if r[0].startswith("ググ") and r[1].startswith("ググ") and r[2] == r[3] and 768 <= int(r[2]) <= 788 and len(r[1]) <= 4]
base = max(v for s, c, v in tmpl if s == "る")
out = []
def add(surface, reading, cid, value, src):
    out.append((surface, kata(reading), str(cid), "501", f"{value:.3f}", src))
for w, ys, seconds, _src, cite in rows(words_p):
    if w in EXCLUDE:
        continue
    for y in ys.split("|"):
        if not re.fullmatch(r"[ぁ-ゖー]+", y):
            sys.exit(f"bad reading: {w} {y}")
        v = VALUE_LONG if len(y) >= 4 else VALUE_SHORT
        if w.endswith("られる") and len(w) > 3:
            add(w[:-2], y[:-2], 780, v, cite + " (〜ら、五段の未然形)")
        elif w.endswith("る") and not re.fullmatch(r"[ぁ-ゖ]+", w):
            for sfx, cid, tv in tmpl:
                if not sfx: continue  # 語幹だけの形 (アサヒ) は足さない (朝日・アサヒ のようなふつうの語とぶつかる)
                add(w[:-1] + sfx, y[:-1] + sfx, cid, v + (tv - base), cite + f" (ググる の形 {cid})")
        else:
            add(w, y, 1285, v, cite)
        for f2 in (seconds.split("|") if seconds else []):
            add(f2, y, 1285, v - SECOND, cite + " (2 番目の形)")
seen = set(); uniq = []
for o in out:
    k = (o[0], o[1], o[2])
    if k not in seen: seen.add(k); uniq.append(o)
with open(out_p, "w", encoding="utf-8", newline="\n") as f:
    f.write("# 表記\t読み\t品詞の番号\t意味の番号\t値\t出典\n")
    for o in uniq: f.write("\t".join(o) + "\n")
print(len(rows(words_p)), "語", len(uniq), "行")
