"""分野辞書の語に GPT-6 luna (codex exec -m gpt-6-luna) で読みを付け、その分野の用語かを判定させる。
形態素解析の読みは使わない (誤りが多いため)。

  run   --pass a / b : 束の組み方を変えて 2 回、別々に付ける (Web 検索なし)
  run   --pass c     : a と b で読みが食い違った語・自信なしの語だけ、Web 検索 (日本語版ウィキペディアなど) で確かめて決める
  todo               : c に回す語を書き出す
  merge              : 決まった読みと判定を 1 つの TSV にする
  (節約の流し方。Codex の枠が少ないとき。2026-10-21)
  todo_b_lite        : a の読みが参考の読み (人が付けた辞書・ウィキペディア) と合わない語・参考の読みが無い語だけ b に回す
  todo_c_lite        : 参考の読みと a・b のどちらも合わない語・参考の読みが無く a と b が食い違う語だけ f (luna、Web) に回す。d・e は使わない
                       (todo_f.tsv・todo_f_in.tsv。前の流し方の c で答えのある語は回さない)
  merge_lite         : c > 参考の読みと合った a か b > a と b の一致、の順で決める

入力 (TSV): 表記<TAB>例 (その語が出てきた法令名や文の一部。判断の手がかり)
作業フォルダ: <workdir>/pass_<a|b|c>/NNNN.json (束ごとの結果。途中で止めても続きから)
使い方: python3 luna_readings.py run --terms <in.tsv> --workdir <dir> --pass a --rules <分野の決まり.txt> [--jobs 8]
        python3 luna_readings.py todo --workdir <dir>        (→ <dir>/todo_c.tsv)
        python3 luna_readings.py run --terms <dir>/todo_c.tsv --workdir <dir> --pass c --rules ...
        python3 luna_readings.py merge --workdir <dir> --out <out.tsv>
"""
import argparse, concurrent.futures, glob, hashlib, json, os, random, re, subprocess, sys, tempfile

# codex の置き場 (PATH に無いときだけ、環境変数 CODEX_BIN_DIR で渡す)
CODEX_PATH = os.environ.get("CODEX_BIN_DIR", "")
KINDS = ["分野の用語", "法令・制度の名前", "機関の名前", "一般語", "地名", "人名", "組織・会社の名前", "他分野の用語", "切れ端"]
SCHEMA = {
    "type": "object", "additionalProperties": False, "required": ["items"],
    "properties": {"items": {"type": "array", "items": {
        "type": "object", "additionalProperties": False,
        "required": ["id", "reading", "alt", "kind", "keep", "confidence", "source"],
        "properties": {
            "id": {"type": "string"},
            "reading": {"type": "string"},
            "alt": {"type": "array", "items": {"type": "string"}},
            "kind": {"type": "string", "enum": KINDS},
            "keep": {"type": "boolean"},
            "confidence": {"type": "string", "enum": ["high", "low"]},
            "source": {"type": "string"},
        }}}},
}
COMMON_RULES = """各語について、日本語の IME（ローマ字かな入力）でその語を打つ人が入力する「読み」をカタカナで書け。
- 読みは表記の全体（送り仮名も含む）。申立て=モウシタテ、差押え=サシオサエ。長音は綴りどおり（効率=コウリツ、大きい=オオキイ）
- 連濁・音便に注意する（株式会社=カブシキガイシャ、手続=テツヅキ）
- その分野で一般の読みと別の読みが使われる語は、reading にいちばんよく使われる読み、alt にほかの読みを入れる（例: 遺言 reading=ユイゴン alt=[イゴン]）。無ければ alt は空
- 読みに自信がなければ confidence を low にする
- kind はその語の種類。keep は「この分野の文章を書く人が 1 語として打つ語で、分野の辞書に入れる価値があるか」（どの種類を入れるかは下の分野の決まりに従う）
  - 切れ端（語の途中で切れたもの・語が不自然につながったもの・助詞や数を含むもの）・他分野の用語・ありふれた一般語は keep=false
- source は読みを確かめた URL（Web を調べたときだけ。調べていなければ空文字）
- 全件を返す。id はそのまま返す"""


