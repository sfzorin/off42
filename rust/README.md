# off42 for Windows

This separate Rust workspace implements six native Windows apps; the macOS Swift apps remain separate. The current
implementation is a work in progress, not a feature-parity or Windows release claim. Native interaction and saved output are checked on Windows CI; see [the parity tracker](PARITY.md) for evidence
and failures. The Microsoft virtual PDF driver has been exercised; physical printing and scanner hardware still need testing.

## Build and run

Install stable Rust with the `x86_64-pc-windows-msvc` toolchain, Visual Studio Build Tools with the Desktop C++
workload and Windows SDK, CMake/Ninja, Git, and Python 3. In a Developer PowerShell from the repository root:

```powershell
cd rust
./scripts/build-ocr.ps1
python scripts/fetch-ocr-models.py target/x86_64-pc-windows-msvc/release
$env:OFF42_OCR_DATA = (Resolve-Path target/x86_64-pc-windows-msvc/release/ocr/tessdata).Path
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
settings. PDF42 uses the adjacent `pdfium.dll` for text search and selection; DOC42/XLS42 also use it for
label preview and PDF printing. The fetch script supplies a pinned
runtime and licenses. `PDFIUM_ARCHIVE` can select a local pinned archive. A custom `PDFIUM_DLL` also requires
`PDFIUM_DLL_SHA256` and `PDFIUM_LICENSE_DIR`. PDF42 and SCN42 use the statically linked OCR engine and twenty pinned offline models.
Keep the complete `ocr` folder beside the executables. SCN42 also retains optional external Tesseract support.

GNU all-targets `cargo check` remains supported. Native packaging currently uses MSVC; the legacy GNU
packaging script requires a separately cross-built static OCR SDK and is not a verified release path. `rust/scripts/package-source.sh` creates
the source archive and excludes build outputs and local `AGENTS.md` and `STATUS.md`. Cross-building does not verify
Windows runtime behavior. The Windows CI workflow also builds with MSVC.

## Implemented scope and limits

| App | Current implementation | Important limits |
| --- | --- | --- |
| DOC42 | Rich text, lists, images and tables with merge/split, sizing and formatting; comments, notes, links, range bookmarks and linked TOC; DOCX, RTF, ODT, HTML and Markdown subsets; ruler, preview and print | Office adapters are bounded; individual table-edge styles and revisions are incomplete. TOC requires explicit refresh, which invalidates cached page numbers; unsupported annotation or TOC exports refuse fidelity loss. The editing canvas is not fully paginated. Markdown image sidecar saves are not OS-atomic. |
| XLS42 | Native grid with sizing, frozen/hidden rows and columns, merged cells, selection, typed clipboard and inline editing; sheets, formulas, tables, styles, validation, charts, filters and paginated print; XLSX and ODS adapters | Pivot summaries are materialized, not interactive Excel pivots. Dynamic arrays, complex imported tables and structured references are incomplete; complex ODS changes, external filter reconfiguration and ODS print export are guarded. |
| PPT42 | Mixed-run in-place text editing, 187 preset shapes with adjustment handles, native objects, multi-selection, grouping and rotation, slides, timed cut/fade/push-left slideshow transitions and printing; PPTX/ODP adapters | Complex source text and existing source-group structural edits are guarded. Complex source transforms and rotated source groups may be preserved without rendering. ODP transition duration is limited to 250/500/1000 ms. Object animation, full master/theme semantics, charts and media playback are incomplete. |
| PDF42 | WinRT rendering; PDFium search, selection and supported top-level text-object edits; annotations, forms, nested bookmarks, attachments, password-prompted PDF open and AES-128 copy, compression, printing, image export and raster redaction | Complex text, forms and navigation remain guarded. Protected files use conservative permissions; new AES-128 passwords require 1–32 printable ASCII characters, and original permissions and encryption are not preserved. Encrypted interoperability is unverified. Redaction removes metadata, attachments, forms and navigation. Detached CMS integrity and Windows cached certificate-chain trust can be inspected. A first detached approval signature can be created in a separate copy. Trusted timestamps and PDF/A conformance remain open. |
| IMG42 | Transforms, tone, text, masks and lasso; directory navigation, GIF/WebP frames, multipage TIFF, PDF and bounded SVG import; standard EXIF fields, DPI/ICC, folder batch export and printing | Proprietary MakerNote, TIFF EXIF subdirectories, palette/CMYK and floating-point TIFF remain limited. TIFF and PNG support 16-bit edits; other 16-bit sources may require conversion. Some PNG metadata requires a converted copy; profile conversion is not color-managed. Batch PNG/JPEG exports the first frame; TIFF exports all frames, with explicit 8-bit white flattening. |
| SCN42 | Asynchronous WIA acquisition, PDF import and WIC processing; v4 projects with stable page IDs, DPI, OCR language and geometry; batch built-in or external Tesseract OCR, Unicode searchable PDF, deskew, perspective correction, cleanup, compression, image/TIFF export and printing | The bundled OCR backend is verified on Windows; optional external Tesseract needs its own data. Physical devices have not been exercised. |

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
page content. Headers and numbers use embedded Noto Sans; unsupported glyphs are refused. Other font choices
remain parity work. **Also number other PDF files** continues the current Bates sequence through selected PDFs
in natural filename order. **Apply Headers to Other PDFs** repeats the saved settings later. Outputs are copies
in a selected folder, with no overwrite. A per-file atomic save means cancellation retains completed copies; the
report lists saved, failed and cancelled files, and only successful copies consume Bates numbers. Limits are
128 files, 1000 pages, 256 MiB input and 512 MiB output. Locked files require an unlocked copy first. Independent Poppler checks cover four rotations,
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
The current engine matches all 492 fixed cases. This measures example results, not complete
function coverage or Excel conformance; unsupported regular-expression syntax, locale formatting and wider function semantics still need verification.
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

DOC42 searches the native Rich Edit text with next/previous wrapping, match case, whole words and
regular expressions. Replace can update the selected match and advance, or replace all matches in one
history step. Regular expressions accept capture references such as `$1` in replacements. Rich
replacement preserves table/image geometry, run formatting, hyperlinks and remapped annotation
anchors. Rich replacements are scoped to body and table paragraphs. Matches that would alter note
markers, revision/field anchors or imported cached TOC entries are guarded. Regex lookaround and
pattern backreferences are still outside the supported subset. The native Windows test verifies the options, structured selection, history and reopen.

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
paragraphs and displayed lines. When text is selected, it shows the same counts for the selected native
range, including across table cells and images. The line count follows the current editor layout and wrapping.
The status bar shows the whole body word count. Text statistics exclude notes, comments, image alternate text
and page decorations. Counting preserves the selection and document history. Dictionary segmentation and
paginated page counts remain work.

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
one OCR fixture ignored; all GNU Windows targets compile. Native run 42412900551 passed the report, row navigation, temporary orange pixels, explicit keep, clear/history
and saved/reopened PDFium text/annotation checks on 07e178d (294 targeted MSVC tests, all 30 groups).


## Merge PDF files (development branch)

**Merge Files…** selects multiple PDFs and a new output filename, preserves their selected order and performs
the merge in the background. The completed output opens in a separate window; the current document and its
unsaved history remain intact. **Cancel PDF Merge** stops pending work. Output publication is atomic and never
replaces an existing file, including a race after the picker. Input files are read only.

The merge retains page geometry, ordinary annotations, supported bookmarks/forms and logical page labels.
Imported label prefixes and numbering rules are resolved before PDF object IDs change, including indirect
values. Work is limited to 128 files, 1000 pages and 256 MB of input data. Unsupported imported document names,
named destinations, tagged structure and active document actions are refused rather than silently discarded;
XFA/signature restrictions still apply. This is not unrestricted format fidelity.

Portable regressions cover order, crop/rotation, annotations, bookmark targets, label prefixes, cancellation,
corrupt inputs and no-clobber publication. Independent Poppler rendering produces byte-identical page PNGs
before and after a three-page merge. Workspace: 418 tests pass, zero fail, one OCR fixture ignored; GNU Windows
all-targets compilation passes. Native multi-file picker/cancellation, separate result window, preserved dirty
parent/history and PDFium text/labels/bookmarks checks await a compatible build.

## Computed references and filtered summaries

OFFSET, INDIRECT (A1 and absolute/relative R1C1) and reference-form INDEX keep their source rectangle for
SUM/statistical functions and reference metadata. INDEX with zero row/column selects the corresponding column/row.
XMATCH supports exact, nearest and wildcard matches with forward/reverse search; HLOOKUP searches rows.
External workbook references are refused; array-valued INDEX and dynamic-array expressions remain incomplete.

SUBTOTAL and AGGREGATE exclude filtered rows, optionally exclude manually hidden rows, and handle nested totals
and error-skipping options. The native contains-text filter is written as a standard ODS database range as well
as an XLSX autofilter. Simple single-column contains filters are imported without relying on the private project
payload. Input edits recalculate filtered visibility before external XLSX/ODS output; unrelated package data
remains guarded by the preservation checks. General filter expressions and external filter reconfiguration remain
parity work. XLSX cannot distinguish a manually hidden row that also fails the filter from a row hidden by the
filter alone; the private native snapshot retains that distinction.

Imported XLSX/ODS saves now refresh supported scalar formula caches throughout the workbook, including
dependent sheets and formulas whose text did not change. Numeric, Boolean, string and error results retain their
file types; the original formulas, cell styles, relationships and unrelated parts remain in place. Unsupported
results and repeated ODF formula caches are cleared after an edit so an office application can recalculate them.
This does not add dynamic arrays or general imported calculation-chain editing. The cache audit can inspect saved
values without recalculating: `python3 rust/scripts/audit-statistics-office.py --fixtures DIR --output DIR --cached-only`.

XLSX imports hide Excel's `_xlfn.`/`_xlws.` compatibility prefixes in user-facing function tokens, including
FORMULATEXT results. Quoted document strings, sheet names and original package formulas keep their text.


### Calculated arrays and clipboard values (development branch)

XLS42 calculates bounded rectangular operands, arithmetic over ranges, array constants, LET, SEQUENCE/RANDARRAY,
SORT/SORTBY, FILTER/UNIQUE, TRANSPOSE, VSTACK/HSTACK, TOCOL/TOROW, CHOOSECOLS/CHOOSEROWS, TAKE/DROP,
EXPAND, WRAPROWS/WRAPCOLS, matrix multiplication/inversion/determinants, FREQUENCY, MODE.MULT and SUMPRODUCT.
Dynamic results fill empty cells within the current sheet dimensions; conflicts report `#SPILL!`. Source formulas
remain authoritative. Results shrink and recalculate after edits without turning calculated children into inputs.
Operands are limited to 100,000 cells and eight MiB of cumulative matrix text, with separate evaluation work limits.
Automatic sheet expansion, legacy CSE semantics and unsupported formula syntax remain open.

