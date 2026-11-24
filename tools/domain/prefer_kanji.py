"""交ぜ書き (側わん・木ぐい) と漢字の書き方 (側弯・木杭) が同じ読みで分野辞書に並んでいるとき、漢字の書き方を上にする (2026-10-31)。
漢字の書き方を交ぜ書きと同じ値まで上げ、交ぜ書きをそれより 0.5 下げる (2 番目の候補に残す)。漢字の書き方が 2 つあれば
常用漢字の多いほう・画数の少ないほう (抑鬱/抑欝 → 抑鬱) だけを上げる。同じ読み・同じ値なので、一般の語とのぶつかり方は交ぜ書きと同じ。
組の見つけ方: かなを含む語と、かなを含まず、その語の漢字をすべて含んでさらに漢字の多い語 (送り仮名だけの違い 取り消し/取消 は組にしない)。
教科書の書き方が交ぜ書きの分野 (地学の 花こう岩・数学の べき根) には使わない。
使い方: python3 prefer_kanji.py <名前> [--only <交ぜ書きの語> ...] [--dry-run]
         KANJIDIC2 (常用漢字・画数) は環境変数 HANIWORD_BUILD の data_ext/kanjidic/kanjidic2.xml.gz から読む
"""
import argparse, collections, gzip, os, re, sys
import xml.etree.ElementTree as ET

REPO = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")


def is_kanji(c): return "一" <= c <= "鿿" or "㐀" <= c <= "䶿" or c == "々"
def has_kana(s): return any("ぁ" <= c <= "ゖ" for c in s)


def load_kanjidic():
    path = os.path.join(os.environ["HANIWORD_BUILD"], "data_ext", "kanjidic", "kanjidic2.xml.gz")
    info = {}
    for _, e in ET.iterparse(gzip.open(path), events=("end",)):
        if e.tag != "character":
            continue
        grade = e.findtext("misc/grade")
        strokes = e.findtext("misc/stroke_count")
        info[e.findtext("literal")] = (grade is not None and int(grade) <= 8, int(strokes or 30))
        e.clear()
    return info


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("name")
    ap.add_argument("--only", nargs="*", help="この交ぜ書きの語の組だけ")
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    path = os.path.join(REPO, "domain_dictionaries", a.name + ".tsv")
    lines = open(path, encoding="utf-8").read().split("\n")
    rows = {i: l.split("\t") for i, l in enumerate(lines) if l and not l.startswith("#")}
    by_reading = collections.defaultdict(list)
    for i, c in rows.items():
        by_reading[c[1]].append(i)
    kd = load_kanjidic()
    def rank(word):  # 常用でない字の数・画数の合計 (小さいほうを選ぶ)
        ks = [c for c in word if is_kanji(c)]
        return (sum(not kd.get(c, (False, 30))[0] for c in ks), sum(kd.get(c, (False, 30))[1] for c in ks))
    changed = 0
    for i, c in sorted(rows.items()):
        word = c[0]
        if not has_kana(word) or not any(is_kanji(ch) for ch in word) or (a.only is not None and word not in a.only):
            continue
        ks = {ch for ch in word if is_kanji(ch)}
        kanji_forms = [j for j in by_reading[c[1]] if j != i and not has_kana(rows[j][0]) and {ch for ch in rows[j][0] if is_kanji(ch)} > ks]
        if not kanji_forms:
            continue
        best = min(kanji_forms, key=lambda j: rank(rows[j][0]))
        mv, kv = float(c[4]), float(rows[best][4])
        new_k = max(kv, mv)
        new_m = min(mv, new_k - 0.5)
        if (new_k, new_m) == (kv, mv):
            continue
        print(f"{word} {mv:g} → {new_m:g}\t{rows[best][0]} {kv:g} → {new_k:g}")
        rows[best][4] = f"{new_k:g}"
        c[4] = f"{new_m:g}"
        others = [j for j in kanji_forms if j != best and float(rows[j][4]) >= new_k]
        for j in others:  # 選ばなかった漢字の書き方 (側彎症) は、選んだ書き方と交ぜ書きのあいだ
            print(f"\t{rows[j][0]} {rows[j][4]} → {new_k - 0.25:g}")
            rows[j][4] = f"{new_k - 0.25:g}"
        for j in (i, best, *others):
            if not rows[j][8].endswith(";kanji_first"):
                rows[j][8] += ";kanji_first"
        changed += 1
    print(f"{a.name}: {changed} 組")
    if not a.dry_run and changed:
        for i, c in rows.items():
            lines[i] = "\t".join(c)
        open(path, "w", encoding="utf-8", newline="\n").write("\n".join(lines))


if __name__ == "__main__":
    main()
