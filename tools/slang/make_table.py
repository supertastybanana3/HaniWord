"""2026-11-07 C: 仕事 43。src/articles.jsonl (カテゴリの記事の題・版・冒頭) から、見出しの語・読み・意味の要点の表を作る。
見出しの語は記事の題から括弧の注 (「 (ネット用語)」) を除いたもの。読みは冒頭の「語（よみ）」のかなの括弧、無ければ空 (かなだけの語は表記をそのまま)。
リダイレクトの記事は、行き先を note に。出力 TSV: 語<TAB>読み<TAB>意味 (冒頭の最初の文、120 字まで)<TAB>版<TAB>版の日時<TAB>カテゴリ<TAB>note"""
import json, re, sys
rows = [json.loads(l) for l in open(sys.argv[1], encoding="utf-8")]
def kana_only(s): return bool(re.fullmatch(r"[ぁ-ゖァ-ヺー・〜]+", s))
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
out = []
for r in rows:
    word = re.sub(r"\s*\([^)]*\)$", "", r["title"])
    intro, note = r["intro"], ""
    m = re.match(r"#(?:REDIRECT|転送)\s*\[\[([^\]]+)\]\]", intro, re.I)
    if m:
        note = "リダイレクト → " + m.group(1); intro = ""
    text = re.sub(r"\{\{[^{}]*\}\}|<ref[^>]*>.*?</ref>|<ref[^>]*/>|'''|''", "", intro, flags=re.S)
    text = re.sub(r"\[\[(?:[^|\]]*\|)?([^\]]*)\]\]", r"\1", text)
    reading = ""
    m = re.search(re.escape(word) + r"\s*[（(]([^）)]{1,40})[）)]", text)
    if m:
        cand = re.split(r"[、,，/／]", m.group(1))[0].strip()
        if re.fullmatch(r"[ぁ-ゖァ-ヺー]+", cand): reading = hira(cand)
    if not reading and kana_only(word): reading = hira(word.replace("・", ""))
    first = re.split(r"(?<=。)", text.strip())[0].replace("\n", " ")[:120]
    out.append((word, reading, first, str(r["oldid"]), r["timestamp"] or "", r["category"].replace("Category:", ""), note))
with open(sys.argv[2], "w", encoding="utf-8") as f:
    f.write("# 語\t読み\t意味 (冒頭の最初の文)\t版 (oldid)\t版の日時\tカテゴリ\tnote\n")
    for o in out: f.write("\t".join(o) + "\n")
print(len(out), "rows", sum(1 for o in out if not o[1]), "no reading")
