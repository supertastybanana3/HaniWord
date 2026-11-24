"""2026-11-07 C: 仕事 43・44。月ごとの上位の記事 (top_monthly.jsonl) を記事ごとにまとめ、道具的な記事を外す。
外す: 名前空間のある題 (特別:・Wikipedia:・ファイル:・Category: など)・メインページ・- (不明)・一覧の記事 (〜一覧・〜の一覧)・
年や日付の記事 (2023年・1月1日・2023年の日本・令和5年 など)・曖昧さ回避の注 (曖昧さ回避) の題
出力 TSV: 題 (_ を空白に)<TAB>上位に入った月の数<TAB>上位に入った最初の月<TAB>最後の月<TAB>いちばん多かった月<TAB>その月の閲覧<TAB>直近 12 か月 (2025-11〜2026-10) に上位にいた月の数
使い方: python3 -I filter_top.py <top_monthly.jsonl> <out.tsv> <excluded.tsv>"""
import json, re, sys
from collections import defaultdict
src, out, exc = sys.argv[1:4]
per = defaultdict(list)
for l in open(src, encoding="utf-8"):
    r = json.loads(l)
    for t, v, rank in r["articles"]: per[t.replace("_", " ")].append((r["month"], v))
def why(t):
    if t in ("メインページ", "-") or re.match(r"^[^:：]{1,20}[:：]", t) and re.match(r"^(特別|Wikipedia|ファイル|Category|ヘルプ|Portal|プロジェクト|ノート|利用者|Template|テンプレート|MediaWiki|Help|File|Special|User|Draft)[:：]", t): return "道具の記事"
    if re.search(r"一覧", t): return "一覧の記事"
    if re.fullmatch(r"(\d{1,4}年|\d{1,2}月\d{1,2}日|\d{1,2}月|(明治|大正|昭和|平成|令和)\d{1,2}年|\d{4}年の.+|\d{4}年代|\d{1,2}世紀|紀元前\d+年)", t): return "年や日付の記事"
    if "曖昧さ回避" in t: return "曖昧さ回避"
    return ""
kept = excl = 0
with open(out, "w", encoding="utf-8") as fo, open(exc, "w", encoding="utf-8") as fe:
    fo.write("# 題\t上位に入った月の数\t最初の月\t最後の月\tいちばん多かった月\tその月の閲覧\t直近 12 か月に上位にいた月の数\n")
    fe.write("# 題\t外した理由\n")
    for t, ms in sorted(per.items(), key=lambda x: -len(x[1])):
        w = why(t)
        if w: fe.write(f"{t}\t{w}\n"); excl += 1; continue
        ms.sort(); peak = max(ms, key=lambda x: x[1])
        recent = sum(1 for m, _ in ms if "2025-11" <= m <= "2026-10")
        fo.write("\t".join(map(str, [t, len(ms), ms[0][0], ms[-1][0], peak[0], peak[1], recent])) + "\n"); kept += 1
print("残す", kept, "外す", excl)