Native XLSX exports write standard dynamic-array ranges, metadata and typed caches; ODS writes standard matrix
spans and caches. Round trips through native project payloads preserve formulas and history. Independent macOS
XLS42 recalculation agrees on all 90 array/text/regression fixtures in each format, including blocked spills, implicit intersection,
conditional arrays and matrix constants. `array-fixtures` and `rust/scripts/audit-arrays-office.py` reproduce this
check. Standard external XLSX/ODS arrays can also be imported, resized and saved without the native payload; the
remaining adapter limits are listed below.

External clipboard text contains calculated, formatted values. The private off42 clipboard retains formulas,
styles and physical source coordinates for translated internal paste. Copying part of a spill stores typed result
values; copying its anchor and children retains one formula and empty calculated children. Both clipboard formats
share one calculation, including volatile functions. Native Windows array controls/history/save/reopen checks pass in run 42423267384; external array edits have
additional native scenarios awaiting the next compatible build.


The calculation engine also supports TEXTSPLIT with Unicode delimiter arrays and padding, LINEST/LOGEST
statistics, and TREND/GROWTH prediction. Regression uses centered/scaled Householder QR and bounded dense work;
large-offset and singular-data regressions are covered. FIXED/DOLLAR currently use the existing Windows formula
convention (`.` decimal, `,` grouping and `$` currency); full locale formatting remains open. ROMAN follows macOS
classic form and ARABIC accepts signed Roman text. REGEX, REGEXTEST, REGEXEXTRACT and REGEXREPLACE use a compiled
Unicode expression engine with occurrence/capture/array/replacement support and bounded output/work. Look-around
and pattern backreferences currently return `#VALUE!`; their macOS syntax compatibility remains open. The engine
is statically linked and adds no runtime DLL requirement. CELL metadata now observes calculated spill values.


### External calculated-array files (development branch)

External XLSX imports recognize pure XLDAPR dynamic-array metadata, and ODS imports recognize supported matrix
formulas. Cached children retain their styles and become disposable values. Saving refreshes standard typed caches,
range metadata and dimensions, clears former children after resizing/removal/blocking, and preserves explicit inputs.
A disposable reader copy accommodates modern Excel errors such as `#SPILL!`; original package bytes remain retained.
New XLSX arrays register their standard metadata relationships/content types when no metadata part exists. Existing
pure dynamic records keep their original index. Unknown or mixed metadata and legacy CSE arrays retain edit guards;
registration alongside unsupported existing metadata, structural shifts and repeated matrix anchors remain limited.

