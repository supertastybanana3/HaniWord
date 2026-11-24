"""(2026-10-22、M3.5 の B: 成句) JMdict の表現 (exp)・慣用句 (id) のうち、azooKey の辞書に無く、今の変換 (AI) でも正しく出ず、
今の変換との違いが表記ゆれ・数字の書き方だけでないものを、オフにできない追加辞書の行 (C の形) にする。
品詞の番号は句の最後の語 (Sudachi の A 単位) の azooKey での右の番号 (後ろにつながる側に合わせる)。
最後の語が動詞・形容詞の基本形なら、同じ活用の型でその語の活用形 (azooKey の辞書にある形: 叩か・叩き・叩い…) を句の頭に付けた形も作る (尻を叩い + て)。
値はどれも同じ (句の読み全体を打ったときに勝つ程度。引数で変える)
--prefer <表>: 今の変換に関わらず足す書き方 (読み<TAB>書き方[<TAB>値[<TAB>出典]]。代わりの字の書き方が意味に合わない成句: 論を俟たない/論を待たない。
         judge_variants.py で 2 つの判定がそろったもの)。足すと句の読みでその書き方が 1 番目になり、かな書き・代わりの字の書き方はその下の候補に残る
--any-reading: --prefer の行は、表記と読みが合わなくても入れる (誤った読みで本来の形を出す core_misreadings。のべつくまなし → のべつまくなし)
--exclude <表>: 入れない句 (表記<TAB>理由。測った文で、文の正しい句と同じ読みでぶつかって悪くした句。活用形も入れない)
使い方: HANIWORD_BUILD=<作業場所> <Sudachi のある Python> make_core_phrases.py <作業の置き場 (azookey_dict.tsv・azk_pos.tsv・jmdict_siblings.json のある所)> <jm_full.jsonl> <out.tsv> [値 (既定 -11)] [--prefer <表>]"""
import collections, difflib, json, os, re, sys
from sudachipy import dictionary, tokenizer
from common import analyze as load_analyze
args = sys.argv[1:]
prefer_path = exclude_path = None
any_reading = "--any-reading" in args
if any_reading:
    args.remove("--any-reading")
if "--prefer" in args:
    i = args.index("--prefer"); prefer_path = args[i + 1]; del args[i:i + 2]
if "--exclude" in args:
    i = args.index("--exclude"); exclude_path = args[i + 1]; del args[i:i + 2]
excluded = {l.split("\t")[0] for l in open(exclude_path, encoding="utf-8") if l.strip() and not l.startswith("#")} if exclude_path else set()
Q, src, out = args[:3]
value = float(args[3]) if len(args) > 3 else -11.0
analyze = load_analyze()
def kata(s): return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
label = {int(l.split("\t")[0]): l.split("\t")[2].split(",") for l in open(f"{Q}/azk_pos.tsv", encoding="utf-8")}
by_word = collections.defaultdict(list)    # (読み, 表記) -> [(rcid, mid)]
by_stem = collections.defaultdict(list)    # 表記の末尾のかなを除いたもの -> [(読み, 表記, rcid, mid)]
HIRA = "ぁ-ゖ"
for l in open(f"{Q}/azookey_dict.tsv", encoding="utf-8"):
    r, w, lc, rc, mid, v = l.rstrip("\n").split("\t")
    by_word[(r, w)].append((int(rc), int(mid)))
    stem = w.rstrip("".join(chr(c) for c in range(0x3041, 0x3097)))
    if stem != w and stem:
        by_stem[stem].append((r, w, int(rc), int(mid)))
# 同じ JMdict の項目の書き方 (読みが同じもの。かなだけの書き方を含む): 今の変換がそのどれかなら、どの書き方を選ぶかは今の辞書と AI に任せる
siblings = json.load(open(os.path.join(Q, "jmdict_siblings.json"), encoding="utf-8"))
tok = dictionary.Dictionary(dict="full").create()
A = tokenizer.Tokenizer.SplitMode.A
def entry_for(surface, reading, major):
    """最後の語の azooKey の項目 (右の番号・意味の番号)。品詞の大分類が Sudachi と同じものを選ぶ"""
    items = by_word.get((reading, surface), [])
    same = [x for x in items if label.get(x[0], [""])[0] == major]
    pick = collections.Counter(same or items).most_common(1)
    return pick[0][0] if pick else None
rows, why = set(), collections.Counter()
items = [(json.loads(line), False) for line in open(src, encoding="utf-8")]
if prefer_path:
    items += [({"reading": c[0], "expected": c[1], "value": float(c[2]) if len(c) > 2 else value, "source": c[3] if len(c) > 3 else ""}, True)
              for c in (l.rstrip("\n").split("\t") for l in open(prefer_path, encoding="utf-8") if l.strip() and not l.startswith("#"))]
