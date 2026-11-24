"""2026-10-21: english_boundary_words.txt を作る。

ローマ字として最後まで読めてしまう英単語 (see = せえ、issue = いっすえ) のうち、そのかなが変換の辞書で日本語の語にならないもの。
IME 本体は、この語が語の切れ目 (入力の頭・終わり・助詞・英単語) に収まっているときだけ英語にする (auto_english.rs の promote_short_terms)。

入力: 変換サーバーの試験 KanaWordsProbe の出力 (AZK_KANA_WORDS_IN に SCOWL の語を渡して作る)。1 行
  語<TAB>かな<TAB>辞書だけの 1 位<TAB>その語の数<TAB>1 位の点数<TAB>1 語で読み全体になる語<TAB>その点数<TAB>1 位の語の並び (表記:点数:助詞なら j を | でつなぐ)
日本語の語とみなす条件: 辞書だけの 1 位が 1 語で点数が -16 以上、または読み全体が 1 語になる語 (読みのカタカナにしただけの語 -14・-7 は除く) が -16 以上、
または 1 位の並びに助詞が入る (aide = 愛 + で・none = の値。語 + 助詞 の形は日本語の文の中に出やすい)
使い方: python3 make_boundary_words.py <KanaWordsProbe の出力> <english_romaji_priority.txt> <出力>
"""
import sys

THRESHOLD = -16.0


def katakana(text):
    return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in text)


def japanese_like(kana, best, count, best_value, single, single_value, elements):
    if count == 1 and best_value >= THRESHOLD:
        return True
    if any(element.endswith(":j") for element in elements.split("|")):
        return True
    fallback = single == katakana(kana) and single_value in (-14.0, -7.0)
    return bool(single) and not fallback and single_value >= THRESHOLD


def main(probe, priority, out):
    existing = {line.strip() for line in open(priority, encoding="utf-8") if line.strip() and not line.startswith("#")}
    words = []
    for line in open(probe, encoding="utf-8"):
        word, kana, best, count, best_value, single, single_value, elements = line.rstrip("\n").split("\t")
        if len(word) < 3 or word in existing:
            continue
        if not japanese_like(kana, best, int(count), float(best_value), single, float(single_value), elements):
            words.append(word)
    with open(out, "w", encoding="utf-8") as f:
        f.write("# ローマ字として最後まで読めてしまう英単語のうち、そのかなが変換の辞書で日本語の語にならないもの (see = せえ、issue = いっすえ)。\n")
        f.write("# 語の切れ目 (入力の頭・終わり・助詞・英単語) に収まっているときだけ英語にする。tools/english_lists/make_boundary_words.py で作成\n")
        f.write("# (SCOWL 2020.12.07 の大きさ 35 までの 3〜8 文字の語。6 文字以上で english_romaji_priority.txt にある語は除く)\n")
        for word in sorted(words):
            f.write(word + "\n")
    print(len(words))


if __name__ == "__main__":
    main(*sys.argv[1:4])