Independent inspection and actual macOS XLS42 recalculation agree on all 92 edited/resized external fixtures in each
format, with no private project payload and an unchanged foreign package entry. Portable regressions cover complete
Undo/Redo, blocking/removal, reopened values, literal text, metadata records, ODS repeats and strict style preservation.
Unchanged cells retain their original absence of a style when the ODS writer would otherwise inject a value default.
Import range work is bounded. These source changes are not a public release.


### Numeric presentation and data precision (development branch)

The grid and plain clipboard use the macOS General projection: up to eleven significant digits at ordinary
magnitudes and a bounded scientific representation at extremes. Actual Swift formatting produces the 105 General
and 14 fixed-pattern oracle cases in `macos-number-display-cases.json`; regenerate them with
`python3 rust/scripts/export-number-display-cases.py`. Decimal/group separators still follow the existing Windows
convention; full locale and richer custom-format support remain open.

`Workbook::calculated_cells()` supplies full-precision calculated text for data processing, retaining source
representations for literal cells. Formula evaluation, chart/pivot calculation and typed Office caches keep their
precision. Numerical reference tests use this data projection with their original expected values and tolerances.
Number formats apply only to numeric results, including calculated children; text identifiers such as `0007` retain
their type and leading zeroes. Cell display and TEXT share bounded zero-padding, grouping, decimals and percentage
patterns. Native ODS custom-number-format export remains guarded.

Filter matching, summaries, external hidden-row classification and saved XLSX row flags use the same presentation.
The XLSX writer's separate automatic filter hiding is disabled in favor of calculated visibility. Portable checks
verify display/clipboard values, full-precision XLSX/ODS numeric caches, formatted spill children, text typing,
Undo/Redo and filter/manual-hidden-row round trips. Native formatted-copy checks await the next compatible build.

### Built-in OCR implementation (native verification pending)

PDF42 now captures selected pages without changing their rotation, crop or annotations and recognizes them
on a cancellable worker. Results become one undoable invisible text layer; stale or failed jobs change no
pages. Repeating recognition replaces only validated OFF42-owned layers. Existing visible content is retained.
SCN42 shares the same offline recognizer and can export invisible Unicode text, including characters outside
the bundled visible font. Visible text font validation remains strict.

Windows builds provision pinned Tesseract/Leptonica through `native/ocr/vcpkg.json`, link native code and CRT
statically, and bundle twenty SHA-256-verified `tessdata_fast` models plus dependency licenses. This does
not use the desktop Windows.Media.Ocr API, which requires package identity. Default `auto` recognizes
English, Russian and Turkish together; explicit language tags or up to three `+`-separated model codes are
supported. Missing or altered bundled models produce an error. No runtime download occurs.

Current portable verification: 457 tests pass, zero fail, one external OCR fixture remains ignored; GNU
Windows all-targets compilation passes. Real engine pixels, packaged UI and static runtime checks are
pending a new Windows run; this implementation is not yet the public preview.

Independent OCR-layer interoperability: `pdf42-core` `pdf-ocr-fixtures` and `scn42-core` `scan-ocr-fixtures` examples plus
`scripts/audit-ocr-pdf.py` verify extraction using Poppler 26.10.0. Latin, Russian, Chinese, Thai and emoji
remain extractable; original/replaced/undone PDF layers retain identical rendered pixels for all four
rotations with nonzero crop origins. Uncompressed and compressed scanned-PDF pixels also match exactly.
Evidence: `build/ocr-interop-v1/poppler-v1/results.json` (local, not a shipped artifact). The native audit now
includes real image OCR through PDF/scan menus, selected-page scope, PDFium text/pixels, history and reopen;
those new GUI checks are pending Windows CI.

### Image Live Text implementation (native verification pending)

IMG42 analyzes the current image on a cancellable worker using the shared bundled OCR backend. In viewing
mode, recognized words can be selected by double-click or drag, copied, selected together, or cleared with
Escape. Drawing tools keep their pointer gestures; text overlays change no pixels or save history. Switching
files/frames or modifying pixels invalidates old results. Space/Page Down/Page Up navigate images after text
selection. Extra recognition settings stay in the Live Text menu; only viewing and Copy appear in the toolbar.
Immutable analysis snapshots share pixels without encoding a temporary file. Recognition still has bounded
page/word limits and only English/Russian/Turkish in `auto`; explicit supported languages use bundled models.

Portable workspace: 460 tests pass, zero fail, one external OCR fixture ignored; GNU Windows all targets
compile. New actual IMG pointer/clipboard/drawing/navigation assertions are pending Windows CI. The earlier
static OCR build has passed its runtime dependency gate and targeted Windows tests; complete run evidence
will be recorded after the workflow finishes.

**Remove Uniform Background** makes border-connected pixels transparent when their color matches a
uniform background within the chosen tolerance. It runs offline, retains the image dimensions and
uses one Undo step. It is intended for solid or nearly solid backgrounds. Subject segmentation for
complex scenes remains open.

Windows run 42429149583 on ebc82b8 succeeded: 362 targeted MSVC tests passed, zero failed, one external OCR
fixture ignored, all 30 existing native groups passed, and the seven-PE static runtime gate passed. This
includes actual recognition of image pixels and verification of all twenty bundled models. New PDF/SCN/IMG
OCR GUI workflows remain pending. Follow-up native canvas changes refresh Copy availability immediately
after image selection; image unit regressions are now included in quick CI.

### Verified OCR GUI and vector PDF label artwork

Build cd1b742 / run 42430554087 passes 399 targeted MSVC tests (zero failures, one optional external OCR
fixture ignored), all 30 existing native groups and the static runtime gate. The new PDF OCR UI scenario
also passes. Follow-up runs 42431277055 and 42431605616 execute the formerly skipped IMG42/SCN42 branches
on those exact binaries: real OCR, page scope, text/pixel output, history and reopen; image double-click,
clipboard, Select All/Escape, drawing/Undo and navigation. The latter checks actual image/selection pixels;
its inspected screenshot confirms visible highlighting. The first screenshot was captured before repaint.

PDF42 now offers the current PDF page as reusable vector label artwork, with an optional PDF-coordinate
region; selected text can supply a tightly bounded artwork region. A disposable PDFium copy flattens visible
annotations. Label assembly keeps the source fonts/images/graphics and visible rotation/crop, fits the
artwork proportionally into stock cells, and reuses one form for copies. Other source pages and unrelated
metadata are not carried into the output. Limits: 64 MiB source/content, 100,000 source objects, 5,000 copies
and 500 output pages. This source implementation and new GUI scenarios are awaiting a Windows run.

