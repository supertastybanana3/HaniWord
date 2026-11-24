#!/bin/bash
# 1 つの束に読みを付ける、Codex の枠を節約する流し方 (2026-10-21、Codex の週の枠が残り少ないとき)。途中で止めても続きから
#   a (luna、Web なし) → 参考の読み (ウィキペディア・Mozc の公開辞書・候補の段で持っている人が付けた読み) と合えば決まり
#   → 合わない語・参考の読みが無い語だけ b (luna、Web なし) → 参考の読みとも a とも決まらない語だけ f (luna、Web で確かめる) → merged.tsv
#   terra (d) と 6.1-sol (e) は使わない
# 使い方: CANDIDATES="<候補.tsv> ..." pipeline_readings_lite.sh <workdir> <分野の決まり.txt> [reference_readings.py に渡す引数]
# 環境変数 HANIWORD_BUILD (Mozc の公開辞書 data_ext/mozc_oss のある作業場所。strong_refs.py が読む) が要る
set -e
: "${HANIWORD_BUILD:?set HANIWORD_BUILD to the build workspace that holds data_ext/mozc_oss}"
HERE="$(cd "$(dirname "$0")" && pwd)"
W="$(cd "$1" && pwd)"; RULES="$2"; shift 2
R() { python3 "$HERE/luna_readings.py" "$@"; }
run_pass() { R run --terms "$1" --workdir "$W" --pass "$2" --rules "$RULES" --jobs "${JOBS:-3}" > "$W/run_$2.log" 2>&1; tail -1 "$W/run_$2.log"; }
run_pass "$W/terms.tsv" a
python3 "$HERE/reference_readings.py" --terms "$W/terms.tsv" --out "$W/refs.tsv" "$@"
python3 "$HERE/strong_refs.py" --terms "$W/terms.tsv" --out "$W/refs_strong.tsv" --candidates ${CANDIDATES:-}
REFS="$W/refs.tsv $W/refs_strong.tsv"
R todo_b_lite --workdir "$W" --terms "$W/terms.tsv" --refs $REFS
[ -s "$W/todo_b.tsv" ] && run_pass "$W/todo_b.tsv" b
R todo_c_lite --workdir "$W" --terms "$W/terms.tsv" --refs $REFS
[ -s "$W/todo_f_in.tsv" ] && run_pass "$W/todo_f_in.tsv" f
R merge_lite --workdir "$W" --refs $REFS --out "$W/merged.tsv"
awk -F'\t' 'NR>1{print $6, $5}' "$W/merged.tsv" | sort | uniq -c
