"""2026-11-11 C: ソフト・アプリ・サービスの製品名の表を作る (まとめ役 09:42 の決め)。
入力: collect_articles.py の出力 (記事の題・月の平均の閲覧数)・redirects.py の出力 (転送の題)・リポジトリの頭 (英字の表)・azooKey の辞書 (dump_azookey_dict.py の出力)
表記 (空白を含むか、頭のほかに大文字のある表記。数字の入った版の名も):
- 記事の題 (英字のもの)。ウィキペディアの題は頭が大文字になるので、i の後ろに大文字が続く題 (IOS・ITunes) は i に、語ごとに表記直しの表 (english_case・
  english_brands) にある語はその表記にする
- 頭の語を除いた形 (Adobe After Effects → After Effects): 転送の題にその形があるか、頭の語が会社の頭 (英字の題 3 つ以上の頭で、社名・表記直しの表にある語) で、
  残りにふつうの英単語でない語があるとき
- 転送の題のうち、製品名に版の語 (Pro・Elements・Server など) を 1 つ足した形 (Adobe Premiere → Premiere Pro・Adobe Premiere Pro)
- 外す: 3 字以下の 1 語・1 語でふつうの英単語の綴り (BASIC)・1 字の小文字の語を含む (Time t)・2 語目から小文字の題 (Virtual private network)・
  大文字だけの転送 (MINECRAFT)・今の表記直しの表と同じ表記
線: 記事の月の平均の閲覧数が --min (既定 300) 以上
出力 1 english_products.txt (かなモードの英字): 英数字の語を空白 1 つで並べた表記と、1 語の表記。打つ綴り (小文字の英数字だけ: aftereffects) が
  ローマ字として読める (A の ime_readable.py で R) ものは入れない (abema・paypay を英字にしない)。記号の入る表記 (Yahoo! JAPAN・7-Zip) は、打った字と
  字数が変わるので入れない (読みの辞書には入れる)
出力 2 core_products.tsv (読みからの変換): 表記と読み (カタカナ)。読みは
  - カタカナの転送の題 (ダビンチリゾルブ)。語ごとの読みが分かる語は、その読みが順に入っているものだけ (Windows XP ← リモートアシスタンス は採らない)。
    どの語の読みも分からないときは、記事の題の表記だけに付ける。読みの字数が表記の英字の数の半分から 1.6 倍まで (イラレ・つべ のような略称は採らない)
  - 英単語の表 (english_words.tsv・english_words_extra.tsv: JMdict と CMUdict のカタカナ語) で語ごとに組み立てた読み。複数の s は落とした読みも
    (effects → えふぇくつ・えふぇくと、teams → ちーむず)
  値と品詞の番号は仕事 46 にならう (1288、5 字以上 -13.0・4 字まで -14.5)。同じ読みの語が azooKey の辞書か追加辞書にあれば入れない (46 の 3351b40 と同じ)
使い方: python3 -I make_products.py <articles.tsv> <redirects.tsv> <リポジトリ> <azookey_dict.tsv> <english_products.txt> <core_products.tsv> <候補の一覧.tsv>
        --readable-script <ime_readable.py> [--min 300]"""
import glob, itertools, re, subprocess, sys, collections
arts_p, red_p, repo, dic_p, eng_out, dict_out, cand_out = sys.argv[1:8]
def opt(n, d=None): return sys.argv[sys.argv.index(n) + 1] if n in sys.argv else d
LINE, readable_script = int(opt("--min", "300")), opt("--readable-script")
LATIN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9 .+&'!\-]*$")
PLAIN = re.compile(r"^[A-Za-z0-9]+( [A-Za-z0-9]+)*$")   # 英数字の語を空白 1 つで並べた表記 (かなモードの英字に入れられる形)
def strip_note(t): return re.sub(r"\s*\([^)]*\)$", "", t).strip()
def is_kata(s): return bool(re.fullmatch(r"[ァ-ヺー・ 　]+", s))
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
def key(f): return re.sub(r"[^a-z0-9]", "", f.lower())
def lines(p): return [l.strip() for l in open(p, encoding="utf-8") if l.strip() and not l.startswith("#")]
scowl = {w.lower() for w in lines(f"{repo}/english_words_scowl.txt")}
# 2026-11-19: 1 語の表記がふつうの英単語かは、カタカナ語の元の英単語 (english_words.tsv。JMdict と CMUdict から自動で作った表) でも見る
# (neutrino → ニュートリノ を いつも NEUTRINO にしていた)。english_words_context.txt (SCOWL の size 60) は製品名・技術用語 (cobol・sqlite) も
# 小文字で入っているので使わない。手で足す english_words_extra.tsv は製品名 (GeoGuessr) が入るので使わない
ordinary = set(scowl)
for l in open(f"{repo}/english_words.tsv", encoding="utf-8"):
    c = l.rstrip("\n").split("\t")
    if not l.startswith("#") and len(c) >= 2 and re.fullmatch(r"[a-z]+", c[1]): ordinary.add(c[1])
