"""Wikidata (CC0) から、分野辞書の語の候補 (日本語のラベル・名前のカナ表記 P1814・日本語版ウィキペディアの記事名・国) を集める。
日本語版ウィキペディアに記事がある項目だけ (よく知られたものに絞るため)。クエリは 1 つずつ送り、User-Agent に連絡先を書く。
  persons : 人物 (Q5) のうち名前のカナ表記 (P1814) を持つもの (日本の人物)。生年で区切って取る (--from から --to まで、--step 年ずつ)
  things  : 戦い・戦争・反乱・条約・時代区分・王朝・歴史上の国・元号・歴史上の出来事など (--classes の Q 番号、下位の種類も含む)
  has_property : --properties の性質を持つ項目 (例: CAS 登録番号 P231 = 化学物質)
出力 (JSONL、1 行 1 項目): qid, label (日本語), kana (P1814), title (jawiki の記事名), country (国の Q 番号の一覧。人物は国籍 P27 と生まれた場所の国), kind,
  links (その項目の記事がある言語版の数。知られている度合いの目安)
使い方: python3 collect_wikidata.py persons <出力.jsonl> [--from -800 --to 1900 --step 25]
        python3 collect_wikidata.py things <出力.jsonl> [--classes Q178561 ...]
        python3 collect_wikidata.py has_property <出力.jsonl> --properties P231
"""
import argparse, json, os, sys, time, urllib.parse, urllib.request

UA = "HaniWord-domain-dictionary/0.1 (https://github.com/supertastybanana3) python-urllib/3.12"
ENDPOINT = "https://query.wikidata.org/sparql"
# 種類の Q 番号 (2026-10-20 に Wikidata の API でラベルを確かめた): 戦闘 Q178561・戦争 Q198・反乱 Q124734・条約 Q131569・時代 Q11514315・
# 王朝 Q164950・かつて存在した国家 Q3024240・元号 Q256408・歴史的事象 Q13418847・クーデター Q45382・地震 Q7944・遺跡 Q839954・
# 職位 Q4164871・一揆 Q8191556・藩 Q841985・幕府 Q1074150・令制国 Q860290
# 国籍の Q 番号 (2026-10-20 に API でラベルを確かめた): 日本 Q17・大日本帝国 Q188712・江戸幕府 Q205662・室町幕府 Q1154406・鎌倉幕府 Q736839・
# 律令日本 Q132869361・倭国 Q10888424・ヤマト王権 Q2658252・琉球王国 Q28025・日本の封建制 Q3092690、
# 中国の王朝など (中華人民共和国 Q148・中国 Q29520・清 Q8733・明 Q9903・唐 Q9683・宋 Q7462・元 Q7313・中華民国 Q13426199/Q865・北宋 Q319460・
# 南宋 Q1147043・北魏 Q875305・隋 Q7405・漢 Q7209・後漢 Q1147037・魏 Q320930・梁 Q1069868・北周 Q551067・晋 Q7352・前漢 Q1072949・西魏 Q1143164・
# 金 Q5066・周 Q35216・東晋 Q306928・西晋 Q1145470・秦朝 Q7183・蜀漢 Q320925・呉 Q274488・殷 Q128938・モンゴル帝国 Q12557)、
# 朝鮮 (李氏朝鮮 Q28179・高麗 Q28208・新羅 Q28456・高句麗 Q28370・渤海 Q28322・大韓帝国 Q28233・朝鮮 Q18097・統一新羅 Q715257)、ベトナム (阮朝 Q6500483・大越 Q430309)
COUNTRIES = ["Q17", "Q188712", "Q205662", "Q1154406", "Q736839", "Q132869361", "Q10888424", "Q2658252", "Q28025", "Q3092690",
             "Q148", "Q29520", "Q8733", "Q9903", "Q9683", "Q7462", "Q7313", "Q13426199", "Q865", "Q319460", "Q1147043", "Q875305",
             "Q7405", "Q7209", "Q1147037", "Q320930", "Q1069868", "Q551067", "Q7352", "Q1072949", "Q1143164", "Q5066", "Q35216",
             "Q306928", "Q1145470", "Q7183", "Q320925", "Q274488", "Q128938", "Q12557",
             "Q28179", "Q28208", "Q28456", "Q28370", "Q28322", "Q28233", "Q18097", "Q715257", "Q6500483", "Q430309"]
