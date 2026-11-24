"""2026-11-08 C: 仕事 44 の確かめ (3b) で、固有名詞の表の語が正しかった所を替えた文を探す (まとめ役 11-08 01:0x の決め: 正しかった 1 番目を替えた名前は外す。
変わった文の中で、その所が正しかったのに替わった名前も外す)。
文ごとに、前 (表なし) と後 (表あり) の変換 (AI あり full・辞書だけ dict) を正解と並べ、後にだけ出た表の語を見る:
  - 崩れた: 前が正解と同じで、後が違う文に出た表の語のうち、正解に無いもの
  - 所が崩れた: 後にだけ出た表の語 (正解に無いもの) の置き場で、前は正解と合っていた字が後で合わなくなったか、正しかった所に字が足されたもの
  - 良: 後にだけ出た表の語で、正解にあるもの
出力: 語ごとの数と例 (<out.tsv>)。標準出力に、外す語 (崩れた・所が崩れた が 1 以上) の数
使い方: python3 -I names_culprits.py <core_names.tsv> <out.tsv> <前.jsonl> <後.jsonl> [<前.jsonl> <後.jsonl> ...]"""
import json, sys
from collections import defaultdict
from difflib import SequenceMatcher
table, out = sys.argv[1:3]
pairs = list(zip(sys.argv[3::2], sys.argv[4::2]))
by_first = defaultdict(set)
for l in open(table, encoding="utf-8"):
    c = l.rstrip("\n").split("\t")
    if len(c) > 1 and not c[0].startswith("#"): by_first[c[0][0]].add(c[0])
def found(text):  # text に出る表の語 (置き場つき)
    hits = []
    for i, ch in enumerate(text):
        for w in by_first.get(ch, ()):
            if text.startswith(w, i): hits.append((i, w))
    return hits
def matched(ref, text):  # 正解の字のうち、text と合っている字の位置
    m = set()
    for a, b, n in SequenceMatcher(None, ref, text, autojunk=False).get_matching_blocks(): m.update(range(a, a + n))
    return m
stat = defaultdict(lambda: {"崩れた": 0, "所が崩れた": 0, "良": 0, "例": []})
for before, after in pairs:
    name = after.rsplit("/", 1)[-1]
    B = [json.loads(l) for l in open(before, encoding="utf-8")]
    A = [json.loads(l) for l in open(after, encoding="utf-8")]
    for x, y in zip(B, A):
        ref = y["expected"]
        for key in ("full", "dict"):
            old, new = x.get(key, ""), y.get(key, "")
            if old == new: continue
            old_words = {w for _, w in found(old)}
            new_hits = [(i, w) for i, w in found(new) if w not in old_words]
            if not new_hits: continue
            broke = old == ref and new != ref
            lost = matched(ref, old) - matched(ref, new)
            ops = SequenceMatcher(None, ref, new, autojunk=False).get_opcodes()
            for i, w in new_hits:
                s = stat[w]
                if w in ref: s["良"] += 1; continue
                # w の置き場に当たる正解の字の範囲
                span, inserted = set(), False
                old_ok = matched(ref, old)
                for tag, i1, i2, j1, j2 in ops:
                    if tag != "equal" and j1 < i + len(w) and i < max(j2, j1 + 1):
                        span.update(range(i1, max(i2, i1 + 1)))
                        # 字が足されただけ (将軍 → 将軍SHŌGUN): 前は足された所の両どなりの正解の字が合っていた
                        if tag == "insert" and (i1 - 1 in old_ok or i1 == 0) and (i1 in old_ok or i1 == len(ref)): inserted = True
                if broke: s["崩れた"] += 1
                elif span & lost or inserted: s["所が崩れた"] += 1
                else: continue
                if len(s["例"]) < 3: s["例"].append(f"{name}:{key}: {old} → {new}")
bad = {w: s for w, s in stat.items() if s["崩れた"] or s["所が崩れた"]}
with open(out, "w", encoding="utf-8") as f:
    f.write("# 語\t崩れた\t所が崩れた\t良\t例\n")
    for w, s in sorted(stat.items(), key=lambda kv: (-(kv[1]["崩れた"] + kv[1]["所が崩れた"]), -kv[1]["良"])):
        f.write(f"{w}\t{s['崩れた']}\t{s['所が崩れた']}\t{s['良']}\t{' | '.join(s['例'])}\n")
print(f"外す語 {len(bad)} (崩れた {sum(1 for s in bad.values() if s['崩れた'])}・所が崩れた だけ {sum(1 for s in bad.values() if not s['崩れた'])})、"
      f"良だけの語 {sum(1 for s in stat.values() if s['良'] and not (s['崩れた'] or s['所が崩れた']))}")
for w, s in list(bad.items())[:40]: print(" ", w, s["崩れた"], s["所が崩れた"], s["良"])
