"""裁判所ウェブサイトの裁判例検索 (https://www.courts.go.jp/hanrei/) から、分野辞書 (法律) の語を集めるための文を取る。

  sample : 事件の ID (2026-10-20 の時点で 5 万台後半〜9 万台。詳細ページは /hanrei/<ID>/detail2/index.html) を無作為に選び
           (乱数の種は固定)、詳細ページ (事件名・判示事項・裁判要旨・参照法条・裁判所) と全文の PDF を取る。
           PDF は pdftotext で文字にし、文字の層が無いもの (スキャン) は使わない (scanned と記録するだけ)。ID に事件が無ければ none と記録する。
  list   : 検索結果の一覧 (30 件ずつ)。1 ページに 10 秒ほどかかり全件で 7 時間になるので、ふだんは使わない

サーバーに負担をかけないよう、要求は 1 つずつ・間隔 --interval 秒 (既定 1.5) で送る。途中で止めても続きから取れる。
断られたとき (404 のほかの 4xx。403 など) は、やり直さずにそこで止める。429・503 は Retry-After の秒数 (か日時) まで待ってからやり直し、
Retry-After が 1 時間を超えるときや、やり直しても 429 が続くときも止める (取り直す前に、サイトの条件を確かめる)。
robots.txt (2026-10-20 に確認) は公示送達のページだけを禁じている。
使い方: python3 collect_courts.py sample [--out DIR] [--count 4000] [--id-range 50500-97200]
"""
import argparse, datetime, email.utils, gzip, html, json, os, random, re, subprocess, sys, tempfile, time, urllib.error, urllib.parse, urllib.request

BASE = "https://www.courts.go.jp/hanrei/search2/index.html"
UA = "HaniWord-domain-dictionary/0.1 (+https://github.com/supertastybanana3; collects legal terms for an IME dictionary)"
QUERY = {"filter[judgeDateMode]": "2", "filter[judgeGengoFrom]": "昭和", "filter[judgeYearFrom]": "1",
         "filter[judgeGengoTo]": "令和", "filter[judgeYearTo]": "9"}


MAX_RETRY_AFTER = 3600


def retry_after(value):
    """Retry-After (秒か HTTP の日時) を、今から待つ秒に。無いか読めなければ None"""
    value = (value or "").strip()
    if value.isdigit():
        return int(value)
    try:
        when = email.utils.parsedate_to_datetime(value)
    except (TypeError, ValueError, IndexError):
        return None
    if when is None:
        return None
    if when.tzinfo is None:
        when = when.replace(tzinfo=datetime.timezone.utc)
    return max(0.0, (when - datetime.datetime.now(datetime.timezone.utc)).total_seconds())


def get(url, interval, tries=8):
    """取れなければ None (ネットワークが切れたときと 5xx は 1.5 秒・6 秒・24 秒… と待ってやり直す。合計で 1 時間ほど)。
    断られたとき (404 のほかの 4xx) は止める。429・503 は Retry-After を守って待ち、1 時間を超えるときと、やり直しても 429 のときは止める"""
    for k in range(tries):
        time.sleep(interval)
        try:
            req = urllib.request.Request(url, headers={"User-Agent": UA})
            with urllib.request.urlopen(req, timeout=60) as r:
                return r.read(), r.headers.get("Content-Type", "")
        except urllib.error.HTTPError as e:
            if e.code == 404:  # その ID に事件が無い
                return b"", "404"
            if e.code in (429, 503):
                wait = retry_after(e.headers.get("Retry-After") if e.headers else None)
                if wait is not None and wait > MAX_RETRY_AFTER:
                    sys.exit(f"stop: HTTP {e.code} with Retry-After {wait:.0f}s {url} (1 時間を超えるので止める。続きはあとで同じコマンドで)")
                if e.code == 429 and k == tries - 1:
                    sys.exit(f"stop: HTTP 429 after {tries} tries {url} (要求を減らすよう求められているので止める)")
                wait = wait if wait is not None else min(interval * (4 ** k), 900)
                print(f"wait {wait:.0f}s (HTTP {e.code}) {url}", file=sys.stderr)
                time.sleep(wait)
                continue
            if 400 <= e.code < 500:  # 断られた (403 など)。やり直さない
                sys.exit(f"stop: HTTP {e.code} {url} (断られたので止める。取り直す前に、サイトの条件を確かめる)")
            print(f"retry {k + 1} {url}: {e}", file=sys.stderr)
            time.sleep(min(interval * (4 ** k), 900))
        except Exception as e:  # 一時的な失敗は間隔を延ばしてやり直す
            print(f"retry {k + 1} {url}: {e}", file=sys.stderr)
            time.sleep(min(interval * (4 ** k), 900))
    print(f"gave up {url}", file=sys.stderr)
    return None, ""


