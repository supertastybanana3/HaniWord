"""2026-11-04 C: 仕事 19 (1) の材料。よくある日本の名前のローマ字の表 (japanese_names_romaji.txt) を広げる (今の表は Mozc の名・姓の 5,000 読みずつ)。
- Mozc の公開辞書 (dictionary_oss) の人名の項目すべて: 名 (1922)・姓 (1923)・人名一般 (1921) の、左右の品詞が同じ行のひらがなの読み
- Wikidata (CC0) の日本の人物 (P31=Q5・P27=Q17) の名前のカナ (P1814) のうち、空白などで 2 つに分かれたもの (姓 名)。1 つにつながったもの・3 つ以上は使わない
綴りは今の表と同じ (tools/names/build_names_romaji.py の romaji・spellings を写した: ヘボン式・訓令式・n/nn・長音を書かない形。2 字以上)。
印: 英単語 (IME 本体の english_words_scowl.txt、SCOWL の大きさ 35 まで) と同じ綴り、ローマ字でよく打つ日本語の語 (wordfreq 3.1.1 の日本語の頻度の上位 --top 語の、
     Sudachi の読みの綴り。人名・地名の語は除く) と同じ綴り。A が守るかを決める
  nice -n 19 <Sudachi の python> -I build_names_v2.py <mozc_oss> <jp_people_kana.tsv> <english_words_scowl.txt> <scowl_levels.tsv> <ja_freq.tsv> <出力の置き場> [--top 30000]
出力: names_romaji_v2.tsv (綴り<TAB>出どころ<TAB>読み (3 つまで)<TAB>Wikidata の人数<TAB>Mozc のコスト (いちばん低いもの)<TAB>英単語 (SCOWL の大きさ)<TAB>日本語の語 (語:頻度の順位))、
      japanese_names_romaji_v2.txt (綴りだけ、今の表と同じ形)、stats.txt"""
import collections, glob, re, sys
args = sys.argv[1:]
top = 30000
if "--top" in args:
    i = args.index("--top"); top = int(args[i + 1]); del args[i:i + 2]
mozc, wd_path, scowl_path, levels_path, freq_path, out = args
hira = lambda s: "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
# --- 綴り (tools/names/build_names_romaji.py と同じ) ---
BASE = {
    "あ": "a", "い": "i", "う": "u", "え": "e", "お": "o", "か": "ka", "き": "ki", "く": "ku", "け": "ke", "こ": "ko", "さ": "sa", "し": "si", "す": "su",
    "せ": "se", "そ": "so", "た": "ta", "ち": "ti", "つ": "tu", "て": "te", "と": "to", "な": "na", "に": "ni", "ぬ": "nu", "ね": "ne", "の": "no",
    "は": "ha", "ひ": "hi", "ふ": "hu", "へ": "he", "ほ": "ho", "ま": "ma", "み": "mi", "む": "mu", "め": "me", "も": "mo", "や": "ya", "ゆ": "yu",
    "よ": "yo", "ら": "ra", "り": "ri", "る": "ru", "れ": "re", "ろ": "ro", "わ": "wa", "を": "wo", "が": "ga", "ぎ": "gi", "ぐ": "gu", "げ": "ge",
    "ご": "go", "ざ": "za", "じ": "zi", "ず": "zu", "ぜ": "ze", "ぞ": "zo", "だ": "da", "ぢ": "di", "づ": "du", "で": "de", "ど": "do", "ば": "ba",
    "び": "bi", "ぶ": "bu", "べ": "be", "ぼ": "bo", "ぱ": "pa", "ぴ": "pi", "ぷ": "pu", "ぺ": "pe", "ぽ": "po"}
HEP = {"し": "shi", "ち": "chi", "つ": "tsu", "ふ": "fu", "じ": "ji", "ぢ": "ji", "づ": "zu"}
YOON = {"ゃ": "a", "ゅ": "u", "ょ": "o"}
HEP_Y = {"し": "sh", "ち": "ch", "じ": "j"}
def romaji(kana, hepburn, nn):
    o, i = [], 0
    while i < len(kana):
        c = kana[i]; nx = kana[i + 1] if i + 1 < len(kana) else ""
        if c == "ん":
            o.append("nn" if nn else "n"); i += 1; continue
        if c == "っ":
            rest = romaji(kana[i + 1:i + 3], hepburn, nn)
            if not rest or rest[0] in "aiueon": return None
            o.append("t" if hepburn and rest.startswith("ch") else rest[0]); i += 1; continue
        if nx in YOON and c in BASE:
            o.append((HEP_Y[c] if hepburn and c in HEP_Y else BASE[c][:-1] + "y") + YOON[nx]); i += 2; continue
        if c == "ー": return None
        if hepburn and c in HEP: o.append(HEP[c]); i += 1; continue
        if c not in BASE: return None
        o.append(BASE[c]); i += 1
    return "".join(o)
def spellings(kana):
    forms = set()
    for hepburn in (True, False):
        for nn in (False, True):
            s = romaji(kana, hepburn, nn)
            if not s: continue
            forms.add(s)
            if hepburn:
                forms.add(s.replace("ou", "o").replace("uu", "u").replace("oo", "o"))
    return {f for f in forms if len(f) >= 2}
