"""(2026-10-22、M3.5) 試しのステージの domain フォルダに、オフにできない追加辞書だけを書く (分野辞書はそのまま)。
build_domain.py の write_dictionary をそのまま使う (リポジトリには何も書かない)。名前を 1 つも渡さなければ追加辞書を全部外す
使い方: python3 build_always.py <リポジトリ> <domain フォルダ> <charID.chid> [名前=TSV のパス ...]"""
import glob, os, sys
repo, out, chid_path = sys.argv[1:4]
sys.path.insert(0, os.path.join(repo, "tools", "domain"))
import build_domain, louds
for old in glob.glob(os.path.join(out, "domain_core_*")):
    os.remove(old)
chid = louds.load_chid(chid_path)
names = []
for arg in sys.argv[4:]:
    name, path = arg.split("=", 1)
    entries, nodes = build_domain.write_dictionary(path, "domain_" + name, chid, out)
    print(f"{name}: {len(entries)} entries, {nodes} nodes")
    names.append(name)
if names:
    with open(os.path.join(out, "always.txt"), "w", encoding="utf-8", newline="\n") as f:
        f.write("# 試し\n" + "".join(n + "\n" for n in names))
elif os.path.exists(os.path.join(out, "always.txt")):
    os.remove(os.path.join(out, "always.txt"))