def text_of(fragment):
    t = html.unescape(re.sub(r"<[^>]+>", "\n", fragment))
    return [line.strip() for line in t.split("\n") if line.strip()]


def parse_list(page):
    rows = []
    for m in re.finditer(r'<tr>\s*<th\s*>\s*<a href="\./\.\./(\d+)/detail(\d)/index\.html"[^>]*>([^<]*)</a>(.*?)</tr>', page, re.S):
        lines = text_of(m.group(4))
        rows.append({"id": int(m.group(1)), "detail": m.group(2), "type": m.group(3).strip(),
                     "case_no": lines[0] if lines else "", "case_name": lines[1] if len(lines) > 1 else "",
                     "lines": lines[2:], "pdf": f"hanrei-pdf-{m.group(1)}.pdf" in m.group(4)})
    total = re.search(r"([0-9,]+)件中", page)
    return rows, int(total.group(1).replace(",", "")) if total else None


def cmd_list(a):
    os.makedirs(a.out, exist_ok=True)
    path = os.path.join(a.out, "list.jsonl")
    done = set()
    if os.path.exists(path):
        for line in open(path, encoding="utf-8"):
            done.add(json.loads(line)["offset"])
    offset, total = 0, None
    with open(path, "a", encoding="utf-8") as out:
        while total is None or offset < total:
            if offset in done and total is not None:
                offset += 30
                continue
            q = dict(QUERY, offset=str(offset))
            body, _ = get(BASE + "?" + urllib.parse.urlencode(q), a.interval)
            rows, total = parse_list(body.decode("utf-8", "replace"))
            if offset not in done:
                out.write(json.dumps({"offset": offset, "rows": rows}, ensure_ascii=False) + "\n")
                out.flush()
            print(f"list {offset}/{total} rows={len(rows)}", flush=True)
            if not rows:
                break
            offset += 30


def parse_detail(page):
    lines = text_of(re.sub(r"<script.*?</script>", "", page, flags=re.S))
    keys = ["事件番号", "事件名", "裁判年月日", "法廷名", "裁判所名", "裁判種別", "結果", "判例集等巻・号・頁", "原審裁判所名",
            "判示事項", "裁判要旨", "参照法条", "全文"]
    out, cur = {}, None
    for line in lines:
        if line in keys:
            cur = line
            out.setdefault(cur, [])
            continue
        if line in ("ページ上部に戻る",):
            break
        if cur and cur != "全文":
            out[cur].append(line)
    return {k: "\n".join(v) for k, v in out.items() if v}


def pdf_text(data):
    """全文の PDF を文字にする。文字の層が無い (スキャン) なら None"""
    with tempfile.TemporaryDirectory() as d:
        p = os.path.join(d, "a.pdf")
        open(p, "wb").write(data)
        fonts = subprocess.run(["pdffonts", p], capture_output=True, text=True, errors="replace").stdout.splitlines()[2:]
        info = subprocess.run(["pdfinfo", p], capture_output=True, text=True, errors="replace").stdout
        pages = int((re.search(r"Pages:\s+(\d+)", info) or [0, 1])[1])
        text = subprocess.run(["pdftotext", "-enc", "UTF-8", p, "-"], capture_output=True).stdout.decode("utf-8", "replace")
    chars = len(re.sub(r"\s", "", text))
    if not fonts or chars < 200 * max(pages, 1) * 0.3:
        return None, pages, chars
    return text, pages, chars


