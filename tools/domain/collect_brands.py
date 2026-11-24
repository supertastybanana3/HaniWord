"""社名・製品名の英字の表 (英字の判定の表に無い dynabook・AQUOS・Neovim のような語) を Wikidata (CC0) と日本語版ウィキペディアの記事名から作る。
日本で使われる語に絞るため、日本語版ウィキペディアに記事があり、記事名が英字だけの 1 語のものから始める (全体のダンプは落とさない)。
  titles  : 日本語版ウィキペディアの記事名のうち、英字で始まる記事 (転送でないもの) を API の allpages で取る
  items   : その記事名の Wikidata の項目・日本語と英語のラベル・種類 (P31)・サイトリンクの数を SPARQL で取る (300 件ずつ)
  by_class: 上の種類の下にある種類の項目のうち、日本語版の記事があり英語のラベルが英字だけのもの (記事名がかな・漢字の 食べログ・クックパッド も拾う)
  classes : 出てきた種類 (P31) が、企業・ブランド・製品・ソフトウェア・Web サイト・オンラインサービス・ゲームの下 (P279*) にあるかを SPARQL で確かめる
  select  : 種類が合い、英字だけで、IME の判定でローマ字として読めず、英字の判定の表に無い語を選ぶ。表記の形は日本語のラベル (英字だけのとき)、
            無ければ英語のラベル・記事名。4 字未満 (日本語の打ちかけとぶつかりやすい)・サイトリンクの少ない語は分けておく
MediaWiki API・SPARQL は 1 つずつ・間をあけて送り、User-Agent に連絡先を書く (API:Etiquette)。途中で止めても続きから。
使い方: python3 collect_brands.py titles|items|classes|select --work <作業フォルダ> [--readable-script <ime_readable.py>] [--known <英字の判定の語の表>...]
"""
import argparse, json, os, re, sys, time, urllib.parse, urllib.request

UA = "HaniWord-dictionary-builder/0.1 (https://github.com/supertastybanana3) python-urllib/3.12"
API = "https://ja.wikipedia.org/w/api.php"
SPARQL = "https://query.wikidata.org/sparql"
# 上の種類 (2026-10-24 に Wikidata の API でラベルを確かめる: classes の初めに英語のラベルを書き出す)
# 製品の型 Q10929058・型のシリーズ Q811701 は 2026-10-24 23:45 に足した (AQUOS は製品の型、iPhone はスマートフォンの型のシリーズで、商品 Q2424752 の下に無かった)
TOP_CLASSES = ["Q4830453", "Q431289", "Q2424752", "Q7397", "Q35127", "Q19967801", "Q7889", "Q7058673", "Q9143", "Q167270", "Q10929058", "Q811701"]
TOKEN = re.compile(r"[A-Za-z][A-Za-z0-9]*(?:[.+&'-][A-Za-z0-9]+)*[+]*")
LETTERS = re.compile(r"[A-Za-z]+")


def get_json(url, params=None, data=None, accept="application/json"):
    for k in range(6):
        try:
            full = url + ("?" + urllib.parse.urlencode(params) if params else "")
            req = urllib.request.Request(full, data=urllib.parse.urlencode(data).encode() if data else None,
                                         headers={"User-Agent": UA, "Accept": accept})
            with urllib.request.urlopen(req, timeout=120) as r:
                return json.loads(r.read().decode("utf-8"), strict=False)
        except Exception as e:
            print("retry", k, e, file=sys.stderr)
            time.sleep(10 * (2 ** k))
    raise SystemExit("gave up: " + url)


def sparql(query):
    return get_json(SPARQL, data={"query": query, "format": "json"}, accept="application/sparql-results+json")["results"]["bindings"]


