#!/usr/bin/env python3
"""Move the Xray-core pin in core/xray.json to another release tag.

Reads the SHA-256 digests from the release's official `.dgst` files and
rewrites the manifest in place. Used by `.github/workflows/core-update.yml`
to open a reviewed bump PR whenever XTLS publishes a new release.

Usage: scripts/update-core-pin.py v26.4.1
"""

import json
import re
import sys
import urllib.request

MANIFEST = "core/xray.json"
DGST_URL = "https://github.com/XTLS/Xray-core/releases/download/{tag}/{file}.dgst"


def dgst_sha256(tag: str, file_name: str) -> str:
    url = DGST_URL.format(tag=tag, file=file_name)
    with urllib.request.urlopen(url, timeout=60) as response:
        text = response.read().decode("utf-8", "replace")
    match = re.search(r"SHA2-256=\s*([0-9a-f]{64})", text, re.IGNORECASE)
    if not match:
        raise SystemExit(f"no SHA2-256 digest in {url}")
    return match.group(1).lower()


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: scripts/update-core-pin.py <tag, e.g. v26.4.1>")
    tag = sys.argv[1]
    if not tag.startswith("v"):
        tag = "v" + tag
    if not re.fullmatch(r"v\d+\.\d+\.\d+", tag):
        raise SystemExit(f"unexpected release tag: {tag}")

    with open(MANIFEST, encoding="utf-8") as fh:
        manifest = json.load(fh)
    if manifest["release"] == tag:
        print(f"already pinned to {tag}")
        return

    for asset in manifest["assets"]:
        asset["sha256"] = dgst_sha256(tag, asset["file"])
        print(f"{asset['file']}: {asset['sha256'][:16]}…")
    manifest["release"] = tag
    with open(MANIFEST, "w", encoding="utf-8") as fh:
        json.dump(manifest, fh, indent=2, ensure_ascii=False)
        fh.write("\n")
    print(f"pinned {MANIFEST} → {tag}")


if __name__ == "__main__":
    main()
