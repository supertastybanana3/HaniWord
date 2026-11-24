"""2026-11-07 C: 仕事 43 (流行語)。ウィキペディア「新語・流行語大賞」の記事 (wikitext、版は article.meta.json) の表から、受賞語の候補を拾う。
表: 金賞・金賞以外の各賞 (1984〜1990)・年間大賞・年間大賞以外の受賞語 (トップテン選出以前・以後)。一覧は候補の語を出すのに使うだけで、
入れるかどうかは閲覧数・全文の数で決める (選び方を写さない)。
出力 TSV: 年<TAB>賞<TAB>語 (表示の字。「」を除く)<TAB>リンク先の記事 (無ければ空)<TAB>括弧の読み (語のすぐあとのかなの括弧)<TAB>受賞者 (リンク先の記事名 | 区切り)
使い方: python3 -I parse_awards.py <article.wikitext> <out.tsv>"""
import re, sys
src, out = sys.argv[1:3]
text = open(src, encoding="utf-8").read()
def clean(s):
    s = re.sub(r"<ref[^>]*/>|<ref[^>]*>.*?</ref>", "", s, flags=re.S)
    while True:
        t = re.sub(r"\{\{[^{}]*\}\}", "", s)
        if t == s: break
        s = t
    s = re.sub(r"<!--.*?-->", "", s, flags=re.S)
    return s.strip()
def links(s): return re.findall(r"\[\[([^\]|#]+)(?:#[^\]|]*)?(?:\|([^\]]*))?\]\]", s)
def display(s): return re.sub(r"\[\[(?:[^\]|]*\|)?([^\]]*)\]\]", r"\1", s)
rows = []
# 見出しの行ごとに分ける (見出しが 2 つ続く「年間大賞以外の受賞語」→「トップテン選出以前」でも、あとの見出しの表を落とさない)
sections = re.split(r"\n(?=={2,5}[^=\n]+={2,5}\n)", text)
cur = ""
for part in sections:
    if part.startswith("="):
        head, _, part = part.partition("\n")
        cur = head.strip("= ").strip()
    for tab in re.findall(r"\{\|.*?\n\|\}", part, flags=re.S):
        header = re.search(r"\n!(.*)", tab)
        cols = [c.strip() for c in re.split(r"!!", header.group(1))] if header else []
        year = award = ""
        pending = {}  # rowspan で続く列の値
        for row in re.split(r"\n\|-[^\n]*", tab)[1:]:
            cells_raw = re.split(r"\n\||\|\|", "\n" + row.strip().lstrip("|"))
            cells = []
            for c in cells_raw:
                c = c.strip()
                if not c or c.startswith("!") or c.startswith("}"): continue
                m = re.match(r'(?:(?:rowspan|nowrap|style|align)[^|]*)\|(.*)$', c, flags=re.S)
                if m and not c.startswith("[["): c = m.group(1).strip()
                cells.append(c)
            if not cells: continue
            # 年の列 ([[1994年]]) があれば、その行から年が変わる。回 (数字)・年・部門 (新語・流行語・表現)・賞 (金賞・トップテン など) の列を除いた残りが
            # 受賞語と受賞者 (表によって列の数と rowspan が違うので、位置ではなく中身で見分ける)
            rest = []
            for c in cells:
                y = re.search(r"\[\[(\d{4})年\]\]", c)
                if y: year = y.group(1); continue
                d = clean(display(c))
                if re.fullmatch(r"\d+", d): continue
                if d in ("新語", "流行語", "表現", "大衆", "大衆語", "特別", "特別語", "新語部門", "流行語部門"): continue
                if re.fullmatch(r"(トップテン|年間大賞|[^\s]{0,12}賞)", d) and not c.startswith("[["): award = d; continue
                rest.append(c)
            if not rest: continue
            # 受賞語と受賞者はどの表でも右の 2 列 (部門の名前 大衆・特別 などが残っても、右の 2 つを取る)。1 つだけなら受賞者は上の行と同じ (rowspan)
            word_cell, who_cell = (clean(rest[-2]), clean(rest[-1])) if len(rest) > 1 else (clean(rest[0]), "")
            who = "|".join(t for t, _ in links(who_cell)) or display(who_cell)
            for piece in re.split(r"<br\s*/?>|、(?=「)|(?<!\[)[/／](?![^\[]*\]\])", word_cell):
                piece = piece.strip()
                if not piece: continue
                lk = links(piece)
                w = display(piece).strip().strip("「」『』").strip()
                # リンク先の記事: 語と同じ名前の記事にリンクしているときだけ (別の記事へのリンク [[おしん|オシンドローム]] は語の記事ではない)
                target = lk[0][0].strip() if lk and re.sub(r"\s*\([^)]*\)$", "", lk[0][0].strip()) == w.strip("「」") else ""
                yomi = ""
                m = re.match(r"^(.+?)[（(]([ぁ-ゖァ-ヺー・ ]+)[）)]$", w)
                if m: w, yomi = m.group(1).strip("「」").strip(), m.group(2)
                rows.append((year, award or cur, w, target, yomi, who))
with open(out, "w", encoding="utf-8") as f:
    f.write("# 年\t賞\t語\tリンク先の記事\t括弧の読み\t受賞者\n")
    for r in rows: f.write("\t".join(r) + "\n")
print(len(rows), "rows", len({r[2] for r in rows}), "words")
