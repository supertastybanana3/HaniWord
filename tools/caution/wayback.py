"""2026-11-06 (仕事 40): の参考文献の一覧のために、文献の URL ごとに Internet Archive (Wayback Machine) の写しを探し、無ければ作る。
- まず availability の API (https://archive.org/wayback/available) で、参照日に一番近い写しを探す
- 無ければ Save Page Now (https://web.archive.org/save/<URL>) で新しく写しを作る (--save を付けたときだけ)
- 送るのは公開ページの URL だけ。1 件ごとに間隔を空け (既定 探す 3 秒・作る 20 秒)、429 (多すぎる) が返ったら長く待って 1 回だけやり直す
- 作れない (取得を断られた・見つからない) ときは、その旨を書く。結果は TSV (URL・写しの URL・写しの日時・状態・メモ)。途中で止めても続きから
使い方: python3 -I wayback.py <urls.txt (1 行 1 URL、# は説明)> <out.tsv> [--date YYYYMMDD (参照日)] [--save] [--wait 3] [--save-wait 20] [--stop-on-429]
"""
import argparse, json, os, sys, time, urllib.error, urllib.parse, urllib.request

UA = "HaniWord-dict-tools/0.1 (https://github.com/supertastybanana3)"


def get(url, timeout=60):
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.status, r.geturl(), dict(r.headers), r.read()


def available(url, date):
    q = urllib.parse.urlencode({"url": url, "timestamp": date})
    _, _, _, body = get("https://archive.org/wayback/available?" + q)
    snap = json.loads(body.decode("utf-8")).get("archived_snapshots", {}).get("closest")
    if snap and snap.get("available") and str(snap.get("status", "200")).startswith("2"):
        return snap["url"].replace("http://", "https://", 1), snap["timestamp"]
    return None


def save(url):
    status, final, headers, _ = get("https://web.archive.org/save/" + url, timeout=180)
    loc = headers.get("Content-Location") or headers.get("content-location")
    snap = "https://web.archive.org" + loc if loc and loc.startswith("/web/") else final
    if "/web/" in snap:
        ts = snap.split("/web/", 1)[1].split("/", 1)[0]
        return snap, ts
    return None


def main():
    p = argparse.ArgumentParser()
    p.add_argument("urls"); p.add_argument("out")
    p.add_argument("--date", default=time.strftime("%Y%m%d"))
    p.add_argument("--save", action="store_true")
    p.add_argument("--wait", type=float, default=3.0)
    p.add_argument("--save-wait", type=float, default=20.0)
    p.add_argument("--stop-on-429", action="store_true", help="429 (多すぎる) が返ったら、やり直さずにそこで止める")
    a = p.parse_args()
    urls = [l.strip() for l in open(a.urls, encoding="utf-8") if l.strip() and not l.startswith("#")]
    done = set()
    if os.path.exists(a.out):
        # 写しのある行だけを済みとみなす (写しの無い行は、--save で頼み直す。読む側は同じ URL のあとの行を使う)
        done = {c[0] for c in (l.rstrip("\n").split("\t") for l in open(a.out, encoding="utf-8") if not l.startswith("#")) if len(c) > 1 and c[1]}
    new = not os.path.exists(a.out)
    with open(a.out, "a", encoding="utf-8") as f:
        if new:
            f.write("# URL\t写しの URL\t写しの日時 (UTC)\t状態\tメモ\n")
        for url in urls:
            if url in done or not url.startswith(("http://", "https://")):
                continue
            row = None
            for attempt in (1, 2):
                try:
                    hit = available(url, a.date)
                    if hit:
                        row = (hit[0], hit[1], "ある写し", "")
                    elif a.save:
                        time.sleep(a.save_wait)
                        made = save(url)
                        row = (made[0], made[1], "作った写し", "") if made else ("", "", "写しなし", "Save Page Now が写しの場所を返さなかった")
                    else:
                        row = ("", "", "写しなし", "まだ作っていない (--save で作る)")
                    break
                except urllib.error.HTTPError as e:
                    if e.code == 429 and a.stop_on_429:
                        print("429 が返ったので止める", url, flush=True)
                        return
                    if e.code == 429 and attempt == 1:
                        time.sleep(300)
                        continue
                    why = "Save Page Now が頼みの多さで断った。あとで作り直す" if e.code == 429 else "取得を断られた・見つからないなど"
                    row = ("", "", "写しなし", f"HTTP {e.code} ({why})")
                    break
                except Exception as e:  # 時間切れ・つながらない
                    row = ("", "", "写しなし", f"{type(e).__name__}")
                    break
            f.write("\t".join((url,) + row) + "\n"); f.flush()
            print(row[2], url, row[0], flush=True)
            time.sleep(a.wait)


main()
