"""azooKey の辞書の形 (LOUDS: <名前>.louds・<名前>.loudschars2・<名前><番号>.loudstxt3) を書き出す・読み戻す。
並びは変換器の学習データの書き出し (LearningMemory.swift の LongTermLearningMemory.update) と同じ:
幅優先・子は文字の番号の昇順・先頭に空のノードが 2 つ。2026-10-11 に学習データとバイト単位で一致することを確かめた書き方。"""
import os, struct

SPLIT = 2048


def load_chid(path):
    """文字 → 番号 (辞書の charID.chid は文字を番号の順に並べた UTF-8 の文字列)"""
    return {c: i for i, c in enumerate(open(path, encoding="utf-8").read())}


def _bits_to_u64(bits):
    unit = 64
    q, r = divmod(len(bits), unit)
    bits = bits + [True] * ((unit - r) % unit)
    out = []
    for i in range(q + 1):
        v = 0
        for j in range(unit):
            if i * unit + j < len(bits) and bits[i * unit + j]:
                v |= 1 << (unit - j - 1)
        out.append(v)
    return out


def _entry_bytes(ruby, items):
    b = struct.pack("<H", len(items))
    for (word, lcid, rcid, mid, value) in items:
        b += struct.pack("<HHHf", lcid, rcid, mid, value)
    text = "\t".join([ruby] + ["" if w == ruby else w for (w, *_rest) in items])
    return b + text.encode("utf-8")


def _txt(blocks):
    data = [_entry_bytes(r, it) for r, it in blocks]
    header = [2 + 4 * len(data)]
    for d in data[:-1]:
        header.append(header[-1] + len(d))
    return struct.pack("<H", len(data)) + struct.pack(f"<{len(header)}I", *header) + b"".join(data)


def build(entries, chid, outdir, ident):
    """entries: [(読み (カタカナ), 表記, lcid, rcid, mid, 値)]。辞書の文字表に無い文字を含む読みは飛ばす。(ノードの数, 飛ばした数) を返す"""
    nodes = [{"data": [], "ch": {}}]
    skipped = 0
    for ruby, word, lcid, rcid, mid, value in entries:
        try:
            chars = [chid[c] for c in ruby]
        except KeyError:
            skipped += 1
            continue
        idx = 0
        for c in chars:
            if c not in nodes[idx]["ch"]:
                nodes[idx]["ch"][c] = len(nodes)
                nodes.append({"data": [], "ch": {}})
            idx = nodes[idx]["ch"][c]
        nodes[idx]["data"].append((ruby, (word, lcid, rcid, mid, value)))
    n2c = [0, 0]
    blocks = [("", []), ("", [])]
    bits = [True, False]
    cur = sorted(nodes[0]["ch"].items())
    bits += [True] * len(cur) + [False]
    while cur:
        for c, ni in cur:
            n2c.append(c)
            d = nodes[ni]["data"]
            blocks.append((d[0][0] if d else "", [x[1] for x in d]))
            bits += [True] * len(nodes[ni]["ch"]) + [False]
        cur = [kv for _, ni in cur for kv in sorted(nodes[ni]["ch"].items())]
    os.makedirs(outdir, exist_ok=True)
    u = _bits_to_u64(bits)
    open(os.path.join(outdir, f"{ident}.louds"), "wb").write(struct.pack(f"<{len(u)}Q", *u))
    open(os.path.join(outdir, f"{ident}.loudschars2"), "wb").write(bytes(n2c))
    for k in range(len(blocks) // SPLIT + 1):
        open(os.path.join(outdir, f"{ident}{k}.loudstxt3"), "wb").write(_txt(blocks[k * SPLIT:(k + 1) * SPLIT]))
    return len(nodes), skipped


def read_txt(path):
    """loudstxt3 を [(読み, [(表記, lcid, rcid, mid, 値)])] に読み戻す"""
    d = open(path, "rb").read()
    n = struct.unpack("<H", d[:2])[0]
    idx = list(struct.unpack(f"<{n}I", d[2:2 + 4 * n]))
    out = []
    for i in range(n):
        seg = d[idx[i]: idx[i + 1] if i + 1 < n else len(d)]
        c = struct.unpack("<H", seg[:2])[0]
        items = [struct.unpack("<HHHf", seg[2 + 10 * j: 12 + 10 * j]) for j in range(c)]
        parts = seg[2 + 10 * c:].split(b"\t")
        ruby = parts[0].decode()
        words = [p.decode() or ruby for p in parts[1:]]
        out.append((ruby, [(w,) + tuple(x) for w, x in zip(words, items)]))
    return out
