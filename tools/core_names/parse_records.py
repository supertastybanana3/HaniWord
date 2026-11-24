"""2026-11-07 C: 仕事 44 (競走馬)。記事の wikitext の戦績の表 (見出しの行に 競走名・格・着順 がある表) から、GI (国際の G1 を含む) と JpnI の
出走・勝利を数える。格の欄が GI・G1・GⅠ (国際GI も) なら GI、JpnI・Jpn1・JpnⅠ なら JpnI。着順の欄が 1着 (か 1) なら勝ち。
出力 TSV: 題<TAB>GI 出走<TAB>GI 勝ち<TAB>JpnI 出走<TAB>JpnI 勝ち<TAB>戦績の表の行の数<TAB>GI を勝った競走 (| 区切り)<TAB>版
使い方: python3 -I parse_records.py <wikitext.jsonl> <out.tsv>"""
import json, re, sys
src, out = sys.argv[1:3]
def clean(s):
    s = re.sub(r"<ref[^>]*/>|<ref[^>]*>.*?</ref>", "", s, flags=re.S)
    # 格の template ({{JRAGI}}・{{GI}}・{{G1}}・{{JpnI}}) は名前を格として読む (JRA・NAR・国際 は除く)
    s = re.sub(r"\{\{(?:JRA|NAR|国際)?((?:G|Jpn)(?:I{1,3}|[123]))\}\}", r"\1", s)
    for _ in range(4):
        s = re.sub(r"\{\{(?:color|colour|font color)\|[^|{}]*\|([^{}]*)\}\}", r"\1", s, flags=re.I)
        s = re.sub(r"\{\{(?:0|00|nowrap|small|big)\}\}", "", s, flags=re.I)
        s = re.sub(r"\{\{0\|[^{}]*\}\}", "", s)
        s = re.sub(r"\{\{(?:nowrap|small|big|sup|sub)\|([^{}]*)\}\}", r"\1", s, flags=re.I)
    s = re.sub(r"\[\[(?:[^\]|]*\|)?([^\]]*)\]\]", r"\1", s)
    s = re.sub(r"'''?|<br\s*/?>|<[^>]+>", " ", s)
    return s.strip()
def cells_of(line):
    line = line.lstrip("|!")
    out = []
    for c in re.split(r"\|\||!!", line):
        m = re.match(r'^\s*(?:(?:style|align|rowspan|colspan|bgcolor|class|width)[^|]*)\|(.*)$', c)
        span = re.search(r'colspan\s*=\s*"?(\d+)', c)
        text = m.group(1) if m else c
        out.extend([clean(text)] * (int(span.group(1)) if span else 1))
    return out
G1 = re.compile(r"^(国際)?(GI|G1|GⅠ|G I)$"); J1 = re.compile(r"^(JpnI|Jpn1|JpnⅠ|Jpn I)$")
with open(out, "w", encoding="utf-8") as f:
    f.write("# 題\tGI 出走\tGI 勝ち\tJpnI 出走\tJpnI 勝ち\t戦績の表の行の数\tGI・JpnI を勝った競走\t版\n")
    for l in open(src, encoding="utf-8"):
        r = json.loads(l); t = r["wikitext"]
        g_run = g_win = j_run = j_win = nrows = 0; won = []
        for tab in re.findall(r"\{\|.*?\n\|\}", t, flags=re.S):
            # 見出し: 競走名 と 着順 を含む行のまとまりの「!」の行を全部つなぐ (1 行に !! で並べる書き方と、1 欄ずつ ! の行にする書き方がある)
            cols = None
            for seg in re.split(r"\n\|-[^\n]*", tab):
                hl = [ln for ln in seg.split("\n") if ln.startswith("!")]
                if hl and "競走名" in "".join(hl) and "着順" in "".join(hl):
                    cols = [c for ln in hl for c in cells_of(ln)]
                    break
            if not cols: continue
            try: ic, ig, ip = cols.index("競走名"), cols.index("格"), cols.index("着順")
            except ValueError: continue
            for row in re.split(r"\n\|-[^\n]*", tab)[1:]:
                cells = []
                for ln in row.split("\n"):
                    if ln.startswith("|") and not ln.startswith(("|}", "|-")): cells.extend(cells_of(ln))
                if len(cells) <= max(ic, ig, ip) or any(x.startswith("!") for x in cells): continue
                if "競走名" in cells: continue
                nrows += 1
                g, place, race = cells[ig].replace(" ", ""), cells[ip], cells[ic]
                win = bool(re.match(r"^1着|^1$", place.replace(" ", "")))
                if G1.match(g): g_run += 1; g_win += win; won += [race] if win else []
                elif J1.match(g): j_run += 1; j_win += win; won += [race] if win else []
        f.write("\t".join(map(str, [r["title"], g_run, g_win, j_run, j_win, nrows, "|".join(won[:12]), r.get("revid") or ""])) + "\n")
