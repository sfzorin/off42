#!/bin/sh
# Cross-build a portable Windows x64 archive. Requires Rust target, MinGW-w64 and Python 3.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
TARGET=x86_64-pc-windows-gnu
OUT="$ROOT/build/rust-port-windows"
command -v x86_64-w64-mingw32-gcc >/dev/null
command -v x86_64-w64-mingw32-objdump >/dev/null
cd "$ROOT/rust"
command -v x86_64-w64-mingw32-windres >/dev/null
mkdir -p "$OUT"
(cd scripts && x86_64-w64-mingw32-windres windows-app.rc -O coff -o "$OUT/windows-app-res.o")
RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=$OUT/windows-app-res.o" cargo build --locked --release --target "$TARGET" -p img42 -p doc42 -p xls42 -p ppt42 -p pdf42 -p scn42
mkdir -p "$OUT"
STAGE=$(mktemp -d "$OUT/.package.XXXXXXXX")
trap 'rm -rf "$STAGE"' EXIT HUP INT TERM
mkdir -p "$STAGE/Suite42/licenses"
for app in img42 doc42 xls42 ppt42 pdf42 scn42; do
    NAME=$(printf '%s' "$app" | tr '[:lower:]' '[:upper:]')
    cp "target/$TARGET/release/$app.exe" "$STAGE/Suite42/$NAME.exe"
    x86_64-w64-mingw32-objdump -p "$STAGE/Suite42/$NAME.exe" > "$OUT/$NAME-objdump.txt"
    cp "$STAGE/Suite42/$NAME.exe" "$OUT/$NAME.exe"
done
cp "$ROOT/LICENSE" "$STAGE/Suite42/licenses/Suite42-LICENSE"
python3 scripts/fetch-pdfium.py "$STAGE/Suite42"
cp crates/doc42-layout-probe/fixtures/OFL.txt "$STAGE/Suite42/licenses/NotoSans-OFL.txt"
cat > "$STAGE/Suite42/README.txt" <<'TEXT'
Suite42 for Windows x64 — preliminary build

PDF42 uses the included PDFium runtime. SCN42 OCR requires a separately
installed Tesseract executable and language data; these are not included.
This cross-built package has not been validated on Windows.
TEXT
cargo metadata --locked --format-version 1 --filter-platform "$TARGET" > "$STAGE/metadata.json"
python3 scripts/collect-licenses.py "$STAGE/metadata.json" "$STAGE/Suite42/licenses"
RUST_LICENSES="$(rustc --print sysroot)/share/doc/rust/licenses"
if [ -d "$RUST_LICENSES" ]; then cp -R "$RUST_LICENSES" "$STAGE/Suite42/licenses/rust-toolchain"; fi
rm -f "$OUT/Suite42-windows-x64.zip"
(cd "$STAGE" && zip -qr "$OUT/Suite42-windows-x64.zip" Suite42)
shasum -a 256 "$OUT"/*.exe "$OUT/Suite42-windows-x64.zip"
