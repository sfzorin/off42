#!/bin/sh
# Cross-build a portable Windows x64 archive. Requires Rust target, MinGW-w64 and Python 3.
set -eu
: "${OFF42_OCR_SDK:?GNU packaging needs a cross-built static OCR SDK; use MSVC scripts/build-ocr.ps1 for verified Windows builds}"
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
mkdir -p "$STAGE/off42/licenses"
for app in img42 doc42 xls42 ppt42 pdf42 scn42; do
    NAME=$(printf '%s' "$app" | tr '[:lower:]' '[:upper:]')
    cp "target/$TARGET/release/$app.exe" "$STAGE/off42/$NAME.exe"
    x86_64-w64-mingw32-objdump -p "$STAGE/off42/$NAME.exe" > "$OUT/$NAME-objdump.txt"
    cp "$STAGE/off42/$NAME.exe" "$OUT/$NAME.exe"
done
cp "$ROOT/LICENSE" "$STAGE/off42/licenses/off42-LICENSE"
python3 scripts/fetch-pdfium.py "$STAGE/off42"
python3 scripts/fetch-ocr-models.py "$STAGE/off42"
cp -R "$OFF42_OCR_SDK/licenses" "$STAGE/off42/licenses/ocr/native"
cp crates/doc42-layout-probe/fixtures/OFL.txt "$STAGE/off42/licenses/NotoSans-OFL.txt"
python3 scripts/audit-windows-runtime.py --bin-dir "$STAGE/off42" --output "$OUT/runtime-dependencies.json"
cat > "$STAGE/off42/README.txt" <<'TEXT'
off42 for Windows x64 — preliminary build

PDF42 uses the included PDFium runtime. PDF42 and SCN42 use the included
offline OCR models; retain the complete ocr directory beside the apps.
This cross-built package has not been validated on Windows.
TEXT
cargo metadata --locked --format-version 1 --filter-platform "$TARGET" > "$STAGE/metadata.json"
python3 scripts/collect-licenses.py "$STAGE/metadata.json" "$STAGE/off42/licenses"
RUST_LICENSES="$(rustc --print sysroot)/share/doc/rust/licenses"
if [ -d "$RUST_LICENSES" ]; then cp -R "$RUST_LICENSES" "$STAGE/off42/licenses/rust-toolchain"; fi
rm -f "$OUT/off42-windows-x64.zip"
(cd "$STAGE" && zip -qr "$OUT/off42-windows-x64.zip" off42)
shasum -a 256 "$OUT"/*.exe "$OUT/off42-windows-x64.zip"
