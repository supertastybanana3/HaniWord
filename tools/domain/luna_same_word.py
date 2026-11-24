"""分野の語と、その読みでの今の変換について、今の変換と分野の語が
「同じ語の書き方の違い」(蛋白質/タンパク質、燐酸化/リン酸化、菊科/キク科) かを 6.1-sol に判定させ、
同じ語なら、その分野の文章でふつう使う書き方がどちらかも答えさせる。
今の変換のほうがふつうなら、分野の語は入れても得がなく、同音語の優遇で今の正しい書き方を押しのける (リン酸化 → 燐酸化) ので入れない。
「今の変換が自然か」が 0 の語も見る: 専門の語は「一般の人が打つ自然な日本語」ではないと判定されるので、今の変換が分野の正しい書き方
(酸化的リン酸化) でも、分野の語 (酸化的燐酸化) が勝つ重みで入っていた。
入力: 分野辞書の TSV (domain_dictionaries/<名前>.tsv)。出力 (TSV): 表記 / 読み / 今の変換 / 同じ語か (1/0) / ふつうの書き方 (current か term)
使い方: python3 luna_same_word.py <名前>.tsv <作業フォルダ> <出力.tsv> --field 生物 [--which all|natural|broken] [--jobs 6]
"""
import argparse, concurrent.futures, hashlib, json, os, random, subprocess

# codex の置き場 (PATH に無いときだけ、環境変数 CODEX_BIN_DIR で渡す)
CODEX_PATH = os.environ.get("CODEX_BIN_DIR", "")
SCHEMA = {"type": "object", "additionalProperties": False, "required": ["items"],
          "properties": {"items": {"type": "array", "items": {
              "type": "object", "additionalProperties": False, "required": ["id", "same", "prefer"],
              "properties": {"id": {"type": "string"}, "same": {"type": "boolean"},
                             "prefer": {"type": "string", "enum": ["current", "term"]}}}}}}


def rules(field):
    return f"""各要素は、「{field}」の分野の語 term と、同じ読み (reading) を今の IME で変換した結果 current である。
current と term が同じ語 (同じものを指す語) の書き方の違いなら same=true。書き方の違いとは、漢字とかな・カタカナの違い、
旧字や常用外の字と常用の字、送り仮名、別の漢字の当て方 (例: 蛋白質/タンパク質、燐酸化/リン酸化、醗酵/発酵、菊科/キク科、カラフト鱒/樺太マス、
円すい細胞/円錐細胞、取消し/取り消し、ウマの進化/馬の進化)。
same=true のときは、{field}の教科書・論文・解説でふつう使う書き方を prefer に入れる (current か term。迷ったら current)。
違う語 (指すものが違う。例: 監理官/管理官、施行/思考、核小体/各小隊、速筋/側近) なら same=false、prefer=term。全件を返す。id はそのまま返す。"""


def run(path, items, schema, empty, model, field):
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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tsv")
    ap.add_argument("workdir")
    ap.add_argument("out")
    ap.add_argument("--field", required=True)
    ap.add_argument("--jobs", type=int, default=6)
    ap.add_argument("--batch", type=int, default=100)
    ap.add_argument("--model", default="gpt-6.1-sol")
    ap.add_argument("--which", choices=["all", "natural", "broken"], default="all", help="今の変換が自然とされた語だけ (natural)・そうでない語だけ (broken)・両方")
    a = ap.parse_args()
    rows = []
    for line in open(a.tsv, encoding="utf-8"):
        if line.startswith("#") or not line.strip():
            continue
        c = line.rstrip("\n").split("\t")
        if c[0] != c[6] and (a.which == "all" or (c[7] == "1") == (a.which == "natural")):
            rows.append((c[0], c[1], c[6]))
    rows = sorted(set(rows))
    wd = os.path.abspath(a.workdir)
    # 束の結果は束の番号で残すので、入力が変わったら前の結果は使えない (別のフォルダに移す)
    digest = hashlib.sha256(("\n".join("\t".join(r) for r in rows) + a.field).encode("utf-8")).hexdigest()
    stamp = os.path.join(wd, "input.sha256")
    if os.path.exists(stamp) and open(stamp).read() != digest:
        os.rename(wd, wd + ".old_" + open(stamp).read()[:8])
    empty = os.path.join(wd, "empty")
    os.makedirs(empty, exist_ok=True)
    open(stamp, "w").write(digest)
    schema = os.path.join(wd, "schema.json")
    json.dump(SCHEMA, open(schema, "w"))
    order = list(range(len(rows)))
    random.Random(5).shuffle(order)
    jobs = []
    for k in range(0, len(order), a.batch):
        items = [{"id": str(i), "reading": rows[i][1], "current": rows[i][2], "term": rows[i][0]} for i in order[k:k + a.batch]]
        jobs.append((os.path.join(wd, f"{k // a.batch:05d}.json"), items))
    with concurrent.futures.ThreadPoolExecutor(a.jobs) as pool:
        results = list(pool.map(lambda j: run(j[0], j[1], schema, empty, a.model, a.field), jobs))
    print({r: results.count(r) for r in set(results)})
    got = {}
    for path, _ in jobs:
        if os.path.exists(path):
            for g in json.load(open(path, encoding="utf-8")):
                got[int(g["id"])] = g
    with open(a.out, "w", encoding="utf-8") as out:
        out.write("# 表記\t読み\t今の変換\t同じ語か\tふつうの書き方\n")
        for i, r in enumerate(rows):
            if i in got:
                out.write(f"{r[0]}\t{r[1]}\t{r[2]}\t{int(got[i]['same'])}\t{got[i]['prefer']}\n")


if __name__ == "__main__":
    main()