def cmd_titles(a):
    out = os.path.join(a.work, "titles.jsonl")
    state = os.path.join(a.work, "titles.continue")
    cont = json.load(open(state)) if os.path.exists(state) else {}
    if cont.get("done"):
        print("titles already done"); return
    params = {"action": "query", "list": "allpages", "apnamespace": 0, "apfilterredir": "nonredirects", "aplimit": "max",
              "apfrom": "A", "apto": "z~", "format": "json"}
    n = 0
    with open(out, "a", encoding="utf-8") as f:
        while True:
            r = get_json(API, dict(params, **cont))
            for p in r["query"]["allpages"]:
                f.write(json.dumps({"title": p["title"]}, ensure_ascii=False) + "\n"); n += 1
            f.flush()
            if "continue" not in r:
                json.dump({"done": True}, open(state, "w")); break
            cont = r["continue"]; json.dump(cont, open(state, "w"))
            time.sleep(1)
    print("titles", n)


def base_title(t):
    """記事名の後ろの曖昧さ回避の補い「 (…)」を外した形 (Dynabook (企業) → Dynabook)"""
    return re.sub(r" \([^)]*\)$", "", t)


def single_token_titles(work):
    seen = set()
    for l in open(os.path.join(work, "titles.jsonl"), encoding="utf-8"):
        t = json.loads(l)["title"]
        if LETTERS.fullmatch(base_title(t)) and t not in seen:
            seen.add(t); yield t


def cmd_items(a):
    out = os.path.join(a.work, "items.jsonl")
    done = {json.loads(l)["title"] for l in open(out, encoding="utf-8")} if os.path.exists(out) else set()
    todo = [t for t in single_token_titles(a.work) if t not in done]
    print("titles to look up", len(todo), file=sys.stderr)
    with open(out, "a", encoding="utf-8") as f:
        for k in range(0, len(todo), 300):
            batch = todo[k:k + 300]
            # 記事の URL は MediaWiki と同じく ;:@$!*(),/~ を置き換えない (括弧を %28 にすると Wikidata の記事の URL と合わず、補い付きの記事名が 1 件も見つからなかった)
            values = " ".join("<https://ja.wikipedia.org/wiki/" + urllib.parse.quote(t.replace(" ", "_"), safe=";:@$!*(),/~") + ">" for t in batch)
            q = f"""SELECT ?article ?item ?ja ?en ?links (GROUP_CONCAT(DISTINCT STRAFTER(STR(?c), "entity/"); separator=" ") AS ?classes) WHERE {{
  VALUES ?article {{ {values} }}
  ?article schema:about ?item.
  ?item wikibase:sitelinks ?links.
  OPTIONAL {{ ?item rdfs:label ?ja. FILTER(LANG(?ja) = "ja") }}
  OPTIONAL {{ ?item rdfs:label ?en. FILTER(LANG(?en) = "en") }}
  OPTIONAL {{ ?item wdt:P31 ?c. }}
}} GROUP BY ?article ?item ?ja ?en ?links"""
            got = {}
            for b in sparql(q):
                t = urllib.parse.unquote(b["article"]["value"].rsplit("/wiki/", 1)[1]).replace("_", " ")
                got[t] = {"title": t, "qid": b["item"]["value"].rsplit("/", 1)[1], "ja": b.get("ja", {}).get("value", ""),
                          "en": b.get("en", {}).get("value", ""), "links": int(b["links"]["value"]),
                          "classes": b.get("classes", {}).get("value", "").split()}
            for t in batch:  # 項目の無い記事も「済み」として書く (問い合わせ直さない)
                f.write(json.dumps(got.get(t, {"title": t, "qid": ""}), ensure_ascii=False) + "\n")
            f.flush()
            print(k + len(batch), "/", len(todo), file=sys.stderr, flush=True)
            time.sleep(2)