Independent reference: `label42 --example artwork-fixtures`, native `scripts/label-artwork-reference.swift`
and `scripts/audit-label-artwork.py`. All eight page/rotation comparisons match native CoreGraphics within
one pixel for the colored artwork, with exact text/copy counts and nonzero crop origins. Evidence:
`build/label-artwork-interop-v1/poppler-v1/results.json`. Portable workspace: 463 pass, zero fail, one optional
external OCR fixture ignored. GNU all-targets compilation passes.

DOC42 also offers numbered picture sources for artwork labels. Only the chosen PNG/JPEG is decoded,
with a 16 MiB encoded / 16 million pixel limit. Its displayed aspect ratio is retained; alpha is composited
on the sheet's white paper. Surrounding document text, alternate text and input values are not stored in
label preferences. Mixed selected DOC content and exact caret-to-picture mapping remain separate work.
Portable workspace: 466 pass, zero fail, one optional external OCR fixture ignored; GNU all targets compile.
Native DOC picture color/alpha/copies/source-state checks have been added and await Windows CI.

Run 42432232653 builds successfully and passes its MSVC tests, including PDFium annotation flattening.
Its GUI audit stops at a test race: it looks up the PDF canvas before the rotated/cropped render appears.
The audit now waits for that canvas; label-specific GUI assertions have not yet been counted as passing.

### Label preview and direct PDF printing

Label generation now opens an embedded preview in PDF42, DOC42 and XLS42. It supports page navigation,
zoom/panning, Home/End, save without closing, settings Cancel and regeneration. The window stays inside
the monitor work area and wraps measured translated controls. New preview strings cover all twenty
interface languages. Preview rendering retains one bounded page bitmap and the immutable output PDF; it
does not preload a raster for every sheet. Save/Print/Settings use the currently generated snapshot.

The shared host adds an actual-size PDF print job using PDFium directly on the printer DC, retaining PDF
resources through EndDoc. It exposes page ranges, uses printer-managed copies/collation, honors printer
calibration and hard margins, and defaults A4 label jobs to A4 without changing system preferences. It also
passes the standard Print to File destination to StartDoc. Actual printer/driver output is not yet verified.
The new native gate inspects an opaque PDF path printed into an EMF for vector records instead of bitmaps;
preview pixel, navigation, resize, save/settings tests and available-printer inventory await Windows CI.

The preceding 6f1854c run 42433032037 succeeds: 412 targeted MSVC tests, zero failures, one optional external
OCR fixture ignored; all 30 native groups and static runtime gate. Its PDF page/region label and DOC picture
label scenarios actually execute and pass. Portable workspace remains 466/0/1; GNU all-targets checks pass.

PDF42 also has **Create Labels from PDF Area…** in its menu. Drag a graphic/text region on the current
page; Escape cancels. The area uses unrotated PDF coordinates after crop/rotation mapping and opens the
shared artwork settings/preview after releasing the pointer. It does not add an annotation or history
step. A changed document invalidates the one-shot source. Portable geometry/source tests pass; the native
mouse/preview-output scenario awaits Windows CI. Run 42435906021 failed the new EMF probe; direct printing
and preview are not yet counted as verified.

DOC42 now offers **Selected Content Artwork** for a nonempty native selection. It freezes only that
selection as RTF and lays it out in a disposable Rich Edit control at the body width. Styled text and
pictures render together into a bounded, white-paper PDF image at up to 300 DPI; very large selections
reduce resolution to stay under sixteen million pixels. This first path rasterizes text, so full vector
fidelity remains open. It leaves the clipboard, selection, document and history untouched. Selected-text
labels use native UTF-16 ranges, including offsets after objects/tables, instead of the plain model view.
Windows native selection/pixel/offset tests are added; runtime verification is pending.

### Automatic recovery copies (Windows crash audit pending)

All six apps take local recovery snapshots every thirty seconds while modified. Protected PDFs remain
excluded until their encrypted recovery path is implemented.
This matches the thirty-second backup policy configured in the macOS document apps and also covers scan
projects. The File menu has **Recover Unsaved Document…**; a launch without a file offers inactive copies
left by a terminated process. Cancel retains them. Recovery opens an independent, unsaved document;
Save and closing still ask for a destination or confirmation. The original source is never a backup target.
Saving, discarding or normally closing clears that session's recovery copy. Other live windows retain
exclusive locks on their own copies. Interface labels cover all twenty languages.

Backups are bounded to 256 MB each, one GiB per app and 128 sessions. A single atomic record carries
metadata, payload length and SHA-256; incomplete or damaged records cannot be restored. Disk writes run
on a serial worker; failed replacement does not erase the last complete snapshot. Office spreadsheets
and presentations retain their original container. A presentation's inline draft enters a temporary
export copy without changing live Undo/Cancel or pending Save revisions. Protected/encrypted PDF
sessions are excluded from automatic plaintext backups. IMG42 recovery is not implemented yet.

Portable storage/history/Office-container regressions pass and Windows GNU targets compile. The new
native audit kills only its own isolated processes, then checks startup/menu recovery, active-window
exclusion, Cancel, unsaved state, save destination, content/pixels, cleanup and reopen. Runtime execution
of that audit is pending; this is not a published release.

Native run 42438400230 passes 418 targeted MSVC tests, including tight mixed-selection artwork. Its DOC
mixed-selection GUI scenario passes; the PDF area scenario catches a real Escape-dispatch defect. The
source now handles Escape and releases mouse capture, with the same native regression retained.
Focused run 42439054997 succeeds on those 48db96f binaries: all six audit groups pass, including the real
Microsoft Print to PDF queue, page-two range, A4 and actual-size ink bounds. Earlier Poppler inspection
independently confirms a nonblank one-page printer PDF with placement within about one point.

### Shared document printing and PDF fit (next Windows run pending)

All six apps now expose ordinary printing under **File → Print** with **Ctrl+P**. The original app print
commands still drive the same operation. DOC42 shows a compact primary toolbar; secondary formatting,
page, table and image commands remain in their menus. PPT printing reads the current inline text draft
without committing it, so Print Cancel does not consume the user's editing/Undo state.

PDF42's ordinary **Print Pages…** now uses the current immutable PDF directly. Each page is rendered
onto the native printer DC, centered on the physical sheet and only shrunk when oversized, matching the
macOS PDF print path. Crop/rotation and print flags stay with the snapshot. Page ranges live in the
standard system dialog. **Print PDF as Image…** retains the explicit DPI/range raster fallback; it stays
out of the primary toolbar. Calibration applies to physical positions and both axes after fitting.

Portable placement and draft-state tests pass; GNU targets compile. A new Windows audit checks actual
Ctrl+P in all six apps, File-menu routing, compact DOC layout, and PDF range/fit/rotation/crop/annotations
through Microsoft Print to PDF, including source/history and Undo/Redo. Its first compatible run is pending.
The clipboard adapter now retries a short competing lease; a deterministic native contention case is added.

