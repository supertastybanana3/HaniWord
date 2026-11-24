"""分野辞書のデータ (domain_dictionaries/<名前>.tsv) から、変換サーバーが読む形 (domain_<名前>.louds ほか) と domains.txt を書き出す。
配布物を作るとき (scripts/release/package.py) に、files\\domain に向けて呼ぶ想定。書き出したあと読み戻して、全部の語が入ったか確かめる。

<名前>.tsv の列 (# で始まる行は説明): 表記 / 読み (カタカナ) / 品詞の番号 (左右とも) / 意味の番号 / 値 / 出典 (| 区切り。Wikipedia:<記事名> を含む)
同音語の優遇 (変換サーバーの DomainBoost) のために、domain_<名前>_boost.tsv (分野辞書の語と <名前>_boost.tsv の語: 表記<TAB>読み) と
domain_<名前>_signals.txt (<名前>_signals.txt の目印の語) も書く
使い方: python3 build_domain.py --out <置き場所> [--chid <charID.chid>] [--licenses <ライセンス表示の置き場所>] [名前 ...] [--always [名前 ...]]
        (名前を省くと domain_dictionaries の全部)。--licenses を付けると、出典にウィキペディアの記事がある語の記事の一覧
        domain_<名前>_wikipedia.txt (CC BY-SA 4.0 の表示) を書く。分野辞書のライセンス文 (domain_dictionaries/LICENSE.txt) は
        辞書の隣に LICENSE.txt として、--licenses の置き場所に domain_dictionaries_LICENSE.txt として写す
2026-10-22: オフにできない追加辞書 (辞書の抜け M3.5。extra_dictionaries/<名前>.tsv、名前は core_ で始める。列は分野辞書と同じ) も
        同じ形 (domain_<名前>.louds ほか) で書き、名前を always.txt に並べる (domains.txt には入れない)。--always で選ぶ。
        分野の名前も --always も省くと全部。分野の名前だけ付けたとき (calibrate.py) は追加辞書も always.txt もさわらない。
        追加辞書のライセンス文 (extra_dictionaries/LICENSE.txt) は辞書の隣に LICENSE_extra.txt として、--licenses の置き場所に
        extra_dictionaries_LICENSE.txt として写す (--licenses を付けたときは無ければ止める)
2026-11-06 (仕事 39): 方言の言い方の辞書 (dialect_dictionaries/<名前>.tsv。列は分野辞書と同じ、<名前>_forms.tsv は形の出どころの表で辞書ではない) も
        同じ形で domain_dialect_<名前>.louds ほかに書き、名前を dialects.txt に並べる (domains.txt には入れない)。--dialects で選ぶ。
        設定 input.dialects で選んだ人だけ読む (初期値は使わない)。同音語の優遇は使わない。ライセンス文 (dialect_dictionaries/LICENSE.txt) は
        辞書の隣に LICENSE_dialect.txt として、--licenses の置き場所に dialect_dictionaries_LICENSE.txt として写す
2026-11-07 (仕事 43): くだけた言葉の辞書 (slang_dictionaries/<名前>.tsv、今は internet = ネットスラング。列は分野辞書と同じ) も
        同じ形で domain_slang_<名前>.louds ほかに書き、名前を slang.txt に並べる。--slang で選ぶ。設定でオンにした人だけ読む (初期値はオフ)。
        同音語の優遇は使わない。ライセンス文 (slang_dictionaries/LICENSE.txt) は辞書の隣に LICENSE_slang.txt として、
        --licenses の置き場所に slang_dictionaries_LICENSE.txt として写す
"""
import argparse, glob, os, re, shutil, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import louds

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
CHID = os.path.join(REPO, "server-swift", "azooKey_dictionary_storage", "Dictionary", "louds", "charID.chid")
NAME = re.compile(r"[a-z0-9_]{1,32}")


def read_entries(path):
    entries = []
    for k, line in enumerate(open(path, encoding="utf-8"), start=1):
        if not line.strip() or line.startswith("#"):
            continue
        c = line.rstrip("\n").split("\t")
        word, ruby, cid, mid, value = c[0], c[1], int(c[2]), int(c[3]), float(c[4])
        if not re.fullmatch(r"[ァ-ヺー]+", ruby) or not word or len(word) > 64 or not (0 <= cid < 1319) or not (0 <= mid < 502) \
                or not -40.0 <= value <= -5.0:
            sys.exit(f"{path}:{k}: bad entry {c[:5]}")
        entries.append((ruby, word, cid, cid, mid, value))
    return sorted(set(entries))