SEARCH_RULES = """この回は読みを Web で確かめる回である。各語について必ず Web 検索をして読みを確かめ、確かめたページの URL を source に書く。
- 日本語版ウィキペディアの記事の冒頭の括弧の読みを最優先する。無ければ辞書・法令の解説・官公庁のページ
- 語そのものが見つからなければ、語を組み立てている部分 (例: 供託書 → 供託) の読みを確かめ、その URL を書く
- 調べても確かめられなければ source は空にし、confidence を low にする
- example に 1・2 回目の読みがある。どちらが正しいか、どちらも誤りかを調べて決める"""


def katakana(s):
    return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in re.sub(r"[\s・]", "", s))


def batches(terms, size, seed):
    order = list(range(len(terms)))
    random.Random(seed).shuffle(order)
    return [order[i:i + size] for i in range(0, len(order), size)]


def run_batch(path, items, rules, effort, search, empty, model="gpt-6-luna"):
    if os.path.exists(path):
        return "skip"
    prompt = (COMMON_RULES + "\n\n" + rules + "\n\n入力（JSON の配列。各要素は id・word・example）:\n" + json.dumps(items, ensure_ascii=False)
              + "\n\n出力は {\"items\": [...]} の JSON だけ。" + (SEARCH_RULES if search else "ツールやファイルは使わない。"))
    schema = os.path.join(os.path.dirname(empty), "schema.json")
    env = dict(os.environ, PATH=(CODEX_PATH + ":" if CODEX_PATH else "") + os.environ["PATH"])
    for attempt in range(3):
        tmp = path + ".tmp"
        cmd = ["codex"] + (["--search"] if search else []) + ["exec", "--skip-git-repo-check", "--ephemeral", "-m", model,
                                                             "-c", f"model_reasoning_effort={effort}", "-s", "read-only",
                                                             "--output-schema", schema, "-o", tmp, prompt]
        try:
            subprocess.run(cmd, cwd=empty, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           timeout=1800)
            got = json.load(open(tmp, encoding="utf-8"))["items"]
        except Exception:
            continue
        ids = {it["id"] for it in items}
        for g in got:
            g["reading"] = katakana(g["reading"])
            g["alt"] = [katakana(x) for x in g["alt"] if re.fullmatch(r"[ァ-ヺー]+", katakana(x))]
        good = [g for g in got if g["id"] in ids and re.fullmatch(r"[ァ-ヺー]+", g["reading"])]
        if len({g["id"] for g in good}) >= len(ids) * 0.95:
            json.dump(good, open(path, "w", encoding="utf-8"), ensure_ascii=False, indent=0)
            os.remove(tmp)
            return "ok"
    return "fail"


def cmd_run(a):
    terms = []
    for line in open(a.terms, encoding="utf-8"):
        if line.strip() and not line.startswith("#"):
            c = line.rstrip("\n").split("\t")
            terms.append((c[0], c[1] if len(c) > 1 else ""))
    rules = open(a.rules, encoding="utf-8").read() if a.rules else ""
    a.workdir = os.path.abspath(a.workdir)
    out = os.path.join(a.workdir, "pass_" + a.pass_)
    empty = os.path.join(a.workdir, "empty")
    # 束の結果は束の番号で残すので、入力が変わったら前の結果は使えない (別のフォルダに移す)
    digest = hashlib.sha256(open(a.terms, "rb").read()).hexdigest()
    stamp = os.path.join(out, "input.sha256")
    if os.path.exists(stamp) and open(stamp).read() != digest:
        os.rename(out, out + ".old_" + open(stamp).read()[:8])
    os.makedirs(out, exist_ok=True)
    open(stamp, "w").write(digest)
    os.makedirs(empty, exist_ok=True)
    json.dump(SCHEMA, open(os.path.join(a.workdir, "schema.json"), "w"))
    seed = {"a": 1, "b": 2, "c": 3, "d": 4, "e": 5, "f": 6}[a.pass_]
    search = a.pass_ in ("c", "d", "e", "f")
    size = a.batch or (10 if search else 150)
    effort = a.effort or ("high" if search else "low")
    model = a.model or {"d": "gpt-5.6-terra", "e": "gpt-6.1-sol"}.get(a.pass_, "gpt-6-luna")
    jobs = []
    for k, idx in enumerate(batches(terms, size, seed)):
        items = [{"id": str(i), "word": terms[i][0], "example": terms[i][1]} for i in idx]
        jobs.append((os.path.join(out, f"{k:05d}.json"), items))
    # 束の中の id は入力の行番号。入力の写しを残して、merge で表記に戻す
    json.dump([t[0] for t in terms], open(os.path.join(out, "terms.json"), "w", encoding="utf-8"), ensure_ascii=False)
    done = 0
    with concurrent.futures.ThreadPoolExecutor(a.jobs) as pool:
        futs = [pool.submit(run_batch, p, items, rules, effort, search, empty, model) for p, items in jobs]
        for f in concurrent.futures.as_completed(futs):
            done += 1
            if done % 10 == 0 or f.result() == "fail":
                print(f"{done}/{len(jobs)} {f.result()}", flush=True)
    print("finished", sum(os.path.exists(p) for p, _ in jobs), "/", len(jobs))