Focused run 42440754175 actually passes all five crash-recovery scenarios on 02ad18e binaries (11 groups
including ordinary app audits). DOC/SCN recovery screenshots were inspected. The printed QR from run
42439920521 independently decodes as PCB-PRINT-42 using ZXing-C++ 2.3.0 after Poppler rendering at 300 DPI.
A later cleanup fix permits deletion of a damaged recovery payload without loading it; its core test passes
and a native damaged-copy/source-safety check is added for the next build. No public release has changed.

### Character formatting at the native caret (Windows audit pending)

DOC42's Bold, Italic, Underline, Font and Text Color commands use the actual Rich Edit selection.
They support the insertion point, table-cell text and selections spanning paragraphs or following pictures.
Changing insertion-point attributes leaves existing content and document history clean; subsequent typing
uses those attributes. Selected formatting is committed once through the guarded RTF projection adapter.
If the adapter refuses an unsupported structure, the native editor restores its previous contents and selection.
Font opens with the current selection's attributes; Cancel changes nothing. Native selection notifications keep
menus, shortcuts and toolbar overflow states in sync. Core formatting remains available to portable callers.

The Windows unit and GUI scenarios cover typing attributes, mixed selection, table/image preservation,
multiple paragraphs, Cancel, complete Undo/Redo and save/reopen. Windows runtime verification is pending.

Run 42443183563 on 0daaf47 passes 486 targeted MSVC tests (zero failures, one optional test ignored) and
all 35 native audit groups, including common Ctrl+P, ordinary PDF virtual-driver output, print flags,
crop/rotation/fit/range/source/history, damaged recovery deletion and clipboard contention. The DOC
recovery screenshot confirms one primary toolbar row. This precedes the native caret-formatting change.

### Lossless image recovery (native crash audit pending)

IMG42's journal stores all current pages/frames, the active frame, exact 8/16-bit samples, density,
retained EXIF/ICC/PNG color/TIFF text metadata, animation timing/loops and conversion/precision guards.
It uses a bounded private snapshot instead of exporting through a potentially lossy image codec.
Snapshot capture leaves pending saves, dirty state and Undo/Redo untouched. Restoration opens an
independent unsaved copy; a normal Save still asks for an image destination. The original stays untouched.
Frame counts, dimensions, byte lengths, metadata and memory use are checked before publication/restoration.

The portable workspace passes 490 tests (zero failures, one optional test ignored); GNU Windows targets
compile. The forced-termination native recovery audit now includes IMG42 and verifies exact rotated PNG
pixels, Cancel, independent save, cleanup and reopen. This new adapter awaits its Windows run.

### Goal Seek

XLS42's **Tools → Goal Seek…** accepts a formula cell, numeric target and one empty/numeric variable cell
on the current sheet. It computes on a frozen workbook in a cancelable background job; the live cells,
source file and history remain unchanged until the user accepts the result. The confirmation shows the
variable, calculated formula value and target. No discards the proposal. Accepted results use the normal
cell validation/protection path and form one Undo step; a changed workbook refuses a stale proposal.
**Cancel Goal Seek** stops the pending job. These commands stay in the menu; XLS42's primary toolbar is compact.

The bounded calculation uses the macOS secant method with a bisection fallback. It freezes calculation time,
retains cross-sheet dependencies and full precision, and refuses nonnumeric/unsupported formulas and false
convergence across a discontinuity. Limits are 1,000 evaluations and ten seconds; cancellation is checked
between evaluations. The multivariable constrained Solver is described below.
Portable regressions cover the Mac square-root/loan examples, cross-sheet references, blank-cell semantics,
bisection, cancellation, stale state, final validation, protection and complete Undo/Redo. Workspace:
496 passed, zero failed, one optional test ignored; Windows GNU targets compile. Native confirmation,
No/Yes, saved values, source preservation, protection, Undo/Redo and reopen checks pass in Windows run 42445809138.

Focused Windows run 42445175112 catches a native DOC picture-projection refusal and the IMG recovery
extension guard; those GUI scenarios are not yet verified. The private IMG extension now has an explicit
recovery-only capability, and a reproduced RTF table/picture ordering defect is fixed with a portable
regression. The new DOC diagnostic retains rejected fixture RTF only when OFF42_AUDIT_OUTPUT is set.

The RTF picture reader also handles nested BLIP identifiers and bounded binary image payloads. It keeps
following body text separate and refuses truncated/mixed encodings. Portable regressions pass; the exact
Windows typing failure remains under native verification.

Focused native run 42445809138 passes all six app groups on 8437ea8. Goal Seek's No/Yes, unchanged
source/formulas/other cells, numeric square-root value (1.414213562373095), complete one-step Undo/Redo,
save/reopen, unreachable target and protection checks actually pass. Constrained Solver verification is recorded below.

### Native document projection compatibility

Run 42445530680 verifies crash/restart/recover/save/reopen in all six apps, including IMG42's exact rotated
pixels; its recovery screenshot was inspected. Run 42445939068 records 500 passing MSVC tests (zero
failures, one optional skip) and leaves one actual DOC failure in its 36 native groups. The captured Windows
RTF adds standard table padding, inline-picture properties and a PNG rewrite with identical decoded pixels.

The projection adapter now accepts only those known view defaults/properties and restores original picture
bytes, format, metadata and description after bounded sample comparison. Changed picture samples or other
placement/padding values still refuse the edit without changing model/history. The exact captured fixture
now passes portable text/style/table/picture/Undo/Redo regressions. Paragraph alignment also uses the native
caret/range, including content changes with a collapsed caret; Font Cancel restores editor focus. New native
formatting/focus/alignment checks await the next compatible Windows build. Portable workspace: 501/0/1;
Windows GNU targets compile. This is development work, not a public release or complete parity.

### Rescan the current page (native virtual-source audit pending)

SCN42's **Edit → Rescan Page** captures the selected page identity and opens the normal WIA acquisition
flow. The first returned image replaces that page; additional feeder images append, as on macOS. Other
pages retain their identities, pixels, OCR and resolution. The replacement has a fresh identity and clears
old OCR. The entire acquired batch validates before one history commit, with complete Undo/Redo and
project persistence. A full document can reuse the replaced page's slot/memory. Cancel, errors and late
results after a project change leave the document unchanged; a second acquisition cannot start in flight.

Portable core/adapter regressions cover target selection changes, batch failure, cancellation, stale callbacks,
full capacity, exact state and saved/reopened output. The workspace passes 506 tests (zero failures, one
optional skip); Windows GNU targets compile. A native UI scenario uses an explicitly configured virtual
source through the actual STA/WIC decoder and acquisition callback, checking exact samples/DPI, source
preservation, old OCR removal, history, Cancel and a delayed result after reopening. Its Windows execution
is pending. This fixture does not exercise a physical WIA device or its settings dialog.

