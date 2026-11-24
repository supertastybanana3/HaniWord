"""分野ごとの英字の略語の表 (domain_dictionaries/<名前>_abbrev.txt) を作る (2026-11-01)。
よく打つ分野 (設定 input.domains_focus) に選んだ人の IME 本体が、英字の判定の語の表に足す (読み込みは IME 本体の側)。
表は 1 行 1 語、表記そのまま (正しい大文字小文字)、英字と数字だけ。

  candidates: 分野の記事 (collect_wikipedia.py の出力。日本語版ウィキペディアの記事の冒頭) に出る英字の略語らしい語と、
              JMdict (CC BY-SA 4.0) の英字の見出しで分野の印の付いた語を集める。大文字が 2 字以上か、小文字 1〜2 字のあとに大文字 (mRNA・IgG)。
              ローマ数字・化学式 (数字か小文字を含むもの)・単位を除き、その分野の記事に、いちばん多く出る分野の半分以上出て、多くの分野に広く出る一般の略語でないものだけ残す
  judge:      6.1-sol に「その分野の文章を書く人が打つ略語か」を判定させる (固有名詞・型番や登録番号・普通の英単語は false)
  write:      判定で残った語から、小文字で打つとローマ字として読める語 (IME 本体の判定と同じ。--readable に判定の Python を渡す) と、
              今の英字の判定の表 (english_words_scowl・english_terms・english_romaji_priority・english_case・english_boundary_words) にある語を除いて書く。
              除いた語と理由も書く
使い方:
  python3 abbrev.py candidates --pages <分野>=<記事.jsonl> ... --jmdict <JMdict_e.gz> --out <候補.json>
  python3 abbrev.py judge <候補.json> <分野> <作業フォルダ> --field 医学 [--jobs 3]
  python3 abbrev.py write <候補.json> <分野> <作業フォルダ> --readable <ime_readable.py> --out <名前>_abbrev.txt --log <除いた語.tsv>
"""
import argparse, collections, concurrent.futures, gzip, hashlib, importlib.util, json, os, re, subprocess, sys, unicodedata

REPO = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")
# codex の置き場 (PATH に無いときだけ、環境変数 CODEX_BIN_DIR で渡す)
CODEX_PATH = os.environ.get("CODEX_BIN_DIR", "")
TOKEN = re.compile(r"(?<![A-Za-z0-9])([A-Za-z][A-Za-z0-9]{1,10})(?![A-Za-z0-9])")
ROMAN = re.compile(r"^[IVXLCDM]+$")
ELEMENTS = set("""H He Li Be B C N O F Ne Na Mg Al Si P S Cl Ar K Ca Sc Ti V Cr Mn Fe Co Ni Cu Zn Ga Ge As Se Br Kr Rb Sr Y Zr Nb Mo Tc Ru Rh
Pd Ag Cd In Sn Sb Te I Xe Cs Ba La Ce Pr Nd Pm Sm Eu Gd Tb Dy Ho Er Tm Yb Lu Hf Ta W Re Os Ir Pt Au Hg Tl Pb Bi Po At Rn Fr Ra Ac Th Pa U Np Pu Am Cm""".split())
UNITS = set("""kDa Da nM mM uM mL dL uL kHz MHz GHz kPa MPa hPa keV MeV GeV TeV eV kW MW GW kJ kWh mV kV mA dB kB MB GB TB Mbps Gbps""".split())
# JMdict の分野の印 → 分野
JM_FIELDS = {"medicine": "medicine", "psychiatry": "medicine", "pharmacology": "medicine", "biology": "biology", "biochemistry": "biology",
             "genetics": "biology", "chemistry": "chemistry", "computing": "it", "Internet": "it", "telecommunications": "it",
             "physics": "physics", "electricity, elec. eng.": "physics", "mathematics": "math", "geology": "earth", "astronomy": "earth",
             "meteorology": "earth", "architecture": "architecture", "law": "law"}


def is_candidate(w):
    if not re.fullmatch(r"[A-Za-z][A-Za-z0-9]{1,10}", w) or ROMAN.match(w) or w in UNITS:
        return False
    parts = re.findall(r"[A-Z][a-z]?|\d+", w)
    if "".join(parts) == w and all(p in ELEMENTS or p.isdigit() for p in parts) and re.search(r"[0-9a-z]", w):  # 化学式 (CO2・CH3・NaCl)。OS・CPU は略語として残す
        return False
    upper = sum(c.isupper() for c in w)
    return upper >= 2 or bool(re.fullmatch(r"[a-z]{1,2}[A-Z][A-Za-z0-9]*", w))


