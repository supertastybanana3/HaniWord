"""2026-11-05 A (仕事 22): / と . の打ち分けを測る組を、Wikipedia 日本語版の文から作る (区切りの probe と変換の測りの両方に使う)。
型ごとに、試験でない記事 (dev) と試験の記事 (記事 ID の 1 の位が 0、test) から別々に文を取り、Sudachi の読みで打つローマ字を作る。
  <Sudachi の python> -I make_sets.py <出力のフォルダ> <kana_romaji.py のあるフォルダ> <型ごとの文 (型<TAB>ID<TAB>文)>
型: dd_slash (数字/数字)・dd_dot_table (数字・数字＋表の語)・dd_dot_other (数字・数字＋ほか)・ll_slash (英字/英字)・ll_dot (英字・英字か英字・カタカナ)・
    jj_dot (日本語・日本語)・jj_slash (日本語/日本語)・dot_ext (かな＋. か 。＋拡張子)
出力 (型ごと・dev と test ごと):
  <型>_<dev|test>_kunrei_nn.txt   打つ列<TAB>(空)  … segmentation_probe 用 (期待する区切りは書かない。比べは別の道具で)
  <型>_<dev|test>.cases.tsv        読み<TAB>正しい文<TAB>(前の文脈は空)  … convert.py 用。読みの / と . は、打った列の左が英字・数字なら半角、ほかは ・ と 。 (仕事 22 の 1)
  <型>_<dev|test>.before.cases.tsv 同じ。読みの / と . はいつも ・ と 。 (前の形)
  <型>_<dev|test>.typed.tsv        打つ列<TAB>正しい文  … 打った列の / . の位置と、正しい文の ・ / 。 . を照らす用
打ち方: かな・漢字は Sudachi の読み (訓令式・ん は nn)、英字・数字はそのまま、・ と / と ／ は /、。 と . は .、、 は ,。ほかの記号を含む文は使わない"""
import collections, os, random, sys

out_dir, romaji_dir, src = sys.argv[1], sys.argv[2], sys.argv[3]
sys.path.insert(0, romaji_dir)
from kana_romaji import romaji  # noqa: E402
from sudachipy import dictionary, tokenizer  # noqa: E402

tok = dictionary.Dictionary(dict="full").create()
MODE = tokenizer.Tokenizer.SplitMode.C
os.makedirs(out_dir, exist_ok=True)
KEEP = set("・/／。.、")


def to_hira(s):
    return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)


def typed_and_reading(sentence):
    """(打つ列, 読み) か、使えない文なら None"""
    typed, reading = [], []
    for m in tok.tokenize(sentence, MODE):
        s = m.surface()
        if all(c.isascii() and (c.isalnum()) for c in s):
            typed.append(s); reading.append(s); continue
        if s in KEEP or all(c in KEEP for c in s):
            for c in s:
                t = "/" if c in "・/／" else "." if c in "。." else ","
                typed.append(t)
                # 読みの記号は、打った列がそろってから決める (marks)
                reading.append(t)
            continue
        if any(not ("ぁ" <= c <= "ゖ" or "ァ" <= c <= "ヺ" or c == "ー" or "一" <= c <= "鿿" or c in "々〆ヶ") for c in s):
            return None
        kana = to_hira(m.reading_form())
        if not kana or any(not ("ぁ" <= c <= "ゖ" or c == "ー") for c in kana):
            return None
        typed.append(romaji(kana, "kunrei_nn")); reading.append(kana)
    return "".join(typed), marks(reading, False), marks(reading, True)


def marks(parts, before):
    """読みの / . , を IME 本体の入れる字に。before: 前の形 (/ はいつも ・、. は数字にはさまれたら .、ほかは 。)。
    後の形 (仕事 22 の 1): / は左が数字か小文字だけの英字の語 (前が ・ でない) なら /、ほかは ・。. は左右が英字・数字なら .、ほかは 。"""
    text = "".join(parts)
    out = []
    for i, c in enumerate(text):
        left, right = text[i - 1:i], text[i + 1:i + 2]
        alnum = lambda x: x.isascii() and x.isalnum()
        if c == "/":
            # 左が数字か、小文字だけの英字の語 (その前が ・ でない) なら / (まとめ役 12:28)。略語・大文字の入る語・名前の中は ・ のまま
            head = text[:i].rstrip("abcdefghijklmnopqrstuvwxyz")
            lower = len(head) < i and not (head[-1:].isascii() and head[-1:].isalpha()) and head[-1:] not in ("・", "/")
            out.append("/" if not before and (left.isdigit() or lower) else "・")
        elif c == ".":
            if before: out.append("." if left.isdigit() and right.isdigit() else "。")
            else: out.append("." if alnum(left) and alnum(right) else "。")
        elif c == ",":
            out.append("," if left.isdigit() and right.isdigit() else "、")
        else:
            out.append(c)
    return "".join(out)


rows = collections.defaultdict(list)
for line in open(src, encoding="utf-8"):
    kind, aid, s = line.rstrip("\n").split("\t", 2)
    split = "test" if int(aid) % 10 == 0 else "dev"
    rows[(kind, split)].append(s)
random.seed(20261105)
summary = []
for (kind, split), sentences in sorted(rows.items()):
    random.shuffle(sentences)
    seen, picked = set(), []
    for s in sentences:
        if s in seen or len(s) > 120: continue
        seen.add(s)
        r = typed_and_reading(s)
        if r is None: continue
        picked.append((r[0], r[1], s, r[2]))
        if len(picked) >= 300: break
    base = os.path.join(out_dir, f"{kind}_{split}")
    with open(base + "_kunrei_nn.txt", "w", encoding="utf-8", newline="\n") as f:
        f.writelines(f"{t}\n" for t, _, _, _ in picked)
    with open(base + ".cases.tsv", "w", encoding="utf-8", newline="\n") as f:
        f.writelines(f"{r}\t{s}\t\n" for _, r, s, _ in picked)
    with open(base + ".before.cases.tsv", "w", encoding="utf-8", newline="\n") as f:
        f.writelines(f"{b}\t{s}\t\n" for _, _, s, b in picked)
    with open(base + ".typed.tsv", "w", encoding="utf-8", newline="\n") as f:
        f.writelines(f"{t}\t{s}\n" for t, _, s, _ in picked)
    summary.append(f"{kind}_{split}: {len(picked)} 文 (候補 {len(sentences)})")
print("\n".join(summary))
