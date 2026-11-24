"""2026-11-03: よくある日本の名前のローマ字の表 (japanese_names_romaji.txt) を、Mozc の公開辞書 (dictionary_oss) の人名から作る。
IME 本体が、Shift で打ち始めた英字を かな に戻すかを決めるときに使う (英字で打った名前 Aiko・Takemoto を割らないため。auto_english の temp_ascii_return)。
  python3 build_names_romaji.py <dictionary_oss の dictionary0*.txt の置き場> <出力> [--given N] [--family N] [--exclude <名前の表 (1 行 1 語)>]
- 人名の名 (品詞 1922)・姓 (1923) のうち、左右の品詞が同じ行の読みを、コストの低い (よく出る) 順に名 N 個・姓 N 個
- 読みをローマ字に: ヘボン式 (長音を書く shouta・書かない shota)・訓令式 (syouta)。ん は n と nn の両方
- --exclude の語 (小文字のローマ字) の綴りが 1 つでも入る読みは、どの綴りも入れない (測るとき、表に無い名前を作るため)"""
import argparse, glob, re
p = argparse.ArgumentParser()
p.add_argument("dictionary")
p.add_argument("out")
p.add_argument("--given", type=int, default=5000)
p.add_argument("--family", type=int, default=5000)
p.add_argument("--exclude")
a = p.parse_args()
best = {"1922": {}, "1923": {}}
for path in sorted(glob.glob(f"{a.dictionary}/dictionary0*.txt")):
    for line in open(path, encoding="utf-8"):
        r = line.rstrip("\n").split("\t")
        if len(r) < 5 or r[1] != r[2] or r[1] not in best or not re.fullmatch(r"[ぁ-ゖー]+", r[0]): continue
        cost = int(r[3])
        if r[0] not in best[r[1]] or cost < best[r[1]][r[0]]: best[r[1]][r[0]] = cost
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
    out, i = [], 0
    while i < len(kana):
        c = kana[i]; nx = kana[i + 1] if i + 1 < len(kana) else ""
        if c == "ん":
            out.append("nn" if nn else "n"); i += 1; continue
        if c == "っ":
            rest = romaji(kana[i + 1:i + 3], hepburn, nn)
            if not rest or rest[0] in "aiueon": return None
            out.append("t" if hepburn and rest.startswith("ch") else rest[0]); i += 1; continue
        if nx in YOON and c in BASE:
            out.append((HEP_Y[c] if hepburn and c in HEP_Y else BASE[c][:-1] + "y") + YOON[nx]); i += 2; continue
        if c == "ー": return None
        if hepburn and c in HEP: out.append(HEP[c]); i += 1; continue
        if c not in BASE: return None
        out.append(BASE[c]); i += 1
    return "".join(out)
def spellings(kana):
    forms = set()
    for hepburn in (True, False):
        for nn in (False, True):
            s = romaji(kana, hepburn, nn)
            if not s: continue
            forms.add(s)
            if hepburn:
                # 長音を書かない形 (shouta → shota・yuuki → yuki・oono → ono)
                forms.add(s.replace("ou", "o").replace("uu", "u").replace("oo", "o"))
    return {f for f in forms if len(f) >= 2}
exclude = set()
if a.exclude:
    exclude = {l.strip().lower() for l in open(a.exclude, encoding="utf-8") if l.strip() and not l.startswith("#")}
names = set()
for cid, n in (("1922", a.given), ("1923", a.family)):
    for kana, _ in sorted(best[cid].items(), key=lambda kv: (kv[1], kv[0]))[:n]:
        forms = spellings(kana)
        # 外す語の綴りが 1 つでも入る読みは、ほかの綴りも入れない (shouta を外すなら syouta・shota も)
        if forms & exclude: continue
        names |= forms
with open(a.out, "w", encoding="utf-8", newline="\n") as f:
    f.write("# HaniWord: よくある日本の名前のローマ字 (1 行 1 つ、小文字)。Shift で打ち始めた英字の名前を割らないために IME 本体が読む。\n")
    f.write("# 出典: Mozc の公開辞書 (dictionary_oss) の人名の読み (tools/names/build_names_romaji.py)。許諾は extra_dictionaries/LICENSE.txt\n")
    for name in sorted(names): f.write(name + "\n")
print(len(names), "names")
