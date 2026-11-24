"""2026-11-07 C: 仕事 44。常に効く固有名詞の表 (core_names) を作る (2026-11-07 11:38 の決め)。
入れるもの:
- 人・グループ・1 語の名前・作品・団体・出来事・地名と施設: 線 (閲覧数の上位 1,000 に入った月 1 以上 か 全文の目安の数 300 以上) を越えたもの。
  英字だけの名前は括弧の読みがあれば入れる (英字・数字だけの名前は上位の月だけで線を見る)。印「人かどうか確かめられない」「表記がふつうの語と同じ」「1 字」は入れない
- 上場企業: 読みの取れた候補を全部 (線によらない)
- 流行語大賞の受賞語のうち、語の記事のカテゴリが固有名詞 (A) のもの
品詞の番号は azooKey の辞書の同じ種類の語にならう: 人・グループ・競走馬 1289 (米津玄師・乃木坂46・オグリキャップ)、その他の固有名詞 1288、作品・出来事 1288 (鬼滅の刃・関ヶ原の戦い)、会社・団体 1292 (トヨタ自動車)、地名と施設 1293 (東京スカイツリー)
値: 読みが 5 字以上は -13.0、4 字までは -14.5。同じ読みの語が辞書にあれば、そのいちばん高い値 (単漢字の一覧の -30 は除く) より 0.5 下にする (決まり 1: ふつうの語より上に出ない)
外す: azooKey の辞書か追加辞書に同じ表記と読みがある (決まり 2)・読みが無い・読みにかな以外の字
2026-11-08 (3b の測りのあと、まとめ役 01:0x の決め):
- 表記に会社・法人の形の語 (株式会社・一般社団法人 など) があり、読みにその読みが無ければ、表記からその語を外す (株式会社新潮社 → 新潮社)
- かな・漢字と英字の混じる表記で、英字を除いた部分だけで読みどおりに作れる (辞書の語とかなの並びで) ものは外す (将軍SHŌGUN の読み ショウグン)
- 記号: 残すのは ・ ー 々 と、半角・全角の - . & ' / だけ。それ以外の記号・句読点 (、。！？・括弧・☆★♪→〜 など。Unicode の区分 S・P) を含む名前は入れない
  (まとめ役 11-08 05:15。前の「記号を含む題」の一覧 (「」『』“”！？【】〈〉《》) を広げたもの)
- 読みが 2 字までの名前は入れない (KANA・里歩 など。ふつうの語の並びの切れ目と読みが重なり、出力に名前が出なくても AI の選び方を変える。
  医学の文で 鈍り|ほかの が 里歩 (リホ) にまたがり 他の → ほかの になった。まとめ役 11-08 14:45)
- --drop <表>: 3b の測りで正しかった所を替えた名前 (names_culprits.py の出力の 崩れた・所が崩れた が 1 以上の語) を外す
出力: <out.tsv> (core_*.tsv と同じ列: 表記・読み・cid・mid・値・出典) と <excluded.tsv> (外したものと理由)
使い方: python3 -I make_names_table.py <a_candidates.tsv> <ryukogo classified.tsv> <azookey_dict.tsv> <out.tsv> <excluded.tsv> [追加辞書 ...] [--pv a_pv.tsv --pv-min 300] [--drop culprits.tsv]"""
import re, sys, unicodedata
from collections import defaultdict
ac_p, ry_p, dic_p, out_p, exc_p = sys.argv[1:6]
extra = [a for a in sys.argv[6:] if not a.startswith("--")]
def opt(n): return sys.argv[sys.argv.index(n) + 1] if n in sys.argv else None
# 2026-11-07 (まとめ役 12:24・12:53): 物差しを月の閲覧数の平均に替える。--pv <pv_line.py の出力> --pv-min <線> を付けたら、線はそれだけで見る
pv_p, pv_min = opt("--pv"), opt("--pv-min")
drop_p = opt("--drop")
extra = [a for a in extra if a not in (pv_p, pv_min, drop_p)]
drop = set()
if drop_p:
    for l in open(drop_p, encoding="utf-8"):
        c = l.rstrip("\n").split("\t")
        if c[0].startswith("#") or len(c) < 3: continue
        if int(c[1]) or int(c[2]): drop.add(c[0])
pv_of = {}
if pv_p:
    for l in open(pv_p, encoding="utf-8"):
        if l.startswith("#"): continue
        c = l.rstrip("\n").split("\t")
        pv_of[(c[1], c[2])] = max(pv_of.get((c[1], c[2]), 0), int(c[3]))
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
def rows(p): return [l.rstrip("\n").split("\t") for l in open(p, encoding="utf-8") if not l.startswith("#") and l.strip()]
best, pairs, pieces = defaultdict(lambda: -99.0), set(), defaultdict(set)
for l in open(dic_p, encoding="utf-8"):
    c = l.rstrip("\n").split("\t"); v = float(c[5]); pairs.add((c[0], c[1]))
    if v > -29.0: best[c[0]] = max(best[c[0]], v); pieces[c[1]].add(c[0])
for p in extra:
    for c in rows(p):
        if len(c) > 4:
            pairs.add((kata(c[1]), c[0]))
            try:
                if float(c[4]) > -29.0: best[kata(c[1])] = max(best[kata(c[1])], float(c[4]))
            except ValueError: pass
