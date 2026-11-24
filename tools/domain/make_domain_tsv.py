"""luna で決めた読み (luna_readings.py merge) から、分野辞書のデータ (domain_dictionaries/<名前>.tsv) を作る。

  prepare : 入れる語 (keep=1) の 読み<TAB>表記 を書き出す (convert.py で今の変換を見るため。読みが複数あれば全部)
  natural : convert.py の結果から、表記がそのまま出なかったものを luna_natural.py の入力 (読み<TAB>今の変換<TAB>表記) にする
  build   : 品詞・意味の番号と重みを決めて <名前>.tsv を書く
  drop-same: 今の変換と同じ語の書き方違いで、分野でも今の変換の書き方がふつうな語 (luna_same_word.py で判定) を <名前>.tsv から除く
            - 品詞・意味の番号: 変換器の辞書に同じ表記・同じ読みがあればそれ、無ければ語の後ろの部分 (2 字以上で辞書にある最長のもの。
              即時抗告 → 抗告) の番号 (サ変の名詞なら「〜する」とつながる)。どれも無ければ一般名詞 (1285)・意味 501
            - 重み: 今の変換が自然な日本語 (luna_natural で 1) なら --natural-value (候補には出るが 1 位は今のまま)。
              そのときぶつかる語が辞書の 1 語なら、品詞・意味の番号もその語に合わせる。変換器の辞書に送り仮名違いがあれば入れない。
              意味の通らない切れ端なら --broken-value (分野の語が勝つ)。そのまま出る読みは入れない
            calibrate.py が、作ったあと全部の語を変換し直して重みを詰める
使い方: python3 make_domain_tsv.py prepare --merged <merged.tsv> --out <readings.tsv>
        python3 make_domain_tsv.py natural --probe <convert の出力.jsonl> --out <natural_in.tsv>
        python3 make_domain_tsv.py build --merged ... --probe ... --natural <luna_natural の出力> --dict <変換器の辞書の全件> --sources <表記<TAB>出典> --out <名前>.tsv
        python3 make_domain_tsv.py drop-same --same <luna_same_word の出力> --out <名前>.tsv
"""
import argparse, collections, json, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common


def merged_rows(path):
    for line in open(path, encoding="utf-8"):
        if line.startswith("#") or not line.strip():
            continue
        c = line.rstrip("\n").split("\t")
        if len(c) < 5:
            print("skipped a broken line:", line[:60].rstrip(), file=sys.stderr)  # 列が足りない (前の行の欄に改行が入っていた)
            continue
        term, reading, alt, kind, keep = common.normalize_term(c[0]), c[1], [x for x in c[2].split(",") if x], c[3], c[4] == "1"
        if keep and term and len(term) >= 2:
            yield term, [reading] + [x for x in alt if x != reading], kind


def load_probe(paths):
    probe = {}
    for path in paths:
        for line in open(path, encoding="utf-8"):
            try:
                r = json.loads(line)
            except ValueError:
                continue
            probe[(common.to_katakana(r["reading"]), r["expected"])] = r
    return probe


def read_exclude(path):
    """<名前>_exclude.txt (表記<TAB>理由) の表記。無ければ空"""
    if not path or not os.path.exists(path):
        return set()
    return {l.split("\t")[0].strip() for l in open(path, encoding="utf-8") if l.strip() and not l.startswith("#")}


def read_bad_readings(exclude_path):
    """<名前>_exclude.txt の隣の <名前>_bad_readings.txt (表記<TAB>読み (カタカナ)<TAB>理由) の (表記, 読み)。無ければ空。
    その読みの行だけ入れない (語のほかの読みは入れる)。Web で確かめる回の luna が付けた誤ったほかの読み (尾括式 ビカッコウシキ) を外す"""
    path = exclude_path and exclude_path.replace("_exclude.txt", "_bad_readings.txt")
    if not path or path == exclude_path or not os.path.exists(path):
        return set()
    return {tuple(c.strip() for c in l.split("\t")[:2]) for l in open(path, encoding="utf-8") if l.strip() and not l.startswith("#")}


