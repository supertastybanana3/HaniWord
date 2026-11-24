# 2026-10-18 Claude (2026-10-19: cfg "vocab" and "turkish" for other languages): how often the next English word is among the top 3 suggestions,
# given the previous word and the first 0 / 1 / 2 letters of the word being typed.
# Models: count tables from ngcount (Dirichlet smoothing toward the unigram),
# the LibriSpeech ARPA-derived table (backoff), a no-context unigram baseline, and mixtures.
# Usage: eval_next.py CONFIG.json   (see run_eval.sh)
import json, random, re, sys, time
import numpy as np

cfg = json.load(open(sys.argv[1]))
SRC = cfg["src"]
WORD = re.compile(r"[^\W\d_]+(?:['’][^\W\d_]+)*")
PREFIXES = (0, 1, 2)
TOP = 3
MIN_TARGET_LEN = 4
SAMPLE = cfg.get("sample", 40000)


def load_vocab():
    words = set()
    if "vocab" in cfg:  # 2026-10-19: one word per line (build_table2.py writes OUT.vocab.txt)
        return sorted(l.strip() for l in open(cfg["vocab"], encoding="utf-8") if l.strip())
    for name in ("english_words_scowl.txt", "english_words_context.txt", "english_terms.txt"):
        for line in open(f"{SRC}/{name}", encoding="utf-8"):
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            words.add(line.lstrip("^").lower())
    return sorted(words)


VOCAB = load_vocab()
VID = {w: i for i, w in enumerate(VOCAB)}
VLEN = np.array([len(w) for w in VOCAB])
PRE1 = np.array([w[:1] for w in VOCAB])
PRE2 = np.array([w[:2] for w in VOCAB])


TURKISH = cfg.get("turkish", False)


def lower(text):
    if TURKISH:
        text = text.replace("I", "ı").replace("İ", "i")
    return text.lower()


def runs(text):
    """Word runs separated only by spaces (same rule as ngcount)."""
    for line in text.split("\n"):
        pos, run = 0, []
        for m in WORD.finditer(lower(line).replace("’", "'")):
            gap = line[pos:m.start()]
            if run and gap.strip(" \t"):
                yield run
                run = []
            run.append(m.group(0))
            pos = m.end()
        if run:
            yield run


def test_pairs(path):
    rng = random.Random(20261018)
    pairs = []
    for run in runs(open(path, encoding="utf-8").read()):
        for a, b in zip(run, run[1:]):
            if b in VID and len(b) >= MIN_TARGET_LEN:
                pairs.append((a, b))
    rng.shuffle(pairs)
    return pairs[:SAMPLE]


class Model:
    """succ[w1] = (ids sorted by score desc, scores); fallback multiplier m(w1) times the unigram score."""

    def __init__(self, name):
        self.name = name
        self.succ = {}
        self.mult = {}
        self.uni = np.zeros(len(VOCAB))

    def finish_unigram(self):
        # per prefix: vocab ids with len > prefix length sorted by unigram score (top 64)
        self.by_prefix = {}
        order = np.argsort(-self.uni, kind="stable")
        order = order[self.uni[order] > 0]
        for p in PREFIXES:
            buckets = {}
            for i in order:
                if VLEN[i] <= p:
                    continue
                key = VOCAB[i][:p]
                b = buckets.setdefault(key, [])
                if len(b) < 64:
                    b.append(i)
            self.by_prefix[p] = buckets

    def candidates(self, w1, prefix, n):
        """Top n (id, score) for words starting with prefix and longer than it."""
        p = len(prefix)
        out, seen = [], set()
        if w1 in self.succ:
            ids, scores = self.succ[w1]
            if p == 0:
                mask = VLEN[ids] > 0
            elif p == 1:
                mask = (PRE1[ids] == prefix) & (VLEN[ids] > 1)
            else:
                mask = (PRE2[ids] == prefix) & (VLEN[ids] > 2)
            sel = np.nonzero(mask)[0][:n]
            for j in sel:
                out.append((int(ids[j]), float(scores[j])))
                seen.add(int(ids[j]))
        m = self.mult.get(w1, 1.0)
        for i in self.by_prefix[p].get(prefix, [])[: n + len(seen)]:
            if i not in seen:
                out.append((i, m * self.uni[i]))
        out.sort(key=lambda x: -x[1])
        return out[:n]

    def score(self, w1, i):
        if w1 in self.succ:
            ids, scores = self.lookup[w1]
            j = np.searchsorted(ids, i)
            if j < len(ids) and ids[j] == i:
                return float(scores[j])
        return self.mult.get(w1, 1.0) * self.uni[i]

    def build_lookup(self):
        self.lookup = {}
        for w1, (ids, scores) in self.succ.items():
            order = np.argsort(ids, kind="stable")
            self.lookup[w1] = (ids[order], scores[order])


