# 2026-10-19 Claude: build_table.py (2026-10-18, English) for any language.
# Mix count tables into one next-word probability table:
#   P(w|w1) = sum_m weight_m * (c_m(w1,w) + k*Pu_m(w)) / (n_m(w1) + k)   (Dirichlet toward each unigram)
# Candidates: the words of the given word lists, plus words with an apostrophe (c'est, l'homme) seen at least
# APOS_MIN times in the first table. Writes OUT.uni.tsv, OUT.pairs.tsv, OUT.mult.tsv (read by eval_next.py kind "prob"),
# OUT.vocab.txt (the candidate list) and OUT.case.tsv (lowercase word -> usual written form, when it has capitals).
# Usage: build_table2.py OUT N K VOCAB_FILE[,VOCAB_FILE...] APOS_MIN name1:weight1:prefix1 name2:weight2:prefix2 ...
import sys, collections, heapq

out, top_n, k = sys.argv[1], int(sys.argv[2]), float(sys.argv[3])
vocab_files, apos_min = sys.argv[4].split(","), int(sys.argv[5])
parts = [p.split(":") for p in sys.argv[6:]]

vocab = set()
for name in vocab_files:
    for line in open(name, encoding="utf-8"):
        line = line.strip()
        if line and not line.startswith("#"):
            vocab.add(line.lstrip("^").lower())
if apos_min > 0:
    for line in open(parts[0][2] + ".1gram.tsv", encoding="utf-8"):
        w, c = line.rstrip("\n").split("\t")
        if "'" in w and int(c) >= apos_min:
            vocab.add(w)

unis, pairs, totals = [], [], []
for name, weight, prefix in parts:
    uni = {}
    for line in open(prefix + ".1gram.tsv", encoding="utf-8"):
        w, c = line.rstrip("\n").split("\t")
        if w in vocab:
            uni[w] = int(c)
    s = sum(uni.values())
    unis.append({w: c / s for w, c in uni.items()})
    succ = collections.defaultdict(dict)
    n = collections.Counter()
    for line in open(prefix + ".2gram.tsv", encoding="utf-8"):
        a, b, c = line.rstrip("\n").split("\t")
        if a not in vocab:
            continue
        c = int(c)
        n[a] += c
        if b in vocab:
            succ[a][b] = c
    pairs.append(succ)
    totals.append(n)
    print(f"# {name}: {len(uni)} words, {sum(len(v) for v in succ.values())} pairs", file=sys.stderr, flush=True)

weights = [float(w) for _, w, _ in parts]
words = set().union(*[u.keys() for u in unis])
puni = {w: sum(wt * u.get(w, 0.0) for wt, u in zip(weights, unis)) for w in words}
with open(out + ".uni.tsv", "w", encoding="utf-8") as f:
    for w, p in sorted(puni.items(), key=lambda x: -x[1]):
        f.write(f"{w}\t{p:.6g}\n")
with open(out + ".vocab.txt", "w", encoding="utf-8") as f:
    for w in sorted(vocab):
        f.write(w + "\n")

# usual written form: the capitalized form when it is at least half of the occurrences that are not run-initial
inner, forms = collections.Counter(), collections.defaultdict(collections.Counter)
for _, _, prefix in parts:
    try:
        for line in open(prefix + ".case.tsv", encoding="utf-8"):
            w, n_inner, form, c = line.rstrip("\n").split("\t")
            forms[w][form] += int(c)
    except FileNotFoundError:
        pass
for _, _, prefix in parts:
    seen = set()
    try:
        for line in open(prefix + ".case.tsv", encoding="utf-8"):
            w, n_inner, _, _ = line.rstrip("\n").split("\t")
            if w not in seen:
                inner[w] += int(n_inner)
                seen.add(w)
    except FileNotFoundError:
        pass
cased = 0
with open(out + ".case.tsv", "w", encoding="utf-8") as f:
    for w in sorted(forms):
        if w not in puni or inner[w] < 5:
            continue
        form, c = forms[w].most_common(1)[0]
        if c * 2 >= inner[w]:
            f.write(f"{w}\t{form}\n")
            cased += 1

w1s = set().union(*[s.keys() for s in pairs])
kept = 0
with open(out + ".pairs.tsv", "w", encoding="utf-8") as fp, open(out + ".mult.tsv", "w", encoding="utf-8") as fm:
    for a in sorted(w1s):
        denom = [totals[m][a] + k for m in range(len(parts))]
        # fallback: the score of a word no table has seen after a, relative to the mixed unigram
        mult = sum(wt * k / d for wt, d in zip(weights, denom))
        cand = set().union(*[s.get(a, {}).keys() for s in pairs])
        scored = []
        for b in cand:
            p = sum(wt * (pairs[m].get(a, {}).get(b, 0) + k * unis[m].get(b, 0.0)) / denom[m]
                    for m, wt in enumerate(weights))
            scored.append((p, b))
        best = heapq.nlargest(top_n, scored) if top_n > 0 else sorted(scored, reverse=True)
        for p, b in best:
            fp.write(f"{a}\t{b}\t{p:.6g}\n")
        kept += len(best)
        fm.write(f"{a}\t{mult:.6g}\n")
print(f"# {out}: {len(vocab)} vocab, {len(w1s)} first words, {kept} pairs kept, {cased} cased", file=sys.stderr)
