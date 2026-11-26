#!/bin/sh
# Builds the off42 engine (crates/off42-mobile) for iPadOS/iOS: static libraries for devices and the simulator,
# UniFFI Swift bindings and, when Xcode is available, an XCFramework for the SwiftUI app (ARCHITECTURE.md D11).
#
# Output in rust/build/apple:
#   swift/Off42Mobile.swift, swift/Off42MobileFFI.h, swift/Off42MobileFFI.modulemap
#   lib/<target>/liboff42_mobile.a
#   Off42MobileFFI.xcframework   (needs xcodebuild)
#
# Requires macOS with Xcode and rustup targets aarch64-apple-ios aarch64-apple-ios-sim.
set -eu
rust=$(cd "$(dirname "$0")/.." && pwd)
out="$rust/build/apple"
profile=${OFF42_RUST_PROFILE:-mobile-release}
targets=${OFF42_APPLE_TARGETS:-aarch64-apple-ios aarch64-apple-ios-sim}

cargo_home=${CARGO_HOME:-$HOME/.cargo}
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$rust=/off42/rust"
export IPHONEOS_DEPLOYMENT_TARGET=${IPHONEOS_DEPLOYMENT_TARGET:-16.0}

cd "$rust"
rm -rf "$out"
mkdir -p "$out/swift"
for target in $targets; do
  cargo rustc --locked -p off42-mobile --lib --profile "$profile" --target "$target" --crate-type staticlib
  mkdir -p "$out/lib/$target"
  cp "target/$target/$profile/liboff42_mobile.a" "$out/lib/$target/"
done

# Bindings come from a host build of the same crate (library mode reads the exported interface).
cargo rustc --locked -p off42-mobile --lib --crate-type cdylib
cargo run --locked -q -p off42-mobile --features cli --bin uniffi-bindgen -- generate --no-format \
  --library target/debug/liboff42_mobile.dylib --language swift \
  --out-dir "$out/swift"

if command -v xcodebuild > /dev/null 2>&1 && xcodebuild -version > /dev/null 2>&1; then
  headers="$out/headers"
  mkdir -p "$headers"
  cp "$out/swift/Off42MobileFFI.h" "$headers/"
  cp "$out/swift/Off42MobileFFI.modulemap" "$headers/module.modulemap"
  set --
  for target in $targets; do
    set -- "$@" -library "$out/lib/$target/liboff42_mobile.a" -headers "$headers"
  done
  xcodebuild -create-xcframework "$@" -output "$out/Off42MobileFFI.xcframework"
fi
find "$out" -maxdepth 3 -exec ls -ld {} \;
