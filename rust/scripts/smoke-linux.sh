#!/usr/bin/env bash
# Headless smoke checks for the experimental Linux build.
# Usage: scripts/smoke-linux.sh BIN_DIR
# BIN_DIR holds PDF42, DOC42, XLS42, PPT42, IMG42 and SCN42 (any case).
set -euo pipefail
bin_dir=$(cd "$1" && pwd)
here=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
app() {
  local name=$1; shift
  if [ -x "$bin_dir/$name" ]; then "$bin_dir/$name" "$@"; else "$bin_dir/$(echo "$name" | tr 'A-Z' 'a-z')" "$@"; fi
}
expect() { grep -q -- "$1" || { echo "FAIL: expected '$1' ($2)"; exit 1; }; }

for a in PDF42 DOC42 XLS42 PPT42 IMG42 SCN42; do
  app "$a" --version | expect "$a" "$a --version"
  app "$a" --formats > /dev/null
done

printf '# Heading\n\nLinux *preview* text.\n' > "$work/a.md"
app DOC42 --convert "$work/a.md" "$work/a.docx"
app DOC42 --convert "$work/a.md" "$work/a.odt"
app DOC42 --convert "$work/a.md" "$work/a.rtf"
app DOC42 --text "$work/a.docx" | expect "Linux preview text" "DOC42 DOCX round trip"
app DOC42 --text "$work/a.odt" | expect "Heading" "DOC42 ODT round trip"

printf 'name,value\nalpha,2\nbeta,=B2*21\n' > "$work/s.csv"
app XLS42 --convert "$work/s.csv" "$work/s.xlsx"
app XLS42 --convert "$work/s.xlsx" "$work/s.ods"
app XLS42 --table "$work/s.xlsx" | expect "42" "XLS42 formula through XLSX"
app XLS42 --table "$work/s.ods" | expect "beta" "XLS42 ODS round trip"

app IMG42 --convert "$here/crates/win42/fixtures/ocr-english.png" "$work/i.jpg"
app IMG42 --convert "$work/i.jpg" "$work/i.png"
app IMG42 --items "$work/i.png" | expect "Frame 1" "IMG42 JPEG/PNG round trip"

pdf="$here/apps/pdf42/fixtures/page-export.pdf"
app PDF42 --items "$pdf" | expect "Page 3" "PDF42 page list"
app PDF42 --convert "$pdf" "$work/copy.pdf"
app PDF42 --export 80 "$pdf" "$work/page.pdf"
app PDF42 --items "$work/page.pdf" | expect "Page 1" "PDF42 page extraction"
app PDF42 --export 81 "$pdf" "$work/pages.zip"
test -s "$work/pages.zip" || { echo "FAIL: PDF42 split ZIP"; exit 1; }

echo "Linux smoke checks passed"