def cmd_prepare(a):
    n = 0
    with open(a.out, "w", encoding="utf-8") as out:
        for term, readings, _ in merged_rows(a.merged):
            for r in readings:
                out.write(f"{r}\t{term}\n")
                n += 1
    print("readings", n)


def current_of(r, term):
    """2026-11-06: 分野辞書なしの今の変換で、その語の代わりに出る形。AI ありで出なければ AI ありの結果、AI ありでは出て辞書だけで外れれば
    辞書だけの結果 (ライブ変換の短い入力は辞書だけで変換する)。両方で出れば None (今の辞書で足りている)。辞書だけの結果が無い試しは AI ありだけで見る"""
    if r["full"] != term:
        return r["full"]
    d = r.get("dict", term)
    return None if d == term else d


def cmd_natural(a):
    probe = load_probe(a.probe)
    n = 0
    with open(a.out, "w", encoding="utf-8") as out:
        for (reading, term), r in sorted(probe.items()):
            cur = current_of(r, term)
            if cur is not None:
                out.write(f"{common.to_hiragana(reading)}\t{cur}\t{term}\n")
                n += 1
    print("natural cases", n)


def cmd_build(a):
    probe = load_probe(a.probe)
    natural = {}
    for line in open(a.natural, encoding="utf-8"):
        reading, current, term, nat = line.rstrip("\n").split("\t")
        natural[(common.to_katakana(reading), term)] = (nat == "1", current)
    by_word = collections.defaultdict(list)   # 表記 → [(読み, 品詞, 意味, 値)]
    by_reading = collections.defaultdict(list)  # 読み → [表記]
    for line in open(a.dict, encoding="utf-8"):
        w, r, l, rc, m, v = line.rstrip("\n").split("\t")
        by_word[w].append((r, int(l), int(m), float(v)))
        by_reading[r].append(w)
    sources = collections.defaultdict(set)
    for path in a.sources or []:
        for line in open(path, encoding="utf-8"):
            t, s = line.rstrip("\n").split("\t")[:2]
            sources[common.normalize_term(t) or t].add(s)  # 辞書の表記はそろえてあるので、出典の表も同じようにそろえて引く

    def ids(term, reading):
        same = [e for e in by_word.get(term, []) if e[0] == reading]
        if same:
            best = max(same, key=lambda e: e[3])
            return best[1], best[2], "same"
        for k in range(1, len(term) - 1):
            head = term[k:]
            if head in by_word:
                best = max(by_word[head], key=lambda e: e[3])
                return best[1], best[2], "head:" + head
        return 1285, 501, "default"

    def variant(current, term):
        """送り仮名・数字の書き方だけの違い (供託物払い渡し/供託物払渡、第1号/第一号) なら True。今の変換も正しい表記なので押しのけない"""
        def skeleton(x):
            x = x.translate(str.maketrans("0123456789", "〇一二三四五六七八九"))
            return "".join(c for c in x if not ("ぁ" <= c <= "ゖ"))
        return skeleton(current) == skeleton(term) and common.KANJI.search(current) is not None

    exclude, bad = read_exclude(a.exclude), read_bad_readings(a.exclude)
    stats = collections.Counter()
    rows = []
    for term, readings, kind in merged_rows(a.merged):
        if term in exclude:
            stats["excluded"] += 1
            continue
        for reading in readings:
            if (term, reading) in bad:
                stats["bad_reading"] += 1
                continue
            p = probe.get((reading, term))
            if p is None:
                stats["unprobed"] += 1
                continue
            cur = current_of(p, term)
            if cur is None:
                stats["already"] += 1
                continue
            if cur.replace("-", "") == term.replace("-", ""):
                stats["already_but_hyphen"] += 1  # ハイフンだけの違い (抗原-抗体複合体 と 抗原抗体複合体)。今の変換で足りる。中黒は法令名の正式な表記にあるので残す
                continue
            nat = natural.get((reading, term))
            if nat is None:
                stats["unjudged"] += 1
                continue
            if any(w != term and variant(w, term) for w in by_reading[reading]):
                stats["dict_has_variant"] += 1  # 変換器の辞書に送り仮名違いがある (仮差押 と 仮差押え)。文の中ではそちらが正しく出る
                continue
            if p["ok"] and nat[0]:
                # 2026-11-06: AI ありでは出て、辞書だけの変換も自然な別の語 (かがくしんか → 科学進化)。足しても重みの合わせで外れるだけなので足さない
                stats["already_ai_dict_natural"] += 1
                continue
            cid, mid, how = ids(term, reading)
            is_natural = nat[0] or variant(nat[1], term)
            if is_natural:
                # 一般の語とぶつかる項目は、ぶつかる語 (今の変換が辞書の 1 語なら) と同じ品詞・意味の番号にする。
                # つながり方の差が無くなり、重みだけで比べられる (文の中で 管理官室 が 監理官室 にならない)
                same = [e for e in by_word.get(nat[1], []) if e[0] == reading]
                if same:
                    best = max(same, key=lambda e: e[3])
                    cid, mid, how = best[1], best[2], "collides:" + nat[1]
            value = a.natural_value if is_natural else a.broken_value
            stats["natural" if nat[0] else "variant" if is_natural else "broken"] += 1
            rows.append((term, reading, cid, mid, value, "|".join(sorted(sources[term])) or "-", nat[1], int(is_natural), how))  # 記事名に使えない | で区切る
    with open(a.out, "w", encoding="utf-8") as out:
        out.write("# 表記\t読み\t品詞\t意味\t値\t出典\t今の変換 (分野辞書なし)\t今の変換が自然か\t番号の決め方\n")
        for row in sorted(rows, key=lambda r: (r[1], r[0])):
            out.write("\t".join(str(x) for x in row) + "\n")
    print(dict(stats), "entries", len(rows))


