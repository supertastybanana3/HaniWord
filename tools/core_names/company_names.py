"""2026-11-07 C: 仕事 44。会社の記事の書き出しの表 (extract_names.py の --companies-out) から、全文で数える名前を書き出す
(記事名・書き出しの名前・その名前から 株式会社 などを除いた形。2 字以上)。使い方: python3 -I company_names.py <titled.tsv (extract_names.py の --companies-out)> [会社の記事名の表 (1 列目。あればその記事だけ)]"""
import re, sys
KINDS = r"(株式会社|有限会社|合同会社|合資会社|合名会社)"
names = set()
only = {l.split("\t", 1)[0].strip() for l in open(sys.argv[2], encoding="utf-8") if not l.startswith("#")} if len(sys.argv) > 2 else None
for l in open(sys.argv[1], encoding="utf-8"):
    if l.startswith("#"): continue
    c = l.rstrip("\n").split("\t")
    if only is not None and c[1] not in only: continue
    for n in (c[1], c[2], re.sub("^" + KINDS + "|" + KINDS + "$", "", c[2]).strip()):
        n = re.sub(r"\s*\([^)]*\)$", "", n).strip()
        if len(n) >= 2: names.add(n)
print("\n".join(sorted(names)))
