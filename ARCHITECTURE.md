# off42 architecture

Status: agreed by the project owner on 2026-11-26. Changing a decision needs the owner's agreement.

## 1. Where we are

off42 has two independent implementations of the same six apps (PDF42, DOC42, XLS42, PPT42, IMG42, SCN42):

| Part | Technology | Size | Notes |
| --- | --- | --- | --- |
| macOS apps | Swift/AppKit, shared library `Kit42` (`Sources/`) | ~114k lines | Shipping product; preview DMG on the site |
| Windows apps | Rust workspace (`rust/`) | ~140k lines | Preview 4 ZIP on the site |
| — format and document cores | `rust/crates/*-core`, `label42`, `marks42`, `suite42-*` | ~89k lines | No Windows dependencies, except `suite42-core` (file locking) and `suite42-pdf-raster` (WinRT PDF rendering) behind `cfg(windows)` |
| — portable app services | `rust/crates/suite42-app` | ~12k lines | Phase 1: the `App` host contract and its data types, recovery, recents, preferences, localization, labels, generators, grid and page geometry; PDF comparison/text export/merge/Bates/OCR pipeline/signature binding, Goal Seek/Solver/sorting/charts/sheet print pages, scan naming and history, slide outline and show overlays, image batch and background removal. Builds for `wasm32` |
| — Windows UI | `rust/crates/win42` | ~16k lines | Win32 shell only (windows, dialogs, drawing, clipboard, PDFium loading, OCR engine, spelling, theme); re-exports the `suite42-app` contract under its old names |
| — app crates | `rust/apps/*` | ~23k lines | Win32 wiring of each app model; PDFium, WinRT, CryptoAPI, WIA and Windows OCR bindings |
| Website | Node generator (`website/`), Cloudflare | — | 20 languages |
| Linux | PR #4, paused | — | egui host + `off42` CLI over the Rust cores |
| Web | groundwork (`web/`, `rust/crates/off42-wasm`) | — | Not deployed; cores build for wasm32 |

Every feature is written twice, once in Swift and once in Rust. `rust/PARITY.md` tracks the drift, and some
ports (for example `marks42`) must stay bit-identical to Swift by hand.

## 2. Decisions

### D1. The Mac keeps its Swift implementation for now — **Agreed**

The Swift apps work and ship. They are not rewritten or re-platformed now. New Mac features continue in Swift.
Moving the Mac onto the shared core happens much later, as a separate, planned project (see §5, phase 4).

### D2. One Rust core for Windows and every other platform — **Agreed**

Windows, Linux, the command line and a future web version all run the same Rust core. Platform code is a thin
shell over it. Nothing that can run on more than one platform lives in a platform shell.

### D3. Shape of the core — **Agreed**

Recommendation:

- **Format and engine crates** stay as they are (`doc42-core`, `xls42-core`, `ppt42-core`, `pdf42-core`,
  `img42-core`, `scn42-core`, `label42`, `marks42`, `suite42-pdf-text`).
- **A new portable app-services crate** (working name `suite42-app`) takes everything in `win42` and
  `rust/apps/*` that is not Win32: document sessions, Undo/Redo command history, recovery snapshots, recents,
  localization catalogs, preferences, label/print layout, generators, batch operations, PDF comparison/merge,
  Goal Seek/Solver, export naming. UI shells call it; they do not reimplement it.
- **I/O boundary**: the core takes and returns bytes and in-memory documents; file paths, file locking,
  dialogs and the clipboard belong to the shell. This keeps the core usable from a CLI, a GUI and WebAssembly.
- **Platform services as traits** (`Ocr`, `PdfRaster`, `Scanner`, `Fonts`, `CertificateStore`) with one
  implementation per platform, chosen at build time.

Alternative: keep logic in `win42` and copy it per platform. Rejected: it recreates the Swift/Rust duplication
inside Rust.

### D4. User interface per platform — **Agreed**

