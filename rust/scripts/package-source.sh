#!/bin/sh
# Create a source handoff; run again after documentation updates.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
OUT="$ROOT/build/rust-port-windows"
mkdir -p "$OUT"
rm -f "$OUT/off42-windows-source.zip"
cd "$ROOT"
zip -qr "$OUT/off42-windows-source.zip" rust .cargo/config.toml .github/workflows/rust-windows.yml LICENSE README.md DESIGN.md -x 'rust/target/*' 'rust/build/*' 'rust/**/.DS_Store' 'rust/scripts/windows-app.res' 'rust/**/AGENTS.md' 'rust/**/STATUS.md'
shasum -a 256 "$OUT/off42-windows-source.zip"
