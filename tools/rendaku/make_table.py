"""2026-11-03: 連濁の形の読みの語の表 (rendaku_readings.tsv。変換サーバーの rendaku.swift が読む) を作る。
候補: azooKey の辞書で、頭の 1 字が濁った読みの行が一般名詞・サ変接続の名詞 (左右の品詞 1285・1283) で、同じ表記に頭の 1 字の濁らない読みの行もある語
  (車 ぐるま／くるま、桜 ざくら／さくら)。接尾の品詞の行 (川 がわ・箱 ばこ) は複合語の後ろに付く使い方なので候補にしない。
コーパスで確かめる: Sudachi (C 単位) で文を区切り、候補の表記の語が助詞のすぐ後ろか文の頭に単独で来たときの読みを数え、
濁った読みがほとんど出ない (割合 1% 未満・3 回未満) ものだけを表にする (側 がわ のように単独でも濁る語は落ちる)。コーパスに出ない語も表に入れる。
  <Sudachi の python> -I make_table.py <azookey_dict.tsv> <出力.tsv> <コーパス (ID<TAB>文) ...> --kanjidic <kanjidic2.xml.gz> [--every N] [--jobs N]
出力: 読み (カタカナ)<TAB>表記。# の行に数の説明。別に <出力>.counts.tsv (読み・表記・濁った読みの単独の回数・濁らない読みの単独の回数・入れたか)"""
import collections, multiprocessing, re, sys
args = sys.argv[1:]
opt = {}
kanjidic = None
if "--kanjidic" in args:
    i = args.index("--kanjidic"); kanjidic = args[i + 1]; del args[i:i + 2]
for k in ("--every", "--jobs"):
    if k in args:
        i = args.index(k); opt[k] = int(args[i + 1]); del args[i:i + 2]
azd, out, *corpora = args
EVERY, JOBS = opt.get("--every", 1), opt.get("--jobs", 4)
V2U = dict(zip("ガギグゲゴザジズゼゾダヂヅデドバビブベボパピプペポ", "カキクケコサシスセソタチツテトハヒフヘホハヒフヘホ"))
# 2026-11-03: 訓読みの連濁 (ぐるま・ざくら・がみ・だな・ぎ・ど) だけにする。漢字 1 字の語は、濁らない読みが KANJIDIC2 の訓読みのときだけ
# (分 ふん・道 とう・善 せん のような音読みの濁りは入れない。木 き・戸 と・葉 は は入れる)。送り仮名のある語 (払い) と 2 字以上の語はコーパスの数えで決める
# (前の「2 字以上・終わりが ン・ウ・イ・ク・ツ・キ・チ でない」の決めつけの規則は、払い ばらい・靴 ぐつ・木 ぎ などの本当の連濁を落としたのでやめた)
hira = lambda s: "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
kun = collections.defaultdict(set)  # 漢字 -> 訓読み (ひらがなの語幹, 送り仮名)。か.ける → ("か", "ける")
if kanjidic:
    import gzip, xml.etree.ElementTree as ET
    for _, el in ET.iterparse(gzip.open(kanjidic)):
        if el.tag == "character":
            lit = el.findtext("literal")
            for r in el.iter("reading"):
                if r.get("r_type") == "ja_kun" and r.text:
                    stem, _, oku = r.text.strip("-").partition(".")
                    kun[lit].add((stem, oku))
            el.clear()
def kun_like(w, unvoiced):
    if len(w) != 1 or not kun:
        return True
    # 訓読みそのもの (木 き) か、送り仮名のある訓読みの語幹で始まり残りが 3 字まで (返 かえし ← かえ.す、掛 かけ ← か.ける)
    u = hira(unvoiced)
    return any(u == stem or u == stem + oku or (oku and u.startswith(stem) and len(u) - len(stem) <= 3) for stem, oku in kun.get(w, set()))
rows = collections.defaultdict(dict)
for l in open(azd, encoding="utf-8"):
    p = l.rstrip("\n").split("\t")
    if len(p) == 6 and re.search(r"[一-鿿]", p[1]):
        rows[p[1]].setdefault(p[0], set()).add((int(p[2]), int(p[3])))
cands = {}
on_like = {}  # 漢字 1 字で、濁らない読みが訓読みでない (音読みの濁り) ので入れない
for w, d in rows.items():
    for r, ids in d.items():
        if r and r[0] in V2U and any(lc == rc and lc in (1283, 1285) for lc, rc in ids) and V2U[r[0]] + r[1:] in d:
            if kun_like(w, V2U[r[0]] + r[1:]):
                cands[(r, w)] = V2U[r[0]] + r[1:]
            else:
                on_like[(r, w)] = V2U[r[0]] + r[1:]
words = {w for _, w in cands}
WORDS_RE = re.compile("|".join(sorted(map(re.escape, words), key=len, reverse=True)))

tok = None
def work(lines):
    global tok
    if tok is None:
        from sudachipy import dictionary, tokenizer
        tok = (dictionary.Dictionary(dict="core").create(), tokenizer.Tokenizer.SplitMode.C)
    t, mode = tok
    cnt = collections.Counter()
    for l in lines:
        s = l.rstrip("\n").split("\t", 1)[-1][:2000]
        if not s or not WORDS_RE.search(s):
            continue
        try:
            ms = list(t.tokenize(s, mode))
        except Exception:
            continue
        for i, m in enumerate(ms):
            w = m.surface()
            if w not in words:
                continue
            prev = ms[i - 1] if i > 0 else None
            if prev is not None and prev.part_of_speech()[0] not in ("助詞", "補助記号"):
                continue
            cnt[(m.reading_form(), w)] += 1
    return cnt

def chunks():
    for path in corpora:
        buf = []
        for i, l in enumerate(open(path, encoding="utf-8", errors="replace")):
            if i % EVERY:
                continue
            buf.append(l)
            if len(buf) >= 5000:
                yield buf; buf = []
        if buf:
            yield buf

if __name__ == "__main__":
    total = collections.Counter()
    with multiprocessing.Pool(JOBS) as pool:
        for c in pool.imap_unordered(work, chunks(), chunksize=1):
            total.update(c)
    kept = []
    with open(out + ".counts.tsv", "w", encoding="utf-8") as f:
        f.write("# 読み\t表記\t濁った読みで単独 (助詞のあと・文の頭)\t濁らない読みで単独\t入れたか\n")
        for (r, w), u in sorted(cands.items()):
            v, n = total[(r, w)], total[(u, w)]
            keep = v < 3 and v <= 0.01 * (v + n)
            if keep:
                kept.append((r, w))
            f.write(f"{r}\t{w}\t{v}\t{n}\t{'入れる' if keep else ''}\n")
        for (r, w), u in sorted(on_like.items()):
            f.write(f"{r}\t{w}\t-\t-\t入れない (1 字で、濁らない読み {u} が訓読みでない)\n")
    with open(out, "w", encoding="utf-8") as f:
        f.write(f"# 連濁の形の読みの語 (読み<TAB>表記)。tools/rendaku/make_table.py が azooKey の辞書とコーパスから作る。候補 {len(cands)}・入れた {len(kept)}。出典は extra_dictionaries/LICENSE.txt\n")
        for r, w in kept:
            f.write(f"{r}\t{w}\n")
    print("候補", len(cands), "入れた", len(kept), "音読みの濁りで入れない", len(on_like))
