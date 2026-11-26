# suite42-app

Portable application services shared by every off42 shell: the Win32 apps (`win42`, `rust/apps/*`), the Linux/egui
host and `off42` CLI (phase 2) and the WebAssembly build (phase 3). See [ARCHITECTURE.md](../../../ARCHITECTURE.md), decision D3.

## What belongs here

Logic and data formats that any shell needs and that do not call a platform UI API:

| Module | Contents |
| --- | --- |
| `host` | The `App` contract between a document model and its shell, and the plain data it exchanges (commands, file formats, canvas scenes, print jobs, grid views, dialog and clipboard requests) |
| `input`, `report` | Shell-neutral input forms and table reports |
| `recovery` | Recovery journal and the `OFF42RC1` record format (`encode_record`/`decode_record` work in memory) |
| `recents`, `window_layout`, `label_preferences`, `print_calibration`, `localization` | Stored preferences and the twenty-language UI catalogs (`resources/`) |
| `storage` | The per-user data root: `%LOCALAPPDATA%/off42` on Windows, `~/Library/Application Support/off42` on macOS, `$XDG_DATA_HOME/off42` or `~/.local/share/off42` on Linux |
| `production`, `generator`, `marks` | Label sheets and codes, signature and stamp generation, the picture library |
| `grid_axis`, `grid_geometry`, `sparse_table`, `rich_pages`, `note_pagination`, `pdf_print_layout`, `clipboard_envelope` | Grid, page and print geometry; typed clipboard envelope |
| `pdf` | Text comparison and export, OCR page pipeline, signature field binding, page export, merge, batch headers, label regions |
| `sheet` | Goal Seek, Solver, sort options, chart scenes, sheet print pages |
| `scan` | Export naming, scan history |
| `slides` | Outline export, rich text conversion, chart objects, slide show markup/overview/settings |
| `image` | Folder batch conversion, uniform background removal |
| `document` | Document navigator outline |
| `platform` | Services a shell must supply: `PdfText` (PDFium on Windows) and `OcrImage` for OCR engines |

## What stays in the shells

Windows, dialogs, menus, drawing, the clipboard API, registry, certificate stores, scanners, native OCR and PDFium
loading, spelling, dark mode and Explorer's filename order stay in `win42` and `rust/apps/*`. Where moved code needs
one of them, the shell passes it in: a `platform` trait object (`PdfText`), a function (`batch_headers::NameOrder`,
OCR `render`/`recognize` callbacks) or a folder (recovery root).

## Rules

- No `windows`, `windows-sys` or other platform UI dependency. The crate checks for `wasm32-unknown-unknown`.
- Stored formats (recovery records, preferences, histories, libraries) stay byte-compatible; tests pin them.
- Prefer bytes and in-memory documents at the boundary. Some services still read and write files or start worker
  threads (recovery journal, preferences, batch workers); they compile for WebAssembly but need a desktop shell at run
  time. `storage::local_data()` returns the per-user root on Windows, macOS and Linux and `None` in the browser.
- Move tests with the code. `cargo test -p suite42-app` runs on any host.