cased = {}
for p in ("english_brands.txt", "english_case.txt"):
    for w in lines(f"{repo}/{p}"): cased[w.lstrip("~").lower()] = w.lstrip("~")
kana = collections.defaultdict(set)
for p in ("english_words.tsv", "english_words_extra.tsv"):
    for l in open(f"{repo}/{p}", encoding="utf-8"):
        if l.startswith("#"): continue
        c = l.rstrip("\n").split("\t")
        if len(c) >= 2: kana[c[1].lower()].add(c[0])
skipped_case = []
def fix_case(f):
    """語ごとの大文字小文字を表記直しの表 (english_brands・english_case) にそろえる。ウィキペディアは題の頭の字を大文字にするので、表に無い頭の語は:
    - 「I＋大文字＋小文字」(ITunes・IMessage) は i にする (前は I＋大文字 のどれも i にしていて、IBM Db2 を iBM Db2 にしていた。2026-11-19)
    - ほかの「大文字 1 つ＋大文字で始まるふつうの英単語」(UBlock = U＋Block) は、頭が小文字 (uBlock) か大文字 (XQuery) か分からないので None (表に入れない)
    - それ以外 (FFmpeg・KKday・OCaml) は書かれたまま
    表の書き方が頭の 1 字だけ小文字 (IOS → iOS) なら、表の書き方にする"""
    out = []
    for i, w in enumerate(f.split(" ")):
        c = cased.get(w.lower())
        if i == 0 and c and c[0].islower() and c[1:] == w[1:]: out.append(c); continue
        if i == 0 and not c:
            if re.fullmatch(r"I[A-Z][a-z]+", w): out.append("i" + w[1:]); continue
            if re.fullmatch(r"[A-Z][A-Z][a-z]+", w) and w[1:].lower() in ordinary: skipped_case.append(f); return None
        if c and c != w and not (w.isupper() and len(w) <= 4): w = c
        out.append(w)
    return " ".join(out)
def qualifies(f):
    if " " not in f and len(f) <= 3: return False
    ws = f.split(" ")
    if " " not in f and f.lower() in ordinary: return False   # 1 語でふつうの英単語の綴り (BASIC = basic・neutrino・handbrake) は単独で大文字にしない
    if any(len(w) == 1 and w.islower() for w in ws): return False   # 1 字の小文字の語 (Time t)
    if any(w[0].islower() and w.isalpha() for w in ws[1:]): return False   # 2 語目から小文字の題 (Virtual private network。製品名でなく概念の題)
    return " " in f or any(c.isupper() for c in f[1:]) or (f[0].islower() and any(c.isupper() for c in f))
# カタカナ語として azooKey の辞書にある読みの値 (エフェクト・プレミア)
katakana_value = {}
for l in open(dic_p, encoding="utf-8"):
    c = l.rstrip("\n").split("\t")
    if len(c) > 5 and c[0] == c[1]: katakana_value[c[0]] = max(katakana_value.get(c[0], -99.0), float(c[5]))
def by_value(rs): return sorted(rs, key=lambda r: (-katakana_value.get(kata(r), -99.0), len(r), r))
def plural(r): return r[:-1] + "つ" if r.endswith("と") else r[:-1] + "ず" if r.endswith("ど") else r if r[-1] in "すず" else r + ("す" if r[-1] in "くぷふ" else "ず")
# 2026-11-19: s 付きの英字の語が、s 付きの読み (ス・ズ・ツ で終わる) で azooKey の辞書にある語 (Windows ウィンドウズ・Works ワークス・Tools)
plural_words = set()
for l in open(dic_p, encoding="utf-8"):
    c = l.rstrip("\n").split("\t")
    if len(c) > 1 and re.fullmatch(r"[A-Za-z]+s", c[1]) and c[0][-1:] in ("ス", "ズ", "ツ"): plural_words.add(c[1].lower())
def word_readings(w, katas=()):
    """語の読み (2 つまで、よい順): 辞書にカタカナ語としてある読み、複数の s の語は単数の読みに ず・す・つ を付けた読み、語そのものの読み (短い順に 1 つ)、単数の読み。
    単数の読み (Effects → エフェクト) は、s 付きの語が辞書の語になっているか (Windows ウィンドウズ)、記事のカタカナの転送の題 katas に s 付きの読みがある
    (Microsoft Teams の チームズ) なら作らない (2026-11-19。マイクロソフトウィンドウ・マイクロソフトチーム・ラインワーク を製品名にしていた)"""
    w = w.lower()
    if re.fullmatch(r"\d+", w): return []
    exact = by_value(kana.get(w, ()))
    known = [r for r in exact if kata(r) in katakana_value]
    sing = by_value(kana.get(w[:-1], ())) if w.endswith("s") and len(w) > 3 else []
    plural_attested = bool(sing) and (w in plural_words or any(plural(sing[0]) in k for k in katas))
    out = []
    for r in known + ([plural(sing[0])] if sing else []) + exact[:1] + ([] if plural_attested else sing[:1]):
        if r not in out: out.append(r)
    return out[:2]
