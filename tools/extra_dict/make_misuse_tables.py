"""よくある誤用 (M3.6) の表を作る (2026-10-22)。入力は作業場所の data_ext/misuse/misuse.tsv (原因・誤った形・誤った読み・本来の形・本来の読み・出典・ライセンス・印・備考)。
考え方: IME は打ったとおりに従う。打った読みと違う語を先に出さない。
- 字の誤り (同じ読み。危機一発→危機一髪): 代わりの字の成句と同じ並べ替え → misuse_spellings.txt (make_variant_spellings.py が読む)。
  意味で決まるもの (若干〔年齢〕/弱冠。若干 は別の正しい語) は入れない
- 音の誤りのうち、本来の形が崩れただけでほかに意味のある語が無いもの (音位転換・清濁・区切り): 誤った読みで本来の形を出す追加辞書の行
  → core_misreadings_prefer.txt (make_core_phrases.py --prefer --any-reading で core_misreadings.tsv にする)
- 音の誤りのうち、誤った形が語の並びになるもの (似た音・音の脱落) と、印のあるもの (読みのゆれ・一生懸命): 本来の形を候補の一覧に足す
  → misreading_candidates.tsv (変換サーバーが読む。読み<TAB>本来の形[<TAB>正しい読み])
  2026-11-20: 誤り (似た音・音の脱落で、印が無く本来の読みのあるもの) には 3 列目に正しい読みを書く (変換サーバーが一覧の 4 番目に足し、注記「もしかして: 正しい読み」)。
  読みのゆれ・印のあるものは 3 列目なし (今までどおり一覧の後ろ、注記なし)。
  Mozc の reading_correction.tsv (作業場所の data_ext/mozc_reading_correction/。commit は COMMIT) の行 (語・誤った読み・正しい読み) もみな足す
  (誤った読みがほかの語にならないものは core_misreadings にもあり、一覧ではもう出ているので注記だけ付く)
  誤った読みが azooKey の辞書で強い語 (本来の形とそのかな以外で、値 -8 以上。かいし の 開始・じゅうよう の 重要) の読みでもある行は、
  4 列目に「後」を書く (変換サーバーが 4 番目でなく一覧の後ろに、注記つきで足す。開始・重要 を打った人の一覧の上のほうを汚さない。まとめ役 11-20)。
  辞書は作業場所の data_ext/misreading_top/azookey_dict.tsv (dump_azookey_dict.py で stage の Dictionary/louds から書き出したもの)
- 注記 (オプション。既定でオフ): 言い回しの混ざり・重ね言葉・字の誤り・似た音の「本来は〜」と、異字同訓の使い分け → misuse_notes.tsv (変換サーバーが読む)
  列: 種類 (誤用|異字同訓)<TAB>引き方 (含む|読み)<TAB>鍵<TAB>注記。含む = 候補の字が鍵を含む (的を得 → 的を得た)。読み = 鍵が 読み:表記
使い方: HANIWORD_BUILD=<作業場所> python3 make_misuse_tables.py <リポジトリ>"""
import os, re, sys
from common import B, hira

repo = sys.argv[1]
src = os.path.join(B, "data_ext", "misuse", "misuse.tsv")
rows = [l.rstrip("\n").split("\t") for l in open(src, encoding="utf-8") if l.strip() and not l.startswith("#")]
assert rows[0][0] == "原因"
rows = [dict(zip(rows[0], r)) for r in rows[1:]]
quoted = lambda t: (re.findall(r"「([^」]+)」", t) or [t])[0].replace("、", "")
spell, misread_top, misread_back, notes = [], [], [], []
for r in rows:
    cause, wrong, wrong_reading, right, mark = r["原因"], r["誤った形"], r["誤った読み"], r["本来の形"], r["印"]
    weak = "誤用ではないとの考えもある" in mark or "本来の読みを示していない" in mark
    if cause == "字の誤り":
        if "（" in wrong:   # 若干（年齢に使う）: 意味で決まる
            continue
        spell.append((wrong_reading, right, wrong))
        notes.append(("誤用", "含む", wrong, f"本来は {right}"))
    elif cause.startswith("音の誤り"):
        kind = cause[cause.find("（") + 1:-1] if "（" in cause else ""
        surface = quoted(right) if right else ""
        for reading in (x for x in wrong_reading.split("／") if x):
            if kind in ("音位転換", "清濁", "区切り") and not weak:
                misread_top.append((reading, surface))
            elif surface:
                right_reading = hira(r["本来の読み"]) if r["本来の読み"] and not weak and kind in ("似た音", "音の脱落") else ""
                misread_back.append((reading, re.sub(r"（[^）]*）", "", surface), right_reading))
        if kind == "似た音":
            notes.append(("誤用", "含む", re.sub(r"（[^）]*）", "", wrong), f"本来は {re.sub(r'（[^）]*）', '', right)}"))
    elif cause in ("言い回しの混ざり", "重ね言葉"):
        key = re.sub(r"（[^）]*）", "", wrong)
        if len(key) >= 4 and "ぁ" <= key[-1] <= "ゖ":
            key = key[:-1]   # 活用で変わる語尾を除く (的を得る → 的を得: 的を得た にも当たる)
        right_text = re.sub(r"（[^）]*）", "", right).replace("／", "・")
        note = f"本来は {right_text}" if right_text else "重ね言葉"
        notes.append(("誤用", "含む", key, note + ("（誤用ではないとの考えもある）" if weak else "")))
    elif cause == "異字同訓":
        reading = r["本来の読み"]
        for part in r["備考"].split("／"):
            if "：" not in part:
                continue
            forms, gloss = part.split("：", 1)
            gloss = gloss.rstrip("*").split("。")[0]
            for form, form_reading in zip(forms.split("・"), (reading.split("・") * 2)[:len(forms.split("・"))]):
                notes.append(("異字同訓", "読み", f"{form_reading}:{form}", gloss))


