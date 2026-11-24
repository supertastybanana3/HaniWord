"""azooKey の辞書に終止形 (基本形) だけあって、活用形が無い動詞・形容詞を探し、同じ品詞の番号で同じかなで終わる語 (手本: 待つ) の
活用形にならって足りない形を作る。Mozc の公開辞書に同じ読み・表記がある形だけ残す (2026-10-21、M3.5 の A: 活用形)。
出力 <作業の置き場>/missing_forms.tsv: 読み(カタカナ)<TAB>表記<TAB>lcid<TAB>rcid<TAB>mid<TAB>点数<TAB>元の語<TAB>手本
使い方: HANIWORD_BUILD=<作業場所> python3 missing_forms.py <作業の置き場 (azookey_dict.tsv・azk_pos.tsv のある所)> [語 (手本を見る)]"""
import collections, glob, os, sys
from common import MOZC as M
work = sys.argv[1]
def hira(s): return ''.join(chr(ord(c) - 0x60) if 'ァ' <= c <= 'ヶ' else c for c in s)
label = {int(l.split('\t')[0]): l.split('\t')[2] for l in open(os.path.join(work, 'azk_pos.tsv'), encoding='utf-8')}
mozc = set()
for path in sorted(glob.glob(f'{M}/dictionary0*.txt')):
    for l in open(path, encoding='utf-8'):
        p = l.rstrip('\n').split('\t')
        if len(p) >= 5: mozc.add((p[0], p[4]))
entries = []
have = set()
for l in open(os.path.join(work, 'azookey_dict.tsv'), encoding='utf-8'):
    ruby, word, lc, rc, mid, v = l.rstrip('\n').split('\t')
    entries.append((ruby, word, int(lc), int(rc), int(mid), float(v))); have.add((ruby, word))
kanji = lambda s: any('\u4e00' <= c <= '\u9fff' for c in s)
# 基本形の語: 読みと表記の最後の 1 字が同じかな (待つ / マツ の つ)。語幹 = 最後の 1 字を除いたもの
is_base = lambda lc: ('動詞' in label.get(lc, '') or '形容詞' in label.get(lc, '')) and '基本形' in label.get(lc, '')
bases = [e for e in entries if is_base(e[2]) and kanji(e[1][:-1]) and len(e[1]) >= 2 and len(e[0]) >= 2 and hira(e[0][-1]) == e[1][-1]]
# 手本: 同じ lcid・同じ最後のかなの基本形で、同じ語幹から作られた形 (語幹 + かな) を集める
by_word_stem = collections.defaultdict(list)
for e in entries:
    for k in range(1, len(e[1])):
        by_word_stem[(e[1][:k], e[0][:max(0, len(e[0]) - (len(e[1]) - k))])].append(e)
# 活用の型 (五段・カ行イ音便・一段・形容詞・アウオ段 など。Mozc の品詞の 5 つ目の欄)。基本形と同じ型の形だけを、その語の活用形とみなす
ctype = lambda lc: (label.get(lc, '').split(',') + [''] * 7)[4]
def forms(b):
    ruby, word, lc, rc, mid, v = b
    ws, rs = word[:-1], ruby[:-1]
    return [e for e in by_word_stem.get((ws, rs), []) if e is not b and e[1].startswith(ws) and e[0].startswith(rs)
            and 1 <= len(e[1]) - len(ws) <= 2 and all('ぁ' <= c <= 'ゖ' for c in e[1][len(ws):]) and ctype(e[2]) == ctype(lc)
            and e[2] == e[3]]   # 左右の品詞が同じ = 1 つの活用形 (見ちゃう・見てるの のようにつながった形は真似しない)
templates = collections.defaultdict(list)
for b in bases:
    if not kanji(b[1][:-1]): continue
    fs = forms(b)
    templates[(b[2], b[1][-1])].append((len(fs), b, fs))
# 手本は、同じ品詞・同じ最後のかなの語のうち、いちばんよく使う語 (点数が高い。形が 3 つ以上)。形の数で選ぶと、語幹の字が別の動詞と
# 同じ語 (滾つ と 滾る) が選ばれて、ほかの動詞の形が混ざる
best = {k: max([x for x in v if x[0] >= 3] or v, key=lambda x: x[1][5]) for k, v in templates.items()}
out = []; per_base = collections.Counter()
for b in bases:
    if (b[2], b[1][-1]) not in best: continue
    n, t, tfs = best[(b[2], b[1][-1])]
    if n == 0: continue
    ws, rs = b[1][:-1], b[0][:-1]; tws, trs = t[1][:-1], t[0][:-1]
    for f in tfs:
        word = ws + f[1][len(tws):]; ruby = rs + f[0][len(trs):]
        if (ruby, word) in have or (hira(ruby), word) not in mozc: continue
        # 低い重み: もとの語より 1 低く、どれだけ高くても -10 まで (辞書だけの変換ではまず選ばず、AI が文脈で選べる程度)
        value = min(min(b[5] + (f[5] - t[5]), b[5]) - 1.0, -10.0)
        out.append((ruby, word, f[2], f[3], b[4], round(value, 3), b[1], t[1])); per_base[b[1]] += 1
# 同じ形 (読み・表記・品詞) は 1 つに (高いほう)
merged = {}
for o in out:
    k = o[:4]
    if k not in merged or o[5] > merged[k][5]: merged[k] = o
out = sorted(merged.values())
with open(os.path.join(work, 'missing_forms.tsv'), 'w', encoding='utf-8') as fo:
    for o in out: fo.write('\t'.join(map(str, o)) + '\n')
print('基本形', len(bases), '足す形', len(out), '足す語', len(per_base))
if len(sys.argv) > 2:
    for b in bases:
        if b[1] == sys.argv[2]:
            key = (b[2], b[1][-1]); print('base', b, 'template', best.get(key, (0, None, []))[1], [f[1] for f in best.get(key, (0, None, []))[2]])
