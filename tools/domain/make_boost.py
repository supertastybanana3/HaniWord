"""同音語の優遇のデータ (domain_dictionaries/<名前>_boost.tsv と <名前>_signals.txt) を書く。
  <名前>_boost.tsv  : 変換器の辞書に既にある分野の語 (表記<TAB>読み)。
                      (1) select_terms.py で「同じ読みの一般の語が出る」として捨てた語を luna に回し、分野の語と判定されたもの (merged.tsv の keep=1)
                      (2) 目印の語に、変換器の辞書の読みを付けたもの
  <名前>_signals.txt: 目印の語 (luna_signals.py の出力)
使い方: python3 make_boost.py <名前> --field 生物 --merged <同音語の束の merged.tsv> --signals <luna_signals の出力> --dict <変換器の辞書の全件>
"""
import argparse, collections, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "domain_dictionaries")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("name")
    ap.add_argument("--field", required=True)
    ap.add_argument("--merged", required=True)
    ap.add_argument("--signals", required=True)
    ap.add_argument("--dict", required=True)
    a = ap.parse_args()
    readings = collections.defaultdict(set)
    value = {}  # (表記, 読み) → 変換器の辞書の値 (大きいほどよく使う)
    for line in open(a.dict, encoding="utf-8"):
        c = line.rstrip("\n").split("\t")
        readings[c[0]].add(c[1])
        value[(c[0], c[1])] = max(value.get((c[0], c[1]), -99.0), float(c[5]))
    homophones = set()
    for line in open(a.merged, encoding="utf-8"):
        if line.startswith("#") or not line.strip():
            continue
        c = line.rstrip("\n").split("\t")
        term = common.normalize_term(c[0])
        if term and c[4] == "1":
            for r in [c[1]] + [x for x in c[2].split(",") if x]:
                if r in readings.get(term, ()):  # 変換器の辞書にある読みだけ (優遇は辞書の語の差し替え)
                    homophones.add((term, r))
    signals = [l.strip() for l in open(a.signals, encoding="utf-8") if l.strip() and not l.startswith("#")]
    from_signals = {(w, r) for w in signals for r in readings.get(w, ())} - homophones
    # 同じ読みで、漢字の一部をかなにした語 (蛋白質 に対する タンパク質、燐酸 に対する リン酸) が変換器の辞書にあり、そちらのほうがよく使われる
    # (辞書の値が大きい) なら、それがふつうの書き方なので優遇しない (生物の文で タンパク質 → 蛋白質 に差し替えていた)
    by_reading = collections.defaultdict(set)
    for w, rs in readings.items():
        for r in rs:
            by_reading[r].add(w)
    def kanji(x):
        return {ch for ch in x if common.KANJI.match(ch)}
    def has_kana_variant(w, r):
        return any(v != w and kanji(v) and kanji(v) < kanji(w) and any("ぁ" <= ch <= "ヺ" for ch in v)
                   and value[(v, r)] > value[(w, r)] for v in by_reading[r])
    variants = {(w, r) for w, r in homophones | from_signals if has_kana_variant(w, r)}
    homophones -= variants
    from_signals -= variants
    print("dropped (the dictionary has a kana variant):", " ".join(sorted(w for w, _ in variants)))
    # 入れない語の表 (<名前>_exclude.txt。測った文で悪くした語と理由)
    ex_path = os.path.join(OUT, a.name + "_exclude.txt")
    if os.path.exists(ex_path):
        exclude = {l.split("\t")[0].strip() for l in open(ex_path, encoding="utf-8") if l.strip() and not l.startswith("#")}
        homophones = {x for x in homophones if x[0] not in exclude}
        from_signals = {x for x in from_signals if x[0] not in exclude}
    with open(os.path.join(OUT, a.name + "_boost.tsv"), "w", encoding="utf-8", newline="\n") as f:
        f.write(f"# {a.field}の文脈で優遇する語のうち、変換器の辞書に既にある語 (表記<TAB>読み)。\n")
        f.write(f"# (1) 一般の変換では同じ読みの別の語になる語を luna で{a.field}の語と判定したもの\n")
        f.write(f"# (2) {a.field}の目印の語 ({a.name}_signals.txt) に、変換器の辞書の読みを付けたもの\n")
        f.writelines(f"{w}\t{r}\n" for w, r in sorted(homophones) + sorted(from_signals))
    with open(os.path.join(OUT, a.name + "_signals.txt"), "w", encoding="utf-8", newline="\n") as f:
        f.write(f"# {a.field}の目印の語 (luna_signals.py、gpt-6.1-sol)\n")
        f.writelines(w + "\n" for w in signals)
    print(f"{a.name}: boost {len(homophones)} + {len(from_signals)} (signals with dictionary readings), signals {len(signals)}")


if __name__ == "__main__":
    main()