def one_line(x):
    """確かめた URL などに改行やタブが入っていると TSV の行が割れる (歴史 wd1 で組み上げが落ちた) ので、空白にする"""
    return " ".join(str(x).split())


def load_pass(workdir, p):
    d = os.path.join(workdir, "pass_" + p)
    if not os.path.exists(os.path.join(d, "terms.json")):
        return {}
    terms = json.load(open(os.path.join(d, "terms.json"), encoding="utf-8"))
    res = {}
    for path in glob.glob(os.path.join(d, "[0-9]*.json")):
        for g in json.load(open(path, encoding="utf-8")):
            res[terms[int(g["id"])]] = g
    return res


def strong_ref(src):
    """人が付けた辞書の読み (strong_refs.py の出どころ: Mozc の公開辞書・候補の TSV の JMdict など)。
    Wikipedia の記事の頭から取った読み (wikipedia・jawiki) には、途中で切れた読み (邯鄲市 カンタン) や 2 つがつながった読み (丹沢山 タンザワサンタンザワヤマ) がある"""
    return src == "mozc" or src.startswith("cand:")


def load_refs(paths):
    """reference_readings.py・strong_refs.py の出力: 表記 → {読み: 出どころ}。同じ読みが両方にあれば、人が付けた辞書の出どころを残す"""
    refs = {}
    for path in paths or []:
        for line in open(path, encoding="utf-8"):
            t, r, src = line.rstrip("\n").split("\t")
            if not strong_ref(refs.setdefault(t, {}).get(r, "")):
                refs[t][r] = src
    return refs


def cmd_todo(a):
    pa, pb = load_pass(a.workdir, "a"), load_pass(a.workdir, "b")
    refs = load_refs(a.refs)
    n = 0
    with open(os.path.join(a.workdir, "todo_c.tsv"), "w", encoding="utf-8") as out:
        for term in sorted(set(pa) & set(pb)):
            x, y = pa[term], pb[term]
            if not (x["keep"] or y["keep"]):
                continue  # 2 回とも辞書に入れないと判断した語は調べない
            note = []
            if x["reading"] != y["reading"] or "low" in (x["confidence"], y["confidence"]) or set(x["alt"]) != set(y["alt"]):
                note.append(f"a={x['reading']}{x['alt']} b={y['reading']}{y['alt']}")
            if x["keep"] != y["keep"]:
                note.append(f"入れるかの判断が分かれた (a={x['kind']} b={y['kind']})")
            known = refs.get(term, {})
            if known and not ({x["reading"], *x["alt"]} & set(known)):
                # ウィキペディアなどの読みと合わない (どれか 1 つでも合えばよい)
                note.append("参考の読み: " + ", ".join(f"{r}({s})" for r, s in known.items()) + f" luna={x['reading']}")
            if note:
                out.write(f"{term}\t{' / '.join(note)}\n")
                n += 1
    print("todo_c", n, "of", len(set(pa) & set(pb)))