def cmd_classes(a):
    out = os.path.join(a.work, "classes.json")
    ok = json.load(open(out, encoding="utf-8")) if os.path.exists(out) else {}
    labels = sparql("SELECT ?c ?l WHERE { VALUES ?c { " + " ".join("wd:" + c for c in TOP_CLASSES) + " } ?c rdfs:label ?l. FILTER(LANG(?l) = \"en\") }")
    print("top classes:", {b["c"]["value"].rsplit("/", 1)[1]: b["l"]["value"] for b in labels})
    classes = set()
    for l in open(os.path.join(a.work, "items.jsonl"), encoding="utf-8"):
        classes.update(json.loads(l).get("classes", []))
    todo = sorted(c for c in classes if c not in ok)
    print("classes to check", len(todo), "of", len(classes), file=sys.stderr)
    for k in range(0, len(todo), 200):
        batch = todo[k:k + 200]
        q = f"""SELECT DISTINCT ?c ?top WHERE {{
  VALUES ?c {{ {' '.join('wd:' + c for c in batch)} }}
  VALUES ?top {{ {' '.join('wd:' + c for c in TOP_CLASSES)} }}
  ?c wdt:P279* ?top.
}}"""
        hit = {}
        for b in sparql(q):
            hit.setdefault(b["c"]["value"].rsplit("/", 1)[1], []).append(b["top"]["value"].rsplit("/", 1)[1])
        for c in batch:
            ok[c] = sorted(hit.get(c, []))
        json.dump(ok, open(out, "w", encoding="utf-8"))
        print(k + len(batch), "/", len(todo), file=sys.stderr, flush=True)
        time.sleep(2)


def cmd_by_class(a):
    """日本語版の記事名がかな・漢字の語 (食べログ・クックパッド・ゼルダの伝説) も拾うため、上の種類の下にある種類 (classes.json) の項目のうち、
    日本語版ウィキペディアに記事があり、英語のラベルが英字だけの 1 語のものを引く (種類 20 ずつ)"""
    classes = json.load(open(os.path.join(a.work, "classes.json"), encoding="utf-8"))
    out = os.path.join(a.work, "by_class.jsonl")
    state = os.path.join(a.work, "by_class.done")
    done = set(open(state, encoding="utf-8").read().split()) if os.path.exists(state) else set()
    todo = sorted({c for c, tops in classes.items() if tops} | set(TOP_CLASSES))
    todo = [c for c in todo if c not in done]
    print("classes to look up", len(todo), file=sys.stderr)
    with open(out, "a", encoding="utf-8") as f, open(state, "a", encoding="utf-8") as g:
        for k in range(0, len(todo), 20):
            batch = todo[k:k + 20]
            q = f"""SELECT ?item ?title ?ja ?en ?links (GROUP_CONCAT(DISTINCT STRAFTER(STR(?c2), "entity/"); separator=" ") AS ?classes) WHERE {{
  VALUES ?c {{ {' '.join('wd:' + c for c in batch)} }}
  ?item wdt:P31 ?c.
  ?item rdfs:label ?en. FILTER(LANG(?en) = "en" && REGEX(?en, "^[A-Za-z]+$"))
  ?article schema:about ?item; schema:isPartOf <https://ja.wikipedia.org/>; schema:name ?title.
  ?item wikibase:sitelinks ?links.
  OPTIONAL {{ ?item rdfs:label ?ja. FILTER(LANG(?ja) = "ja") }}
  ?item wdt:P31 ?c2.
}} GROUP BY ?item ?title ?ja ?en ?links"""
            for b in sparql(q):
                f.write(json.dumps({"title": b["title"]["value"], "qid": b["item"]["value"].rsplit("/", 1)[1], "ja": b.get("ja", {}).get("value", ""),
                                    "en": b["en"]["value"], "links": int(b["links"]["value"]), "classes": b.get("classes", {}).get("value", "").split(),
                                    "by_class": True}, ensure_ascii=False) + "\n")
            f.flush()
            g.write(" ".join(batch) + "\n"); g.flush()
            print(k + len(batch), "/", len(todo), file=sys.stderr, flush=True)
            time.sleep(2)


