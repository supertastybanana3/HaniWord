"""変換サーバーの DLL (azookey-server.dll) に読みを渡して変換する。Windows の Python で動かす (py -3 -I convert.py ...)。
分野辞書の作り方 (今の辞書で既に正しく変換できる語を捨てる) と、効果の測定 (分野の文で上がり、一般の文で下がらない) に使う。

  入力 (TSV、# で始まる行は説明): 読み<TAB>正解[<TAB>文脈]。読みはひらがなかカタカナ (カタカナはひらがなに直して渡す)
  文脈 (3 列目。前の文など、左の文脈として変換サーバーに渡す) は --context を付けたときだけ使う。2026-11-06 まで 3 列目をいつも文脈に渡していて、
  語の試しの塊 (draft_readings.py の 読み<TAB>表記<TAB>出どころ) では egov・sudachi などの出どころの字を前に置いて変換していた
  出力 (JSONL、1 行 1 問): reading, expected, full (AI の変換), dict (辞書だけの変換), ok (full が正解と同じ)
  途中で止めても、出力の行数から続きを始める。

隔離のため APPDATA を引数で指定する (WSL から py.exe に環境変数は渡らないので、KEY=VALUE を引数で受けて os.environ に入れる)。
利用者の %APPDATA% を指していたら止める。学習は使わない (settings.json の learning=false を確かめる)。
--reset: 1 問ごとに決まった読みを先に変換する (サーバーは直前の変換の続きを使い回すので、順番で結果が揺れるのを防ぐ)
使い方: py -3 -I convert.py --stage <stage> --cases <in.tsv> --out <out.jsonl> APPDATA=<隔離したフォルダ> [--reset] [--no-dict] [--context]
"""
import argparse, ctypes, json, os, sys, time
from pathlib import Path


class Candidate(ctypes.Structure):
    _fields_ = [("text", ctypes.c_char_p), ("subtext", ctypes.c_char_p), ("hiragana", ctypes.c_char_p), ("count", ctypes.c_int)]


def hira(s):
    return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)


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
    p.add_argument("--reset", action="store_true")
    p.add_argument("--no-dict", action="store_true")
    p.add_argument("--context", action="store_true", help="3 列目を左の文脈として渡す (文の組。語の試しでは付けない)")
    p.add_argument("--spare-cpus", type=int, default=4, help="使わずに残す論理プロセッサーの数 (ほかのアプリのため。優先度も通常以下にする。8 個なら 4 個まで使う＝2 本同時でも合わせて半分)")
    a = p.parse_args([x for x in sys.argv[1:] if x not in env])
    # 2026-10-20: ほかのアプリに影響しないよう、優先度を通常以下にし、最後の --spare-cpus 個の論理プロセッサーを使わない
    k32 = ctypes.windll.kernel32
    k32.GetCurrentProcess.restype = ctypes.c_void_p  # 64 ビットのハンドルを int で受けると壊れる
    k32.SetPriorityClass.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
    k32.SetProcessAffinityMask.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
    me = k32.GetCurrentProcess()
    if not k32.SetPriorityClass(me, 0x00004000):  # BELOW_NORMAL_PRIORITY_CLASS
        print("SetPriorityClass failed", file=sys.stderr)
    cpus = os.cpu_count() or 1
    if cpus > a.spare_cpus and not k32.SetProcessAffinityMask(me, (1 << (cpus - a.spare_cpus)) - 1):
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
    dll.ConvertSegments.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_bool, ctypes.c_bool, ctypes.POINTER(ctypes.c_int)]
    dll.ConvertSegments.restype = ctypes.POINTER(ctypes.POINTER(Candidate))
    dll.Initialize((str(a.stage.resolve()) + "\\").encode(), True)

    def convert(reading, context, dict_only):
        n = ctypes.c_int()
        r = dll.ConvertSegments(reading.encode(), context.encode(), False, dict_only, ctypes.byref(n))
        return "".join(r[i].contents.text.decode() for i in range(n.value))

    cases = []
    for line in a.cases.read_text(encoding="utf-8").splitlines():
        if line.strip() and not line.startswith("#"):
            c = line.split("\t")
            cases.append((hira(c[0]), c[1] if len(c) > 1 else "", c[2] if a.context and len(c) > 2 else ""))
    done = sum(1 for _ in a.out.open(encoding="utf-8")) if a.out.exists() else 0
    t0 = time.perf_counter()
    with a.out.open("a", encoding="utf-8") as out:
        for k, (reading, expected, context) in enumerate(cases[done:], start=done):
            if a.reset:
                convert("あ", "", False)
            full = convert(reading, context, False)
            rec = {"reading": reading, "expected": expected, "full": full, "ok": full == expected}
            if not a.no_dict:
                rec["dict"] = convert(reading, context, True)
            out.write(json.dumps(rec, ensure_ascii=False) + "\n")
            if k % 500 == 0:
                out.flush()
                print(f"{k}/{len(cases)} {time.perf_counter() - t0:.0f}s", flush=True)
    print("done", len(cases), f"{time.perf_counter() - t0:.0f}s")


main()
