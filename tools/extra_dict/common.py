"""オフにできない追加辞書 (辞書の抜け M3.5) の道具で共通に使うもの。
作業場所 (リポジトリの外。Mozc の公開辞書・写経の文の組・試しのステージを置く) は環境変数 HANIWORD_BUILD で渡す"""
import collections, glob, os, sys

B = os.environ.get("HANIWORD_BUILD") or sys.exit("set HANIWORD_BUILD to the build workspace (data_ext/mozc_oss, probe/e2e, ...)")
MOZC = os.path.join(B, "data_ext", "mozc_oss")


def kata(s):
    return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)


def hira(s):
    return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)


def mozc_iddef():
    """Mozc の品詞の番号 -> 品詞の名前 (id.def)"""
    return {int(l.split(" ", 1)[0]): l.rstrip("\n").split(" ", 1)[1] for l in open(os.path.join(MOZC, "id.def"), encoding="utf-8")}


def mozc_entries():
    """Mozc の公開辞書の (読み (カタカナ), 表記) -> (左の品詞, 右の品詞, コスト)。同じ組はコストの低いほう"""
    out = {}
    for path in sorted(glob.glob(os.path.join(MOZC, "dictionary0*.txt"))):
        for l in open(path, encoding="utf-8"):
            p = l.rstrip("\n").split("\t")
            if len(p) >= 5:
                k = (kata(p[0]), p[4])
                if k not in out or int(p[3]) < out[k][2]:
                    out[k] = (int(p[1]), int(p[2]), int(p[3]))
    return out


def analyze():
    """写経の外れの型 (作業場所の写経の集計 probe/e2e/analyze.py の kind・lev)"""
    sys.path.insert(0, os.path.join(B, "probe", "e2e"))
    import analyze as a
    return a
