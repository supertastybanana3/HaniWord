#!/bin/bash
# 1 言語ぶんの次の語の候補の表 (next_<言語>.hwn) を作る。fetch_fw2.py と Tatoeba で取った元データを WORK/src/<ISO3> に置いてから動かす。
#   WORK/src/<ISO3>/fw2_train.txt   FineWeb-2 の <ISO3>_Latn/train/000_00000.parquet の先頭 (約 2 億語。fetch_fw2.py で取る)
#   WORK/src/<ISO3>/fw2_test.txt    同じく test (当たり率を測るときだけ)
#   WORK/src/<ISO3>/tatoeba.tsv.bz2 https://downloads.tatoeba.org/exports/per_language/<ISO3>/<ISO3>_sentences.tsv.bz2
# 手順: 隣り合う 2 語を数える (ngcount2) → Tatoeba を学習用と試験用に分ける → FineWeb-2 0.7・Tatoeba 0.3 で混ぜる (build_table2.py)
#       → 前の語を見ない順位と上位 3 つが変わる組だけ残す (prune_smart.py) → .hwn に書く (hwn.py) → 当たり率を測る (eval_next.py)
# 使い方: tools/next_words/make_table.sh WORK ISO3   (例: make_table.sh ~/next_words_work fra)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
WORK=$1; L=$2
case $L in fra) V=fr C=fr;; deu) V=de C=de;; spa) V=es C=es;; ita) V=it C=it;; por) V=pt C=pt;; pol) V=pl C=pl;; ces) V=cs C=cs;;
  tur) V=tr C=tr;; dan) V=nordic C=da;; swe) V=nordic C=sv;; nob) V=nordic C=nb;; isl) V=nordic C=is;; fin) V=nordic C=fi;;
  *) echo "unknown language $L" >&2; exit 1;; esac
T=""; [ "$L" = tur ] && T=tr
cd "$WORK"
D=work/$L; mkdir -p $D hwn
NG=$HERE/ngcount2/target/release/ngcount2
[ -x "$NG" ] || cargo build --release --manifest-path "$HERE/ngcount2/Cargo.toml"
$NG $D/fw2 2 $T < src/$L/fw2_train.txt 2> $D/fw2.log
python3 -I "$HERE/tatoeba_split2.py" src/$L/tatoeba.tsv.bz2 $D/fw2 $D $T > $D/tatoeba_split.log
$NG $D/tat 1 $T < $D/tatoeba_train.txt 2> $D/tat.log
python3 -I "$HERE/build_table2.py" $D/mix 0 20 "$REPO/latin_words/$V.txt" 2000 fw2:0.7:$D/fw2 tat:0.3:$D/tat 2> $D/build_mix.log
python3 -I "$HERE/prune_smart.py" $D/mix $D/mix_s3 > $D/prune.log
python3 -I "$HERE/hwn.py" write $D/mix_s3 $D/mix.case.tsv $C hwn/next_$C.hwn $T
TURK=false; [ "$L" = tur ] && TURK=true
cat > $D/cfg.json <<JSON
{"src": "$REPO", "vocab": "$D/mix.vocab.txt", "turkish": $TURK, "sample": 10000,
 "tests": {"fw2_test": "src/$L/fw2_test.txt", "tatoeba_clean": "$D/test_tatoeba_clean.txt"},
 "models": [{"name": "mix_s3", "kind": "prob", "path": "$D/mix_s3"}, {"name": "unigram", "kind": "unigram", "base": "mix_s3"}]}
JSON
if [ -s src/$L/fw2_test.txt ]; then python3 -I "$HERE/eval_next.py" $D/cfg.json > $D/eval.tsv 2> $D/eval.log; fi
echo "hwn/next_$C.hwn"