def cmd_todo_e(a):
    """c (luna) と d (terra) で読みか入れるかの判断が食い違った語を、e (6.1-sol) に回す"""
    pa, pb, pc, pd = (load_pass(a.workdir, p) for p in "abcd")
    n = 0
    with open(os.path.join(a.workdir, "todo_e.tsv"), "w", encoding="utf-8") as out:
        for term in sorted(set(pc) & set(pd)):
            c, d = pc[term], pd[term]
            if c["reading"] != d["reading"] or c["keep"] != d["keep"]:
                note = " / ".join(f"{k}: 読み={g['reading']}{g['alt']} 種類={g['kind']} 入れる={g['keep']}"
                                  for k, g in (("1回目", pa[term]), ("2回目", pb[term]), ("3回目", c), ("4回目", d)))
                out.write(f"{term}\t{note}\n")
                n += 1
    print("todo_e", n)


def lite_state(a):
    """節約の流し方の材料。Web で確かめた結果は、前の流し方の c と、節約の流し方の f (f が優先)"""
    pa, pb, pc, pf = (load_pass(a.workdir, p) for p in "abcf")
    refs = load_refs(a.refs)
    return pa, pb, dict(pc, **pf), refs


def readings_of(g):
    return {g["reading"], *g["alt"]}


def cmd_todo_b_lite(a):
    pa, pb, _, refs = lite_state(a)
    terms = dict(l.rstrip("\n").split("\t", 1) for l in open(a.terms, encoding="utf-8") if "\t" in l)
    n = 0
    with open(os.path.join(a.workdir, "todo_b.tsv"), "w", encoding="utf-8") as out:
        for term, x in sorted(pa.items()):
            if not x["keep"] or term in pb:
                continue  # 入れない語・b が既にある語 (前の流し方で全部に b を付けた束)
            if readings_of(x) & set(refs.get(term, {})):
                continue  # 参考の読みと合った
            out.write(f"{term}\t{terms.get(term, '')}\n")
            n += 1
    print("todo_b", n, "of", len(pa))


def cmd_todo_c_lite(a):
    pa, pb, pcf, refs = lite_state(a)
    terms = dict(l.rstrip("\n").split("\t", 1) for l in open(a.terms, encoding="utf-8") if "\t" in l)
    n = 0
    with open(os.path.join(a.workdir, "todo_f.tsv"), "w", encoding="utf-8") as out, \
            open(os.path.join(a.workdir, "todo_f_in.tsv"), "w", encoding="utf-8") as out_in:
        for term, x in sorted(pa.items()):
            y = pb.get(term)
            if term in pcf:
                continue  # 前の流し方の c で Web で確かめた
            if not (x["keep"] or (y and y["keep"])):
                continue
            known = set(refs.get(term, {}))
            if readings_of(x) & known or (y and readings_of(y) & known):
                continue
            if not known and y and x["reading"] == y["reading"]:
                continue
            if not known and y is None:
                continue  # b がまだ (todo_b に入っていない語は a で決まっている)
            note = f"a={x['reading']}{x['alt']}" + (f" b={y['reading']}{y['alt']}" if y else "")
            if known:
                note += " / 参考の読み: " + ", ".join(f"{r}({s})" for r, s in refs[term].items())
            out.write(f"{term}\t{note}\n")
            out_in.write(f"{term}\t出てきたところ: {terms.get(term, '')} / 1・2 回目の結果: {note}\n")
            n += 1
    print("todo_f", n, "of", len(pa))


