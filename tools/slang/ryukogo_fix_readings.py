"""2026-11-07 C: 仕事 43 (流行語)。words.tsv の読みを決め直す (閲覧数は取り直さない):
- 記事の冒頭の読みは、冒頭が語で始まり、そのすぐあとの括弧の頭のかなだけ (記事の途中の「失楽園（する）」のような括弧は使わない)
- かなだけの語は表記のかなを読みに (括弧の読みが別の書き方のとき: マルサ（まるた）)
- ほかは前と同じ (受賞の表の括弧・DEFAULTSORT と合う Sudachi の読み)
使い方: python3 -I fix_readings.py <words.tsv> <articles.jsonl> <out.tsv>"""
import json, re, sys
wd, ar, out = sys.argv[1:4]
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
arts = {json.loads(l)["word"]: json.loads(l) for l in open(ar, encoding="utf-8")}
lines = open(wd, encoding="utf-8").read().splitlines()
with open(out, "w", encoding="utf-8") as f:
    f.write(lines[0] + "\n")
    for l in lines[1:]:
        c = l.split("\t"); w = c[0]
        # 順: かなだけの語は表記のかな → 記事の冒頭の括弧 (冒頭の語のすぐあと) → 受賞の表の括弧 (語の字の数以上のかなのときだけ。失楽園（する） は読みでない) → 前の読み (DEFAULTSORT)
        intro = arts.get(w, {}).get("intro", "")
        m = re.search(r"[「『]?" + re.escape(w) + r"[」』]?\s*[（(]([^）)]{1,40})[）)]", intro[:len(w) + 30]) if intro else None
        first = re.split(r"[、,，/／]", m.group(1))[0].strip().replace("・", "").replace(" ", "") if m else ""
        if re.fullmatch(r"[ぁ-ゖァ-ヺー・〜]+", w):
            c[1], c[2] = hira(re.sub(r"[・〜]", "", w)), "かなだけの語"
        elif re.fullmatch(r"[ぁ-ゖァ-ヺー]+", first):
            c[1], c[2] = hira(first), "記事の冒頭の括弧"
        elif c[2] == "受賞の表の括弧" and len(c[1]) < len(w):
            c[1], c[2] = "", ""
        elif c[2] == "記事の冒頭の括弧":
            c[1], c[2] = "", ""
        f.write("\t".join(c) + "\n")
