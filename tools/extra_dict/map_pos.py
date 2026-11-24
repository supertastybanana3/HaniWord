"""両方の辞書にある (読み, 表記) を突き合わせて、azooKey の品詞の番号ごとに Mozc の品詞の名前を当てる (2026-10-21、M3.5)。
azooKey と今の Mozc は品詞の番号の体系が違う (azooKey 1285 = 名詞一般、Mozc 1285 = 動詞非自立) ので、語で突き合わせる。
出力 <作業の置き場>/azk_pos.tsv: azooKey の左の番号<TAB>語の数<TAB>いちばん多い Mozc の品詞<TAB>その割合
使い方: HANIWORD_BUILD=<作業場所> python3 map_pos.py <作業の置き場 (azookey_dict.tsv のある所)>"""
import collections, glob, os, sys
from common import MOZC, hira
work = sys.argv[1]
iddef = {}
for l in open(f'{MOZC}/id.def', encoding='utf-8'):
    i, name = l.rstrip('\n').split(' ', 1); iddef[int(i)] = name
mozc = collections.defaultdict(set)
for path in sorted(glob.glob(f'{MOZC}/dictionary0*.txt')):
    for l in open(path, encoding='utf-8'):
        p = l.rstrip('\n').split('\t')
        if len(p) >= 5: mozc[(p[0], p[4])].add(int(p[1]))
pairs = collections.defaultdict(collections.Counter)
for l in open(os.path.join(work, 'azookey_dict.tsv'), encoding='utf-8'):
    ruby, word, lc, rc, mid, v = l.rstrip('\n').split('\t')
    for mid_ in mozc.get((hira(ruby), word), ()):
        pairs[int(lc)][iddef.get(mid_, '?')] += 1
with open(os.path.join(work, 'azk_pos.tsv'), 'w', encoding='utf-8') as f:
    for lc in sorted(pairs):
        c = pairs[lc]; name, k = c.most_common(1)[0]; tot = sum(c.values())
        f.write(f'{lc}\t{tot}\t{name}\t{k/tot:.2f}\n')
print(len(mozc), len(pairs))