have = {k for k in by_word}
for r, preferred in items:
    surface, reading = r["expected"], kata(r["reading"])
    if surface in excluded: why["入れない句の表にある"] += 1; continue
    if preferred and (reading, surface) in have: why["本来の書き方が azooKey にある"] += 1; continue
    if preferred: pass
    elif r["ok"]: why["AI で正しい"] += 1; continue
    if not preferred and (len(surface) < 2 or len(reading) < 4): why["短い (読み 4 字未満)"] += 1; continue
    if not preferred and r["full"] in siblings.get(f"{r['reading']}\t{surface}", ()): why["今の変換が同じ項目の別の書き方"] += 1; continue
    if not preferred and analyze.kind(surface, r["full"]) in ("表記ゆれ", "数字や記号"): why["AI の変換との違いが表記ゆれ・数字だけ"] += 1; continue
    # 同じ読みを変換した結果なので、違う所のどちらかがかなだけなら書き方の違い (其内/そのうち・御尤も/ごもっとも)。両方に漢字がある違いだけを残す
    if not preferred:
        spans = [(surface[i1:i2], r["full"][j1:j2]) for op, i1, i2, j1, j2 in difflib.SequenceMatcher(None, surface, r["full"]).get_opcodes() if op != "equal"]
        kana = lambda t: re.fullmatch(r"[ぁ-ゖァ-ヺー]*", t) is not None
        if all(kana(a) or kana(b) for a, b in spans): why["違う所がかなの書き方だけ"] += 1; continue
    # 表記と読みの組が合っているか (JMdict の書き方と読みの組み合わせ): 表記のかなの並びが、どれも読みの中に順に出てくる
    pos_ = 0
    for run in re.findall(r"[ぁ-ゖー]+", surface):
        at = reading.find(kata(run), pos_)
        if at < 0: break
        pos_ = at + len(run)
    else:
        run = None
    if run is not None and not (preferred and any_reading): why["表記と読みが合わない"] += 1; continue
    ms = list(tok.tokenize(surface, A))
    last = ms[-1]; major = last.part_of_speech()[0]; ls, lr = last.surface(), last.reading_form()
    found = entry_for(ls, lr, major)
    if found is None: why["最後の語が azooKey に無い"] += 1; continue
    cid, mid = found
    src_note = r.get("source") or "JMdict (CC BY-SA 4.0) の表現・慣用句" + ("・本来の字の書き方" if preferred else "")
    v = r.get("value", value)
    rows.add((surface, reading, cid, mid, v, src_note)); why["入れる (本来の字の書き方)" if preferred else "入れる"] += 1
    lab = label.get(cid, [""] * 7)
    if lab[0] not in ("動詞", "形容詞") or (lab + [""] * 7)[5] != "基本形" or not reading.endswith(lr) or not surface.endswith(ls):
        continue
    # 語幹 = 基本形から最後のかな 1 字を除いたもの (押す → 押・懸ける → 懸け・悪い → 悪)。活用形は 語幹 + かな 0〜2 字で、読みもちょうどそれに合うもの。
    # サ変 (挺する) は する を除いたものを語幹にし、かな 3 字まで (挺し・挺さ・挺すれ・挺しろ。2026-10-22)
    sahen = lab[4].startswith("サ変") and ls.endswith("する") and lr.endswith("スル")
    if not sahen and (not "ぁ" <= ls[-1] <= "ゖ" or not lr.endswith(kata(ls[-1]))): continue
    stem, stem_reading = (ls[:-2], lr[:-2]) if sahen else (ls[:-1], lr[:-1])
    if not stem: continue
    head_s, head_r = surface[:-len(ls)], reading[:-len(lr)]
    n = 0
    for fr, fw, fc, fm in by_stem.get(stem.rstrip("".join(chr(c) for c in range(0x3041, 0x3097))), []):
        fl = label.get(fc, [""] * 7) + [""] * 7
        rest = fw[len(stem):]
        if fw.startswith(stem) and len(rest) <= (3 if sahen else 2) and re.fullmatch(r"[ぁ-ゖ]*", rest) and fr == stem_reading + kata(rest) \
                and fl[0] == lab[0] and fl[4] == lab[4] and fl[5] != "基本形" and fw != ls:
            rows.add((head_s + fw, head_r + fr, fc, fm, v, src_note + "の活用形")); n += 1
    why["活用形を足した句"] += n > 0
with open(out, "w", encoding="utf-8") as f:
    f.write("# 表記\t読み\tcid\tmid\t値\t出典\n" + "".join("\t".join(map(str, x)) + "\n" for x in sorted(rows)))
print(dict(why), "行", len(rows))
