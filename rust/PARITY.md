# macOS and Windows parity

The reference is the Swift implementation on main plus the local technical-element and label workflows.
“Implemented” requires a normal UI entry point and persisted output where applicable. “Verified” records
actual checks. A source build alone does not establish Windows runtime behavior.

| Area | Current Windows coverage | Remaining work |
| --- | --- | --- |
| Files and document lifecycle | Open/save snapshots, conflict detection, Undo/Redo, successful-file recents with clear menu; verified independent New/New Window sessions in all six apps | Verify drag/drop, recovery reference behavior and associations |
| Localization and appearance | Twenty bundled catalogs, verified shared picker/restart and process language override; prompt label measurement and verified outer window placement | Complete Windows-specific translations, dark mode, panel visibility/splits and multi-display DPI |
| Text proofing | System Rich Edit proofing integration in DOC42/PPT42; native flags verified | Actual suggestions and installed-language behavior; full grammar and other editors |
| Signatures and stamps | Shared PNG/JPEG library in all six apps; optional white-paper removal; Undo/Redo for placement | Generators, sample learning and full geometry parity |
| Technical objects | Four barcode encoders, fourteen symbols, reusable PNG recipes, insertion/editing in PDF/DOC/XLS/PPT and document round trips | Full technical-object selection/placement controls and richer label artwork sources |
| Label production | A4 custom grids, 559 stock templates, row mappings, copies, first slot, serial identifiers | PDF artwork/region sources, preview and direct print |
| Printing | Native dialogs, actual-size PDF scene path, optional per-printer calibration and measurement sheet | Native print-to-file output and physical driver checks |
| PDF42 | Reading, page edits/labels, selected-page extraction/split ZIP, four text-export formats, watermarks/numbering/header-footer tokens, bounded text edits, annotations, simple forms, links, outlines, attachments, password copies, raster redaction | Comparison, batch merge/Bates workflows, decoration font choices, richer forms, OCR, certificate signing and validation, remote signing and CryptoPro integration |
| DOC42 | Rich text, tables, images, comments, notes, hyperlinks, bookmarks, TOC, structured literal Find/Replace, whole-body statistics and several format adapters | Complete page canvas, full sections/columns, fields, tracked changes, style management, navigator, text search options, table/image caret mapping, selection/layout counts and dictionary word segmentation, picture geometry and rich page decorations |
| XLS42 | Grid editing, engineering/text/math/statistical/financial/calendar formulas, styles, charts, summaries, tables, filtering, print layout | Formula coverage and dynamic arrays, full floating-object editing, advanced filtering/sorting, validation controls, outlines/subtotals, goal seek/solver, split panes, per-cell/password protection and fuller format fidelity |
| PPT42 | Native objects and text, 187 preset geometries with adjustment controls, grouping/transforms, basic transitions, PPTX/ODP and printing | Custom geometry editing, charts/SmartArt, master/layout editing, themes/templates, comments, media and animation playback, show tools and export workflows |
| IMG42 | Image decoding, transforms, adjustments, selections, text, metadata and batch output | Full markup selection/editing, smart lasso, live text, background removal, viewing panels and additional supported system codecs |
| SCN42 | WIA acquisition, image/PDF import, page processing, OCR and searchable output | Scan areas and rescan, device/preset controls, scan history, full export settings and physical-device checks |

## Recorded checks

- Windows preview.3 passed 319 MSVC tests and packaged all six apps. One OCR test was ignored.
- Native audit run 42341903682 launched all six actual executables, invoked About, and generated PNG barcodes
  and PDF label sheets through the real dialogs in PDF42, DOC42 and XLS42.
- Ongoing changes are developed on `codex/windows-parity`. Release links remain tied to verified tagged binaries.

- Run 42344453119 verified production workflows in DOC42/PDF42/XLS42 and proofing flags in DOC42/PPT42.
  IMG42/SCN42 mark checks were blocked by the audit harness retaining a temporary shell dialog handle.
- Portable image regressions verify native and external XLSX/ODS image geometry/data round trips,
  insertion/movement/deletion, preservation of an unrelated package entry, protection and Undo/Redo.
  Spreadsheet native mouse/render/save checks are added and await a Windows run.

- The technical-object adapter regressions pass insertion, replacement as one history step, Undo/Redo,
  and reopen for PDF, DOCX/ODT/RTF, XLSX/ODS and PPTX/ODP. The full portable workspace passed 334 tests
  with one OCR test ignored; Windows GNU target compilation passed. Native technical-object checks await a build.

