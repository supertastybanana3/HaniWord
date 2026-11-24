"""2026-11-07 (仕事 40): merge.py の decided.jsonl のうち、迷う語から (b) に移した語と出どころが 1 つの (b) の語を、根拠の文の分け
((i) 今の差別・侮辱・言い換え・弾きの語と書いている (ii) 昔の使い方だけ (iii) 別の意味・別の話の例 (iv) 一覧に語があるだけ) で絞る。
分けは 2 つのモデルに別々にさせ (classify/out_*.jsonl と、決まりを足して分け直した out_*_v2.jsonl)、2 つとも (i) の語だけを残す。
(ii)〜(iv) と、2 つが割れた語は入れない (excluded_classify.tsv に控える。配らない)。
注記の文の種類: 悪口の分類の例として挙がった語 → common (人に向けると侮辱になることがあります)、色の名前の フレッシュ → color、
一覧の名前だけが分類を示す NG 語の一覧 (攻撃的/差別的な表現・差別キーワード) の語 → other (使い方によっては侮辱や差別と受け取られることがある語です)
使い方: python3 finalize.py <decided.jsonl> <classify のフォルダ> <out_final.jsonl> <excluded_classify.tsv>
"""
import json, os, sys

dec_path, cdir, out, excl_out = sys.argv[1:5]
L = lambda p: {json.loads(l)["word"]: json.loads(l) for l in open(p, encoding="utf-8")}
S0, O0 = L(os.path.join(cdir, "out_sonnet.jsonl")), L(os.path.join(cdir, "out_opus.jsonl"))
S1 = L(os.path.join(cdir, "out_sonnet_v2.jsonl")) if os.path.exists(os.path.join(cdir, "out_sonnet_v2.jsonl")) else {}
O1 = L(os.path.join(cdir, "out_opus_v2.jsonl")) if os.path.exists(os.path.join(cdir, "out_opus_v2.jsonl")) else {}
classified = set(S0)
final, excluded = [], []
for line in open(dec_path, encoding="utf-8"):
    d = json.loads(line)
    w = d["word"]
    if w in classified:
        s, o = (S1.get(w) or S0[w]), (O1.get(w) or O0[w])
        if not (s["class"] == o["class"] == "i"):
            why = f"2 つとも ({s['class']})" if s["class"] == o["class"] else f"割れた (({s['class']})・({o['class']}))"
            excluded.append((w, why, s.get("reason", ""), o.get("reason", ""), d["rows"]))
            continue
        cats = " ".join(r.get("category") or "" for r in d["rows"])
        if d.get("from_review"):
            if any(k in cats for k in ("筒井康隆", "星野命", "比喩による侮蔑", "動物に例える")):
                d["note_type"] = "common"
            elif w == "フレッシュ":
                d["note_type"] = "color"
            elif d["note_type"] == "ng_list" and all(("Offensive.txt" in (r.get("category") or "") or "discrimination_keyw" in (r.get("category") or ""))
                                                    for r in d["rows"] if r.get("_src") == "a4_"):
                d["note_type"] = "other"
    final.append(d)
with open(out, "w", encoding="utf-8") as f:
    for d in final:
        f.write(json.dumps(d, ensure_ascii=False) + "\n")
with open(excl_out, "w", encoding="utf-8") as f:
    f.write("# 入れない語: 語\t分け\t1 つめのモデルの理由\t2 つめのモデルの理由\t根拠の行\n")
    for w, why, rs, ro, rows in excluded:
        ev = " | ".join(f"{r.get('site', '')}「{r.get('page_title', '')}」: {(r.get('quote') or '')[:120]}" for r in rows)
        f.write(f"{w}\t{why}\t{rs}\t{ro}\t{ev}\n")
print(len(final), "words", len(excluded), "excluded")
