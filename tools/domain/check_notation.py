"""分野辞書の表記の崩れを洗い出して直す: 漢字の一部が片仮名 1 字になった表記 (耿湋 → 耿イ、倪元璐 → 倪元ロ)。
Wikidata の日本語の名前に、JIS にない漢字を片仮名に置き換えたものがあり、それが表記として入っていた。
拾う形: 前後が片仮名でない片仮名 1 字が、漢字と隣り合う (語の終わり・漢字にはさまれる・漢字の前)。
拾った語は、出典の表 (<分野>/sources.tsv) にある Wikipedia の記事名 (括弧の曖昧さ回避を除く) と比べる。
  - 記事名と字数が同じで、違うのが拾った片仮名の位置だけ (記事名ではそこが漢字) → 記事名の表記に直す (読みはそのまま)。
    直した表記の同じ読みの行が既にあれば、崩れた行は外す
  - それ以外 (記事名と同じ・比べる記事名が無い・記事名が別の理由で違う) → 残す (〇ヶ岳・〇ノ島・ヒ素・ソ連・〇ニ関スル法律 のような正しい表記)
出力: 直した <名前>.tsv (--out) と、拾った語の扱いの一覧 (--log: 表記 / 読み / 扱い (fixed・dropped・kept) / 直した表記 / 比べた記事名)
使い方: python3 check_notation.py --tsv <名前>.tsv --sources <出典.tsv>... --out <名前>.tsv --log <一覧.tsv>
"""
import argparse, collections, os, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common

SINGLE_KATAKANA = re.compile(r"(?<![ァ-ヺー])[ァ-ヶ](?![ァ-ヺー])")
KANJI = re.compile(r"[一-龯㐀-䶵々〆\U00020000-\U0002FFFF]")


def suspicious(term):
    """漢字と隣り合う、前後が片仮名でない片仮名 1 字の位置"""
    return [m.start() for m in SINGLE_KATAKANA.finditer(term)
            if (m.start() > 0 and KANJI.match(term[m.start() - 1])) or (m.start() + 1 < len(term) and KANJI.match(term[m.start() + 1]))]


def repaired(term, positions, titles):
    """記事名のうち、拾った位置だけが漢字で違うもの (直した表記)。無ければ None"""
    if term in titles:
        return None
    for title in sorted(titles):
        if len(title) != len(term):
            continue
        diff = [i for i, (a, b) in enumerate(zip(term, title)) if a != b]
        if diff and set(diff) <= set(positions) and all(KANJI.match(title[i]) for i in diff):
            return title
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tsv", required=True)
    ap.add_argument("--sources", nargs="+", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--log", required=True)
    a = ap.parse_args()
    titles = collections.defaultdict(set)
    for path in a.sources:
        for line in open(path, encoding="utf-8"):
            c = line.rstrip("\n").split("\t")
            if len(c) >= 2 and c[1].startswith("Wikipedia:"):
                t = re.sub(r" \([^)]*\)$", "", c[1][len("Wikipedia:"):])
                titles[common.normalize_term(c[0]) or c[0]].add(common.normalize_term(t) or t)
    lines = open(a.tsv, encoding="utf-8").readlines()
    have = {tuple(l.split("\t")[:2]) for l in lines if not l.startswith("#")}
    out, log, stats = [], [], collections.Counter()
    for line in lines:
        c = line.rstrip("\n").split("\t")
        pos = [] if line.startswith("#") else suspicious(c[0])
        if not pos:
            out.append(line)
            continue
        fixed = repaired(c[0], pos, titles.get(c[0], ()))
        if fixed is None:
            stats["kept"] += 1
            log.append((c[0], c[1], "kept", "", "|".join(sorted(titles.get(c[0], ())))))
            out.append(line)
        elif fixed and (fixed, c[1]) not in have:
            stats["fixed"] += 1
            log.append((c[0], c[1], "fixed", fixed, "|".join(sorted(titles[c[0]]))))
            out.append("\t".join([fixed] + c[1:]) + "\n")
        else:
            stats["dropped"] += 1
            log.append((c[0], c[1], "dropped", fixed, "|".join(sorted(titles.get(c[0], ())))))
    with open(a.out, "w", encoding="utf-8", newline="\n") as f:
        f.writelines(out)
    with open(a.log, "w", encoding="utf-8", newline="\n") as f:
        f.write("# 表記\t読み\t扱い\t直した表記\t比べた記事名\n" + "".join("\t".join(r) + "\n" for r in log))
    print("notation check:", dict(stats))


if __name__ == "__main__":
    main()
