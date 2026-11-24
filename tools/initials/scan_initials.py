"""2026-11-04 C: 仕事 19 (2) の材料。Wikipedia の本文から「英字 1 字＋漢字・カタカナの語」(B級・S級・Nゲージ・Tシャツ・X線・Jリーグ・A型・iモード・eスポーツ) を数える。
大文字と小文字は分けて数える (語ごとの字の形の割合は initials_words.py で出す)。
試験の記事 (記事 ID の 1 の位が 0。A の試験の組 wiki_initial などはここから作る) は外す。
英字の前に英字・数字が付く所 (DNA・3D・CDプレーヤー) は入れない。全角の英字 (Ｔシャツ) は半角にそろえる。
漢字・カタカナの続き (12 字まで) をそのまま数え (B級グルメ)、語に切るのは数えたあとに異なる続きだけ (initials_words.py)。
  nice -n 19 python3 -I scan_initials.py <出力 runs.tsv> <Wikipedia の文 (ID<TAB>文) ...>
出力: 英字 (大文字・小文字のまま)<TAB>続き<TAB>回数<TAB>記事の数"""
import collections, re, sys
out, *paths = sys.argv[1:]
PAT = re.compile(r"(?<![A-Za-zＡ-Ｚａ-ｚ0-9０-９])([A-Za-zＡ-Ｚａ-ｚ])([一-鿿々〆ヶァ-ヺー]{1,12})")
cnt = collections.Counter(); arts = collections.defaultdict(set)
lines = skipped = 0
for path in paths:
    for line in open(path, encoding="utf-8", errors="replace"):
        aid, _, s = line.partition("\t")
        lines += 1
        if not aid.isdigit() or int(aid) % 10 == 0:
            skipped += 1
            continue
        for m in PAT.finditer(s):
            c = m.group(1)
            if "Ａ" <= c <= "Ｚ" or "ａ" <= c <= "ｚ":
                c = chr(ord(c) - 0xFEE0)
            k = (c, m.group(2))
            cnt[k] += 1
            if len(arts[k]) < 1000:
                arts[k].add(aid)
    print(path, lines, "試験の記事で外した文", skipped, "続き", len(cnt), file=sys.stderr, flush=True)
with open(out, "w", encoding="utf-8", newline="\n") as f:
    f.write("# 英字 (大文字・小文字のまま)<TAB>続き (漢字・カタカナ 12 字まで)<TAB>回数<TAB>記事の数 (1000 まで)。試験の記事 (記事 ID の 1 の位が 0) を外した Wikipedia 日本語版の本文\n")
    for (c, run), v in sorted(cnt.items(), key=lambda kv: -kv[1]):
        f.write(f"{c}\t{run}\t{v}\t{len(arts[(c, run)])}\n")
print("文", lines, "外した文", skipped, "続き", len(cnt))
