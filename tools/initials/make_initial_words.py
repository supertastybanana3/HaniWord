"""2026-11-04 (仕事 19 の 3a): 英字 1 字＋語 (Jリーグ・B級・X線) を小文字で打った所 (jri-gu・bkyuu・xsenn) を区切るための表 (initial_words.txt) を作る。
IME 本体 (auto_english の initial_word_span) が、子音の字のすぐ後ろが語の部分の綴りのとき、字だけを英字にして表の形 (大文字・小文字) にする。
  python3 make_initial_words.py <initials_words.tsv> <出力> [--min-count N]
- 材料: 英字 1 字＋漢字・カタカナの語の表 (Wikipedia 日本語版の本文での回数、語の読みは Sudachi。data_ext\\initials の initials_words.tsv)
- 子音の字だけ (母音の字 A面・iモード は、ローマ字として全部読めるので区切りだけでは決めない。n も ん とぶつかるので入れない)。合計 N 回以上 (初期値 20)
- 語の部分の読みをローマ字に: ヘボン式・訓令式、ん は n と nn、ー は -。小さいかなの組 (ふぁ・てぃ・しぇ …) も
- 行: 打つ綴り (字＋語の部分、小文字)<TAB>字の形 (表で多い形)<TAB>合計<TAB>多い形の割合。同じ綴りは合計の多い語の行だけ"""
import argparse

p = argparse.ArgumentParser()
p.add_argument("table")
p.add_argument("out")
p.add_argument("--min-count", type=int, default=20)
a = p.parse_args()

BASE = {
    "あ": "a", "い": "i", "う": "u", "え": "e", "お": "o", "か": "ka", "き": "ki", "く": "ku", "け": "ke", "こ": "ko", "さ": "sa", "し": "si", "す": "su",
    "せ": "se", "そ": "so", "た": "ta", "ち": "ti", "つ": "tu", "て": "te", "と": "to", "な": "na", "に": "ni", "ぬ": "nu", "ね": "ne", "の": "no",
    "は": "ha", "ひ": "hi", "ふ": "hu", "へ": "he", "ほ": "ho", "ま": "ma", "み": "mi", "む": "mu", "め": "me", "も": "mo", "や": "ya", "ゆ": "yu",
    "よ": "yo", "ら": "ra", "り": "ri", "る": "ru", "れ": "re", "ろ": "ro", "わ": "wa", "を": "wo", "が": "ga", "ぎ": "gi", "ぐ": "gu", "げ": "ge",
    "ご": "go", "ざ": "za", "じ": "zi", "ず": "zu", "ぜ": "ze", "ぞ": "zo", "だ": "da", "ぢ": "di", "づ": "du", "で": "de", "ど": "do", "ば": "ba",
    "び": "bi", "ぶ": "bu", "べ": "be", "ぼ": "bo", "ぱ": "pa", "ぴ": "pi", "ぷ": "pu", "ぺ": "pe", "ぽ": "po", "ゔ": "vu"}
HEP = {"し": "shi", "ち": "chi", "つ": "tsu", "ふ": "fu", "じ": "ji", "ぢ": "ji", "づ": "zu"}
YOON = {"ゃ": "a", "ゅ": "u", "ょ": "o"}
HEP_Y = {"し": "sh", "ち": "ch", "じ": "j"}
# 小さい母音のかなの組 (外来語)。ヘボン式と訓令式で同じ綴り
SMALL = {"ふぁ": "fa", "ふぃ": "fi", "ふぇ": "fe", "ふぉ": "fo", "てぃ": "thi", "でぃ": "dhi", "とぅ": "twu", "どぅ": "dwu", "うぃ": "wi", "うぇ": "we",
         "うぉ": "who", "しぇ": "she", "ちぇ": "che", "じぇ": "je", "ゔぁ": "va", "ゔぃ": "vi", "ゔぇ": "ve", "ゔぉ": "vo", "つぁ": "tsa", "てゅ": "thu"}


def romaji(kana, hepburn, nn):
    out, i = [], 0
    while i < len(kana):
        c = kana[i]
        nx = kana[i + 1] if i + 1 < len(kana) else ""
        if c + nx in SMALL:
            out.append(SMALL[c + nx]); i += 2; continue
        if c == "ん":
            out.append("nn" if nn else "n"); i += 1; continue
        if c == "ー":
            out.append("-"); i += 1; continue
        if c == "っ":
            rest = romaji(kana[i + 1:i + 3], hepburn, nn)
            if not rest or rest[0] in "aiueon-": return None
            out.append("t" if hepburn and rest.startswith("ch") else rest[0]); i += 1; continue
        if nx in YOON and c in BASE:
            out.append((HEP_Y[c] if hepburn and c in HEP_Y else BASE[c][:-1] + "y") + YOON[nx]); i += 2; continue
        if hepburn and c in HEP:
            out.append(HEP[c]); i += 1; continue
        if c not in BASE: return None
        out.append(BASE[c]); i += 1
    return "".join(out)


def spellings(kana):
    return {s for hepburn in (True, False) for nn in (False, True) if (s := romaji(kana, hepburn, nn))}


best = {}
for line in open(a.table, encoding="utf-8"):
    if line.startswith("#"): continue
    # 多い形の語 英字 語の部分 読み 合計 大文字 小文字 多い形 多い形の割合 …
    r = line.rstrip("\n").split("\t")
    letter, reading, total, form, ratio = r[1], r[3], int(r[4]), r[7], float(r[8])
    # 母音の字 (全部ローマ字として読める) と n (後ろの子音と ん になる打ち方がある) は IME 本体が見ないので入れない
    if total < a.min_count or len(letter) != 1 or not letter.isascii() or not letter.isalpha() or letter in "aiueon": continue
    shown = letter.upper() if form == "大" else letter.lower()
    for s in spellings(reading.replace("ゐ", "い").replace("ゑ", "え")):
        key = letter + s
        if key not in best or total > best[key][1]:
            best[key] = (shown, total, ratio)
with open(a.out, "w", encoding="utf-8", newline="\n") as f:
    f.write("# HaniWord: 英字 1 字＋語 (Jリーグ・B級・X線) を小文字で打った綴りの表 (打つ綴り<TAB>字の形<TAB>合計<TAB>多い形の割合)。子音の字だけ。\n")
    f.write("# 出典: Wikipedia 日本語版 (CC BY-SA) の本文での語の形と回数 (文そのものは入らない)・SudachiDict (Apache License 2.0) の読み (tools/initials/make_initial_words.py)\n")
    for key in sorted(best):
        shown, total, ratio = best[key]
        f.write(f"{key}\t{shown}\t{total}\t{ratio:.3f}\n")
print(len(best), "spellings")
