"""2026-11-06: english_suru_words.txt を作る (英単語＋する。IME 本体の auto_english.rs の promote_before_suru が使う)。

サ変の形 (する・した・して …) のすぐ前なら英字にまとめる英単語。ローマ字として読んだかなが、変換の辞書で日本語の語にならないもの:
  語全体がローマ字として読める語 (release = れぇあせ・delete = でぇて・save = さゔぇ) は、その語のかな
  t で終わる語 (commit・edit・reset) は、t が する の す と合わさって つ になる形 (commit + suru = こっみつる) のかな
日本語の語とみなす条件は make_boundary_words.py とほぼ同じ (辞書だけの 1 位が 1 語で点数が -16 以上、読み全体が 1 語になる語が -16 以上、
1 位の並びに助詞が入る)。ただし助詞の条件は、1 位の点数が PARTICLE_FLOOR (-25) 以上のときだけ (2026-11-06。1 位がでたらめな並び
(delete = で|ェ|て -35.1・generate・execute) まで日本語とみなすと、て で終わる読みの英単語が入らなかった。-25 以上の 109 語 (待って -14.1・
愛で -14.5・子で -12.0・間で -11.1) は外したまま)。ためした (tame = ため)・までした (made = まで)・まつる (mat + suru) のように日本語の語になる語は入れない。
語＋する の全体がローマ字として読めない語 (restart) は IME 本体が表なしで英字にするので、この表には要らない (変換サーバーの試験も読める語しか書き出さない)。

足す語 (suru_extra_words.txt。SCOWL・english_terms に無い git の動詞 rebase・stash など。出典は README.md) は、この表の元にだけ使う。
足す語は英単語の表の語ではないので、IME 本体の「全体が読めない語は表なしで英字に」が効かない。ローマ字として読めない足す語 (stash) は
変換サーバーの試験が書き出さないので、日本語の語とみなす行が 1 つも無ければ表に入れる (読めない並びは日本語の語にならない)。

使い方:
  python3 -I make_suru_words.py inputs <出力 (KanaWordsProbe の入力)> <足す語の表> <英単語の表>...
      英単語の表 (english_words_scowl.txt・english_terms.txt など。1 行 1 語、# は説明、english_terms の頭の ^ は外す) と足す語の表の、小文字の英字だけの 4〜14 字の語。
      1 行に「ローマ字<TAB>語<TAB>形<TAB>元」(形は word = 語そのもの、t = 語＋suru。元は table = 英単語の表、extra = 足す語)
  変換サーバーの試験 KanaWordsProbe を、AZK_KANA_WORDS_IN に上の出力を渡して流す
  python3 -I make_suru_words.py pick <inputs の出力> <KanaWordsProbe の出力> <english_suru_words.txt>
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from make_boundary_words import THRESHOLD, katakana  # noqa: E402

PARTICLE_FLOOR = -25.0


def japanese_like(kana, best, count, best_value, single, single_value, elements):
    """make_boundary_words.japanese_like と同じで、助詞の条件だけ 1 位の点数が PARTICLE_FLOOR 以上のとき"""
    if count == 1 and best_value >= THRESHOLD:
        return True
    if best_value >= PARTICLE_FLOOR and any(element.endswith(":j") for element in elements.split("|")):
        return True
    fallback = single == katakana(kana) and single_value in (-14.0, -7.0)
    return bool(single) and not fallback and single_value >= THRESHOLD


def inputs(out, extra, *tables):
    seen = set()
    rows = []
    for table in (*tables, extra):
        source = "extra" if table == extra else "table"
        for line in open(table, encoding="utf-8"):
            w = line.strip()
            if w.startswith("^"):
                w = w[1:]
            if not w or w.startswith("#") or not (4 <= len(w) <= 14) or not w.isascii() or not w.isalpha() or not w.islower() or w in seen:
                continue
            seen.add(w)
            rows.append((w, w, "word", source))
            if w.endswith("t"):
                rows.append((w + "suru", w, "t", source))
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        for row in rows:
            f.write("\t".join(row) + "\n")
    print(len(seen), "語", len(rows), "行")


def pick(inputs_path, probe, out):
    word_of, extra = {}, set()
    for line in open(inputs_path, encoding="utf-8"):
        roman, w, kind, source = line.rstrip("\n").split("\t")
        word_of[roman] = (w, kind)
        if source == "extra":
            extra.add(w)
    picked, excluded, japanese = set(), set(), {"word": 0, "t": 0}
    for line in open(probe, encoding="utf-8"):
        roman, kana, best, count, best_value, single, single_value, elements = line.rstrip("\n").split("\t")
        if roman not in word_of:
            continue
        w, kind = word_of[roman]
        if japanese_like(kana, best, int(count), float(best_value), single, float(single_value), elements):
            japanese[kind] += 1
            excluded.add(w)
            continue
        picked.add(w)
    picked |= extra
    picked -= excluded
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        f.write("# サ変の形 (する・した・して …) のすぐ前なら英字にまとめる英単語。ローマ字として読んだかな (release = れぇあせ・commit + する = こっみつる) が、\n")
        f.write("# 変換の辞書で日本語の語にならないもの。tools/english_lists/make_suru_words.py で作成 (英単語の表の、小文字の英字だけの 4〜14 字の語)\n")
        for w in sorted(picked):
            f.write(w + "\n")
    print(len(picked), "語を入れた。日本語の語とみなして外した:", japanese)


if __name__ == "__main__":
    {"inputs": lambda: inputs(sys.argv[2], *sys.argv[3:]), "pick": lambda: pick(*sys.argv[2:5])}[sys.argv[1]]()
