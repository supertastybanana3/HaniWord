"""分野の「目印の語」を選ぶ: 文脈にこの語が出ていれば、その分野の文とみなして同音語を優遇する (変換サーバーの DomainBoost)。
一般の文章にもよく出る語 (場合・主張・必要) を目印にすると、一般の文でも優遇が働いてしまうので、
「その分野の文章に特有で、一般の文章 (ニュース・ブログ・小説・教科書のほかの科目) にはあまり出ない語」だけを選ばせる。
判定は 6.1-sol (luna は判定が甘い)。入力: 1 行 1 語。出力: 目印にする語 (1 行 1 語)
使い方: python3 luna_signals.py <候補.txt> <作業フォルダ> <出力.txt> --field 法律 [--jobs 4] [--model gpt-6.1-sol]
"""
import argparse, concurrent.futures, hashlib, json, os, random, subprocess

# codex の置き場 (PATH に無いときだけ、環境変数 CODEX_BIN_DIR で渡す)
CODEX_PATH = os.environ.get("CODEX_BIN_DIR", "")
SCHEMA = {"type": "object", "additionalProperties": False, "required": ["items"],
          "properties": {"items": {"type": "array", "items": {
              "type": "object", "additionalProperties": False, "required": ["id", "specific"],
              "properties": {"id": {"type": "string"}, "specific": {"type": "boolean"}}}}}}


def rules(field):
    return f"""各要素の word が、「{field}」の分野の文章に特有の語かを判定せよ。
specific=true: {field}の文章ではよく使い、それ以外の一般の文章（ニュースの一般記事・ブログ・小説・ほかの科目の教科書）にはほとんど出ない語。
specific=false: 一般の文章にもふつうに出る語（例: 場合・必要・主張・判断・結果・規定・期間）、意味の広い語、固有名詞、語の切れ端。
迷ったら false にする（false の語を目印にしても害は無いが、true の語が一般の文に出ると誤った変換を招く）。全件を返す。id はそのまま返す。"""


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
    ap.add_argument("words")
    ap.add_argument("workdir")
    ap.add_argument("out")
    ap.add_argument("--field", required=True)
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--batch", type=int, default=200)
    ap.add_argument("--model", default="gpt-6.1-sol")
    a = ap.parse_args()
    words = [w.strip() for w in open(a.words, encoding="utf-8") if w.strip() and not w.startswith("#")]
    wd = os.path.abspath(a.workdir)
    digest = hashlib.sha256(("\n".join(words) + a.field).encode()).hexdigest()
    stamp = os.path.join(wd, "input.sha256")
    if os.path.exists(stamp) and open(stamp).read() != digest:
        os.rename(wd, wd + ".old_" + open(stamp).read()[:8])
    empty = os.path.join(wd, "empty")
    os.makedirs(empty, exist_ok=True)
    open(stamp, "w").write(digest)
    schema = os.path.join(wd, "schema.json")
    json.dump(SCHEMA, open(schema, "w"))
    order = list(range(len(words)))
    random.Random(5).shuffle(order)
    jobs = [(os.path.join(wd, f"{k // a.batch:05d}.json"), [{"id": str(i), "word": words[i]} for i in order[k:k + a.batch]])
            for k in range(0, len(order), a.batch)]
    with concurrent.futures.ThreadPoolExecutor(a.jobs) as pool:
        results = list(pool.map(lambda j: run(j[0], j[1], schema, empty, a.model, a.field), jobs))
    print({r: results.count(r) for r in set(results)})
    specific = set()
    for path, _ in jobs:
        if os.path.exists(path):
            specific |= {words[int(g["id"])] for g in json.load(open(path, encoding="utf-8")) if g["specific"]}
    with open(a.out, "w", encoding="utf-8") as out:
        out.write(f"# {a.field}の目印の語 (luna_signals.py、{a.model})\n")
        for w in words:
            if w in specific:
                out.write(w + "\n")
    print("signals", len(specific), "of", len(words))


if __name__ == "__main__":
    main()