- Native audit run 42345868618 passed all six apps on the built 8298357 executables, including actual
  spreadsheet rendered pixels, mouse movement, Undo/Redo, deletion/save/reopen and IMG/SCN mark pixels.
- Run 42346392111 verified technical recipe editing and save/reopen in PDF42, XLS42 and PPT42.
  It found a DOC42 command-state defect: Undo/Redo was blocked with a collapsed caret despite the menu
  being enabled. The dispatcher now checks history availability separately from text-format selection.
- Vector QA covers fourteen symbols and four captioned code types: independent Poppler renders match
  reference SVG/PNG ink geometry within one pixel. It found and fixed the text-subtree transform for
  captions. PDF annotations and standalone PDF exports retain the macOS recipe key and have no raster
  image resource in the technical appearance. The next Windows build must verify the updated dispatcher.

- DOC label sources now enumerate explicit tables or custom/selected text. Layout and mapping profiles
  remember only settings; header signatures guard restored mappings. Portable tests cover source
  snapshots, merged covered slots, Undo, layout restoration and exclusion of document values from
  preferences. A native DOC-table/PDFium-output/preferences audit is included for the next build.

- Language host changes pass 341 portable tests (one OCR test ignored) and Windows GNU compilation.
  Translation checks cover all catalogs, language aliases, CLI paths and bounded preference round trips.
  Numeric opt-in command tracing and a nonpublishing debug CI build support diagnosis of DOC42 About,
  which still times out in runs 42347783278 and 42347967756. The earlier refresh deferral did not resolve it.

- ODS picture regression reproduced duplication after sheet reordering. Original image identity now resolves
  through the imported sheet ID/order instead of its current position. All 66 XLS core tests pass, including
  move/save/reopen/delete and opaque package preservation. This is portable format evidence only.

- Run 42350186789 verified all six debug Windows executables after the notification dispatcher fix.
  DOC42 About, proofing toggle, technical Undo/Redo/save/reopen and real table-derived labels now pass.
  The table audit checks PDFium-extracted text/copies, source preservation and restored profiles.
  All twenty DOC42 language launches pass translated File menu/current-language checks, prompt actions,
  translated choice identity and unchanged document/code data. Six real prompt screenshots were inspected;
  special Windows labels still often use English fallback. This is not complete UI translation coverage.

- Run 42350595478 passed all six apps, twenty language scenarios and the recent-document lifecycle audit.
  The latter checks ordering, deduplication, actual Cancel/Discard handling with unsaved text, process restart,
  Clear Menu persistence and byte-for-byte source preservation using isolated preferences.

- PDF page-list extraction and per-page ZIP exports are implemented with background progress/cancellation and
  no-clobber final publication. Portable regressions cover reverse order, effective labels, annotations,
  bookmarks/forms, overlapping ranges, source preservation, cancellation and existing destination safety.
  The portable workspace passed 348 tests (one OCR test ignored); Windows GNU compilation passed.
  Native page-export interaction and PDFium-output checks await a Windows build.

- Full release validation run 42350889660 (84f2e3f) passed all six optimized apps, twenty language prompts,
  shared-picker Cancel/Discard/restart and next-launch settings in all six apps, and recent-document lifecycle.
- Run 42351585775 (689ee17 debug) passed shared/PDF/XLS targeted MSVC tests and the other app scenarios,
  but the added PDF export scenario timed out opening its next three-page source. Poppler independently
  confirms the fixture has three valid text pages. Native export verification is pending investigation;
  no assertions were relaxed. Numeric open-phase tracing and a repeated-async-open regression were added.
- Editable page-label ranges, numbering styles, prefixes, clear/Undo and navigation by logical label are now
  implemented. Source/export/history regressions pass in the portable suite (351 passed, one OCR test ignored).
  Native label edits, PDFium label extraction and navigation after reopen are added for the next build.

- A command-ID review found that host Save As (18) intercepted IMG/SCN Crop and PDF Draw Rectangle. Save As
  moved to 65507; native pixel/annotation/history regressions are added. Idle canvas mouse moves no longer
  trigger caption/repaint work; current tools need pointer motion only during gestures.
