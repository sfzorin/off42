# off42 web

A web app over the off42 Rust cores compiled to WebAssembly (ARCHITECTURE.md, decisions D4 and D5). Files are read with the browser's file picker, processed inside the tab and offered back as downloads. Nothing is
uploaded: the built page carries a Content Security Policy with `connect-src 'self'`, and the tests check that no
request leaves the page's own origin. The site serves it at https://off42.cc/app/ (see *Publishing on off42.cc*).

## What it does

The page is an application shell in the off42.cc identity, one screen per app: Documents, Spreadsheets,
Presentations, PDF, Labels & codes and Signatures & stamps, each with its app colour and its own URL (`#documents`,
`#spreadsheets`, `#presentations`, `#pdf`, `#labels`, `#signatures`). The theme follows the system's light or dark
setting; Arabic is laid out right to left.

- **Top bar** (home and tool screens): ☰ opens a slide-in drawer with the six apps, a link to the site's download
  page, About/Privacy links and the engine version (scrim, Esc, focus kept inside); the app logo and name; a search
  over the recent files; a language menu with all 20 languages; Help opens About.
- **Home screen** of a file app: for documents and spreadsheets a *Blank document* / *Blank spreadsheet* tile first
  (also File ▸ New in the editor; a new file is called "Untitled document.docx" / "Untitled spreadsheet.xlsx" until
  renamed), an *Open from device* tile (or drop a file anywhere on the page; a file opened in the wrong app goes to
  its own) and the *Recent* list with name, time opened and size. Only those three are stored, in this browser's
  `localStorage`; the file contents are not. Files opened in this tab stay in memory and reopen at once; an older
  entry asks for the file again with the file picker. Entries can be removed from the list. New files are not in the
  list (they have no bytes until downloaded).
- **Editor**: a title row (app logo back to the home screen, file name — renaming changes the name of downloads —,
  format badge, "On this device", "Edited"/"Saved" once something was edited, a *Download* pill), a menu bar (File,
  Edit, View, Insert, Format and the app's own menus, Help; keyboard navigable) and one rounded toolbar, then the
  content on a grey canvas. Menus and toolbar carry only what works: New, Open, Download, *Download as ▸* every export
  format, Rename, Print, Close, Undo/Redo, clipboard, zoom, full screen, and per app the items below. *Download*
  returns the file exactly as opened until it is edited, then saves the edited content in the file's format; any
  download of the edited content turns "Edited" into "Saved". Closing an editor, opening another file in its place or
  leaving the page with changes that were not downloaded asks first. Messages appear in one snackbar at the bottom
  start, above the sheet tabs in spreadsheets; a new message replaces the previous one.
- **Phone** (≤ 600 px): a compact bar (back, file name, download, ⋮). The ⋮ menu holds the same commands as the menu
  bar; the toolbar moves to the bottom; touch targets are at least 44 px.

| Input | Shown as | Download as |
| --- | --- | --- |
| DOCX, ODT, RTF, Markdown, HTML, text, or a blank document | An editable A4 page drawn by the engine: paragraphs with their bold/italic/underline/strike, size, font and colour runs, headings, lists, alignment and indents, links (shown, not followed); tables and pictures are shown and not editable. Word count in the toolbar, *Document statistics* (words, characters, paragraphs, format, size); zoom; print. Documents with tracked changes open as before, as the core's HTML on a sandboxed page, marked *View only* | DOCX, ODT, RTF, Markdown, HTML, text |
| XLSX, ODS, XLS (read-only), CSV, or a blank workbook | A full-width grid with column letters and row numbers, sticky headers and the selected row/column highlighted; a formula bar with the cell address (or range) and the cell's input (a formula, not its result), editable; click, shift-click, drag and arrow-key selection with the sum and count of numbers in a status chip; sheet tabs at the bottom, a sheet list and a Sheet menu; calculated, formatted values (bold, italic, underline shown); first 1000 rows × 100 columns; print of the current sheet | XLSX, ODS, CSV, displayed values of a sheet as CSV |
| PPTX, ODP | A slide strip with the selected slide ringed, the slide as a 16:9 card (fit or zoomed) with its text, tables and picture placeholders, speaker notes below (can be hidden); arrow keys, Home/End and the Slide menu move between slides; print of all slides | PPTX, ODP |
| PDF | A summary page (title, author, pages, size, properties, bookmarks as contents) with a one-line note that page rendering is not available; a side panel (toggle) with document properties, bookmark tree, attachments and form fields | — (Download returns the PDF) |

### Editing

Every change is a command of the editing layer (`rust/crates/edit42`, ARCHITECTURE.md D13), sent to an edit session
in the engine; the page is then redrawn from the session's snapshot. One user action (or one paste, one toolbar
click) is one request and one Undo step; consecutive typing at the caret is one step until the caret moves or another
command runs. Sessions keep 100 steps. The same JSON commands are meant for the AI assistant.

