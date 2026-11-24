"""2026-11-07 C: 仕事 44。Wikidata の MediaWiki の API (www.wikidata.org/w/api.php) を API:Etiquette に沿って呼ぶ小さな道具
(連絡先つきの名乗り・1 つずつ・1 秒あける・maxlag=5・429 と maxlag なら待つ)。query.wikidata.org/sparql は robots.txt が断るので使わない"""
import json, time, urllib.error, urllib.parse, urllib.request
UA = "HaniWord-dict-tools/0.1 (https://github.com/supertastybanana3)"
API = "https://www.wikidata.org/w/api.php"
def call(**params):
    params.update(format="json", formatversion="2", maxlag="5")
    url = API + "?" + urllib.parse.urlencode(params)
    for attempt in range(8):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": UA}), timeout=60) as r:
                data = json.loads(r.read().decode("utf-8"))
            if data.get("error", {}).get("code") == "maxlag":
                time.sleep(int(data["error"].get("lag", 5)) + 5); continue
            time.sleep(1.0)
            return data
        except urllib.error.HTTPError as e:
            if e.code == 429:
                time.sleep(int(e.headers.get("Retry-After") or 60)); continue
            raise
    raise RuntimeError("too many retries: " + url)