# --- 名前の読み ---
src = collections.defaultdict(set); readings = collections.defaultdict(set); people = collections.Counter(); cost = {}
LABEL = {"1922": "Mozc 名", "1923": "Mozc 姓", "1921": "Mozc 人名一般"}
mozc_readings = collections.Counter()
for path in sorted(glob.glob(f"{mozc}/dictionary0*.txt")):
    for line in open(path, encoding="utf-8"):
        r = line.rstrip("\n").split("\t")
        if len(r) < 5 or r[1] != r[2] or r[1] not in LABEL or not re.fullmatch(r"[ぁ-ゖー]+", r[0]): continue
        for s in spellings(r[0]):
            src[s].add(LABEL[r[1]]); readings[s].add(r[0])
            cost[s] = min(cost.get(s, 10 ** 9), int(r[3]))
        mozc_readings[LABEL[r[1]]] += 1
wd_rows = wd_used = 0
for l in open(wd_path, encoding="utf-8"):
    if l.startswith("?"): continue
    q, k = l.rstrip("\n").split("\t")
    m = re.match(r'^"(.*)"(@[\w-]+)?$', k); v = hira(m.group(1) if m else k)
    wd_rows += 1
    parts = [p for p in re.split(r"[ 　・＝=]+", v.strip()) if p]
    if len(parts) != 2: continue
    wd_used += 1
    for p, lab in zip(parts, ("Wikidata 姓", "Wikidata 名")):
        if not re.fullmatch(r"[ぁ-ゖー]+", p): continue
        for s in spellings(p):
            src[s].add(lab); readings[s].add(p); people[s] += 1
# --- 印 ---
scowl = {l.strip() for l in open(scowl_path, encoding="utf-8") if l.strip() and not l.startswith("#")}
level = {}
for l in open(levels_path, encoding="utf-8"):
    p = l.rstrip("\n").split("\t")
    if len(p) == 2 and p[1].isdigit(): level[p[0]] = int(p[1])
from sudachipy import dictionary, tokenizer
tok = dictionary.Dictionary(dict="core").create(); MODE = tokenizer.Tokenizer.SplitMode.C
common = {}  # 綴り -> (語, 順位)
rank = 0
for l in open(freq_path, encoding="utf-8"):
    w = l.split("\t")[0]
    if not re.search(r"[ぁ-ゖァ-ヺ一-鿿]", w): continue
    rank += 1
    if rank > top: break
    ms = tok.tokenize(w, MODE)
    if len(ms) != 1: continue
    pos = ms[0].part_of_speech()
    if pos[1] == "固有名詞" and pos[2] in ("人名", "地名"): continue
    rd = hira(ms[0].reading_form())
    if not re.fullmatch(r"[ぁ-ゖー]+", rd): continue
    for s in spellings(rd):
        if s not in common: common[s] = (w, rank)
names = sorted(src)
with open(f"{out}/names_romaji_v2.tsv", "w", encoding="utf-8", newline="\n") as f:
    f.write("# 2026-11-04 C 仕事 19: よくある日本の名前のローマ字 (Mozc の公開辞書の人名すべて・Wikidata (CC0) の日本の人物の名前のカナ)。build_names_v2.py\n")
    f.write("# 綴り\t出どころ\t読み (3 つまで)\tWikidata の人数\tMozc のコスト (低いほどよく出る)\t英単語 (SCOWL の大きさ。english_words_scowl.txt にある綴り)\t日本語の語 (wordfreq の日本語の上位の語:順位)\n")
    for s in names:
        eng = (str(level.get(s, "")) or "あり") if s in scowl else ""
        ja = f"{common[s][0]}:{common[s][1]}" if s in common else ""
        f.write(f"{s}\t{'・'.join(sorted(src[s]))}\t{'・'.join(sorted(readings[s])[:3])}\t{people[s] or ''}\t{cost.get(s, '')}\t{eng}\t{ja}\n")
with open(f"{out}/japanese_names_romaji_v2.txt", "w", encoding="utf-8", newline="\n") as f:
    f.write("# HaniWord: よくある日本の名前のローマ字 (1 行 1 つ、小文字)。Shift で打ち始めた英字の名前を割らないために IME 本体が読む。\n")
    f.write("# 出典: Mozc の公開辞書 (dictionary_oss) の人名の読みと、Wikidata (CC0) の日本の人物の名前のカナ (P1814)。印つきの表は names_romaji_v2.tsv\n")
    for s in names: f.write(s + "\n")
st = collections.Counter()
for s in names:
    st["綴り"] += 1
    if src[s] & {"Mozc 名", "Mozc 姓", "Mozc 人名一般"}: st["Mozc にある"] += 1
    if src[s] & {"Wikidata 姓", "Wikidata 名"}: st["Wikidata にある"] += 1
    if not src[s] & {"Mozc 名", "Mozc 姓", "Mozc 人名一般"}: st["Wikidata だけ"] += 1
    if s in scowl: st["英単語の印"] += 1
    if s in common: st["日本語の語の印"] += 1
lines = [f"Mozc の行 (ひらがなの読みで左右の品詞が同じもの) {dict(mozc_readings)}", f"Wikidata の行 {wd_rows}・姓 名の 2 つに分かれた行 {wd_used}", f"日本語の語 (wordfreq 上位 {top} 語のうち 1 語で読みのあるもの) の綴り {len(common)}"] + [f"{k} {v}" for k, v in st.items()]
open(f"{out}/stats.txt", "w", encoding="utf-8").write("\n".join(lines) + "\n")
print("\n".join(lines))
