#!/bin/sh
# Builds the off42 engine (rust/crates/off42-mobile) for the Android app into android/app/rust-out:
#   jniLibs/<abi>/liboff42_mobile.so   for each ABI (cargo-ndk; ANDROID_NDK_HOME must point at an NDK)
#   kotlin/                            UniFFI Kotlin bindings, generated from the built library
#   host/                              the library for this machine, used by the JVM unit tests
#
# Usage: android/scripts/build-rust.sh [abi ...]     (default: arm64-v8a armeabi-v7a x86_64)
#        OFF42_SKIP_ANDROID=1 android/scripts/build-rust.sh   bindings and host library only (no NDK needed)
# Requires rustup targets aarch64-linux-android armv7-linux-androideabi x86_64-linux-android and
# `cargo install cargo-ndk`.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
rust=$(cd "$here/../rust" && pwd)
out="$here/app/rust-out"
profile=${OFF42_RUST_PROFILE:-mobile-release}
abis=${*:-arm64-v8a armeabi-v7a x86_64}

# Keep the local checkout's paths out of panic messages in the shipped library.
cargo_home=${CARGO_HOME:-$HOME/.cargo}
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$rust=/off42/rust"

triple() {
  case "$1" in
    arm64-v8a) echo aarch64-linux-android ;;
    armeabi-v7a) echo armv7-linux-androideabi ;;
    x86_64) echo x86_64-linux-android ;;
    x86) echo i686-linux-android ;;
    *) echo "Unknown ABI: $1" >&2; exit 1 ;;
  esac
}

mkdir -p "$out"
if [ "${OFF42_SKIP_ANDROID:-}" != 1 ]; then
  for abi in $abis; do
    t=$(triple "$abi")
    # The cdylib crate type is requested here only, so workspace builds never link it.
    (cd "$rust" && cargo ndk --platform 26 -t "$abi" rustc --locked -p off42-mobile --lib --profile "$profile" \
      --crate-type cdylib)
    mkdir -p "$out/jniLibs/$abi"
    cp "$rust/target/$t/$profile/liboff42_mobile.so" "$out/jniLibs/$abi/"
  done
fi

# Host library: generates the bindings (UniFFI library mode reads the interface from the compiled library) and
# lets the JVM unit tests call the real engine.
(cd "$rust" && cargo rustc --locked -p off42-mobile --lib --crate-type cdylib)
case "$(uname -s)" in
  Darwin) host_lib=liboff42_mobile.dylib ;;
  MINGW*|MSYS*|CYGWIN*) host_lib=off42_mobile.dll ;;
  *) host_lib=liboff42_mobile.so ;;
esac
rm -rf "$out/host" "$out/kotlin"
mkdir -p "$out/host"
cp "$rust/target/debug/$host_lib" "$out/host/"
(cd "$rust" && cargo run --locked -q -p off42-mobile --features cli --bin uniffi-bindgen -- generate --no-format \
  --library "$out/host/$host_lib" --language kotlin --out-dir "$out/kotlin")
find "$out" -type f -exec ls -l {} \;
