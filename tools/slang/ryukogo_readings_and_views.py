"""2026-11-07 C: 仕事 43 (流行語)。受賞語ごとに読みと閲覧数を決める。
- 読み: 記事なら冒頭の「語（よみ）」のかなの括弧、表の語のすぐあとのかなの括弧 (関空（かんくう）)、かなだけの語は表記のかな、
  転送なら DEFAULTSORT と合う読み (Sudachi の読みで濁点を戻す、tools/slang/fill_yomi.py と同じ決まり)。どれも無ければ空
- 閲覧数: 記事 (転送でない) だけ。Wikimedia の pageviews (人の閲覧・全端末・月ごと) を 2015-07〜2026-10 で取り、
  直近 12 か月 (2025-11〜2026-10) の月の平均と、記録のある範囲でいちばん多かった月 (年月と数) を出す。1 つずつ 2 秒おき、429 なら 60 秒待つ
出力 TSV: 語<TAB>読み<TAB>読みの出どころ<TAB>記事 (記事・転送・無し)<TAB>記事の題<TAB>版<TAB>12 か月の月の平均<TAB>いちばん多かった月<TAB>その月の数
使い方: python -I readings_and_views.py <awards.tsv> <articles.jsonl> <out.tsv>"""
import json, re, sys, time, unicodedata, urllib.error, urllib.parse, urllib.request
from sudachipy import dictionary, tokenizer
awards, arts, out = sys.argv[1:4]
UA = "HaniWord-dict-tools/0.1 (https://github.com/supertastybanana3)"
BASE = "https://wikimedia.org/api/rest_v1/metrics/pageviews/per-article/ja.wikipedia/all-access/user/{}/monthly/2015070100/2026103100"
def hira(s): return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)
SMALL = str.maketrans("ぁぃぅぇぉっゃゅょゎ", "あいうえおつやゆよわ")
def strip(s):
    s = unicodedata.normalize("NFD", hira(s)); s = "".join(c for c in s if c not in "゙゚")
    return unicodedata.normalize("NFC", s).translate(SMALL).replace("ー", "")
tok = dictionary.Dictionary(dict="full").create(); MODE = tokenizer.Tokenizer.SplitMode.C
table_yomi = {}
for l in open(awards, encoding="utf-8"):
    if l.startswith("#"): continue
    c = l.rstrip("\n").split("\t")
    if c[4] and re.fullmatch(r"[ぁ-ゖァ-ヺー]+", c[4].replace("・", "")): table_yomi[c[2]] = hira(c[4].replace("・", ""))
def views(title):
    url = BASE.format(urllib.parse.quote(title.replace(" ", "_"), safe=""))
    for _ in range(3):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": UA}), timeout=60) as r:
                items = json.loads(r.read().decode("utf-8")).get("items", [])
            time.sleep(2); return [(i["timestamp"][:6], i["views"]) for i in items]
        except urllib.error.HTTPError as e:
            time.sleep(2)
            if e.code == 404: return []
            if e.code == 429: time.sleep(60); continue
            raise
    return None
rows = [json.loads(l) for l in open(arts, encoding="utf-8")]
with open(out, "w", encoding="utf-8") as f:
    f.write("# 語\t読み\t読みの出どころ\t記事\t記事の題\t版\t直近 12 か月の月の平均\tいちばん多かった月\tその月の閲覧\n")
    for r in rows:
        w = r["word"]; kind = "記事" if r["exists"] and "redirect_to" not in r else "転送" if r["exists"] else "無し"
        y, src = "", ""
        if kind == "記事":
            m = re.search(re.escape(w) + r"\s*[（(]([^）)]{1,40})[）)]", r.get("intro", ""))
            if m:
                cand = re.split(r"[、,，/／]", m.group(1))[0].strip().replace("・", "")
                if re.fullmatch(r"[ぁ-ゖァ-ヺー]+", cand): y, src = hira(cand), "記事の冒頭の括弧"
        if not y and w in table_yomi: y, src = table_yomi[w], "受賞の表の括弧"
        if not y and re.fullmatch(r"[ぁ-ゖァ-ヺー・]+", w): y, src = hira(w.replace("・", "")), "かなだけの語"
        if not y and kind == "転送" and r.get("defaultsort"):
            sud = hira("".join(t.reading_form() for t in tok.tokenize(w, MODE)))
            if re.fullmatch(r"[ぁ-ゖー]+", sud) and strip(sud) == strip(r["defaultsort"]): y, src = sud, "Sudachi の読み (DEFAULTSORT と合う)"
        avg = peak = peakv = ""
        if kind == "記事":
            v = views(r["title"])
            if v:
                last = [n for ym, n in v if "202511" <= ym <= "202610"]
                avg = str(round(sum(last) / len(last))) if last else "0"
                pm = max(v, key=lambda x: x[1]); peak, peakv = pm[0][:4] + "-" + pm[0][4:], str(pm[1])
        f.write("\t".join(map(str, [w, y, src, kind, r.get("title", ""), r.get("revid", "") or "", avg, peak, peakv])) + "\n")
        f.flush()
print("done", time.strftime("%Y-%m-%d %H:%M:%S %z"))
