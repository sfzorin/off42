# off42 web (groundwork)

A thin web interface over the off42 Rust cores compiled to WebAssembly (ARCHITECTURE.md, decisions D4 and D5).
Files are read with the browser's file picker, processed inside the tab and offered back as downloads. Nothing is
uploaded: the built page carries a Content Security Policy with `connect-src 'self'`, and the tests check that no
request leaves the page's own origin. This is not deployed anywhere yet and is not linked from off42.cc.

## What it does

**Files** — open a local file:

| Input | Shown as | Save as |
| --- | --- | --- |
| DOCX, ODT, RTF, Markdown, HTML, text | HTML preview (sandboxed, no scripts), word count | DOCX, ODT, RTF, Markdown, HTML, text |
| XLSX, ODS, XLS (read-only), CSV | Sheets with calculated, formatted values | XLSX, ODS, CSV, displayed values of a sheet as CSV |
| PPTX, ODP | Text outline of the slides (titles, text, tables, notes) | PPTX, ODP |
| PDF | Page count, metadata, bookmarks, attachments, form fields | — |

Conversions use the cores' own encoders and their fidelity rules: a conversion that would lose content the target
format cannot carry is refused with the core's message (for example bookmarks to Markdown, or plain text from a
formatted document). Word and OpenDocument files keep opaque package parts (settings, unknown styles) that only a
same-format save can preserve; converting them to another format asks first and then drops only those parts.
doc42-core refuses digitally signed DOCX files rather than silently dropping the signature.

**Labels & codes** — QR, Data Matrix, Code 128 and Aztec as PNG (300 dpi) or SVG; A4 label sheets as PDF from the
built-in stock catalogue or a custom size; the product mark catalogue (CE, UKCA, recycling, handling, …) as SVG or
PNG (600 dpi).

**Signatures & stamps** — generated handwritten signatures (form, pen, ink; three designs per run, new impressions on
request) and stamps from the stamp catalogue, as transparent PNG. Randomness comes from Web Crypto; the date in date
stamps comes from the browser.

### Limitations

- No page rendering or PDF output for documents, spreadsheets or presentations: the cores have no PDF writer for
  them (the desktop apps print through the operating system), and PDF page rendering needs PDFium for wasm32.
- Presentations show a text outline, not rendered slides.
- Stamps use only the bundled Noto Sans font (Latin, Cyrillic, Greek); stamp templates for Chinese, Japanese,
  Korean, Arabic or Hebrew text need fonts that a browser does not expose to WebAssembly.
- All work runs on the main thread; very large files block the page while they are processed.
- English interface only.
- The engine is one wasm file of about 16 MB (about 6 MB gzip-compressed).

## Layout

- `rust/crates/off42-wasm` — the bytes-in/bytes-out API (plain Rust, tested natively with `cargo test`) and its
  wasm-bindgen exports (`src/bindings.rs`, compiled only for `wasm32`).
- `web/src` — the page (`index.html`, `src/main.ts`, `src/style.css`); `src/wasm` is generated and not committed.
- `web/test/smoke.mjs` — Node loads the wasm package and exercises every export on fixtures.
- `web/test/offline.mjs` — checks that the production build is self-contained (CSP, no remote URLs).
- `web/test/ui.mjs` — headless Chromium (Playwright) drives the built page: opens files, downloads conversions,
  generates codes, label sheets, marks, signatures and stamps, and fails on any request outside the page's origin.

## Build

Tested with Rust 1.98 (stable), Node 22, wasm-bindgen 0.2.130, Vite 8.3.8, TypeScript 5.9.3 and Playwright 1.65.1.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked   # must equal the crate's wasm-bindgen version

cd web
npm ci
npm run build:wasm   # cargo rustc -p off42-wasm --profile wasm-release --target wasm32-unknown-unknown
                     #   --crate-type cdylib, then wasm-bindgen --target web --out-dir src/wasm
npm run build        # type-check and bundle into dist/
npm test             # Node smoke test + self-contained build check
npx playwright install --only-shell chromium && npm run test:ui   # headless browser test
```

`npm run dev` serves the page with hot reload (without the Content Security Policy, which the dev server's
inline scripts would break); `npm run preview` serves `dist/`. Any static file server works for `dist/`; it must
serve `.wasm` as `application/wasm`.

The `wasm-release` Cargo profile (in `rust/Cargo.toml`) optimises for size with LTO; native profiles are unchanged.
The cdylib crate type is requested on the command line only, so native workspace builds of `off42-wasm` stay a
plain library.

## Core crates on wasm32-unknown-unknown

All core crates build for `wasm32-unknown-unknown` (CI checks this): `suite42-core`, `suite42-pdf-text`,
`doc42-core`, `xls42-core`, `ppt42-core`, `pdf42-core`, `img42-core`, `scn42-core`, `label42`, `marks42`.
Changes made for it, all limited to `cfg(target_arch = "wasm32")` dependencies:

- `pdf42-core`, `suite42-pdf-text`: `getrandom` with the `wasm_js` backend (Web Crypto).
- `xls42-core`: `chrono` `wasmbind`, `time` `wasm-bindgen` and `rust_xlsxwriter` `wasm`, so clock reads (formula
  `NOW()`/`TODAY()`, workbook timestamps, ZIP entry dates) go to JavaScript instead of panicking.
- `xls42-core`: `Workbook::from_csv_bytes` (CSV from memory; `open_csv` now reads the file and calls it).

Compiling is not the same as working: code paths that read `std::time::SystemTime`, the file system or threads
panic or fail at run time in the browser. The exported API avoids them (for example the host passes seeds and
dates), and the smoke tests run every export inside wasm. Known paths that would still fail if exposed later:
`marks42::seal::today` and `SeededRandom::fresh_seed`, `doc42-core` revision timestamps (`now_iso8601`),
file-path APIs (`open`, `save_as`, attachments) and `tempfile` users in `img42-core`, `scn42-core` and
`suite42-core`.