def count_model(name, prefix_path, needed_w1, k, min_pair):
    t0 = time.time()
    m = Model(name)
    total = 0
    for line in open(prefix_path + ".1gram.tsv", encoding="utf-8"):
        w, c = line.rstrip("\n").split("\t")
        i = VID.get(w)
        if i is not None:
            m.uni[i] += int(c)
            total += int(c)
    m.uni /= total
    raw = {}
    w1count = {}
    for line in open(prefix_path + ".2gram.tsv", encoding="utf-8"):
        a, b, c = line.rstrip("\n").split("\t")
        c = int(c)
        if a not in needed_w1:
            continue
        w1count[a] = w1count.get(a, 0) + c
        i = VID.get(b)
        if i is None or c < min_pair:
            continue
        raw.setdefault(a, ([], []))
        raw[a][0].append(i)
        raw[a][1].append(c)
    for a, (ids, cs) in raw.items():
        ids = np.array(ids)
        n = w1count[a]
        scores = (np.array(cs, dtype=float) + k * m.uni[ids]) / (n + k)
        order = np.argsort(-scores, kind="stable")
        m.succ[a] = (ids[order], scores[order])
        m.mult[a] = k / (n + k)
    for a, n in w1count.items():
        m.mult.setdefault(a, k / (n + k))
    m.finish_unigram()
    m.build_lookup()
    print(f"# loaded {name}: {len(m.succ)} w1, {sum(len(v[0]) for v in m.succ.values())} pairs, {time.time()-t0:.0f}s", flush=True)
    return m


def libri_model(name, d, needed_w1):
    m = Model(name)
    for line in open(f"{d}/ime_1gram.tsv", encoding="utf-8"):
        w, lp, bo = line.rstrip("\n").split("\t")
        i = VID.get(w)
        if i is not None:
            m.uni[i] = 10 ** float(lp)
        if w in needed_w1:
            m.mult[w] = 10 ** float(bo)
    import gzip
    raw = {}
    for line in gzip.open(f"{d}/ime_2gram.tsv.gz", "rt", encoding="utf-8"):
        a, b, lp = line.rstrip("\n").split("\t")
        if a not in needed_w1:
            continue
        i = VID.get(b)
        if i is None:
            continue
        raw.setdefault(a, ([], []))
        raw[a][0].append(i)
        raw[a][1].append(10 ** float(lp))
    for a, (ids, ps) in raw.items():
        ids, ps = np.array(ids), np.array(ps)
        order = np.argsort(-ps, kind="stable")
        m.succ[a] = (ids[order], ps[order])
    m.finish_unigram()
    m.build_lookup()
    print(f"# loaded {name}", flush=True)
    return m


def prob_model(name, prefix, needed_w1):
    """A table from build_table.py: top successors with probabilities, fallback multiplier, unigram."""
    m = Model(name)
    for line in open(prefix + ".uni.tsv", encoding="utf-8"):
        w, p = line.rstrip("\n").split("\t")
        m.uni[VID[w]] = float(p)
    for line in open(prefix + ".mult.tsv", encoding="utf-8"):
        w, x = line.rstrip("\n").split("\t")
        if w in needed_w1:
            m.mult[w] = float(x)
    raw = {}
    for line in open(prefix + ".pairs.tsv", encoding="utf-8"):
        a, b, p = line.rstrip("\n").split("\t")
        if a in needed_w1:
            raw.setdefault(a, ([], []))
            raw[a][0].append(VID[b])
            raw[a][1].append(float(p))
    for a, (ids, ps) in raw.items():
        ids, ps = np.array(ids), np.array(ps)
        order = np.argsort(-ps, kind="stable")
        m.succ[a] = (ids[order], ps[order])
    m.finish_unigram()
    m.build_lookup()
    print(f"# loaded {name}", flush=True)
    return m


def unigram_model(name, base):
    m = Model(name)
    m.uni = base.uni
    m.finish_unigram()
    m.build_lookup()
    return m


class Mix:
    def __init__(self, name, parts):
        self.name, self.parts = name, parts  # [(weight, model)]

    def candidates(self, w1, prefix, n):
        pool = set()
        for _, mdl in self.parts:
            pool.update(i for i, _ in mdl.candidates(w1, prefix, 10))
        scored = [(i, sum(wt * mdl.score(w1, i) for wt, mdl in self.parts)) for i in pool]
        scored.sort(key=lambda x: -x[1])
        return scored[:n]


def evaluate(model, pairs):
    hits = {p: 0 for p in PREFIXES}
    top1 = {p: 0 for p in PREFIXES}
    for a, b in pairs:
        target = VID[b]
        for p in PREFIXES:
            cands = [i for i, _ in model.candidates(a, b[:p], TOP)]
            if target in cands:
                hits[p] += 1
                if cands[0] == target:
                    top1[p] += 1
    n = len(pairs)
    return {p: (100 * hits[p] / n, 100 * top1[p] / n) for p in PREFIXES}


def main():
    tests = {name: test_pairs(path) for name, path in cfg["tests"].items()}
    needed = set(a for ps in tests.values() for a, _ in ps)
    for name, ps in tests.items():
        print(f"# test {name}: {len(ps)} pairs", flush=True)
    models = {}
    for spec in cfg["models"]:
        kind = spec["kind"]
        if kind == "count":
            models[spec["name"]] = count_model(spec["name"], spec["path"], needed, spec.get("k", 20), spec.get("min_pair", 1))
        elif kind == "libri":
            models[spec["name"]] = libri_model(spec["name"], spec["path"], needed)
        elif kind == "prob":
            models[spec["name"]] = prob_model(spec["name"], spec["path"], needed)
        elif kind == "unigram":
            models[spec["name"]] = unigram_model(spec["name"], models[spec["base"]])
        elif kind == "mix":
            models[spec["name"]] = Mix(spec["name"], [(w, models[n]) for w, n in spec["parts"]])
    print("model\ttest\t" + "\t".join(f"top3@{p}\ttop1@{p}" for p in PREFIXES))
    for mname, model in models.items():
        for tname, ps in tests.items():
            r = evaluate(model, ps)
            print(f"{mname}\t{tname}\t" + "\t".join(f"{r[p][0]:.1f}\t{r[p][1]:.1f}" for p in PREFIXES), flush=True)


main()