THINGS = ["Q178561", "Q198", "Q124734", "Q131569", "Q11514315", "Q164950", "Q3024240", "Q256408", "Q13418847",
          "Q45382", "Q7944", "Q839954", "Q4164871", "Q8191556", "Q841985", "Q1074150", "Q860290"]


def sparql(query):
    for k in range(6):
        try:
            req = urllib.request.Request(ENDPOINT + "?" + urllib.parse.urlencode({"query": query, "format": "json"}),
                                         headers={"User-Agent": UA, "Accept": "application/sparql-results+json"})
            with urllib.request.urlopen(req, timeout=120) as r:
                # ラベルに制御文字が入っていることがある (strict=False で読む)
                return json.loads(r.read().decode("utf-8"), strict=False)["results"]["bindings"]
        except Exception as e:
            print("retry", k, e, file=sys.stderr)
            time.sleep(10 * (2 ** k))
    print("gave up", query[:200], file=sys.stderr)
    return []


def rows_to_items(rows, kind):
    items = {}
    for b in rows:
        q = b["item"]["value"].rsplit("/", 1)[1]
        it = items.setdefault(q, {"qid": q, "label": b["label"]["value"], "kana": "", "title": "", "country": [], "kind": kind,
                                  "links": int(b["links"]["value"]) if "links" in b else 0})
        if "kana" in b:
            it["kana"] = b["kana"]["value"]
        if "article" in b:
            it["title"] = urllib.parse.unquote(b["article"]["value"].rsplit("/wiki/", 1)[1]).replace("_", " ")
        if "country" in b:
            c = b["country"]["value"].rsplit("/", 1)[1]
            if c not in it["country"]:
                it["country"].append(c)
    return items.values()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["persons", "persons_by_country", "things", "has_property"])
    ap.add_argument("out")
    ap.add_argument("--from", dest="start", type=int, default=-800)
    ap.add_argument("--to", type=int, default=1900)
    ap.add_argument("--step", type=int, default=25)
    ap.add_argument("--classes", nargs="*", default=THINGS)
    ap.add_argument("--countries", nargs="*", default=COUNTRIES)
    ap.add_argument("--properties", nargs="*", default=[])
    a = ap.parse_args()
    done = set()
    if os.path.exists(a.out):
        done = {json.loads(l)["chunk"] for l in open(a.out + ".chunks", encoding="utf-8")} if os.path.exists(a.out + ".chunks") else set()
    out = open(a.out, "a", encoding="utf-8")
    chunks = open(a.out + ".chunks", "a", encoding="utf-8")
    if a.cmd == "persons":
        # 年で人物全体を絞ると 1 回の問い合わせで人物の全件を走査して時間切れになる (2026-10-20、504)。
        # 名前のカナ表記 (P1814、日本の人名に付く) を持つ項目から始め、生年で区切る。日本以外の人物はウィキペディアのカテゴリから拾う
        jobs = [(y, min(y + a.step, a.to)) for y in range(a.start, a.to, a.step)]
        for lo, hi in jobs:
            key = f"{lo}:{hi}"
            if key in done:
                continue
            q = f"""SELECT ?item ?label ?kana ?article ?country ?links WHERE {{
  hint:Query hint:optimizer "None".
  ?item wdt:P1814 ?kana.
  ?item wdt:P31 wd:Q5.
  ?item wdt:P569 ?born.
  FILTER(YEAR(?born) >= {lo} && YEAR(?born) < {hi})
  ?article schema:about ?item; schema:isPartOf <https://ja.wikipedia.org/>.
  ?item rdfs:label ?label. FILTER(LANG(?label) = "ja")
  ?item wikibase:sitelinks ?links.
  OPTIONAL {{ {{ ?item wdt:P27 ?country. }} UNION {{ ?item wdt:P19/wdt:P17 ?country. }} }}
}}"""
            items = list(rows_to_items(sparql(q), "person"))
            for it in items:
                out.write(json.dumps(it, ensure_ascii=False) + "\n")
            out.flush()
            chunks.write(json.dumps({"chunk": key, "n": len(items)}) + "\n")
            chunks.flush()
            print(key, len(items), file=sys.stderr, flush=True)
            time.sleep(2)
    elif a.cmd == "persons_by_country":
        # 名前のカナ表記の無い人物 (藤原道長など) も取るため、国籍 (P27) ごとに 1900 年より前に生まれた人物を取る
        for country in a.countries:
            if country in done:
                continue
            q = f"""SELECT ?item ?label ?kana ?article ?country ?links WHERE {{
  hint:Query hint:optimizer "None".
  ?item wdt:P27 wd:{country}.
  ?item wdt:P31 wd:Q5.
  ?item wdt:P569 ?born. FILTER(YEAR(?born) < 1900)
  ?article schema:about ?item; schema:isPartOf <https://ja.wikipedia.org/>.
  ?item rdfs:label ?label. FILTER(LANG(?label) = "ja")
  ?item wikibase:sitelinks ?links.
  OPTIONAL {{ ?item wdt:P1814 ?kana. }}
  BIND(wd:{country} AS ?country)
}}"""
            items = list(rows_to_items(sparql(q), "person"))
            for it in items:
                out.write(json.dumps(it, ensure_ascii=False) + "\n")
            out.flush()
            chunks.write(json.dumps({"chunk": country, "n": len(items)}) + "\n")
            chunks.flush()
            print(country, len(items), file=sys.stderr, flush=True)
            time.sleep(2)
    elif a.cmd == "has_property":
        for prop in a.properties:
            if prop in done:
                continue
            q = f"""SELECT ?item ?label ?kana ?article ?country ?links WHERE {{
  ?item wdt:{prop} [].
  ?article schema:about ?item; schema:isPartOf <https://ja.wikipedia.org/>.
  ?item rdfs:label ?label. FILTER(LANG(?label) = "ja")
  ?item wikibase:sitelinks ?links.
  OPTIONAL {{ ?item wdt:P1814 ?kana. }}
}}"""
            items = list(rows_to_items(sparql(q), prop))
            for it in items:
                out.write(json.dumps(it, ensure_ascii=False) + "\n")
            out.flush()
            chunks.write(json.dumps({"chunk": prop, "n": len(items)}) + "\n")
            chunks.flush()
            print(prop, len(items), file=sys.stderr, flush=True)
    else:
        for cls in a.classes:
            if cls in done:
                continue
            q = f"""SELECT ?item ?label ?kana ?article ?country ?links WHERE {{
  ?item wdt:P31/wdt:P279* wd:{cls}.
  ?article schema:about ?item; schema:isPartOf <https://ja.wikipedia.org/>.
  ?item rdfs:label ?label. FILTER(LANG(?label) = "ja")
  ?item wikibase:sitelinks ?links.
  OPTIONAL {{ ?item wdt:P1814 ?kana. }}
  OPTIONAL {{ ?item wdt:P17 ?country. }}
}}"""
            items = list(rows_to_items(sparql(q), cls))
            for it in items:
                out.write(json.dumps(it, ensure_ascii=False) + "\n")
            out.flush()
            chunks.write(json.dumps({"chunk": cls, "n": len(items)}) + "\n")
            chunks.flush()
            print(cls, len(items), file=sys.stderr, flush=True)
            time.sleep(2)


if __name__ == "__main__":
    main()
