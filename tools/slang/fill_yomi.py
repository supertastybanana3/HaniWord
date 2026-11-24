"""2026-11-07 C: 仕事 43。読みの無い行 (リダイレクトなど) に、記事の DEFAULTSORT (濁点を落とした並べ替えの鍵) から読みを付ける。
Sudachi の読み (C 単位) か azooKey の辞書の読みのうち、濁点・半濁点・小さい字を落とすと DEFAULTSORT と同じになるものを使う。合わなければ空のまま。
使い方: python -I fill_yomi.py <table.tsv> <src/articles.jsonl> <azookey_dict.tsv> <out.tsv (語・読み・出どころ)>"""
import json, re, sys, unicodedata
from sudachipy import dictionary, tokenizer
table, arts, dic, out = sys.argv[1:5]
rows = [l.rstrip("\n").split("\t") for l in open(table, encoding="utf-8") if not l.startswith("#")]
A = [json.loads(l) for l in open(arts, encoding="utf-8")]
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
SMALL = str.maketrans("ぁぃぅぇぉっゃゅょゎ", "あいうえおつやゆよわ")
def strip(s):
    s = unicodedata.normalize("NFD", hira(s))
    s = "".join(c for c in s if c not in "゙゚")
    return unicodedata.normalize("NFC", s).translate(SMALL).replace("ー", "")
need = {r[0] for r in rows if not r[1]}
dyomi = {}
for l in open(dic, encoding="utf-8"):
    c = l.rstrip("\n").split("\t")
    if len(c) >= 2 and c[1] in need: dyomi.setdefault(c[1], set()).add(hira(c[0]))
tok = dictionary.Dictionary(dict="full").create(); MODE = tokenizer.Tokenizer.SplitMode.C
with open(out, "w", encoding="utf-8") as f:
    f.write("# 語\t読み\t出どころ\tDEFAULTSORT\n")
    for a, r in zip(A, rows):
        if r[1]: continue
        w = r[0]
        m = re.search(r"\{\{(?:DEFAULTSORT|デフォルトソート):([^}]+)\}\}", a.get("intro", ""))
        key = m.group(1).strip() if m else ""
        cands = []
        sud = hira("".join(t.reading_form() for t in tok.tokenize(w, MODE)))
        cands.append((sud, "Sudachi の読み"))
        for y in sorted(dyomi.get(w, ())): cands.append((y, "azooKey の辞書の読み"))
        got = [(y, src) for y, src in cands if key and re.fullmatch(r"[ぁ-ゖー]+", y) and strip(y) == strip(key)]
        y, src = got[0] if got else ("", "合わない (Sudachi: " + sud + ")")
        f.write("\t".join([w, y, src, key]) + "\n")
