"""分野辞書の「ほかの読み」(主な読みと別の読みの行) が、その語の読みとして本当に通用するかを 6.1-sol に判定させる。
読み付けの Web で確かめる回 (luna) が、別の語の読み (プレヤデス星団 スバル)・字の読みとして成り立たない誤読 (有孔虫 ユウアナチュウ) を
ほかの読みとして付けることがあった。通用しないと判定された読みは <名前>_bad_readings.txt に足して外す (make_domain_tsv.py build が読む)。
打ち方の揺れ (春日造 カスガヅクリ/カスガズクリ、鋼管杭 コウカンクイ/コウカングイ) は通用する側。迷ったら通用する側。
入力: 分野辞書の TSV (domain_dictionaries/<名前>.tsv) と、主な読みを決めた読みの表 (束の merged.tsv)。
出力 (TSV): 表記 / 主な読み / ほかの読み / 通用するか (1/0)
使い方: python3 luna_alt_check.py <名前>.tsv <作業フォルダ> <出力.tsv> --merged <束の merged.tsv>... [--jobs 3] [--bad-readings <名前>_bad_readings.txt]
      --search --rows <表記<TAB>主な読み<TAB>ほかの読み の表>: Web 検索つきで確かめ直す (2026-10-22、Web の回だけが挙げた読み
      web_only_alt_readings.tsv)。通用するとするのは、その読みが載ったページの URL があるか、主な読みの打ち方の揺れのときだけ。出力に URL の列を足す
      (そのあと make_domain_tsv.py drop-bad で、組み上がった <名前>.tsv からその読みの行を除く)
"""
import argparse, concurrent.futures, hashlib, json, os, random, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from luna_same_word import CODEX_PATH
from make_domain_tsv import merged_rows
import subprocess

SCHEMA = {"type": "object", "additionalProperties": False, "required": ["items"],
          "properties": {"items": {"type": "array", "items": {
              "type": "object", "additionalProperties": False, "required": ["id", "ok"],
              "properties": {"id": {"type": "string"}, "ok": {"type": "boolean"}}}}}}

SEARCH_SCHEMA = {"type": "object", "additionalProperties": False, "required": ["items"],
                 "properties": {"items": {"type": "array", "items": {
                     "type": "object", "additionalProperties": False, "required": ["id", "ok", "source"],
                     "properties": {"id": {"type": "string"}, "ok": {"type": "boolean"}, "source": {"type": "string"}}}}}}

SEARCH_RULES = """この回は Web で確かめる回である。各要素について Web 検索をし、term を alt の読みで読むことが載っているページ
(日本語版ウィキペディアの記事の冒頭の読み・自治体や官公庁のページ・辞書のサイト・教科書や解説) を探して、その URL を source に書く。
ok=true にするのは、(1) そのようなページが見つかったとき、または (2) alt が reading のかなの書き方・発音の揺れ (ヅ/ズ・ヂ/ジ、長音の有無、連濁の有無、促音) だけの違いのとき
(このときは source は空でよい)。どちらでもなければ ok=false (source は空、または別の読みが載っていたページの URL)。"""

RULES = """各要素は、分野辞書の語 term と、その主な読み reading と、ほかの読みの候補 alt (どれもカタカナ) である。
alt が term の読みとして実際に通用するなら ok=true。
通用する読み: 辞書・教科書・その分野の文章で使われる読み、慣用の読み・別の読み方 (法堂 ハットウ/ホウドウ、雲母 ウンモ/キララ、二上山 ニジョウザン/フタガミヤマ)、
かなの書き方や発音の揺れで、その読みで打つ人がいるもの (ヅ/ズ・ヂ/ジ、長音の有無、連濁の有無、促音: 春日造 カスガヅクリ/カスガズクリ、鋼管杭 コウカンクイ/コウカングイ、
10進数 ジッシンスウ/ジュッシンスウ)。中国・朝鮮の地名・人名の日本語の漢字音の読みと現地の音に近い読みは、どちらも通用する。
通用しない読み: 別の語の読み (プレヤデス星団 スバル、処女宮 オトメザ、矢座 サジッタ)、語の一部が抜けた読み・余計な音が付いた読み (第3四分位数 ダイサンシブンスウ)、
字の読みとして成り立たない誤読 (有孔虫 ユウアナチュウ、温位 ユタカイ、尾括式 ビカッコウシキ)。
迷ったら ok=true。全件を返す。id はそのまま返す。"""


