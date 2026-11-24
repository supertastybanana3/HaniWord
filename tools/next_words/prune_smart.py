# 2026-10-18 Claude: keep only the pairs that change what is shown.
# For each first word and each typed prefix of 0, 1 or 2 letters, the top 3 (pairs first, then the
# unigram order) is compared with the plain unigram top 3 for that prefix; when they differ, the pairs
# in that top 3 are kept. Input pairs must be sorted by probability within a first word.
# Usage: prune_smart.py IN_PREFIX OUT_PREFIX [TOP]
import sys, shutil

src, dst = sys.argv[1], sys.argv[2]
top = int(sys.argv[3]) if len(sys.argv) > 3 else 3
uni = [(w, float(p)) for w, p in (l.rstrip("\n").split("\t") for l in open(src + ".uni.tsv", encoding="utf-8"))]
uni.sort(key=lambda x: -x[1])

# plain unigram top lists per prefix (words longer than the prefix), with a few spares for filling
spare = top * 4
uni_top = {}
for w, _ in uni:
    for p in (0, 1, 2):
        if len(w) <= p:
            continue
        lst = uni_top.setdefault(w[:p], [])
        if len(lst) < spare:
            lst.append(w)


def shown(pair_words, key):
    out = list(pair_words[:top])
    for w in uni_top.get(key, []):
        if len(out) >= top:
            break
        if w not in out:
            out.append(w)
    return out


kept = total = 0


def flush(a, rows, out):
    global kept
    buckets = {}
    for line, b in rows:
        for p in (0, 1, 2):
            if len(b) > p:
                lst = buckets.setdefault(b[:p], [])
                if len(lst) < top:
                    lst.append(b)
    keep = set()
    for key, words in buckets.items():
        if shown(words, key) != uni_top.get(key, [])[:top]:
            keep.update(words)
    for line, b in rows:
        if b in keep:
            out.write(line)
            kept += 1


with open(dst + ".pairs.tsv", "w", encoding="utf-8") as out:
    cur, rows = None, []
    for line in open(src + ".pairs.tsv", encoding="utf-8"):
        a, b, _ = line.split("\t", 2)
        total += 1
        if a != cur:
            if cur is not None:
                flush(cur, rows, out)
            cur, rows = a, []
        rows.append((line, b))
    if cur is not None:
        flush(cur, rows, out)
shutil.copy(src + ".uni.tsv", dst + ".uni.tsv")
shutil.copy(src + ".mult.tsv", dst + ".mult.tsv")
print(f"{dst}: kept {kept}/{total}")
