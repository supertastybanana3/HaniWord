# くだけた言葉の辞書 (slang_dictionaries)

設定の「ネットスラング」(input.internet_slang、初期値はオフ) をオンにした人だけが読む辞書です。今は internet (ネットスラング) だけ。
次の 3 つの出どころの語 (親ガチャ・激おこぷんぷん丸・オールドメディア・ミャクミャク など) を、まれな語を外して足します (2026-11-07 の決め)。
- ウィキペディアの「Category:日本のインターネットスラング」の記事の見出しの語
- ウィキペディアの「新語・流行語大賞」の受賞語 (一覧は候補を出すのに使うだけで、入れるかどうかは下の数で決める)
- 日本語版ウィキペディアの月ごとの閲覧数の上位 1,000 記事のうち、カテゴリがインターネット・ミーム・流行語・ネットスラング・俗語のもの
語の記事のカテゴリが固有名詞 (作品・団体など) の受賞語は、こちらでなく常に効く固有名詞の表 (tools/core_names) に入れます。

## ファイル

| ファイル | 中身 |
|---|---|
| `slang_dictionaries/internet.tsv` | ネットスラングの辞書 (列は分野辞書と同じ: 表記・読み・品詞・意味・値・出典) |
| `slang_dictionaries/LICENSE.txt` | ライセンス (CC BY-SA 4.0) と、入れた語の記事の版 (make_license.py が書く) |
| `tools/domain/build_domain.py` | `domain/domain_slang_<名前>.*` と `domain/slang.txt` を書く (`--slang` で選べる) |

変換サーバーは `domain\slang.txt` に並ぶ名前のうち、設定でオンにしたものだけを読みます (azookey_server.swift の slangIdentifiers。
internet は input.internet_slang)。同音語の優遇は使いません。試験用のビルドでは `AZK_SLANG_INTERNET=1/0` で切り替えられます。

## 作り方

1. 記事を取る: `python3 -I fetch_category.py <src>` (カテゴリと下位のカテゴリの記事の題・版・冒頭。MediaWiki の API。
   連絡先つきの名乗り・1 つずつ・間を空ける・429 と maxlag なら待つ)。冒頭が空の記事は `fetch_missing_intro.py` で取り直す
2. `python3 -I make_table.py <src/articles.jsonl> table.tsv` (語・読み・意味・版)
3. まれかどうかの数:
   - 閲覧数: `python3 -I fetch_pageviews.py <src/articles.jsonl> pageviews.tsv` (直近 12 か月の月の平均、Wikimedia の pageviews API、1 つずつ 2 秒おき)
   - 全文の数: `python3 -I count_wiki_all.py table.tsv src/articles.jsonl wiki_all.tsv <文の束 ...>` (手元の日本語版ウィキペディアの全文で語を含む文の数。
     その語の記事の文は除く。重いので夜に) → `boundary_ratio.py` (1 つの束で語ごとに 300 文まで抜き、Sudachi の短い単位の語の切れ目に
     語の頭と終わりがそろう割合。目安の数 = 全文の数 × 割合。スピる が スピルバーグ に当たるような数を除く)
4. `fill_yomi.py` (読みの無いリダイレクトに、DEFAULTSORT と合う読み) → `make_sheet.py` (閲覧・全文・azooKey の辞書のあるなし) →
   `make_proposal.py` (区分: 辞書にある・型 (○○ガチャ)・語ではない (文・記事の題)・読みが無い・(a) の候補)
5. 流行語大賞: `ryukogo_parse_awards.py article.wikitext awards.tsv` (記事の今の版の表から 年・賞・語・受賞者) →
   `ryukogo_fetch_articles.py` (語と同じ名前の記事・転送・冒頭・DEFAULTSORT) → `ryukogo_readings_and_views.py` (閲覧数の 12 か月の平均といちばん多かった月) →
   `ryukogo_fix_readings.py` (読み: かなだけの語は表記 → 冒頭の語のすぐあとの括弧 → 受賞の表の括弧 (語の字の数以上のときだけ) → DEFAULTSORT) →
   全文の数 (count_wiki_all.py・boundary_ratio.py、記事の無い語・転送だけの語に使う) → `ryukogo_classify.py` (区分と (A)/(B))
6. 閲覧数の上位の記事: tools/core_names の fetch_top.py・filter_top.py・fetch_categories.py・classify_ab.py のあと、`b_candidates.py` (ふつうの語の候補) と
   `fetch_views.py` (候補の記事ごとの閲覧数)
7. `proposal_all.py` (3 つを同じ線で並べる: 記事のある語は 12 か月の月の平均の閲覧、記事の無い語は全文の目安の数) →
   `prepare_words.py` (線を越えた語に、冒頭の括弧に並ぶ読みを全部と、かなの 2 番目の形。上位の記事はスラングのカテゴリだけ、ほかは控えの表へ) →
   `python3 -I make_dict.py words.tsv <azookey_dict.tsv> ../../slang_dictionaries/internet.tsv` と
   `python3 -I make_license.py ../../slang_dictionaries/internet.tsv table.tsv ../../slang_dictionaries/LICENSE.txt`

線 (2026-11-07 の決め): 記事のある語は直近 12 か月の月の平均の閲覧 300 以上、記事の無い語・転送だけの語は全文の目安の数 20 以上 (ネットミームはどちらかが越えれば)。
性的な語も線のとおりに入れます (注意の設定をオンにした人には注記が出ます)。下の「入れない語」は線を越えても入れません。HaniWord の開発者が足した語 (requested.tsv) は線とは別に入れます。

品詞はふつう名詞 (1285)。〜る で終わる動詞 は azooKey の辞書の ググる の活用形 (語幹だけの形は除く) を写し、〜られる (ギュられる) は 〜ら だけを足します。
値は読みが 4 字以上なら -12.0、3 字までなら -14.0、かなの 2 番目の形 (ぬるぽ に対する ヌルポ) はそれより 0.5 下です。読みが複数ある語 (嫌儲) は全部の読みで入れます。

## 入れない語 (2026-11-22 の決め)

`tools/slang/exclude.tsv` の語は、線を越えても入れません (make_dict.py が読んで、その語と活用形を飛ばします)。語ごとには選ばず、
出典の記事 (転送のときは転送先の記事) の日本語版ウィキペディアのカテゴリで決めます (2026-11 のダンプの page・categorylinks・linktarget で確かめた)。
1. 一般の人の出来事から来た呼び名: カテゴリ「人物の愛称」に入る記事の語 (1 語)
2. 会社・国・地域・人の集まりをさげすむ語: カテゴリの名前に「蔑称」「差別用語」「侮蔑」「軽蔑語」を含むか、「日本の反〇感情」で〇が国・民族 (反中・反朝・反韓 など) の記事の語
   (9 語。〜る の動詞は活用形もみな)
- 2026-11-22 に、この決まりで 10 語 (辞書の行で 19 行) を外しました。性的な語はこの決まりの外で、今までどおり入れます (設定でオンにした人だけが使う辞書のため)
- 新しく語を足すときは、記事のカテゴリを見て、決まりに当たれば exclude.tsv に足してから make_dict.py・make_license.py を流します

## 確かめ方

- 入れた語の読みを変換して、目当ての語が 1 番目に出る数 (表なし・表あり、AI あり・辞書だけ)
- 一般の文 (wiki_luna_full、3,675 文) を、設定をオンにした形で rc と比べ、正しかった 1 番目が替わった数 0 を確かめる
