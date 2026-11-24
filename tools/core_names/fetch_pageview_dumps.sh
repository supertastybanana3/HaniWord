#!/bin/bash
# 2026-11-07 C: 仕事 44 の物差しの直し (まとめ役 12:24)。Wikimedia の月ごとの閲覧数のダンプ (pageview_complete、人の閲覧 user、CC0) を
# 1 か月ずつ取りながら展開し、日本語版ウィキペディア (ja.wikipedia) の行だけを記事ごとに足し合わせる (取ったファイルは残さない)。
# 決まり (2026-11-07 に開いて確かめた): dumps.wikimedia.org は User-Agent の決まりを守らせ、速さを絞り、1 つの IP から 3 つまでの接続。
# robots.txt は 404。ここでは 1 つずつ取る (連絡先つきの名乗り)。1 か月 5.4〜5.7GB、4MB/秒ほどで 25 分ほど。
# 出力: months/<YYYYMM>.tsv (記事の題<TAB>その月の閲覧)。そろった月は飛ばす。MONTHS で月を指定 (既定 202511〜202610)。DRY=1 なら手元の頭 50MB で
UA="HaniWord-dict-tools/0.1 (https://github.com/supertastybanana3)"
D=${PVC_DIR:?PVC_DIR に出力の置き場}; O=$D/months; [ "$DRY" = 1 ] && O=$D/dry; mkdir -p $O
log() { echo "$(date '+%m-%d %H:%M:%S') $*" >> $O/run.log; }
MONTHS=${MONTHS:-"202511 202512 202601 202602 202603 202604 202605 202606 202607 202608 202609 202610"}
AGG='$1==W { t = (NF >= 6 ? $5 : $4); s[$2] += t } END { for (k in s) print k "\t" s[k] }'
log "始め (DRY ${DRY:-0}・$MONTHS)"
for m in $MONTHS; do
  out=$O/$m.tsv
  [ -s $out ] && { log "済み $m"; continue; }
  [ -n "${ON_AC:-}" ] && until $ON_AC; do sleep 300; done
  y=${m:0:4}; mm=${m:4:2}
  url="https://dumps.wikimedia.org/other/pageview_complete/monthly/$y/$y-$mm/pageviews-$m-user.bz2"
  log "取る $m"
  if [ "$DRY" = 1 ]; then
    bzip2 -dc ${DRY_FILE:?} 2>/dev/null | LC_ALL=C awk -v W="${WIKI:-ja.wikipedia}" "$AGG" > $out.part
  else
    curl -sS -f -A "$UA" "$url" | nice -n 15 bzip2 -dc | LC_ALL=C nice -n 15 awk -v W="${WIKI:-ja.wikipedia}" "$AGG" > $out.part
    [ ${PIPESTATUS[0]} = 0 ] || { log "取れなかった $m (curl ${PIPESTATUS[0]})"; rm -f $out.part; continue; }
  fi
  mv $out.part $out; log "終わり $m $(wc -l < $out) 記事"
done
log "PVC_DONE"; touch $O/run.done
