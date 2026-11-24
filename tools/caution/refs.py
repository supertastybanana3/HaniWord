"""2026-11-06 (仕事 40): 集めた語の記録 (collect/*.jsonl) から、出典の URL ごとに番号を振り、参考文献の一覧を SIST 02 の Web の形で書く。
形: 著者・組織名. "記事・ページの題". サイト名. 公開日または更新日. URL, (参照 年-月-日).  (著者・日付が無ければその所を省く)
Wayback の写し (wayback.py の出力 TSV) があれば、各文献の次の行に「Internet Archive の写し: <URL> (<写しの日時>)」を、無ければ「写しなし (理由)」を添える。
使い方: python3 refs.py <out_refs.txt> <out_words.tsv> [--wayback wayback.tsv] collect/a1_wikipedia.jsonl collect/a2_media.jsonl ...
  out_words.tsv: 語<TAB>読み<TAB>段<TAB>文献の番号 (, 区切り)  (語ごとに、どの文献に拠るかを番号で引ける)
"""
import argparse, json, re
from collections import OrderedDict


def sist(ref):
    parts = []
    who = ref.get("author") or ref.get("site_org") or ""
    if who:
        parts.append(f"{who}.")
    parts.append(f"“{ref['page_title']}”.")
    if ref.get("site") and ref.get("site") != who:
        parts.append(f"{ref['site']}.")
    if ref.get("published"):
        parts.append(f"{ref['published']}.")
    parts.append(f"{ref['url']}, (参照 {ref['accessed']}).")
    return " ".join(parts)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("out_refs"); p.add_argument("out_words")
    p.add_argument("--wayback")
    p.add_argument("inputs", nargs="+")
    a = p.parse_args()
    rows = []
    for path in a.inputs:
        for line in open(path, encoding="utf-8"):
            if line.strip():
                rows.append(json.loads(line))
    refs = OrderedDict()  # url → 書誌
    for r in rows:
        url = r.get("url", "").strip()
        if not url:
            continue
        if url not in refs:
            refs[url] = {k: (r.get(k) or "").strip() for k in ("author", "page_title", "site", "published", "url", "accessed")}
        else:  # 参照した日は遅いほう
            refs[url]["accessed"] = max(refs[url]["accessed"], (r.get("accessed") or "").strip())
    # 番号: サイト名 → 題 の順に並べて振る (並びから出どころの広がりが分かるように)
    ordered = sorted(refs.values(), key=lambda x: (x["site"], x["page_title"], x["url"]))
    number = {ref["url"]: i for i, ref in enumerate(ordered, start=1)}
    wb = {}
    if a.wayback:
        for line in open(a.wayback, encoding="utf-8"):
            if line.startswith("#"):
                continue
            c = line.rstrip("\n").split("\t")
            if len(c) >= 5:
                wb[c[0]] = c[1:5]
    with open(a.out_refs, "w", encoding="utf-8") as f:
        f.write("参考文献 (SIST 02 の Web の形。番号は語の表の「文献」の番号)\n\n")
        for ref in ordered:
            f.write(f"[{number[ref['url']]}] {sist(ref)}\n")
            if "wikipedia.org/w/index.php?oldid=" in ref["url"]:
                f.write("     (ウィキペディアの版の固定リンク。版は残るので写しは作っていない)\n")
            elif ref["url"] in wb:
                snap, ts, state, memo = wb[ref["url"]]
                if snap:
                    f.write(f"     Internet Archive の写し: {snap} (写しの日時 UTC {ts[:4]}-{ts[4:6]}-{ts[6:8]} {ts[8:10]}:{ts[10:12]})\n")
                else:
                    f.write(f"     写しなし ({memo or state})\n")
    words = OrderedDict()
    for r in rows:
        key = (r["word"], r.get("reading", ""))
        w = words.setdefault(key, {"tiers": set(), "refs": set()})
        w["tiers"].add(r.get("tier", "?"))
        if r.get("url") in number:
            w["refs"].add(number[r["url"]])
    with open(a.out_words, "w", encoding="utf-8") as f:
        f.write("# 語\t読み\t段 (出典の書き方から: a 差別・侮辱 / b 言い換え / ? 迷い)\t文献の番号\n")
        for (word, reading), w in sorted(words.items(), key=lambda x: x[0][1]):
            f.write(f"{word}\t{reading}\t{'|'.join(sorted(w['tiers']))}\t{','.join(map(str, sorted(w['refs'])))}\n")
    print(len(ordered), "refs", len(words), "words")


main()
