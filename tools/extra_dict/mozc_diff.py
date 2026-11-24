"""Mozc の公開辞書にあって azooKey の辞書に無い (読み, 表記) を数え、種類に分ける (2026-10-21、M3.5)。
出力 <作業の置き場>/mozc_missing.tsv: 読み (カタカナ)<TAB>表記<TAB>Mozc の左の品詞<TAB>右の品詞<TAB>コスト
使い方: HANIWORD_BUILD=<作業場所> python3 mozc_diff.py <作業の置き場 (azookey_dict.tsv のある所)>"""
import collections, glob, os, re, sys
from common import MOZC as M
work = sys.argv[1]
def kata(s): return ''.join(chr(ord(c) + 0x60) if 'ぁ' <= c <= 'ゖ' else c for c in s)
iddef = {int(l.split(' ', 1)[0]): l.rstrip('\n').split(' ', 1)[1] for l in open(f'{M}/id.def', encoding='utf-8')}
have = set()
for l in open(os.path.join(work, 'azookey_dict.tsv'), encoding='utf-8'):
    p = l.split('\t'); have.add((p[0], p[1]))
miss = {}
for path in sorted(glob.glob(f'{M}/dictionary0*.txt')):
    for l in open(path, encoding='utf-8'):
        p = l.rstrip('\n').split('\t')
        if len(p) < 5: continue
        k = (kata(p[0]), p[4])
        if k in have: continue
        if k not in miss or int(p[3]) < miss[k][2]: miss[k] = (int(p[1]), int(p[2]), int(p[3]))
kinds = collections.Counter(); ex = collections.defaultdict(list)
for (ruby, word), (lid, rid, cost) in miss.items():
    pos = iddef.get(lid, '?').split(',')
    if re.fullmatch(r'[ァ-ヺー・]+', word): k = 'カタカナだけ'
    elif '固有名詞' in pos: k = '固有名詞'
    elif pos[0] in ('動詞', '形容詞') and pos[5] != '基本形': k = '動詞・形容詞の活用形'
    elif pos[0] in ('動詞', '形容詞'): k = '動詞・形容詞の基本形'
    elif re.search(r'[をにがでとはも]', word) and lid != rid: k = '句 (助詞を含む)'
    elif pos[0] == '名詞': k = '名詞'
    else: k = 'ほか ' + pos[0]
    kinds[k] += 1
    if len(ex[k]) < 12: ex[k].append(f'{word}/{ruby}/{cost}')
print('Mozc にあって azooKey に無い', len(miss))
for k, n in kinds.most_common(): print(f'{k}\t{n}\t' + ' '.join(ex[k]))
with open(os.path.join(work, 'mozc_missing.tsv'), 'w', encoding='utf-8') as f:
    for (ruby, word), (lid, rid, cost) in sorted(miss.items()): f.write(f'{ruby}\t{word}\t{lid}\t{rid}\t{cost}\n')
