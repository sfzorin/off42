# off42 for Windows

This separate Rust workspace implements six native Windows apps; the macOS Swift apps remain separate. The current
implementation is a work in progress, not a feature-parity or Windows release claim. Native interaction and saved output are checked on Windows CI; see [the parity tracker](PARITY.md) for evidence
and failures. Printing drivers and scanner hardware still need testing on a logged-in Windows desktop.

## Build and run

Install stable Rust with the `x86_64-pc-windows-msvc` toolchain, Visual Studio Build Tools with the Desktop C++
workload and Windows SDK, and Python 3. In a Developer PowerShell from the repository root:

```powershell
cd rust
python scripts/fetch-pdfium.py target/x86_64-pc-windows-msvc/debug/deps
cargo test --locked --workspace --target x86_64-pc-windows-msvc
Push-Location scripts
rc.exe /fo windows-app.res windows-app.rc
Pop-Location
$env:RUSTFLAGS = "-C target-feature=+crt-static -C link-arg=$((Resolve-Path scripts/windows-app.res).Path)"
cargo build --locked --release --target x86_64-pc-windows-msvc -p img42 -p doc42 -p xls42 -p ppt42 -p pdf42 -p scn42
python scripts/fetch-pdfium.py target/x86_64-pc-windows-msvc/release
.\target\x86_64-pc-windows-msvc\release\pdf42.exe
```

Run the other apps from `target/x86_64-pc-windows-msvc/release` similarly. The resource embeds common controls v6 and PerMonitorV2
settings. PDF42 uses the adjacent `pdfium.dll` for text search and selection; the fetch script supplies a pinned
runtime and licenses. `PDFIUM_ARCHIVE` can select a local pinned archive. A custom `PDFIUM_DLL` also requires
`PDFIUM_DLL_SHA256` and `PDFIUM_LICENSE_DIR`. SCN42 OCR requires a separately installed Tesseract executable and
language data; select its path and available languages in the app.

For a GNU cross-build on macOS or Linux, install the `x86_64-pc-windows-gnu` Rust target, MinGW-w64 tools and
Python 3, then run `rust/scripts/package-windows.sh` from the repository root. It creates
`build/rust-port-windows/off42-windows-x64.zip` with PDFium and licenses. `rust/scripts/package-source.sh` creates
the source archive and excludes build outputs and local `AGENTS.md` and `STATUS.md`. Cross-building does not verify
Windows runtime behavior. The Windows CI workflow also builds with MSVC.

## Implemented scope and limits

| App | Current implementation | Important limits |
| --- | --- | --- |
| DOC42 | Rich text, lists, images and tables with merge/split, sizing and formatting; comments, notes, links, range bookmarks and linked TOC; DOCX, RTF, ODT, HTML and Markdown subsets; ruler, preview and print | Office adapters are bounded; individual table-edge styles and revisions are incomplete. TOC requires explicit refresh, which invalidates cached page numbers; unsupported annotation or TOC exports refuse fidelity loss. The editing canvas is not fully paginated. Markdown image sidecar saves are not OS-atomic. |
| XLS42 | Native grid with sizing, frozen/hidden rows and columns, merged cells, selection, typed clipboard and inline editing; sheets, formulas, tables, styles, validation, charts, filters and paginated print; XLSX and ODS adapters | Pivot summaries are materialized, not interactive Excel pivots. Dynamic arrays, complex imported tables and structured references are incomplete; complex ODS changes and active filter/print export are guarded. |
| PPT42 | Mixed-run in-place text editing, 187 preset shapes with adjustment handles, native objects, multi-selection, grouping and rotation, slides, timed cut/fade/push-left slideshow transitions and printing; PPTX/ODP adapters | Complex source text and existing source-group structural edits are guarded. Complex source transforms and rotated source groups may be preserved without rendering. ODP transition duration is limited to 250/500/1000 ms. Object animation, full master/theme semantics, charts and media playback are incomplete. |
| PDF42 | WinRT rendering; PDFium search, selection and supported top-level text-object edits; annotations, forms, nested bookmarks, attachments, password-prompted PDF open and AES-128 copy, compression, printing, image export and raster redaction | Complex text, forms and navigation remain guarded. Protected files use conservative permissions; new AES-128 passwords require 1–32 printable ASCII characters, and original permissions and encryption are not preserved. Encrypted interoperability is unverified. Redaction removes metadata, attachments, forms and navigation. Trusted digital signatures and PDF/A conformance are not supported. |
| IMG42 | Transforms, tone, text, masks and lasso; directory navigation, GIF/WebP frames, multipage TIFF, PDF and bounded SVG import; standard EXIF fields, DPI/ICC, folder batch export and printing | Proprietary MakerNote, TIFF EXIF subdirectories, palette/CMYK and floating-point TIFF remain limited. TIFF and PNG support 16-bit edits; other 16-bit sources may require conversion. Some PNG metadata requires a converted copy; profile conversion is not color-managed. Batch PNG/JPEG exports the first frame; TIFF exports all frames, with explicit 8-bit white flattening. |
| SCN42 | Asynchronous WIA acquisition, PDF import and WIC processing; v4 projects with stable page IDs, DPI, OCR language and geometry; batch Tesseract or Windows OCR, Unicode searchable PDF, deskew, perspective correction, cleanup, compression, image/TIFF export and printing | Tesseract and its language data are external; Windows OCR requires installed languages. Windows OCR and physical devices have not been exercised. |