def write_dictionary(path, ident, chid, out):
    """<名前>.tsv を変換サーバーが読む形 (<ident>.louds ほか) にして、読み戻して全部の語が入ったか確かめる"""
    entries = read_entries(path)
    for old in glob.glob(os.path.join(out, ident + ".*")) + glob.glob(os.path.join(out, ident + "[0-9]*.loudstxt3")):
        os.remove(old)
    nodes, skipped = louds.build(entries, chid, out, ident)
    if skipped:
        sys.exit(f"{ident}: {skipped} readings use characters missing from charID.chid")
    restored = sorted((r, *item) for p in glob.glob(os.path.join(out, ident + "[0-9]*.loudstxt3"))
                      for r, items in louds.read_txt(p) for item in items)
    if [(r, w, l, rc, m) for r, w, l, rc, m, _ in restored] != [(r, w, l, rc, m) for r, w, l, rc, m, _ in entries] or \
            any(abs(x[5] - y[5]) > 1e-4 for x, y in zip(restored, entries)):
        sys.exit(f"{ident}: the written dictionary does not read back the same")
    return entries, nodes


def write_wikipedia_list(name, outdir):
    """出典の列の「Wikipedia:<記事名>」を集めて、記事の URL の一覧にする"""
    import urllib.parse
    titles = set()
    for line in open(os.path.join(REPO, "domain_dictionaries", name + ".tsv"), encoding="utf-8"):
        if line.startswith("#") or not line.strip():
            continue
        cols = line.rstrip("\n").split("\t")
        for src in (cols[5] if len(cols) > 5 else "").split("|"):
            if src.startswith("Wikipedia:"):
                titles.add(src[len("Wikipedia:"):])
    # 英字の略語の表 (<名前>_abbrev.txt) の語を拾った記事も載せる (2026-11-01)
    abbrev_sources = os.path.join(REPO, "domain_dictionaries", name + "_abbrev_sources.tsv")
    if os.path.exists(abbrev_sources):
        for line in open(abbrev_sources, encoding="utf-8"):
            cols = line.rstrip("\n").split("\t")
            if not line.startswith("#") and len(cols) > 1 and cols[1].startswith("Wikipedia:"):
                titles.add(cols[1][len("Wikipedia:"):])
    os.makedirs(outdir, exist_ok=True)
    with open(os.path.join(outdir, f"domain_{name}_wikipedia.txt"), "w", encoding="utf-8", newline="\n") as f:
        f.write(f"分野辞書 domain_{name} のうち、次の日本語版ウィキペディアの記事の題名と冒頭の読みを素材にした語があります。\n"
                "記事の文章はクリエイティブ・コモンズ 表示-継承 4.0 国際 (CC BY-SA 4.0, https://creativecommons.org/licenses/by-sa/4.0/) の下で公開されています。\n"
                "使ったのは題名と読みだけで、読みは付け直して確かめています (改変)。\n\n")
        for t in sorted(titles):
            f.write(f"{t}\thttps://ja.wikipedia.org/wiki/{urllib.parse.quote(t.replace(' ', '_'))}\n")
    # ウィキブックスの高校の教科書 (化学・情報・地理など)
    pages = set()
    for line in open(os.path.join(REPO, "domain_dictionaries", name + ".tsv"), encoding="utf-8"):
        cols = line.rstrip("\n").split("\t")
        if not line.startswith("#") and len(cols) > 5:
            pages |= {s[len("Wikibooks:"):] for s in cols[5].split("|") if s.startswith("Wikibooks:")}
    if pages:
        with open(os.path.join(outdir, f"domain_{name}_wikibooks.txt"), "w", encoding="utf-8", newline="\n") as f:
            f.write(f"分野辞書 domain_{name} のうち、次の日本語版ウィキブックスのページの本文に出てくる語を素材にしたものがあります。\n"
                    "ページの文章はクリエイティブ・コモンズ 表示-継承 4.0 国際 (CC BY-SA 4.0, https://creativecommons.org/licenses/by-sa/4.0/) の下で公開されています。\n"
                    "使ったのは語 (名詞の連なり) だけで、読みは付け直して確かめています (改変)。\n\n")
            for t in sorted(pages):
                f.write(f"{t}\thttps://ja.wikibooks.org/wiki/{urllib.parse.quote(t.replace(' ', '_'))}\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--chid", default=CHID)
    ap.add_argument("--licenses")
    ap.add_argument("names", nargs="*")
    ap.add_argument("--always", nargs="*", help="オフにできない追加辞書 (extra_dictionaries/<名前>.tsv) の名前")
    ap.add_argument("--dialects", nargs="*", help="方言の言い方の辞書 (dialect_dictionaries/<名前>.tsv) の名前")
    ap.add_argument("--slang", nargs="*", help="くだけた言葉の辞書 (slang_dictionaries/<名前>.tsv) の名前")
    a = ap.parse_args()
    # 名前を省いたら domain_dictionaries の分野辞書を全部 (同音語の優遇の表 <名前>_boost.tsv・略語の出どころ <名前>_abbrev_sources.tsv は分野ではない)
    names = a.names or sorted(n for n in (os.path.splitext(os.path.basename(p))[0] for p in glob.glob(os.path.join(REPO, "domain_dictionaries", "*.tsv")))
                              if not n.endswith(("_boost", "_abbrev_sources")))
    always = a.always if a.always is not None else \
        [] if a.names else sorted(os.path.splitext(os.path.basename(p))[0] for p in glob.glob(os.path.join(REPO, "extra_dictionaries", "*.tsv")))
    chid = louds.load_chid(a.chid)
    os.makedirs(a.out, exist_ok=True)
    for name in names:
        if not NAME.fullmatch(name) or name.startswith("core_"):
            sys.exit(f"bad domain name: {name}")
        entries, nodes = write_dictionary(os.path.join(REPO, "domain_dictionaries", name + ".tsv"), "domain_" + name, chid, a.out)
        ident = "domain_" + name
        print(f"{name}: {len(entries)} entries, {nodes} nodes")
        # 同音語の優遇: 分野辞書の語 + 変換器の辞書にある分野の語 (<名前>_boost.tsv)、文脈の目印 (<名前>_signals.txt)
        # 送り仮名・数字の書き方だけ違う項目 (取消/取り消し、手続/手続き) は優遇しない (法令風の表記を押し付けない)
        def skeleton(x):
            return "".join(c for c in x.translate(str.maketrans("0123456789", "〇一二三四五六七八九")) if not ("ぁ" <= c <= "ゖ"))
        variants = set()
        for line in open(os.path.join(REPO, "domain_dictionaries", name + ".tsv"), encoding="utf-8"):
            c = line.rstrip("\n").split("\t")
            if not line.startswith("#") and len(c) > 6 and c[6] and skeleton(c[6]) == skeleton(c[0]):
                variants.add((c[0], c[1]))
        boost = {(w, r) for r, w, *_ in entries} - variants
        extra = os.path.join(REPO, "domain_dictionaries", name + "_boost.tsv")
        if os.path.exists(extra):
            for line in open(extra, encoding="utf-8"):
                c = line.rstrip("\n").split("\t")
                if not line.startswith("#") and len(c) >= 2 and re.fullmatch(r"[ァ-ヺー]+", c[1]) and c[0]:
                    boost.add((c[0], c[1]))
        with open(os.path.join(a.out, ident + "_boost.tsv"), "w", encoding="utf-8", newline="\n") as f:
            f.write("# 分野の文脈で優遇する語 (表記<TAB>読み)\n" + "".join(f"{w}\t{r}\n" for w, r in sorted(boost)))
        signals = os.path.join(REPO, "domain_dictionaries", name + "_signals.txt")
        with open(os.path.join(a.out, ident + "_signals.txt"), "w", encoding="utf-8", newline="\n") as f:
            f.write("# 文脈に出たらこの分野の文とみなす語\n")
            if os.path.exists(signals):
                f.writelines(l for l in open(signals, encoding="utf-8") if l.strip() and not l.startswith("#"))
        # 英字の略語の表 (よく打つ分野に選んだ人の IME 本体が英字の判定の語の表に足す。2026-11-01)
        abbrev = os.path.join(REPO, "domain_dictionaries", name + "_abbrev.txt")
        if os.path.exists(abbrev):
            shutil.copyfile(abbrev, os.path.join(a.out, ident + "_abbrev.txt"))
        if a.licenses:
            write_wikipedia_list(name, a.licenses)
    # 分野辞書のファイル自体のライセンス (CC BY-SA 4.0) と出典を、辞書の隣とライセンス表示の置き場所に
    shutil.copyfile(os.path.join(REPO, "domain_dictionaries", "LICENSE.txt"), os.path.join(a.out, "LICENSE.txt"))
    if a.licenses:
        shutil.copyfile(os.path.join(REPO, "domain_dictionaries", "LICENSE.txt"), os.path.join(a.licenses, "domain_dictionaries_LICENSE.txt"))
    with open(os.path.join(a.out, "domains.txt"), "w", encoding="utf-8", newline="\n") as f:
        f.write("# HaniWord の分野辞書 (変換サーバーが読む。設定 input.domains_off でオフにできる)\n" + "".join(n + "\n" for n in names))
    if a.always is None and a.names:
        return
    # オフにできない追加辞書 (分野の名前とぶつからないよう core_ で始める)。同音語の優遇は使わない
    for name in always:
        if not NAME.fullmatch(name) or not name.startswith("core_"):
            sys.exit(f"bad always-on dictionary name (core_<名前>): {name}")
        entries, nodes = write_dictionary(os.path.join(REPO, "extra_dictionaries", name + ".tsv"), "domain_" + name, chid, a.out)
        print(f"{name} (always on): {len(entries)} entries, {nodes} nodes")
    license_txt = os.path.join(REPO, "extra_dictionaries", "LICENSE.txt")
    if always and os.path.exists(license_txt):
        shutil.copyfile(license_txt, os.path.join(a.out, "LICENSE_extra.txt"))
        if a.licenses:
            shutil.copyfile(license_txt, os.path.join(a.licenses, "extra_dictionaries_LICENSE.txt"))
    elif always and a.licenses:
        sys.exit(f"missing {license_txt} (the license of the always-on dictionaries)")
    with open(os.path.join(a.out, "always.txt"), "w", encoding="utf-8", newline="\n") as f:
        f.write("# HaniWord のオフにできない追加辞書 (変換サーバーが読む。辞書の抜け: 活用形・成句・語)\n" + "".join(n + "\n" for n in always))
    write_dialects(a, chid)
    write_slang(a, chid)