- The technical reopen audit now first makes an unsaved payload change, explicitly discards it while opening
  the saved file, and checks clean saved state. Earlier same-filename waits could read the existing model;
  earlier results do not establish asynchronous PDF reopen. Open-phase tracing in 42352293454 shows no
  later polling after the initial document commit. The updated host awaits native verification.

- Native audit 42353607494 passed all six aaed968 executables with the strengthened dirty/discard/reopen
  checks, real IMG/SCN crop pixels and history, drawn PDF rectangles, ordered extraction/split ZIP and
  editable labels/Undo/Redo/clear/navigation after reopen. PDFium independently checks output text,
  annotation counts and labels. Tracing confirms later async opens reach identity verification and commit
  after idle canvas work was removed. Harness fixes identify confirmation/errors separately from closing pickers.
- XLS adds 22 engineering functions, 112 shared units/27 prefixes, digit-bearing function tokenization and
  Excel compatibility prefixes. Portable tests cover reference examples, signed bounds, shifts, coercion,
  dimensional/unit limits and native/external XLSX/ODS formulas. Workspace: 357 passed; one OCR ignored.
  Windows GNU compilation passes; native editing/cached-XLSX-result checks await a new build.

- Generated Office caches now retain formula result types: XLSX numeric-looking text keeps leading zeroes,
  Boolean/string/error results remain distinct, and ODS keeps scalar cached text/Boolean values. XML regressions
  inspect types and values directly, including literal spreadsheet escapes and invalid XML character encoding.
  External XLSX/ODS tests edit inputs before recalculation to exclude saved-cache false positives.

- Static-runtime build 42355577552 passed PE ordinary/delayed-import checks for all six executables and bundled
  PDFium: no VC++ Redistributable DLL imports remain. Its native suite passed the other app/language/recent cases;
  the engineering case retained an older harness interaction. Public preview.3 was not changed.
- PDF text export now offers TXT/RTF/HTML/DOCX through one format/page-list prompt and a correctly configured
  picker. Portable regressions cover Unicode, text styling, page/blank-page boundaries, HTML escaping and safe
  cancellation/no-overwrite publication. Windows-only regressions check extraction from the current redacted and
  page-edited snapshot. Workspace: 360 portable tests passed, one OCR fixture ignored; Windows GNU test compilation
  passed. Native four-format output/history/source checks await the next build.

- A reproducible report covers all 494 macOS function examples (two locale-dependent cases skipped). The starting
  Windows evaluator matched 110 fixed examples; after 17 new text functions and range-aware CONCAT/CONCATENATE,
  136 match and 356 remain different. This is example coverage, not a count of implemented functions.
- Text regressions cover optional/missing arguments, negative delimiter occurrences, wildcard search/escapes,
  Unicode and count/code-point bounds, error propagation, range blanks/Boolean values and input edits after
  independent XLSX/ODS import. Portable workspace: 363 passed, one OCR test ignored; GNU test compilation passes.
  Native XLSX text-result and dirty/discard/reopen checks are included for the next compatible build.

- Native run 42356965075 passed all six 5918f9b executables, twenty language prompts, shared-picker/recent
  lifecycle and strict engineering dirty/discard/reopen checks. PDF text export passes real TXT/RTF/HTML/DOCX
  output from unsaved page edits, page ordering/boundaries, cancellation and unchanged source/history checks.
  The Windows PDFium regression separately verifies redacted source text is absent. Its fixture retains the
  refusal of shared-resource partial redaction and duplicates only its first page's font for the allowed case.
- Another 36 math functions plus TRUNC digit support bring fixed reference agreement to 176/492 (316 gaps).
  Domain/arity/overflow and large-finite-number regressions pass, along with every supported math reference case
  and independent XLSX/ODS recalculation. Workspace: 366 portable passes, one OCR fixture ignored; GNU tests compile.
  Native numeric/error cache and reopen scenarios are extended for the next compatible build.

- Full optimized run 42357299879 passed all workspace MSVC tests, runtime imports and the native suite on 5334878,
  including the added text-formula cache/reopen rows. Incremental run 42357757678 also passed on 25fcb16, including
  math/text numeric/error/text cache types and discard/reopen. These are development artifacts, not public releases.
- New/New Window now starts an independent document/viewer process, retains current unsaved work and forwards the
  language override. PPT exposes New. All six native window lifecycles and dirty DOC parent/child history/save/
  closure are covered by a new audit pending its first compatible build. No autosave/association claim is added.

