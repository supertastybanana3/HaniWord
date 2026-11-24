"""2026-11-06 (仕事 39): ウィキペディア「日本語の方言の比較表」(CC BY-SA 4.0) の表 (wikitext) を行ごとにほどく。
列見出し (方言の名前) と、行の項目 (断定・否定 など) ごとの形を TSV に書く。colspan は同じ値をくり返し、rowspan は下の行に持ち越す。
使い方: python3 parse_table.py <記事.wikitext> <出力.tsv>"""
import re, sys
text = open(sys.argv[1], encoding="utf-8").read()
def clean(s):
    s = re.sub(r"<ref[^>]*/>|<ref[^>]*>.*?</ref>", "", s, flags=re.S)
    s = re.sub(r"\[\[(?:[^|\]]*\|)?([^\]]*)\]\]", r"\1", s)
    s = re.sub(r"'''?", "", s)
    s = re.sub(r"<br\s*/?>", "／", s)
    s = re.sub(r"<[^>]+>", "", s)
    s = re.sub(r"\{\{[^{}]*\}\}", "", s)
    return s.replace("&nbsp;", " ").strip()
def cells(line):
    kind = line[0]
    rest = line[1:]
    if rest[:1] == kind:  # 行頭が「!!」「||」(前の行の続きのセル)
        rest = rest[1:]
    parts = re.split(r"\|\||!!", rest)
    out = []
    for p in parts:
        m = re.match(r"\s*(?:td\s+)?((?:colspan|rowspan|style|class|align|bgcolor|width)\s*=[^|]*)\|(.*)$", p, re.S)
        attrs, body = (m.group(1), m.group(2)) if m and "[[" not in m.group(1) else ("", p)
        cs = int(re.search(r'colspan\s*=\s*"?(\d+)', attrs).group(1)) if "colspan" in attrs else 1
        rs = int(re.search(r'rowspan\s*=\s*"?(\d+)', attrs).group(1)) if "rowspan" in attrs else 1
        out.append((kind, clean(body), cs, rs))
    return out
rows_out = []
section = ""
for block in re.findall(r"(?:^==[^\n]*==\s*$)|(?:\{\|.*?\n\|\})", text, flags=re.S | re.M):
    if block.startswith("=="):
        section = block.strip("= \n"); continue
    rows, cur = [], []
    for line in block.split("\n")[1:]:
        if line.startswith("|-") or line.startswith("|}"):
            if cur: rows.append(cur)
            cur = []
        elif line.startswith("|+"):
            continue
        elif line.startswith("|") or line.startswith("!"):
            cur.extend(cells(line))
        elif cur:  # 前のセルの続き
            k, b, cs, rs = cur[-1]; cur[-1] = (k, b + " " + clean(line), cs, rs)
    if cur: rows.append(cur)
    # 表を格子に (rowspan を持ち越す)
    grid, carry = [], {}
    for r in rows:
        line, col, it = [], 0, iter(r)
        cellsq = list(r)
        while cellsq or col in carry:
            if col in carry:
                v, left = carry[col]; line.append(v)
                if left > 1: carry[col] = (v, left - 1)
                else: del carry[col]
                col += 1; continue
            k, b, cs, rs = cellsq.pop(0)
            for _ in range(cs):
                line.append(b)
                if rs > 1: carry[col] = (b, rs - 1)
                col += 1
        grid.append(line)
    # 方言の名前の行 (〜弁・〜言葉・〜ことば がいちばん多い行) と、まとまりの行 (〜方言 がいちばん多い行)
    top = grid[:8]
    head = max(top, key=lambda l: len({x for x in l if re.search(r"弁|言葉|ことば", x)}), default=[])
    group = max(top, key=lambda l: len({x for x in l if re.search(r"方言", x)}), default=[])
    for l in grid:
        if l is head or l is group or not l or not l[0]: continue
        for i, v in enumerate(l[1:], start=1):
            if i < len(head) and v and v != l[0]:
                rows_out.append((section, l[0].strip(), head[i], group[i] if i < len(group) else "", v))
with open(sys.argv[2], "w", encoding="utf-8") as f:
    f.write("# 区分\t項目\t方言\tまとまり\t形 (ウィキペディア「日本語の方言の比較表」CC BY-SA 4.0 から表をほどいたもの)\n")
    f.writelines("\t".join(r) + "\n" for r in rows_out)
print(len(rows_out), "rows")
