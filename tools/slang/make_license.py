"""2026-11-07 C: 仕事 43。ネットスラングの辞書のライセンス文 (slang_dictionaries/LICENSE.txt) を、辞書に入れた語の記事の版の一覧つきで書く。
使い方: python3 -I make_license.py <internet.tsv> <table.tsv (語・読み・意味・版・版の日時・カテゴリ・note)> <out LICENSE.txt>"""
import re, sys
dic_p, table_p, out_p = sys.argv[1:4]
rows = [l.rstrip("\n").split("\t") for l in open(dic_p, encoding="utf-8") if not l.startswith("#")]
words = []  # (記事の題, 版)。出典の列の「「題」(版 N)」を全部拾う (流行語は 新語・流行語大賞 の記事と語の記事の 2 つ)
for r in rows:
    for t, o in re.findall(r"「(.+?)」\(版 (\d+)\)", r[5]):
        if (t, o) not in words: words.append((t, o))
table = {r[0]: r for r in (l.rstrip("\n").split("\t") for l in open(table_p, encoding="utf-8") if not l.startswith("#"))}
added = sorted({r[0] for r in rows if r[5].startswith("HaniWord の開発者が足した語")})
head = """HaniWord のくだけた言葉の辞書 (slang_dictionaries/*.tsv と、そこから作る domain/domain_slang_*.louds ほか)

設定の「ネットスラング」をオンにした人だけが使う辞書です (初期値はオフ)。ネットで使われる言葉 (親ガチャ・激おこぷんぷん丸・ミャクミャク など) を変換に足します。
HaniWord のプログラム (MIT License) とは別に、クリエイティブ・コモンズ 表示-継承 4.0 国際 (CC BY-SA 4.0) で配布します。
https://creativecommons.org/licenses/by-sa/4.0/deed.ja

■ 語の出典 (日本語版ウィキペディア、CC BY-SA 4.0。2026-11-07 に MediaWiki の API で取得した版)
次の 3 つから語を選び、冒頭の括弧の読みを付けました (編集・加工)。
- 「Category:日本のインターネットスラング」とその下位のカテゴリの記事の見出しの語
- 「新語・流行語大賞」の記事の受賞語の表 (候補の語を出すのに使い、入れるかどうかは下の数で決めました)
- 日本語版ウィキペディアの月ごとの閲覧数の上位 1,000 記事 (2015-07〜2026-10) のうち、カテゴリがインターネット・ミーム・流行語・ネットスラング・俗語の記事の見出しの語
読みが冒頭に無いリダイレクトの記事は、並べ替えの鍵 (DEFAULTSORT) と合う読みを付けました。
各記事の著者は、記事の履歴 (https://ja.wikipedia.org/w/index.php?title=<記事名>&action=history) にあります。辞書に入れた語の記事と版:
"""
body = "".join(f"- 「{w}」 https://ja.wikipedia.org/w/index.php?oldid={o}" + (f" ({table[w][4][:10]} の版)" if w in table and table[w][4] else "") + "\n" for w, o in words)
tail = (f"\n■ HaniWord の開発者が足した語\n{'・'.join(added)} は、HaniWord の開発者が足しました (ウィキペディアの記事からではありません)。\n" if added else "") + """
■ 語の選び方 (辞書には入っていない材料)
まれな語を外すのに、記事ごとの直近 12 か月の閲覧数 (Wikimedia の pageviews API) と、日本語版ウィキペディアの全文
(Hugging Face の wikimedia/wikipedia 20231101.ja。2023-11-01 のダンプで、元は CC BY-SA 3.0 / GFDL) で語を含む文の数を数えました。数えた結果だけを使い、文は入っていません。

■ 品詞の番号・変換の重み
品詞・意味の番号と変換の重みは、azooKey の辞書 (Apache License 2.0) の語にならって HaniWord の開発者が付けました。
〜る で終わる動詞の活用形は、azooKey の辞書の ググる の活用形にならいました。

■ 作り方
tools/slang/README.md にあります。
"""
with open(out_p, "w", encoding="utf-8", newline="\n") as f:
    f.write(head + body + tail)
print(len(words), "記事")
