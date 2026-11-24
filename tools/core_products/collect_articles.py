"""2026-11-11 C: ソフト・アプリ・サービスの製品名の表 (english_products.txt・extra_dictionaries/core_products.tsv) の材料 1。
jawiki のダンプ (page・linktarget・categorylinks を gakumon47 の sql_to_tsv.py で TSV にしたもの) で、ソフトウェア・アプリケーションソフトウェア・
ウェブサービス・オンラインサービスのカテゴリから、ソフト・アプリ・サービスの名のカテゴリだけをたどり (人・グループ・作品へ広がらないように)、
そこに入る記事 (転送でないもの) を集め、月の閲覧数のダンプ (4 か月) の月の平均を付ける。
出力 TSV: 記事の題<TAB>月の平均の閲覧数<TAB>入っているカテゴリ (| 区切り)
使い方: python3 -I collect_articles.py <jawiki の tsv の置き場> <月の閲覧数の置き場 (202601.tsv ...)> <out.tsv> [--depth 6]"""
import collections, re, sys
W, M, out_p = sys.argv[1:4]
DEPTH = int(sys.argv[sys.argv.index("--depth") + 1]) if "--depth" in sys.argv else 6
ROOTS = ["ソフトウェア", "アプリケーションソフトウェア", "ウェブサービス", "オンラインサービス"]
# たどるカテゴリの名 (頭の 4 つの下では、これに当たる名のカテゴリだけ)
SOFT = re.compile(r"ソフトウェア|ソフト$|ソフト\b|アプリケーション|アプリ|ウェブサービス|オンラインサービス|クラウド|ブラウザ|エディタ|開発環境|開発ツール|"
                  r"プログラミング言語|フレームワーク|ライブラリ|データベース|オペレーティングシステム|ミドルウェア|ゲームエンジン|オフィススイート|DAW|"
                  r"ソーシャル・ネットワーキング・サービス|検索エンジン|メッセンジャー|インスタントメッセージ|動画共有|ストリーミング|ウェブアプリ|"
                  r"電子メールクライアント|メディアプレーヤ|3DCG|CAD|ツール$|サービス$")
EXCLUDE = re.compile(r"ゲーム|作品|楽曲|人物|企業|会社|開発者|技術者|歴史|一覧|スタブ|画像|ロゴ|テンプレート|キャラクター|ウイルス|マルウェア|YouTuber|配信者|番組")
lt = {}
for l in open(f"{W}/linktarget.tsv", encoding="utf-8"):
    i, _, t = l.rstrip("\n").partition("\t"); lt[t.strip('"')] = int(i)
cat_page, title_of, redir = {}, {}, set()
for l in open(f"{W}/page.tsv", encoding="utf-8"):
    c = l.rstrip("\n").split("\t"); pid = int(c[0])
    if c[1] == "14": cat_page[pid] = c[2]
    elif c[1] == "0":
        title_of[pid] = c[2]
        if c[3] == "1": redir.add(pid)
members = collections.defaultdict(list)
for l in open(f"{W}/categorylinks.tsv", encoding="utf-8"):
    a, _, b = l.rstrip("\n").split("\t"); members[int(b)].append(int(a))
seen, queue = {}, [(r, 0) for r in ROOTS if r in lt]
while queue:
    name, d = queue.pop(0)
    if name in seen or EXCLUDE.search(name) or (d > 0 and not SOFT.search(name)): continue
    seen[name] = d
    if d >= DEPTH: continue
    for pid in members.get(lt[name], []):
        sub = cat_page.get(pid)
        if sub and sub in lt and sub not in seen: queue.append((sub, d + 1))
arts = collections.defaultdict(set)
for name in seen:
    for pid in members.get(lt[name], []):
        if pid in title_of and pid not in redir: arts[title_of[pid]].add(name)
tot = collections.Counter()
for m in ("202601", "202604", "202607", "202610"):
    for l in open(f"{M}/{m}.tsv", encoding="utf-8"):
        t, _, v = l.rstrip("\n").rpartition("\t")
        if t in arts: tot[t] += int(v)
with open(out_p, "w", encoding="utf-8", newline="\n") as f:
    for t, cs in sorted(arts.items()): f.write(f"{t.replace('_', ' ')}\t{tot[t] / 4:.0f}\t{'|'.join(sorted(cs))}\n")
print("カテゴリ", len(seen), "記事", len(arts))
