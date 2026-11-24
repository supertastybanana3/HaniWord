# 2026-10-19 Claude: read only the text column of the first row groups of a remote FineWeb-2 parquet file
# over HTTP range requests (the line here is ~3 MB/s, so whole 4.85 GB files are too slow), and write the
# documents to a text file until about TARGET words. Documents are separated by a blank line.
# Usage: fetch_fw2.py URL OUT.txt TARGET_WORDS
import sys, time, urllib.request
import pyarrow.parquet as pq


class RangeFile:
    """Seekable read-only file over HTTP range requests (pyarrow reads the footer, then each column chunk)."""

    def __init__(self, url):
        self.url, self.pos, self.fetched = url, 0, 0
        req = urllib.request.Request(url, method="HEAD")
        with urllib.request.urlopen(req, timeout=60) as r:
            self.size = int(r.headers["Content-Length"])

    def seek(self, offset, whence=0):
        self.pos = offset if whence == 0 else (self.pos + offset if whence == 1 else self.size + offset)
        return self.pos

    def tell(self):
        return self.pos

    def seekable(self):
        return True

    def readable(self):
        return True

    @property
    def closed(self):
        return False

    def read(self, n=-1):
        if n is None or n < 0:
            n = self.size - self.pos
        n = min(n, self.size - self.pos)
        if n <= 0:
            return b""
        for attempt in range(6):
            try:
                req = urllib.request.Request(self.url, headers={"Range": f"bytes={self.pos}-{self.pos + n - 1}"})
                with urllib.request.urlopen(req, timeout=300) as r:
                    data = r.read()
                if len(data) != n:
                    raise IOError(f"short read {len(data)}/{n}")
                break
            except Exception as e:  # retry flaky connections
                if attempt == 5:
                    raise
                print(f"# retry {attempt + 1}: {e}", file=sys.stderr, flush=True)
                time.sleep(5 * (attempt + 1))
        self.pos += n
        self.fetched += n
        return data


url, out, target = sys.argv[1], sys.argv[2], int(sys.argv[3])
f = RangeFile(url)
pf = pq.ParquetFile(f)
words = docs = 0
t0 = time.time()
with open(out, "w", encoding="utf-8") as w:
    for g in range(pf.num_row_groups):
        table = pf.read_row_group(g, columns=["text"])
        for text in table.column("text").to_pylist():
            w.write(text.replace("\r", ""))
            w.write("\n\n")
            words += len(text.split())
            docs += 1
        print(f"# group {g + 1}/{pf.num_row_groups}: {docs} docs, {words / 1e6:.1f}M words, "
              f"{f.fetched / 1e6:.0f} MB fetched, {time.time() - t0:.0f}s", file=sys.stderr, flush=True)
        if words >= target:
            break
print(f"{out}\t{docs}\t{words}\t{f.fetched}", flush=True)
