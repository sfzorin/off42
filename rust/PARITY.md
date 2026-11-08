# macOS and Windows parity

The reference is the Swift implementation on main plus the local technical-element and label workflows.
“Implemented” requires a normal UI entry point and persisted output where applicable. “Verified” records
actual checks. A source build alone does not establish Windows runtime behavior.

| Area | Current Windows coverage | Remaining work |
| --- | --- | --- |
| Files and document lifecycle | Open/save snapshots, conflict detection, Undo/Redo, recents; verified crash recovery in all six apps, including IMG lossless snapshots | Encrypted-PDF recovery, drag/drop verification and associations |
| Localization and appearance | Twenty bundled catalogs, verified shared picker/restart and process language override; prompt label measurement and verified window placement and panel visibility | Complete Windows-specific translations, dark mode, splits and multi-display DPI |
| Text proofing | System Rich Edit proofing integration in DOC42/PPT42; native flags verified | Actual suggestions and installed-language behavior; full grammar and other editors |
| Signatures and stamps | Shared PNG/JPEG library in all six apps; optional white-paper removal; Undo/Redo for placement | Generators, sample learning and full geometry parity |
| Technical objects | Four barcode encoders, fourteen symbols, reusable PNG recipes, insertion/editing in PDF/DOC/XLS/PPT and document round trips | Full technical-object selection/placement controls and richer label artwork sources |
| Label production | A4 custom grids, 559 stock templates, row mappings, copies, first slot, serial identifiers, PDF page/region and DOC picture artwork | Vector fidelity for mixed DOC selections; precise picture/caret integration |
| Printing | Verified File/Ctrl+P dialogs in all six apps; actual-size label and centered/shrink-only PDF paths, per-printer calibration and measurement sheet | Printer copies/calibration scenarios and physical driver checks |
| PDF42 | Reading, page edits/labels, selected-page extraction/split ZIP, four text-export formats, watermarks/numbering/header-footer tokens, word comparison, batch merge and batch Bates copies, bounded text edits, annotations, simple forms, links, outlines, attachments, password copies, raster redaction and bounded offline OCR | Decoration font choices, richer forms, certificate signing and validation, remote signing and CryptoPro integration |
| DOC42 | Rich text, tables, images, comments, notes, hyperlinks, bookmarks, TOC, structured literal Find/Replace, whole-body statistics and several format adapters | Complete page canvas, full sections/columns, fields, tracked changes, style management, navigator, text search options, table/image caret mapping, selection/layout counts and dictionary word segmentation, picture geometry and rich page decorations |
| XLS42 | Grid editing, calculated arrays and external array cache fidelity, engineering/text/math/statistical/financial/calendar formulas, typed formatting, styles, charts, summaries, tables, filtering, print layout and verified Goal Seek/constrained Solver | Full formula semantics/syntax and locale formats, full-size native verification/automatic expansion, legacy arrays, full floating-object editing, advanced filtering/sorting, validation controls, outlines/subtotals, split panes, per-cell/password protection and fuller format fidelity |
| PPT42 | Native objects and text, 187 preset geometries with adjustment controls, grouping/transforms, basic transitions, PPTX/ODP and printing | Custom geometry editing, charts/SmartArt, master/layout editing, themes/templates, comments, media and animation playback, show tools and export workflows |
| IMG42 | Image decoding, transforms, adjustments, selections, recognized text selection/copy, metadata and batch output | Full markup selection/editing, smart lasso, full Live Text interaction, background removal, viewing panels and additional supported system codecs |
| SCN42 | WIA acquisition and verified virtual rescan, image/PDF import, page processing, OCR and searchable output | Scan areas, device/preset controls, scan history, full export settings and physical-device checks |

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


