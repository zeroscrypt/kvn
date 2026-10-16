#!/usr/bin/env bash
# Fetch and verify the pinned Xray-core release (pin: core/xray.json).
#
# Usage:
#   scripts/fetch-xray.sh                  # .tools/xray-<platform>/xray for this host
#   scripts/fetch-xray.sh --tools [target] # target = platform tag or "all"
#   scripts/fetch-xray.sh --bundle [triple]# sidecar binary for `tauri build`
#
# Modes:
#   --tools   local runs and tests: result in .tools/xray-<platform>/ (gitignored)
#   --bundle  the binary Tauri `externalBin` expects:
#             apps/desktop/src-tauri/binaries/xray-<triple>[.exe] (gitignored)
#
# core/xray.json is the single source of truth (release tag, file names,
# SHA-256 digests from the release `.dgst` files). Every download is verified
# against the pinned digest before anything is extracted — a swapped upload
# fails here exactly like it fails inside the app.
set -euo pipefail
cd "$(dirname "$0")/.."

MANIFEST="core/xray.json"
CACHE=".tools/cache"

die() { echo "error: $*" >&2; exit 1; }

PY="$(command -v python3 || command -v python || true)"
[[ -n "$PY" ]] || die "python3 (or python) is required by this script"

# asset_info <platform-tag-or-triple> → "release|platform|triple|file|sha256"
asset_info() {
  "$PY" - "$1" <<'PY'
import json, sys
sel = sys.argv[1]
with open("core/xray.json", encoding="utf-8") as fh:
    manifest = json.load(fh)
for asset in manifest["assets"]:
    if sel in (asset["platform"], asset["triple"]):
        print("|".join([
            manifest["release"], asset["platform"], asset["triple"],
            asset["file"], asset["sha256"],
        ]))
        break
else:
    sys.exit(f"no pin for {sel!r} in core/xray.json")
PY
}

manifest_release() {
  "$PY" -c 'import json; print(json.load(open("core/xray.json"))["release"])'
}

host_platform() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) echo macos-arm64 ;;
    Darwin-x86_64) echo macos-amd64 ;;
    Linux-x86_64) echo linux-amd64 ;;
    Linux-aarch64|Linux-arm64) echo linux-arm64 ;;
    MINGW*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64) echo windows-amd64 ;;
    *) die "unsupported host: $(uname -s)/$(uname -m) — pass an explicit target" ;;
  esac
}

host_triple() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) echo aarch64-apple-darwin ;;
    Darwin-x86_64) echo x86_64-apple-darwin ;;
    Linux-x86_64) echo x86_64-unknown-linux-gnu ;;
    Linux-aarch64|Linux-arm64) echo aarch64-unknown-linux-gnu ;;
    MINGW*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64) echo x86_64-pc-windows-msvc ;;
    *) die "unsupported host: $(uname -s)/$(uname -m) — pass an explicit triple" ;;
  esac
}

digest_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

download_verified() { # <file> <sha256> <dest>
  local file="$1" sha="$2" dest="$3"
  local url="https://github.com/XTLS/Xray-core/releases/download/$(manifest_release)/${file}"
  echo "downloading ${file} ($(manifest_release))…"
  curl -fsSL --retry 3 -o "$dest" "$url" || die "download failed: $url"
  local actual
  actual="$(digest_of "$dest")"
  [[ "$actual" == "$sha" ]] || die "SHA-256 mismatch for ${file}: expected ${sha}, got ${actual}"
}

extract_xray() { # <zip> <dest-file>
  "$PY" - "$1" "$2" <<'PY'
import os, sys, zipfile
zip_path, dest = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(zip_path) as archive:
    for info in archive.infolist():
        name = info.filename.replace("\\", "/").rsplit("/", 1)[-1]
        if name in ("xray", "xray.exe") and not info.is_dir():
            with open(dest, "wb") as out:
                out.write(archive.read(info))
            break
    else:
        sys.exit(f"no xray binary inside {zip_path}")
os.chmod(dest, 0o755)
PY
}

fetch_one() { # <platform-tag-or-triple> <dest-file>
  local info file sha zip
  info="$(asset_info "$1")"
  file="$(echo "$info" | cut -d'|' -f4)"
  sha="$(echo "$info" | cut -d'|' -f5)"
  mkdir -p "$CACHE" "$(dirname "$2")"
  zip="$CACHE/$file"
  if [[ -f "$zip" ]] && [[ "$(digest_of "$zip")" == "$sha" ]]; then
    echo "using cached ${file}"
  else
    download_verified "$file" "$sha" "$zip"
  fi
  extract_xray "$zip" "$2"
  echo "✓ $2"
}

case "${1:-}" in
  --bundle)
    triple="${2:-$(host_triple)}"
    case "$triple" in
      *-pc-windows-msvc) dest="apps/desktop/src-tauri/binaries/xray-${triple}.exe" ;;
      *) dest="apps/desktop/src-tauri/binaries/xray-${triple}" ;;
    esac
    fetch_one "$triple" "$dest"
    ;;
  --tools)
    target="${2:-$(host_platform)}"
    if [[ "$target" == "all" ]]; then
      "$PY" -c 'import json; [print(a["platform"]) for a in json.load(open("core/xray.json"))["assets"]]' |
        while IFS= read -r platform; do
          fetch_one "$platform" ".tools/xray-${platform}/xray"
        done
    else
      fetch_one "$target" ".tools/xray-${target}/xray"
    fi
    ;;
  "")
    fetch_one "$(host_platform)" ".tools/xray-$(host_platform)/xray"
    ;;
  *)
    die "unknown argument: $1 (see usage at the top of scripts/fetch-xray.sh)"
    ;;
esac