def run(path, items, schema, empty, model, search=False):
    if os.path.exists(path):
        return "skip"
    prompt = RULES + "\n\n" + (SEARCH_RULES if search else "") + "\n\n入力:\n" + json.dumps(items, ensure_ascii=False) \
        + "\n\n出力は {\"items\": [...]} の JSON だけ。" + ("" if search else "ツールやファイルは使わない。")
    env = dict(os.environ, PATH=CODEX_PATH + ":" + os.environ["PATH"])
    for _ in range(3):
        tmp = path + ".tmp"
        try:
            subprocess.run(["codex"] + (["--search"] if search else []) + ["exec", "--skip-git-repo-check", "--ephemeral", "-m", model,
                            "-c", f"model_reasoning_effort={'medium' if search else 'low'}",
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
    ap.add_argument("--merged", nargs="+", default=[])
    ap.add_argument("--search", action="store_true", help="Web 検索つきで確かめる (束は 10 語)")
    ap.add_argument("--rows", help="確かめる行の表 (表記<TAB>主な読み<TAB>ほかの読み。# で始まる行は説明)。省けば <名前>.tsv と --merged から作る")
    ap.add_argument("--jobs", type=int, default=3)
    ap.add_argument("--batch", type=int, default=None)
    ap.add_argument("--model", default="gpt-6.1-sol")
    ap.add_argument("--bad-readings", help="通用しないと判定された読みを足す表 (domain_dictionaries/<名前>_bad_readings.txt)")
    a = ap.parse_args()
    main_reading = {}
    for path in a.merged:
        for term, readings, _ in merged_rows(path):
            main_reading.setdefault(term, readings[0])
    rows = set()
    if a.rows:
        rows = {tuple(l.rstrip("\n").split("\t")[:3]) for l in open(a.rows, encoding="utf-8") if l.strip() and not l.startswith("#")}
    else:
        for line in open(a.tsv, encoding="utf-8"):
            if line.startswith("#") or not line.strip():
                continue
            c = line.rstrip("\n").split("\t")
            if c[0] in main_reading and c[1] != main_reading[c[0]]:
                rows.add((c[0], main_reading[c[0]], c[1]))
    rows = sorted(rows)
    batch = a.batch or (10 if a.search else 100)
    wd = os.path.abspath(a.workdir)
    # 束の結果は束の番号で残すので、入力が変わったら前の結果は使えない (別のフォルダに移す)
    digest = hashlib.sha256("\n".join("\t".join(r) for r in rows).encode("utf-8")).hexdigest()
    stamp = os.path.join(wd, "input.sha256")
    if os.path.exists(stamp) and open(stamp).read() != digest:
        os.rename(wd, wd + ".old_" + open(stamp).read()[:8])
    empty = os.path.join(wd, "empty")
    os.makedirs(empty, exist_ok=True)
    open(stamp, "w").write(digest)
    schema = os.path.join(wd, "schema.json")
    json.dump(SEARCH_SCHEMA if a.search else SCHEMA, open(schema, "w"))
    order = list(range(len(rows)))
    random.Random(5).shuffle(order)
    jobs = []
    for k in range(0, len(order), batch):
        items = [{"id": str(i), "term": rows[i][0], "reading": rows[i][1], "alt": rows[i][2]} for i in order[k:k + batch]]
        jobs.append((os.path.join(wd, f"{k // batch:05d}.json"), items))
    with concurrent.futures.ThreadPoolExecutor(a.jobs) as pool:
        results = list(pool.map(lambda j: run(j[0], j[1], schema, empty, a.model, a.search), jobs))
    print("alt rows", len(rows), {r: results.count(r) for r in set(results)})
    got = {}
    for path, _ in jobs:
        if os.path.exists(path):
            for g in json.load(open(path, encoding="utf-8")):
                got[int(g["id"])] = g
    with open(a.out, "w", encoding="utf-8") as out:
        out.write("# 表記\t主な読み\tほかの読み\t通用するか" + ("\t確かめた URL" if a.search else "") + "\n")
        for i, r in enumerate(rows):
            if i in got:
                out.write(f"{r[0]}\t{r[1]}\t{r[2]}\t{int(got[i]['ok'])}" + (f"\t{' '.join(got[i].get('source', '').split())}" if a.search else "") + "\n")
    if a.bad_readings:
        # 通用しないと判定された読みを <名前>_bad_readings.txt に足す (前からある行は残す)
        have = set()
        if os.path.exists(a.bad_readings):
            have = {tuple(l.split("\t")[:2]) for l in open(a.bad_readings, encoding="utf-8") if l.strip() and not l.startswith("#")}
        new = [(r[0], r[2], r[1]) for i, r in enumerate(rows) if i in got and not got[i]["ok"] and (r[0], r[2]) not in have]
        with open(a.bad_readings, "a", encoding="utf-8", newline="\n") as f:
            if not have and os.path.getsize(a.bad_readings) == 0:
                f.write("# 分野辞書に入れない読み (表記<TAB>読み<TAB>理由)。その読みの行だけ外し、語のほかの読みは入れる。make_domain_tsv.py build が読む\n")
            how = "Web で確かめて (6.1-sol、luna_alt_check.py --search)" if a.search else "6.1-sol の判定で (luna_alt_check.py)"
            f.writelines(f"{t}\t{alt}\t{how} 読みとして通用しない (主な読み {main})\n" for t, alt, main in new)
        print("bad readings added", len(new))
    if "fail" in results:
        sys.exit(1)


if __name__ == "__main__":
    main()
