# ソフト・アプリ・サービスの製品名の表 (english_products.txt・extra_dictionaries/core_products.tsv)

After Effects・Premiere Pro・DaVinci Resolve・Visual Studio Code のような、空白を含むか大文字と小文字の決まった製品名を、2 つの打ち方で出す表です (2026-11-11)。

- かなモードで小文字の英数字を続けて打つ (aftereffects・davinciresolve・windows11) → その表記 (After Effects)。`english_products.txt` (リポジトリの頭)。
  IME 本体 (crates/client の auto_english.rs の product_phrases) が、英数字の並びの頭か、かなの語のすぐ後ろから始まり区切りの境目で終わる所が表の綴りと同じなら、
  表記の語ごとの英字の区切りにする。空白を含まない表記 (iTunes) は表記直しの表と同じ扱い。表記直しの設定 (input.english_case_fix) に従う
- カタカナの読みを打って変換する (あふたーえふぇくと) → 候補に After Effects。`extra_dictionaries/core_products.tsv` (オフにできない追加辞書)

単独のふつうの英単語 (illustrator・premiere・resolve・code) は大文字にしない。表の綴りは表記全体 (Adobe Illustrator = adobeillustrator) だけ。

## 材料

- 日本語版ウィキペディアのダンプ (2026-11、page・linktarget・categorylinks・redirect。gakumon47 の fetch_dumps.sh と sql_to_tsv.py)
- Wikimedia の月ごとの閲覧数のダンプ (2026-01・04・07・10。tools/core_names/README.md の 8.)
- HaniWord の英字の表 (english_case.txt・english_brands.txt・english_words_scowl.txt) と、英単語とカタカナ語の表 (english_words.tsv・english_words_extra.tsv)
- azooKey の辞書 (tools/extra_dict/dump_azookey_dict.py の出力)
- A の「ローマ字として読めるか」の判定 (ime_readable.py。IME 本体の native_romaji_readable と同じ)

## 手順

1. `collect_articles.py <jawiki の tsv の置き場> <月の閲覧数の置き場> articles.tsv` (ソフト・アプリ・サービスの名のカテゴリだけをたどった記事と月の平均の閲覧数)
2. `redirects.py <jawiki-latest-redirect.sql.gz> <jawiki の tsv の置き場> articles.tsv redirects.tsv` (その記事への転送の題)
3. `make_products.py articles.tsv redirects.tsv <リポジトリ> <azookey_dict.tsv> english_products.txt core_products.tsv candidates.tsv --readable-script <ime_readable.py> --min 300`
   (決まりは頭の説明。線は記事の月の平均の閲覧数 300 以上 (仕事 44 と同じ)。数字の入った版の名 (Windows 11) も入れる)

## 決まりの直し (2026-11-19、まとめ役の rc-next 234b9b9 の見直し)

- 大文字小文字: ウィキペディアは題の頭の字を大文字にするので、表記直しの表に無い頭の語は、「I＋大文字＋小文字」(ITunes・IMessage) だけ i にする
  (前は I＋大文字 のどれも i にしていて、IBM Db2 を iBM Db2 にしていた)。ほかの「大文字 1 つ＋大文字で始まるふつうの英単語」(UBlock) は、
  頭が小文字か大文字か分からないので入れない。表の書き方が頭の 1 字だけ小文字 (iOS) なら、表の書き方にする
- 1 語のふつうの英単語: SCOWL の表に加えて、カタカナ語の元の英単語 (english_words.tsv。JMdict と CMUdict から自動で作った表) にある語も
  単独では入れない (neutrino を いつも NEUTRINO にしていた)。SCOWL の size 60 の表 (english_words_context.txt) は製品名・技術用語も小文字で
  入っているので使わない。handbrake はどちらの表にも無いので HandBrake のまま
- 単数の読み (Effects → エフェクト) は、s 付きの語が s 付きの読みで azooKey の辞書にある (Windows ウィンドウズ・Works ワークス) か、
  記事のカタカナの転送の題に s 付きの読みがある (Microsoft Teams の チームズ) なら作らない (マイクロソフトウィンドウ・マイクロソフトチーム・ラインワーク を外した)
- 表記が azooKey の辞書にもうある語 (COBOL コボル・iTunes アイチューンズ) には、カタカナの転送の題の読み (コボラー・イツネス) を足さない
  (転送の題は別の語や綴りのローマ字読みのことがある)。語ごとに組み立てた読み (After Effects ← アフターエフェクト) は足す

## 確かめ

入れる前に、A の誤判定のベンチで、日本語を英字にしてしまう形が増えないかを見る。
