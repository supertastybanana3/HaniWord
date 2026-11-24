# オフにできない追加辞書 (辞書の抜け) の作り方

`extra_dictionaries/core_*.tsv` を作る道具です。azooKey の辞書に無い活用形・成句・語を、低めの重みで足します。
組み立て (LOUDS にする) と変換サーバーでの読み込みは `tools/domain/build_domain.py --always` (分野辞書と同じ形) です。

- `core_forms`: azooKey の辞書に基本形だけある動詞・形容詞の活用形 (俟つ → 俟た・俟ち・俟っ…)。手本の語 (待つ・持つ…) の形にならって作り、Mozc の公開辞書に同じ読み・表記がある形だけ残す
- `core_words`: 文章 (写経の文の組) に出てくる語のうち、Mozc の公開辞書にあって azooKey の辞書に無く、今の変換で正しく出ないもの
- `core_phrases`: JMdict の表現・慣用句のうち、azooKey の辞書に無く、今の変換で正しく出ないもの (動詞・形容詞で終わるものは活用形も)
- `core_rare_kanji`: JMdict の常用外の字の表記のうち、azooKey の辞書に同じ読みで無いもの (嗾ける・揶揄う・縞々。2026-11-07、仕事 42)。同じ読み・同じ品詞の語のいちばん下に置く

`extra_dictionaries/` の `.tsv` は辞書だけにする (build_domain.py が全部を辞書として読む)。辞書を作るための表は `.txt` (core_phrases_exclude.txt・core_phrases_prefer.txt・variant_pairs.txt)。
ライセンスは `extra_dictionaries/LICENSE.txt` (Mozc の公開辞書 = IPAdic の許諾、JMdict = CC BY-SA 4.0)。

## 材料

作業場所 (リポジトリの外) を環境変数 `HANIWORD_BUILD` で渡します。