def gen_readings(form, katas=()):
    ws = form.split()
    per = [word_readings(w, katas) for w in ws]
    if not all(per) or len(ws) > 5: return set()
    return {"".join(p) for p in itertools.product(*per)}
def in_order(reading, form):
    """語ごとの読みが分かる語の読みが、カタカナの読みに順に入っているか。どの語も分からなければ None"""
    known = [word_readings(w) for w in form.split()]
    if not any(known): return None
    at = 0
    for rs in known:
        if not rs: continue
        hits = [reading.find(r, at) for r in rs if reading.find(r, at) >= 0]
        if not hits: return False
        at = min(hits) + 1
    return True
arts = [(l.split("\t")[0], int(l.split("\t")[1])) for l in open(arts_p, encoding="utf-8")]
redir = collections.defaultdict(list)
for l in open(red_p, encoding="utf-8"):
    t, rs = l.rstrip("\n").split("\t"); redir[t] = rs.split("|")
first = collections.Counter(strip_note(t).split()[0] for t, _ in arts if LATIN.match(strip_note(t)) and len(strip_note(t).split()) >= 2)
companies = {w for w, n in first.items() if n >= 3 and w[0].isupper() and w.lower() in cased}
EDITION = {"pro", "plus", "elements", "express", "community", "professional", "premium", "lite", "max", "ultra", "home", "server", "studio", "x",
           "cc", "classic", "online", "mobile", "desktop", "cloud", "enterprise", "standard"}
rows, skipped_forms = [], collections.Counter()
for t, pv in arts:
    if pv < LINE: continue
    base = strip_note(t)
    if not LATIN.match(base): continue
    bw = base.split(); has_co = len(bw) >= 2 and bw[0] in companies
    product = " ".join(bw[1:]) if len(bw) >= 2 else base
    reds = [strip_note(x) for x in redir.get(t, [])]
    red_keys = {key(x) for x in reds}
    forms = {}
    def add(f, src):
        f = fix_case(f)
        if f is None: skipped_forms["頭の語の大文字小文字が分からない"] += 1; return
        if qualifies(f) and key(f) not in {key(k) for k in forms}: forms[f] = src
    add(base, "題")
    if len(bw) >= 2 and (key(product) in red_keys or (has_co and any(w.lower() not in scowl for w in product.split() if not w.isdigit()))):
        add(product, "頭の語を除いた形")
    def edition(xs, ps): return len(xs) == len(ps) + 1 and [w.lower() for w in xs[:len(ps)]] == [w.lower() for w in ps] and xs[-1].lower() in EDITION
    for x in reds:
        if not LATIN.match(x) or key(x) == key(base) or x.isupper(): continue
        xs, ps = x.split(), product.split()
        if edition(xs, ps) or (len(bw) >= 2 and xs[0] == bw[0] and edition(xs[1:], ps)) or (len(bw) < 2 and edition(xs, bw)): add(x, "転送の題 (版の語)")
    katas = [hira(re.sub(r"[・ 　]", "", x)) for x in redir.get(t, []) if is_kata(x)]
    for f, src in forms.items():
        if key(f) in cased and cased[key(f)] == f: continue   # 今の表記直しの表と同じ
        letters = len(re.sub(r"[^A-Za-z]", "", f)) or 1
        kr = set()
        for k in katas:
            if not 0.5 <= len(k) / letters <= 1.6: continue
            o = in_order(k, f)
            if o or (o is None and src == "題"): kr.add(k)
        rows.append((pv, f, src, t, key(f), sorted(kr), sorted(gen_readings(f, katas))))
rows.sort(key=lambda r: (-r[0], r[1]))
# かなモードの英字: 英数字の語を空白で並べた表記 (と 1 語の表記) で、打つ綴りがローマ字として読めないもの
plain = [r for r in rows if PLAIN.match(r[1])]
keys = sorted({r[4] for r in plain})
readable = set()
if readable_script:
    out = subprocess.run([sys.executable, "-I", readable_script], input="\n".join(keys) + "\n", capture_output=True, text=True, check=True).stdout
    readable = {l.split("\t")[0] for l in out.splitlines() if l.endswith("\tR")}
