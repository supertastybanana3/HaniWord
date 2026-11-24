"""2026-11-05 A (仕事 22): make_sets.py に渡す 型<TAB>ID<TAB>文 を、Wikipedia の文 (ID<TAB>文) から型ごとに選ぶ。1 文は 1 つの型だけ (まれな型を先に見る)。
型ごと・dev と test ごとに一様に 2,000 文まで (make_sets.py がそこから 300 文を選ぶ)
  python3 -I classify_sets.py <follow の表 (follow_100_0.9.txt)> <出力> <文 (ID<TAB>文) ...>"""
import collections, random, re, sys

table_path, out, *paths = sys.argv[1:]
words = [l.split("\t")[0] for l in open(table_path, encoding="utf-8") if not l.startswith("#") and not l.startswith("-")]
J = "ぁ-んァ-ヺー一-鿿々"
EXT = "pdf|docx|xlsx|pptx|png|jpg|jpeg|gif|txt|csv|zip|exe|mp3|mp4|md"
W = "|".join(sorted(map(re.escape, words), key=len, reverse=True))
KINDS = [
    ("dot_ext", re.compile(rf"[ぁ-ゖァ-ヺ一-鿿][.。]({EXT})(?![A-Za-z])")),
    ("dd_dot_table", re.compile(rf"(?<![0-9])[0-9]{{1,4}}・[0-9]{{1,4}}(?:{W})")),
    ("jj_slash", re.compile(rf"[{J}]/[{J}]")),
    ("ll_slash", re.compile(r"(?<![A-Za-z])[A-Za-z]{2,}/[A-Za-z]")),
    ("ll_dot", re.compile(r"(?<![A-Za-z])[A-Za-z]+・[A-Za-zァ-ヺ]")),
    ("dd_dot_other", re.compile(rf"(?<![0-9])[0-9]{{1,4}}・[0-9]{{1,4}}(?![0-9])(?!{W})")),
    ("dd_slash", re.compile(r"(?<![0-9/.])[0-9]{1,4}/[0-9]{1,4}(?![0-9/.])")),
    ("jj_dot", re.compile(rf"[{J}]・[{J}]")),
]
LIMIT = 2000
random.seed(20261105)
seen = collections.Counter()
keep = collections.defaultdict(list)
for path in paths:
    for line in open(path, encoding="utf-8", errors="replace"):
        aid, _, s = line.rstrip("\n").partition("\t")
        if not aid.isdigit(): continue
        split = "test" if int(aid) % 10 == 0 else "dev"
        for kind, pat in KINDS:
            if pat.search(s):
                # 型ごとに一様に選ぶ (reservoir)
                key = (kind, split)
                seen[key] += 1
                if len(keep[key]) < LIMIT: keep[key].append((aid, s))
                else:
                    j = random.randrange(seen[key])
                    if j < LIMIT: keep[key][j] = (aid, s)
                break
with open(out, "w", encoding="utf-8", newline="\n") as f:
    for (kind, split), rows in sorted(keep.items()):
        for aid, s in rows: f.write(f"{kind}\t{aid}\t{s}\n")
for k in sorted(keep):
    print(k, len(keep[k]), "/", seen[k])