Run 42446821420 on 33e173d succeeds: 502 MSVC tests and all 36 native groups pass. DOC's real toolbar Font
Cancel/focus, insertion/table/range styles, paragraph alignment, original picture bytes, complete Undo/Redo
and persistence pass; the resulting document screenshot was inspected. This precedes the rescan addition.
Native formatting toggle buttons now display their authoritative checked state in the compact toolbar.

### Constrained Solver

XLS42's **Tools → Solver…** edits a sheet's objective (maximum, minimum or target value), variable cells,
constraints (`<=`, `>=`, `=`, `integer`, `binary`), non-negative assumption and linear/nonlinear method in
one dialog. Constraints have a multiline field; the primary toolbar stays compact. **Cancel Solver** cancels
its private background task. Trials freeze calculation time and never modify live cells or history. The
result confirmation applies either the model alone (No) or the model and values (Yes), as one undoable edit.
Stale results, protected sheets, invalid cells and final data-validation failures refuse the whole commit.
Linear models use two-phase simplex with integer branch-and-bound; nonlinear models use bounded penalized
Nelder–Mead and report approximate results rather than a global optimum. Limits: 32 variables, 128 constraint
entries / 256 scalar rows, 100,000 work steps and 15 seconds, checked between bounded evaluations.

Models persist in private workbooks and standard sheet-scoped hidden `solver_*` names in XLSX/ODS, including
unknown option values and matching LibreOffice engine names. Imported models prefill the same dialog. Unrelated
names/package parts stay intact; unsupported or invalid models remain opaque. Structural edits shift native
model references; deleting its objective clears the model. Portable tests cover linear/integer/binary/free
variables, numeric/reference RHS, nonlinear/target cases, infeasible versus unbounded integer relaxations,
readonly trials, cancel, stale state, final validation, history and independent Office imports. Native UI
assertions pass in run 42453016096, including actual Windows dispatch and the inspected dialog capture.


Windows run 42453016096 (558ea36) passes all 37 native groups with 520 passing MSVC tests, no failures and one
optional OCR test ignored. The constrained Solver dialog, LP No/Yes, one-step Undo/Redo, numeric optimum,
integer/nonlinear solutions, protection and standard XLSX/ODS model persistence/independent import are verified.
Its screenshot was inspected; the public preview download remains unchanged.

### Explicit spelling review (native verification pending)

DOC42/PPT42 expose **Edit → Check Spelling…** alongside continuous spell-check flags. The shared reviewer lists
actually installed Windows dictionaries in the supported interface languages and offers dictionary suggestions,
Replace, Skip, Skip All and Cancel. The original word is readonly. Each accepted correction prepares a precise change to the authoritative
text runs and validates the new editor projection before one undoable commit. Native font substitutions are
kept out of the stored model; a stale projection or adapter refusal preserves the prior state.
Native UTF-16 positions include table delimiters, images and emoji, so visible-text offsets are never substituted.
It checks a selected range or the current editor's text. Work is bounded to a million-character editor, 32,768-unit
check chunks, eight million checked units and 1,000 review steps. Windows language-pack availability applies;
interface translation does not install a spelling dictionary.
Native engine/range and actual DOC correction/format/table/image/history/reopen assertions are added but pending.


### Shared window panels

View offers checked entries for the sidebar, toolbar, status bar and available document rulers. Hidden panels
release their space to the document; normal application commands stay in their menus. Each app remembers these
choices with its window placement without changing document contents or Undo history. Old placement files keep
all panels enabled by default; unavailable panels are disabled in the menu. Fullscreen slide shows retain their
existing presentation layout. Portable preference round trips/corruption checks pass; native six-app toggle,
geometry, history and relaunch assertions pass in run 42512382933.


The native en-US review verifies body/table/emoji positions, exact formatting and image retention,
Cancel/Skip, full Undo/Redo and saved reopen in run 42515993014. Other installed dictionaries and editor
surfaces remain in scope for spelling verification.


### Header/footer fonts

PDF42's existing Header & Footer dialog adds a font choice and color. Choices are the bundled Noto Sans,
Arial/Arial Bold, Times New Roman/Times New Roman Bold and Courier New. Selected system font programs are
embedded in the saved PDF with actual widths/metrics and ToUnicode maps; their appearance is retained without
requiring that font on the recipient's computer. Missing fonts or missing glyphs refuse the operation. These
choices also apply to batch headers. TrueType embedding flags are checked, forbidden outline embedding is
refused, and a font marked no-subsetting is embedded in full. Each program is limited to 16 MB and live
page-decoration programs to 32 MB; the UI caches only its fixed choices. Complex-script shaping is unchanged.

Portable permission/subset/Unicode/history tests and native font-family/embedding tests pass. Run
42515993014 verifies all five Windows choices, distinct rendered glyphs, Cyrillic text, selected page, color,
Undo/Redo/reopen and Microsoft Print to PDF.

### Sparse worksheets

XLS42 accepts sheets up to 1,048,576 rows by 16,384 columns, including XFD1048576. Grid text, formula
spills, filters, clipboard, labels, charts, validation and print text use occupied-cell projections. Sparse
row/column sizes and visible-cell painting retain frozen panes, hidden rows and merges; 64-bit coordinates
and scaled Windows scrollbars avoid pixel overflow. Selecting a distant range brings it into view.

XLSX imports stream actual records. ODS imports skip empty repeated gaps and expand bounded meaningful
records, preserving spaces, tabs, line breaks and paragraphs. Value-only ODS edits with unchanged layout/
styles and no active filter preserve neighboring XML directly; other ODS edits retain preservation guards.

Stored cells are limited to one million across the book, with four MB of text. Operations that materialize
rectangles (copy/paste, style, fill and CSV) retain a separate 100,000-cell bound; charts allow up to
1,000 points per series. Printing supports up to 100,000 cells and 4,096 pages: select a smaller print area
for distant sparse data. Oversized operations are refused before document mutation. Dense rectangular text
projections are test helpers; data-processing clients use `display_cells` or `calculated_cells`.

Portable tests cover far references, independent XLSX/ODS edge imports, all native formats, validation,
atomic operation refusals and selected-area printing. The full-size native UI scenario passes in run 42521108743: far selection/scroll, edit, scalar summary
caches, distinct error/text types and complete history/reopen. Run 42523096085 also passes the
flushed-paint selected-cell pixel assertion; its capture shows XFD1048576 with the expected value. Native blank-grid growth is verified below; remaining formula/format semantics remain in `PARITY.md`.

