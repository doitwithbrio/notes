#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <version>" >&2
  exit 1
fi

version="$1"
repo="doitwithbrio/notes"
asset="notes_${version}_amd64.AppImage"
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

gh release download "v${version}" --repo "$repo" --pattern "$asset" --dir "$tmpdir"
checksum=$(shasum -a 256 "$tmpdir/$asset" | awk '{print $1}')
root_dir=$(CDPATH= cd -- "$(dirname "$0")" && pwd)

python3 - <<'PY' "$version" "$checksum" "$root_dir"
from pathlib import Path
import re
import sys

version = sys.argv[1]
checksum = sys.argv[2]
root = Path(sys.argv[3])

pkgbuild = root / "PKGBUILD"

pkg_text = pkgbuild.read_text()
pkg_text = re.sub(r'^pkgver=.*$', f'pkgver={version}', pkg_text, flags=re.MULTILINE)
pkg_text = re.sub(r'^pkgrel=.*$', 'pkgrel=1', pkg_text, flags=re.MULTILINE)
pkg_text = re.sub(r"^sha256sums_x86_64=\('.*'\)$", f"sha256sums_x86_64=('{checksum}')", pkg_text, flags=re.MULTILINE)
pkgbuild.write_text(pkg_text)
PY

(cd "$root_dir" && makepkg --printsrcinfo > .SRCINFO)

echo "updated PKGBUILD and .SRCINFO for v${version}"