def candidates(a):
    docs, npages, example = {}, {}, {}
    for spec in a.pages:
        dom, path = spec.split("=", 1)
        c = collections.Counter(); n = 0
        for line in open(path, encoding="utf-8"):
            try:
                page = json.loads(line)
            except ValueError:
                continue
            n += 1
            seen = set()
            for m in TOKEN.finditer(unicodedata.normalize("NFKC", page.get("lead", ""))):
                w = m.group(1)
                if is_candidate(w) and w not in seen:
                    seen.add(w); c[w] += 1; example.setdefault((dom, w), "Wikipedia:" + page["title"])
        docs[dom], npages[dom] = c, n
    jm = collections.defaultdict(set)
    if a.jmdict:
        import xml.etree.ElementTree as ET
        for _, e in ET.iterparse(gzip.open(a.jmdict), events=("end",)):
            if e.tag != "entry":
                continue
            fields = {f.text for s in e.findall("sense") for f in s.findall("field")}
            doms = {JM_FIELDS[f] for f in fields if f in JM_FIELDS}
            if doms:
                for h in [k.findtext("keb") for k in e.findall("k_ele")] + [r.findtext("reb") for r in e.findall("r_ele")]:
                    w = unicodedata.normalize("NFKC", h or "")
                    if is_candidate(w):
                        for d in doms:
                            jm[d].add(w); example.setdefault((d, w), "JMdict:" + e.findtext("ent_seq"))
            e.clear()
    out = {}
    for dom in set(docs) | set(jm):
        rows = []
        for w in set(docs.get(dom, {})) | jm.get(dom, set()):
            n = docs.get(dom, {}).get(w, 0)
            rate = n / max(1, npages.get(dom, 1))
            rates = [docs[o].get(w, 0) / max(1, npages[o]) for o in docs]
            other_rate = max([docs[o].get(w, 0) / npages[o] for o in docs if o != dom] or [0.0])
            spread = sum(r >= 0.0005 for r in rates)
            # 記事の冒頭に 3 件以上出て、いちばん多く出る分野の半分以上で、5 分野以上に千に 0.5 件以上出る語 (JR・NHK などの一般の語) でない
            # (または JMdict でこの分野の印)
            if w in jm.get(dom, set()) or (n >= a.min_docs and rate >= 0.5 * max(rates) and spread <= 4):
                rows.append({"word": w, "docs": n, "rate": round(rate * 1000, 2), "other": round(other_rate * 1000, 2),
                             "jmdict": w in jm.get(dom, set()), "example": example.get((dom, w), "")})
        # 同じ語の大文字小文字違い (Mrna/mRNA) は記事に多いほうだけ
        best = {}
        for r in rows:
            k = r["word"].lower()
            if k not in best or (r["docs"], r["jmdict"]) > (best[k]["docs"], best[k]["jmdict"]):
                best[k] = r
        out[dom] = sorted(best.values(), key=lambda r: (-r["docs"], r["word"]))
        print(dom, npages.get(dom, 0), "pages,", len(out[dom]), "candidates")
    json.dump(out, open(a.out, "w", encoding="utf-8"), ensure_ascii=False, indent=0)


SCHEMA = {"type": "object", "additionalProperties": False, "required": ["items"],
          "properties": {"items": {"type": "array", "items": {
              "type": "object", "additionalProperties": False, "required": ["id", "keep"],
              "properties": {"id": {"type": "string"}, "keep": {"type": "boolean"}}}}}}


def rules(field):
    return f"""各要素の word は、「{field}」の分野の文章 (日本語) に出てくる英字の語である。example はその語が出てきた所。
keep=true: {field}の文章を書く人 (学生・専門家) が、日本語の文の中に英字のまま打つ略語・記号で、この大文字小文字の表記が正しいもの
  (例: 医学なら MRI・CRP・NSAIDs・IgG、生物なら mRNA・ATP・NADPH、情報なら CPU・API・RSA、歴史なら GHQ・NATO)。{field}でよく使う団体や規格の略称も true。
keep=false: 会社・製品・作品・人・場所の名前 (IBM・NTT・NHK など。団体の略称で{field}の文章によく出るものは除く)、型番・登録番号・カタログ番号・識別子 (CAS・INN・NGC・HD・ISBN)、
  大文字で書いた普通の英単語 (THE・JAPAN)、化学式・単位・ローマ数字、{field}とは関係の薄い語、語の切れ端・誤記、表記の大文字小文字が正しくないもの。
迷ったら false にする。全件を返す。id はそのまま返す。"""


def judge_batch(path, items, schema, empty, model, field):
    if os.path.exists(path):
        return "skip"
    prompt = rules(field) + "\n\n入力:\n" + json.dumps(items, ensure_ascii=False) + "\n\n出力は {\"items\": [...]} の JSON だけ。ツールやファイルは使わない。"
    env = dict(os.environ, PATH=(CODEX_PATH + ":" if CODEX_PATH else "") + os.environ["PATH"])
    for _ in range(3):
        tmp = path + ".tmp"
        try:
            subprocess.run(["codex", "exec", "--skip-git-repo-check", "--ephemeral", "-m", model, "-c", "model_reasoning_effort=low",
                            "-s", "read-only", "--output-schema", schema, "-o", tmp, prompt],
                           cwd=empty, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=1800)
            got = json.load(open(tmp, encoding="utf-8"))["items"]
        except Exception:
            continue
        if {g["id"] for g in got} >= {it["id"] for it in items}:
            json.dump(got, open(path, "w", encoding="utf-8"))
            os.remove(tmp)
            return "ok"
    return "fail"


