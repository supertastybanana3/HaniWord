"""(2026-10-22、M3.5 の C: 使われている抜けの語) 文章に出てくる (読み, 表記) のうち、Mozc の公開辞書にあって azooKey の辞書に無いものを拾う。
文は Sudachi (full) の A 単位でつながった 1〜4 語と C 単位の語。読みは Sudachi の読み (カタカナ)。
入力: 一般の文 (wiki_luna_full.cases.tsv) と、記事の名前 (写経の全文の組 part1〜4)。出力: used.tsv (読み・表記・Mozc の lid・cost・出た記事)
使い方: HANIWORD_BUILD=<作業場所> <Sudachi のある Python> pick_used.py <作業の置き場 (mozc_missing.tsv のある所)> <out.tsv> [文の組 (cases.tsv) ...]
文の組を付けたら、一般の文の代わりにそれを使い、記事の名前は文の組のファイル名にする"""
import collections, glob, os, re, sys
from sudachipy import dictionary, tokenizer
from common import B, MOZC as M
work, out = sys.argv[1], sys.argv[2]
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
iddef = {int(l.split(" ", 1)[0]): l.rstrip("\n").split(" ", 1)[1] for l in open(f"{M}/id.def", encoding="utf-8")}
missing = {}
for l in open(os.path.join(work, "mozc_missing.tsv"), encoding="utf-8"):
    ruby, word, lid, rid, cost = l.rstrip("\n").split("\t")
    missing[(ruby, word)] = (int(lid), int(rid), int(cost))
groups = {}
for i in range(1, 5):
    for l in open(f"{B}/probe/e2e/results/night_installer-full800_part{i}.txt", encoding="utf-8-sig"):
        p = l.rstrip("\n").split("\t")
        if len(p) > 2: groups.setdefault(p[1], p[2])
sentences = [l.rstrip("\n").split("\t")[1] for l in open(f"{B}/data_ext/domain/eval/wiki_luna_full.cases.tsv", encoding="utf-8") if l.strip() and not l.startswith("#")]
if len(sys.argv) > 3:
    sentences, groups = [], {}
    for path in sys.argv[3:]:
        name = os.path.basename(path).split(".")[0]
        for l in open(path, encoding="utf-8"):
            if l.strip() and not l.startswith("#"):
                s = l.rstrip("\n").split("\t")[1]; sentences.append(s); groups.setdefault(s, name)
tok = dictionary.Dictionary(dict="full").create()
A, C = tokenizer.Tokenizer.SplitMode.A, tokenizer.Tokenizer.SplitMode.C
KANJI_KATA = re.compile(r"[一-鿿々〆ァ-ヺ]")
found = collections.defaultdict(collections.Counter)
for s in sentences:
    spans = set()
    a = [(m.surface(), m.reading_form()) for m in tok.tokenize(s, A)]
    for i in range(len(a)):
        for n in range(1, 5):
            if i + n <= len(a):
                spans.add(("".join(r for _, r in a[i:i + n]), "".join(w for w, _ in a[i:i + n])))
    spans |= {(m.reading_form(), m.surface()) for m in tok.tokenize(s, C)}
    for ruby, word in spans:
        if (ruby, word) not in missing or len(word) < 2 or not KANJI_KATA.search(word) or re.search(r"[0-9A-Za-z０-９Ａ-Ｚａ-ｚ]", word):
            continue
        pos = iddef.get(missing[(ruby, word)][0], "").split(",")[0]
        if pos in ("記号", "助詞", "助動詞", "フィラー", "その他"):
            continue
        found[(ruby, word)][groups[s]] += 1
with open(out, "w", encoding="utf-8") as f:
    for (ruby, word), arts in sorted(found.items()):
        lid, rid, cost = missing[(ruby, word)]
        f.write("\t".join([ruby, word, str(lid), str(rid), str(cost), iddef.get(lid, "?"), ";".join(f"{a}:{n}" for a, n in arts.most_common())]) + "\n")
print(len(sentences), "文", len(found), "語")
