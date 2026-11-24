"""日本語版ウィキペディアのカテゴリから、分野辞書の語の候補 (記事の題名) と、記事の冒頭の括弧の中の読みを集める。

ライセンス: 文章は CC BY-SA 4.0 (と GFDL)。使うのは題名と読みだけ。出典の表示は THIRD_PARTY_NOTICES に記事の場所 (URL) で書く。
MediaWiki API は 1 つずつ・1 秒おきに送り、User-Agent に連絡先を書く (API:Etiquette)。
効果を測る記事 (リポジトリの外の測りの組の .py にある ARTICLE_SETS) は --holdout で除く (辞書の効果を高く見積もらないため)。
--holdout を省くと環境変数 HANIWORD_HOLDOUT の .py を使い、それも無ければ除かない。
使い方: python3 collect_wikipedia.py <出力.jsonl> --roots 日本の法 私法 ... [--depth 3] [--holdout <測りの組の .py>]
出力 (1 行 1 記事): {"title", "term" (題名から括弧の注記を除いたもの), "reading" (冒頭の括弧の読み。無ければ ""), "lead", "cats"}
"""
import argparse, ast, json, os, re, sys, time, urllib.parse, urllib.request

UA = "HaniWord-domain-dictionary/0.1 (https://github.com/supertastybanana3) python-urllib/3.12"
API = "https://ja.wikipedia.org/w/api.php"
# 下りないカテゴリ (人物・学位・雑誌・個別の事件や判例・外国の個別の法令・保守用)
SKIP_CAT = re.compile(r"学者|研究者|出身者|関係者|受賞者|人物|取得者|博士|修士|学位|雑誌|学派|事件|判例|裁判例|画像|スタブ|プロジェクト|テンプレート|一覧|ウィキ|"
                      r"法律家|弁護士|裁判官|検察官|政治家|団体|組織|企業|書籍|作品|ドラマ|映画|漫画|アニメ|小説|"
                      r"アメリカ|イギリス|ドイツ|フランス|中国|中華|韓国|朝鮮|台湾|ロシア|イスラーム|ローマ|各国|州の|欧州|EU|"
                      r"二国間条約|決議|加盟国|戦争|紛争|軍|宇宙|海洋")


def api(**q):
    q.update(format="json", maxlag="5")
    for k in range(6):
        time.sleep(1.0)
        try:
            req = urllib.request.Request(API + "?" + urllib.parse.urlencode(q), headers={"User-Agent": UA})
            with urllib.request.urlopen(req, timeout=60) as r:
                d = json.load(r)
            if "error" in d and d["error"].get("code") == "maxlag":
                time.sleep(5 * (k + 1))
                continue
            return d
        except Exception as e:
            print("retry", e, file=sys.stderr)
            time.sleep(5 * (2 ** k))
    print("gave up", q, file=sys.stderr)
    return None


def members(cat, cmtype):
    out, cont = [], {}
    while True:
        d = api(action="query", list="categorymembers", cmtitle="Category:" + cat, cmtype=cmtype, cmlimit="500", **cont)
        if d is None:
            return out  # 取れなかったカテゴリは飛ばす
        out += d["query"]["categorymembers"]
        if "continue" not in d:
            return out
        cont = {"cmcontinue": d["continue"]["cmcontinue"]}


def holdout_titles(path):
    """測りの組の .py の ARTICLE_SETS を、コードを実行せずに読む (path が空なら除く記事なし)"""
    if not path:
        return set()
    tree = ast.parse(open(os.path.expanduser(path), encoding="utf-8").read())
    for node in tree.body:
        if isinstance(node, ast.Assign) and any(getattr(t, "id", "") == "ARTICLE_SETS" for t in node.targets):
            titles = set()
            for v in node.value.values:
                if isinstance(v, ast.List):
                    titles |= {e.value for e in v.elts if isinstance(e, ast.Constant)}
            return titles
    raise SystemExit("ARTICLE_SETS not found in " + path)


def lead_reading(title, lead):
    """冒頭の「題名（よみ、…）」の読み (ひらがな) を取る"""
    base = re.sub(r"\s*\(.*?\)$", "", title)
    m = re.search(re.escape(base) + r"[」』]?\s*[（(]([^）)]{1,80})[）)]", lead[:300])
    if not m:
        return ""
    first = re.split(r"[、,;；]|\s|または|又は", m.group(1).strip())[0]
    return first if re.fullmatch(r"[ぁ-ゖー・]+", first) else ""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--roots", nargs="+", required=True)
    ap.add_argument("--depth", type=int, default=3)
    ap.add_argument("--holdout", default=os.environ.get("HANIWORD_HOLDOUT", ""))
    ap.add_argument("--max-pages", type=int, default=20000)
    ap.add_argument("--skip", help="下りないカテゴリの正規表現 (省くと法律用の SKIP_CAT。歴史では国名・戦争のカテゴリにも下りたいので分野ごとに渡す)")
    a = ap.parse_args()
    skip = re.compile(a.skip) if a.skip else SKIP_CAT
    hold = holdout_titles(a.holdout)
    state = a.out + ".pages.json"  # カテゴリをたどった途中経過 (止まっても続きから)
    if os.path.exists(state):
        st = json.load(open(state, encoding="utf-8"))
        pages, seen_cats, frontier = st["pages"], set(st["seen"]), [tuple(x) for x in st["frontier"]]
    else:
        pages, seen_cats, frontier = {}, set(), [(r, 0) for r in a.roots]
    while frontier and len(pages) < a.max_pages:
        cat, depth = frontier.pop(0)
        if cat in seen_cats:
            continue
        seen_cats.add(cat)
        for m in members(cat, "page"):
            if m["ns"] == 0:
                pages.setdefault(m["title"], []).append(cat)
        if depth < a.depth:
            for m in members(cat, "subcat"):
                sub = m["title"].split(":", 1)[1]
                if not skip.search(sub):
                    frontier.append((sub, depth + 1))
        print(f"cat {cat} depth={depth} pages={len(pages)} queue={len(frontier)}", file=sys.stderr, flush=True)
        if len(seen_cats) % 20 == 0:
            json.dump({"pages": pages, "seen": sorted(seen_cats), "frontier": frontier}, open(state, "w", encoding="utf-8"), ensure_ascii=False)
    json.dump({"pages": pages, "seen": sorted(seen_cats), "frontier": frontier}, open(state, "w", encoding="utf-8"), ensure_ascii=False)
    skipped = sorted(t for t in pages if t in hold)
    titles = [t for t in pages if t not in hold]
    print("holdout skipped:", skipped, file=sys.stderr)
    with open(a.out, "w", encoding="utf-8") as out:
        for i in range(0, len(titles), 20):
            batch = titles[i:i + 20]
            d = api(action="query", prop="extracts", exintro="1", explaintext="1", exchars="300", titles="|".join(batch), redirects="1")
            leads = {p["title"]: p.get("extract", "") for p in d["query"]["pages"].values()} if d else {}
            for t in batch:
                lead = leads.get(t, "")
                term = re.sub(r"\s*\(.*?\)$", "", t)
                out.write(json.dumps({"title": t, "term": term, "reading": lead_reading(t, lead), "lead": lead[:200],
                                      "cats": pages[t]}, ensure_ascii=False) + "\n")
            out.flush()
    print("pages", len(titles), "cats", len(seen_cats), file=sys.stderr)


if __name__ == "__main__":
    main()