Explicit reference aggregates (SUM, AVERAGE, MIN, MAX, COUNT, COUNTA, PRODUCT and aggregate math) traverse
stored values and spill results; COUNTBLANK computes implicit blanks without expanding the reference area.
Full-sheet calculations preserve number/text/boolean/error distinctions and empty-string formula semantics.
Whole-row/column reference syntax and other full-range functions are described below. The 492 fixed macOS formula cases
still match; the two locale/volatile examples remain explicitly skipped.

Native XLSX/ODS exports now preserve literal error cells separately from error-looking text and keep cell
formatting. Standard and supported modern XLSX error codes retain their error type; unknown literal XLSX
error codes are refused. Independent packages without the private snapshot verify all sixteen supported
codes, direct error propagation, ISERROR, ordinary strings and formatting. This does not expand the existing
guards for external XLSX error-input edits or all ODS layout/style changes.


Whole-column/row references such as `A:A`, `$A:$B`, `1:1` and `$2:$4` work with the supported sparse
aggregates and reference metadata functions. Qualified quoted sheet names, INDIRECT and the reference
form of INDEX are supported. Formula references use the global Excel address space, including implicit
blank cells beyond a small sheet's displayed extent; references past XFD1048576 remain errors. Array and
other materialized functions retain their existing budgets. XLSX/ODS formula and active-sheet metadata
are checked after removing the optional native snapshot. External tab-only saves update the primary view
and selected-tab flags while retaining worksheet data, styles, secondary views and unknown package parts.
Missing view metadata is created; Undo restores the original package byte-for-byte. Native Windows tab
commands and external saves await a compatible UI run. Complete locale/function semantics and automatic
viewport growth remain open.

COUNTIF/SUMIF, COUNTIFS/SUMIFS, AVERAGEIF/AVERAGEIFS and MINIFS/MAXIFS now evaluate large references
without visiting every empty coordinate. Criteria and result ranges share relative offsets; implicit blank
counts are computed algebraically, formula-empty strings count as empty, and spill values remain nonblank.
SUMIF/AVERAGEIF align their optional target from its top-left corner; IFS ranges require equal shapes.
Portable tests include full columns/full sheets, distant values, selected errors, wildcards, numeric-only
averages and complete history/Office round trips. Independent XLSX/ODS packages without the native snapshot
exercise these formulas too. Run 42525411109 verifies their saved caches through the actual Windows UI: 37 groups pass,
MSVC 553/0/1.
Other function/localization semantics and materialization limits remain as described in PARITY.md.

Blank spreadsheet viewport space now grows near scroll/keyboard edges without changing allocated workbook
bounds or history. The range dialog can address any valid Excel cell. Editing a newly visible cell, formatting
it and pasting typed/TSV clipboard data grow only the affected document bounds, with complete one-step Undo.
Protection, operation limits and validation still apply. XLSX/ODS external value edits use the existing
surgical package paths, and independent format tests verify far formulas/opaque parts and byte-exact Undo.
Saved project reopening restores the stored selection. Workspace tests pass; the new native scroll/edit/
format/paste/Undo/Redo/reopen scenario awaits Windows verification. Some range tools still require allocated
data; this is not a claim that every command or format supports unlimited areas.

Technical-object/picture insertion and movement, the picture geometry prompt and row/column resizing now
use the same atomic growth boundary for newly visible blank areas. Portable checks verify a distant QR
recipe, exact image position/offsets/data, axis sizes, selection retention, complete Undo/Redo and Office
round trips. The corresponding normal QR/geometry UI scenario awaits a compatible Windows build.

Run 42526620649 stops at the existing XLS picture-pixel check before reaching the growth scenario. Grid
refresh previously tracked only focus coordinates, allowing another document/sheet with the same focus to
retain stale scroll offsets after blank viewport growth. It now tracks document/sheet identity and resets
scroll/drag state at that boundary. Open/New/rejected-open identities have a portable regression; native
repeat is pending and the pixel assertion is unchanged. Copy/fill, serials, range rules, merge, print-area,
hiding and freezing also use bounded growth where needed; normal command/history/project checks pass.

Formula entry/evaluation now accepts the same 310 Russian function aliases as the macOS parser and
semicolon argument separators. Inline-array row separators, strings, quoted sheet names and structured
headers are preserved. Seven Russian constant-error spellings are recognized in formulas. Editor/native
project input stays unchanged; standard XLSX/ODS output uses canonical English functions and separators,
verified in independent packages without the private snapshot, including dynamic spills and external edits.
Unquoted Unicode sheet names are quoted for standard output and matched without case. OpenFormula
constant errors are consumed as tokens rather than mistaken for sheet-reference punctuation. Portable
workspace 561/0/1, GNU all targets and fixed Swift oracle 492 pass/0 gaps/2 skips; native entry/cache/history
assertions await a compatible build. This does not provide every locale's formula names or number formats.

Run 42527552826 passes the prior picture-pixel check after the document identity reset, and saved artifacts
verify blank scrolling/selection plus text, formatting and clipboard growth with complete Undo/Redo/reopen.
Its new QR assertion incorrectly looked for plain text inside a base64 descriptor; decoding the saved
descriptor and embedded PNG confirms the expected QR recipe. The audit now checks both decoded objects.
A separate PDF artwork scenario queried a visible canvas immediately after asynchronous rotation; it now
waits for that actual canvas before the unchanged gesture/geometry checks. A repeat is still required.

Basic sorting now uses typed calculated keys with stable ordering: numbers, text, logical values, errors,
then blanks; blank lines stay last in both directions. Advanced Sort adds multiple keys, a header, row/
column direction, case sensitivity, natural ASCII digit segments and custom priorities in one menu dialog.
Sorting traverses stored keys/cells and computes blank-line positions algebraically, including full-sheet
selections and distant XFD data. It preserves cell contents/styles and translates relative formula references.
Key slots are bounded to one million; merged/array/spill blocks and failed validation refuse atomically.

For a supported list-validation rule, select the cell and click its arrow or press **Alt+Down**.
The **Cells → Choose Allowed Value** command opens the same list. Choices preserve text such as
`007` and formula-like identifiers literally. Applying a value is one Undo step. Supported rules
are retained in native projects and off42-generated XLSX files. XLS42 also reads visible inline
lists with stop-style errors from ordinary XLSX files and preserves their original validation XML.
Foreign Office source packages retain their existing sort-preservation guard. Unicode lexical text ordering
does not establish every locale's collation; custom-list preferences remain open. Tests verify full-sheet/
multikey/horizontal sorting, types, exact history and native Office round trips; numeric-format XLSX retains
its format while the existing unsupported ODS numeric-format export remains explicitly refused. Workspace
566/0/1 and GNU all targets pass; normal native advanced/full-size sort scenarios await Windows verification.