Shared save snapshots detect changes to existing destinations and use atomic replacement. Multi-file saves stage
sidecars before the main document with verified reuse and best-effort rollback; they are not OS-atomic. Async open
keeps the existing document and session until the new identity is verified and loading succeeds. The apps include
native commands and toolbars, document history, asynchronous work and cancellation. Format support is a subset, and
preservation varies by feature and source file; test important documents before relying on round trips.

## Labels, barcodes and signatures

- In PDF42, DOC42 and XLS42, **Production → Create Barcode** saves a PNG with physical resolution metadata.
  Choose QR, Data Matrix, Code 128 or Aztec. Code 128 accepts printable ASCII. Check the final size after
  inserting a PNG into another application.
- **Production → Create Label Sheet** exports a vector A4 PDF. Set label dimensions and gaps in millimetres,
  a copy count, and the first slot to skip used positions. PDF42 and DOC42 can start with selected text.
  XLS42 uses displayed values and skips hidden rows and columns. Column numbers count visible selected columns.
  Enter 0 to omit the barcode or copy-count column, and 1 in the header field to skip the first visible row.
  Print at actual size (100%). Label text uses the bundled Noto Sans glyphs; scripts that require text shaping
  and characters absent from that font need another text workflow.
- In XLS42, select part of one column and use **Production → Fill Serial Numbers** to enter literal identifiers
  such as `0007` or `PCB-0007`. Filtered or hidden rows are skipped. Undo restores the whole operation.
- In all six apps, **Signatures & Stamps** imports PNG or JPEG copies into the shared local library
  in `%LOCALAPPDATA%/off42/Signatures and Stamps`. Use **Insert from Library** to place an image.
  PNG imports retain transparency; import can remove white paper. IMG42 and SCN42 place the mark in a pixel rectangle
  and support Undo/Redo. SCN42 clears OCR for a changed page and restores it with Undo. These are visual marks.

## Spreadsheet pictures

XLS42 inserts PNG and JPEG pictures at the active cell. Click to select, drag to move, double-click for
position and physical size, or press Delete. Each edit supports Undo/Redo. Pictures follow row and column
insertions, respect hidden anchors and print at their recorded size, including page clipping.
Native XLS42, XLSX and ODS output contains the image and geometry. Supported PNG pictures in external
XLSX/ODS packages can be moved or deleted, and new pictures can be added while preserving unrelated ZIP
parts. Cropped/rotated drawings and unsupported image formats remain opaque. CSV export rejects pictures.
Technical PNG files retain the same editable recipe and physical width used by the macOS apps.

## PDF watermarks, numbering and headers

PDF42 adds text or PNG/JPEG image watermarks, with opacity, rotation and selected pages. Text watermarks
become vector outlines and do not enter search/copy results. Page Numbers supports six positions, custom
starting numbers, skipping the first selected page and three formats. Header & Footer provides six text
slots with `<<page>>`, `<<pages>>`, `<<label>>`, `<<date>>`, `<<file>>` and `<<bates>>` tokens. Bates settings
include prefix, suffix, zero padding, starting number and date text; numbering continues from the next value.
Tokens resolve in document order. `<<file>>` uses the current successful document destination.

