"""2026-11-07 C: 仕事 44。手元の Wikipedia の全文 (wikimedia/wikipedia 20231101.ja の parquet) から、人物とグループの記事の書き出しを拾う。
- 人物: 「米津 玄師（よねづ けんし、1991年3月10日 - ）は、日本の…」。括弧の頭がかなの読みで、そのあとに年がある。姓と名の区切り (空白) を残す
- 1 語の名前: 「あいみょん（1995年3月6日 - ）は、日本の…」。名前がかなだけで、括弧の頭が年
- グループ: 「嵐（あらし）は、日本の男性アイドルグループ」。括弧が読みだけで、は、のあとにグループ・バンド・ユニット・コンビ・トリオ
題に括弧の注 (曖昧さ回避など) がある記事は、題の括弧の前と書き出しの名前が同じときだけ。読みの取れない記事は入れない。
出力 TSV: 記事の番号<TAB>題<TAB>表記<TAB>読み (ひらがな)<TAB>姓名の区切り (表記|読み)<TAB>種類<TAB>は、のあとの最初の文 (60 字まで)<TAB>自分の記事の中で表記が出た回数
使い方: python -I extract_names.py <out.tsv> <parquet ...> [--row-groups N (試しに頭の N 塊だけ)]
2026-11-07 09:4x (まとめ役 09:33、仕事 44 の上場企業): --titles <記事名の表 (1 列目)> --companies-out <out.tsv> で、表の記事 (Wikidata の上場企業) の
書き出しも同じ読み通しで拾う (重い読み通しを 1 回で済ませるため)。会社の出力 TSV: 記事の番号<TAB>題<TAB>書き出しの名前 (括弧の前)<TAB>括弧の中 (120 字まで)
<TAB>は、のあと (60 字まで。形が合わないときは書き出しの 160 字)<TAB>自分の記事の中で題が出た回数。読みの決め方は昼に (company_readings.py)"""
import re, sys
import pyarrow.parquet as pq

def opt(name):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else None
titles_path, companies_out = opt("--titles"), opt("--companies-out")
args = [a for a in sys.argv[1:] if not a.startswith("--") and a not in (titles_path, companies_out)]
company_titles = {l.split("\t", 1)[0].strip() for l in open(titles_path, encoding="utf-8") if not l.startswith("#") and l.strip()} if titles_path else set()
P_COMPANY = re.compile(r"^(?P<name>[^（()\n]{1,60}?)（(?P<inner>[^）]{0,120})）は、(?P<desc>[^。\n]{0,80})")
companies = []
rg_limit = int(sys.argv[sys.argv.index("--row-groups") + 1]) if "--row-groups" in sys.argv else 0
if rg_limit:
    args = [a for a in args if a != str(rg_limit)]
out, files = args[0], args[1:]
KANA = r"[ぁ-ゖァ-ヺー・ 　]"
P_PERSON = re.compile(r"^(?P<name>[^（()\n]{1,24}?)（(?P<yomi>[ぁ-ゖー・ 　]{2,40})[、,，](?P<rest>[^）]{0,120})）は、(?P<desc>[^。\n]{0,80})")
P_MONO = re.compile(r"^(?P<name>[ぁ-ゖァ-ヺー・]{2,20})（(?P<rest>[^）]{0,120})）は、(?P<desc>[^。\n]{0,80})")
P_GROUP = re.compile(r"^(?P<name>[^（()\n]{1,24}?)（(?P<yomi>[ぁ-ゖー・ 　]{2,40})）は、(?P<desc>[^。\n]{0,80})")
YEAR = re.compile(r"\d{1,4}年")
GROUP_WORDS = ("グループ", "バンド", "ユニット", "コンビ", "トリオ")


def hira(s):
    return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)


rows = []
for path in files:
    pf = pq.ParquetFile(path)
    n_rg = pf.metadata.num_row_groups if not rg_limit else min(rg_limit, pf.metadata.num_row_groups)
    for g in range(n_rg):
        t = pf.read_row_group(g, columns=["id", "title", "text"])
        ids, titles, texts = t.column("id").to_pylist(), t.column("title").to_pylist(), t.column("text").to_pylist()
        for aid, title, text in zip(ids, titles, texts):
            head = (text or "")[:300].split("\n", 1)[0]
            if title in company_titles:
                mc = P_COMPANY.match(head)
                companies.append((aid, title, mc.group("name").strip() if mc else "", mc.group("inner") if mc else "", (mc.group("desc")[:60] if mc else head[:160]),
                                  str((text or "").count(title))))
            base = re.sub(r"\s*[（(][^）)]*[）)]$", "", title)
            kind = m = None
            m = P_PERSON.match(head)
            if m and YEAR.search(m.group("rest")):
                kind = "人物"
            else:
                m = P_MONO.match(head)
                if m and YEAR.match(m.group("rest").strip()):
                    kind = "1 語の名前"
                else:
                    m = P_GROUP.match(head)
                    if m and any(w in m.group("desc")[:40] for w in GROUP_WORDS):
                        kind = "グループ"
                    else:
                        m = None
            if not m:
                continue
            name = m.group("name").strip()
            surface = re.sub(r"[ 　]", "", name)
            if surface != re.sub(r"[ 　]", "", base):
                continue
            if kind == "1 語の名前":
                yomi = hira(name).replace("・", "")
                split = ""
            else:
                yomi_sp = m.group("yomi").strip()
                yomi = re.sub(r"[ 　・]", "", yomi_sp)
                split = f"{name}|{yomi_sp}" if re.search(r"[ 　]", name) and re.search(r"[ 　]", yomi_sp) else ""
            if not re.fullmatch(r"[ぁ-ゖー]{2,40}", yomi):
                continue
            rows.append((aid, title, surface, yomi, split, kind, m.group("desc")[:60], str((text or "").count(surface))))
with open(out, "w", encoding="utf-8") as f:
    f.write("# 記事の番号\t題\t表記\t読み\t姓名の区切り (表記|読み)\t種類\tは、のあと\t自分の記事の中で表記が出た回数\n")
    for r in rows:
        f.write("\t".join(r) + "\n")
print(len(rows), "rows")
if companies_out:
    with open(companies_out, "w", encoding="utf-8") as f:
        f.write("# 記事の番号\t題\t書き出しの名前 (括弧の前)\t括弧の中\tは、のあと\t自分の記事の中で題が出た回数\n")
        for r in companies:
            f.write("\t".join(x.replace("\t", " ").replace("\n", " ") for x in r) + "\n")
    print(len(companies), "companies")