- `$HANIWORD_BUILD/data_ext/mozc_oss/`: Mozc の公開辞書 (google/mozc の `src/data/dictionary_oss/dictionary0*.txt` と `id.def`・`LICENSE`。commit f183d4d3)
- JMdict_e.gz (https://www.edrdg.org/jmdict/j_jmdict.html)
- azooKey の辞書: 変換サーバー一式 (stage) の `Dictionary/louds`
- 写経の文の組: `$HANIWORD_BUILD/data_ext/domain/eval/*.cases.tsv` と、記事の名前に `$HANIWORD_BUILD/probe/e2e/results/night_installer-full800_part{1..4}.txt`
- Sudachi (`sudachipy`・`sudachidict_full`) のある Python、作業場所の写経の集計 `probe/e2e/analyze.py` (外れの型)

## 手順 (WORK は途中のファイルの置き場)

```sh
export HANIWORD_BUILD=...
python3 dump_azookey_dict.py <stage>/Dictionary/louds $WORK/azookey_dict.tsv
python3 map_pos.py $WORK            # azooKey の品詞の番号 -> Mozc の品詞の名前 (azk_pos.tsv)
python3 mozc_diff.py $WORK          # Mozc にあって azooKey に無い組 (mozc_missing.tsv)

# 活用形
python3 missing_forms.py $WORK
python3 make_core_forms.py $WORK/missing_forms.tsv ../../extra_dictionaries/core_forms.tsv

# 語 (一般の文と分野の文に出てくる語。分野の文の語は、分野辞書にある語を先に除く)
$PY pick_used.py $WORK $WORK/words/used.tsv
$PY pick_used.py $WORK $WORK/used_dom.tsv $HANIWORD_BUILD/data_ext/domain/eval/domain_*_full.cases.tsv
python3 prefilter_words.py ../.. $WORK/used_dom.tsv $WORK/used_dom_f $WORK/words/used.tsv
# 今の変換で確かめる (Windows の Python。隔離した設定 APPDATA は learning=false)
py -3 -I conv_probe.py <stage> used_cases.tsv used_conv.jsonl APPDATA=<隔離した設定>   # used.tsv・used_dom_f.tsv の 読み<TAB>表記 を
# used.tsv と used_dom_f.tsv、used_conv.jsonl どうしを合わせて 1 つの置き場 ($WORK/words/all) に置き
$PY make_core_words.py $WORK $WORK/words/all     # core_words.tsv

# 成句
python3 jmdict_expressions.py JMdict_e.gz $WORK            # jm_cases.tsv・jmdict_siblings.json
py -3 -I conv_probe.py <stage> jm_cases.tsv jm_dict.jsonl APPDATA=<隔離した設定> --dict-only
$PY filter_phrases.py $WORK/jm_dict.jsonl $WORK/jm_full_cases.tsv
py -3 -I conv_probe.py <stage> jm_full_cases.tsv jm_full.jsonl APPDATA=<隔離した設定>
$PY make_core_phrases.py $WORK $WORK/jm_full.jsonl ../../extra_dictionaries/core_phrases.tsv
```

`conv_probe.py` で確かめるときの変換サーバーは、分野辞書を全部入れ、追加辞書は入れないもの (今の配布の形) にします。

```sh
# 常用外の字の表記 (core_rare_kanji。除く表は core_rare_kanji.tsv 以外の追加辞書)
EX=$(ls ../../extra_dictionaries/core_*.tsv | grep -v core_rare_kanji)
python3 -I rare_kanji_find.py JMdict_e.gz kanjidic2.xml.gz $WORK/azookey_dict.tsv $WORK/rare_missing.tsv $EX
python3 -I rare_kanji_plain.py $WORK/rare_missing.tsv $WORK/azookey_dict.tsv $WORK/azk_pos.tsv $WORK/rare_plain.tsv $WORK/rare_plain_skip.tsv $EX
python3 -I rare_kanji_forms.py JMdict_e.gz kanjidic2.xml.gz $WORK/azookey_dict.tsv $WORK/azk_pos.tsv $WORK/rare_forms.tsv $WORK/rare_forms_words.tsv $EX
python3 -I rare_kanji_table.py $WORK/rare_plain.tsv $WORK/rare_forms.tsv ../../extra_dictionaries/core_rare_kanji.tsv
```

常用外の字 (KANJIDIC2 の grade が 1〜8 でない字) の表記 (rare_kanji_*.py) は、次の決まりで足します (2026-11-07、仕事 42)。
- 足さない: 表記の印 sK・oK・iK (rK は入れる)、語義の印 X・vulg・derog、どの語義も古語か廃語、同じ項目で前に並ぶ表記 (よく使う書き方) が辞書にある語、
  片仮名を含む動詞の表記、2 字までの読みの活用形、同じ読みの語が辞書に無い語 (値の手本が無い)、かなの動詞が辞書に無い動詞
- 品詞の番号は、azooKey の辞書の同じ読みの語のうち JMdict の品詞に合う語から写す。動詞・形容詞の活用形は、同じ読みのかなの動詞の形のうち、
  その活用の型の かなの語の 3 割以上にある形だけを写す
- 値は、同じ読み・同じ品詞の語 (追加辞書の語も入れる) のいちばん低い値より 0.5 下 (uk の印の語は 1.5 下)、-13 より上には置かない。
  辞書だけの変換の 1 番目は変わらず、候補の一覧で今ある候補を押し下げない (一覧の上の 2 つは AI が選ぶので、そこは一般の文で確かめる)
- 候補の一覧での置き場 (2026-11-08、まれな書き方は下の候補): AI は辞書の値を見ずに字を選ぶので、値だけでは一覧の 1 番目に来ることがある。
  変換サーバーは、一覧の 1 番目が表の語を含むときだけ、表を隠して一覧を作り直し、その 1 番目 (表が無いときの 1 番目) を見る。
  それが辞書の 1 語・辞書の語＋送り仮名や助詞・同じ語の一部をかなにした形なら 1 番目に戻し (表の語はすぐ後ろ)、ばらばらの語をつないだ崩れた形
  (消しかけて → 嗾けて) なら表の語を 1 番目のままにする (server-swift/Sources/azookey-server/rare_spelling.swift。変換には触らない)

## 決まり

- 値: 活用形は「元の語より 1 低く、-10 より上にしない」。語は、両方の辞書にある語の Mozc のコストと azooKey の値の当てはめ (名詞) から 1 下げ、-10 より上にしない。成句は -11 (句の読み全体を打ったときに勝つ程度)
- 品詞の番号: 追加辞書は左右 1 つの番号なので、後ろにつながる側 (語の終わりの品詞) に合わせる
- 今の変換との違いが表記ゆれ・数字の書き方だけのものは入れない (どの書き方にするかは今の辞書と AI に任せる。数字の書き方は規則では変えない)
- 成句は、違う所のどちらかがかなだけのもの (其内/そのうち)、今の変換が同じ JMdict の項目の別の書き方のもの (到る所/至る所)、読みが 4 字未満のものも入れない

## 代わりの字の成句 (variant_spellings.tsv)

本来の字 (表外字) を、意味の違う常用漢字で代わりに書く成句 (論を俟たない/論を待たない・身を挺する/身を呈する・癇に障る/勘に障る) は、
本来の書き方かかな書きを先にし、代わりの字の書き方を後ろにします。変換サーバー (variant_spellings.swift) が表を読み、
変換の結果に代わりの字の書き方が句の読みのまま出たら本来の書き方にし、候補の一覧では本来の書き方とかな書きを代わりの字の書き方より前に置きます。
JMdict の書き方の印には「意味の違う字での代わり」の印が無い (論を待たない は印が無く、1 番目の書き方) ので、次のように選びました。

1. 候補: JMdict の表現・慣用句で、同じ項目の書き方どうしが 1 字だけ違い、片方が表外字・片方が常用漢字の組 (常用漢字は KANJIDIC2 の学年 1〜6・8 の 2,136 字)。2026-10-22 は 271 項目・367 組
2. 判定: 互いの答えを見ない 2 つの判定 (LLM) に、A 字体の違い・B 代わりの字が標準 (損害を被る)・C 意味の違う字での代わり (論を待たない)・
   D 表外字のほうが誤り (無碍/無下) の 4 つに分けさせ、2 つとも迷わず C とした組だけを `extra_dictionaries/variant_pairs.txt` に入れる (理由も書く)。2026-10-22 は 28 組
3. 表: `make_variant_spellings.py` が、本来の書き方の活用形を make_core_phrases.py と同じ作り方で作り、同じ所の字を替えて代わりの字・かな書きの形にする

```sh
$PY make_variant_spellings.py $WORK ../../extra_dictionaries/variant_pairs.txt ../../variant_spellings.tsv \
    --current $WORK/variant_current.jsonl ../../extra_dictionaries/core_phrases_prefer.txt
$PY make_core_phrases.py $WORK $WORK/jm_full.jsonl ../../extra_dictionaries/core_phrases.tsv \
    --exclude ../../extra_dictionaries/core_phrases_exclude.txt --prefer ../../extra_dictionaries/core_phrases_prefer.txt
```

`--current` は、組の読みを今の変換 (分野辞書と追加辞書を入れた変換サーバー) にかけた結果 (conv_probe.py)。それをもとに、句を 1 つの文節にして本来の書き方を候補に出すための行を成句の辞書に足す
(今が代わりの字なら 本来の書き方 -11 だけ、今がかな書きなら かな書き -11・本来の書き方 -11.5。かな書きは変換サーバーが候補の一覧に入れる)。

## よくある誤用 (M3.6)

考え方は「IME は打ったとおりに従う。打った読みと違う語を先に出さない」。材料は作業場所の data_ext/misuse/misuse.tsv (文化庁「国語に関する世論調査」・
文化審議会「「異字同訓」の漢字の使い分け例（報告）」・Wikipedia「日本語の誤用」)。`make_misuse_tables.py` が次の表を作る。

- 字の誤り (同じ読み。危機一発→危機一髪): `extra_dictionaries/misuse_spellings.txt` → make_variant_spellings.py が variant_spellings.tsv に入れる (代わりの字の成句と同じ並べ替え)
- 音の誤りで、本来の形が崩れただけのもの (音位転換・清濁・区切り。のべつくまなし→のべつまくなし): `core_misreadings_prefer.txt`。
  2026-10-30: 読み間違い (誤読・音位転換) で、誤った読みがほかの語にならないもの (ふいんき→雰囲気・しゅみれーしょん→シミュレーション):
  `core_misreadings_prefer_readings.txt` (Mozc の reading_correction.tsv と Wikipedia から選んだ。選び方はファイルの頭の説明。make_misuse_tables.py は書き換えない)。
  2 つをつないで渡す:
  `cat ../../extra_dictionaries/core_misreadings_prefer.txt ../../extra_dictionaries/core_misreadings_prefer_readings.txt > $WORK/prefer_all.txt`
  `make_core_phrases.py <WORK> <空の jsonl> ../../extra_dictionaries/core_misreadings.tsv --prefer $WORK/prefer_all.txt --any-reading`
  (<WORK> には azookey_dict.tsv・azk_pos.tsv・jmdict_siblings.json (読み間違いだけなら {} でよい) を置く)
- 音の誤りで、誤った形も語の並びになるもの (似た音。てんかのほうとう→伝家の宝刀) と読みのゆれ (せろん→世論): `misreading_candidates.tsv` (変換サーバーが、一覧に無ければ後ろに足す)
  2026-11-20: 読み間違い (正しい読みのある行。似た音・音の脱落の誤りと、Mozc の reading_correction.tsv の全行) は 3 列目に正しい読みを書き、
  変換サーバーが一覧の 4 番目に足して注記「もしかして: 正しい読み」を出す (IME 本体の設定に関係なく)。Mozc の表は作業場所の
  data_ext/mozc_reading_correction/ (reading_correction.tsv と COMMIT) に置く。新しい commit にするときは、そこを替えて make_misuse_tables.py を流し直す
- 注記 (設定でオンのときだけ。既定でオフ): 言い回しの混ざり・重ね言葉・字の誤り・似た音の「本来は〜」と、異字同訓の使い分け: `misuse_notes.tsv`
- 入れないもの: 意味の誤用 (確信犯・役不足)、意味で決まる字の誤り (若干/弱冠)。言い回しの混ざり (汚名挽回・的を得る) は候補に足さず注記だけ

```sh
python3 make_misuse_tables.py ../..
$PY make_variant_spellings.py $WORK ../../extra_dictionaries/variant_pairs.txt,../../extra_dictionaries/misuse_spellings.txt ../../variant_spellings.tsv \
    --current $WORK/variant_current.jsonl ../../extra_dictionaries/core_phrases_prefer.txt
```

## JMdict の版を新しくする

JMdict のライセンス (https://www.edrdg.org/edrdg/licence.html の 4 節) は、データを入れたソフトに、最新の版から定期的に更新する手順があることを求めています。
HaniWord の決まり (2026-11-23 に、続けられる形に改めた): JMdict_e.gz は、開発者が配布のページ (https://www.edrdg.org/jmdict/j_jmdict.html) から手で取ります (道具では取りません)。HaniWord の版を出す前に、表に使った JMdict の作成日を確かめ、3 か月より古ければ、新しい JMdict_e.gz を取って作り直してから出します (分野辞書の tools/domain/README.md と同じ)。
版を出さない間は作り直しません。どの表をどの版から作ったかは、`extra_dictionaries/LICENSE.txt` の「使った JMdict」の行にあります。

新しい JMdict_e.gz を取ってきて、上の「成句」の手順 (jmdict_expressions.py から make_core_phrases.py まで) を流し直し、`core_phrases.tsv` を差し替えます。
`extra_dictionaries/LICENSE.txt` の「使った JMdict」の行 (ファイルの中の `<!-- JMdict created: ... -->` の日付と取った日) も書き直します。
変換サーバーの版が変わったときも、今の変換で正しく出るものを除き直すために流し直します。

## 確かめ方

足した語の読みを含まない文は変換が変わらないので、読みを含む文だけを比べます (`affected.py`)。
`run_always_eval.sh` は試しのステージ (環境変数 `STAGE`) の追加辞書を入れ替えて `tools/domain/convert.py` で変換し、`cmp.py` で基準と比べます。
選んだ文で測ると効きが大きく出るので、語は記事を 2 つに分け、片方の記事で選んだ語をもう片方の文で測ります (`make_core_words.py` が `core_wordsa/b.tsv`・`cases_a/b.tsv` を書く)。
