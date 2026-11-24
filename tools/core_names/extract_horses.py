"""2026-11-07 C: 仕事 44 (競走馬、まとめ役 11:41)。手元の Wikipedia の全文 (20231101) から競走馬の記事を拾う。
競走馬と分かる記事: 書き出しの「名前（…）は、」のあとの説明 (80 字まで) に 競走馬・種牡馬・繁殖牝馬 がある記事
出力 TSV: 記事の番号<TAB>題<TAB>名前 (括弧の前)<TAB>括弧の中<TAB>説明<TAB>本文の G1 の書き方の数 (GI・G1・GⅠ・JpnI・Jpn1・JpnⅠ)
使い方: python -I extract_horses.py <out.tsv> <parquet ...>"""
import re, sys
import pyarrow.parquet as pq
out, files = sys.argv[1], sys.argv[2:]
P = re.compile(r"^(?P<name>[^（()\n]{1,40}?)（(?P<inner>[^）]{0,160})）(?:[^は\n]{0,30})?は、(?P<desc>[^。\n]{0,80})")
G1 = re.compile(r"(GI(?![IV])|G1(?!\d)|GⅠ|JpnI(?![IV])|Jpn1(?!\d)|JpnⅠ)")
rows = []
for path in files:
    pf = pq.ParquetFile(path)
    for g in range(pf.metadata.num_row_groups):
        t = pf.read_row_group(g, columns=["id", "title", "text"])
        for aid, title, text in zip(t.column("id").to_pylist(), t.column("title").to_pylist(), t.column("text").to_pylist()):
            head = (text or "")[:400].split("\n", 1)[0]
            m = P.match(head)
            if not m or not re.search(r"(競走馬|種牡馬|繁殖牝馬)", m.group("desc")): continue
            rows.append((aid, title, m.group("name").strip(), m.group("inner")[:160], m.group("desc")[:80], str(len(G1.findall(text or "")))))
with open(out, "w", encoding="utf-8") as f:
    f.write("# 記事の番号\t題\t名前\t括弧の中\t説明\t本文の G1 の書き方の数\n")
    for r in rows: f.write("\t".join(x.replace("\t", " ") for x in r) + "\n")
print(len(rows), "horses")
