"""2026-11-06: 配布物の変換サーバーの一式 (release/files) で、どの DLL がどこからも読み込まれていないかを、PE の import 表 (と遅延 import 表) から数える。
根: azookey-server.exe・azookey-server.dll・llama_*/*.dll。たどり着けない DLL を書き出す (外せる候補。外すのは動かして確かめてから)
使い方: python3 pe_imports.py <release/files>"""
import glob, os, struct, sys

def imports(path):
    d = open(path, "rb").read()
    pe = struct.unpack_from("<I", d, 0x3C)[0]
    nsec = struct.unpack_from("<H", d, pe + 6)[0]
    optsz = struct.unpack_from("<H", d, pe + 20)[0]
    opt = pe + 24
    magic = struct.unpack_from("<H", d, opt)[0]
    ddir = opt + (112 if magic == 0x20B else 96)
    secs = []
    for i in range(nsec):
        o = opt + optsz + 40 * i
        va, vsz, raw, rawsz = struct.unpack_from("<IIII", d, o + 12)[:1] + struct.unpack_from("<I", d, o + 8) + struct.unpack_from("<II", d, o + 20)
        secs.append((struct.unpack_from("<I", d, o + 12)[0], struct.unpack_from("<I", d, o + 8)[0], struct.unpack_from("<I", d, o + 20)[0]))
    def off(rva):
        for va, vsz, raw in secs:
            if va <= rva < va + max(vsz, 1):
                return rva - va + raw
        return None
    def cstr(o):
        e = d.index(b"\0", o)
        return d[o:e].decode("ascii", "replace")
    names = set()
    for idx, size in ((1, 20), (13, 32)):  # import 表・遅延 import 表
        rva = struct.unpack_from("<I", d, ddir + 8 * idx)[0]
        if not rva:
            continue
        o = off(rva)
        while o is not None:
            name_rva = struct.unpack_from("<I", d, o + (12 if idx == 1 else 4))[0]
            if not name_rva:
                break
            no = off(name_rva)
            if no is None:
                break
            names.add(cstr(no).lower())
            o += size
    return names

root = sys.argv[1]
files = {os.path.basename(p).lower(): p for p in glob.glob(os.path.join(root, "*.dll")) + glob.glob(os.path.join(root, "*.exe"))}
llama = glob.glob(os.path.join(root, "llama_*", "*.dll"))
todo = [files["azookey-server.exe"], files["azookey-server.dll"]] + llama
seen, used = set(), set()
while todo:
    p = todo.pop()
    if p in seen:
        continue
    seen.add(p)
    for n in imports(p):
        if n in files and files[n] not in seen:
            used.add(n)
            todo.append(files[n])
unused = sorted(n for n in files if n.endswith(".dll") and n not in used and n != "azookey-server.dll")
size = sum(os.path.getsize(files[n]) for n in unused)
print(f"DLL {sum(1 for n in files if n.endswith('.dll'))} 使う {len(used)} 使わない {len(unused)} ({size / 1e6:.1f}MB)")
for n in unused:
    print(f"  {n}\t{os.path.getsize(files[n])}")
