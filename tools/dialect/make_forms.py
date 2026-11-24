"""2026-11-06 (仕事 39): 方言の比較表をほどいた hikaku.tsv と、役割語の記事から拾った語尾 (下の ROLE) を、1 つの表 forms.tsv にまとめる。
使い方: python3 make_forms.py hikaku.tsv ../../dialect_dictionaries/dialect_forms.tsv (hikaku.tsv は parse_table.py で作る)
"""
import sys
from collections import OrderedDict

SOURCES = [
    ("1", "日本語の方言の比較表", 110668082, "2026-08-16"),
    ("2", "役割語", 110771186, "2026-08-25"),
    ("3", "お嬢様言葉", 111459006, "2026-10-21"),
    ("4", "老人語", 111596552, "2026-11-01"),
]
# 関西を先に。そのあと記事の表にある主な地域
DIALECTS = ["大阪弁", "京言葉", "江州弁", "丹波弁", "但馬弁", "丹後弁", "伊勢弁",
            "広島弁", "博多弁", "北九州弁", "名古屋弁（尾張弁）", "土佐弁", "津軽弁", "南部弁", "仙台弁", "秋田弁", "庄内弁"]
SKIP_ITEM = ("アクセント", "イントネーション", "母音", "仮名", "音", "麦粒腫", "中舌", "区別", "無声化")
# 役割語の語尾 (記事の地の文にあるもの。用例の引用はそのまま写さず、語尾だけ)
ROLE = [
    ("老人", "一人称・断定", "わし・〜じゃ・〜じゃよ", "2", "「そうなんじゃ、わしは知っとるんじゃ」の形の例 (役割の節)"),
    ("老人", "打ち消し・存在", "〜ぬ（〜ん）・おる", "2", "老人語の節"),
    ("老人", "一人称・断定・打ち消し", "わし・〜じゃ・〜ぬ（〜ん）", "4", "フィクションにおける老人語の節"),
    ("昔話の似非方言", "語尾", "〜じゃのう", "2", "方言の節 (さねとうあきら)"),
    ("田舎者 (東北弁風)", "一人称・断定", "おら・〜するだ・〜だべ", "2", "方言の節"),
    ("貴婦人・お嬢様", "語尾", "〜ですわ・〜ますわ・わたくし", "2", "役割の節の例"),
    ("お嬢様", "文末", "〜てよ・動詞＋わ・名詞＋ですわ・〜こと", "3", "文末表現の節"),
    ("お嬢様", "一人称", "わたくし・あたくし", "3", "一人称・敬語表現の節"),
    ("お嬢様", "挨拶", "ごきげんよう", "3", "挨拶表現の節"),
    ("女学生ことば", "文末", "〜ですの・〜わ・〜御座いますの・〜遊ばして", "3", "女学校を通じた拡散の節"),
    ("女性語 (小説)", "文末", "〜だわ・〜だもの・〜なのよね・〜たのよ", "2", "役割の節の引用"),
    ("アルヨことば", "文末", "〜ある・〜あるよ・〜あるね・〜あるぞ・〜あるな・〜あるか・〜するよろし（い）", "2", "アルヨことばの節"),
    ("ありんす詞", "文末", "〜ありんす", "2", "有りんす詞の節"),
    ("キャラ語尾", "文末", "〜なのだ・〜ナリ・〜だっちゃ・〜クポ・〜やんす・〜ワン・〜ニャ", "2", "キャラ語尾の節"),
    ("おばあさん語", "文末", "〜かね・〜おくれ", "2", "千と千尋の神隠しの節"),
    ("神様語", "二人称", "そなた", "2", "千と千尋の神隠しの節"),
]


def main():
    src, out = sys.argv[1], sys.argv[2]
    forms = OrderedDict()
    for line in open(src, encoding="utf-8"):
        if line.startswith("#"):
            continue
        sec, item, dia, group, form = line.rstrip("\n").split("\t")
        dia = dia.strip()
        if dia not in DIALECTS or any(s in item for s in SKIP_ITEM):
            continue
        key = (dia, item.strip())
        vals = forms.setdefault(key, [])
        if form not in vals:
            vals.append(form)
    with open(out, "w", encoding="utf-8") as f:
        f.write("# 方言と役割語の文の終わり・助動詞の形 (HaniWord の開発者が 2026-11-06 に記事から拾った。方言の言い方の辞書 (kansai.tsv など) と試しの文の材料)\n")
        f.write("# 出典 (どれもウィキペディア日本語版、CC BY-SA 4.0。版の番号と、その版の日付。取ったのは 2026-11-06):\n")
        for n, title, rev, day in SOURCES:
            f.write(f"#  [{n}] 「{title}」 https://ja.wikipedia.org/w/index.php?oldid={rev} ({day} の版)\n")
        f.write("# 地域・役割\t項目\t形\t出典\t出典の中の場所\n")
        for d in DIALECTS:
            for (dia, item), vals in forms.items():
                if dia == d:
                    f.write(f"{dia}\t{item}\t{' ／ '.join(vals)}\t1\t表\n")
        for who, item, form, n, where in ROLE:
            f.write(f"{who}\t{item}\t{form}\t{n}\t{where}\n")
    print(sum(1 for _ in open(out, encoding="utf-8")) - 6, "rows")


main()
