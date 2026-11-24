"""2026-11-06 (仕事 39): 方言・役割語の試しの文 (kansai_src.txt など) から、変換器に渡す組 (読み<TAB>よい形) を作り、結果を数える。
使い方:
  python3 cases.py make <src.txt> <out.tsv>
  python3 cases.py score <src.txt> <result.jsonl> [<result2.jsonl> ...]
数えるもの: 全体が「よい形」か「ほかによい形」と同じ (完全)、「かならず残る形」が全部、結果の中にそのまま入っている (語尾)。
AI (full) と辞書だけ (dict) を別々に
"""
import json, re, sys
from pathlib import Path


def load(src):
    out = []
    for line in Path(src).read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        cols = [c.strip() for c in re.split(r"\s*/\s*", line.strip())]
        cols += [""] * (4 - len(cols))
        reading, good, keys, alts = cols[:4]
        keys = [k for k in keys.split(",") if k]
        alts = [x for x in alts.split("|") if x]
        if not re.fullmatch(r"[ぁ-ゖー]+", reading):
            sys.exit(f"読みがひらがなだけでない: {line}")
        for k in keys:
            if not any(key_ok(f, [k]) for f in [good] + alts):
                sys.exit(f"かならず残る形がよい形に無い: {k} / {line}")
        out.append((reading, good, keys, alts))
    return out


def key_ok(text, keys):
    # キーの中の | はどちらか (寝や|ねや)
    return all(any(x in text for x in k.split("|")) for k in keys)


def main():
    cmd = sys.argv[1]
    cases = load(sys.argv[2])
    if cmd == "make":
        seen = set()
        with open(sys.argv[3], "w", encoding="utf-8") as f:
            f.write("# 読み\tよい形\n")
            for reading, good, _, _ in cases:
                if reading in seen:
                    sys.exit(f"読みが重なる: {reading}")
                seen.add(reading)
                f.write(f"{reading}\t{good}\n")
        print(len(cases), "cases")
        return
    by = {c[0]: c for c in cases}
    for res in sys.argv[3:]:
        rows = [json.loads(l) for l in Path(res).read_text(encoding="utf-8").splitlines() if l.strip()]
        n = len(rows)
        stat = {"full": [0, 0], "dict": [0, 0]}
        bad = []
        rows = [r for r in rows if r["reading"] in by]  # 試しの文を直す前の結果の、もう無い読みは数えない
        n = len(rows)
        for r in rows:
            reading, good, keys, alts = by[r["reading"]]
            for m in ("full", "dict"):
                t = r.get(m) or ""
                exact = t == good or t in alts
                stat[m][0] += exact
                stat[m][1] += key_ok(t, keys)
            if not key_ok(r.get("full") or "", keys) or not key_ok(r.get("dict") or "", keys):
                bad.append((reading, good, r.get("full"), r.get("dict")))
        print(f"== {res}  {n} 文")
        for m in ("full", "dict"):
            print(f"  {'AI あり' if m == 'full' else '辞書だけ'}: 完全 {stat[m][0]}/{n}  語尾 {stat[m][1]}/{n}")
        for reading, good, full, d in bad:
            print(f"  ✗ {good}  AI={full}  辞書={d}")


main()
