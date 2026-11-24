"""2026-11-03: english_case.txt の ~ の行 (普通の英単語でもある名前。和文にすき間なくつながっているとき (windowsno・zoomde) だけ、その表記にする) を、
日本語版ウィキペディアの数えから作る。

数える所: 英字の並び (前後に英字・数字が続かないもの) のうち、すぐ前かすぐ後ろが和文の字 (ひらがな・カタカナ・漢字・々・ー) のもの。
語ごとに、大文字小文字の形 (LINE・Line・line) ごとの回数を数える。記事 ID の偶数 (dev) と奇数 (test) を分けて数える。
選ぶ語: 一番多い形が小文字だけでなく、その形の割合が --min-ratio 以上、回数が --min-count 以上 (dev で決め、test で確かめる)。

  数える:  python3 -I make_case_tilde.py count <英単語の表>... --sents <文の tsv>... --out <数えた結果.json>
  選ぶ:    python3 -I make_case_tilde.py pick <数えた結果.json>... --min-count N --min-ratio R [--half dev|test|all] [--case english_case.txt]
英単語の表: 1 行 1 語 (タブの後ろは無視、# の行は説明)。文の tsv: 記事 ID<TAB>文
数えた結果.json: {"dev": {語: {形: 回数}}, "test": {...}} (数えるのを何本かに分けて流したときは、pick にまとめて渡す)"""
import collections, json, re, sys

JAPANESE = "ぁ-ゖァ-ヺー々〆ヵヶ㐀-䶿一-鿿豈-﫿"
RUN = re.compile(r"(?<![A-Za-z0-9])[A-Za-z]+(?![A-Za-z0-9])")
JA = re.compile(f"[{JAPANESE}]")


def count(args):
    tables, sents, out = [], [], None
    mode = "tables"
    for a in args:
        if a == "--sents": mode = "sents"; continue
        if a == "--out": mode = "out"; continue
        if mode == "tables": tables.append(a)
        elif mode == "sents": sents.append(a)
        else: out = a
    words = set()
    for t in tables:
        for line in open(t, encoding="utf-8"):
            w = line.split("\t")[0].strip()
            if w and not w.startswith("#") and w.isascii() and w.isalpha():
                words.add(w.lower())
    counts = {"dev": collections.defaultdict(collections.Counter), "test": collections.defaultdict(collections.Counter)}
    for p in sents:
        for line in open(p, encoding="utf-8"):
            pid, _, s = line.rstrip("\n").partition("\t")
            half = "dev" if pid[-1:] in "02468" else "test"
            for m in RUN.finditer(s):
                w = m.group()
                lower = w.lower()
                if lower not in words: continue
                i, j = m.start(), m.end()
                if (i > 0 and JA.match(s[i - 1])) or (j < len(s) and JA.match(s[j])):
                    counts[half][lower][w] += 1
        print(p, flush=True)
    json.dump({h: {w: dict(c) for w, c in d.items()} for h, d in counts.items()}, open(out, "w", encoding="utf-8"), ensure_ascii=False)


def pick(args):
    files, min_count, min_ratio, half, case = [], 100, 0.9, "dev", None
    i = 0
    while i < len(args):
        a = args[i]
        if a == "--min-count": min_count = int(args[i + 1]); i += 2
        elif a == "--min-ratio": min_ratio = float(args[i + 1]); i += 2
        elif a == "--half": half = args[i + 1]; i += 2
        elif a == "--case": case = args[i + 1]; i += 2
        else: files.append(a); i += 1
    merged = collections.defaultdict(collections.Counter)
    for f in files:
        data = json.load(open(f, encoding="utf-8"))
        for h in (["dev", "test"] if half == "all" else [half]):
            for w, forms in data[h].items():
                merged[w].update(forms)
    current = set()
    if case:
        current = {l.strip()[1:] for l in open(case, encoding="utf-8") if l.startswith("~")}
    picked = []
    for w, forms in merged.items():
        total = sum(forms.values())
        form, n = forms.most_common(1)[0]
        if total >= min_count and form != w and n / total >= min_ratio:
            picked.append((form, n, total))
    picked.sort(key=lambda x: -x[2])
    for form, n, total in picked:
        print(f"~{form}\t{n}/{total}\t{n / total:.3f}" + ("\t(今の表)" if form in current else ""))
    missing = sorted(current - {f for f, _, _ in picked})
    print(f"# 選んだ {len(picked)} 語。今の表の ~ のうち選ばれなかった語: {' '.join(missing) if missing else 'なし'}", file=sys.stderr)
    for m in missing:
        forms = merged.get(m.lower(), {})
        print(f"#   {m}: {dict(collections.Counter(forms).most_common(4))}", file=sys.stderr)


if __name__ == "__main__":
    {"count": count, "pick": pick}[sys.argv[1]](sys.argv[2:])