- Native run 42358211751 passed 3481fb3, including independent windows in all six apps, inherited language,
  two DOC New commands with child history/save and unchanged dirty parent/source, and closing either side without
  terminating the other. The preceding optimized 5334878 run passed 368 MSVC tests with one OCR fixture ignored.
- All six hosts now persist normal/maximized outer placement with validated atomic preferences and paired native
  placement APIs. Fullscreen PPT preserves its original frame. Portable preference corruption/bounds/atomicity
  tests pass; GNU tests compile. Native resize/maximize/restart and synthetic offscreen relocation await a build.

- Independent ZXing-C++ 2.3.0 QA decoded 26 generated PNG/Poppler-rendered PDF outputs, covering all four barcode
  formats, Cyrillic, mixed CJK/Arabic/emoji and longer payloads (Code 128 remains ASCII). All values and actual
  decoder formats match. Eleven additional earlier vector/native-dialog output files also decode independently.
  The optional QA tools are not dependencies of installed apps; physical printer/scanner behavior remains separate.

- Native run 42358688773 passed 211659c, including all six normal/maximized frame restore paths and relocation
  from a synthetic offscreen saved position. This does not establish real multi-monitor DPI behavior.
- DOC literal replacement now includes table paragraphs, preserves images/styles/links and remaps bookmark,
  comment and note anchors in one bounded transaction. It fixes a reproduced stale bookmark-offset case.
  A generated ODT round trip exposed outer bookmark markers being consumed by inline note parsing; the reader
  now saves/restores that context, with both independent external ODT and structured round-trip regressions.
- Rich native Find uses the actual Rich Edit projection/ranges instead of the incomplete plain document projection.
  Portable workspace: 370 passed, one OCR fixture ignored; GNU tests compile. Native table/image/surrogate Find,
  wraparound, Replace All, complete saved history and dirty/discard/reopen checks await a compatible build.

