"""2026-11-04: オフにできない追加辞書 extra_dictionaries/core_interjections.tsv (感動詞・つなぎの言葉。文の頭で崩れる ほーん → ホーン、ふう → 風 を減らす)。
材料: JMdict (CC BY-SA 4.0) の感動詞 (jmdict_interjections.py の出力) のうち、ひらがな (ー・〜・小書き を含む) だけの読み、
      HaniWord の開発者が足したよく打つ言葉 (下の ADDED。よい形が 2 つあるものは | 区切り)、
      それらの ー・〜・小書きの形 (あー → あ〜、まあ → まぁ・まー、ふう → ふぅ)
azooKey の辞書に同じ (読み, 表記) が無い語だけを、品詞 感動詞 (3)・値 -14.0 で書く。2 字以上の読みだけ。〜 の形は辞書の読みの字に無いので入れない
  python3 -I build_table.py <jmdict_interjections.tsv> <azookey_dict.tsv> <出力 core_interjections.tsv> [<casual_words.tsv (リポジトリの頭)>]
2026-11-06: くだけた一言 (casual_words.tsv。だる・きも・おけ・りょ・ちょ など) も足す。よい形の 1 番目を値 -14.0、2 番目を -14.5 にする。
どちらを先にするかは表の よい形 の順で、語ごとに決めてある (変換サーバーの casual.swift も同じ表を読む)"""
import collections, os, re, sys
src, azd, out_path = sys.argv[1:4]
casual_path = sys.argv[4] if len(sys.argv) > 4 else None
kata = lambda s: "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)
HIRA = re.compile(r"^[ぁ-ゖー〜]+$")
# HaniWord の開発者が足した、よく打つ感動詞・つなぎの言葉 (よい形は | 区切り)
ADDED = {
    "そういや": "そういや", "てか": "てか", "ほーん": "ほーん", "あ": "あ", "ん": "ん", "んー": "んー", "あー": "あー",
    "いってー": "いってー|痛ってー", "えー": "えー", "あーん": "あーん", "えーと": "えーと", "うんうん": "うんうん",
    "ぐえー": "ぐえー", "なんでや": "なんでや|何でや", "こわ": "こわ|怖", "そっか": "そっか", "やば": "やば|ヤバ",
    "なんか": "なんか", "わ": "わ", "うっ": "うっ", "あっ": "あっ", "くっ": "くっ", "まあ": "まあ", "わあ": "わあ", "ふう": "ふう",
    "はあ": "はあ", "あーね": "あーね",
}
words = {}  # 読み -> (よい形, 出どころ, 口語)
for l in open(src, encoding="utf-8"):
    if l.startswith("#"):
        continue
    r, kebs, col, seq = (l.rstrip("\n").split("\t") + [""] * 4)[:4]
    if HIRA.match(r) and len(r) <= 8:
        words.setdefault(r, (r, "JMdict", col))
for r, forms in ADDED.items():
    words[r] = (forms, "開発者", words.get(r, ("", "", ""))[2])
SMALL = dict(zip("あいうえお", "ぁぃぅぇぉ"))
VOWEL = {c: v for v, row in zip("あいうえお", ["あかさたなはまやらわがざだばぱゃ", "いきしちにひみりぎじぢびぴ", "うくすつぬふむゆるぐずづぶぷゅ", "えけせてねへめれげぜでべぺ", "おこそとのほもよろをごぞどぼぽょ"]) for c in row}
def variants(r):
    out = set()
    if "ー" in r:
        out.add(r.replace("ー", "〜"))
    # 前の字と同じ母音の あいうえお で終わる語は、小書き (まあ → まぁ) と ー (まあ → まー)
    if len(r) >= 2 and r[-1] in SMALL and VOWEL.get(r[-2]) == r[-1]:
        out |= {r[:-1] + SMALL[r[-1]], r[:-1] + "ー"}
    return out
for r, (forms, how, col) in list(words.items()):
    for v in variants(r):
        if v not in words:
            words[v] = ("|".join(f.replace(r, v) if f == r else f.replace("ー", "〜") if "〜" in v else f for f in forms.split("|")), how + " の形", col)
casual = {}  # 読み -> [よい形 (先が 1 番目)]
if casual_path:
    for l in open(casual_path, encoding="utf-8"):
        if l.startswith("#") or not l.strip():
            continue
        r, forms = l.rstrip("\n").split("\t")[:2]
        casual[r] = forms.split("|")
have = set()
for l in open(azd, encoding="utf-8"):
    p = l.split("\t")
    if len(p) >= 2:
        have.add((p[0], p[1]))
added = 0
with open(out_path, "w", encoding="utf-8", newline="\n") as f:
    f.write("# 表記\t読み\tcid\tmid\t値\t出典 (感動詞 3。azooKey の辞書に同じ読み・表記の無いもの)\n")
    for r in sorted(words):
        forms, how, col = words[r]
        for w in forms.split("|"):
            # 〜 は辞書の読みの字に無いので、〜 の形は変換サーバーの表だけ
            if (kata(r), w) in have or len(r) < 2 or "〜" in r:
                continue
            f.write(f"{w}\t{kata(r)}\t3\t501\t-14.0\t{'JMdict (CC BY-SA 4.0) の感動詞' if how.startswith('JMdict') else 'HaniWord で足した感動詞'}\n")
            added += 1
    # くだけた一言: 1 番目の形を -14.0、2 番目からは -14.5 (候補の一覧で 2 番目に)
    casual_added = 0
    for r in sorted(casual):
        for k, w in enumerate(casual[r]):
            if (kata(r), w) in have or (r in words and w in words[r][0].split("|")) or "〜" in r:
                continue
            f.write(f"{w}\t{kata(r)}\t3\t501\t{-14.0 if k == 0 else -14.5}\tくだけた一言 (RealPersonaChat・ウィクショナリー (CC BY-SA 4.0) から HaniWord の開発者が選んだ)\n")
            added += 1
            casual_added += 1
print("くだけた一言", len(casual), "行", casual_added if casual else 0)
print("感動詞の読み", len(words), "うち開発者", sum(1 for v in words.values() if v[1].startswith("開発者")), "追加辞書に足す行", added)
