#!/bin/bash
# 1 つの束 (<workdir>/terms.tsv: 表記<TAB>例) に読みを付ける手順を通して流す。途中で止めても続きから (束ごとの結果が残る)
#   a・b (luna、Web なし) → 参考の読み (refs.tsv) と照らし合わせ → c (luna)・d (terra) が Web で確かめる → c と d が食い違えば e (6.1-sol) → merged.tsv
# 使い方: pipeline_readings.sh <workdir> <分野の決まり.txt> [reference_readings.py に渡す引数 (--jawiki … --egov … --wiki …)]
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
W="$(cd "$1" && pwd)"; RULES="$2"; shift 2
R() { python3 "$HERE/luna_readings.py" "$@"; }
run_pass() { R run --terms "$1" --workdir "$W" --pass "$2" --rules "$RULES" --jobs "${JOBS:-3}" > "$W/run_$2.log" 2>&1; tail -1 "$W/run_$2.log"; }
run_pass "$W/terms.tsv" a & run_pass "$W/terms.tsv" b & wait
python3 "$HERE/reference_readings.py" --terms "$W/terms.tsv" --out "$W/refs.tsv" "$@"
R todo --workdir "$W" --refs "$W/refs.tsv"
# 3・4 回目には、出てきた例と 1・2 回目の結果を渡す
python3 - "$W" <<'EOF'
import sys
w = sys.argv[1]
ex = dict(l.rstrip("\n").split("\t", 1) for l in open(f"{w}/terms.tsv", encoding="utf-8") if "\t" in l)
with open(f"{w}/todo_c_in.tsv", "w", encoding="utf-8") as out:
    for l in open(f"{w}/todo_c.tsv", encoding="utf-8"):
        t, n = l.rstrip("\n").split("\t", 1)
        out.write(f"{t}\t出てきたところ: {ex.get(t, '')} / 1・2 回目の結果: {n}\n")
EOF
if [ -s "$W/todo_c_in.tsv" ]; then
  run_pass "$W/todo_c_in.tsv" c & run_pass "$W/todo_c_in.tsv" d & wait
  R todo_e --workdir "$W"
  [ -s "$W/todo_e.tsv" ] && run_pass "$W/todo_e.tsv" e
fi
R merge --workdir "$W" --out "$W/merged.tsv"
awk -F'\t' 'NR>1{print $6, $5}' "$W/merged.tsv" | sort | uniq -c