def write_slang(a, chid):
    """くだけた言葉の辞書 (2026-11-07、仕事 43)。分野の名前も --always も --dialects も --slang も省いたときは全部、--slang を付けたらその名前だけ"""
    if a.slang is None and (a.names or a.always is not None or a.dialects is not None):
        return
    folder = os.path.join(REPO, "slang_dictionaries")
    slang = a.slang if a.slang is not None else \
        sorted(os.path.splitext(os.path.basename(p))[0] for p in glob.glob(os.path.join(folder, "*.tsv")))
    for name in slang:
        if not NAME.fullmatch(name) or name.startswith("core_") or name[-1].isdigit():
            sys.exit(f"bad slang dictionary name: {name}")
        entries, nodes = write_dictionary(os.path.join(folder, name + ".tsv"), "domain_slang_" + name, chid, a.out)
        print(f"{name} (slang): {len(entries)} entries, {nodes} nodes")
    license_txt = os.path.join(folder, "LICENSE.txt")
    if slang:
        if not os.path.exists(license_txt):
            sys.exit(f"missing {license_txt} (the license of the slang dictionaries)")
        shutil.copyfile(license_txt, os.path.join(a.out, "LICENSE_slang.txt"))
        if a.licenses:
            shutil.copyfile(license_txt, os.path.join(a.licenses, "slang_dictionaries_LICENSE.txt"))
    with open(os.path.join(a.out, "slang.txt"), "w", encoding="utf-8", newline="\n") as f:
        f.write("# HaniWord のくだけた言葉の辞書 (変換サーバーが読む。設定でオンにしたものだけ。internet = input.internet_slang)\n" + "".join(n + "\n" for n in slang))


