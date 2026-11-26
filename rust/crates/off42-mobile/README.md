# off42-mobile

UniFFI bindings over the off42 cores for the native mobile apps: Kotlin for Android (`android/`, phase 5) and Swift for
iPadOS/iOS (phase 6). See [ARCHITECTURE.md](../../../ARCHITECTURE.md), decisions D10 and D11.

## API

The capability set is the web version's. Every exported function calls the plain Rust API of
[`off42-wasm`](../off42-wasm) (on native targets that crate is an ordinary library; its wasm-bindgen exports exist only
for wasm32) and returns typed records instead of JSON. Bytes in, bytes out; no paths, network or threads.

| Area | Functions |
| --- | --- |
| Files | `file_kind`, `normalized_extension`, `save_formats`, `mime_type`, `version` |
| Documents | `document_convert`, `document_html`, `document_info` |
| Spreadsheets | `spreadsheet_open` (→ `Workbook`/`Sheet`/`Cell`), `spreadsheet_convert`, `spreadsheet_values_csv` |
| Presentations | `presentation_outline` (→ `Slide`/`SlideObject`), `slide_title_object`, `presentation_convert` |
| PDF | `pdf_info` (→ `PdfInfo`) |
| Codes, labels, marks | `barcode_png`, `barcode_svg`, `label_stocks`, `parse_label_lines`, `label_sheet_pdf`, `product_marks`, `product_mark_svg`, `product_mark_png` |
| Signatures, stamps | `signature_options`, `signature_designs`, `signature_png`, `stamp_templates`, `stamp_png` |

Errors are `Off42Error` (Kotlin `Off42Exception`, Swift `Off42Error`): `UnsupportedFormat`, `PackagePartsWouldBeLost`
(ask the user, then convert again with `drop_package_parts`), `InvalidInput` and `Failed`, each with the core's English
message. Calls are synchronous; clients run them off the UI thread. Seeds are `u64` supplied by the client from a
secure random source.

Nothing is duplicated: the web, Android and iOS clients run the same off42-wasm functions. Moving that shared layer
into a crate of its own (for example `off42-api`) would change the source hash that `web/scripts/site-build.mjs`
checks for the published web app, so it is left for a change that also republishes `website/public/app`.

## Building

Workspace builds compile only the Rust library (`cargo test -p off42-mobile` runs anywhere). The platform libraries
are requested per build with `--crate-type`, as for off42-wasm:

- Android: [`android/scripts/build-rust.sh`](../../../android/scripts/build-rust.sh) (cargo-ndk, `cdylib`).
- iPadOS/iOS: [`rust/scripts/build-mobile-apple.sh`](../../scripts/build-mobile-apple.sh) builds `staticlib`s for
  `aarch64-apple-ios` and `aarch64-apple-ios-sim`, generates `Off42Mobile.swift`, `Off42MobileFFI.h` and the module
  map, and with Xcode creates `Off42MobileFFI.xcframework` in `rust/build/apple`. CI runs it on `macos-latest` and
  type-checks the Swift file against the simulator SDK.

Bindings are generated in UniFFI library mode from a built library by the crate's own `uniffi-bindgen` binary
(`--features cli`), so the generator always matches the pinned `uniffi` version (0.33.0). Package and module names are
in [uniffi.toml](uniffi.toml): Kotlin `cc.off42.mobile`, Swift `Off42Mobile`.