| Platform | Options | Recommendation |
| --- | --- | --- |
| Windows | Keep Win32 (`win42`) · switch to a cross-platform toolkit | **Keep Win32.** It is native, verified by CI audits and shipping. |
| Linux | egui (as in PR #4) · GTK 4 · Slint · Tauri (web UI) | **egui**: one Rust code base, no system dependencies, already green in CI. |
| Web | WASM core + web UI (TypeScript) · egui compiled to WASM | **Web UI over the WASM core.** egui in a browser does not look or behave like a web page. |
| Mac | AppKit (current) | Unchanged until phase 4. |

Linux and the web keep separate interfaces: four UIs in total (AppKit, Win32, egui, web), each a thin shell
over the same core. The owner decided against merging the Linux and web interfaces (2026-11-26).

### D5. Web version — **Agreed**

Options: (a) the core compiled to WebAssembly, documents processed in the browser; (b) a server that runs the
core or the CLI on uploaded files.

Decision: **(a) WebAssembly in the browser.** The site promises that documents stay on the user's
computer; (b) breaks that promise, needs servers, storage and abuse protection, and costs money per user.
Cost of (a): PDFium and OCR must be built for `wasm32` (both have known builds), and features that need the
operating system (certificate stores, smart cards, scanners, printing) are unavailable or replaced by browser
equivalents (download instead of print, Web Crypto with imported keys).

### D6. Command-line interface — **Agreed**

Recommendation: one `off42` CLI over the core (convert, export PDF, labels, codes, marks, signatures, PDF
operations). It is the cheapest way to make Linux useful immediately, it is the main surface for automated
tests, and it documents the core's API. The existing per-app CLI flags in the Swift apps stay as they are.

### D7. Third-party engines — **Agreed**

| Need | Today on Windows | Recommendation |
| --- | --- | --- |
| PDF rendering and text-object editing | PDFium (bundled DLL) and WinRT `Windows.Data.Pdf` | **PDFium everywhere** (also builds for Linux and WASM); drop the WinRT path once PDFium covers it. |
| PDF structure | `lopdf` | Keep. |
| OCR | Tesseract (static, pinned models) and Windows OCR | **Tesseract in the core** for all platforms; Windows OCR stays an optional Windows extra. |
| Text shaping/fonts | `rustybuzz`, `cosmic-text`, `ttf-parser`, `fontdb` | Keep. |
| Scanning | WIA | Windows only; SANE on Linux later; none on the web. |

### D8. Verification — **Agreed**

- **Core**: unit and golden-file tests run with `cargo test` on Linux, Windows and macOS runners.
- **CLI**: scripted conversions against fixture files, compared with expected outputs.
- **GUI**: native UI audits run only in CI virtual machines (Windows today, Linux next). They are never run on
  the owner's working Mac. Mac GUI audits run only when the owner asks for them.
- **Mac ↔ core consistency** (until phase 4): shared fixture documents with expected results that both the
  Swift selftests and the Rust tests check, so the two implementations cannot silently diverge.

### D9. Releases — **Agreed**

One version number for all platforms; each release lists which platforms it covers. Windows and Linux builds
come from tagged CI runs. Mac: notarization when a Developer ID is available; until then, the first-launch
instructions on the download page.

## 3. Target layering (Windows, Linux, CLI, web)

```
 Win32 shell (win42)   egui/Linux shell   off42 CLI   Web UI (TypeScript)
          \                 |                |              /
           \                |                |        wasm-bindgen API
            \               |                |            /
             +-------- suite42-app: sessions, history, recovery, ---+
             |         preferences, localization, batch, labels     |
             +------------------------------------------------------+
             | format/engine crates: doc42 xls42 ppt42 pdf42 img42  |
             | scn42 label42 marks42 pdf-text                       |
             +------------------------------------------------------+
             | platform traits: Ocr PdfRaster Scanner Fonts Certs   |
             |   impls: PDFium, Tesseract, WIA, Windows OCR, ...    |
             +------------------------------------------------------+
 macOS: Swift/AppKit + Kit42, unchanged (phase 4 moves it onto the core)
```

## 4. Rules until the plan is done

- No new architecture (new crates, new UI toolkits, new platforms) beyond these decisions is merged without
  the owner's agreement.
- New portable logic for Windows goes into a core crate or `suite42-app`, not into `win42` or `rust/apps/*`.
- New Mac features stay in Swift; if the same feature exists in Rust, add a shared fixture (D8).
- No desktop GUI audits on the owner's Mac unless asked.

## 5. Plan

| Phase | Work | Depends on | Status |
| --- | --- | --- | --- |
| 0 | Finish the current release work on the current architecture: Windows dark mode, signature learning, DOC42 pages; macOS preview 2 DMG | — | — |
| 1 | Create `suite42-app`; move portable logic out of `win42` and `rust/apps/*`; Windows keeps working at every step (Windows CI green) | D3 | Moved on branch `phase1-suite42-app` (pull request, not merged); see below |
| 2 | `off42` CLI and the Linux preview on top of phase 1 (rebase PR #4 onto it) | D4, D6 | On PR #4 (not merged); see below |
| 3 | WebAssembly build of the core and the web UI | D5, D7 | — |
| 4 | Move the Mac apps onto the core, one app at a time, behind the existing AppKit UI | phases 1–3, owner's go-ahead | — |

Phase 1 result: `suite42-app` holds the `App` contract and every module listed in §1 that has no Win32 call; the
crate's README lists them. Code that needs the platform is passed in by the shell: `platform::PdfText` (PDFium),
OCR render/recognize callbacks, the filename order for batches. Still in the shells: app models that implement
`App` (`impl Pdf`, `impl Xls`, … including decorations, viewports, image recovery), the Win32 host, PDFium and
WinRT rendering, the Tesseract/Windows OCR engines, CryptoAPI signing, WIA scanning and spelling. The per-user data
root (`storage::local_data`) is still `%LOCALAPPDATA%/off42`; phase 2 adds the Linux location.

Phase 2 result (PR #4): the egui host is its own crate, `crates/egui42`, which depends on `suite42-app` and not on
`win42`; each app's `main` picks `win42::run` on Windows and `egui42::run` elsewhere, and `win42` has no eframe
dependency. App models import the moved contract from `suite42_app` directly. `storage::local_data` has a Linux root
(`$XDG_DATA_HOME/off42`, else `~/.local/share/off42`) and a macOS one (`~/Library/Application Support/off42`); the
Windows path is unchanged. The `off42` CLI calls `suite42_app` for PDF merge, page export, text export and comparison,
label sheets, codes, marks, signatures and stamps. Its `PdfText` is `suite42_app::pdf::lopdf_text` (plain text, no
styles or glyph boxes) on every platform; PDFium for Linux/macOS (D7) and Tesseract outside Windows are not done, so
OCR and PDF rendering stay Windows-only, and the PDF42 window off Windows still has no text export.

## 6. Work in flight

- **PR #4 (Linux)**: rebased onto phase 1 and reworked as phase 2 (above); not merged.
- **Windows dark mode, signature learning, DOC42 pages**: Win32 features within the current Windows shell;
  compatible with D1–D2. Portable parts (for example signature learning in `marks42`) already sit in a core crate.
