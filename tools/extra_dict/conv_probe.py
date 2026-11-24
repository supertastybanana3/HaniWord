"""(2026-10-22、M3.5) 読み<TAB>表記 の組を、隔離したサーバーで変換する。先に辞書だけの速い変換をし、表記と同じならそこで止める (AI の変換を省く)
使い方: py -3 -I conv_probe.py <stage> <cases.tsv> <out.jsonl> APPDATA=<隔離したフォルダ> [--dict-only]
--dict-only: 辞書だけの速い変換だけをする (AI の変換は、表記ゆれなどを外したあとの残りにだけかける)"""
import ctypes, json, os, sys
from pathlib import Path
for kv in [a for a in sys.argv[1:] if "=" in a]:
    k, v = kv.split("=", 1); os.environ[k] = v
dict_only = "--dict-only" in sys.argv
args = [a for a in sys.argv[1:] if "=" not in a and not a.startswith("--")]
stage, cases, out = Path(args[0]), Path(args[1]), Path(args[2])
if Path(os.environ.get("APPDATA", "")).resolve() == (Path.home() / "AppData" / "Roaming").resolve(): sys.exit("APPDATA must be isolated")
class Candidate(ctypes.Structure):
    _fields_ = [("text", ctypes.c_char_p), ("subtext", ctypes.c_char_p), ("hiragana", ctypes.c_char_p), ("count", ctypes.c_int)]
for d in (stage, stage / "llama_vulkan"): os.add_dll_directory(str(d.resolve()))
os.environ["PATH"] = str((stage / "llama_vulkan").resolve()) + ";" + os.environ["PATH"]
dll = ctypes.CDLL(str((stage / "azookey-server.dll").resolve()))
dll.Initialize.argtypes = [ctypes.c_char_p, ctypes.c_bool]
dll.ConvertSegments.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_bool, ctypes.c_bool, ctypes.POINTER(ctypes.c_int)]
dll.ConvertSegments.restype = ctypes.POINTER(ctypes.POINTER(Candidate))
dll.Initialize((str(stage.resolve()) + "\\").encode(), True)
def convert(reading, fast, dict_only):
    n = ctypes.c_int(); r = dll.ConvertSegments(reading.encode(), b"", fast, dict_only, ctypes.byref(n))
    return "".join(r[i].contents.text.decode() for i in range(n.value))
done = sum(1 for _ in open(out, encoding="utf-8")) if out.exists() else 0
lines = [l for l in cases.read_text(encoding="utf-8").splitlines() if l.strip() and not l.startswith("#")]
with out.open("a", encoding="utf-8") as f:
    for i, line in enumerate(lines[done:], start=done):
        reading, expected = line.split("\t")[:2]
        d = convert(reading, True, True)
        full = d if d == expected or dict_only else convert(reading, False, False)
        f.write(json.dumps({"reading": reading, "expected": expected, "dict": d, "full": full, "ok": full == expected}, ensure_ascii=False) + "\n"); f.flush()
        if i % 2000 == 0: print(f"{i}/{len(lines)}", flush=True)
print("done", len(lines))
