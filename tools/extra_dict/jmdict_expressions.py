"""JMdict (EDRDG、CC BY-SA 4.0。https://www.edrdg.org/edrdg/licence.html) の表現 (pos が exp)・慣用句 (misc が id) の、
表記と読みの組を取り出す (2026-10-22、M3.5 の B: 成句)。読みの制限 (re_restr) を守り、かなだけの読み (re_nokanji) は除く。
azooKey の辞書に同じ (読み, 表記) がある組は除く。
出力: <作業の置き場>/jm_cases.tsv (読み<TAB>表記。conv_probe.py に渡す) と jmdict_siblings.json (読み<TAB>表記 -> 同じ項目の、同じ読みの書き方と読み。JMdict の並び順)
使い方: python3 jmdict_expressions.py <JMdict_e.gz> <作業の置き場 (azookey_dict.tsv のある所)>"""
import collections, gzip, json, os, re, sys
from common import kata
src, work = sys.argv[1:3]
xml = gzip.open(src, "rt", encoding="utf-8").read()
rows, siblings = [], {}
for e in re.findall(r"<entry>(.*?)</entry>", xml, re.S):
    pos = set(re.findall(r"<pos>&([^;]+);</pos>", e)); misc = set(re.findall(r"<misc>&([^;]+);</misc>", e))
    if not ("exp" in pos or "id" in misc):
        continue
    kebs = re.findall(r"<keb>([^<]+)</keb>", e)
    if not kebs:
        continue
    pairs = []
    for r_ele in re.findall(r"<r_ele>(.*?)</r_ele>", e, re.S):
        reb = re.search(r"<reb>([^<]+)</reb>", r_ele).group(1)
        if "<re_nokanji/>" in r_ele:
            continue
        restr = re.findall(r"<re_restr>([^<]+)</re_restr>", r_ele)
        pairs += [(reb, k) for k in kebs if not restr or k in restr]
    for reb, k in pairs:
        rows.append((reb, k))
        # JMdict の並び順のまま (先の書き方ほどよく使う)
        siblings[f"{reb}\t{k}"] = list(dict.fromkeys([kk for rr, kk in pairs if rr == reb] + [rr for rr, _ in pairs]))
have = set()
for l in open(os.path.join(work, "azookey_dict.tsv"), encoding="utf-8"):
    p = l.split("\t"); have.add((p[0], p[1]))
seen, n_have = [], 0
for reb, k in rows:
    if (kata(reb), k) in have: n_have += 1; continue
    if (reb, k) not in seen: seen.append((reb, k))
with open(os.path.join(work, "jm_cases.tsv"), "w", encoding="utf-8") as f:
    f.writelines(f"{reb}\t{k}\n" for reb, k in seen)
json.dump(siblings, open(os.path.join(work, "jmdict_siblings.json"), "w", encoding="utf-8"), ensure_ascii=False)
print("組", len(rows), "azooKey にある", n_have, "確かめる", len(seen))
