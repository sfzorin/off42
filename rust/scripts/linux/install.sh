#!/bin/sh
# Install the off42 Linux preview for the current user (no root needed).
# Copies the apps to ~/.local/opt/off42, links them into ~/.local/bin and adds desktop entries.
set -eu
src=$(cd "$(dirname "$0")" && pwd)
prefix=${XDG_DATA_HOME:-$HOME/.local/share}
dest=$HOME/.local/opt/off42
mkdir -p "$dest" "$HOME/.local/bin" "$prefix/applications"
cp -R "$src"/. "$dest"/
entry() {
  # name, comment, MIME types
  ln -sf "$dest/$1" "$HOME/.local/bin/$1"
  cat > "$prefix/applications/off42-$(echo "$1" | tr 'A-Z' 'a-z').desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=$1
Comment=$2 (off42 Linux preview)
Exec=$dest/$1 %f
Terminal=false
Categories=Office;
MimeType=$3
DESKTOP
}
entry PDF42 "PDF documents" "application/pdf;"
entry DOC42 "Text documents" "application/vnd.openxmlformats-officedocument.wordprocessingml.document;application/vnd.oasis.opendocument.text;application/rtf;text/markdown;text/plain;"
entry XLS42 "Spreadsheets" "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet;application/vnd.oasis.opendocument.spreadsheet;text/csv;"
entry PPT42 "Presentations" "application/vnd.openxmlformats-officedocument.presentationml.presentation;application/vnd.oasis.opendocument.presentation;"
entry IMG42 "Images" "image/png;image/jpeg;image/tiff;image/gif;image/webp;image/bmp;"
entry SCN42 "Scans" ""
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$prefix/applications" || true
echo "Installed to $dest. Apps are in your menu and ~/.local/bin (PDF42, DOC42, ...)."
