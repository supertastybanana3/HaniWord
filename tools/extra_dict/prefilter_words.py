"""分野の文などから拾った抜けの語の候補 (pick_used.py の出力) を、変換で確かめる前に絞る (2026-10-22、M3.5 の C)。
終わり (右の品詞) が名詞でない・表記がかなを含む・分野辞書 (domain_dictionaries) にある・もう確かめた組 (別の候補の表) にあるものを除く。
make_core_words.py と同じ決まりのうち、変換しなくても決まるものを先に当てて、変換の数を減らすだけ
出力: <out>.tsv (候補の行) と <out>_cases.tsv (読み<TAB>表記。conv_probe.py に渡す)
使い方: HANIWORD_BUILD=<作業場所> python3 prefilter_words.py <リポジトリ> <候補.tsv> <out の頭> [もう確かめた候補.tsv ...]"""
import collections, glob, os, sys
from common import mozc_iddef
repo, src, out = sys.argv[1:4]
iddef = mozc_iddef()
dom = set()
for p in glob.glob(os.path.join(repo, "domain_dictionaries", "*.tsv")):
    if p.endswith(("_boost.tsv", "_abbrev_sources.tsv")):
        continue
    for l in open(p, encoding="utf-8"):
        if l.startswith("#") or not l.strip():
            continue
        c = l.split("\t"); dom.add((c[1], c[0]))
checked = {tuple(l.split("\t")[:2]) for p in sys.argv[4:] for l in open(p, encoding="utf-8")}
why, keep = collections.Counter(), []
for l in open(src, encoding="utf-8"):
    c = l.rstrip("\n").split("\t"); ruby, word, rid = c[0], c[1], int(c[3])
    if iddef.get(rid, "").split(",")[0] != "名詞": why["終わりが名詞でない"] += 1; continue
    if any("ぁ" <= ch <= "ゖ" for ch in word): why["かなを含む"] += 1; continue
    if (ruby, word) in dom: why["分野辞書にある"] += 1; continue
    if (ruby, word) in checked: why["もう確かめた"] += 1; continue
    keep.append(l); why["確かめる"] += 1
open(out + ".tsv", "w", encoding="utf-8").writelines(keep)
open(out + "_cases.tsv", "w", encoding="utf-8").writelines(l.split("\t")[0] + "\t" + l.split("\t")[1] + "\n" for l in keep)
print(dict(why))
