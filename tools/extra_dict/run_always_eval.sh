#!/bin/bash
# (2026-10-22、M3.5) 試しのステージの追加辞書を入れ替えて、文の組を変換する (tools/domain/convert.py)。変換サーバーは同時に 1 つだけ流す
# 環境変数: HANIWORD_BUILD (作業場所)・STAGE (試しのステージ。domain フォルダに分野辞書を書いておく)・APPDATA_EVAL (learning=false の隔離した設定)
# 使い方: run_always_eval.sh <out.jsonl> <cases.tsv> [名前=TSV ...]   名前を付けなければ追加辞書なし (基準)
set -e
: "${HANIWORD_BUILD:?}" "${STAGE:?}" "${APPDATA_EVAL:?}"
HERE="$(cd "$(dirname "$0")" && pwd)"; REPO="$(cd "$HERE/../.." && pwd)"
out=$1; cases=$2; shift 2
# 2026-11-06: 組の 3 列目 (文脈) は明示したときだけ使う (tools/domain/run_eval.sh と同じ決まり: *.cases.tsv か EVAL_CONTEXT=1、EVAL_CONTEXT=0 なら使わない)
ctx=""; case "$cases" in *.cases.tsv) ctx=" --context";; esac
[ "${EVAL_CONTEXT:-}" = 1 ] && ctx=" --context"; [ "${EVAL_CONTEXT:-}" = 0 ] && ctx=""
python3 "$HERE/build_always.py" "$REPO" "$STAGE/domain" "$STAGE/Dictionary/louds/charID.chid" "$@"
rm -f "$out"
powershell.exe -NoProfile -Command "[System.Diagnostics.Process]::GetCurrentProcess().PriorityClass='Idle'; py -3 -I '$(wslpath -w "$REPO/tools/domain/convert.py")' --stage '$(wslpath -w "$STAGE")' --cases '$(wslpath -w "$cases")' --out '$(wslpath -w "$out")'$ctx 'APPDATA=$(wslpath -w "$APPDATA_EVAL")' --reset 2>\$null" | grep -E '^(done|[0-9]+/)' | tail -1