A multi-page addition or removal is one Undo step. Live decorations follow the crop box and page rotation.
Saving adds isolated content and preserves existing page resources. After reopening, decorations are ordinary
page content. Headers and numbers use embedded Noto Sans; unsupported glyphs are refused. Other fonts and
automated Bates processing of other files remain parity work. Independent Poppler checks cover four rotations,
nonzero crop origins, source text, Unicode tokens and exclusion of watermarks from extracted text.
Native run 42405270583 passed the dialog, rendered text/image pixels, selected-page numbering, complete
Undo/Redo and dirty/discard/reopen checks on dd33481. This is development-build evidence.

## Presentation shapes

PPT42 offers the same 187 OOXML preset definitions as macOS through **Insert Shape**.
Selected presets have yellow adjustment handles and **Shape Type / Shape Adjustments** commands.
Curves, compound holes, shaded subpaths and text frames are evaluated at the current aspect ratio;
canvas, slide show and printing keep vector paths. Dragging a handle commits one Undo/Redo step.
Projects retain preset identity and adjustments. PPTX writes standard `prstGeom`/`avLst`; ODP writes
OOXML preset types, ordered modifiers and a literal enhanced path. Supported preset changes and resizing
can be saved back into imported PPTX/ODP packages while preserving unrelated package parts.
Mirrored/custom guide formulas outside this subset remain preserved opaque content.

The geometry oracle is regenerated with `python3 rust/scripts/export-shape-cases.py` on macOS.
It compares 620 actual Swift-engine cases across all 187 presets, two aspect ratios and changed adjustments.
The MPL-2.0 data source, notice and license are included in Windows archives under `licenses/drawingml-presets`.
Native Windows insertion/render/handle/history/reopen checks passed in run 42366299902 on source `efe85f7`.
The independent Office audit also checks all 187 catalog labels and physical slide size in both formats.

## Native text proofing