def cmd_aliases(a):
    """ラベルが 2 語 (Sharp Aquos) やかな (アクオス) でも、別名に英字の名前がある (AQUOS) 項目を拾う。上の種類の下の種類の項目のうち、
    日本語版ウィキペディアに記事があり、日本語か英語の別名が英字だけの 4 字以上のもの (種類 20 ずつ。問い合わせの順を固定して時間切れを避ける)"""
    classes = json.load(open(os.path.join(a.work, "classes.json"), encoding="utf-8"))
    out = os.path.join(a.work, "aliases.jsonl")
    state = os.path.join(a.work, "aliases.done")
    done = set(open(state, encoding="utf-8").read().split()) if os.path.exists(state) else set()
    todo = [c for c in sorted({c for c, tops in classes.items() if tops} | set(TOP_CLASSES)) if c not in done]
    print("classes to look up", len(todo), file=sys.stderr)
    with open(out, "a", encoding="utf-8") as f, open(state, "a", encoding="utf-8") as g:
        for k in range(0, len(todo), 20):
            batch = todo[k:k + 20]
            q = f"""SELECT ?item ?title ?alias ?links (GROUP_CONCAT(DISTINCT STRAFTER(STR(?c2), "entity/"); separator=" ") AS ?classes) WHERE {{
  hint:Query hint:optimizer "None".
  VALUES ?c {{ {' '.join('wd:' + c for c in batch)} }}
  ?item wdt:P31 ?c.
  ?article schema:about ?item; schema:isPartOf <https://ja.wikipedia.org/>; schema:name ?title.
  ?item skos:altLabel ?alias. FILTER((LANG(?alias) = "ja" || LANG(?alias) = "en") && REGEX(?alias, "^[A-Za-z]{{4,}}$"))
  ?item wikibase:sitelinks ?links.
  ?item wdt:P31 ?c2.
}} GROUP BY ?item ?title ?alias ?links"""
            for b in sparql(q):
                f.write(json.dumps({"title": b["title"]["value"], "qid": b["item"]["value"].rsplit("/", 1)[1], "alias": b["alias"]["value"],
                                    "lang": b["alias"].get("xml:lang", ""), "links": int(b["links"]["value"]),
                                    "classes": b.get("classes", {}).get("value", "").split()}, ensure_ascii=False) + "\n")
            f.flush()
            g.write(" ".join(batch) + "\n"); g.flush()
            print(k + len(batch), "/", len(todo), file=sys.stderr, flush=True)
            time.sleep(2)


def latin_title_qids(work):
    """日本語版ウィキペディアの記事名 (補いを外す) が英字だけの項目 (日本でもその綴りで書く語)"""
    qids = set()
    for name in ("items.jsonl", "by_class.jsonl", "aliases.jsonl"):
        path = os.path.join(work, name)
        if os.path.exists(path):
            for l in open(path, encoding="utf-8"):
                it = json.loads(l)
                if it.get("qid") and LETTERS.fullmatch(base_title(it["title"])):
                    qids.add(it["qid"])
    return qids


def cmd_japan(a):
    """記事名がかな・漢字の項目 (by_class・aliases) のうち、日本と結びつくもの (国 P17・原産国 P495・本社の国 P159/P17、
    製造元 P176・持ち主 P127 の国か本社の国が日本 Q17。AQUOS のようにブランドの項目に国が無く、製造元にだけあるものも拾う)。300 ずつ。
    前に日本と結びつかないとした項目も、広げた見方で確かめ直す (--recheck)"""
    latin = latin_title_qids(a.work)
    qids = set()
    for name in ("by_class.jsonl", "aliases.jsonl"):
        for l in open(os.path.join(a.work, name), encoding="utf-8"):
            q = json.loads(l)["qid"]
            if q not in latin:
                qids.add(q)
    out = os.path.join(a.work, "japan.json")
    got = json.load(open(out, encoding="utf-8")) if os.path.exists(out) else {}
    todo = sorted(q for q in qids if q not in got or (a.recheck and not got[q]))
    print("items to check", len(todo), file=sys.stderr)
    for k in range(0, len(todo), 300):
        batch = todo[k:k + 300]
        hit = {b["item"]["value"].rsplit("/", 1)[1] for b in sparql(
            "SELECT DISTINCT ?item WHERE { VALUES ?item { " + " ".join("wd:" + q for q in batch) + " } "
            "?item (wdt:P17|wdt:P495|wdt:P159/wdt:P17|wdt:P176/wdt:P17|wdt:P176/wdt:P159/wdt:P17|wdt:P127/wdt:P17|wdt:P127/wdt:P159/wdt:P17) wd:Q17. }")}
        for q in batch:
            got[q] = q in hit
        json.dump(got, open(out, "w", encoding="utf-8"))
        print(k + len(batch), "/", len(todo), file=sys.stderr, flush=True)
        time.sleep(2)


