#!/bin/sh
# Builds the off42-wasm crate for the browser and generates its JavaScript bindings into web/src/wasm.
# Requires: rustup target wasm32-unknown-unknown, and wasm-bindgen-cli matching the crate's wasm-bindgen
# version (see web/README.md).
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
rust="$here/../rust"
profile=${OFF42_WASM_PROFILE:-wasm-release}

expected=$(sed -n 's/^wasm-bindgen = "=\(.*\)"$/\1/p' "$rust/crates/off42-wasm/Cargo.toml")
actual=$(wasm-bindgen --version | awk '{print $2}')
if [ "$expected" != "$actual" ]; then
  echo "wasm-bindgen-cli $actual does not match the crate's wasm-bindgen $expected:" >&2
  echo "  cargo install wasm-bindgen-cli --version $expected --locked" >&2
  exit 1
fi

# The cdylib crate type is requested here only, so native workspace builds never link a cdylib.
cargo rustc --manifest-path "$rust/Cargo.toml" --locked -p off42-wasm --lib \
  --profile "$profile" --target wasm32-unknown-unknown --crate-type cdylib

rm -rf "$here/src/wasm"
wasm-bindgen --target web --out-dir "$here/src/wasm" \
  "$rust/target/wasm32-unknown-unknown/$profile/off42_wasm.wasm"
ls -l "$here/src/wasm"
