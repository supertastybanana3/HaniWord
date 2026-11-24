"""ライブ変換の 1 キーあたりの時間を、変換サーバーの DLL で測る。Windows の Python で動かす (py -3 -I latency.py ...)。
IME はキーを押すたびに、いまの読み全体を「辞書だけ・速い変換」(ConvertSegments の fast=1, dictionaryOnly=1) で変換し直す。
それをまねて、文の読みを 1 字ずつ伸ばしながら変換し、1 回ごとの時間を記録する。前の文の末尾 40 字を文脈 (SetContext) にする。
分野辞書の入り切りは、隔離した APPDATA の settings.json の input.domains_off で変える (分野ごとに別のフォルダを用意する)。

  入力 (TSV): 読み<TAB>正解 (eval_sets.py の cases と同じ)。先頭から --sentences 文を使う
  出力 (JSONL に 1 行追記): 名前・回数・中央値・90%・99%・最大・100ms/300ms/1000ms 超の回数 (ms)、
    1 キーあたりのファイルの読み出しの回数 (reads_per_key。GetProcessIoCounters の ReadOperationCount の増え方)
使い方: py -3 -I latency.py --stage <stage> --cases <in.tsv> --out <out.jsonl> --name <名前> APPDATA=<隔離したフォルダ> [--sentences 60] [--full]
  --full: 文を打ち終えたところで、AI の変換 (fast=0, dictionaryOnly=0) の時間も別に記録する
"""
import argparse, ctypes, json, os, sys, time
from pathlib import Path


class IoCounters(ctypes.Structure):
    _fields_ = [(n, ctypes.c_ulonglong) for n in ("ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
                                                    "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]


def io_reads():
    k32 = ctypes.windll.kernel32
    k32.GetCurrentProcess.restype = ctypes.c_void_p
    k32.GetProcessIoCounters.argtypes = [ctypes.c_void_p, ctypes.POINTER(IoCounters)]
    c = IoCounters()
    k32.GetProcessIoCounters(k32.GetCurrentProcess(), ctypes.byref(c))
    return c.ReadOperationCount, c.ReadTransferCount


class Candidate(ctypes.Structure):
    _fields_ = [("text", ctypes.c_char_p), ("subtext", ctypes.c_char_p), ("hiragana", ctypes.c_char_p), ("count", ctypes.c_int)]


def hira(s):
    return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)


def summary(ms):
    s = sorted(ms)
    if not s:
        return {}
    q = lambda p: round(s[min(len(s) - 1, int(p * len(s)))], 1)
    return {"n": len(s), "median": q(0.5), "p90": q(0.9), "p99": q(0.99), "max": round(s[-1], 1),
            "over100": sum(x > 100 for x in s), "over300": sum(x > 300 for x in s), "over1000": sum(x > 1000 for x in s)}


def main():
    env = [x for x in sys.argv[1:] if "=" in x and not x.startswith("-")]
    for kv in env:
        k, v = kv.split("=", 1)
        os.environ[k] = v
    p = argparse.ArgumentParser()
    p.add_argument("--stage", type=Path, required=True)
    p.add_argument("--backend", default="llama_vulkan")
    p.add_argument("--cases", type=Path, required=True)
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--name", required=True)
    p.add_argument("--sentences", type=int, default=60)
    p.add_argument("--full", action="store_true")
    p.add_argument("--cpus", default="", help="使う論理プロセッサーの番号 (例 6,7)。ほかの測りと重ならないようにする")
    a = p.parse_args([x for x in sys.argv[1:] if x not in env])
    if a.cpus:
        k32 = ctypes.windll.kernel32
        k32.GetCurrentProcess.restype = ctypes.c_void_p
        k32.SetProcessAffinityMask.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        if not k32.SetProcessAffinityMask(k32.GetCurrentProcess(), sum(1 << int(c) for c in a.cpus.split(","))):
            print("SetProcessAffinityMask failed", file=sys.stderr)
    appdata = Path(os.environ.get("APPDATA", ""))
    if appdata.resolve() == (Path.home() / "AppData" / "Roaming").resolve():
        sys.exit("APPDATA must point to an isolated folder")
    settings = json.loads((appdata / "HaniWord" / "settings.json").read_text(encoding="utf-8"))
    if settings.get("zenzai", {}).get("learning", True):
        sys.exit("learning must be off in the isolated settings.json")
    sys.stdout.reconfigure(encoding="utf-8")
    for d in (a.stage, a.stage / a.backend):
        os.add_dll_directory(str(d.resolve()))
    os.environ["PATH"] = str((a.stage / a.backend).resolve()) + ";" + os.environ["PATH"]
    dll = ctypes.CDLL(str((a.stage / "azookey-server.dll").resolve()))
    dll.Initialize.argtypes = [ctypes.c_char_p, ctypes.c_bool]
    dll.SetContext.argtypes = [ctypes.c_char_p]
    dll.ConvertSegments.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_bool, ctypes.c_bool, ctypes.POINTER(ctypes.c_int)]
    dll.ConvertSegments.restype = ctypes.POINTER(ctypes.POINTER(Candidate))
    t0 = time.perf_counter()
    dll.Initialize((str(a.stage.resolve()) + "\\").encode(), True)
    init_ms = (time.perf_counter() - t0) * 1000

    def convert(reading, fast, dict_only):
        n = ctypes.c_int()
        t = time.perf_counter()
        r = dll.ConvertSegments(reading.encode(), b"", fast, dict_only, ctypes.byref(n))
        ms = (time.perf_counter() - t) * 1000
        return "".join(r[i].contents.text.decode() for i in range(n.value)), ms

    cases = []
    for line in a.cases.read_text(encoding="utf-8").splitlines():
        if line.strip() and not line.startswith("#"):
            c = line.split("\t")
            cases.append((hira(c[0]), c[1] if len(c) > 1 else ""))
    cases = cases[:a.sentences]
    keys, full, first = [], [], []
    context = ""
    key_reads = key_bytes = 0
    for reading, expected in cases:
        dll.SetContext(context[-40:].encode())
        r0, b0 = io_reads()
        for i in range(1, len(reading) + 1):
            _, ms = convert(reading[:i], True, True)
            keys.append(ms)
            if i == 1:
                first.append(ms)
        r1, b1 = io_reads()
        key_reads += r1 - r0
        key_bytes += b1 - b0
        if a.full:
            _, ms = convert(reading, False, False)
            full.append(ms)
        context += expected
    rec = {"name": a.name, "init_ms": round(init_ms), "keys": summary(keys), "first_key": summary(first),
           "reads_per_key": round(key_reads / max(1, len(keys)), 2), "kb_per_key": round(key_bytes / 1024 / max(1, len(keys)), 1)}
    if a.full:
        rec["full"] = summary(full)
    with a.out.open("a", encoding="utf-8") as out:
        out.write(json.dumps(rec, ensure_ascii=False) + "\n")
    print(json.dumps(rec, ensure_ascii=False))
    print("LATENCY_DONE")


main()