def cmd_class_labels(a):
    """上の種類の下にある種類の英語のラベル (成人向けの種類を外すのに使う)。200 ずつ"""
    classes = json.load(open(os.path.join(a.work, "classes.json"), encoding="utf-8"))
    out = os.path.join(a.work, "class_labels.json")
    labels = json.load(open(out, encoding="utf-8")) if os.path.exists(out) else {}
    todo = sorted(c for c, tops in classes.items() if tops and c not in labels)
    for k in range(0, len(todo), 200):
        batch = todo[k:k + 200]
        for b in sparql("SELECT ?c ?l WHERE { VALUES ?c { " + " ".join("wd:" + c for c in batch) + " } ?c rdfs:label ?l. FILTER(LANG(?l) = \"en\") }"):
            labels[b["c"]["value"].rsplit("/", 1)[1]] = b["l"]["value"]
        for c in batch:
            labels.setdefault(c, "")
        json.dump(labels, open(out, "w", encoding="utf-8"), ensure_ascii=False)
        print(k + len(batch), "/", len(todo), file=sys.stderr, flush=True)
        time.sleep(2)


def cmd_select(a):
    """表に入れる語を選ぶ。読めるかは IME と同じ判定 (A の ime_readable.py。R = 読める、U = 読めない) に語を渡して決める"""
    import subprocess
    classes = json.load(open(os.path.join(a.work, "classes.json"), encoding="utf-8"))
    known = set()
    for path in a.known or []:  # 英字の判定に使う語の表 (小文字にして ^ と ~ を外す)
        for l in open(path, encoding="utf-8"):
            if l.strip() and not l.startswith("#"):
                known.update(x.strip().strip("^~").lower() for x in l.rstrip("\n").split("\t") if x.strip())
    # 普通の英単語 (英文の中だけで使う表のうち、SCOWL から来た語。手で選んだ技術の語 english_context_curated.txt は名前なので残す)。
    # 名前の表記直しで、普通の単語の大文字小文字を変えないため (A の注文)
    common = set()
    if a.context:
        curated = {l.strip().lower() for l in open(a.curated, encoding="utf-8") if l.strip() and not l.startswith("#")} if a.curated else set()
        common = {l.strip().lower() for l in open(a.context, encoding="utf-8") if l.strip() and not l.startswith("#")} - curated
    labels = json.load(open(os.path.join(a.work, "class_labels.json"), encoding="utf-8")) if os.path.exists(os.path.join(a.work, "class_labels.json")) else {}
    adult = {c for c, l in labels.items() if re.search(r"porn|adult|erotic|hentai", l, re.I)}
    adult_word = re.compile(r"porn|hentai|xxx|sex", re.I)  # 種類で外れない成人向けの名前 (表記で見る)
    # 記事名がかな・漢字の項目 (ナベグラヴィ) は、日本の人がその英字の綴りを打つことは少ない。日本と結びつく項目か、サイトリンクの多い (世界で知られた) 項目だけ
    latin = latin_title_qids(a.work)
    japan = json.load(open(os.path.join(a.work, "japan.json"), encoding="utf-8")) if os.path.exists(os.path.join(a.work, "japan.json")) else {}
    # 測って悪くした語・普通の英単語と同じ綴りなのに SCOWL に無かった語 (brands_exclude.txt: 表記<TAB>理由。小文字で比べる)
    excluded = {l.split("\t")[0].strip().lower() for l in open(a.exclude, encoding="utf-8") if l.strip() and not l.startswith("#")} if a.exclude else set()
    items, seen = [], set()
    alias_path = os.path.join(a.work, "aliases.jsonl")
    if os.path.exists(alias_path):  # 別名は 1 つずつ別の行 (表記は別名そのもの)
        for l in open(alias_path, encoding="utf-8"):
            it = json.loads(l)
            # 英語の別名は雑多 (古い名前・略称・ほかの言語の語) なので、サイトリンクの多い (よく知られた) 項目だけ。日本語の別名 (AQUOS・Dynabook) はそのまま
            if it.get("lang") != "ja" and it["links"] < a.min_alias_links:
                continue
            if (it["qid"], it["alias"]) not in seen:
                seen.add((it["qid"], it["alias"]))
                it["form"] = it["alias"]
                items.append(it)
    for name in ("items.jsonl", "by_class.jsonl"):
        path = os.path.join(a.work, name)
        if not os.path.exists(path):
            continue
        for l in open(path, encoding="utf-8"):
            it = json.loads(l)
            if not it.get("qid") or it["qid"] in seen:
                continue
            seen.add(it["qid"])
            # 表記の形: 記事名から引いた項目は、日本語のラベル・英語のラベル・記事名 (補いを外す) のうち、英字だけで記事名と大文字小文字だけ違うもの
            # (dynabook・AQUOS・GitHub)。種類から引いた項目 (記事名がかな・漢字) は、日本語のラベルが英字だけならそれ、無ければ英語のラベル
            # 表記は Wikidata のラベル (CC0) だけから取る。記事名は、ラベルが記事の語と同じかを照らすのにだけ使う
            base = it["en"] if it.get("by_class") else base_title(it["title"])
            it["form"] = next((x for x in (it.get("ja", ""), it.get("en", "")) if LETTERS.fullmatch(x) and x.lower() == base.lower()), "")
            items.append(it)
    forms = sorted({it["form"] for it in items if it["form"]})
    out = subprocess.run([sys.executable, "-I", a.readable_script], input="\n".join(forms) + "\n", capture_output=True, text=True, check=True).stdout
    unreadable = {l.split("\t")[0] for l in out.splitlines() if l.endswith("\tU")}
    rows, stats = [], {}
    for it in items:
        tops = sorted({t for c in it.get("classes", []) for t in classes.get(c, [])})
        form = it["form"]
        why = ("not_letters" if not form else "no_class" if not tops else "adult" if adult & set(it.get("classes", [])) or adult_word.search(form) else
               "known" if form.lower() in known else "common_word" if form.lower() in common else "excluded" if form.lower() in excluded else
               "readable" if form not in unreadable else "short" if len(form) < a.min_len else "rare" if it["links"] < a.min_links else
               "foreign_kana" if it["qid"] not in latin and not japan.get(it["qid"]) and it["links"] < a.min_foreign_links else "ok")
        stats[why] = stats.get(why, 0) + 1
        rows.append((why, form or it["title"], it["qid"], it["links"], ",".join(tops), "alias" if "alias" in it else "label"))
    with open(os.path.join(a.work, "select.tsv"), "w", encoding="utf-8") as f:
        f.write("# 扱い\t表記\tQ 番号\tサイトリンクの数\t上の種類\t表記の出どころ (label か alias)\n")
        for r in sorted(rows, key=lambda r: (r[0], -r[3], r[1])):
            f.write("\t".join(map(str, r)) + "\n")
    print(stats)


