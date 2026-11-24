"""分野辞書の重みを詰める: 作った辞書を試験用の stage に置き、全部の語の読みを変換し直して、決めたとおりになっているか確かめる。
  - 今の変換が自然な語 (献血・各遺伝子): 分野辞書を入れても今の変換のまま出ること。押しのけていたら重みを --step 下げる (--floor より下は外す)
  - 今の変換が切れ端の語 (胃尿地・即時広告): 分野の語が出ること。出なければ重みを 1 上げる (--ceil まで。届かなければ候補に残るだけ)
    読みが短い語 (--short-len 字以下) は --short-ceil まで (2026-10-22: 読み 3〜4 字の珍しい語 (元叉 ゲンサ・波才 ハサイ) が上限 -5 まで上がり、
    一般の文の中の同じ音の並び (方言差・採算性) を取っていた。-10 で止めると、悪くなった文 35 のうち 17 が戻り、良くなった文 107 のうち 2 を失うだけ)
を --rounds 回まで繰り返し、<名前>.tsv を書き直す (前のものは作業フォルダの <名前>.tsv.before_calibrate に残す)。
環境変数 HANIWORD_BUILD (stage と data_ext/domain のある作業場所) が要る。
使い方: python3 calibrate.py <名前> --stage <stage のフォルダ名> --appdata <appdata のフォルダ名> --work <作業フォルダ>
"""
import argparse, json, os, shutil, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, HERE)
import common


def read_rows(path):
    header, rows = [], []
    for line in open(path, encoding="utf-8"):
        if line.startswith("#"):
            header.append(line)
        elif line.strip():
            rows.append(line.rstrip("\n").split("\t"))
    return header, rows


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("name")
    ap.add_argument("--stage", required=True)
    ap.add_argument("--appdata", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--rounds", type=int, default=4)
    ap.add_argument("--step", type=float, default=2.0)
    ap.add_argument("--floor", type=float, default=-20.0)
    ap.add_argument("--ceil", type=float, default=-5.0)
    ap.add_argument("--short-ceil", type=float, default=-10.0)
    ap.add_argument("--short-len", type=int, default=4)
    a = ap.parse_args()
    build = os.environ["HANIWORD_BUILD"]
    tsv = os.path.join(REPO, "domain_dictionaries", a.name + ".tsv")
    os.makedirs(a.work, exist_ok=True)
    shutil.copyfile(tsv, os.path.join(a.work, a.name + ".tsv.before_calibrate"))
    header, rows = read_rows(tsv)
    for r in rows:  # 読みが短い語は、前の調整や作ったときの値 (切れ端の語は -8) が上限より上でも上限まで下げる
        if len(r[1]) <= a.short_len and float(r[4]) > a.short_ceil:
            r[4] = f"{a.short_ceil:g}"
    domain_dir = os.path.join(build, a.stage, "domain")
    for rnd in range(a.rounds):
        # 辞書を書き出して stage に置く
        with open(tsv, "w", encoding="utf-8") as f:
            f.writelines(header)
            f.writelines("\t".join(r) + "\n" for r in rows)
        subprocess.run([sys.executable, os.path.join(HERE, "build_domain.py"), "--out", domain_dir, a.name], check=True)
        cases = os.path.join(a.work, f"round{rnd}.tsv")
        out = os.path.join(a.work, f"round{rnd}.jsonl")
        with open(cases, "w", encoding="utf-8") as f:
            for r in rows:
                term, reading, current, natural = r[0], r[1], r[6], r[7] == "1"
                f.write(f"{common.to_hiragana(reading)}\t{current if natural else term}\n")
        if os.path.exists(out):
            os.remove(out)
        subprocess.run([os.path.join(HERE, "run_eval.sh"), a.stage, a.appdata, cases, out], check=True)
        results = [json.loads(line) for line in open(out, encoding="utf-8")]
        changed, dropped, stuck = 0, 0, 0
        keep = []
        for r, res in zip(rows, results):
            value, natural = float(r[4]), r[7] == "1"
            if res["ok"]:
                keep.append(r)
                continue
            if natural:
                value -= a.step
                if value < a.floor:
                    dropped += 1
                    continue
                changed += 1
            elif value < (ceil := a.short_ceil if len(r[1]) <= a.short_len else a.ceil):
                value = min(value + 1.0, ceil)
                changed += 1
            else:
                stuck += 1
            r[4] = f"{value:g}"
            keep.append(r)
        ok = sum(x["ok"] for x in results)
        print(f"round {rnd}: ok {ok}/{len(results)} changed {changed} dropped {dropped} stuck {stuck}", flush=True)
        rows = keep
        if changed == 0 and dropped == 0:
            break
    with open(tsv, "w", encoding="utf-8") as f:
        f.writelines(header)
        f.writelines("\t".join(r) + "\n" for r in rows)
    subprocess.run([sys.executable, os.path.join(HERE, "build_domain.py"), "--out", domain_dir, a.name], check=True)
    print("entries", len(rows))


if __name__ == "__main__":
    main()
