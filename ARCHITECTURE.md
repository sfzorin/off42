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
| Web | `web/`, `rust/crates/off42-wasm`, `rust/crates/edit42` | — | off42.cc/app (phase 3); documents and spreadsheets editable (phase 8, stage 2) |

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

### D10. Android app — **Agreed** (2026-11-26)

A native Android app for tablets first and phones too: Kotlin and Jetpack Compose over the shared Rust core,
called through a UniFFI bindings crate (`off42-mobile`). Same core as Windows, Linux, the CLI and the web; no web
wrapper. Published on Google Play.

### D11. iPad and iPhone app — **Agreed** (2026-11-26)

A native SwiftUI app for iPadOS and iOS over the same Rust core through the same `off42-mobile` UniFFI crate,
published on the App Store. The Mac's Swift/Kit42 code is not ported to iOS (D1/D2); this app is also the first
Apple client of the shared core, ahead of phase 4.

### D12. Mac App Store — **Agreed** (2026-11-26)

The six existing Mac apps are also published on the Mac App Store, as a sandboxed build variant of the current
Swift apps; the downloadable DMG stays. The same Apple Developer account is used to notarize the DMG.

### D13. AI assistant in the editors — **Agreed** (2026-11-26)

An optional assistant panel in the web editors (documents and spreadsheets first) edits the open file on request.

- It works only through the editor's command layer: the same serializable commands the toolbar and menus use, defined
  in the Rust core (stage 2 editing), so every assistant change is one Undo step and the same commands can later reach
  Windows, Linux, Android and iPad.
- The agent loop runs in the browser; tools execute locally on the open document. Requests go to a same-origin
  Cloudflare Worker (`off42.cc/api/assistant`) that holds the OpenRouter API key as a secret (`OPENROUTER_API_KEY`,
  set by the owner with `wrangler secret put`) and forwards to OpenRouter's chat-completions API with tool calling. The key never reaches the browser; the page still makes no requests to other origins.
