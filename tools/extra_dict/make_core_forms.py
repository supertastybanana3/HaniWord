"""missing_forms.tsv (活用形の材料) を、オフにできない追加辞書の形 (表記・読み・cid・mid・値・出典) にする (2026-10-22、M3.5 の A)
使い方: python3 make_core_forms.py <作業の置き場>/missing_forms.tsv <リポジトリ>/extra_dictionaries/core_forms.tsv"""
import sys
src, out = sys.argv[1:3]
rows = set()
for l in open(src, encoding="utf-8"):
    if not l.strip() or l.startswith("#"):
        continue
    ruby, word, lc, rc, mid, v, base, model = l.rstrip("\n").split("\t")
    assert lc == rc, l   # 活用形は左右の品詞が同じ (1 つの形) だけを作っている
    rows.add((word, ruby, int(lc), int(mid), float(v), f"Mozc 公開辞書 (IPAdic)・{base} の活用形 (手本 {model})"))
with open(out, "w", encoding="utf-8") as f:
    f.write("# 表記\t読み\tcid\tmid\t値\t出典\n" + "".join("\t".join(map(str, r)) + "\n" for r in sorted(rows)))
print(len(rows))
