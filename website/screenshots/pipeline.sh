#!/bin/zsh
# Full screenshot pipeline for the given languages (comma-separated): documents, scanner input, app shots, WebP.
set -e
HERE=${0:A:h}
REPO=${OFF42_REPO:-${HERE:h:h}}
export OFF42_WEBSITE=${OFF42_WEBSITE:-$REPO/website}
LANGS=${1:?Usage: pipeline.sh language[,language...]}
if [[ -d "$HERE/samples" ]]; then DEFAULT_SAMPLES="$HERE/samples"; else DEFAULT_SAMPLES="$HERE/../samples"; fi
export OFF42_SAMPLES=${OFF42_SAMPLES:-$DEFAULT_SAMPLES}
[[ -x "$REPO/.build/debug/PDF42" ]] || { print -u2 "Missing PDF42 binary: $REPO/.build/debug/PDF42"; exit 1; }
[[ -d "$OFF42_SAMPLES" ]] || { print -u2 "Missing samples: $OFF42_SAMPLES"; exit 1; }
cd "$HERE"
"$REPO/.build/debug/PDF42" --site-samples "$OFF42_SAMPLES" docs
python3 make_docs.py docs docs "$OFF42_WEBSITE/public/assets/office-still-life.png" "$OFF42_SAMPLES" ${(s:,:)LANGS}
python3 scanner_input.py ${(s:,:)LANGS}
SHOT_LANGS=$LANGS python3 run_shots.py
python3 encode.py ${(s:,:)LANGS}