DOC42 and PPT42 enable inline spelling through Windows Rich Edit and TSF. The Edit menu toggles it.
Available languages and proofing tips depend on installed Windows text services.
[Microsoft documents the Rich Edit language options](https://learn.microsoft.com/en-us/windows/win32/controls/em-getlangoptions).

## Validation boundary

Workspace tests and cross-build checks do not establish native event dispatch, controls, rendered output, DPI,
printing, scanner acquisition or third-party interoperability. On Windows, exercise actual controls and keyboard
commands, Undo/Redo, failure paths and save/reopen for representative documents in each app. Test PDFium alongside
the packaged executable, print to file, and separately test scanner hardware and installed OCR languages. Retain
fixtures for defects and record the executable identity and any skipped checks.

## Technical objects inside documents

In PDF42, DOC42, XLS42 and PPT42, **Production → Insert Symbol or Code** places a QR, Data Matrix,
Code 128, Aztec or technical symbol at the current document location. **Edit Symbol or Code** changes
the saved value, format, caption and width in millimetres. Choose an object if several are available;
PDF42 lists the current page. Insertion and replacement each form one Undo step.
PNG recipes survive DOCX/ODT/RTF, XLSX/ODS and PPTX/ODP exports for the supported image objects. PDF42
uses the same `/Suite42TechnicalElement` annotation key as the macOS implementation and restores the
editor after reopening. PDF42 stores resource-free vector paths, including outlined captions; Office
formats retain PNG pictures with recipes. **Create Symbol or Code** exports PNG, SVG or an editable
vector PDF without changing the current document.

## Label data sources and remembered options

DOC42 can use a document table for labels. When tables are present, choose the table or custom/selected
text before setting the text, code and copy-count columns. Covered merged cells remain empty in the
data snapshot. Export does not edit the document.
Label dialogs remember their layout, stock, code format and copy/start settings in local preferences.
Column mappings are restored only when the source header signature matches; document text and code
payloads are never stored in these preferences. Text labels and tabular sources have separate profiles.

### Windows interface languages (development branch)

View → Language lists the same twenty languages as macOS, using their native names. English is the default.
The chosen language is stored for all six Windows apps; switching asks about unsaved work and restarts the app.
`--language ru -- document.docx` overrides only the current process. Bundled catalogs translate matching menus,
toolbar actions, prompt labels and choices; Windows-specific strings without a matching entry fall back to English.
Document text, barcode values, filenames and formula syntax are not translated. Windows system dialogs follow
Windows settings. Arabic prompt labels align right; document and grid coordinates retain their normal layout.
Native File menus and technical prompts pass data-preservation checks in all twenty languages. Six prompt
screenshots were inspected. The symbol/code dialog and its insertion/edit menus also have explicit Windows translations. Complete string
coverage remains separate. The shared picker restart/next-app-language scenario is included in CI.

### Recent documents (development branch)

File → Open Recent lists the ten most recently opened or saved documents for each app. Selecting one uses the
normal document open path and unsaved-work confirmation. Clear Menu removes only this local list; files remain
unchanged. Paths are kept in `%LOCALAPPDATA%/off42/Recent Documents/`; missing files report the normal open error.
Failed opens and uncommitted asynchronous imports are not recorded. Native interaction checks are included in CI.

### PDF page export (development branch)

PDF42 File → Extract PDF Pages accepts a list such as `1, 3-5` and descending ranges such as `5-3`.
Split PDF Pages to ZIP creates one PDF per chosen page with ordered filenames. Overlapping selections are rejected.
Export works on a captured snapshot in a background worker; Cancel PDF Page Export requests cancellation between
pages. Files publish only after completion and never replace an existing destination. Export does not change the
source or mark it saved. Current export bounds are 1,000 pages, 256 MB per PDF and 512 MB total split PDF data.
Page labels, supported bookmarks, forms and annotations follow retained page references. Tagged-document page
removal and unresolved references are refused rather than discarded. Native menu/output checks are included in CI.

### PDF page labels (development branch)

Page Labels edits an inclusive range with decimal, Roman, PDF letter numbering or prefix-only labels. Prefixes
support Unicode. The page list displays logical labels, and Go to Page accepts a label or a physical page number.
Clear Page Labels and changes to ranges support Undo/Redo. Labels stay with their pages after reordering and
extraction and are written into the standard PDF page-label tree. Source extremes have bounded display formatting.

### Engineering formulas (development branch)

XLS42 calculates decimal/binary/octal/hexadecimal conversions, `BASE`, `DECIMAL`, bitwise AND/OR/XOR/shifts,
`DELTA`, `GESTEP` and `CONVERT`. The conversion table is based on the macOS 112 units and 27 SI/binary prefixes,
including squared/cubed dimensions, information sizes and temperature offsets. Binary prefixes are limited to
information units. Formula names containing digits and Excel `_xlfn.`/`_xlws.` prefixes retain normal A1 references.
Signed conversions use the respective 10/30/40-bit formats; bitwise values are bounded to 48 bits. These bounds
follow the suite and [Microsoft's engineering function reference](https://support.microsoft.com/en-us/excel/engineering-functions-reference).
Raw formulas remain authoritative in saved workbooks. This adds a subset of the macOS function catalog;
dynamic arrays and the remaining functions are tracked separately.

The Windows exporter corrects the source table's `flb` factor to foot pound-force, derived from its `lbf` and `ft`
constants. Microsoft names that unit [foot-pound](https://support.microsoft.com/en-us/excel/functions/convert-function);
[NIST distinguishes it from foot poundal](https://www.nist.gov/pml/special-publication-811/nist-guide-si-appendix-b-conversion-factors/nist-guide-si-appendix-b9).
The macOS source correction is separate from this Windows branch.

Generated XLSX keeps formula text results as strings, including numeric-looking identifiers with leading zeroes.
Cached Boolean, number and standard error values keep their corresponding types. ODS stores scalar text/Boolean/
number caches; cached error display remains textual because the ODS writer has no distinct error value type.

### Windows runtime packaging (development branch)

New MSVC builds use a static C runtime, configured for the explicit Windows target. Host build scripts and procedural
macros keep their host flags. Release resource flags also retain `+crt-static`. Before packaging, CI inspects ordinary
and delayed PE imports for all six apps and PDFium; non-system dependencies other than bundled PDFium fail the build.
The import report includes binary hashes. This gate is checked before ZIP creation. The existing public preview.3
still uses a separate VC++ runtime as described in its release notes; source configuration does not replace that asset.


### PDF text export (development branch)

**Export Text…** offers TXT, RTF, HTML and DOCX in one settings dialog, with an explicit page list and format.
The picker uses the selected extension. Extraction runs in the background against the current saved-output
snapshot: unsaved page edits and applied raster redactions are reflected, and the source PDF is not modified.
Cancel is available during extraction; publication uses a temporary file and refuses an existing destination.

Text follows PDFium reading order. Rich formats retain basic font family, size, bold/italic and text color, and
page boundaries (form feeds in TXT). This is text extraction, not reconstruction of PDF columns, tables, images,
positioned layout or embedded fonts. Scans without a text layer need OCR first. Exports are bounded to 1000 pages,
4 MB of text and 100000 style runs. Unmapped glyphs are represented by a replacement character.


### Text formulas and reference coverage (development branch)

XLS42 adds PROPER, CLEAN, NUMBERVALUE, TEXTBEFORE/TEXTAFTER, SUBSTITUTE, REPLACE, REPT, FIND/SEARCH,
CHAR/UNICHAR, CODE/UNICODE, T/N and TEXTJOIN. CONCAT/CONCATENATE and TEXTJOIN accept row-major ranges,
preserving Boolean values and empty slots. SEARCH supports case-insensitive `*`, `?` and `~` escapes;
text-before/after support negative occurrences, optional defaults and end matching. Search work and output size
are bounded. NUMBERVALUE accepts explicit decimal/group separators; defaults use the existing `.`/`,` formula
convention. CHAR/CODE follow off42's Unicode/Latin-1 reference semantics, not the Windows ANSI code page.
Invalid code points/counts and formula errors are reported rather than silently clamped.

`cargo run --manifest-path rust/Cargo.toml -p xls42-core --example formula-parity` prints a JSON report for 494
macOS reference cases, including every mismatch. Two cases have locale-dependent expected output and are skipped.
After this block, 136 fixed cases match and 356 differ. This measures example results, not complete function
coverage or Excel conformance; arrays and many statistical/financial/date functions still need porting.
Regenerate the fixture with `python3 rust/scripts/export-formula-cases.py`; its source hash is recorded.


### Additional math formulas (development branch)

A further 36 math functions cover inverse/hyperbolic trigonometry, reciprocal functions, combinations and
permutations, double factorial, GCD/LCM, sums of squares, multinomial coefficients, MROUND, EVEN/ODD and the
MATH/PRECISE ceiling/floor variants. TRUNC now accepts decimal digits. Domain errors, arity errors and overflow
remain distinct, with bounded integer/combination loops and finite-result checks. Range aggregates preserve the
existing rule that text and Boolean source cells are excluded.

The reference report now matches 176 fixed examples, with 316 differing and the same two locale cases skipped.
This remains far short of complete formula parity. FACTDOUBLE rejects negative inputs according to the documented
Excel domain; the macOS reference currently permits -1 and needs a separate correction, like the previously recorded
foot-pound unit constant. External XLSX/ODS regressions change input values before asserting recalculation.


### Document windows (development branch)

**File → New** opens another blank document in DOC42, XLS42, PPT42 and SCN42, retaining the existing document.
**File → New Window** (`Ctrl+Shift+N`) opens another window in all six apps. Each window has an independent process,
file session and Undo/Redo history; it inherits the current interface language. Windows can be saved and closed
independently. This does not add document recovery autosave or change file associations.


The apps remember their normal window rectangle and maximized state when a window closes. The next window of
that app restores the saved placement; Windows relocates a completely offscreen frame onto a current display.
Settings live under `%LOCALAPPDATA%/off42/Window Layout`. Corrupt/unsupported preferences use the normal default
window. This covers outer placement only; panel visibility, split positions and cross-display DPI still need work.


### Independent barcode QA

`cargo run --manifest-path rust/Cargo.toml -p label42 --example barcode-fixtures -- /path/to/fixtures`
creates thirteen PNG/PDF pairs and a case manifest. In a separate QA environment with Pillow, `zxing-cpp==2.3.0`
and Poppler, run `python3 rust/scripts/audit-barcodes.py /path/to/fixtures`. It independently decodes the PNGs and
Poppler PDF renders, checking both format and exact payload. These are optional developer tools, not app runtime
requirements. The 26-output check passes for ASCII, Cyrillic, mixed CJK/Arabic/emoji and longer payloads; Code 128
cases use printable ASCII. This does not replace checking actual printed labels at their final physical size.


### Find and Replace in structured documents (development branch)

DOC42's literal **Find next** uses actual native Rich Edit offsets, so table cells, pictures and surrogate pairs
no longer break selection or wraparound. **Replace all** includes body/table paragraphs while preserving picture
bytes/geometry, run formatting and hyperlinks. Bookmarks, comments and note anchors follow the changed text;
Undo/Redo covers the whole operation. Replacements across rich paragraph boundaries, cached TOC content and
matches covering note markers and matched revision/field anchors remain guarded. It does not add regex or whole-word options.

ODT import also retains surrounding body bookmarks when a paragraph contains an inline footnote/endnote. Portable
regressions reopen project, DOCX and ODT output and verify bookmark destinations plus tables, pictures, comments and
notes. Replacement output is capped at 16 MB and 100000 occurrences before document mutation.


### Verified development build

[Windows validation run 42360592434](https://github.com/sfzorin/off42/actions/runs/42360592434) builds source
`6e4c55bdc5d9a4015428267a924800e162c17b6c` with release optimization: 377 MSVC tests passed (one OCR fixture ignored),
all 30 native result groups passed, and packaged binary dependency checks passed. The `off42-windows-x64` workflow
artifact contains the six apps, PDFium and licenses. ZIP SHA-256:
`ede6e36ba0dc3a81673ed79680d988e129fe307838db202d3a09cd8a3cbeac7c` (61,197,732 bytes).
This is a development artifact, separate from the public preview.3 download; it is not a full parity claim.


## Body statistics and statistical formulas (development branch)

DOC42's **Statistics…** dialog counts body words, grapheme characters with/without spaces and nonempty
paragraphs, including table cells even when pictures prevent a plain editor projection. The status word count
uses the same whole-body traversal. Counting is read-only and retains formatting/history. It excludes notes,
comments, image alternate text and page decorations. Unicode word boundaries do not establish the same
language-specific dictionary segmentation as macOS; selection statistics and layout line counts remain work.

XLS42 adds descriptive statistics, ranks/percentiles/quartiles, regression/covariance and probability
distributions/inverses (normal, lognormal, Student t, chi-square, F, gamma, beta, binomial and Poisson),
confidence intervals, Fisher transforms and error functions. Scalar/reference coercion, blanks, errors and
A-variant Boolean/text handling remain distinct. Scientific numeric literals are supported; iteration and
argument text budgets refuse excessive computation or allocation. Small right-tail probabilities are evaluated
directly to avoid subtraction from one. Dynamic statistical arrays remain incomplete.

The fixed macOS report now matches **293/492** cases, with **199 differences** and two additional locale-dependent
cases skipped. A separate checked-in oracle contains **277** mpmath cases at 160-digit precision; regenerate it
with `rust/scripts/export-distribution-cases.py` in a QA environment containing mpmath 1.3.0. This is a QA-only
dependency. These counts measure examples rather than complete function or Excel compatibility.

Office output rebuilds the ZIP container and registers its optional native snapshot in XLSX content types or
the ODF manifest. ODF formula names distinguish modern left-tail F functions from legacy right-tail aliases.
`statistical-fixtures` and `rust/scripts/audit-statistics-office.py` independently load and recalculate both full
packages in LibreOffice. Nineteen of twenty XLSX-origin cases and eighteen of twenty ODS-origin cases agree;
the audit records LibreOffice's invalid-percentile error classifications and its ODS ERF interval precision
loss separately. Native results retain the independently checked values. Native runs 42408293307 (DOC statistics) and 42409672916 (XLS formulas) verify the added scenarios;
the first run also exposed the standard editor replacement-history defect fixed in the second.


## Financial calculations and calendars (development branch)

XLS42 adds PV/FV/NPER, interest/principal and cumulative loan payments, RATE, IRR/XIRR/XNPV/MIRR,
depreciation, effective/nominal rates and growth-duration calculations. The financial family shares typed
arguments and bounded iterations; root finding stays in the rate > -1 domain, normalizes cash flows and uses
a bracket fallback. Future balances and interest calculations avoid amplification of rounded payment errors.
A separate 112-case mpmath oracle checks 100-digit reference values, including tiny rates and long terms.
`rust/scripts/export-finance-cases.py` regenerates this QA fixture; mpmath is not an app dependency.

Calendar functions include EDATE/EOMONTH, weekday/week numbers, DATEDIF, DAYS/DAYS360/YEARFRAC, working-day
counts and shifts with custom weekend masks/holidays, TIMEVALUE and time components. Calendar arithmetic
retains Excel's fictional serial day 60 and workbook date offsets; business-day searches avoid stepping through
millions of days. Real ISO dates entered in cells now have a date type, including in holiday/cash-flow ranges.
XLSX/ODS retain those entered values and recalculate after external editing. Automatic local date-entry formats,
the fictional date as typed ISO text and broader ODS number-format editing still need work.

The fixed macOS example report now matches **356/492**, with **136 gaps** and two locale cases skipped. This
remains incomplete formula parity: array literals/dynamic arrays and several lookup/information families remain.
Independent LibreOffice checks on 46 full-package cases agree on all 26 new financial/calendar examples in both
formats; the previously recorded three external statistical differences remain explicit (45/46 and 44/46 total).
Native run 42409672916 passed formula/date editing, typed caches, complete Undo/Redo and dirty/discard/reopen
on a666c7b. It passed 284 targeted MSVC tests and all 30 native result groups. This is a development artifact.


## Conditions and cell information (development branch)

XLS42 adds lazy IFS/SWITCH/CHOOSE, optional IF branches, MAXIFS/MINIFS, logical aggregation over typed
ranges, NA/error literals and information functions for types, formulas, addresses, sheet/reference metadata
and simple CELL queries. Nested calculations retain the original formula-cell position for ROW/COLUMN/CELL.
AND/OR now propagate errors from every operand, matching the macOS engine; IF/IFS/CHOOSE evaluate only the
selected result. Imported error cells remain distinct from text that looks like an error. RAND/RANDBETWEEN
use an in-process generator and validate finite, exactly representable integer bounds.

The fixed example report now matches **398/492**, with **94 differences** and two locale skips. Array results,
computed reference expressions and further CELL metadata remain incomplete. Independent LibreOffice QA checks
all 19 new logical/information examples from both formats: 64/65 XLSX-origin and 63/65 ODS-origin cases agree;
only the previously recorded external statistical differences remain. Portable workspace: 407 tests pass,
none fail, one OCR fixture is ignored; all GNU Windows targets compile. Full optimized run 42410651353 passed 412 MSVC tests and all 30 native groups on 5932667,
including the added logical/information caches and history assertions.


## PDF text comparison (development branch)

**Compare With…** compares the current unsaved PDF snapshot with another PDF in the background. The native,
resizable report lists page, removed words and added words; selecting a row navigates to and selects its source
text. Removed words receive temporary red highlights, replacements orange. Closing the report removes temporary
highlights. **Keep highlights when saving** explicitly adds ordinary PDF highlight annotations as one history
transaction; **Remove Comparison Highlights** removes live comparison marks and supports Undo/Redo. Other
annotations remain intact. **Cancel PDF Comparison** cancels pending work without replacing the document.

This compares extracted words page by page; it does not compare page artwork, typography or punctuation, and
does not recognize scanned images. Unicode word boundaries need not match macOS dictionary segmentation for
all languages. Ten checked-in cases from the actual macOS Foundation comparison implementation agree, including
Cyrillic, Arabic, Greek, Turkish, repeated words, page changes and electrical values. Regenerate them with
`python3 rust/scripts/export-comparison-cases.py` on macOS with Swift available.

Comparison has explicit text/word/change/work budgets and linear working memory for the word diff. Source and
history identities discard stale background results without hashing or serializing every page on each UI poll.
Portable tests cover exact temporary-pixel positions at four rotations/nonzero crop origins, black-text retention,
colored saved annotations, one-step history and separate document identities. Workspace: 414 pass, zero fail,
one OCR fixture ignored; all GNU Windows targets compile. Native report/pixel/keep/history/reopen assertions
are included for the next compatible Windows build.
