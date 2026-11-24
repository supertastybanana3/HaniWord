# 注意の注記の表 (extra_dictionaries/caution_notes.txt)

候補はそのまま出し、設定でオンにした人の候補窓で「注意」の注記を添えるための表です。2 段あり、どちらも初期値オフです。

- 種類「差別」(設定 input.caution_slurs): 出どころが差別・侮辱の語、蔑称として書いている語
- 種類「言い換え」(設定 input.caution_broadcast): 放送・新聞・出版で言い換えられる語、チャットなどで弾かれる語、人に向けると侮辱になる語など。
  注記の文は出どころの書き方に合わせて分けています (放送・新聞で言い換えられる・チャットなどで弾かれる・人に向けると侮辱になる・使い方によっては侮辱や差別と受け取られる・
  商標を避けて言い換えられる・色の名前としては言い換えられる・出版などで言い換えられる)

表は tools/extra_dict/make_misuse_tables.py が misuse_notes.tsv の後ろに入れ、変換サーバーが候補に注記を付け、IME 本体が設定に合う種類だけを出します。
注記の末尾の [n] は、配布物の licenses\caution_references.txt (extra_dictionaries/caution_references.txt) の文献の番号です。

## 列

`種類<TAB>引き方<TAB>鍵<TAB>注記<TAB>除く語`

- 引き方「含む」: 候補の字が鍵を含むときに付ける。除く語 (| 区切り) のどれかを候補が含めば付けない
- 引き方「読み」: 鍵が `読み:表記`。文節がその語ちょうどのときだけ付ける (1 字の語と、かなだけの 4 字までの語。考えた の えた・ばかり の ばか に付かないように)

## 作り方

1. 集める: 出どころごとにサブエージェントに開いて確かめさせ、1 語ごとに URL・根拠の一文 (そのまま)・書誌・段の目安を JSONL に書かせる。
   決まり: 実際に開いたページだけ。robots.txt と AI の取得を断る表示を守る (Claude-User・ClaudeBot・AI 全般への断りがあれば取らない)、
   名乗りを偽らない (連絡先つきの名乗り)、取得を拒否されたページを別の道具で取り直さない、1 つずつ間を空ける。放送禁止用語の一覧のまとめサイトは使わない。
   NG 語の一覧は、ライセンスが書かれていて許しの広いもの (MIT・Apache・BSD・CC0・CC BY) の、差別・侮辱の分類の語だけ
2. `python3 merge.py <out> collect/*.jsonl`: 段を決める (段 a = 差別、段 b = 放送・新聞の記事とウィキペディアの言い換え)。迷う語は (b) に回し、注記の文の種類を出どころで決める
3. 迷う語と出どころが 1 つの (b) の語の根拠の文を、2 つのモデルに別々に (i) 今の差別・侮辱・言い換え・弾きの語と書いている (ii) 昔の使い方だけ
   (iii) 別の意味・別の話の例 (iv) 一覧に語があるだけ、に分けさせ (classify/out_*.jsonl)、`python3 finalize.py` で 2 つとも (i) の語だけを残す
4. `python3 refs.py <refs.txt> <words.tsv> --wayback wayback.tsv <rows.jsonl>`: 出典の URL ごとに番号を振り、参考文献の一覧を SIST 02 の Web の形で書く
   (`wayback.py` で Internet Archive の写しを探し、無ければ Save Page Now で作る。AI の取得を断るサイトの URL は送らない。ウィキペディアは版の固定リンクなので作らない)
5. 除く語: `scan_exclusions.py` で、手元の Wikipedia の文で鍵を含む長い続きを数え、`straddle_exclusions.py` で、鍵が語の切れ目 (Sudachi の短い単位) を
   またぐ続き (非人為的 の 非人、本土方言 の 土方) を除く語にする。手で足す除く語 (支那 に 支那そば) も合わせる
6. `python3 make_table.py <words.tsv> <azookey_dict.tsv> <rows.tsv> <review.tsv> <rows.jsonl>` (環境変数 DECIDED・MANUAL_EXCL) で表の行を作る

確かめ: 一般の文 (wiki_luna_full、3,675 文) の正解で注記が付く文の数を語ごとに数える (2026-11-07: 17 文・10 語)。候補は替えないので、変換の結果は変わらない。
