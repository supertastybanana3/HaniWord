"""2026-11-07 (仕事 40): 集めた行 (collect/a1_wikipedia・a2_media・a4_lists・a5_public の jsonl) を、決めた分け方で分ける。
- (a) 差別: 出どころが「差別語・侮辱の語・蔑称」として書いている行 (担当の段 a) のある語
- (b) 言い換え: 出どころが「放送・新聞などで言い換えられる・控えられる語」として書いている行 (段 b) のある語。ただし出どころが
  放送・新聞・出版の記事 (a2) かウィキペディア (a1、放送禁止用語・差別用語の記事のマスメディアの扱いの文) のときだけ。公的機関の手引き (a5) の言い換えだけの語は入れない
- 迷う語の表 (人が決める): 段 ? だけの語、ふつうの語と同じ形の印 (【一般語と同形】) のある語、広告・商標の言い換え、a5・a4 の言い換えだけの語
使い方: python3 merge.py <out_dir> collect/*.jsonl  → out_dir/decided.jsonl (語ごと: 段・行) と out_dir/review.tsv
"""
import json, os, sys
from collections import OrderedDict

BROADCAST_SOURCES = ("a1_", "a2_")
PERMISSIVE = ("MIT", "Apache-2.0", "BSD", "CC0", "CC BY 4.0")
SUSPECT = ("MoyuHero", "warui")


def main():
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    words = OrderedDict()
    for path in sys.argv[2:]:
        tag = os.path.basename(path)[:3]
        for line in open(path, encoding="utf-8"):
            if not line.strip():
                continue
            r = json.loads(line)
            r["_src"] = tag
            # NG 語の一覧 (a4): 許しの広いライセンスの一覧の、差別・侮辱の分類の行 (段 a) だけ。GPL・AGPL・NC の一覧、
            # monoroch の一覧と重なる出どころ不明の一覧 (MoyuHero・warui)、分類の無い行 (段 ?)・ほかの不適切語 (段 b) は使わない
            if tag == "a4_" and not (r.get("tier") == "a" and (r.get("note") or "").startswith(PERMISSIVE)
                                     and not any(x in (r.get("page_title") or "") + (r.get("url") or "") for x in SUSPECT)):
                continue
            # 攻撃の語と差別の語が混ざった分類 (attack: 攻撃・侮蔑、攻撃的/差別的な表現。殺す など) は、それだけでは段 a にしない
            if tag == "a4_" and "攻撃" in (r.get("category") or ""):
                r["tier"] = "?"
            words.setdefault(r["word"].strip(), []).append(r)
    decided, review = [], []
    for word, rows in words.items():
        reading = "|".join(sorted({r.get("reading") or "" for r in rows} - {""}))
        reasons = []
        common = any("一般語と同形" in (r.get("note") or "") or "一般語と同じ" in (r.get("note") or "") for r in rows)
        trademark = any("商標" in (r.get("category") or "") + (r.get("note") or "") or "広告" in (r.get("category") or "") for r in rows)
        # 集める担当が「要判断」と書いた行 (日常の語でもある・過去の言い方として書かれている) は、それだけでは段 a にしない
        a_rows = [r for r in rows if r.get("tier") == "a" and "要判断" not in (r.get("note") or "")]
        if not a_rows and any("要判断" in (r.get("note") or "") for r in rows):
            reasons.append("集める担当が要判断とした")
        b_rows = [r for r in rows if r.get("tier") == "b" and r["_src"].startswith(BROADCAST_SOURCES)]
        if common:
            reasons.append("ふつうの語と同じ形")
        if trademark:
            reasons.append("広告・商標の言い換え")
        if reasons or (not a_rows and not b_rows):
            if not a_rows and not b_rows and not reasons:
                tiers = sorted({r.get("tier", "?") for r in rows})
                reasons.append("段が ? だけ" if tiers == ["?"] else f"段 {'・'.join(tiers)} (放送・新聞の出どころでない言い換え)")
            review.append((word, reading, "|".join(sorted({r.get('tier', '?') for r in rows})), str(len({r['url'] for r in rows})),
                           "・".join(reasons), " / ".join(sorted({r['_src'] + ':' + (r.get('site') or '') for r in rows}))[:200]))
            # 2026-11-07 : 迷う語も捨てずに (b) に入れる。注記の文は出どころの種類に合わせる (note_type)
            #   common: ふつうの語と同じ形 (ゴミ・クズ) → 決めつけない文 / trademark: 商標の言い換え / broadcast: 放送・新聞の記事・ウィキペディアの段 b /
            #   ng_list: NG 語の一覧 / other: 揶揄・意見が分かれる・担当が要判断とした語
            srcs = {r["_src"] for r in rows}
            # ふつうの語と同じ形の語は、出どころの分類で 2 つに分ける : 差別の分類 (差別・地域・障害・部落・民族・ethnic・slurs) が
            # あれば other、無くて人への悪口の分類 (侮蔑・悪口・卑罵・insult・軽蔑・侮辱・動物に例える・人称) があれば common、どちらも無ければ other
            # 攻撃と差別をまとめた分類名 (攻撃的/差別的な表現) は、差別の分類として数えない
            cats = " ".join((r.get("category") or "") for r in rows).replace("攻撃的/差別的な表現", "攻撃的な表現")
            person = common and not any(k in cats for k in ("差別", "地域", "障害", "部落", "民族", "ethnic", "slurs")) \
                and any(k in cats for k in ("侮蔑", "悪口", "卑罵", "insult", "軽蔑", "侮辱", "動物に例える", "人称"))
            note_type = ("common" if person else "other" if common else "trademark" if trademark else "broadcast" if b_rows
                         else "ng_list" if "a4_" in srcs else "other")
            decided.append({"word": word, "reading": reading, "kind": "b", "note_type": note_type, "rows": rows, "from_review": True})
            continue
        kind = "a" if a_rows else "b"
        kinds = {r.get("note_kind") for r in b_rows if r.get("note_kind")}
        decided.append({"word": word, "reading": reading, "kind": kind, "note_type": "slur" if kind == "a" else (kinds.pop() if kinds else "broadcast"),
                        "rows": a_rows if kind == "a" else b_rows})
    with open(os.path.join(out, "decided.jsonl"), "w", encoding="utf-8") as f:
        for d in decided:
            f.write(json.dumps(d, ensure_ascii=False) + "\n")
    with open(os.path.join(out, "review.tsv"), "w", encoding="utf-8") as f:
        f.write("# 迷う語 (人が決める): 語\t読み\t段 (担当の目安)\t出どころの数\t迷う理由\t出どころ\n")
        for r in review:
            f.write("\t".join(r) + "\n")
    na = sum(d["kind"] == "a" for d in decided)
    import collections
    print(f"語 {len(words)}: (a) {na}・(b) {len(decided) - na} (うち迷う語から {len(review)})", dict(collections.Counter(d["note_type"] for d in decided)))


main()
