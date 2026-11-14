"""Turns requests recorded by capture.py into test fixtures.

usage: python3 make-fixtures.py <capture_dir> <fixtures_dir> <public_key> <path_prefix>...

Each envelope is re-serialized with machine-specific values replaced:
the public key becomes the test key, the given path prefixes become
/srv/app, host:port pairs become localhost:8080 (the server) or
localhost:3000 (the page), and item lengths are recomputed. Writes
<sdk>-<nn>-<item types>.envelope and a .json with the request line and
headers.
"""
import json
import os
import re
import sys

src, dst, key = sys.argv[1], sys.argv[2], sys.argv[3]
prefixes = sorted(sys.argv[4:], key=len, reverse=True)
TEST_KEY = "0123456789abcdef0123456789abcdef"
os.makedirs(dst, exist_ok=True)


def scrub(data: bytes) -> bytes:
    text = data.decode("utf-8", errors="surrogateescape")
    text = text.replace(key, TEST_KEY)
    for p in prefixes:
        text = text.replace(p, "/srv/app").replace(p.replace("/", "\\/"), "\\/srv\\/app")
        # Module names derived from paths (profiles of the Node SDK).
        text = text.replace(p.replace("/", "."), ".srv.app")
    text = re.sub(r"127\.0\.0\.1(\\?/?:|:)(\d{4,5})", lambda m: "localhost:8080" if "@" in text[max(0, m.start() - 40):m.start()] else "localhost:3000", text)
    text = re.sub(r"/tmp/rupta-smoke\.\w+", "/srv/app", text)
    # Kernel release and host name in OS contexts.
    release = os.uname().release
    text = text.replace(release, "6.8.0-generic").replace(f"Linux {os.uname().nodename} ", "Linux host ")
    return text.encode("utf-8", errors="surrogateescape")


def split_envelope(body: bytes):
    header, _, rest = body.partition(b"\n")
    items = []
    while rest:
        line, _, rest = rest.partition(b"\n")
        if not line.strip():
            continue
        h = json.loads(line)
        if "length" in h:
            n = h["length"]
            payload, rest = rest[:n], rest[n:]
            if rest.startswith(b"\n"):
                rest = rest[1:]
        else:
            payload, _, rest = rest.partition(b"\n")
        items.append((h, payload))
    return header, items


counts = {}
for name in sorted(os.listdir(src)):
    if not name.endswith(".json"):
        continue
    meta = json.load(open(os.path.join(src, name)))
    if meta["method"] != "POST" or meta.get("stored_encoding"):
        continue
    body = open(os.path.join(src, name[:-5] + ".body"), "rb").read()
    ua = meta["headers"].get("user-agent", "")
    sdk = "browser" if "Mozilla" in ua else ua.split("/")[0].replace("sentry.", "").replace("javascript.", "")
    header, items = split_envelope(body)
    out = scrub(header) + b"\n"
    types = []
    for h, payload in items:
        payload = scrub(payload)
        if "length" in h:
            h["length"] = len(payload)
        types.append(h.get("type", "unknown"))
        out += json.dumps(h, separators=(",", ":")).encode() + b"\n" + payload + b"\n"
    counts[sdk] = counts.get(sdk, 0) + 1
    base = f"{sdk}-{counts[sdk]:02}-{'-'.join(dict.fromkeys(types))}"
    open(os.path.join(dst, base + ".envelope"), "wb").write(out)
    headers = {k: scrub(v.encode()).decode() for k, v in meta["headers"].items() if k != "content-encoding"}
    json.dump({"path": meta["path"], "query": scrub(meta["query"].encode()).decode(), "headers": headers}, open(os.path.join(dst, base + ".json"), "w"), indent=1)
    print(base)
