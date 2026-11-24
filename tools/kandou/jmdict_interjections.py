"""2026-11-04: JMdict (CC BY-SA 4.0) の感動詞 (sense の pos に interjection がある項目) の読みを書き出す (build_table.py の材料)。
  python3 -I jmdict_interjections.py <JMdict_e.gz> <出力 jmdict_interjections.tsv>
出力: 読み<TAB>漢字の書き方 (3 つまで、・ 区切り)<TAB>口語の印 (misc に colloquial があれば 口語)<TAB>項目番号"""
import gzip, sys, xml.etree.ElementTree as ET
src, out = sys.argv[1:3]
rows = []
# JMdict は DTD の実体 (&int; など) を使うので、展開した文字列 (interjection (kandoushi)) で見る
for _, el in ET.iterparse(gzip.open(src), events=("end",)):
    if el.tag != "entry":
        continue
    poss = [p.text or "" for s in el.iter("sense") for p in s.iter("pos")]
    if any("interjection" in p for p in poss):
        seq = el.findtext("ent_seq")
        kebs = [k.text for k in el.iter("keb")][:3]
        col = "口語" if any("colloquial" in (m.text or "") for s in el.iter("sense") for m in s.iter("misc")) else ""
        for r in el.iter("reb"):
            rows.append((r.text, "・".join(kebs), col, seq))
    el.clear()
with open(out, "w", encoding="utf-8", newline="\n") as f:
    f.write("# JMdict (CC BY-SA 4.0) の感動詞 (pos に interjection) の読み<TAB>漢字の書き方 (3 つまで)<TAB>口語の印<TAB>項目番号\n")
    for r in rows:
        f.write("\t".join(r) + "\n")
print("項目", len({r[3] for r in rows}), "読み", len(rows), "口語の印の項目", len({r[3] for r in rows if r[2]}))