- **Documents**: typing, Enter (a new paragraph; after a heading it is normal text; in an empty list item it ends the
  list), Backspace/Delete (characters, words with Alt/Ctrl, joining paragraphs; at the start of a heading or list item
  it first makes the paragraph normal text), typing over a selection, paste of plain text (lines become paragraphs),
  cut, input methods (the composed text is committed when composition ends). Toolbar: Undo, Redo, Print, zoom,
  paragraph style (normal text, heading 1–3), bold, italic, underline (with a caret they apply to what is typed next),
  align left/centre/right, bulleted and numbered list, word count. Edit menu: Undo, Redo, Cut, Copy, Paste (Paste
  reads the clipboard through the browser, which may ask; Ctrl/⌘+V always works), Select all. Insert: today's date,
  lists. Format: the styles, bold/italic/underline, the four alignments (justify too), lists. Shortcuts: Ctrl/⌘+Z,
  Ctrl+Y or ⇧⌘Z, Ctrl/⌘+B/I/U.
- **Spreadsheets**: typing in a selected cell replaces it, F2 or a double click edits its input, Enter/Tab (with
  Shift: back) commit and move, arrows commit when typing started the edit, Escape cancels; the formula bar edits the
  active cell (Enter commits); Delete/Backspace clear the selection; Ctrl/⌘+C/X/V copy, cut and paste displayed values
  as tab-separated text. Toolbar: Undo, Redo, Print, zoom, currency (the interface language's currency: $, €, ₽, ¥, …),
  percent, fewer/more decimal places, number formats (automatic, number, currency, percent), bold, italic, insert row
  above, delete rows, insert column left, delete columns, sort A→Z and Z→A by the active column (a single cell sorts
  the table around it, keeping a text header row). Edit, Insert (rows above/below, columns left/right), Format and Data
  menus carry the same commands. Shortcuts: Ctrl/⌘+Z, Ctrl+Y or ⇧⌘Z, Ctrl/⌘+B/I. Inserted or deleted rows and columns
  shift formula references (the core's structural edit). Commands act on the sheet shown.

Print uses the browser's print dialog: the printable content (document, current sheet, slides) is placed in
`#print-root` and print CSS hides the rest of the app.

Conversions use the cores' own encoders and their fidelity rules: a conversion that would lose content the target
format cannot carry is refused with the core's message (for example bookmarks to Markdown, or plain text from a
formatted document). Word and OpenDocument files keep opaque package parts (settings, unknown styles) that only a
same-format save can preserve; converting them to another format asks first and then drops only those parts.
doc42-core refuses digitally signed DOCX files rather than silently dropping the signature.

**Labels & codes** — QR, Data Matrix, Code 128 and Aztec with a live preview as you type and a width control, as
PNG (300 dpi) or SVG; A4 label sheets as PDF from the built-in stock catalogue or a custom size, with a schematic
preview of the first sheet drawn from the stock geometry; the product mark catalogue (CE, UKCA, recycling,
handling, …) with search and category filters, as SVG or PNG (600 dpi).

**Signatures & stamps** — six generated handwritten signature variants at a time (form, pen, ink; a new impression
per variant, new variants on request) and a stamp editor with a live preview, all as transparent PNG. Randomness
comes from Web Crypto; the date in date stamps comes from the browser. The first stamp template depends on the
interface language.

**Loading** — the engine runs in a Web Worker (`src/engine.worker.ts`), so opening or converting a large file does
not freeze the page. The worker streams the wasm download and compiles it while it arrives; the loading screen
shows the percentage and the downloaded size. The interface catalog is loaded before anything is shown, so no
English text appears before another language.

### Limitations

- Editing covers documents and spreadsheets. Presentations and PDFs are view-only (ppt42 slide text editing needs a
  text-frame editing surface, which is not part of this stage). XLS workbooks are read-only.
- Documents: no editing of tables, pictures, headers/footers, fonts, sizes or colours (they are shown and kept); no
  tracked changes — documents that have them are view-only. Headings are saved as outline levels (Word's navigation
  pane and tables of contents see them; a new DOCX has no heading styles, so Word may show them in the body style while
  ODT, RTF, HTML and Markdown show them as headings). Markdown cannot hold alignment and refuses it with the core's
  message. Rich clipboard content is pasted as plain text.
- Spreadsheets: no fonts, colours, borders, merges, row/column sizes or sheet commands yet. XLSX and ODS files written
  by other programs keep their original package in the core, which refuses sorting and filling there (and number
  formats in ODS); files saved by off42 reopen fully editable. Currency formats use a symbol before or after the
  number (`"€"#,##0.00`, `#,##0.00 "₽"`); bracketed locale codes (`[$€-407]`) are shown unformatted.
- No page rendering or PDF output for documents, spreadsheets or presentations: the cores have no PDF writer for
  them (the desktop apps print through the operating system), and PDF page rendering needs PDFium for wasm32.
- Presentations show their text, tables and picture placeholders, not rendered slides.
- Stamps use only the bundled Noto Sans font (Latin, Cyrillic, Greek); stamp templates for Chinese, Japanese,
  Korean, Arabic or Hebrew text need fonts that a browser does not expose to WebAssembly.
- Large spreadsheets show their first 1000 rows and 100 columns (conversions include everything).
- Messages from the engine (conversion refusals, parse errors) are in English. Stamp template names exist in English,
  Russian and Turkish only, stamp categories and field names in English only; product mark names and descriptions
  exist in 11 languages (other languages show English).
- The engine is one wasm file of 13.2 MB (5.4 MB with gzip -9, 5.2 MB with zstd -3, 4.2 MB with brotli -11; measured with the editing layer of stage 2). The
  site-wide `Cache-Control: no-transform` is replaced for `/app/*` in `website/public/_headers`, so Cloudflare may
  compress it (its compression list includes `application/wasm`); whether it does can only be checked after
  deployment, by requesting the wasm file with `Accept-Encoding: br, gzip` and looking at `Content-Encoding`.
  Cloudflare's static assets have no documented support for serving precompressed `.br`/`.gz` copies, so none are
  shipped.

## Languages

The interface uses the 20 languages of off42.cc: `src/locales/<code>.json` are flat catalogs with the same keys
as `en.json` (English is the fallback for a missing key; `{name}` placeholders are filled in by `t()` in
`src/i18n.ts`). The language comes from `?lang=<code>` (as linked from the site), otherwise from the browser's
preferred languages (`zh-TW`/`zh-HK` → `zh-Hant`, `tl` → `fil`), otherwise English; the language menu in the top
bar reloads the page with `?lang=` and keeps the app (open files are not kept). Arabic switches the page to right-to-left. Catalogs other than
English are loaded as separate chunks; the page stays on a text-free loading screen until the catalog is in, while
the engine downloads in parallel. The language is passed to the engine where the cores have translations: `productMarks(lang)`
(label42 mark copy) and `stampTemplates(lang)` (marks42 template titles). Signature forms, pens and inks are
translated in the catalogs (keys `sig.form.<n>`, `sig.pen.<n>`, `ink.<name>`).

## Layout

- `rust/crates/off42-wasm` — the bytes-in/bytes-out API (plain Rust, tested natively with `cargo test`) and its
  wasm-bindgen exports (`src/bindings.rs`, compiled only for `wasm32`).
- `web/src` — the page (`index.html`, `src/main.ts`, `src/style.css`, SVG line icons and app logos in
  `src/icons.ts`; system fonts only, no font downloads), the engine
  worker (`src/engine.worker.ts`) and its async wrapper (`src/engine.ts`: every export becomes
  `call(name, ...args)`; after a wasm trap the worker is replaced, and open edit sessions are lost with it: the page
  reports that the file must be opened again); `src/wasm` is generated and not committed.
- `web/test/smoke.mjs` — Node loads the wasm package and exercises every export on fixtures.
- `web/test/offline.mjs` — checks that the production build is self-contained (CSP, no remote URLs).
- `web/test/ui.mjs` — headless Chromium (Playwright) drives the built page served at `/app/` with the headers of
  `website/public/_headers` (`test/serve.mjs`, so the site's Content-Security-Policy applies as on off42.cc): the
  loading screen and the worker; the home screen (no fake tiles, the tile opens the file picker); the drawer (focus
  trap, Esc, scrim, switching apps); About and the language menu (20 languages, current one checked, keyboard);
  each editor through its menu bar, toolbar and keyboard — creating and editing a blank document (typing, Enter,
  Backspace, heading 1 from the style menu, bold with Ctrl/⌘+B, a bulleted list from the toolbar, Undo/Redo from the
  toolbar, keyboard and Edit menu, Edited/Saved, the DOCX download reopened and checked for text, heading, bold and
  list, HTML with the package-parts question, the close question for changes that were not downloaded) and a blank
  workbook (values and a SUM formula typed with Enter, the formula bar, a row inserted above the summed range moving
  and widening the formula, Tab, Undo/Redo, F2, Delete, the formula bar editing a cell, bold, currency format, the
  XLSX download reopened and checked for values, formula, format and bold, values CSV); documents (original download, *Download as* DOCX, ODT,
  RTF, Markdown and HTML with their bytes, statistics, rename, print content and print CSS, zoom, menu keyboard
  navigation, DOCX → ODT package-parts confirmation), recent files (search, reopening from memory, re-prompt after
  a reload, removal), spreadsheets (formula bar, header highlight, range sum, XLSX/ODS/CSV, values CSV, a two-sheet
  workbook `test/fixtures/sheets.xlsx` with tabs, sheet list and Sheet menu, print), routing of a file to its app,
  presentations (strip ring, 16:9, notes toggle, arrow keys, Slide menu, PPTX/ODP, print of every slide) and the
  PDF fixture `test/fixtures/sample.pdf` (summary page, side panel sections and toggle, no conversions); one
  snackbar replacing the previous message; codes, label sheets, marks, signature variants and the stamp editor;
  languages (`?lang=` with the catalog held back to show that no English appears, language menu, browser default,
  RTL drawer, bar and menus, localized names from the cores), the dark theme and the phone layout (bar sizes, ⋮
  menu with the same commands, bottom toolbar, 44 px targets, no horizontal scrolling); the snackbar never overlaps
  the sheet-tab bar or the phone toolbar. It fails on any request
  outside the page's origin or any page error.
  `--dir ../website/public/app` tests the published copy; `--url <app URL>` tests an app served elsewhere, for
  example by `npx wrangler dev` in `website/`; `--screenshots <dir>` saves screenshots of the main screens.
- `web/scripts/publish-to-site.sh`, `web/scripts/site-build.mjs` — publishing to off42.cc (below).

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

The `wasm-release` Cargo profile (in `rust/Cargo.toml`) optimises for size (`opt-level = "z"`, LTO, one codegen
unit, `panic = "abort"`, stripped symbols); native profiles are unchanged. Against the previous `opt-level = "s"`
without stripping, the engine went from 16,008,161 to 12,885,801 bytes (gzip -9: 6,056,802 → 5,275,968; brotli
-11: 4,534,208 → 4,120,992). `wasm-opt -Oz` (binaryen 132) shrinks it further to 12,262,426 bytes but makes the
compressed file slightly larger (gzip -9 5,299,407, brotli -11 4,159,525) and adds over a minute to the build, so
the build does not run it.
The cdylib crate type is requested on the command line only, so native workspace builds of `off42-wasm` stay a
plain library.

## Publishing on off42.cc

The site is built by Cloudflare Workers Builds from `main`, which runs Node only and cannot compile Rust. The built
app is therefore committed to `website/public/app/`, and the site generator copies it to `dist/app/`:

```sh
cd web
npm run publish:site     # sh scripts/publish-to-site.sh: build:wasm, build, npm test, copy dist/ to
                         #   website/public/app, write website/public/app/BUILD.txt
npm run test:ui -- --dir ../website/public/app
cd ../website && node scripts/build.mjs && node scripts/check.mjs
```

`BUILD.txt` records a SHA-256 over the sources (web/ without tests and generated files, and the off42-wasm crate
without its tests) and the hash and size of every published file. CI (`.github/workflows/web.yml`, job `site-app`)
runs `node web/scripts/site-build.mjs check website/public/app` and fails when the sources changed without
republishing, a published file was edited, or a file exceeds Cloudflare's 25 MiB static asset limit. Changes to
the core crates that off42-wasm links (doc42-core, xls42-core, …) are not part of the hash: republish after such
changes when they should reach the site. Each republish adds a new 13 MB wasm file to the Git history.

Asset names are content-hashed and referenced relative to the page (`base: './'`), so the same build works at
`/app/` and anywhere else. The site's Worker runs only for `/assets/video/*`; everything under `/app/` is served
as static assets, whose MIME type follows the file extension (`.wasm` → `application/wasm`, checked with
`wrangler dev` 4.166 and the browser test). Responses carry the site-wide headers from `website/public/_headers`;
its Content-Security-Policy allows `'wasm-unsafe-eval'` and `blob:` images for the app, and the page's own
`<meta>` policy (scripts, styles, wasm and fetches from its own origin only) applies on top of it. The engine
worker is a same-origin script, allowed by `script-src 'self'`. For `/app/*` the file replaces the site's
`Cache-Control: public, no-transform` (detached with `! Cache-Control`; Cloudflare applies rules in file order):
the page is revalidated on every visit, the content-hashed files under `/app/assets/` are cached for a year, and
without `no-transform` the edge may compress them.

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