eng, seen_keys = [], set()
for r in plain:
    if r[4] in readable or r[4] in seen_keys: continue
    seen_keys.add(r[4]); eng.append(r[1])
with open(eng_out, "w", encoding="utf-8", newline="\n") as f:
    f.write("# HaniWord のソフト・アプリ・サービスの製品名の表 (かなモードで小文字の英数字を続けて打った綴り aftereffects を、この表記 After Effects にする)。1 行 1 表記\n"
            "# 空白を含む表記は、打った英数字の並び全体が表記の英数字 (小文字) と同じときだけ。1 語の表記は表記直しの表 (english_case.txt) と同じ扱い\n"
            "# 出典: 日本語版ウィキペディア (CC BY-SA 4.0) の、ソフトウェア・アプリケーション・ウェブサービスのカテゴリの記事の題と転送の題\n"
            f"#   (2026-11 のダンプ)。記事の月の閲覧数の平均 (4 か月) が {LINE} 以上。作り方は tools/core_products/README.md\n")
    f.writelines(e + "\n" for e in sorted(eng, key=str.lower))
# 読みの辞書
best, pairs = collections.defaultdict(lambda: -99.0), set()
for l in open(dic_p, encoding="utf-8"):
    c = l.rstrip("\n").split("\t")
    if len(c) < 6: continue
    pairs.add((c[0], c[1]))
    if float(c[5]) > -29.0: best[c[0]] = max(best[c[0]], float(c[5]))
for p in glob.glob(f"{repo}/extra_dictionaries/core_*.tsv"):
    if p.replace("\\", "/").endswith(("core_names.tsv", "core_products.tsv")): continue
    for l in open(p, encoding="utf-8"):
        if l.startswith("#"): continue
        c = l.rstrip("\n").split("\t")
        if len(c) > 4:
            pairs.add((kata(c[1]), c[0]))
            try:
                if float(c[4]) > -29.0: best[kata(c[1])] = max(best[kata(c[1])], float(c[4]))
            except ValueError: pass
# 2026-11-19: 表記が azooKey の辞書にもうある語 (COBOL コボル・iTunes アイチューンズ) には、カタカナの転送の題の読み (コボラー・イツネス) を足さない
# (転送の題は、別の語 (COBOL を書く人) や綴りのローマ字読みのことがある。辞書の読みで出せる)。語ごとに組み立てた読み (アフターエフェクト) は足す
dict_forms = {w for (_, w) in pairs}
dict_rows, seen, skipped = [], set(), collections.Counter()
for r in rows:
    for y in sorted(set(r[5]) | set(r[6])):
        R = kata(y)
        if (r[1], R) in seen: continue
        seen.add((r[1], R))
        if (R, r[1]) in pairs: skipped["辞書に同じ表記と読みがある"] += 1; continue
        if r[1] in dict_forms and y not in r[6]: skipped["表記が辞書にある語の転送の題の読み"] += 1; continue
        if best[R] > -99.0: skipped["読みが辞書の語と同じ"] += 1; continue
        if len(y) <= 2: skipped["読みが 2 字まで"] += 1; continue
        v = -13.0 if len(y) >= 5 else -14.5
        dict_rows.append((r[1], R, "1288", "501", f"{v:.3f}", f"Wikipedia「{r[3]}」の題か転送の題 (2026-11 のダンプ)"))
with open(dict_out, "w", encoding="utf-8", newline="\n") as f:
    f.write("# 表記\t読み\tcid\tmid\t値\t出典\n")
    for d in dict_rows: f.write("\t".join(d) + "\n")
with open(cand_out, "w", encoding="utf-8", newline="\n") as f:
    f.write("# 閲覧\t表記\t出どころ\t記事\t打つ綴り\tカタカナの転送の読み\t英単語の表の読み\tかなモードの英字に入れたか\n")
    for r in rows: f.write("\t".join([str(r[0]), r[1], r[2], r[3], r[4], "|".join(r[5]), "|".join(r[6]),
                                      "入れた" if r[1] in eng else ("読める綴り" if r[4] in readable else ("記号の入る表記" if not PLAIN.match(r[1]) else "同じ綴りが先に"))]) + "\n")
print(f"表記 {len(rows)} (線 {LINE})・かなモードの英字 {len(eng)} (空白入り {sum(1 for e in eng if ' ' in e)}・1 語 {sum(1 for e in eng if ' ' not in e)}・"
      f"ローマ字として読めて外した {len({r[4] for r in plain} & readable)})・読みの辞書 {len(dict_rows)} 行 ({len({d[0] for d in dict_rows})} 表記)・外した読み {dict(skipped)}")
print("外した表記", dict(skipped_forms), sorted(set(skipped_case)))
print("会社の頭", sorted(companies))