def cmd_write(a):
    """select.tsv の ok の語を english_brands.txt に書く。小文字にして同じ語は、サイトリンクの多い項目の表記を取る"""
    # 同じ語に大文字の位置の違う書き方 (AQUOS・Aquos) があるときは、日本語版ウィキペディアの記事名の中に語として多く出る書き方を取る。
    # 同じ数なら、ラベル (別名より) の書き方、それも同じならサイトリンクの多い項目の書き方
    usage = {}
    for l in open(os.path.join(a.work, "titles.jsonl"), encoding="utf-8"):
        for tok in re.findall(r"[A-Za-z]+", json.loads(l)["title"]):
            usage[tok] = usage.get(tok, 0) + 1
    best = {}
    for l in open(os.path.join(a.work, "select.tsv"), encoding="utf-8"):
        c = l.rstrip("\n").split("\t")
        if l.startswith("#") or c[0] != "ok":
            continue
        k = c[1].lower()
        score = (usage.get(c[1], 0), c[5] == "label", int(c[3]))
        if k not in best or score > best[k][1]:
            best[k] = (c[1], score)
    with open(a.out, "w", encoding="utf-8", newline="\n") as f:
        f.write("# HaniWord の社名・製品名の英字の表 (英字の判定の語の表と、表記直しの表として読む)。1 行 1 語、表記そのまま\n"
                "# 出典: Wikidata (https://www.wikidata.org/、CC0 1.0) の項目のラベル。企業・ブランド・商品・製品の型・型のシリーズ・ソフトウェア・\n"
                "#   ウェブサイト・オンラインサービス・ゲーム・ゲームシリーズ・プログラミング言語・商標の項目のうち、日本語版ウィキペディアに記事があるもの\n"
                "# 表記: 項目の日本語・英語のラベルと日本語の別名 (英語の別名はサイトリンク 10 以上の項目だけ)。大文字の位置は日本語版ウィキペディアの記事名に多い書き方\n"
                "# 選び方: 英字だけ・4 字以上・IME の判定でローマ字として読めない・英字の判定の語の表と普通の英単語 (SCOWL) に無い・成人向けを除く。\n"
                "#   日本語版の記事名がかな・漢字の項目は、日本と結びつく (国・原産国・本社・製造元・持ち主が日本) か、サイトリンク 20 以上のものだけ\n"
                f"#   (tools/domain/collect_brands.py。{time.strftime('%Y-%m-%d')} に Wikidata から取った)\n")
        f.writelines(form + "\n" for form, _ in sorted(best.values(), key=lambda x: x[0].lower()))
    print("written", len(best))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["titles", "items", "classes", "by_class", "aliases", "japan", "class_labels", "select", "write"])
    ap.add_argument("--out", help="write: 書き出す english_brands.txt")
    ap.add_argument("--context", help="select: 英文の中だけで使う表 (english_words_context.txt)")
    ap.add_argument("--exclude", help="select: 入れない語の表 (tools/domain/brands_exclude.txt)")
    ap.add_argument("--curated", help="select: その中の、手で選んだ技術の語 (english_context_curated.txt)")
    ap.add_argument("--work", required=True)
    ap.add_argument("--readable-script", help="select: IME と同じ「ローマ字として読めるか」の判定 (語<TAB>R か U を返す)")
    ap.add_argument("--known", nargs="*")
    ap.add_argument("--min-len", type=int, default=4, help="これより短い語は short (日本語の打ちかけとぶつかりやすい)")
    ap.add_argument("--min-links", type=int, default=10, help="サイトリンクの数がこれより少ない語は rare")
    ap.add_argument("--min-alias-links", type=int, default=10, help="英語の別名は、サイトリンクの数がこれ以上の項目のものだけ使う")
    ap.add_argument("--recheck", action="store_true", help="japan: 日本と結びつかないとした項目も確かめ直す")
    ap.add_argument("--min-foreign-links", type=int, default=20, help="記事名がかな・漢字で日本と結びつかない項目は、サイトリンクの数がこれ以上のものだけ")
    a = ap.parse_args()
    os.makedirs(a.work, exist_ok=True)
    {"titles": cmd_titles, "items": cmd_items, "classes": cmd_classes, "by_class": cmd_by_class, "aliases": cmd_aliases, "japan": cmd_japan, "class_labels": cmd_class_labels, "select": cmd_select, "write": cmd_write}[a.cmd](a)


if __name__ == "__main__":
    main()
