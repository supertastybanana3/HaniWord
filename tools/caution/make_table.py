"""2026-11-06 (仕事 40): 集めた語 (refs.py の words.tsv: 語・読み・段・文献の番号) と言い換えの例 (collect/*.jsonl の note の「言い換え: 」) から、
misuse_notes.tsv に足す行 (種類<TAB>含む<TAB>鍵<TAB>注記<TAB>除く語) を作る。
- 種類: 段 a → 差別、b → 言い換え。a と b の両方の出どころがある語は a。? だけの語は入れない (表に印を付けて人が決める)
- 引き方: ふつうは「含む」と除く語 (azooKey の辞書で、鍵を含む長い表記のうち値が COMMON 以上のもの。不具 → 不具合)。
  1 字の語と、かなだけの 4 字までの語は「読み」(文節がその語ちょうどのとき)。除く語が 200 字に収まらない語は人が決める
- 注記: 「注意: 差別や侮辱の語とされます [文献の番号]」「注意: 放送などで言い換えられることがある語です (例: 〜) [文献の番号]」。引用の文は入れない
使い方: python3 make_table.py <words.tsv> <azookey_dict.tsv> <out_rows.tsv> <out_review.tsv> collect/*.jsonl
"""
import json, re, sys
from collections import defaultdict

COMMON = -12.0


def main():
    words_tsv, dic, out_rows, out_review = sys.argv[1:5]
    # 2026-11-07: 語ごとの注記の文の種類 (merge.py の decided.jsonl の note_type)。DECIDED の環境変数で渡す
    import os
    note_types = {}
    manual_excl = {}
    if os.environ.get("MANUAL_EXCL"):
        for line in open(os.environ["MANUAL_EXCL"], encoding="utf-8"):
            c = line.rstrip("\n").split("\t")
            if not line.startswith("#") and len(c) == 2:
                manual_excl[c[0]] = c[1].split("|")
    if os.environ.get("DECIDED"):
        for line in open(os.environ["DECIDED"], encoding="utf-8"):
            d = json.loads(line)
            note_types[d["word"]] = d.get("note_type", "")
    alts = defaultdict(list)  # 語 → 言い換えの例
    for path in sys.argv[5:]:
        for line in open(path, encoding="utf-8"):
            if not line.strip():
                continue
            r = json.loads(line)
            m = re.search(r"言い換え[:：]\s*([^。]+)", r.get("note") or "")
            if m:
                for x in re.split(r"[、,・/]", m.group(1)):
                    # 「…」があれば中の語だけ。言い換えの例は短い語 (12 字まで) だけにする (出どころのメモの文をそのまま出さない)
                    q = re.findall(r"「([^」]+)」", x)
                    for y in (q or [x]):
                        y = y.strip("「」 ")
                        if y and len(y) <= 12 and "。" not in y and y not in alts[r["word"]] and y != r["word"]:
                            alts[r["word"]].append(y)
    surfaces = defaultdict(float)
    for line in open(dic, encoding="utf-8"):
        c = line.rstrip("\n").split("\t")
        if len(c) >= 6:
            surfaces[c[1]] = max(surfaces.get(c[1], -99.0), float(c[5]))
    rows, review = [], []
    for line in open(words_tsv, encoding="utf-8"):
        if line.startswith("#") or not line.strip():
            continue
        word, reading, tiers, refs = line.rstrip("\n").split("\t")
        t = set(tiers.split("|"))
        kind = "差別" if "a" in t else "言い換え" if "b" in t else None
        if kind is None:
            review.append((word, reading, tiers, refs, "段が決まらない"))
            continue
        # 短いかなの語 (4 字まで) と 1 字の語は、活用でできる語 (考えた・押した・ばかり) の中にも入るので、「含む」でなく
        # 文節がその語ちょうどのときだけ付く「読み」(読み:表記) で引く。読みが無ければ人が決める
        kana_only = all("ぁ" <= c <= "ゖ" or "ァ" <= c <= "ヺ" or c == "ー" for c in word)
        exact = len(word) == 1 or (kana_only and len(word) <= 4)
        refs_txt = "".join(f"[{n}]" for n in refs.split(",") if n)
        ex = "・".join(alts[word][:3])
        ex_txt = f" (例: {ex})" if ex else ""
        nt = note_types.get(word, "slur" if kind == "差別" else "broadcast")
        if kind == "差別":
            note = f"注意: 差別や侮辱の語とされます {refs_txt}"
        elif nt == "common":
            note = f"注意: 人に向けると侮辱になることがあります {refs_txt}"
        elif nt == "trademark":
            note = f"注意: 放送では商標を避けて言い換えられることがある語です{ex_txt} {refs_txt}"
        elif nt == "ng_list":
            note = f"注意: チャットなどで弾かれることがある語です {refs_txt}"
        elif nt == "color":
            note = f"注意: 色の名前としては言い換えられることがある語です {refs_txt}"
        elif nt == "publishing":
            note = f"注意: 出版などで言い換えられることがある語です{ex_txt} {refs_txt}"
        elif nt == "other":
            note = f"注意: 使い方によっては侮辱や差別と受け取られることがある語です {refs_txt}"
        else:
            note = f"注意: 放送・新聞で言い換えられることがある語です{ex_txt} {refs_txt}"
        if exact:
            readings = [x for x in reading.split("|") if x]
            if not readings:
                review.append((word, reading, tiers, refs, "読みが無い (短い語は読みで引く)"))
                continue
            for rd in readings:
                rows.append((kind, "読み", f"{rd}:{word}", note[:200], ""))
            continue
        excl = sorted(w for w, v in surfaces.items() if len(w) > len(word) and word in w and v >= COMMON)
        # 手で足す除く語 (MANUAL_EXCL の TSV: 鍵<TAB>除く語 | 区切り)。先に入れる (200 字で切られないように)
        manual = [w for w in manual_excl.get(word, []) if word in w and len(w) > len(word)]
        for w in manual:
            surfaces.setdefault(w, 0.0)
            surfaces[w] = max(surfaces[w], 0.0)
        excl = sorted(set(excl) | set(manual))
        # ほかの除く語を含むだけの長い形 (障害児 があれば 知的障害児) は要らない (変換サーバーは「含む」で見る)
        excl = [w for w in excl if not any(o != w and o in w for o in excl)]
        # 1 欄は 200 字まで (変換サーバーの BoundedTable は長すぎる欄の行を捨てる)。よく使う語から入れる
        kept = []
        for w in sorted(excl, key=lambda w: -surfaces[w]):
            if len("|".join(kept + [w])) > 200:
                break
            kept.append(w)
        rows.append((kind, "含む", word, note[:200], "|".join(sorted(kept))))
        if len(kept) < len(excl):
            review.append((word, reading, tiers, refs, f"除く語が {len(excl)} 個 ({len(kept)} 個まで入れた)"))
    with open(out_rows, "w", encoding="utf-8") as f:
        for r in rows:
            f.write("\t".join(r) + "\n")
    with open(out_review, "w", encoding="utf-8") as f:
        f.write("# 人が決める語: 語\t読み\t段\t文献\t理由\n")
        for r in review:
            f.write("\t".join(r) + "\n")
    print(len(rows), "rows", len(review), "to review")


main()
