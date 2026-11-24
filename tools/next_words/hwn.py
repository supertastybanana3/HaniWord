# 2026-10-19 Claude: the next-word table file the IME reads (.hwn), its writer, and a reader used to check it.
#
# Layout (little-endian). Words are sorted by their UTF-8 bytes, so the words starting with a typed prefix are
# one id range. Scores are -ln(p) * 1000 as u16 (65535 = never seen).
#   header (64 bytes): b"HWNX", u32 version=1, 8-byte language, u32 words, u32 pairs, u32 flags (1 = Turkish lowercase),
#                      u32 offsets of: word_index, word_blob, display_index, display_blob, uni, mult, pair_start, pair_data
#   word_index   u32 * (words + 1)  byte offsets into word_blob (lowercase words)
#   display_index u32 * words       offset into display_blob, or 0xFFFFFFFF when the word is written in lowercase
#   display_blob  u8 length + UTF-8 (usual written form: Vater, I, English)
#   uni           u16 * words       -ln P(w) * 1000
#   mult          u16 * words       -ln m(w1) * 1000 (score of a word not stored after w1 = uni + mult)
#   pair_start    u32 * (words + 1) byte offsets into pair_data
#   pair_data     per first word, ascending second-word ids: varint(id delta) + u16 -ln P(w|w1) * 1000
# Usage: hwn.py write TABLE_PREFIX CASE_TSV LANG OUT.hwn [tr]
#        hwn.py query OUT.hwn prev:typed ...
#        hwn.py check OUT.hwn TABLE_PREFIX CASE_TSV N   (random queries, compared with the TSV tables)
import math, random, struct, sys

NEVER = 65535


def q(p):
    return NEVER if p <= 0 else min(NEVER - 1, max(0, round(-math.log(p) * 1000)))


def varint(n):
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def load_tsv(prefix, case_path):
    uni = {}
    for line in open(prefix + ".uni.tsv", encoding="utf-8"):
        w, p = line.rstrip("\n").split("\t")
        if float(p) > 0:
            uni[w] = float(p)
    mult = {}
    for line in open(prefix + ".mult.tsv", encoding="utf-8"):
        a, m = line.rstrip("\n").split("\t")
        mult[a] = float(m)
    pairs = {}
    for line in open(prefix + ".pairs.tsv", encoding="utf-8"):
        a, b, p = line.rstrip("\n").split("\t")
        pairs.setdefault(a, []).append((b, float(p)))
    case = {}
    for line in open(case_path, encoding="utf-8"):
        w, form = line.rstrip("\n").split("\t")
        case[w] = form
    return uni, mult, pairs, case


def write(prefix, case_path, lang, out, turkish):
    uni, mult, pairs, case = load_tsv(prefix, case_path)
    words = sorted(uni, key=lambda w: w.encode("utf-8"))
    wid = {w: i for i, w in enumerate(words)}
    n = len(words)

    def align(buf):
        while len(buf) % 4:
            buf.append(0)

    body = bytearray()
    offsets = {}
    offsets["word_index"] = 64 + len(body)
    blob, idx = bytearray(), []
    for w in words:
        idx.append(len(blob))
        blob += w.encode("utf-8")
    idx.append(len(blob))
    body += struct.pack(f"<{n + 1}I", *idx)
    offsets["word_blob"] = 64 + len(body)
    body += blob
    align(body)
    dblob, didx = bytearray(), []
    for w in words:
        form = case.get(w)
        if form and form != w:
            enc = form.encode("utf-8")
            didx.append(len(dblob))
            dblob.append(len(enc))
            dblob += enc
        else:
            didx.append(0xFFFFFFFF)
    offsets["display_index"] = 64 + len(body)
    body += struct.pack(f"<{n}I", *didx)
    offsets["display_blob"] = 64 + len(body)
    body += dblob
    align(body)
    offsets["uni"] = 64 + len(body)
    body += struct.pack(f"<{n}H", *[q(uni[w]) for w in words])
    align(body)
    offsets["mult"] = 64 + len(body)
    body += struct.pack(f"<{n}H", *[q(mult.get(w, 1.0)) for w in words])
    align(body)
    pdata, pstart, total = bytearray(), [], 0
    for w in words:
        pstart.append(len(pdata))
        prev = 0
        for b, p in sorted(((wid[b], p) for b, p in pairs.get(w, []) if b in wid)):
            pdata += varint(b - prev)
            pdata += struct.pack("<H", q(p))
            prev = b
            total += 1
    pstart.append(len(pdata))
    offsets["pair_start"] = 64 + len(body)
    body += struct.pack(f"<{n + 1}I", *pstart)
    offsets["pair_data"] = 64 + len(body)
    body += pdata
    order = ["word_index", "word_blob", "display_index", "display_blob", "uni", "mult", "pair_start", "pair_data"]
    header = b"HWNX" + struct.pack("<I", 1) + lang.encode("ascii").ljust(8, b"\0") + struct.pack(
        "<III", n, total, 1 if turkish else 0) + struct.pack("<8I", *[offsets[k] for k in order])
    header = header.ljust(64, b"\0")
    with open(out, "wb") as f:
        f.write(header + body)
    print(f"{out}: {n} words, {total} pairs, {(64 + len(body)) / 1e6:.1f} MB")