def write_dialects(a, chid):
    """方言の言い方の辞書 (2026-11-06、仕事 39)。分野の名前も --always も --dialects も省いたときは全部、--dialects を付けたらその名前だけ"""
    if a.dialects is None and (a.names or a.always is not None):
        return
    folder = os.path.join(REPO, "dialect_dictionaries")
    dialects = a.dialects if a.dialects is not None else \
        sorted(n for n in (os.path.splitext(os.path.basename(p))[0] for p in glob.glob(os.path.join(folder, "*.tsv"))) if not n.endswith("_forms"))
    for name in dialects:
        if not NAME.fullmatch(name) or name.startswith("core_") or name[-1].isdigit():
            sys.exit(f"bad dialect name: {name}")
        entries, nodes = write_dictionary(os.path.join(folder, name + ".tsv"), "domain_dialect_" + name, chid, a.out)
        print(f"{name} (dialect): {len(entries)} entries, {nodes} nodes")
    license_txt = os.path.join(folder, "LICENSE.txt")
    if dialects:
        if not os.path.exists(license_txt):
            sys.exit(f"missing {license_txt} (the license of the dialect dictionaries)")
        shutil.copyfile(license_txt, os.path.join(a.out, "LICENSE_dialect.txt"))
        if a.licenses:
            shutil.copyfile(license_txt, os.path.join(a.licenses, "dialect_dictionaries_LICENSE.txt"))
    with open(os.path.join(a.out, "dialects.txt"), "w", encoding="utf-8", newline="\n") as f:
        f.write("# HaniWord の方言の言い方の辞書 (変換サーバーが読む。設定 input.dialects で選んだものだけ)\n" + "".join(n + "\n" for n in dialects))


if __name__ == "__main__":
    main()
