# 方言の言い方の辞書 (dialect_dictionaries)

設定の「方言の語尾」(input.dialects、初期値は使わない) で選んだ人だけが読む辞書です。今は関西 (kansai) だけ。
関西の文の終わり・助動詞のまとまり (へん・ひん・へんかっ・てへん・はる・はった・てはる・ねん・やねん・やん・やんか・かいな・さかい・
せやから・しいや・ええ・あかん・ほんま・はよ・おん・せやろ など) と、ウ音便の動詞 (買う (こう)・手伝う (てつどう)) を足して、
かなのまま出しやすくします (辞書だけの変換で 終わって変 → 終わってへん・言うて貼った → 言うてはった、AI ありで 気張った → 来はった)。

## ファイル

| ファイル | 中身 |
|---|---|
| `dialect_dictionaries/kansai.tsv` | 関西の辞書 (列は分野辞書と同じ: 表記・読み・品詞・意味・値・出典) |
| `dialect_dictionaries/dialect_forms.tsv` | 形の表 (地域・役割ごとの文の終わり・助動詞の形と出典の番号)。辞書ではない |
| `dialect_dictionaries/LICENSE.txt` | ライセンス (CC BY-SA 4.0) と出典の記事の版 |
| `tools/domain/build_domain.py` | `domain/domain_dialect_<名前>.*` と `domain/dialects.txt` を書く (分野の名前を付けたとき (calibrate) は書かない。`--dialects` で選べる) |

変換サーバーは `domain\dialects.txt` に並ぶ名前のうち、設定 input.dialects にあるものだけを読みます (azookey_server.swift の dialectIdentifiers)。
分野辞書と違って初期値は使わず、同音語の優遇も使いません。試験用のビルドでは `AZK_DIALECT_<名前の大文字>=1/0` で切り替えられます。

## 作り方

1. 記事を MediaWiki の API で取る (版の番号 revid と日付を控える)。今の版は LICENSE.txt にあります。
   `curl -G -A '<道具の名前と連絡先の URL>' https://ja.wikipedia.org/w/api.php --data-urlencode action=query --data-urlencode prop=revisions --data-urlencode 'titles=日本語の方言の比較表' --data-urlencode 'rvprop=content|ids|timestamp' --data-urlencode rvslots=main --data-urlencode format=json --data-urlencode formatversion=2`
2. `python3 parse_table.py <日本語の方言の比較表.wikitext> hikaku.tsv` (表を方言の列ごとにほどく。見出しは 〜弁・〜言葉 の行、まとまりは 〜方言 の行)
3. `python3 make_forms.py hikaku.tsv ../../dialect_dictionaries/dialect_forms.tsv` (関西を先に、主な地域と役割語の語尾を並べる。記事の版を変えたら SOURCES も)
4. `python3 make_kansai_dict.py <azookey_dict.tsv> ../../dialect_dictionaries/kansai.tsv`
   (azookey_dict.tsv は tools/extra_dict/dump_azookey_dict.py で書き出す azooKey の辞書。ウ音便の語は、その五段ワ行の動詞の 連用タ接続 (買っ) と 基本形 (買う) から作る)

品詞の番号は、同じ位置に付く標準語の語を手本にしています (た 435・ます 491・ない 473・なかっ 480・接続詞 555・よ 288・ね 279・わ 291・
かな 323・じゃん 508・形容詞 19・形容動詞の語幹 1287・副詞 1282・感動詞 3・五段ワ行の連用タ接続 829・しまっ 1269・ラ行の撥音便 774・ひん 511・へん 512・しろ 608)。

## 確かめ方

- 試しの文: 1 行 `読み / よい形 / かならず残る語尾 (, 区切り。| はどちらか) / ほかによい形 (| 区切り)`。
  `python3 cases.py make <src.txt> <out.tsv>` で変換器に渡す組を作り、`python3 cases.py score <src.txt> <結果.jsonl>` で数える
  (完全に同じ数と、語尾がかなのまま残った数を、AI あり・辞書だけ別に)。`python3 cmp.py <src.txt> <前.jsonl> <後.jsonl>` で文ごとの直った・崩れたを出す。
- 一般の文 (wiki_luna_full、3,675 文) で、辞書を足す前と後の結果を比べ、標準語の文を悪くしないことを確かめる。
- 2026-11-06 の測り (関西の試しの文 156 文): 語尾が残ったのは AI あり 141 → 145 (崩れ 0)、辞書だけ 93 → 124 (崩れ 1)。一般の文から 307 文を抜いた組では変わった文 0。
- 2026-11-07: 一般の文 3,675 (関西を選んだ人の標準語の文) で、AI ありは変わった文 0、辞書だけは崩れ 1・変わった 4 だった (肯定ないし → 買うていないし・王手 → 似合うて・
  鉄道 → 手伝う・元年 → が+んねん)。ウ音便の形の値をふつうの名詞より下 (-13.0 まで) にし、ねん・んねん の 1 語の行を外した (やねん は残す)。
  この形で、変わった 4 文は rc と同じに戻り、関西の試しの文は AI あり 145・辞書だけ 122 (rc 93)
