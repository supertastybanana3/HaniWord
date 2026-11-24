"""e-Gov 法令検索の全法令 XML (一括ダウンロード all_xml.zip を展開したもの) から、法律の分野辞書の語の候補を集める。

出典: e-Gov 法令検索 (https://laws.e-gov.go.jp/) の法令データ。利用規約は公共データ利用規約 (第 1.0 版、PDL1.0)。
法令ごとに、今日の時点で施行されている版 (なければいちばん古い版) を 1 つ選ぶ。

候補の出どころ (src 列):
  title   法令名 (LawTitle)。読み (Kana 属性) つき
  abbrev  法令の略称 (Abbrev)。AbbrevKana は先頭の 1 字だけなので読みには使わない
  ruby    本文のふりがな (Ruby/Rt)。読みつき
  caption 条・項の見出し (ArticleCaption・ParagraphCaption の括弧の中)
  defined 定義された語 (「…」という。)
  text1   本文 (Sentence) の 1 語 (Sudachi の長い単位)
  text    本文の名詞の連なり (2〜3 語、common.chains)。片仮名書きの古い法令の本文は使わない
出力 (TSV): 表記 / 法令数 (その語が出る法令の数) / 回数 / 出どころ / e-Gov の読み (あれば) / 例の法令名
使い方: python collect_egov.py <all_xml のフォルダ> <出力.tsv> [--date 20261020] [--jobs 8]
"""
import argparse, collections, glob, multiprocessing, os, re, sys
import xml.etree.ElementTree as ET
from collections import Counter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common


def choose_versions(root, date):
    by_law = collections.defaultdict(list)
    for d in os.listdir(root):
        parts = d.split("_")
        if len(parts) >= 2 and os.path.isdir(os.path.join(root, d)):
            by_law[parts[0]].append((parts[1], d))
    chosen = []
    for law_id, versions in by_law.items():
        versions.sort()
        current = [v for v in versions if v[0] <= date]
        chosen.append(os.path.join(root, (current[-1] if current else versions[0])[1]))
    return sorted(chosen)


def inner_text(e):
    # ふりがな (Rt) は本文に入れない
    out = [e.text or ""]
    for c in e:
        if c.tag != "Rt":
            out.append(inner_text(c))
        out.append(c.tail or "")
    return "".join(out)


def one_law(folder):
    paths = glob.glob(os.path.join(folder, "*.xml"))
    if not paths:
        return None
    try:
        root = ET.parse(paths[0]).getroot()
    except ET.ParseError:
        return None
    title_e = root.find(".//LawTitle")
    title = inner_text(title_e).strip() if title_e is not None else ""
    found = collections.Counter()   # (表記, 出どころ, 読み) -> 回数
    if title_e is not None:
        # 番号で呼ぶ古い法令 (明治十年太政官達第九十七号（…ノ件）) は名前にしない
        if not re.search(r"第[一二三四五六七八九十百千〇]+号", title):
            found[(title, "title", title_e.get("Kana", ""))] += 1
        # AbbrevKana は略称の読みではなく先頭の 1 字 (索引用) なので使わない
        for a in re.split(r"[,、，]", title_e.get("Abbrev", "") or ""):
            if a and not re.search(r"第[一二三四五六七八九十百千〇]+号", a):
                found[(a, "abbrev", "")] += 1
    for ruby in root.iter("Ruby"):
        rt = "".join(inner_text(r) for r in ruby.findall("Rt"))
        found[((ruby.text or "").strip(), "ruby", rt)] += 1
    for tag in ("ArticleCaption", "ParagraphCaption"):
        for e in root.iter(tag):
            cap = inner_text(e).strip().strip("（）()")
            for piece in re.split(r"・|及び|並びに|又は|若しくは|等", cap) if len(cap) > 12 else [cap]:
                if 2 <= len(piece) <= 16 and common.KANJI.search(piece) and common.plausible(piece):
                    found[(piece, "caption", "")] += 1
    texts = [inner_text(s) for s in root.iter("Sentence")]
    body = "\n".join(texts)
    if common.old_style(body):  # 片仮名書きの古い法令は本文を使わない (法令名・ふりがなだけ)
        return title, Counter({k: v for k, v in found.items() if k[1] in ("title", "abbrev", "ruby")})
    for m in re.finditer(r"「([^「」]{2,16})」という", body):
        if common.KANJI.search(m.group(1)) and not re.search(r"[0-9０-９、。]", m.group(1)) and common.plausible(m.group(1)):
            found[(m.group(1), "defined", "")] += 1
    for surface, _reading, n in common.chains(body):
        found[(surface, "text1" if n == 1 else "text", "")] += 1
    return title, found


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("root")
    ap.add_argument("out")
    ap.add_argument("--date", default="20261020")
    ap.add_argument("--jobs", type=int, default=8)
    a = ap.parse_args()
    folders = choose_versions(a.root, a.date)
    print("laws", len(folders), file=sys.stderr)
    laws = collections.Counter()          # 表記 -> 法令の数
    count = collections.Counter()         # 表記 -> 回数
    srcs = collections.defaultdict(set)
    readings = collections.defaultdict(collections.Counter)
    example = {}
    with multiprocessing.Pool(a.jobs) as pool:
        for k, res in enumerate(pool.imap_unordered(one_law, folders, chunksize=8)):
            if k % 500 == 0:
                print(k, file=sys.stderr, flush=True)
            if res is None:
                continue
            title, found = res
            seen = set()
            for (surface, src, reading), n in found.items():
                if not surface or len(surface) > 40:
                    continue
                count[surface] += n
                srcs[surface].add(src)
                r = common.clean_reading(reading) if reading else None
                if r:
                    readings[surface][r] += n
                if surface not in seen:
                    laws[surface] += 1
                    seen.add(surface)
                    example.setdefault(surface, title)
    with open(a.out, "w", encoding="utf-8") as out:
        out.write("# 表記\t法令数\t回数\t出どころ\te-Govの読み\t例の法令\n")
        for surface, n in sorted(laws.items(), key=lambda kv: (-kv[1], -count[kv[0]], kv[0])):
            r = readings[surface].most_common(1)[0][0] if readings[surface] else ""
            out.write(f"{surface}\t{n}\t{count[surface]}\t{','.join(sorted(srcs[surface]))}\t{r}\t{example[surface]}\n")
    print("terms", len(laws), file=sys.stderr)


if __name__ == "__main__":
    main()