def cmd_fetch(a):
    rows = {}
    for line in open(os.path.join(a.out, "list.jsonl"), encoding="utf-8"):
        for r in json.loads(line)["rows"]:
            rows[r["id"]] = r
    by_type = {}
    for r in rows.values():
        by_type.setdefault(r["type"], []).append(r)
    quota = dict(kv.split("=") for kv in a.per_type)
    rng = random.Random(20261020)
    chosen = []
    for t, rs in sorted(by_type.items()):
        rs.sort(key=lambda r: r["id"])
        n = int(quota.get(t, quota.get("*", 0)))
        chosen += rng.sample(rs, min(n, len(rs)))
    print({t: len(rs) for t, rs in by_type.items()}, "chosen", len(chosen), flush=True)
    path = os.path.join(a.out, "cases.jsonl")
    textdir = os.path.join(a.out, "text")
    os.makedirs(textdir, exist_ok=True)
    done = set()
    if os.path.exists(path):
        done = {json.loads(line)["id"] for line in open(path, encoding="utf-8")}
    with open(path, "a", encoding="utf-8") as out:
        for k, r in enumerate(chosen):
            if r["id"] in done:
                continue
            body, _ = get(f"https://www.courts.go.jp/hanrei/{r['id']}/detail{r['detail']}/index.html", a.interval)
            rec = {"id": r["id"], "type": r["type"], "detail": parse_detail(body.decode("utf-8", "replace"))}
            if r["pdf"]:
                data, ctype = get(f"https://www.courts.go.jp/assets/hanrei/hanrei-pdf-{r['id']}.pdf", a.interval)
                if data[:4] != b"%PDF":
                    rec["pdf"] = "not_pdf"
                else:
                    text, pages, chars = pdf_text(data)
                    rec.update(pages=pages, chars=chars, pdf="text" if text else "scanned")
                    if text:
                        with gzip.open(os.path.join(textdir, f"{r['id']}.txt.gz"), "wt", encoding="utf-8") as g:
                            g.write(text)
            out.write(json.dumps(rec, ensure_ascii=False) + "\n")
            out.flush()
            if k % 20 == 0:
                print(f"fetch {k}/{len(chosen)} id={r['id']} {rec.get('pdf')}", flush=True)


def cmd_sample(a):
    lo, hi = map(int, a.id_range.split("-"))
    ids = random.Random(20261020).sample(range(lo, hi + 1), a.count)
    path = os.path.join(a.out, "cases.jsonl")
    textdir = os.path.join(a.out, "text")
    os.makedirs(textdir, exist_ok=True)
    done = set()
    if os.path.exists(path):
        done = {json.loads(line)["id"] for line in open(path, encoding="utf-8")}
    with open(path, "a", encoding="utf-8") as out:
        for k, case_id in enumerate(ids):
            if case_id in done:
                continue
            body, _ = get(f"https://www.courts.go.jp/hanrei/{case_id}/detail2/index.html", a.interval)
            if body is None:
                continue  # 取れなかった (次に動かしたときにやり直す)
            page = body.decode("utf-8", "replace")
            detail = parse_detail(page)
            court = re.search(r"<title>\s*裁判例結果詳細\s*\|\s*([^<]*?)\s*</title>", page)
            rec = {"id": case_id, "court": court.group(1) if court else "", "detail": detail}
            if "事件番号" not in detail:
                rec["pdf"] = "none"
            elif f"hanrei-pdf-{case_id}.pdf" in page:
                data, _ = get(f"https://www.courts.go.jp/assets/hanrei/hanrei-pdf-{case_id}.pdf", a.interval)
                if data is None:
                    continue
                if data[:4] != b"%PDF":
                    rec["pdf"] = "not_pdf"
                else:
                    text, pages, chars = pdf_text(data)
                    rec.update(pages=pages, chars=chars, pdf="text" if text else "scanned")
                    if text:
                        with gzip.open(os.path.join(textdir, f"{case_id}.txt.gz"), "wt", encoding="utf-8") as g:
                            g.write(text)
            else:
                rec["pdf"] = "no_link"
            out.write(json.dumps(rec, ensure_ascii=False) + "\n")
            out.flush()
            if k % 20 == 0:
                print(f"sample {k}/{len(ids)} id={case_id} {rec.get('pdf')}", flush=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["list", "fetch", "sample"])
    ap.add_argument("--out", default=os.path.expanduser("~/haniword_domain/law/courts"))
    ap.add_argument("--interval", type=float, default=1.5)
    ap.add_argument("--per-type", nargs="*", default=["最高裁判例=3000", "*=700"])
    ap.add_argument("--count", type=int, default=4000)
    ap.add_argument("--id-range", default="50500-97200")
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    {"list": cmd_list, "fetch": cmd_fetch, "sample": cmd_sample}[a.cmd](a)


if __name__ == "__main__":
    main()
