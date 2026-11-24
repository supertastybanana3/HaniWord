"""分野辞書の作り方で共通に使うもの: 読みの正規化と、文から専門用語の候補 (名詞の連なり) を抜き出す処理。
Sudachi (sudachipy + sudachidict_full) を使う。読みは下書き (luna で付け直す前の目安) としてだけ使う。"""
import re

KANJI = re.compile(r"[一-龥々〆ヵヶ]")
KATAKANA_READING = re.compile(r"[ァ-ヺー]+")


def to_katakana(s):
    return "".join(chr(ord(c) + 0x60) if "ぁ" <= c <= "ゖ" else c for c in s)


def to_hiragana(s):
    return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)


def clean_reading(r):
    """読みをカタカナだけにそろえる (空白・中黒を除く)。カタカナと長音だけにならなければ None"""
    if r is None:
        return None
    r = to_katakana(re.sub(r"[\s・･]", "", r))
    return r if KATAKANA_READING.fullmatch(r) else None


FULLWIDTH_ALNUM = str.maketrans({chr(c): chr(c - 0xFEE0) for c in [*range(0xFF10, 0xFF1A), *range(0xFF21, 0xFF3B), *range(0xFF41, 0xFF5B)]})
HYPHENS = str.maketrans({c: "-" for c in "‐‑‒–−－"})
NOT_IN_TERM = re.compile(r"[\s（）()「」『』【】、。，,!?！？]")


def normalize_term(term):
    """辞書に入れる表記をそろえる。入れないものは None。
    全角の英数字は半角に (ＴＬＯ法 → TLO法。変換器は半角で出す)、ハイフンの類は - に、片仮名のあとのダッシュは長音に (アトピ—性 → アトピー性)、
    ウィキペディアの記事名で頭が大文字になったギリシャ文字は小文字に (Α細胞 → α細胞)。括弧・読点・空白を含む語は入れない"""
    t = term.translate(FULLWIDTH_ALNUM).translate(HYPHENS)
    t = re.sub(r"(?<=[ァ-ヺ])[—―]", "ー", t)
    if t and "Α" <= t[0] <= "Ω" and not (len(t) > 1 and ("Α" <= t[1] <= "ω")):
        t = t[0].lower() + t[1:]
    if NOT_IN_TERM.search(t) or re.search(r"[—―]", t):
        return None
    return t


_tokenizer = None


def tokenizer():
    global _tokenizer
    if _tokenizer is None:
        from sudachipy import Dictionary
        _tokenizer = Dictionary(dict="full").create()
    return _tokenizer


# 連なりの頭に来たら捨てる語 (法令・判決の指示語)。終わりに来たら捨てる語 (範囲・時・場合)
HEAD_STOP = {"当該", "前", "同", "次", "各", "本", "該", "他", "別", "上記", "下記", "前記", "後記", "所定", "所要", "右", "左", "其",
             "前項", "同項", "次項", "前条", "同条", "次条", "前号", "同号", "次号", "本条", "本項", "本号", "本法", "同法", "前段", "後段",
             "各号", "各項", "以下", "以上", "当", "旧", "新", "元", "現", "条", "項", "号", "款", "目", "所定"}
TAIL_STOP = {"等", "中", "以下", "以上", "以外", "未満", "内", "間", "後", "前", "時", "際", "場合", "ため", "旨", "ほか", "外", "上",
             "下", "分", "毎", "ごと", "共", "方", "者等", "物等", "第", "号", "項", "条", "款", "目", "編", "章", "節", "附則", "別表"}


def usable(m):
    """名詞の連なりに入れる語: 名詞 (数詞・代名詞を除く)・接頭辞・名詞的な接尾辞。数字だけ・漢数字だけの語は入れない"""
    pos, s = m.part_of_speech(), m.surface()
    if pos[0] == "名詞":
        # 人名・地名は分野の語にしない (判決の裁判官・代理人の名前、法令の地名。「仙台地方裁判所」なら「地方裁判所」だけ残る)
        ok = pos[1] not in ("数詞", "代名詞") and not (pos[1] == "固有名詞" and pos[2] in ("人名", "地名"))
    else:
        ok = pos[0] == "接頭辞" or pos[0] == "接尾辞" and pos[1] == "名詞的"
    return ok and not re.search(r"[0-9０-９]", s) and not re.fullmatch(r"[一二三四五六七八九十百千万〇]+", s) \
        and not re.search(r"[一二三四五六七八九十百千〇][条項号款目編章節年月日円人回箇個分]", s)


PARTICLES = set("のをにがはとでへもや")


def plausible(surface):
    """語らしい形か: ひらがなは送り仮名 (漢字の後ろの 1 字・2 字) だけで助詞を含まない。片仮名は外来語 (3 字以上続く) だけ"""
    for m in re.finditer(r"[ぁ-ゖ]+", surface):
        run = m.group()
        if len(run) > 2 or m.start() == 0 or any(c in PARTICLES for c in run):
            return False
    for m in re.finditer(r"[ァ-ヺー]+", surface):
        if len(m.group()) < 3:
            return False
    return True


def old_style(text):
    """片仮名書きの古い法令 (〜ス。〜ベシ。) なら True"""
    ends = re.findall(r"(.)。", text)
    return bool(ends) and sum(1 for c in ends if "ァ" <= c <= "ヺ") / len(ends) > 0.3


def chains(text, max_tokens=3, max_chars=16):
    """文の中の名詞の連なり (接頭辞・名詞的な接尾辞を含む) と、その中の 1〜max_tokens 語の連なりを (表記, 下書きの読み) で返す。
    数を含むもの・漢字を含まないもの・指示語で始まるもの・範囲や場合で終わるものは返さない"""
    from sudachipy import SplitMode
    out = []
    for sent in re.split(r"[。\n]", text):
        if not sent.strip():
            continue
        try:
            ms = tokenizer().tokenize(sent[:4000], SplitMode.C)
        except Exception:
            continue
        run = []
        for m in list(ms) + [None]:
            if m is not None and usable(m):
                run.append((m.surface(), m.reading_form(), m.part_of_speech()))
                continue
            for i in range(len(run)):
                for j in range(i + 1, min(len(run), i + max_tokens) + 1):
                    part = run[i:j]
                    surface = "".join(p[0] for p in part)
                    if len(surface) > max_chars or len(surface) < 2 or not KANJI.search(surface):
                        continue
                    if part[0][2][0] == "接尾辞" or part[-1][2][0] == "接頭辞":
                        continue
                    if part[0][0] in HEAD_STOP or part[-1][0] in TAIL_STOP:
                        continue
                    if re.search(r"[0-9０-９A-Za-zＡ-Ｚａ-ｚ()（）「」『』、・]", surface) or not plausible(surface):
                        continue
                    out.append((surface, "".join(p[1] for p in part), j - i))
            run = []
    return out
