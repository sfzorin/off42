#!/bin/sh
# Rebuilds the web app (wasm engine + Vite bundle) and copies it to website/public/app, which the site serves at
# https://off42.cc/app/. Cloudflare's build has no Rust toolchain, so the built files are committed; CI fails
# when they are out of date with web/ or the off42-wasm crate (web/scripts/site-build.mjs check).
# Requirements are those of `npm run build:wasm` (see web/README.md). SKIP_WASM=1 reuses src/wasm as built.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
site="$here/../website/public/app"
cd "$here"

[ -f node_modules/.package-lock.json ] || npm ci
if [ "${SKIP_WASM:-}" != 1 ] || [ ! -f src/wasm/off42_wasm_bg.wasm ]; then
  npm run build:wasm
fi
npm run build
npm test

rm -rf "$site"
mkdir -p "$site"
cp -R dist/. "$site/"
node scripts/site-build.mjs record "$site" > /dev/null
node scripts/site-build.mjs check "$site"
du -sh "$site"
echo "Published to website/public/app. Run the browser test against it: npm run test:ui -- --dir ../website/public/app"
