"""azooKey の辞書 (Dictionary/louds/*.loudstxt3) を全部読んで TSV に書き出す (2026-10-21、M3.5)。
出力: 読み(カタカナ)<TAB>表記<TAB>lcid<TAB>rcid<TAB>mid<TAB>点数
形式は AzooKeyKanaKanjiConverter の LOUDS.parseBinary / getDataForLoudstxt3 と同じ
使い方: python3 dump_azookey_dict.py <stage の Dictionary/louds> <作業の置き場>/azookey_dict.tsv"""
import struct, sys
from pathlib import Path
src, out = Path(sys.argv[1]), sys.argv[2]
n = 0
with open(out, 'w', encoding='utf-8') as f:
    for path in sorted(src.glob('*.loudstxt3')):
        b = path.read_bytes()
        lc = struct.unpack_from('<H', b, 0)[0]
        offs = list(struct.unpack_from(f'<{lc}I', b, 2))
        for k in range(lc):
            s = offs[k]; e = offs[k + 1] if k + 1 < lc else len(b)
            blk = b[s:e]
            cnt = struct.unpack_from('<H', blk, 0)[0]
            ids = [struct.unpack_from('<HHHf', blk, 2 + 10 * i) for i in range(cnt)]
            texts = blk[2 + 10 * cnt:].split(b'\t')
            ruby = texts[0].decode('utf-8', 'replace')
            for i, (l, r, m, v) in enumerate(ids):
                w = texts[i + 1].decode('utf-8', 'replace') if i + 1 < len(texts) else ''
                f.write(f"{ruby}\t{w or ruby}\t{l}\t{r}\t{m}\t{v:.3f}\n"); n += 1
print(n)