- Full optimized run [42410651353](https://github.com/sfzorin/off42/actions/runs/42410651353) succeeded on
  5932667: **412 MSVC tests passed**, zero failures, one OCR fixture ignored; **all 30 native groups passed**.
  This includes the logical/information XLSX result types, conditions, complete history and discard/reopen.
  The ZIP passes CRC; all seven executable/PDFium hashes match the runtime import report. Size 61,565,630 bytes;
  SHA-256 `e4641f34016036249631eaf18e99bcdf66ae50c94ad2633a22af8ebeac635654`. It remains a development artifact.
- PDF word comparison now has an ordinary file command, background cancellation, resizable native report,
  navigation/selection, temporary red/orange multiply highlights and explicit kept PDF annotations. Clearing live
  marks retains unrelated annotations and is undoable. Ten cases from the actual macOS Foundation implementation
  agree, with exact reconstruction of the new words. Dictionary segmentation across every language remains separate.
- Comparison portable regressions cover crop/rotation pixel positions, readable black text, colored annotation
  serialization, atomic keep/clear history and rejecting another source with identical PDF object IDs. The shared
  history exposes opaque identities retained through Undo/Redo, avoiding repeated document-content comparisons.
  **414 portable tests pass**, zero failures, one OCR fixture ignored; GNU all targets compile. Native comparison
  report/pixel/keep/history/reopen scenarios await a compatible build. Public preview/main/site are unchanged.


- Native run [42412900551](https://github.com/sfzorin/off42/actions/runs/42412900551) passed 294 targeted MSVC
  tests and all 30 native groups on 07e178d, including the comparison report, row selection/navigation, temporary
  orange pixels, explicit keeping/removal/history, identical inputs and saved/reopened PDFium text/annotations.
  Its report screenshot was taken after moving the window offscreen; the next audit returns it onscreen and
  requires a nonblank capture. The report now also fits its owner's monitor work area.
- Batch merge selects input PDFs and a new destination, runs in the background and opens the atomic output in
  an independent process. It preserves ordered pages, logical labels (including indirect prefixes), ordinary
  annotations and supported bookmark/form structures. Source/current documents remain untouched. Unsupported
  imported names/actions/tagging remain refused. Portable cancellation/failure/no-clobber and format regressions pass.
- Independent Poppler rendering confirms exact PNG equality for all three source/merged pages; extracted text
  has the requested order. Portable workspace: **418 passed**, zero failures, one OCR fixture ignored. GNU all
  targets compile. Native multi-file picker, result window/dirty-parent history and PDFium label/bookmark scenarios
  are added for the next build. No public release, main merge or website update has occurred.

- Run [42414794355](https://github.com/sfzorin/off42/actions/runs/42414794355) on d0cf09d passed the native PDF
  merge scenarios, including ordered PDFium text, labels/bookmarks, a separate result process and dirty-parent
  history. Its comparison report capture is now nonblank and was visually inspected. The overall run failed
  at the XLS edit assertion. The harness could send an edit before its asynchronously clicked range dialog
  completed; it now waits
  for that dialog to close and retains the same value/history assertions; repeat verification remains pending.
- Batch headers/Bates now continue from the current PDF into naturally ordered source PDFs, create no-clobber
  copies with per-file atomic publication, report every outcome, and retain completed copies on cancellation.
  Failed files consume no Bates numbers; private background work does not edit the current PDF/history. Long
  settings forms arrange fields in columns when a single column exceeds the monitor work area.
  Portable workspace: **421 passed**, zero failures, one OCR fixture ignored; GNU all targets compile. New native
  batch controls/report/saved-text/labels/bookmarks/history assertions await the next run. Full parity remains open.

- Native run [42416530246](https://github.com/sfzorin/off42/actions/runs/42416530246) on 714dc02 passes
  **301 targeted MSVC tests and all 30 native groups**. It verifies current-document-to-batch Bates continuation,
  natural file ordering, damaged-file outcomes without consuming a number, independently read PDFium body/token
  text and labels/bookmarks, current PDF Undo/Redo and source preservation. The monitor-aware settings form and
  outcome report were visually inspected. The original XLS values/history scenario also passes again.
- Computed references add OFFSET, A1/R1C1 INDIRECT and row/column INDEX selection, with bounded recursion and
  source identity for CELL/ISREF/ROWS. XMATCH/HLOOKUP and filter-aware SUBTOTAL/AGGREGATE raise agreement to
  **416/492 fixed macOS examples**, leaving 76 gaps and two locale skips. Arrays remain a separate open block.
- Single-column contains filters now use standard ODS database ranges and are read from standard XLSX/ODS
  metadata. External input edits update filtered row visibility while retaining manual hidden rows and unrelated
  structures. No-private-payload regressions verify formulas, filter state, exact row flags, history and reopening.
  General filter expressions, external filter reconfiguration and external formula-cache refresh remain open.
- Independent LibreOffice agrees on all 20 new reference/summary cases in each format. The expanded suite has
  **84/85 XLSX-origin and 83/85 ODS-origin** matches, retaining the prior explicit ERF precision/error-classification
  differences. Portable workspace: **427 passed**, zero failures, one OCR fixture ignored; GNU all targets compile.
  Native reference/filter/ODS/history assertions are added and await the next compatible build. Public state unchanged.

- External Office saves now refresh supported scalar caches across all current sheets, preserving formula text,
  shared-formula attributes, styles and unknown objects. They remove dangling native-snapshot declarations.
  Unsupported/unbounded results and repeated ODF formula caches are cleared for recalculation instead of
  retaining old numbers. Dynamic-array/legacy-array range fidelity and calculation-chain editing remain open.
- Independent direct XML inspection of edited external packages passes **85/85 values in XLSX and 85/85 in ODS**
  before any office recalculation. LibreOffice again gives 84/85 and 83/85, with the same recorded external
  differences. Portable typed-cache regressions independently read number/string/Boolean/error/empty/newline
  results with Calamine, cross-sheet dependencies, Undo/Redo, unknown metadata and clearing unsupported caches.
  Workspace: **430 passed**, zero failures, one OCR fixture ignored; GNU all targets compile. Native external
  XLSX/ODS edit/cache/history scenarios await the next compatible build. No public release/main/site change.

- Full optimized run [42417840799](https://github.com/sfzorin/off42/actions/runs/42417840799) on 123bf3c
  passes **432 MSVC tests**, zero failures, one OCR fixture ignored, all 30 native groups and the seven-binary
  runtime import gate. Native reference/filter/ODS/reopen/history checks pass. Package CRC and all binary hashes
  match the runtime manifest. Development ZIP: 61,728,869 bytes, SHA-256
  `7bf41b95d6874389130cdd3ac40e25f9141a304d5eaa691e6d8b56c4ac75e70a`. It is not a public release.
- Cache run [42418550661](https://github.com/sfzorin/off42/actions/runs/42418550661) on 1bd2c86 exposed an import
  defect: FORMULATEXT showed Excel's `_xlfn.` function prefixes after an external XLSX open. The assertion remains
  unchanged. Import now normalizes function-token prefixes, retaining quoted strings/names and original package
  formulas; portable regression verifies the stored original formula and corrected text cache. Repeat native
  verification is pending. Workspace: **431 passed**, zero failures, one OCR fixture ignored; GNU all targets compile.


- Repeated native run [42419327644](https://github.com/sfzorin/off42/actions/runs/42419327644) on 0e1c87b
  passes **311 targeted MSVC tests and all 30 native groups**, including the unchanged FORMULATEXT assertion,
  imported scalar-cache edit/save/reopen/history and existing reference/filter scenarios.
- Calculated-array source block adds arithmetic/conditional arrays, LET, spill conflict/shrink handling, sorting,
  reshaping and matrix operations. Agreement rises to **469/492 fixed macOS examples**, with 23 gaps and two
  locale skips. Worksheet expansion and external/legacy array fidelity remain open. Scalar and array argument
  text/work are bounded before replication; targeted oversized-input regressions pass.
- Independent saved XML caches and actual macOS XLS42 recalculation match **39/39 cases in XLSX and 39/39 in
  ODS**. The first independent ODS run exposed conflated matrix row/column separators; the converter now writes
  OpenFormula `|` rows and `;` columns, retaining argument separators and quoted text. Evidence is retained under
  `build/windows-parity/arrays-office-fixed/verified/results.json`.
- Plain clipboard text now follows macOS formatted results, with private formulas/styles/coordinates retained;
  partial spills copy typed values and complete anchored spills recalculate after paste. Existing translation and
  history assertions remain, with explicit private-formula assertions replacing the former raw-formula TSV assumption.
- Portable workspace: **437 passed**, zero failures, one OCR fixture ignored. New native array cache/range/
  editing/Undo/Redo/ODS/reopen assertions are added, pending their first Windows run. No public release/main/site change.


- TEXTSPLIT, regular-expression functions, fixed-decimal/Roman conversions and regression arrays close the
  remaining **23 fixed-example gaps: 492/492 match**, with two locale-dependent skips. This is example agreement,
  not full formula semantics or Office parity. Look-around/pattern backreferences and locale formatting remain open.
- Regression uses centered/scaled Householder QR with covariance/statistics and multivariable/row-oriented
  predictions; singular, nonpositive logarithmic, mismatched and large-offset inputs have portable regressions.
  CELL metadata reads typed spill results. Empty regex patterns follow macOS `#VALUE!` after independent comparison.
- Expanded independent standard-cache inspection and actual macOS XLS42 recalculation pass **90/90 per format**
  for XLSX and ODS. Decimal FIXED output was locale-sensitive in the first comparison; the fixture verifies its
  output length alongside independent Windows rounding assertions. The failure evidence remains retained.
- Portable workspace: **442 passed**, zero failures, one OCR fixture ignored; GNU all targets compile. Native
  text/regression spill/range/reopen assertions are extended for the next build. External array adapters, capacity
  and the other application parity gaps remain open. No release/main/site publication.


- First array-native run [42422535506](https://github.com/sfzorin/off42/actions/runs/42422535506) on 4d2fa4a
  passed the native baseline typed values/ranges/metadata, empty child input and blocked-anchor calculation.
  It stopped at a new harness assertion reading a shared-string index as literal XML text. Independent extraction
  resolves C2 to the exact expected `blocked` text. The audit now resolves the XLSX shared-string table and retains
  that exact expected value; full native Undo/Redo/reopen verification is pending a repeat. Grid capture was inspected.


- Run [42423267384](https://github.com/sfzorin/off42/actions/runs/42423267384) on 3e1f271 passes **322 targeted
  MSVC tests and all 30 native groups**. It covers text/regression array values and ranges, complete blocking Undo/Redo,
  ODS matrix spans and saved/reopened results. The shared-string audit correction now passes; grid capture was inspected.
- External standard arrays now import without private snapshots and save updated values/ranges after resizing,
  blocking and removal. Typed child caches, modern Excel errors, source styles and original pure metadata record
  indices are preserved. New XLSX arrays register metadata when no part exists. Mixed/unknown metadata registration,
  legacy CSE semantics, repeated matrix anchors and structural shifts remain guarded/open.
- Independent saved-cache inspection and actual macOS XLS42 recalculation pass **92/92 per format** on external
  XLSX/ODS packages after input edits and array resizing, with no private snapshot and a retained foreign binary part.
  Evidence: `build/windows-parity/external-arrays-office/recalculated/results.json`.
- ODS testing exposed injected default styles on new cached numeric cells. Export now restores the source's lack of
  explicit style before running the unchanged strict preservation check. Repeats, metadata, full history/reopen,
  new registration, literal strings, overlap and total import-work bounds have portable regressions.
- Portable workspace: **450 passed**, zero failures, one OCR fixture ignored; GNU all targets compile. Native
  external-array edit/resize/cache-cleanup/block/history/reopen assertions await the next full optimized run.
  General numeric display precision and locale formatting also remain different from macOS. No public changes.


- General numeric presentation now follows **105 actual Swift General and 14 fixed-pattern oracle cases**.
  Numeric-only formatting preserves text identifiers and supports styled spill children. Shared TEXT/cell patterns
  add bounded zero-padding/grouping/decimal/percentage formats. Full locale/custom-format and native ODS number
  style export remain open. `calculated_grid` and typed Office caches retain full precision.
- Existing numerical oracles retain their expected values/tolerances and now read the explicit full-precision data
  projection. New checks separately verify visible/clipboard text, bit-exact `1/3` caches in XLSX/plain ODS, original
  formulas/types, history and formatted spill children. All **492 fixed formula examples** still match.
- A new formula-filter test exposed the XLSX writer's extra automatic row-hiding pass. Export disables that pass
  and writes the application's calculated row flags. Filter matching, SUBTOTAL and external manual-hidden-row
  classification now share the presentation formatter; no-private-payload scientific-number/hidden-row checks pass.
- Portable workspace: **453 passed**, zero failures, one OCR fixture ignored; GNU all targets compile. Native
  formatted clipboard/text-cache assertions are added, pending a compatible run. Optimized external-array run
  [42425180333](https://github.com/sfzorin/off42/actions/runs/42425180333) on b4e9675 is still running.
  No release/main/site changes have been made; full suite parity remains open.


- Optimized run [42425180333](https://github.com/sfzorin/off42/actions/runs/42425180333) on b4e9675 passes
  **455 MSVC tests**, zero failures, one OCR fixture ignored, and builds all executables. Native external XLSX
  import/resize/Undo/Redo reaches the blocked child, then a new harness reader misses its valid inline string.
  Independent extraction from both failing source artifacts resolves the exact expected `stop` text.
- Run [42426181089](https://github.com/sfzorin/off42/actions/runs/42426181089) on 04bd50d reaches the same assertion,
  after the new rounded General clipboard check passes. Its PPT audit also encounters a transient sharing lock
  while reading the atomically saved JSON. The harness now reads standard inline strings and retries only sharing
  failures within the existing deadline; all exact data/geometry/history assertions remain. Repeat verification pending.


- Run [42426756820](https://github.com/sfzorin/off42/actions/runs/42426756820) on ff84a6b completes the external
  XLSX scenario and ODS resize/Undo/Redo, then exposes another new harness reader omission: ordinary ODS text may
  live only in `text:p`, without `office:string-value`. Independent reading of the actual saved artifact confirms
  `D1=#SPILL!`, `D2=stop` and every former child cleared. The audit now reads that standard text representation;
  the same exact blocking/history/reopen assertions remain. Other native groups, including PPT sharing-lock retry,
  pass. Full native completion remains pending a repeat.
- The optimized b4e9675 development ZIP passes CRC and all seven PE identities match the runtime report
  (six apps plus PDFium.dll): 61,948,557 bytes, SHA-256
  `355e6655b311d609e894c99daadbaa453b9b54bcdbba8f420063065fbc7b2e5e`. It is not a public release.

### Bundled PDF/scan OCR in progress

- Previous native run 42427772518 on 5a120e4 succeeds: all 30 groups, including external dynamic XLSX/ODS
  resizing, blocking, history and reopen. The harness now reads standard inline and ODS paragraph strings.
- PDF OCR capture/geometry, transactional invisible layers, stale/error refusal, own-layer replacement and
  Unicode font mapping are implemented. SCN shares the statically linked backend and bundled models.
- 457 portable tests pass, zero fail, one external OCR fixture remains ignored; GNU all targets compile.
  The pinned static backend build, real image recognition and packaged native OCR UI are not yet verified.
  Public preview, main and website remain unchanged.

- Independent Poppler 26.10.0 extraction/rendering passes for OCR Unicode (Latin/Russian/Chinese/Thai/emoji),
  four rotations/crop origins, exact original pixels, own-layer replacement, complete Undo and scan output
  with/without compression. Local evidence: `build/ocr-interop-v1/poppler-v1/results.json`. New native menu
  scenarios cover real OCR pixels, selected-page scope, PDFium text/pixels, history and reopen; still pending.

### Image Live Text in progress

- IMG42 adds background analysis tied to immutable current pixels, word/range selection and clipboard,
  reading-only overlays, explicit drawing/viewing modes and Space/Page Down/Page Up navigation. Native
  image canvases now dispatch double-clicks and honor pending file navigation from keyboard events.
- Pixel, history and frame invalidation/selection regressions pass. Portable workspace: 460/0/1; GNU all
  targets compile. Native menu/pointer/clipboard/drawing/reopen checks are added but pending Windows CI.
- Recognition settings stay in a separate menu; the primary toolbar shows only viewing and text Copy.

Windows run 42429149583 on ebc82b8 succeeded: 362 targeted MSVC tests passed, zero failed, one external OCR
fixture ignored, all 30 existing native groups passed, and the seven-PE static runtime gate passed. This
includes actual recognition of image pixels and verification of all twenty bundled models. New PDF/SCN/IMG
OCR GUI workflows remain pending. Follow-up native canvas changes refresh Copy availability immediately
after image selection; image unit regressions are now included in quick CI.

### OCR execution and visible painting evidence

- cd1b742 build 42430554087 succeeds: 399 targeted MSVC/0 failures/1 optional external OCR ignored, all
  30 groups and static runtime gate. PDF OCR UI executes and passes. IMG/SCN OCR branches were initially
  skipped by an early test-script return; this was corrected, not counted as verification.
- Focused runs 42431277055 and 42431605616 pass actual PDF/IMG/SCN OCR workflows on cd1b742. The latter
  forces a normal repaint and asserts visible text/selection pixels; screenshot inspected. New native
  drag-across-words coverage is added for the next build.

### Vector PDF label artwork in progress

- Current-page or selected-text-area PDF artwork, optional region, visible annotation flattening, shared
  vector forms, rotation/crop, stock copies/first slot and separate layout-only preferences are implemented.
  A failed artwork capture does not block the existing Custom Text source. Preview/direct print remain.
- Independent native CoreGraphics/Poppler reference: eight page/rotation comparisons within one pixel, exact
  copied text and no unrelated metadata; local evidence `build/label-artwork-interop-v1/poppler-v1/results.json`.
- Portable workspace 463/0/1; GNU all targets compile. New native label text/pixels, annotation flattening,
  current unsaved state and history/source checks await Windows CI. No new public release/site deployment.

### Document picture label artwork in progress

- DOC42 enumerates picture artwork separately from tables/custom text; only the selected encoded picture
  is decoded. Bitmap dimensions/alpha and displayed aspect are retained on white label paper. This does
  not claim mixed selected-content or exact caret/picture mapping parity.
- Portable regressions cover pixels/alpha, small source geometry, bounded decoding, unchanged document
  state and preference privacy. Workspace 466/0/1; GNU all targets compile. Native DOC color/copies/source
  checks await Windows CI.
- f47e26a run 42432232653 passes build/MSVC stages but its PDF artwork GUI scenario stops before assertions
  because the test does not wait for the new rotated/cropped canvas. Corrected the wait; no label GUI pass
  has been claimed. The new native flatten test also checks crop/rotation preservation.

### Native artwork verification and shared label preview

- 6f1854c run 42433032037 succeeds: 412 targeted MSVC/0 failures/1 optional external OCR fixture ignored,
  all 30 groups and static runtime gate. Actual PDF artwork/region/annotation/unsaved-history and DOC
  picture aspect/alpha/copies/source cases execute and pass; the earlier canvas timing issue is fixed.
- Shared preview with navigation, zoom/pan, resizing, translated controls, Save/Settings/Cancel/regeneration
  is implemented. Rendering holds one page bitmap. Its new native screenshot and interaction checks await CI.
- Actual-size PDF printing streams pages to the printer DC, keeps embedded resources until EndDoc and uses
  paper/range/copies/calibration controls. EMF vector output and virtual/physical driver behavior remain
  unverified; the workflow records the print environment. Portable 466/0/1; GNU all targets compile.

- Run 42435906021 (4f541c6) builds all six apps and passes the static import gate, but the new native
  EMF probe finds no path records for its rectangle-only fixture. Its panic poisons the shared test mutex,
  so the following bitmap probe reports an unavailable engine. No preview/printing pass is claimed.
  The gate now includes a nonrectangular path, retains EMF/record diagnostics even on failure, releases
  the engine before assertions, and checks the Boolean return from the exact pinned PDFium print API.
- A menu-only PDF area gesture is implemented: reverse drag, Escape, crop/rotation mapping and a one-shot
  source/history token feed the same artwork preview. Portable tests cover all four rotations, degenerate
  bounds, immutable source/history, one-shot consumption and stale-state refusal. Workspace: 469/0/1;
  GNU targets compile. Real Windows mouse/output checks are added and await the next compatible build.

- Run 42436434958 (4ffa65b) succeeds: the embedded preview's page/zoom/resize/save/regeneration assertions
  actually execute; inspected fit/narrow screenshots show the page and controls. The EMF contains one
  triangle path, a native brush rectangle and zero bitmap transfers. This establishes native vector
  output for the opaque fixture, not arbitrary transparency or a printer spool. Inventory confirms the
  runner has only Microsoft Print to PDF. Source-area and virtual-print audit races are being corrected.
- DOC mixed-selection artwork and native selected-text offsets are implemented. Selection RTF is isolated,
  measured natively and rendered at up to 300 DPI within a sixteen-million-pixel bound. Live content,
  clipboard and history remain outside that capture. Native unit/UI regressions cover colored images,
  surrounding-content exclusion, UTF-16 offsets after pictures and source/history. GNU targets compile;
  Windows execution is pending. Vector fidelity for mixed selected text is still incomplete.

- The macOS DOC/XLS/PPT/PDF app delegates explicitly set backup autosaving to thirty seconds. Shared
  Windows journals now implement that interval in those four apps plus SCN projects: bounded atomic
  records, exclusive live-session locks, original-container exports, readonly draft capture, inactive-copy
  startup/File-menu UI, independent unsaved restoration and normal save/discard cleanup. Protected PDFs
  are excluded rather than backed up as plaintext; IMG recovery remains open.
- Portable regressions check damaged/truncated records, stale replacement, lock/claim exclusion, quota,
  normal cleanup after a failed write, source safety, cache save destinations, pending-save identity,
  XLSX/ODS opaque parts and PPT inline-draft Cancel. Native forced-termination/restart checks for all five
  adapters are added and await Windows CI. Source implementation does not establish crash runtime parity.
- Run 42438400230 passes 418 targeted MSVC tests; DOC native mixed selection actually passes. PDF label
  area Escape fails because the key event did not reach Cancel; the dispatcher/source fix is included.
  Focused run 42439054997 succeeds on the same binaries: six actual app groups, Print Cancel, explicit
  Microsoft Print to PDF, page 2 only, A4 and actual-size ink bounds. Poppler separately confirms the
  driver-created nonblank single-page A4 output. Main, release assets and the site remain unchanged.

- Focused run 42440754175 passes all five actual crash/restart/recover/save/reopen workflows on 02ad18e,
  including active-session exclusion, independent dirty copies, Cancel and unchanged originals. Eleven
  groups pass; DOC/SCN screenshots were inspected. Run 42439920521 passes 430 targeted MSVC tests; its
  remaining GUI failures were a transient clipboard lease plus harness assumptions about a disabled
  startup owner and the binary SCN42 header. The source retry/harness fixes are recorded separately.
- The QR in the actual Microsoft-driver output from 42439920521 independently decodes to PCB-PRINT-42
  with ZXing-C++ 2.3.0 after Poppler rendering at 300 DPI. This is virtual-driver evidence, not a physical scan.
- Ordinary File/Ctrl+P printing is added to all six hosts. PDF normal printing uses a readonly vector job
  with Mac-style centered/no-upscale fit and native ranges; explicit image/DPI fallback remains. PPT scenes
  include an inline draft without committing it. DOC's primary toolbar is compact. Placement/draft tests pass;
  real keyboard, new PDF print-flag/geometry/history and damaged-backup deletion checks await Windows CI.
- DOC character commands now use the native caret/selection for insertion attributes, table cells and
  cross-paragraph ranges, with guarded one-step projection commits and native rollback on failure.
  Font initializes from the selection; Cancel is readonly. The corresponding native unit/UI checks are
  included; runtime verification awaits a compatible Windows build.

- Run 42443183563 (0daaf47) artifacts record 486 MSVC tests (zero failures, one optional test ignored)
  and all 35 native groups passing. Ctrl+P works in all six apps. The actual Microsoft PDF-driver output
  verifies crop/rotation, page range, no-upscale fit, unsaved edits and print-only/nonprinting appearances,
  with source/history and Undo/Redo intact. Damaged-copy deletion and clipboard contention pass.
  The recovered DOC screenshot confirms one primary toolbar row; native caret formatting is a later change.
- IMG42 now participates in common recovery with a bounded lossless sample/metadata snapshot across all
  frames, delays/loops, active frame, density, 16-bit data and conversion guards. Portable regressions
  cover exact state, no save/history mutation and malformed records; workspace 490/0/1 and GNU compilation
  pass. The real crash/Cancel/save/pixel/reopen scenario is added; native execution is still pending.
- One-variable Goal Seek now has a menu/prompt, cancelable readonly worker, explicit result confirmation,
  stale-state refusal and one-step validated commit. Portable Mac loan/root and cross-sheet/bisection/
  validation/cancellation/history regressions pass; workspace 496/0/1, GNU targets compile. Native GUI
  scenarios are added and await Windows execution. Full multivariable Solver remains incomplete.
- Focused run 42445175112 on bc35c6d completes 12 groups with explicit failures: native DOC typing
  rejects an RTF picture destination; IMG restoration rejects its private extension; one PDF enumeration
  races a closing dialog. Five other recovery adapters pass. The native character-format unit passes;
  MSVC artifacts record 487 tests for d6264f0 and 491 for bc35c6d, zero failures and one optional skip.
  Source fixes separate private recovery extensions from user-facing filters and tolerate only destroyed
  dialog handles in the harness. A portable regression also reproduces/fixes RTF image/table order.
  Rejected DOC projections are now captured only under the explicit audit-output environment.
- The RTF picture parser now balances nested BLIP UID groups and length-prefixed binary picture bytes,
  including braces/backslashes inside payloads; truncated, negative and mixed encodings are refused.
  Portable import/ordering/body-preservation regressions pass. Actual rejected Windows RTF capture is
  still needed to confirm which producer representation caused the native typing failure.
- Focused run 42445809138 succeeds on the exact 8437ea8 Windows package: all six app groups pass.
  XLS Goal Seek actually exercises readonly trials/No, explicit Yes, a full-precision numeric square-root
  result, unchanged formulas/other cells/source, one-step Undo/Redo, save/reopen, an unreachable target
  and protection. Written A1 is Number 1.414213562373095. No complete-suite parity is implied.
- Run 42445530680 verifies actual recovery in all six apps; IMG recovered pixels/save/reopen pass and its
  screenshot was inspected. Run 42445939068 records 500/0/1 MSVC tests and 36 GUI groups with one DOC error.
  Actual captured RTF reveals default table padding, inline picprop metadata and equivalent PNG re-encoding.
  The guarded adapter now retains exact source picture bytes/metadata by bounded sample matching, refuses
  changed samples/nondefault layout, and passes the exact native fixture with Undo/Redo. Paragraph alignment
  uses native caret/ranges; Font Cancel returns keyboard focus. Portable workspace 501/0/1; GNU compiles.
  The new real toolbar/focus/paragraph-history/persistence scenarios await Windows execution.
- Run 42446821420 (33e173d) succeeds: 502/0/1 MSVC and all 36 native groups. Actual DOC toolbar Font
  Cancel/editor focus, character/paragraph formatting, exact image retention, complete Undo/Redo and
  persistence pass; the resulting screenshot was inspected. All six recovery adapters remain verified.
- SCN rescan now captures a page identity, reuses replacement capacity and validates a whole acquired batch
  before one commit; selection changes cannot redirect it, and stale/error/canceled results preserve state.
  Core/adapter regressions pass including full capacity, OCR clearing, complete history and persistence.
  Workspace 506/0/1; GNU targets compile. The new normal-UI virtual STA/WIC acquisition scenario is pending;
  physical scanner controls/hardware and the other scanning gaps remain open.

- Focused run 42448751616 verifies virtual WIA rescan, exact pixels/DPI, target identity after selection changes,
  old OCR clearing, complete Undo/Redo, Cancel, late callback rejection and source preservation on abff0ab.
  Physical scanning is not covered. The run also catches a DOC harness file-stat race during atomic replace;
  output readiness now retries that temporary missing/locked destination and still requires the full signature.
- Constrained Solver is implemented through the normal Tools menu, cancelable worker and single-commit
  model/value confirmation. Standard sheet-scoped Excel/LibreOffice names round-trip without private metadata;
  unrelated names and cached formulas are preserved on model-only edits. Portable tests cover LP/MIP/NLP,
  numeric/reference constraints, binary/free variables, target mode, failure/cancel/stale state, validation,
  full Undo/Redo and model reference shifts. Workspace 519/0/1, including the foreign-name/cache regression; GNU all-targets compile. Native assertions are added, awaiting their first compatible build.

- Run 42453016096 (558ea36) records 520/0/1 MSVC tests and all 37 native groups without errors.
  Solver's actual LP No/Yes, two-cell optimum, complete Undo/Redo, integer/nonlinear results, protected-sheet
  disablement and independent XLSX/ODS models/prefill pass. The compact seven-field dialog screenshot was
  inspected. Virtual WIA rescan also passes in this full build. No main/release/site publication is implied.
- Explicit spelling review is added to the existing DOC/PPT Rich Edit host: installed Windows language picker,
  real dictionary suggestions, readonly original word, Replace/Skip/Skip All/Cancel and guarded rich-text commits.
  UTF-16 table/object offsets and surrogate boundaries are preserved. It uses the Windows Spell Checking API;
  installed dictionaries determine language availability, and this adds no full-grammar claim. New UI strings
  cover the twenty interface languages. Portable workspace 520/0/1 and GNU all-targets compile; native dictionary,
  raw Rich Edit table/image offsets and complete DOC correction/history/reopen checks await Windows execution.

- Shared View panel visibility now controls the available sidebar, toolbar, status bar and document rulers.
  Existing DPI layout reclaims the space, focus leaves hidden controls, and preferences persist with per-app
  placement while default panels retain old preference JSON. Portable preference checks and GNU compilation
  pass; native six-app toggle/geometry/unchanged-history/relaunch assertions await the next compatible build.

- Runs 42453756002 and 42454331494 verify actual en-US dictionary suggestions and GT_RAWTEXT table/image
  positions (523/0/1 MSVC). GUI correction catches unrelated native font substitutions; exact model assertions
  remain intact. The fixture also now resets Find's selection to exercise the intended first body occurrence.
  Prepared DOC/PPT word changes preserve source runs/styles/links/object geometry, validate the complete native
  text mapping and new projection, then commit; refusal/stale state remains atomic. Portable workspace 524/0/1
  and GNU targets compile. Native repeats remain pending. The panel audit passed DOC/XLS before reaching an
  unavailable PDF toolbar; it now verifies disabled entries against absent controls rather than requiring a
  toolbar in every application. Actual panel persistence/layout assertions are retained for available panels.

- Run 42512382933 (1b49d26): 527/0/1 MSVC tests, all six panel visibility/geometry/history/relaunch scenarios
  pass. The word correction guard refuses an image-paragraph placeholder space absent from model text;
  the original document is preserved. Mapping now permits at most one such space after each expected image
  marker, rejects extra spaces/unmatched objects, and has a new actual Rich Edit/core integration regression.
  Complete correction assertions remain unchanged; the new native repeat is pending.

- PDF headers/footers now offer bundled Noto Sans plus actual Windows Arial/Arial Bold, Times New Roman/
  Times New Roman Bold and Courier New, with color. Immutable TrueType programs carry real glyph advances,
  metrics and Unicode maps into saved page content and batch headers. Restricted/bitmap-only embedding is
  refused and no-subsetting flags are honored. Font data and caches are bounded. Portable embedding/rights/
  Unicode/history/reopen regressions pass: workspace 526/0/1; GNU all targets compile. Native family/style,
  actual embedded-font, independently rendered glyphs, full history/reopen and virtual print checks are added,
  awaiting their first compatible Windows build. This does not add complex-script shaping coverage.

- The actual hidden-Rich-Edit integration test in run 42513695947 captures FFF9/FFFB table delimiters in
  GT_RAWTEXT. The previous image-space hypothesis is withdrawn: the visible space was the normal image
  placeholder representation. Mapping/checker input now recognizes the real table markers; ordinary extra
  spaces remain rejected and all image markers still require corresponding source images. The exact captured
  UTF-16 sequence is retained as the portable regression. Workspace 526/0/1; GNU targets compile. Native
  correction/format assertions are unchanged and await a repeat.

- Run 42514675694 (8ced511): all 531 MSVC tests pass (one explicit hardware-dependent
  test ignored). The actual word-correction UI now passes body/table/emoji, complete style/image
  retention, Cancel/Skip, full Undo/Redo and saved reopen. The five Windows font embedding tests
  pass; the header-font GUI scenario stops on a missing Python `re` import in the new audit.
  The import is corrected for a repeat; the complete font UI/print scenario remains unverified.

- Sparse worksheet projection replaces the native dense text matrix and production filtering/clipboard/
  label/chart/pivot/validation/print text consumers. Immutable shared grid text and physical-row maps avoid
  empty-cell strings and repeated row-map copies. Existing hidden-row edit/clipboard regression now uses
  the sparse host bridge; new format/spill/error/far-cell and full-size host-storage regressions pass.
  Workspace 528/0/1 and GNU all targets compile. Native repeat is pending. Workbook limits remain unchanged;
  virtual geometry and independent large-operation bounds are required before increasing capacity.

- Run 42515993014 (5d62d85) completes all 37 native UI groups without errors. Header-font choices/color,
  actual embedded families and Cyrillic text, independently distinct glyph pixels, selected-page scope,
  Undo/Redo/reopen and Microsoft Print to PDF now pass. Spelling body/table/emoji correction also passes.
- Grid geometry now uses sparse shared size overrides and 64-bit prefix coordinates; painting/row labels
  and header resize hit tests visit only visible cells, skipping hidden runs. Scrollbar scaling preserves
  exact endpoints beyond 32-bit total height. Full-height/far-cell/frozen/hit/merge/scroll regressions and
  existing hidden-row editing checks pass: workspace 530/0/1, GNU all targets compile. Actual native repeat
  is pending; core worksheet limits remain 1,000 by 100 until remaining large operations/file paths migrate.

- External XLSX values/formulas now stream cell records rather than materializing a dense bounding
  rectangle. An independent edge-coordinate workbook checks literal formula-looking text, booleans,
  calculated caches, editing/Undo/Redo/reopen and exact unknown ZIP part preservation. Core 144/0;
  GNU all targets compile. Existing dimensions remain unchanged until remaining paths are bounded.

- Runs 42516256451 (1030c81) and 42516717268 (c3f27da) both succeed: 37 UI groups each, respectively
  533/0/1 and 535/0/1 MSVC tests. Sparse text and virtualized geometry retain the existing actual native
  edit/filter/clipboard/array/history/save/reopen/print scenarios. The arrays grid capture was inspected.
- ODS record import, style/merge projection and style preservation checks skip empty rectangular area.
  Bounded repetition/text decoding and value-only surgical XML updates preserve untouched rich text and
  unknown ZIP data. New independent ODF tests cover repeats, errors, dates, booleans, literal formula-looking
  text, separated gaps, exact unedited markup, complete history/reopen and invalid/oversized repeats.
  Workspace 534/0/1, GNU all targets compile; native repeat pending. Filter/layout/style/structural edits
  retain conservative reconstruction guards. Existing core dimensions remain unchanged for now.


- Run 42518146989 (310ee06) artifacts show all 37 native UI groups passing and 539/0/1 MSVC tests,
  including existing ODS edit/filter/array/cache/history/reopen scenarios on the new import/value patch path.
- Core sheets now accept 1,048,576 by 16,384 dimensions. One-million stored cells/four-MB text and
  independent 100,000-cell rectangle budgets keep large copy/paste/style/fill/sort/CSV/native merge export/
  printing bounded. Validation checks implicit blank ranges without traversal; chart points keep the old
  1,000 maximum independently of dimensions. Dense text projections are test-only, with example clients
  migrated to sparse values. Distant selection scrolls into view; row headers fit seven digits.
  Workspace 538/0/1, GNU all targets compile, Python audit compiles. Tests cover XFD1048576 references,
  independent edge-coordinate XLSX/ODS, native-format round trips, atomic oversized refusals, full-sheet
  validation and printing only a far selected area. Actual native full-size edit/scroll/cache/history/reopen
  audit is added, pending the first compatible build. Automatic viewport expansion and full-sheet formula
  semantics remain open; this does not establish complete XLS42 parity.

- Run 42519317175 (bf46c4c) passes MSVC units but the XLS picture scenario stops before the new
  full-size scenario: its fixed pixel/drag positions still assumed the old 48-pixel row header. The row
  header deliberately grew to 64 pixels to fit seven-digit row labels. Picture sample/drag coordinates
  move by the exact 16 pixels; geometry/history/reopen assertions remain unchanged. Full-size UI
  verification remains pending a repeat.

- Focused run 42520207999 reaches the full-size scenario after the corrected picture test, then times
  out opening its project. The app open boundary still validated every logical coordinate (17 billion
  cells). It now iterates stored inputs from every sheet before replacing the session. A real App::open
  regression opens the full-size file and refuses bad text in a nonactive sheet without replacing the
  live book: app 26/0; GNU all targets compile. A compatible native repeat is still required.

- Explicit-reference SUM/AVERAGE/MIN/MAX/COUNT/COUNTA/PRODUCT and math aggregates now traverse stored
  coordinates and ordered spills. COUNTBLANK checks implicit blanks algebraically. A full-sheet test covers
  typed values, formula-empty strings, errors and spills, with native-format round trips. Whole-axis syntax
  and remaining large-reference functions are still open.
- Native XLSX/ODS literal error export retains error types and formatting while preserving ordinary strings
  containing the same codes. Sixteen codes are checked in independent packages with no private snapshot,
  including direct propagation and ISERROR; unsupported literal XLSX codes refuse. GETTING_DATA's reader
  compatibility path and modern evaluator error recognition are covered. Workspace 541/0/1, GNU all targets
  compile; fixed Swift oracle 492 pass/0 gaps/2 explicit skips. Full-size native scenario now checks SUM,
  COUNTBLANK and distinct saved error/text types too, pending a compatible build.

- Large-grid selection/frozen queries and picture relocation use binary row/axis lookup. A far-object
  regression moves an image to the last Excel cell and checks exact anchor/offsets/pixels and one-step
  Undo/Redo; existing hidden/filtered geometry tests still pass. XLS app 27/0 and GNU all targets compile;
  actual native picture/full-size repeats on this change remain pending.

- Runs 42520609905 (6202605) and 42521108743 (08ac1e0) complete all 37 UI groups, respectively
  544/0/1 and 546/0/1 MSVC tests. Actual full-size open, far selection/scroll, editing, saved scalar/SUM/
  COUNTBLANK caches, literal error/text types and complete history/reopen pass. The far capture was taken
  before the queued repaint and still showed A1; the audit now flushes the invalidated native paint and
  checks actual selected-cell background pixels before capture. That stricter render check is pending.
- Whole-row/column references, absolute markers, quoted sheet qualifiers, INDIRECT and reference INDEX
  now use global bounds and sparse range consumers. Implicit empty references beyond loaded extent are
  valid; the former `Z99` invalid-reference fixture is moved past the actual Excel boundary, with new valid
  empty-edge and invalid-capacity assertions. Native XLSX/ODS standard axis formulas and active-sheet
  metadata reopen without the private snapshot. Workspace 543/0/1, GNU all targets compile, Swift oracle
  492 pass/0 gaps/2 explicit skips. New native whole-column/render checks await a compatible build.
  External active-tab persistence, viewport growth and remaining large-reference functions are still open.

- Run 42523096085 (86e2fd3) succeeds: all 37 UI groups, MSVC 548/0/1. The stricter native
  flushed-paint background assertion passes and the inspected capture shows XFD1048576 with value 7.
  Actual whole-column SUM/COUNTBLANK, editing, saved caches and full history/reopen pass.
- External XLSX/ODS tab-only saves now preserve the selected sheet in standard metadata, update primary
  XLSX selection flags and create absent view/settings containers. Other views, worksheet data/styles and
  unknown package parts are preserved; Undo emits the byte-exact original package. Reordering retains
  the active sheet identity. Workspace 546/0/1 and GNU all targets pass. New normal Add/Switch Sheet UI
  scenario covers independent Office files, tab-only saves, complete Undo/Redo and reopen; native pending.
  Viewport growth, remaining large-reference functions and other parity entries remain open.