def judge(a):
    rows = json.load(open(a.cands, encoding="utf-8"))[a.dom]
    wd = os.path.abspath(a.workdir)
    os.makedirs(os.path.join(wd, "empty"), exist_ok=True)
    digest = hashlib.sha256(("\n".join(r["word"] for r in rows) + a.field).encode()).hexdigest()
    stamp = os.path.join(wd, "input.sha256")
    if os.path.exists(stamp) and open(stamp).read() != digest:
        sys.exit(f"{wd} は別の入力で判定したもの (消すか別のフォルダに)")
    open(stamp, "w").write(digest)
    schema = os.path.join(wd, "schema.json")
    json.dump(SCHEMA, open(schema, "w"))
    items = [{"id": str(i), "word": r["word"], "example": r["example"]} for i, r in enumerate(rows)]
    jobs = [(os.path.join(wd, f"{k // a.batch:05d}.json"), items[k:k + a.batch]) for k in range(0, len(items), a.batch)]
    with concurrent.futures.ThreadPoolExecutor(a.jobs) as pool:
        results = list(pool.map(lambda j: judge_batch(j[0], j[1], schema, os.path.join(wd, "empty"), a.model, a.field), jobs))
    print(a.dom, {r: results.count(r) for r in set(results)})


def write(a):
    rows = json.load(open(a.cands, encoding="utf-8"))[a.dom]
    keep = set()
    for name in sorted(os.listdir(a.workdir)):
        if re.fullmatch(r"\d{5}\.json", name):
            keep |= {int(g["id"]) for g in json.load(open(os.path.join(a.workdir, name), encoding="utf-8")) if g["keep"]}
    spec = importlib.util.spec_from_file_location("ime_readable", a.readable)
    readable = importlib.util.module_from_spec(spec); spec.loader.exec_module(readable)
    tables = set()
    for name in ("english_words_scowl.txt", "english_terms.txt", "english_romaji_priority.txt", "english_case.txt", "english_boundary_words.txt"):
        for line in open(os.path.join(REPO, name), encoding="utf-8"):
            w = line.split("\t")[0].strip()
            if w and not w.startswith("#"):
                tables.add(w.lower())
    out, log = [], []
    for i, r in enumerate(rows):
        w = r["word"]
        why = ("6.1-sol で分野の略語でない" if i not in keep else
               "小文字で打つとローマ字として読める" if readable.readable(w) else
               "今の英字の判定の表にある" if w.lower() in tables else "")
        (log if why else out).append((w, why, r))
    with open(a.out, "w", encoding="utf-8", newline="\n") as f:
        f.write(f"# {a.field}の英字の略語 (よく打つ分野に選んだ人の英字の判定に足す)。tools/domain/abbrev.py が書く。\n"
                "# 材料: 日本語版ウィキペディアの{0}の記事の冒頭 (CC BY-SA 4.0) と JMdict (CC BY-SA 4.0)。6.1-sol で{0}の略語かを判定した\n".format(a.field))
        f.writelines(w + "\n" for w, _, _ in sorted(out, key=lambda x: x[0].lower()))
    # 出どころ (ライセンス表示の記事の一覧に入れる。build_domain.py が読む)
    with open(os.path.splitext(a.out)[0] + "_sources.tsv", "w", encoding="utf-8", newline="\n") as f:
        f.write("# 略語<TAB>出どころ (Wikipedia:<記事名> は冒頭にその略語が出てくる記事の 1 つ、JMdict:<番号> は JMdict の見出し)\n")
        f.writelines(f"{w}\t{r['example']}\n" for w, _, r in sorted(out, key=lambda x: x[0].lower()))
    with open(a.log, "w", encoding="utf-8", newline="\n") as f:
        f.write("# 語\t入れた/除いた理由\t記事の数\t出どころの例\n")
        for w, why, r in out + log:
            f.write(f"{w}\t{why or '入れた'}\t{r['docs']}\t{r['example']}\n")
    print(a.dom, "abbrev", len(out), "excluded", collections.Counter(why for _, why, _ in log))


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("candidates"); c.add_argument("--pages", nargs="+", required=True); c.add_argument("--jmdict")
    c.add_argument("--out", required=True); c.add_argument("--min-docs", type=int, default=3)
    j = sub.add_parser("judge"); j.add_argument("cands"); j.add_argument("dom"); j.add_argument("workdir"); j.add_argument("--field", required=True)
    j.add_argument("--jobs", type=int, default=3); j.add_argument("--batch", type=int, default=150); j.add_argument("--model", default="gpt-6.1-sol")
    w = sub.add_parser("write"); w.add_argument("cands"); w.add_argument("dom"); w.add_argument("workdir"); w.add_argument("--field", required=True)
    w.add_argument("--readable", required=True); w.add_argument("--out", required=True); w.add_argument("--log", required=True)
    a = ap.parse_args()
    {"candidates": candidates, "judge": judge, "write": write}[a.cmd](a)


if __name__ == "__main__":
    main()
