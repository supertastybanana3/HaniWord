"""2026-11-07 C: 仕事 42。JMdict (CC BY-SA 4.0) の表記のうち、常用外の字 (KANJIDIC2 の grade が 1〜8 でない字) を含み、
azooKey の辞書 (と追加辞書・分野辞書の表) に同じ読みで無い表記を洗い出す (嗾ける・擽る・揶揄う・誑かす)。
- 読みの制限 (re_restr) を守る。かなだけの読み (re_nokanji) は除く。表記の印 sK (検索用)・oK (古い)・iK (不規則) の表記は除く
  (rK (まれな表記: 揶揄う) は入れる)
- 品詞 (pos)、語義に uk (普通はかなで書く) の印があるか、語義の印 (X きわどい・vulg 卑語・derog さげすむ・arch どの語義も古語か廃語)、
  JMdict の並び順 (同じ読みで先の表記ほどよく使う) と、同じ項目で前に並ぶ表記のうち辞書にあるものを残す
出力 TSV: 読み (ひらがな)<TAB>表記<TAB>品詞 (| 区切り)<TAB>uk (1/0)<TAB>同じ読みの表記の中の順<TAB>azooKey の辞書にある同じ読みの表記 (上位 3)
          <TAB>語義の印<TAB>同じ項目で前に並ぶ表記のうち辞書にあるもの (| 区切り)
使い方: python3 -I rare_kanji_find.py <JMdict_e.gz> <kanjidic2.xml.gz> <azookey_dict.tsv> <out.tsv> [追加の表 (表記<TAB>読み の TSV) ...]
"""
import gzip, re, sys
from collections import defaultdict

jm, kd, dic, out = sys.argv[1:5]
extra = sys.argv[5:]
x = gzip.open(kd, "rt", encoding="utf-8").read()
grade = {}
for c in re.findall(r"<character>(.*?)</character>", x, re.S):
    g = re.search(r"<grade>(\d+)</grade>", c)
    grade[re.search(r"<literal>(.)</literal>", c).group(1)] = int(g.group(1)) if g else 0
joyo = {k for k, v in grade.items() if 1 <= v <= 8}


def is_kanji(c):
    return "一" <= c <= "鿿" or "㐀" <= c <= "䶿" or c == "々"


def hira(s):
    return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in s)


have = defaultdict(list)  # 読み (ひらがな) → [(値, 表記)]
for line in open(dic, encoding="utf-8"):
    r, w, lc, rc, mid, v = line.rstrip("\n").split("\t")
    have[hira(r)].append((float(v), w))
surfaces = defaultdict(set)
for k, vs in have.items():
    surfaces[k] = {w for _, w in vs}
for path in extra:
    for line in open(path, encoding="utf-8"):
        c = line.rstrip("\n").split("\t")
        if line.startswith("#") or len(c) < 2:
            continue
        surfaces[hira(c[1])].add(c[0])

xml = gzip.open(jm, "rt", encoding="utf-8").read()
rows = []
for e in re.findall(r"<entry>(.*?)</entry>", xml, re.S):
    k_eles = re.findall(r"<k_ele>(.*?)</k_ele>", e, re.S)
    if not k_eles:
        continue
    kebs = []
    for k in k_eles:
        keb = re.search(r"<keb>([^<]+)</keb>", k).group(1)
        inf = set(re.findall(r"<ke_inf>&([^;]+);</ke_inf>", k))
        if inf & {"sK", "oK", "iK"}:  # 2026-11-07 06:3x: rK (まれな表記: 揶揄う) は入れる。同じ読みのいちばん下に置くので害が無い
            continue
        kebs.append(keb)
    pos = sorted(set(re.findall(r"<pos>&([^;]+);</pos>", e)))
    uk = "1" if "&uk;" in e else "0"
    flags = [f for f in ("X", "vulg", "derog") if f"&{f};" in e]
    senses = re.findall(r"<sense>(.*?)</sense>", e, re.S)
    if senses and all("&arch;" in x or "&obs;" in x for x in senses): flags.append("arch")  # どの語義も古語・廃語
    flags = ",".join(flags)
    for r_ele in re.findall(r"<r_ele>(.*?)</r_ele>", e, re.S):
        if "<re_nokanji/>" in r_ele:
            continue
        reb = hira(re.search(r"<reb>([^<]+)</reb>", r_ele).group(1))
        restr = re.findall(r"<re_restr>([^<]+)</re_restr>", r_ele)
        ok = [k for k in kebs if not restr or k in restr]
        for order, keb in enumerate(ok, start=1):
            if not any(is_kanji(c) and c not in joyo for c in keb):
                continue
            if keb in surfaces.get(reb, ()):
                continue
            top = " ".join(w for _, w in sorted(have.get(reb, []), reverse=True)[:3])
            earlier = [k0 for k0 in ok[:order - 1] if k0 in surfaces.get(reb, ())]  # 同じ項目で前に並ぶ表記 (よく使う書き方) のうち辞書にあるもの
            rows.append((reb, keb, "|".join(pos), uk, str(order), top, flags, "|".join(earlier)))
with open(out, "w", encoding="utf-8") as f:
    f.write("# 読み\t表記\t品詞\tuk\t順\tazooKey の同じ読みの表記 (上位 3)\t語義の印 (X・vulg・derog・arch)\t同じ項目で前に並ぶ表記のうち辞書にあるもの\n")
    for r in sorted(set(rows)):
        f.write("\t".join(r) + "\n")
print(len(set(rows)), "missing spellings")
