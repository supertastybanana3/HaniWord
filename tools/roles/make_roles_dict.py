"""2026-11-06 (仕事 39 の役割語): roles_src.tsv (開いた記事に書いてある役割語の一人称・二人称・語尾・言い回し) から、
いつも読む追加辞書の形 (表記 読み 品詞 意味 値 出典) の表を作る。
- 一人称・二人称は代名詞 (azooKey の 1306。わし・わたくし・吾輩・拙者 と同じ) の位置
- 語尾のまとまりは、同じ位置に付く標準語の語を手本に (です 460・ます 491・だろう 455・くれ 940・遊ばせ 625・ごきげんよう 3)。
  です・ます・わ・よ・の の組み合わせで出る語尾 (ですわ・ますわ・てよ・ですの・あるよ・なのだ・じゃ・じゃよ) は足さない
- 値: BASE と、読みのぶつかる普通の語より MARGIN 下の、低いほう (うち = 内・家、わし = 鷲 のように、読みが普通の語とぶつかる語は 1 番目を取らない)。
  普通の語は、一人称・二人称なら同じ位置に来る名詞 (品詞 1281〜1316) で値が COMMON 以上のもの、語尾なら値が COMMON_END 以上の語 (品詞を問わない)。
  表記が同じ語・同じ読みの役割語の書き分け・代名詞の別の書き方は数えない (名詞のカタカナ ソナタ・ワシ は別の語として数える)
使い方: python3 make_roles_dict.py <roles_src.tsv> <azookey_dict.tsv> <out.tsv>
"""
import sys

COMMON, COMMON_END, MARGIN, BASE = -12.5, -9.0, 0.5, -9.0
EXCLUDE = {"うち": "内・家 の意味が多い語。関西の表の側で決める",
           "ミー": "AI ありで 〜は行く を 〜ハイク にした (みーはいく → ミーハイク、2026-11-07 の人称の 54 文)"}
# 語尾の手本の品詞 (足すものだけ)。None は足さない (組み合わせで出る)
ENDING_CID = {
    "じゃ": None, "んじゃ": None, "じゃよ": None, "じゃのう": None,
    "ござります": 460, "ですわ": None, "ますわ": None, "てよ": None, "ですの": None, "ございますの": None,
    "あそばせ": 625, "あそばして": None, "ざます": 460, "ざあます": 460, "ざんす": 460, "おざんす": 460,
    "ありんす": 491, "でありんす": 460, "あるよ": None, "あるね": None, "あるか": None, "よろし": None,
    "なのだ": None, "だっちゃ": 460, "でやんす": 460, "やんす": 491, "だべ": 455, "するだ": None, "おくれ": 940,
    "ごきげんよう": None,
}


def kata(s):
    return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)


def main():
    src, dic, out = sys.argv[1:4]
    best = {}  # 読み → [(値, 表記, cid)]
    for line in open(dic, encoding="utf-8"):
        r, w, lc, rc, mid, v = line.rstrip("\n").split("\t")
        best.setdefault(r, []).append((float(v), w, int(lc)))
    rows, notes = [], []
    # 同じ読みの役割語の書き分け (麻呂・麿、吾輩・我輩) は、ぶつかる普通の語に数えない
    variants = {}
    for line in open(src, encoding="utf-8"):
        c = line.rstrip("\n").split("\t")
        if not line.startswith("#") and len(c) > 2:
            variants.setdefault(kata(c[1]), set()).add(c[0])
    for line in open(src, encoding="utf-8"):
        if line.startswith("#") or not line.strip():
            continue
        word, ruby, kind, role, articles, where = (line.rstrip("\n").split("\t") + [""] * 6)[:6]
        # 2026-11-07 (まとめ役 10:52): うち (西日本の女性の一人称) は 内・家 の意味が多い語なので、オフにできない役割語の表には入れない
        # (関西の表 (初期値オフ) の側で扱うかは別に決める)
        if word in EXCLUDE:
            notes.append(f"{word}: 外した ({EXCLUDE[word]})")
            continue
        r = kata(ruby)
        if kind in ("一人称", "二人称"):
            cid = 1306
        else:
            cid = ENDING_CID.get(word)
            if cid is None:
                notes.append(f"足さない (組み合わせで出る・もうある): {word}")
                continue
        same = {word, ruby} | variants.get(r, set())
        if cid == 1306:
            # 代名詞 (1306) の別の書き方 (其方・己・ワシ) は同じ語として数えない。名詞のカタカナ (ソナタ) は別の語として数える
            common = [(v, w) for v, w, c in best.get(r, []) if w not in same and 1281 <= c <= 1316 and c != 1306 and v >= COMMON]
        else:
            common = [(v, w) for v, w, c in best.get(r, []) if w not in same and v >= COMMON_END]
        v = min(BASE, max(common)[0] - MARGIN) if common else BASE
        v = min(-5.0, max(-30.0, v))
        mine = [x for x in best.get(r, []) if x[1] == word and x[2] == cid]
        if mine and mine[0][0] >= v:
            notes.append(f"もうある (同じ品詞で値 {mine[0][0]:.2f} ≧ {v:.2f}): {word}")
            continue
        src_txt = "|".join(f"Wikipedia:{a}" for a in articles.split("|") if a)
        why = f"普通の語 {max(common)[1]} ({max(common)[0]:.2f}) より下" if common else "読みのぶつかる普通の語なし"
        rows.append((word, r, cid, v, f"{src_txt} ({role}。{why})"))
    with open(out, "w", encoding="utf-8") as f:
        f.write("# 表記\t読み\t品詞\t意味\t値\t出典\n")
        for word, r, cid, v, s in sorted(rows, key=lambda x: (x[1], x[0])):
            f.write(f"{word}\t{r}\t{cid}\t501\t{v:.3f}\t{s}\n")
    print(len(rows), "rows")
    for n in notes:
        print(" ", n)


main()