def cmd_merge_lite(a):
    pa, pb, pc, refs = lite_state(a)
    with open(a.out, "w", encoding="utf-8") as out:
        out.write("# 表記\t読み\tほかの読み\t種類\t入れる\t決め方\t確かめた URL\n")
        for term, x in sorted(pa.items()):
            y, z, known = pb.get(term), pc.get(term), refs.get(term, {})
            if z is not None:
                g, how, keep = z, "c", z["keep"]
            elif readings_of(x) & set(known):
                g, how, keep = x, "a+ref", x["keep"]
            elif y is not None and readings_of(y) & set(known):
                g, how, keep = y, "b+ref", x["keep"] and y["keep"]
            elif not known and y is not None and x["reading"] == y["reading"]:
                g, how, keep = x, "ab", x["keep"] and y["keep"]
            elif not x["keep"] and not (y and y["keep"]):
                g, how, keep = x, "a", False  # 入れない語 (読みは使わない)
            else:
                continue  # 決まっていない (c がまだ)
            reading = g["reading"]
            if how.endswith("+ref") and reading not in known:
                reading = sorted(readings_of(g) & set(known))[0]  # 主な読みが参考と違い、ほかの読みが合った
            if how.endswith("+ref"):
                # ほかの読みは、luna と参考の両方が挙げた読みと、人が付けた辞書の読みだけ。luna だけが挙げた読みは確かめていない (復号化 フクゴウ)
                alt = (readings_of(g) & set(known)) | {r for r, s in known.items() if strong_ref(s)}
            elif how == "ab":
                alt = set(x["alt"]) & set(y["alt"])  # 1・2 回目の両方が挙げた読みだけ (cmd_merge の ab と同じ)
            else:
                alt = readings_of(g)
            alt = sorted(alt - {reading})
            out.write(f"{term}\t{reading}\t{','.join(alt)}\t{g['kind']}\t{int(keep)}\t{how}\t{one_line(g.get('source', ''))}\n")


def cmd_merge(a):
    pa, pb, pc, pd, pe = (load_pass(a.workdir, p) for p in "abcde")
    with open(a.out, "w", encoding="utf-8") as out:
        out.write("# 表記\t読み\tほかの読み\t種類\t入れる\t決め方\t確かめた URL\n")
        for term in sorted(set(pa) & set(pb)):
            x, y = pa[term], pb[term]
            if term in pe:
                # c と d が食い違い、6.1-sol (e) が Web で確かめて決めた
                e = pe[term]
                others = {pc[term]["reading"], pd[term]["reading"]} - {e["reading"]}
                z, how = dict(e, alt=sorted(set(e["alt"]))), "e"
            elif term in pc and term in pd:
                # luna (c) と terra (d) が Web で確かめた読み。主な読みが同じならそれ、違えば a・b の読みと合うほうを主にして、もう一方はほかの読みに
                c, d = pc[term], pd[term]
                if c["reading"] == d["reading"]:
                    z, how = dict(c, alt=sorted(set(c["alt"]) & set(d["alt"]))), "cd"
                else:
                    ab = {x["reading"], y["reading"]}
                    first, second = (d, c) if d["reading"] in ab and c["reading"] not in ab else (c, d)
                    z, how = dict(first, alt=sorted({second["reading"]})), "cd?"
                z["keep"] = c["keep"] and d["keep"]
            elif term in pc:
                z, how = pc[term], "c"
            elif x["reading"] == y["reading"] and "low" not in (x["confidence"], y["confidence"]) and set(x["alt"]) == set(y["alt"]) \
                    and x["keep"] == y["keep"]:
                z, how = x, "ab"
            else:
                continue  # 食い違ったまま (c がまだ)
            keep = z["keep"] if how == "c" else (x["keep"] and y["keep"])
            out.write(f"{term}\t{z['reading']}\t{','.join(z['alt'])}\t{z['kind']}\t{int(keep)}\t{how}\t{one_line(z.get('source', ''))}\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["run", "todo", "todo_e", "merge", "todo_b_lite", "todo_c_lite", "merge_lite"])
    ap.add_argument("--terms")
    ap.add_argument("--workdir", required=True)
    ap.add_argument("--pass", dest="pass_", choices=["a", "b", "c", "d", "e", "f"])
    ap.add_argument("--model", default=None, help="既定は gpt-6-luna (d は gpt-5.6-terra、e は gpt-6.1-sol)")
    ap.add_argument("--rules")
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--batch", type=int)
    ap.add_argument("--effort")
    ap.add_argument("--out")
    ap.add_argument("--refs", nargs="*", help="todo: reference_readings.py の出力 (ウィキペディアなどの読み)")
    a = ap.parse_args()
    {"run": cmd_run, "todo": cmd_todo, "todo_e": cmd_todo_e, "merge": cmd_merge,
     "todo_b_lite": cmd_todo_b_lite, "todo_c_lite": cmd_todo_c_lite, "merge_lite": cmd_merge_lite}[a.cmd](a)


if __name__ == "__main__":
    main()