# 会社・法人の形の語と読み (読みに無ければ表記から外す)
LEGAL = {"株式会社": ("カブシキガイシャ", "カブシキカイシャ"), "有限会社": ("ユウゲンガイシャ", "ユウゲンカイシャ"), "合同会社": ("ゴウドウガイシャ", "ゴウドウカイシャ"),
         "合資会社": ("ゴウシガイシャ",), "合名会社": ("ゴウメイガイシャ",), "一般社団法人": ("イッパンシャダンホウジン",), "公益社団法人": ("コウエキシャダンホウジン",),
         "一般財団法人": ("イッパンザイダンホウジン",), "公益財団法人": ("コウエキザイダンホウジン",), "学校法人": ("ガッコウホウジン",), "医療法人": ("イリョウホウジン",),
         "社会福祉法人": ("シャカイフクシホウジン",), "宗教法人": ("シュウキョウホウジン",), "独立行政法人": ("ドクリツギョウセイホウジン",),
         "特定非営利活動法人": ("トクテイヒエイリカツドウホウジン",)}
KEEP_SYMBOLS = set("・ー々-.&'/－．＆＇／")
def bad_symbols(name): return sorted({ch for ch in name if unicodedata.category(ch)[0] in "SP" and ch not in KEEP_SYMBOLS})
LATIN, JAPANESE = re.compile(r"[A-Za-zÀ-ÖØ-öø-ɏ]"), re.compile(r"[ぁ-ゖァ-ヺー一-鿿々]")
def is_kana(s): return all("ぁ" <= ch <= "ゖ" or "ァ" <= ch <= "ヺ" or ch == "ー" for ch in s)
def composable(S, R):  # S を辞書の語と読みどおりのかなの並びで、読み R どおりに作れるか
    seen, todo = {(0, 0)}, [(0, 0)]
    while todo:
        i, j = todo.pop()
        if i == len(S) and j == len(R): return True
        for k in range(i + 1, min(len(S), i + 12) + 1):
            sub = S[i:k]; rs = set(pieces.get(sub, ()))
            if is_kana(sub): rs.add(kata(sub))
            for r in rs:
                if R.startswith(r, j) and (k, j + len(r)) not in seen: seen.add((k, j + len(r))); todo.append((k, j + len(r)))
    return False
CID = {"人物": 1289, "グループ": 1289, "1 語の名前": 1289, "競走馬": 1289, "作品": 1288, "出来事": 1288, "その他の固有名詞": 1288, "団体": 1292, "会社": 1292, "地名と施設": 1293}
def est(r): return int(r[3]) if not r[3].endswith("?") else 0
chosen, excluded = [], []
for r in rows(ac_p):
    kind, name, y = r[0], r[1], r[2]
    flags = [f for f in r[7].split("・") if f and f != "英字だけ"]
    # 英字・数字だけの名前 (1960・Mr.・KANA) は、全文の数が年や英語のふつうの語を数えてしまうので、上位の月だけで見る
    ascii_only = bool(re.fullmatch(r"[A-Za-z0-9 .,&'!?\-]+", name))
    line = int(r[5]) >= 1 or (est(r) >= 300 and not ascii_only)
    if pv_p: line = pv_of.get((name, y), 0) >= int(pv_min)
    if flags: excluded.append((name, y, kind, "印: " + "・".join(flags))); continue
    if kind != "会社" and not line: continue
    src = (f"Wikipedia「{r[8]}」の書き出し (手元の全文 20231101)" if kind not in ("会社",) else f"Wikidata の上場企業 ({r[9].split('銘柄')[-1].strip()} )・Wikipedia「{r[8]}」の書き出し")
    chosen.append((kind, name, y, src))
for r in rows(ry_p):
    if r[8] == "候補" and r[11].startswith("A"):
        kind = r[11].split()[1]
        chosen.append((kind, r[0], r[1], f"Wikipedia「新語・流行語大賞」(版 {r[10] or '?'}) の受賞語の記事"))
out, seen = [], set()
for kind, name, y, src in chosen:
    if not y or not re.fullmatch(r"[ぁ-ゖァ-ヺー]+", y): excluded.append((name, y, kind, "読みが無いか、かな以外の字")); continue
    R = kata(y)
    if name in drop: excluded.append((name, y, kind, "3b の測りで正しかった所を替えた")); continue
    for lw, lr in LEGAL.items():
        if lw in name and not any(x in R for x in lr): name = name.replace(lw, "").strip(" 　")
    if not name: excluded.append((name, y, kind, "会社の形の語だけ")); continue
    if bad_symbols(name): excluded.append((name, y, kind, "記号を含む: " + "".join(bad_symbols(name)))); continue
    if len(R) <= 2: excluded.append((name, y, kind, "読みが 2 字まで")); continue
    if LATIN.search(name) and JAPANESE.search(name) and composable("".join(ch for ch in name if JAPANESE.match(ch)), R):
        excluded.append((name, y, kind, "英字の混じる表記で、読みが英字の所を覆っていない")); continue
    if (R, name) in pairs: excluded.append((name, y, kind, "辞書に同じ表記と読みがある")); continue
    if (name, R) in seen: continue
    seen.add((name, R))
    v = -13.0 if len(y) >= 5 else -14.5
    if best[R] > -99.0: v = min(v, round(best[R] - 0.5, 3))
    if v < -40.0: excluded.append((name, y, kind, "値が低すぎる")); continue
    out.append((name, R, str(CID[kind]), "501", f"{v:.3f}", src + f" ({kind})"))
with open(out_p, "w", encoding="utf-8", newline="\n") as f:
    f.write("# 表記\t読み\tcid\tmid\t値\t出典\n")
    for o in out: f.write("\t".join(o) + "\n")
with open(exc_p, "w", encoding="utf-8", newline="\n") as f:
    f.write("# 表記\t読み\t種類\t外した理由\n")
    for e in excluded: f.write("\t".join(e) + "\n")
from collections import Counter
print("入れる", len(out), Counter(o[5].rsplit("(", 1)[1].rstrip(")") for o in out).most_common())
print("外す", Counter(e[3].split(":")[0] for e in excluded).most_common())