- Open to everyone without an account. Spending is limited in money, not requests (owner's decision 2026-11-26):
  - a spending session is one anonymous browser ID on one UTC calendar day. Its $0.10 of model cost is shared
    across tabs, files and chats, including after withdrawing and granting consent again. The next UTC day has a
    fresh budget. Browsers unable to share an ID use their daily network hash for the same $0.10 ceiling.
    The Worker also keeps a per-IP-hash ceiling of $0.50/day so clearing the browser or switching
    browsers doesn't multiply it freely (several people behind one IP share that ceiling). This is approximate by
    design: there is no registration.
  - the Worker adds up the actual cost OpenRouter reports for each request (`usage` accounting) in Cloudflare
    storage keyed by day, id and IP hash; nothing about the document is stored. When a limit is reached it answers
    with a localized "today's limit is used up, try again tomorrow" and makes no model call.
  - a site-wide daily ceiling (default $5, a Worker setting) stops all assistant calls for the day if exceeded,
    plus the owner's credit limit on the OpenRouter key.
  - also: a bot check (Cloudflare Turnstile), a model allowlist and request/response size caps. The Worker does not
    log document content.
- Nothing is sent until the user opens the assistant and agrees; then only the selection, the document outline and the
  text the model asks for through read commands. The privacy policy (all languages) and store notes describe this.
  Without the assistant every app keeps working fully offline.
- Provider: OpenRouter; models are settings in the Worker. Measured in `assistant/eval/RESULTS.md` (PR #18).
  - Paid models only (owner's decision 2026-11-26): free OpenRouter models are capped per account (50 or 1000
    requests/day for the whole site) and would need to keep users' text, so they are not used. Users pay nothing;
    the owner pays per action (about $0.00015).
  - Primary `deepseek/deepseek-v4.1-flash` pinned to Alibaba/Together/Google Vertex with no fallbacks to other
    providers; fallback on 429/5xx or timeout `z-ai/glm-5.3-flash` on Z.AI/Fireworks. Both with
    `data_collection: "deny"`, so providers do not keep or train on what is sent.
  - Stable instructions and tool definitions go first so provider-side prompt caching applies (`assistant/CACHING.md`).
- Neui/Doors (server-side Go UI) was considered and rejected: it would move documents to a server and require
  rewriting the web UI.

Command layer crate (stage 2) — **Proposed** (needs the owner's agreement under §4): the commands live in a new
crate, `rust/crates/edit42`, rather than in `suite42-app` or `off42-wasm`.

- `suite42-app` links image, lopdf, tempfile and the PDF, image and scan cores; the command layer needs only doc42-core,
  xls42-core and serde. Putting it there would pull all of that into the 13 MB browser engine and into every client
  that only edits documents.
- `off42-wasm` is the browser's bytes-in/bytes-out API with wasm-bindgen exports; the commands must also serve the
  Windows/Linux shells, the `off42` CLI and `off42-mobile` without going through the web API, and the assistant later
  sends the same JSON.
- `edit42` holds no platform code: JSON commands in, JSON results out, an edit session per open file (blank or opened
  from bytes, apply, undo/redo, snapshot for drawing, save in a format). One request (one command or an array) runs in
  one `RichDocument::transact` or one `Workbook::batch`, so it is one Undo step and a failing request changes nothing.
  The State-level operations it needs went into the cores (`doc42_core::rich::ops`, `Workbook::batch`,
  `Workbook::update_style`, `Workbook::with_sheet`, history depth), so other hosts can use them without `edit42`.

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
| 0 | Finish the current release work on the current architecture: Windows dark mode, signature learning, DOC42 pages; macOS preview 2 DMG | — | Done: macOS preview 2, Windows preview 5 |
| 1 | Create `suite42-app`; move portable logic out of `win42` and `rust/apps/*`; Windows keeps working at every step (Windows CI green) | D3 | Moved on branch `phase1-suite42-app` (pull request, not merged); see below |
| 2 | `off42` CLI and the Linux preview on top of phase 1 (rebase PR #4 onto it) | D4, D6 | Done (PR #4); Linux preview 1 released |
| 3 | WebAssembly build of the core and the web UI | D5, D7 | Done: off42.cc/app (PR #8, #10) |
| 4 | Move the Mac apps onto the core, one app at a time, behind the existing AppKit UI | phases 1–3, owner's go-ahead | — |
| 5 | `off42-mobile` UniFFI bindings; Android app (Compose, tablets first) | D10 | Done (PR #13); not on Google Play yet (needs the owner's developer account and upload key) |
| 6 | iPad/iPhone SwiftUI app over `off42-mobile` | D11, phase 5 bindings | Built on branch `phase6-ios` (pull request, not merged; not on the App Store); see below |
| 7 | Mac App Store build variant (sandbox) and DMG notarization | D12, Apple Developer account | Prepared without an account: `APP_STORE` variant, `scripts/build-app-store.sh`, DMG notarization in `scripts/sign_and_notarize.sh` (appstore/README.md); signing, upload and sandbox runtime checks wait for the account |
| 8 | Web editing (stage 2) with the shared command layer, then the AI assistant: Worker proxy, assistant panel, consent and policy updates | D13 | Stage 2 (editing) in PR #25 (not merged); see below. Assistant next |

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

Phase 5 result: `crates/off42-mobile` exports the web API's capability set through UniFFI 0.33 (proc macros) as typed
records and a typed error (`UnsupportedFormat`, `PackagePartsWouldBeLost`, `InvalidInput`, `Failed`). It calls the
plain Rust functions of `off42-wasm` rather than copying them; giving that shared layer its own crate is deferred
because it changes the provenance hash of the published web app. The crate builds as a `cdylib` for Android
(arm64-v8a, armeabi-v7a, x86_64 via cargo-ndk) and as a `staticlib` plus Swift bindings and an XCFramework for
`aarch64-apple-ios` and `aarch64-apple-ios-sim` (`rust/scripts/build-mobile-apple.sh`). `android/` is the Compose app
(`cc.off42.office`); `.github/workflows/android.yml` runs the Rust, JVM, lint and emulator tests (tablet and phone) and
the iOS build. Phase 6 reuses the crate, the Swift bindings and XCFramework from that script, the fixture set in
`android/app/src/androidTest/assets/fixtures`, the Play listing texts as a base for the App Store listing, and the
Android screen structure (sections, list/detail on large screens, save through the system dialog).

Phase 8, stage 2 result: `crates/edit42` is the command layer (see D13). Documents: insert/replace/delete text (line
breaks split paragraphs; consecutive typing at the caret joins one Undo step), Enter (`split_paragraph`, ending a
list in an empty item), paragraph styles normal/heading 1–3/bulleted/numbered list (adjacent list paragraphs share a
list), alignment, bold/italic/underline over a range, by paragraph or by found text, and the assistant's planned
`replace_text`, `insert_paragraphs`, `delete_paragraphs`, `get_outline`, `read_range`, `find`. Spreadsheets:
`set_cells` (values and formulas, fill of one value over a range), `clear`, insert/delete rows and columns,
`set_number_format` (Excel codes, including currency literals that xls42 now shows and writes to XLSX and ODS),
`adjust_decimals`, `set_text_style` (bold/italic/underline), `sort_range`, `get_sheet_info`, `read_cells`; every
command can name a sheet. Cores: `SnapshotHistory::set_limit`/`amend`, `RichDocument::transact_amend` and
`set_history_limit`, `doc42_core::rich::ops` (State-level text, style, split/join, insert/remove block with anchors
moved), `Workbook::batch`, `update_style`, `with_sheet`, `set_history_limit`, `mark_saved`; sessions keep 100 Undo
steps. `off42-wasm` exposes sessions as `editNew/editOpen/editApply/editUndo/editRedo/editSnapshot/editSave/
editMarkSaved/editImage/editClose`; its existing functions, which `off42-mobile` calls, are unchanged. The web app
creates blank documents and workbooks (home tile, File ▸ New), edits documents on a contenteditable page whose input
events are all turned into commands (the page is redrawn from the session; IME text is committed at composition end)
and edits cells (typing, F2, double click, formula bar, Enter/Tab/arrows, Delete, clipboard). Documents with tracked
changes stay view-only (the core cannot record new revisions from these commands yet); presentations stay view-only
(ppt42 slide text editing would need a text-frame editing surface, not a small step); XLS stays read-only. XLSX/ODS
files written by other programs keep the core's limits: xls42 keeps their original package, so sorting and filling
are refused there (and number formats in ODS); files saved by off42 reopen fully editable. Next: the assistant (Worker proxy, panel, consent) over the same commands.

Phase 6 result: `ios/` is the SwiftUI app (`cc.off42.office`, iOS/iPadOS 17+, iPad and iPhone), generated as an
Xcode project by XcodeGen from `ios/project.yml`. It compiles the UniFFI Swift bindings and links the XCFramework from
`rust/scripts/build-mobile-apple.sh`; `off42-mobile` is unchanged. The six sections and their features follow the
Android app (a three-column `NavigationSplitView` that collapses to a stack on iPhone; PDF pages drawn by PDFKit). Its
String Catalog is generated from the web and Android strings. `.github/workflows/ios.yml` (which took over the Android
workflow's `apple` job) builds the engine, an unsigned Release archive with privacy checks, and runs XCTest unit tests
and XCUITests on iPhone and iPad simulators, which also produce the App Store screenshots. Listing drafts and the
owner's steps are in `appstore/ios/`.

## 6. Work in flight

- **PR #4 (Linux)**: rebased onto phase 1 and reworked as phase 2 (above); not merged.
- **Windows dark mode, signature learning, DOC42 pages**: Win32 features within the current Windows shell;
  compatible with D1–D2. Portable parts (for example signature learning in `marks42`) already sit in a core crate.