class Table:
    def __init__(self, path):
        self.b = open(path, "rb").read()
        b = self.b
        assert b[:4] == b"HWNX"
        self.n, self.pairs, self.flags = struct.unpack_from("<III", b, 16)
        (self.o_wi, self.o_wb, self.o_di, self.o_db, self.o_uni, self.o_mult, self.o_ps, self.o_pd) = struct.unpack_from("<8I", b, 28)

    def word(self, i):
        s, e = struct.unpack_from("<II", self.b, self.o_wi + 4 * i)
        return self.b[self.o_wb + s:self.o_wb + e].decode("utf-8")

    def wbytes(self, i):
        s, e = struct.unpack_from("<II", self.b, self.o_wi + 4 * i)
        return self.b[self.o_wb + s:self.o_wb + e]

    def display(self, i):
        (d,) = struct.unpack_from("<I", self.b, self.o_di + 4 * i)
        if d == 0xFFFFFFFF:
            return self.word(i)
        ln = self.b[self.o_db + d]
        return self.b[self.o_db + d + 1:self.o_db + d + 1 + ln].decode("utf-8")

    def lower_bound(self, key):
        lo, hi = 0, self.n
        while lo < hi:
            mid = (lo + hi) // 2
            if self.wbytes(mid) < key:
                lo = mid + 1
            else:
                hi = mid
        return lo

    def prefix_range(self, prefix):
        key = prefix.encode("utf-8")
        lo = self.lower_bound(key)
        hi = lo
        while hi < self.n and self.wbytes(hi).startswith(key):
            hi += 1
        return lo, hi

    def find(self, w):
        i = self.lower_bound(w.encode("utf-8"))
        return i if i < self.n and self.word(i) == w else None

    def uni(self, i):
        return struct.unpack_from("<H", self.b, self.o_uni + 2 * i)[0]

    def mult(self, i):
        return struct.unpack_from("<H", self.b, self.o_mult + 2 * i)[0]

    def successors(self, a):
        s, e = struct.unpack_from("<II", self.b, self.o_ps + 4 * a)
        pos, prev = self.o_pd + s, 0
        end = self.o_pd + e
        while pos < end:
            v, shift = 0, 0
            while True:
                byte = self.b[pos]
                pos += 1
                v |= (byte & 0x7F) << shift
                shift += 7
                if not byte & 0x80:
                    break
            prev += v
            (score,) = struct.unpack_from("<H", self.b, pos)
            pos += 2
            yield prev, score

    def suggest(self, prev, typed, n=3, min_gain=1):
        """Top n (display form) for words that start with typed and are at least min_gain letters longer."""
        lo, hi = self.prefix_range(typed)
        need = len(typed) + min_gain
        cands = {}
        a = self.find(prev) if prev else None
        m = 0
        if a is not None:
            m = self.mult(a)
            for b, s in self.successors(a):
                if lo <= b < hi and len(self.word(b)) >= need:
                    cands[b] = s
        for i in range(lo, hi):
            u = self.uni(i)
            if i not in cands and u != NEVER and len(self.word(i)) >= need:
                cands[i] = min(NEVER, u + m)
        top = sorted(cands.items(), key=lambda x: (x[1], x[0]))[:n]
        return [self.display(i) for i, _ in top]


def tsv_suggest(uni, mult, pairs, case, prev, typed, n=3, min_gain=1):
    need = len(typed) + min_gain
    cands = {b: p for b, p in pairs.get(prev, []) if b.startswith(typed) and len(b) >= need}
    m = mult.get(prev, 1.0) if prev in uni or prev in pairs else 1.0
    for w, p in uni.items():
        if w.startswith(typed) and len(w) >= need and w not in cands:
            cands[w] = m * p
    top = sorted(cands.items(), key=lambda x: -x[1])[:n]
    return [case.get(w, w) for w, _ in top]


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "write":
        write(sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5], len(sys.argv) > 6 and sys.argv[6] == "tr")
    elif cmd == "query":
        t = Table(sys.argv[2])
        for qy in sys.argv[3:]:
            prev, typed = qy.split(":")
            print(f"{prev} {typed}… → {' '.join(t.suggest(prev, typed, 5))}")
    elif cmd == "check":
        t = Table(sys.argv[2])
        uni, mult, pairs, case = load_tsv(sys.argv[3], sys.argv[4])
        rng = random.Random(1019)
        firsts = list(pairs)
        same = total = 0
        diffs = []
        for _ in range(int(sys.argv[5])):
            prev = rng.choice(firsts)
            b, _ = rng.choice(pairs[prev])
            typed = b[:rng.choice([0, 1, 2])]
            x, y = t.suggest(prev, typed), tsv_suggest(uni, mult, pairs, case, prev, typed)
            total += 1
            if [w.lower() for w in x] == [w.lower() for w in y]:
                same += 1
            elif len(diffs) < 8:
                diffs.append(f"{prev}:{typed} file={x} tsv={y}")
        print(f"same top-3 order: {same}/{total}")
        print("\n".join(diffs))
