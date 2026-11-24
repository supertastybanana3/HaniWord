#!/bin/bash
# 分野辞書の効果を測る: stage (変換サーバー一式) と 設定 (隔離した APPDATA) を指定し、文の組を変換する (Windows の Python を WSL から呼ぶ)
# 環境変数 HANIWORD_BUILD に、stage と data_ext/domain のある作業場所 (リポジトリの外) を入れておく
# 使い方: run_eval.sh <stage のフォルダ名 (HANIWORD_BUILD の下)> <appdata のフォルダ (HANIWORD_BUILD/data_ext/domain の下)> <cases.tsv> <出力.jsonl> [KEY=VALUE ...]
#         KEY=VALUE は変換サーバーの環境変数 (開発用のビルドの AZK_* だけが読む)
set -e
B="${HANIWORD_BUILD:?set HANIWORD_BUILD to the build workspace that holds the stages and data_ext/domain}"
HERE="$(cd "$(dirname "$0")" && pwd)"
win() { wslpath -w "$1"; }
stage="$B/$1"; appdata="$B/data_ext/domain/$2"; cases="$3"; out="$4"; shift 4
# 2026-11-06: 組の 3 列目 (文脈) は明示したときだけ使う: 組の名前が *.cases.tsv (eval_sets.py が作る文の組。前の文を 3 列目に持つ) か、EVAL_CONTEXT=1。
# EVAL_CONTEXT=0 なら *.cases.tsv でも使わない。語の試しの塊 (読み<TAB>表記<TAB>出どころ) の 3 列目は文脈ではないので使わない
ctx=""; case "$cases" in *.cases.tsv) ctx=" --context";; esac
[ "${EVAL_CONTEXT:-}" = 1 ] && ctx=" --context"; [ "${EVAL_CONTEXT:-}" = 0 ] && ctx=""
extra=""; for kv in "$@"; do extra="$extra '$kv'"; done
[ -f "$stage/azookey-server.dll" ] || { echo "no stage: $stage"; exit 1; }
[ -f "$appdata/HaniWord/settings.json" ] || { echo "no settings: $appdata"; exit 1; }
# 2026-10-22: 変換器 (convert.py) は同時に 2 本まで (CPU は全体で 8 割くらいに。論理プロセッサー 0〜3 は 1 本でふさがるので、増やしても 1 本ずつが遅くなるだけ)。空くまで待つ
slots="$B/data_ext/domain/convert_slots"; mkdir -p "$slots"
while :; do
  for i in 1 2; do exec 9>"$slots/$i.lock"; flock -n 9 && break 2; done
  exec 9>&-; sleep 30
done
powershell.exe -NoProfile -Command "py -3 -I '$(win "$HERE/convert.py")' --stage '$(win "$stage")' --cases '$(win "$cases")' --out '$(win "$out")' 'APPDATA=$(win "$appdata")'$extra --reset$ctx 2>\$null" | grep -E '^(done|[0-9]+/)' | tail -1
