"""2026-11-07 C: 仕事 44 (上場企業)。会社ごとに表記・読み・数をまとめ、決まりで外すものに理由を付ける。
- 表記: 記事名 (括弧の注を除く) を短い形、書き出しの名前 (株式会社 などつき) を正式な形として両方残す
- 読み: 書き出しの括弧の頭のかな (株式会社 などを除いた名前の読み)。無ければ、短い形がかなだけ (ひらがな・カタカナ・長音) なら表記をひらがなにしたもの。
  英字・数字・記号・漢字が混じって括弧の読みが無いものは入れない (まとめ役 09:52)
- 外す: 英字だけの社名 (english_brands.txt の側)・読みが無い・azooKey の辞書か追加辞書に同じ表記と読みがある (決まり 2、機械で)
- 決まり 1 の目印: 同じ読みのふつうの語が辞書にあるか (その語と値)。値はその語より上に出ないように付け、1 番目を替えるなら外す (測りで決める)
- 数: 手元の全文で名前の出た回数 (人物と会社の数え counts.txt。短い形と正式な形の多いほう)・閲覧数の上位 1,000 に入った月の数と直近 12 か月の月の数
使い方: python3 -I company_table.py <titles.tsv> <titled.tsv> <counts.txt> <top_articles.tsv> <azookey_dict.tsv> <out.tsv> [追加辞書 ...]"""
import re, sys
from collections import defaultdict
titles_p, titled_p, counts_p, top_p, dic_p, out_p = sys.argv[1:7]
extra = sys.argv[7:]
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
KINDS = r"(株式会社|有限会社|合同会社|合資会社|合名会社)"
comp = {l.split("\t")[0]: l.rstrip("\n").split("\t") for l in open(titles_p, encoding="utf-8") if not l.startswith("#")}
intro = {}
for l in open(titled_p, encoding="utf-8"):
    if l.startswith("#"): continue
    c = l.rstrip("\n").split("\t")
    if c[1] in comp: intro[c[1]] = c
cnt = {}
for l in open(counts_p, encoding="utf-8"):
    n, _, name = l.rstrip("\n").partition(" "); cnt[name] = int(n)
top = {}
for l in open(top_p, encoding="utf-8"):
    if l.startswith("#"): continue
    c = l.rstrip("\n").split("\t"); top[c[0]] = (int(c[1]), int(c[6]))
pairs, by_reading = set(), defaultdict(list)
for l in open(dic_p, encoding="utf-8"):
    r, w, lc, rc, mid, v = l.rstrip("\n").split("\t")
    pairs.add((r, w)); by_reading[r].append((float(v), w))
for p in extra:
    for l in open(p, encoding="utf-8"):
        c = l.rstrip("\n").split("\t")
        if len(c) > 1 and not c[0].startswith("#"): pairs.add((kata(c[1]), c[0]))
with open(out_p, "w", encoding="utf-8") as f:
    f.write("# 記事名\t表記 (読みの付く形: 登記の名前か、かなだけの社名)\t記事名の短い形 (表記と違うとき)\t正式な形 (書き出し)\t読み\t読みの出どころ\t全文の数\t上位の月の数\t直近 12 か月の上位の月\t同じ読みのふつうの語 (値)\t外す理由\tWikidata\t銘柄コード\n")
    for t, w in comp.items():
        short = re.sub(r"\s*\([^)]*\)$", "", t).strip()
        it = intro.get(t)
        formal = it[2] if it else ""
        # 括弧の読みは、正式な形から 株式会社 などを除いた名前 (登記の名前) の読み。読みの頭か終わりに かぶしきがいしゃ などがあれば除く
        base = re.sub("^" + KINDS + "|" + KINDS + "$", "", formal).strip() if formal else ""
        y, src, surf = "", "", ""
        if it and it[3]:
            first = re.split(r"[、,，]", it[3])[0].strip().replace("・", "").replace(" ", "").replace("　", "")
            first = re.sub(r"^(かぶしきがいしゃ|カブシキガイシャ|ゆうげんがいしゃ|ごうどうがいしゃ)|(かぶしきがいしゃ|カブシキガイシャ|ゆうげんがいしゃ|ごうどうがいしゃ)$", "", first)
            if re.fullmatch(r"[ぁ-ゖァ-ヺー]+", first) and base: y, src, surf = hira(first), "書き出しの括弧", base
        if not y and re.fullmatch(r"[ぁ-ゖァ-ヺー]+", short): y, src, surf = hira(short), "かなだけの社名", short
        if not y and base and re.fullmatch(r"[ぁ-ゖァ-ヺー]+", base): y, src, surf = hira(base), "かなだけの社名 (登記の名前)", base
        n = max(cnt.get(short, 0), cnt.get(formal, 0), cnt.get(base, 0))
        tm, tr = top.get(t, (0, 0))
        same = sorted(by_reading.get(kata(y), []), reverse=True)[:1] if y else []
        common = f"{same[0][1]} ({same[0][0]:.2f})" if same and same[0][1] != surf else ""
        why = ""
        if re.fullmatch(r"[A-Za-z0-9 .,&'・\-]+", surf or short): why = "英字だけの社名"
        elif not it: why = "手元の全文に記事が無い (読みが取れない)"
        elif not y: why = "読みが無い (括弧の読みが無く、かなだけでもない)"
        elif (kata(y), surf) in pairs: why = "辞書に同じ表記と読みがある"
        short_note = short if short != surf else ""
        f.write("\t".join(map(str, [t, surf, short_note, formal, y, src, n, tm, tr, common, why, w[1], w[3]])) + "\n")