- Final optimized run [42360592434](https://github.com/sfzorin/off42/actions/runs/42360592434) succeeded on
  `6e4c55bdc5d9a4015428267a924800e162c17b6c`: **377 MSVC tests passed**, none failed, one OCR fixture ignored.
  **All 30 native result groups passed**, including structured DOC Find/Replace, all six independent-window and
  placement lifecycles, twenty languages, PDF exports and math/text XLSX dirty/discard/reopen checks.
- The downloaded ZIP passes CRC checks and its seven binary hashes match the runtime import report. It contains
  all six optimized apps, PDFium, licenses and the exact build identity. Size: 61,197,732 bytes; SHA-256:
  `ede6e36ba0dc3a81673ed79680d988e129fe307838db202d3a09cd8a3cbeac7c`.
  It is a development artifact; public preview.3, main and the site remain unchanged.
- Latest portable workspace: **372 passed**, none failed, one OCR fixture ignored. All GNU Windows targets compile.
  The source archive passes CRC/membership checks and includes static-CRT configuration while excluding local
  instructions/status and build outputs. A passing development build does not close the remaining gaps above.

- Preset shape work adds all 187 macOS definitions, vector curve/compound fill rendering, text frames,
  yellow adjustment handles, type/adjustment controls and atomic history. The actual Swift engine generated
  620 checked-in reference cases; Windows geometry agrees for paths, fills, frames and handle positions.
  Portable regressions cover full-catalog project/PPTX/ODP output and preset/size/rotation source overlays.
  Native run 42366299902 passed all 30 groups and 263 targeted MSVC tests on efe85f7, including
  the handle/pixel/dirty-discard-reopen scenario. This is a development build, not a public release.

- PDF decorations add text/image watermarks, page numbers, six header/footer slots and shared Bates/date/file/page
  tokens. Multi-page actions and removals commit one history step; watermarks use paths rather than searchable text.
  Portable regressions cover source/inherited-resource preservation, four crop/rotation transforms, image alpha and
  shared form/image resources, token order, bounds, cancellation/settings and successful Save As filename tracking.
  Poppler independently confirms all four rotated/cropped pages retain body/Unicode/token text and exclude the watermark.
- Independent LibreOffice rendering exposed ODP page size falling back to 28 cm. Adding the missing office:styles
  section fixes physical slide size; the Office audit checks 16 pages and all 187 preset labels in both formats.
- Portable workspace: 387 passes, zero failures, one OCR fixture ignored. GNU all-targets compilation passes.
  Native PDF decoration controls/pixels/history/reopen verification is pending the next compatible Windows build.


- Native run [42405270583](https://github.com/sfzorin/off42/actions/runs/42405270583) passed all 30 result groups
  and 269 targeted MSVC tests on dd33481. PDF text/image watermark pixels, rotated pages, Unicode headers,
  Bates/date/file tokens, ordered numbering, removal/history and dirty/discard/reopen pass. Earlier optimized
  run 42403791844 passed tests/import checks but failed the header dialog title wait; its failure is retained.
- DOC whole-body statistics now includes tables beside pictures, joins style runs before Unicode boundaries and
  counts graphemes without changing editor caret projection. Portable tests cover history and project/DOCX/ODT.
- XLS statistical/distribution support raises the macOS fixed-example agreement to **293/492**, leaving **199 gaps**.
  All 277 independent 160-digit mpmath examples pass, including extreme tails/inverses and scientific literals.
  Computation and repeated argument text have shared budgets. Native DOC statistics and XLS formula/cache/history
  scenarios are added but await a compatible build.
- Independent LibreOffice QA reproduced unusable appended XLSX/ODS ZIP containers and an undeclared ODF private
  part. Fresh containers with package declarations now load. Recalculation agrees on 19/20 XLSX-origin and 18/20
  ODS-origin cases; explicit external differences are invalid-percentile error classes and ODS ERF precision loss.
  Golden native results are not weakened to match those differences. ODF function aliases retain tail semantics.
- Current portable workspace: **398 passed**, zero failed, one OCR fixture ignored. All GNU Windows targets compile.
  No new public release, main merge or website update has occurred; the remaining matrix is still open.


- Financial and calendar work raises fixed macOS example agreement to **356/492**, with **136 gaps** and two
  locale-dependent skips. A separate 112-case 100-digit mpmath oracle exposed and now guards long-term annuity
  cancellation, rounded-payment amplification and failed RATE guesses. Solvers and loops share work limits.
- ISO date entry now supplies actual date values to holidays/cash-flow functions; native and external XLSX/ODS
  tests cover edited inputs, complete history, saving and independent reopen without a private snapshot.
  Business-day searches use whole-week counting and bounded monotone search. Serial 60 and 1904 arithmetic
  have direct regressions. Locale entry formats and broader ODS date/number-format fidelity remain open.
- LibreOffice recalculates all 26 new financial/calendar examples from both complete packages. Across 46 cases,
  45 XLSX-origin and 44 ODS-origin agree; earlier external statistical differences are explicitly retained.
  Portable workspace: **403 passed**, zero failed, one OCR fixture ignored; GNU all targets compile.
  New finance/calendar native scenarios await a compatible Windows build.

- Optimized run [42408293307](https://github.com/sfzorin/off42/actions/runs/42408293307) on 373424b passed the
  workspace MSVC tests, all six builds and the runtime import gate. DOC body/table/image statistics passed;
  PDF decorations and the other native groups passed. The XLS statistical scenario failed its strict one-step
  Undo assertion: replacing an EDIT selection emitted separate deletion/insertion EN_CHANGE notifications.
  The standard editor now coalesces notifications within the native replace/paste operation, retaining the
  original complete-history assertion. Repeat native verification is pending; the failed evidence is retained.


- Native run [42409672916](https://github.com/sfzorin/off42/actions/runs/42409672916) passed **284 targeted MSVC
  tests and all 30 native result groups** on a666c7b. The original strict XLS Undo/Redo assertion now passes;
  so do typed date entry, financial/calendar values against the independent oracle, caches and discard/reopen.
  DOC statistics and PDF decoration scenarios remain passing. No public release was changed.
- Logical/metadata work raises fixed example agreement to **398/492**, leaving **94 gaps** and two locale skips.
  Regressions cover lazy conditions, logical error propagation, imported error vs text cells, nested formula origins,
  cross-sheet metadata, Unicode SWITCH matching, addresses and random bounds. AND/OR's earlier short-circuit
  test encoded behavior differing from macOS; it now explicitly requires propagation of the error operand.
- Independent LibreOffice recalculates all 19 new logical/metadata examples in both formats; totals are 64/65
  XLSX-origin and 63/65 ODS-origin, retaining the same external statistical differences. Portable workspace:
  **407 passed**, zero failures, one OCR fixture ignored. GNU all targets compile. New native assertions pending.
