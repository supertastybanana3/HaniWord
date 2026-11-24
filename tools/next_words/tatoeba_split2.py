# 2026-10-19 Claude: tatoeba_split.py (2026-10-18, English) for any language.
# Tatoeba sentences -> train (95%) / test (id % 20 == 0). Names that Tatoeba over-uses (Tom, Mary, ...) become a
# break ("|"): a word is a name when it is written with a capital in at least 90% of its non-initial uses and is
# at least 20 times more frequent (per word) than in the FineWeb-2 counts. The test set drops sentences that are in
# train, or one word away from a train sentence (35% of the English test was), so the score is not inflated.
# Usage: tatoeba_split2.py SENTENCES.tsv.bz2 FW2_PREFIX OUT_DIR [tr]
import bz2, collections, re, sys

src, fw_prefix, out_dir = sys.argv[1], sys.argv[2], sys.argv[3]
turkish = len(sys.argv) > 4 and sys.argv[4] == "tr"
WORD = re.compile(r"[^\W\d_]+(?:['’][^\W\d_]+)*")


def lower(w):
    if turkish:
        w = w.replace("I", "ı").replace("İ", "i")
    return w.lower().replace("’", "'")


rows = []
with bz2.open(src, "rt", encoding="utf-8") as f:
    for line in f:
        parts = line.rstrip("\n").split("\t", 2)
        if len(parts) == 3:
            rows.append((int(parts[0]), parts[2]))

count, inner, capital = collections.Counter(), collections.Counter(), collections.Counter()
for _, text in rows:
    for i, m in enumerate(WORD.finditer(text)):
        w = lower(m.group(0))
        count[w] += 1
        if i > 0:
            inner[w] += 1
            if m.group(0)[0].isupper():
                capital[w] += 1
fw = {}
for line in open(fw_prefix + ".1gram.tsv", encoding="utf-8"):
    w, c = line.rstrip("\n").split("\t")
    fw[w] = int(c)
tat_total, fw_total = sum(count.values()), sum(fw.values())
names = sorted(w for w, c in count.items()
               if c >= 100 and inner[w] >= 20 and capital[w] >= 0.9 * inner[w]
               and (c / tat_total) >= 20 * ((fw.get(w, 0) + 1) / fw_total))
open(f"{out_dir}/tatoeba_names.txt", "w", encoding="utf-8").write("\n".join(names) + "\n")
name_set = set(names)


def strip_names(text):
    return WORD.sub(lambda m: "|" if lower(m.group(0)) in name_set else m.group(0), text)


def key(text):
    return tuple(lower(m.group(0)) for m in WORD.finditer(text))


train_keys, masked = set(), set()
train, test = [], []
for sid, text in rows:
    text = strip_names(text)
    if sid % 20 == 0:
        test.append(text)
    else:
        train.append(text)
        k = key(text)
        train_keys.add(k)
        for i in range(len(k)):
            masked.add(k[:i] + ("\0",) + k[i + 1:])
clean = []
for text in test:
    k = key(text)
    if k in train_keys or any(k[:i] + ("\0",) + k[i + 1:] in masked for i in range(len(k))):
        continue
    clean.append(text)
open(f"{out_dir}/tatoeba_train.txt", "w", encoding="utf-8").write("\n".join(train) + "\n")
open(f"{out_dir}/test_tatoeba_clean.txt", "w", encoding="utf-8").write("\n".join(clean) + "\n")
print(f"{src}: {len(rows)} sentences, train {len(train)}, test {len(test)} -> clean {len(clean)}, names {len(names)}: {' '.join(names[:30])}")