def write(path, header, lines):
    with open(os.path.join(repo, path), "w", encoding="utf-8", newline="\n") as f:
        f.write(header + "".join("\t".join(x) + "\n" for x in lines))


source = "# 出典: 文化庁「国語に関する世論調査」各年度・文化審議会国語分科会「「異字同訓」の漢字の使い分け例（報告）」(平成26年) を加工して作成、Wikipedia 日本語版「日本語の誤用」(CC BY-SA 4.0)。extra_dictionaries/LICENSE.txt\n"
write("extra_dictionaries/misuse_spellings.txt", "# 字の誤り (同じ読み): 読み<TAB>本来の書き方<TAB>誤った書き方。make_variant_spellings.py が variant_spellings.tsv に入れる\n" + source, spell)
write("extra_dictionaries/core_misreadings_prefer.txt", "# 音の誤りで、ほかに意味のある語が無いもの: 読み (誤った読み)<TAB>本来の形<TAB>値<TAB>出典。make_core_phrases.py --prefer --any-reading で core_misreadings.tsv にする\n" + source,
      [(r, s, "-11", "文化庁「国語に関する世論調査」を加工して作成 (音の誤り)") for r, s in misread_top])
# 2026-11-20: Mozc の reading_correction.tsv (語<TAB>誤った読み<TAB>正しい読み) の行を、3 列の行として足す
mozc_dir = os.path.join(B, "data_ext", "mozc_reading_correction")
mozc_commit = open(os.path.join(mozc_dir, "COMMIT"), encoding="utf-8").read().strip()
mozc = [l.rstrip("\n").split("\t") for l in open(os.path.join(mozc_dir, "reading_correction.tsv"), encoding="utf-8") if l.strip() and not l.startswith("#")]
assert all(len(m) == 3 and all(m) for m in mozc), "reading_correction.tsv の行の形"
candidates = {(hira(r), s): right for r, s, right in misread_back}
for word, wrong_reading, right_reading in mozc:
    candidates.setdefault((hira(wrong_reading), word), hira(right_reading))
STRONG = -8.0
kata = lambda t: "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in t)
wanted = {kata(r) for (r, _), right in candidates.items() if right}
strongest = {}
for line in open(os.path.join(B, "data_ext", "misreading_top", "azookey_dict.tsv"), encoding="utf-8"):
    c = line.rstrip("\n").split("\t")
    if len(c) >= 6 and c[0] in wanted:
        strongest.setdefault(c[0], []).append((c[1], float(c[5])))
def placed_at_end(reading, form):
    k = kata(reading)
    return any(v >= STRONG for w, v in strongest.get(k, []) if w not in (form, reading, k))
write("misreading_candidates.tsv", "# 音の誤りで、誤った形も語の並びになるもの・読みのゆれ・読み間違い: 読み (打った読み)<TAB>本来の形[<TAB>正しい読み]。変換サーバーが、その読みの候補の一覧に足す (無いときだけ)。\n"
      "# 正しい読みのある行 (読み間違い) は 4 番目に足して注記「もしかして: 正しい読み」を出し (4 列目が 後 なら後ろに注記つきで)、無い行 (読みのゆれ) は後ろに足す\n" + source
      + f"# 読み間違いの行の多く: Mozc の reading_correction.tsv (https://github.com/google/mozc の src/data/dictionary_oss/reading_correction.tsv、commit {mozc_commit})。"
      "許諾は extra_dictionaries/LICENSE.txt の「Mozc の公開辞書のファイルの許諾」\n",
      sorted((r, s) + ((right,) if right else ()) + (("後",) if right and placed_at_end(r, s) else ()) for (r, s), right in candidates.items()))
# 2026-11-07 (仕事 40): 注意の注記 (差別・言い換え) の行は extra_dictionaries/caution_notes.txt (tools/caution で作る) から、そのまま後ろに足す。
# 5 列目は「含む」の除く語 (| 区切り。候補がそのどれかを含めば注記を付けない)
caution = [tuple(l.rstrip("\n").split("\t")) for l in open(os.path.join(repo, "extra_dictionaries", "caution_notes.txt"), encoding="utf-8")
           if l.strip() and not l.startswith("#")]
assert all(len(c) == 5 and c[0] in ("差別", "言い換え") and c[1] in ("含む", "読み") for c in caution), "caution_notes.txt の行の形"
write("misuse_notes.tsv", "# 候補窓の注記 (設定でオンのときだけ IME 本体が出す。既定でオフ): 種類 (誤用|異字同訓|差別|言い換え)<TAB>引き方 (含む|読み)<TAB>鍵<TAB>注記[<TAB>除く語]\n" + source
      + "# 差別・言い換え の行 (注意の注記): extra_dictionaries/caution_notes.txt。出典は licenses\\caution_references.txt (番号は注記の [n])、extra_dictionaries/LICENSE.txt\n",
      sorted(set(notes)) + caution)
print("字の誤り", len(spell), "上に出す音の誤り", len(misread_top), "一覧に足す", len(candidates), "(正しい読みつき", sum(1 for v in candidates.values() if v), "・Mozc", len(mozc),
      "・後ろに足す", sum(1 for (r, s), v in candidates.items() if v and placed_at_end(r, s)), ")",
      "注記", len(set(notes)), "注意の注記", len(caution))
