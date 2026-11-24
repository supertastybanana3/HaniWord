"""分野の語の読みを今の辞書で変換した結果 (例: いにょうち → 胃尿地、けんけつ → 献血) が、
一般の人がその読みで打ちたい自然な日本語かを GPT-6 luna に判定させる。
自然なら分野の語はそれより下の重みで足し (候補には出るが 1 位は今のまま)、意味の通らない切れ端なら分野の語が勝つ重みにする。
入力 (TSV): 読み<TAB>今の変換<TAB>分野の語。出力 (TSV): 読み / 今の変換 / 分野の語 / 自然か (1/0)
使い方: python3 luna_natural.py <in.tsv> <workdir> <out.tsv> [--jobs 6]
"""
import argparse, concurrent.futures, hashlib, json, os, random, subprocess

# codex の置き場 (PATH に無いときだけ、環境変数 CODEX_BIN_DIR で渡す)
CODEX_PATH = os.environ.get("CODEX_BIN_DIR", "")
SCHEMA = {"type": "object", "additionalProperties": False, "required": ["items"],
          "properties": {"items": {"type": "array", "items": {
              "type": "object", "additionalProperties": False, "required": ["id", "natural"],
              "properties": {"id": {"type": "string"}, "natural": {"type": "boolean"}}}}}}
RULES = """各要素は、日本語の IME で読み (reading) を打ったときの今の変換結果 (current) と、ある専門分野の語 (term) である。
current が「専門分野を知らない一般の人がその読みを打って書きたい、自然な日本語」なら natural=true、
意味の通らない切れ端・ありえない語のつながり・ふつう使わない語の組み合わせ (例: いにょうち→胃尿地、そくじこうこく→即時広告、はあく→は悪、
おうしゅうぶつ→欧州物、かいぎめんじょう→会議免状) なら natural=false。一般の人がその読みで打つ場面がほとんど無ければ false にする。
例: けんけつ→献血 は true (よく使う語)、かくいでんし→各遺伝子 は true (各＋遺伝子 として自然)、てつづき→手続き は true (表記が違うだけ)。
term の良し悪しではなく current だけを判定する。全件を返す。id はそのまま返す。"""


def run(path, items, schema, empty, model="gpt-6-luna"):
    if os.path.exists(path):
        return "skip"
    prompt = RULES + "\n\n入力:\n" + json.dumps(items, ensure_ascii=False) + "\n\n出力は {\"items\": [...]} の JSON だけ。ツールやファイルは使わない。"
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
    ap.add_argument("cases")
    ap.add_argument("workdir")
    ap.add_argument("out")
    ap.add_argument("--jobs", type=int, default=6)
    ap.add_argument("--batch", type=int, default=200)
    ap.add_argument("--model", default="gpt-6.1-sol", help="luna は甘く判定しがち (押収物→欧州物 を自然とする) なので 6.1-sol")
    a = ap.parse_args()
    rows = [line.rstrip("\n").split("\t") for line in open(a.cases, encoding="utf-8") if line.strip() and not line.startswith("#")]
    wd = os.path.abspath(a.workdir)
    # 束の結果は束の番号で残すので、入力が変わったら前の結果は使えない (別のフォルダに移す)
    digest = hashlib.sha256(open(a.cases, "rb").read()).hexdigest()
    stamp = os.path.join(wd, "input.sha256")
    if os.path.exists(stamp) and open(stamp).read() != digest:
        os.rename(wd, wd + ".old_" + open(stamp).read()[:8])
    empty = os.path.join(wd, "empty")
    os.makedirs(empty, exist_ok=True)
    open(stamp, "w").write(digest)
    schema = os.path.join(wd, "schema.json")
    json.dump(SCHEMA, open(schema, "w"))
    order = list(range(len(rows)))
    random.Random(4).shuffle(order)
    jobs = []
    for k in range(0, len(order), a.batch):
        items = [{"id": str(i), "reading": rows[i][0], "current": rows[i][1], "term": rows[i][2]} for i in order[k:k + a.batch]]
        jobs.append((os.path.join(wd, f"{k // a.batch:05d}.json"), items))
    with concurrent.futures.ThreadPoolExecutor(a.jobs) as pool:
        results = list(pool.map(lambda j: run(j[0], j[1], schema, empty, a.model), jobs))
    print({r: results.count(r) for r in set(results)})
    natural = {}
    for path, _ in jobs:
        if os.path.exists(path):
            for g in json.load(open(path, encoding="utf-8")):
                natural[int(g["id"])] = g["natural"]
    with open(a.out, "w", encoding="utf-8") as out:
        for i, r in enumerate(rows):
            if i in natural:
                out.write(f"{r[0]}\t{r[1]}\t{r[2]}\t{int(natural[i])}\n")


if __name__ == "__main__":
    main()