def cmd_drop_same(a):
    """luna_same_word.py で「今の変換と同じ語で、分野でも今の変換の書き方がふつう」とされた語を <名前>.tsv から除く"""
    drop = set()
    for line in open(a.same, encoding="utf-8"):
        if line.startswith("#"):
            continue
        term, reading, _, same, prefer = line.rstrip("\n").split("\t")
        if same == "1" and prefer == "current":
            drop.add((term, reading))
    lines = open(a.out, encoding="utf-8").readlines()
    kept = [l for l in lines if l.startswith("#") or tuple(l.split("\t")[:2]) not in drop]
    with open(a.out, "w", encoding="utf-8", newline="\n") as out:
        out.writelines(kept)
    print("dropped (same word, current notation is usual)", len(lines) - len(kept), "entries", sum(1 for l in kept if not l.startswith("#")))


def cmd_drop_bad(a):
    """<名前>_bad_readings.txt (luna_alt_check.py などで誤りとわかった読み) の (表記, 読み) の行を <名前>.tsv から除く"""
    bad = read_bad_readings(a.exclude)
    lines = open(a.out, encoding="utf-8").readlines()
    kept = [l for l in lines if l.startswith("#") or tuple(l.split("\t")[:2]) not in bad]
    with open(a.out, "w", encoding="utf-8", newline="\n") as out:
        out.writelines(kept)
    print("dropped (bad readings)", len(lines) - len(kept), "entries", sum(1 for l in kept if not l.startswith("#")))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["prepare", "natural", "build", "drop-same", "drop-bad"])
    ap.add_argument("--merged")
    ap.add_argument("--probe", nargs="*", default=[])
    ap.add_argument("--natural")
    ap.add_argument("--exclude", help="build・drop-bad: 入れない語の表 (domain_dictionaries/<名前>_exclude.txt。隣の <名前>_bad_readings.txt も読む)")
    ap.add_argument("--same", help="drop-same: luna_same_word.py の出力 (--out の TSV を書き換える)")
    ap.add_argument("--dict")
    ap.add_argument("--sources", nargs="*")
    ap.add_argument("--natural-value", type=float, default=-15.0)
    ap.add_argument("--broken-value", type=float, default=-8.0)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    {"prepare": cmd_prepare, "natural": cmd_natural, "build": cmd_build, "drop-same": cmd_drop_same, "drop-bad": cmd_drop_bad}[a.cmd](a)


if __name__ == "__main__":
    main()