Run 42528597238 (a7e4302) passes all 37 Windows UI groups, MSVC 566/0/1: new grid text/Bold/Paste,
QR insertion/geometry, serial identifiers, localized formula entry/cache/history and PDF area gestures pass.
The inspected expanded-grid capture shows AE501 = 30 with correct scroll and selection. The next audit
strengthens same-file reopen cases by first making a distinct unsaved change and explicitly discarding it.

Native ODS output now supports fixed-decimal numbers, leading zeroes, grouping and percentages using
standard number/percentage styles. Decimal precision and grouping are explicit; numeric attributes retain
unrounded values, and text caches contain formatted presentation. Independent ODS import resolves these
styles, namespace aliases and parent number styles. Edited literal/formula/array numeric caches keep their
presentation and percentage type without changing their exact values. Numeric-looking strings, Boolean
values and errors remain distinct. Unsupported date/currency/scientific/conditional/optional-digit formats
keep their existing refusal; broader locale/default-row/column style semantics remain open. Bounded XML
updates preserve unrelated parts, and independent external Undo restores the byte-exact source. Portable
workspace 568/0/1 and GNU all targets pass. A normal Number Format/ODS/native-and-foreign reopen/cache/
Undo/Redo audit is added; actual Windows verification awaits a compatible build.

Run 42529958919 (c512753) passes all 37 UI groups and MSVC 571/0/1. Advanced multi-key/custom/natural
sort, horizontal formula shifts with independent XLSX caches, full-sheet XFD-data sort and complete history
pass, including strengthened same-file discard/reopen and XLSX/ODS outputs.


### Document Navigator (development branch)

**View → Navigator** opens a sidebar for headings, tables with text, pictures, bookmarks, comments,
notes, links, sections and fields. Heading levels appear as indentation. Selecting a target jumps
to it in the editor. The sidebar updates after edits and Undo. Navigation checks the native
projection and preserves source bytes and edit history. Each category shows up to 1,000 entries.
Chapter reordering and targets without a visible native anchor remain open.

### Spreadsheet list validation (development branch)

The **List validation** dialog accepts semicolon-separated literal choices or a one-dimensional
source range on the same sheet. Range choices update from cell values and formula results. The
active-cell menu copies the selected value and its type. Use the cell arrow or Alt+Down to open it.
XLSX writes absolute source references and reads compatible visible, stop-style rules from independent
workbooks. Relative, named and cross-sheet references and ODS validation are still open.

### Presentation controls (development branch)

PPT42 can present full-screen or in the editor window with its editing controls hidden. Home and
End move to the first or last shown slide. Type a slide number and press Enter to jump. B and W
blank the display and hold slide timing. Navigation reveals the slide before advancing. Q or Esc
ends the show. Hide Slide is undoable and saved in native projects, PPTX and ODP. Slide Show Settings
stores whether to loop and include hidden slides. **Rehearse with Presenter View** shows the
current slide and next preview alongside scrollable notes, an elapsed timer and presentation
controls. The editor window returns when the show ends. During a show, L activates the laser
pointer, P activates the pen, E erases the current slide's ink and C toggles the cursor. The
first Esc leaves the active tool. Pause or Shift+P pauses and resumes automatic timing. Ink
belongs to the current show and is never written to the presentation. Dual-display presenter
view and slide overview remain open. The windowed/full-screen audit passes in run 42573274769.

**Edit Speaker Notes** opens a multiline editor for the selected slide. An accepted change is one
Undo step. Notes stay with the slide when it moves or is duplicated and are saved in native
projects, PPTX and ODP. Standard plain-text notes in imported presentations can be edited.
Compatible PPTX packages reuse their notes master or acquire one with the first note. Original
complex notes parts are preserved. Notes-page printing remains open.

**File → Export Outline** writes ordered slide headings and text bullets in RTF or Markdown.
Markdown also includes speaker notes as quotations. Hidden slides stay in deck order. The export
uses the native title and body or recognizes title placeholders in imported PPTX and ODP files.

**File → Export Slide Images** writes PNG or JPEG files at 1280, 1920 or 3840 pixels wide.
The chosen name becomes a prefix. Each output includes its original slide number, and hidden
slides are optional. The exporter stages the complete batch and refuses existing destinations.

**File → Export as PDF** writes slide-size pages, A4 speaker-note pages with slide thumbnails and
continuation pages, or A4 handouts with 1, 2, 3, 4, 6 or 9 slides per page. Hidden slides are optional
and omitted by default. Slides use the same Windows raster rendering as image export at up to
144 DPI. Speaker notes add a searchable Unicode text layer. The export preserves the presentation
and refuses existing destinations. **Print Layout** selects slides, notes or handouts for the
native Print dialog. The A4 notes and handouts fit to the chosen printer's paper.


### Scan PDF exports (development branch)

SCN42 Export Quality Settings offers lossless PDF output and 300, 200 or 150 DPI presets. Color and
grayscale images use JPEG for those presets while exact black/white scans stay lossless. The PDF
keeps the source physical page dimensions. Searchable Text controls whether recognized words are
written to the export. Export One PDF per Page uses the chosen filename as a prefix and appends
`-001.pdf`, `-002.pdf`, and so on. Existing destination files cause the operation to stop.

**File → Export Pages as Images** writes all pages as separate PNG or JPEG images. It uses the
chosen path for one page and appends page numbers to the prefix for multiple pages. The batch
keeps each page's pixel dimensions and physical resolution and stages all files before publication.
**Export Name Template** supplies editable filenames in the save dialog. Tokens include
`YYYY-MM-DD`, `HH-mm-ss`, `{n}` or `###` for the counter, and `{title}` for the first recognized
line. The title is used only when requested in the template. The local counter advances after a
successful export, once per batch. Cancelling leaves it unchanged.

**View → Scan History** lists up to 200 recent PDF, TIFF, PNG and JPEG exports with their dates,
page counts and sizes. Each file can be opened in a new SCN42 window or removed from history.
**View → Clear Scan History** clears the records and preserves the exported files. The history
uses local file metadata and paths. Thumbnail previews are still open.


### Signature verification and read-only viewing (development branch)

PDF42 can open unencrypted PDFs that contain signature fields and retain their original bytes.
Verify Signatures in This PDF checks the displayed revision. Verify PDF Signatures can also
inspect another file through the File menu. The report separates detached CMS integrity,
coverage of the entire file or an earlier revision, and Windows cached certificate-chain trust
at the current system time. Save Signed PDF Copy writes the retained bytes to a new path.


Sign PDF Copy with Certificate prepares the current unsigned, unencrypted document and opens the
Windows personal-store certificate picker. The detached approval signature can appear at the lower
right of the current page. It shows the certificate display name, system time in UTC and an optional
reason. Full Unicode metadata stays in the signature dictionary. The app verifies the CMS signature
against the prepared bytes before writing the new file. Additional signatures, custom appearance
placement, trusted timestamps and CryptoPro remain work items.
