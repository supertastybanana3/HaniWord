#!/bin/bash
# 読みを付け終えた束 (pipeline_readings.sh の merged.tsv) を集めて、分野辞書 domain_dictionaries/<名前>.tsv を作り、重みを詰める。
#   1. 入れる語の読みで今の辞書の変換を試す (分野辞書なしの stage)
#   2. 表記がそのまま出なかったものについて、今の変換が自然かを 6.1-sol に判定させる
#   3. make_domain_tsv.py build → 崩れた表記を直す (check_notation.py) → 同じ語の書き方違いを除く (luna_same_word.py、6.1-sol) → 通用しないほかの読みを除く (luna_alt_check.py、6.1-sol)
#      → calibrate.py (分野辞書を置いた stage)
# 環境変数 HANIWORD_BUILD と FIELD (分野の名前。例 生物) が要る。CODEX_JOBS で Codex を並べる数を絞れる (既定 4・6)。作業フォルダに途中の結果を残す (やり直すときは消す)
# 使い方: assemble_domain.sh <名前> <作業フォルダ> <辞書の全件.tsv> <出典.tsv (複数可、: で区切る)> <束のフォルダ>...
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
NAME=$1; W=$2; DICT=$3; SOURCES=$4; shift 4
mkdir -p "$W"
B="${HANIWORD_BUILD:?}"
# 束の merged.tsv をつなぐ (語は束どうしで重ならない)
{ head -1 "$1/merged.tsv"; for d in "$@"; do tail -n +2 "$d/merged.tsv"; done; } > "$W/merged.tsv"
python3 "$HERE/make_domain_tsv.py" prepare --merged "$W/merged.tsv" --out "$W/readings.tsv"
# 前に試した読みは試し直さない
python3 - "$W" <<'EOF'
import json, os, sys
w = sys.argv[1]
done = set()
if os.path.exists(f"{w}/readings_probe.jsonl"):
    for l in open(f"{w}/readings_probe.jsonl", encoding="utf-8"):
        try:
            r = json.loads(l); done.add((r["reading"], r["expected"]))
        except ValueError:
            pass
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
rows = [l for l in open(f"{w}/readings.tsv", encoding="utf-8") if (hira(l.split("\t")[0]), l.rstrip("\n").split("\t")[1]) not in done]
open(f"{w}/readings_todo.tsv", "w", encoding="utf-8").writelines(rows)
print("readings to probe", len(rows))
EOF
if [ -s "$W/readings_todo.tsv" ]; then
  "$HERE/run_eval.sh" "${BASE_STAGE:-stage_domain}" appdata "$W/readings_todo.tsv" "$W/readings_probe_new.jsonl"
  cat "$W/readings_probe_new.jsonl" >> "$W/readings_probe.jsonl"; rm "$W/readings_probe_new.jsonl"
fi
python3 "$HERE/make_domain_tsv.py" natural --probe "$W/readings_probe.jsonl" --out "$W/natural_in.tsv"
python3 "$HERE/luna_natural.py" "$W/natural_in.tsv" "$W/natural" "$W/natural_out.tsv" --jobs "${CODEX_JOBS:-4}" --batch 120
python3 "$HERE/make_domain_tsv.py" build --merged "$W/merged.tsv" --probe "$W/readings_probe.jsonl" --natural "$W/natural_out.tsv" \
  --dict "$DICT" --sources ${SOURCES//:/ } --exclude "$HERE/../../domain_dictionaries/${NAME}_exclude.txt" --out "$HERE/../../domain_dictionaries/$NAME.tsv"
# 漢字の一部が片仮名 1 字になった表記 (Wikidata の名前の 耿湋 → 耿イ) を、出典の Wikipedia の記事名と比べて直す
python3 "$HERE/check_notation.py" --tsv "$HERE/../../domain_dictionaries/$NAME.tsv" --sources ${SOURCES//:/ } \
  --out "$HERE/../../domain_dictionaries/$NAME.tsv" --log "$W/notation_check.tsv"
# 今の変換と同じ語の書き方違い (蛋白質/タンパク質) で、分野でも今の変換の書き方がふつうな語は入れない
python3 "$HERE/luna_same_word.py" "$HERE/../../domain_dictionaries/$NAME.tsv" "$W/same_word" "$W/same_word.tsv" --field "${FIELD:?FIELD に分野の名前 (例 生物) を入れる}" --jobs "${CODEX_JOBS:-6}"
python3 "$HERE/make_domain_tsv.py" drop-same --same "$W/same_word.tsv" --out "$HERE/../../domain_dictionaries/$NAME.tsv"
# ほかの読みが読みとして通用するかを 6.1-sol に判定させ、通用しない読み (別の語の読み・誤読) の行を除く。打ち方の揺れ (ヅ/ズ・連濁) は残す
python3 "$HERE/luna_alt_check.py" "$HERE/../../domain_dictionaries/$NAME.tsv" "$W/alt_check" "$W/alt_check.tsv" --merged "${@/%//merged.tsv}" \
  --jobs "${CODEX_JOBS:-3}" --bad-readings "$HERE/../../domain_dictionaries/${NAME}_bad_readings.txt"
python3 "$HERE/make_domain_tsv.py" drop-bad --exclude "$HERE/../../domain_dictionaries/${NAME}_exclude.txt" --out "$HERE/../../domain_dictionaries/$NAME.tsv"
python3 "$HERE/calibrate.py" "$NAME" --stage "${DOMAIN_STAGE:-stage_domain_new}" --appdata appdata --work "$W/calibrate" | grep -v '^done'
