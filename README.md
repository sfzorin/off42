# off42 — PDF42 · DOC42 · XLS42 · IMG42 · PPT42 · SCN42

off42 brings together six native apps for **macOS and Windows**, covering documents, spreadsheets, presentations,
PDFs, images and scanning. The macOS apps are written in Swift and share Kit42 for menus, toolbars, language
selection, signatures and stamps. The Windows apps use a separate Rust implementation and native Windows controls.
Available features, file-format support and runtime dependencies depend on the platform.

Core editing works offline. The macOS implementation uses Apple's system frameworks; Windows dependencies
and build requirements are documented in [rust/README.md](rust/README.md).
The macOS interface is available in **20 languages**: English, Simplified Chinese, Traditional Chinese (Taiwan), Spanish,
Arabic, French, Portuguese, Indonesian, Russian, German, Japanese, Vietnamese, Turkish, Filipino, Korean, Thai,
Italian, Polish, Malay and Hungarian. Network features in the macOS apps, including remote signing, trusted timestamps,
validation-data retrieval and opening web links,
need a connection. Optional GOST signing uses a separately installed CryptoPro CSP.

The format table and detailed app descriptions below cover the **macOS implementation**.
See [Windows scope and limits](rust/README.md#implemented-scope-and-limits) for the Rust apps.

| App | For | Opens | Saves |
|---|---|---|---|
| **PDF42** | reading, editing, signing PDFs | PDF, images | PDF (also signed, compressed, password-protected), DOCX, RTF, HTML, TXT, PNG/JPEG |
| **DOC42** | rich documents and page layout | DOCX, DOC, RTF, ODT, Markdown, TXT, HTML | the same + PDF |
| **XLS42** | spreadsheets, formulas and charts | XLSX, XLS (97–2003), ODS, CSV/TSV | XLSX, ODS, CSV, PDF |
| **IMG42** | viewing and quick editing of images | JPEG, PNG, HEIC, AVIF, WebP, GIF, TIFF, PSD, RAW, SVG, PDF, … | writable original formats; PNG, JPEG, HEIC, TIFF, AVIF where supported |
| **PPT42** | presentations and slide shows | PPTX, ODP (PPT: a message suggesting conversion) | PPTX/ODP, POTX/OTP templates, PDF (slides, notes pages), PNG/JPEG, outline RTF/Markdown, MP4/MOV |
| **SCN42** | scanning, page cleanup and OCR | scanners (USB, network, AirScan), iPhone/iPad camera, images, PDF | searchable PDF, PDF/A-style, multi-page TIFF, JPEG, PNG, HEIC, TXT/RTF |

## What's new on macOS

- **Twenty languages across all six apps.** Bundled translations, native language names in the welcome-screen
  picker, shared language preference and localized Quick Look previews. Interface language is independent of
  document content, OCR language support and formula syntax.
- **Your signature, from your samples.** Draw in the signature pad or import a scan or photo, then create new copies
  with ballpoint, gel, fountain or felt-tip ink. Name-based signatures and 132 editable stamp templates remain in the
  shared library; each insertion creates a fresh impression.
- **PDF signature workflows.** PAdES baseline levels, long-term validation data and document timestamps, CSC remote
  signing, and GOST/МЧД integration through a separately installed CryptoPro CSP. Provider and interoperability
  limits are recorded in the PDF42 section and the regression coverage below.
- **Deeper editing and file checks.** Recent work covers PDF markup, redaction, forms and page operations;
  XLS42 protected cells, arrays, dates and file round trips; DOC42 rulers and complete-document pagination;
  PPT42 text, tables, object geometry, slide shows and export; and IMG42 selection, colour adjustments, metadata
  and safe image output; SCN42 acquisition, processing, OCR, page marks and export. Regression scenarios exercise native controls,
  Undo/Redo and saved output. The regression checks below describe their scope and remaining gaps.

## Current status

This README describes the source as of **2026-11-08**. [Regression checks](#regression-checks) lists reproducible
commands and coverage limits. Publishing source on GitHub does not update an installed app or imply a notarized
binary release. Match verification results to the recorded revision and executable identity; detailed logs and
local installation history are retained separately.

Final macOS audit on **2026-11-06**: debug/release builds, app selftests and independent file checks passed.
The selected final results comprise **62 behavioral runs**, **40 screenshot captures** and **144 language launches**
across six apps; screenshots and language launches are not counted as functional editing tests. After the last
PDF audit-readiness change, only its page scenario was repeated in debug/release (**73/73 assertions each**).
Later checks should target changed behavior; repeat the full language matrix when localization changes warrant it.

The audit fixed scanner lifecycle/output/Undo issues and stamp-page state, bounded signature rendering work,
and removed false-positive format/sample checks and duplicate runs. One initial IMG42 debug file scenario failed;
it did not reproduce in three diagnostic sequences or subsequent full app checks. Its cause remains unconfirmed;
the stronger fixture baseline and failure diagnostics remain. Hardware and external-provider limits are listed below.
The local macOS packages are ad-hoc signed; this source publication does not install apps or provide a notarized binary release.

## Windows Rust workspace

The separate Rust workspace implements the six apps for Windows. Its formats, runtime dependencies, build commands
and current limitations are described in [rust/README.md](rust/README.md). Native interaction and saved-output checks
run on Windows CI; their scope and failures are recorded in [the parity tracker](rust/PARITY.md). Printer drivers,
scanner hardware and full macOS feature parity remain unverified. The development branch adds a shared language
picker and bundled catalogs; menu/prompt checks pass on twenty languages in Windows CI. Successful-file recents
are also implemented. PDF page-list extraction, per-page ZIP exports, editable page labels and engineering spreadsheet functions are also
implemented on the development branch. Goal Seek has passed native value/history/reopen checks; a constrained
Solver with persisted XLSX/ODS models is implemented and awaiting its native audit. These changes
have not yet shipped in the downloadable Windows preview.

<a id="install"></a>

## Downloads

The off42 1.0 preview DMG contains all six apps for Apple Silicon Macs running macOS 14 or later.
The apps use ad hoc signatures and the preview has no Apple notarization.
[Download the macOS preview DMG](https://github.com/sfzorin/off42/releases/download/v1.0.0-preview.1/off42-1.0.0-preview.1-arm64.dmg).

The Windows preview ZIP contains all six Windows x64 apps, PDFium and tools for
[labels and barcodes](rust/README.md#labels-barcodes-and-signatures). Extract the full ZIP and launch the app you want.
The executables are unsigned. See the [release notes](https://github.com/sfzorin/off42/releases/tag/v1.0.0-windows-preview.3)
for runtime requirements.
[Download the Windows preview ZIP](https://github.com/sfzorin/off42/releases/download/v1.0.0-windows-preview.3/off42-1.0.0-windows-preview.3-x64.zip).

## Build and install on macOS

```bash
git clone https://github.com/sfzorin/off42.git && cd off42
./build.sh                  # all apps → build/*.app
./build.sh PDF42 install    # one app, plus a copy in /Applications
SUITE42_BUILD_DIR=build/languages ./build.sh # isolated output directory for verification
```

The package declares macOS 14+ and Swift tools 5.9. The verification environment is macOS 27.0.1 (arm64) with Swift 6.3.1;
the minimum-supported macOS is not covered by this run.
Install the Xcode Command Line Tools to build. The default build uses ad-hoc signing and does not notarize the apps.
It writes `build/*.app`; only the explicit `install` argument replaces a copy in `/Applications`.
To make an app the default one for a file type:
Finder › Get Info › Open with › Change All.

Quick Look (space bar in Finder) and Finder thumbnails come with the apps, for formats macOS shows as plain text or
not at all: DOC42 brings Markdown (`.md` rendered like a document: headings, lists, task lists, tables, code,
quotes, images next to the file), XLS42 brings OpenDocument spreadsheets (`.ods`: every sheet, merged cells,
bold/italic, fills, column widths) and TSV as a table. Previews follow light/dark mode. The extensions live inside
the app (`Contents/PlugIns`), so installing the app is enough; if Finder keeps the old look, run
`qlmanage -r` (and `qlmanage -r cache` for thumbnails). When another app also previews a type (LibreOffice for
`.ods`, for instance) macOS picks one of them; System Settings › General › Login Items & Extensions › Quick Look
lets you switch extensions off.

## PDF42 — PDF

- Reading: thumbnails and contents, zoom, display modes, search, go to page, forms, password-protected files.
- Pages: add blank pages, delete, rotate, reorder, insert from other PDFs, extract, export as PNG.
  Move Up/Down and rotation apply to the selected thumbnail pages. Reordering, deletion and insertion restore page order,
  current page and selection with Undo/Redo; internal destinations and attachment lookup follow page identities.
- Markup (⇧⌘A): highlight, underline, strikethrough, pencil, shapes, whiteout, text boxes, notes;
  move and resize, change colour and border width, Undo/Redo. The controls show the selected annotation's effective style;
  border width applies to rectangles, ellipses, ink and widgets. Text boxes, image stamps, text markup and page-content
  overlays have no border-width command; image stamps have no recolour action. Escape or choosing another tool cancels
  an unfinished drag/resize/drawing.
  Deleting and undoing an annotation preserves its order among overlapping annotations.
- Text editing: the Edit Text tool (Edit menu, markup bar, or ⌘E with nothing selected) — click a line of page text
  (⌥-click: one word), edit it in place, Return applies, Esc cancels. The original glyphs are covered with white and the
  new text is drawn on the same baseline in the original font (when installed; otherwise Helvetica/Times) and size.
  Double-click to re-edit, undo works; on save the edit is flattened into the page content. A multi-line selection + ⌘E
  places an editable text box instead. Moving edited text retains its original source area for removal on Save, as well as
  covering its new position. Pending in-place text participates in dirty state and is committed before Save/export;
  an unchanged edit creates no Undo entry. Failed Revert keeps the document and draft; successful Revert cancels pending
  edits and interactions, restores saved metadata and clears stale Undo. On save the replaced text is removed from the
  emitted file (see Redaction).
- Redaction: the Redact tool (Markup menu / markup bar) marks boxes (black, or any colour picked); they can be moved,
  resized, deleted. Markup › Apply Redactions… (after a warning) or saving removes everything under the boxes, under
  edited text and under Whiteout boxes from the file: glyphs are cut out of the text operators (Tj/TJ/'/", all text state
  operators, Form XObjects, Type0/CID 2-byte and embedded-CMap fonts, simple TrueType/Type1 fonts with /Widths or the
  standard 14 metrics, Type3) and replaced by equal TJ offsets, so the rest of the line does not move; image pixels
  under a box are overwritten and the image re-encoded (raw samples in their colour space, JPEG/JPEG 2000 re-encoded);
  explicit redaction, whiteout or crop-removal boxes drop an annotation or form field even on partial overlap, so its
  concealed contents cannot survive; /ActualText of marked content and
  the page thumbnail go too; the file is rewritten without the old objects. Vector graphics stay (under the box).
  Pages that cannot be rewritten safely (vertical text, unknown CMaps, inline images under a box…) become a 300 dpi
  picture with an OCR text layer, and the app says so. CLI: `--redact in out x,y,w,h[@page] … [white]` prints what
  was removed (so does `--edit-text`). Saving/exporting fails if redaction or rendering cannot complete; it does not emit an
  unredacted fallback. Apply Redactions changes the document that Save writes, with Undo/Redo and retained document parts.
  Saving itself builds a separate snapshot and preserves the live annotation stack and editor state.
- Forms: fill existing AcroForm fields with the Select tool (values are saved). Form menu / markup bar: drag to add a
  text field, checkbox or dropdown list with a name (and options); with a form tool, fields can be moved, resized and
  deleted. Form tools pick widgets without moving unrelated markup; empty values clear text fields. Saved as real widgets
  with an /AcroForm entry and checkbox on/off appearances, fillable in Acrobat and Preview.
- Merge Files… (⌥⌘M): PDFs and images in a chosen order → a new document. Split… (File): every N pages,
  by ranges ("1-3, 4-10") or one file per page, into a chosen folder.
- Page › Watermark…: text (font size, colour, opacity, rotation — diagonal by default) or an image, on all pages or a range.
  Page › Page Numbers…: position, format ("1", "Page 1 of N", "- 1 -"), start number, skip first page, font size, range.
  Their saved representation is page content. In the open editor they remain removable and undoable; reopening the saved
  file shows the flattened content. Invalid/nonfinite measurements and overflowing number sequences are rejected.
- File › Compare With…: word-level text differences per page in a list (click to jump); removed/replaced words are
  highlighted in the document. The highlights are not saved unless "Keep highlights when saving" is ticked.
- Command line (headless): `--watermark in out TEXT [pages]`, `--number-pages in out [format] [position] [skip-first]`,
  `--split in dir 1-2,3|every:N|each`, `--merge out a.pdf b.png …`, `--compare a b [marked.pdf]`,
  `--edit-text in out OLD NEW`, `--redact in out x,y,w,h[@page] …`, `--add-field in out`, `--fill in out name=value …`, `--fields file`.
  `--add-field` adds three demonstration fields; use the UI for custom field placement.
  Other CLI commands: `--ocr in out`, `--images-to-pdf out images…`, `--export in out.docx|out.txt`,
  `--verify file`, `--sign in identity.p12 password out [visible] [tsa | tsa=URL] [level=pkcs7|B-B|B-T|B-LT|B-LTA]`
  (default: B-T with a TSA, otherwise B-B), `--ltv in out` (add validation data to the existing signatures),
  `--doc-timestamp in out [TSA-URL]`, `--csc-credentials SERVICE-URL [auth]`,
  `--csc-sign in out SERVICE-URL CREDENTIAL-ID [auth] [pin=…] [otp=…] [tsa | tsa=URL] [level=…]`, where auth is
  `token=…`, `user=… password=…` (CSC auth/login) or `client-id=… [client-secret=…] [port=8765] [no-browser]` (OAuth 2.0).
  `--verify` prints signature statuses, the PAdES level and the LTV status; its exit code is not a signature-validity result.
  Invalid ranges, dimensions and unsupported export formats fail explicitly. Missing/unreadable merge inputs do not
  produce a partial merged document. Output files are replaced atomically after successful generation; a set of split
  or batch outputs is not one atomic transaction.
- Signatures and stamps (⇧⌘K): a library of PNGs/scans, the white background is removed automatically; or create them
  (see Common › Signature and stamp generator).
- Digital signature (⌥⌘D): PAdES baseline signatures (ETSI EN 319 142-1, ETSI.CAdES.detached, SHA-256) chosen
  in the Sign dialog's Format menu — **B-B** (signing-certificate-v2, no signing-time), **B-T** (plus an RFC 3161
  signature timestamp), **B-LT** (plus a DSS with the certificate chains, OCSP responses and CRLs fetched at signing
  for the signer and the timestamp authority), **B-LTA** (default; plus a document timestamp over the DSS) — or the
  older basic adbe.pkcs7.detached signature. Certificate from the macOS Keychain, .p12 import, a self-signed one, or a
  remote signing service. Counter-signing is supported (incremental update). Visible signature (select a placed
  signature image first, or bottom right of the page); default TSA: DigiCert. Without a timestamp the level is B-B.
  Verification (⌥⌘V) and a status banner on open; the verification card shows the PAdES level and whether long-term
  validation is enabled (revocation data for every non-root certificate of the signer and timestamp paths).
  Timestamp addresses must be valid HTTP/HTTPS URLs. Visible-signature images are rendered at 3× resolution with the
  same 16,384-pixel side and 64,000,000-pixel total limits as image export.
- Signature › Enable Long-Term Validation: adds or extends the DSS (certificates, OCSP, CRLs; /VRI per signature) of an
  already signed PDF without invalidating it, so a B-T signature becomes B-LT. Signature › Add Document Timestamp: an
  RFC 3161 document timestamp (ETSI.RFC3161) over the whole file — B-LTA for a signed file (its DSS is extended
  afterwards), or a timestamp-only seal for an unsigned one. Validation data comes from the URLs in the certificates
  (AIA OCSP/caIssuers, CRL distribution points), so these steps need a connection.
- Remote signing (Sign dialog › Remote (CSC)…): Cloud Signature Consortium API v2 with a configurable service URL.
  Sign-in by OAuth 2.0 in the browser (authorization code with PKCE and a loopback redirect
  `http://127.0.0.1:PORT/callback` that must be registered with the provider), client credentials, HTTP Basic or a
  pasted access token; credentials/list → credentials/authorize (PIN / one-time code, or credential-scoped OAuth for
  `oauth2code` credentials) → signatures/signHash. Only the SHA-256 hash of the signed attributes is sent; the document
  stays local. RSA (PKCS#1 v1.5) and ECDSA with SHA-256.
- GOST signatures (ГОСТ Р 34.10-2012) through the user's own **CryptoPro CSP 5.0 for macOS**, which PDF42 does not
  include. Install CSP from cryptopro.ru (registration, administrator rights; 90-day trial licence, then a purchased
  serial number), the certificate with its key container (token or folder) and the CA root. PDF42 calls CSP's
  `cryptcp`/`certmgr`/`csptest` from `/opt/cprocsp/bin` (`SUITE42_CPROCSP_BIN` overrides the path) and performs no
  GOST cryptography itself; without CSP, GOST signatures are shown as "not checked", with the signer's details
  taken from the signature.
  - Sign Document lists CryptoPro certificates linked to a key (“GOST: …”, with SNILS/INN) next to Keychain ones,
    with an optional container PIN. The PDF gets a detached CAdES CMS from CSP in `/Contents`
    (`/Filter /CryptoPro#20PDF`, `/SubFilter /adbe.pkcs7.detached`), and CAdES-T when a TSA is set.
    Counter-signing keeps earlier signatures valid. GOST signatures keep this format; the PAdES Format menu applies
    to Keychain and CSC signers.
  - Signature › CryptoPro CSP… shows the CSP version, licence and key containers, and installs a certificate from a
    container into the personal store. When CSP is missing, it shows installation steps and links to the test CA.
  - Signature › Sign Any File with GOST (.sig)… writes a detached CAdES-BES signature `file.sig` (DER), as commonly
    used in Russia. Verify Detached Signature (.sig)… checks it through CSP, optionally with a power of attorney.
  - Power of attorney (МЧД): choose the XML in Sign Document; it is embedded in the PDF before signing, together with
    its own signature (`poa.xml.sig`/`.p7s` next to it), so the new signature covers both. Verification reads the
    unified format (`EMCHD_1`) and format 002 (UTF-8 or windows-1251). It checks validity on the signing date, that the signer is
    the representative (SNILS, otherwise personal INN), the principal's INN, and the МЧД's own signature through CSP.
    Revocation in the FNS registry is not checked; the number to look up is shown.
  - CLI: `--gost-status`, `--gost-certs [all]`,
    `--gost-sign in.pdf out.pdf --thumbprint SHA1 [--pin PIN] [--poa poa.xml] [--tsa URL] [--reason TEXT] [--visible]`,
    `--gost-sign-file file [--out file.sig] --thumbprint SHA1 [--pin PIN] [--tsa URL]`,
    `--gost-verify file.pdf`, `--gost-verify file --sig file.sig [--poa poa.xml]`,
    `--poa-check poa.xml [--sig document.sig] [--date YYYY-MM-DD]`. Unlike `--verify`, these return the verdict as the exit code:
    0 valid, 1 invalid, 2 intact but certificate not trusted, 3 not checked (no CSP), 4 power-of-attorney mismatch.
- Thumbnails: drag to reorder pages, multi-select, drop PDFs/images to insert.
- OCR (⌥⌘T): Vision (ru/en/tr) adds an invisible searchable text layer to scans. It processes a captured document state,
  preserves the page's annotation appearance and avoids applying a stale result after intervening document changes.
  Print selected pages.
- Reduce File Size (File menu): pictures are downsampled (100 / 150 / 220 dpi) and re-encoded as JPEG, text and vector
  graphics stay untouched; the resulting size of every level is shown before choosing (a 17 MB scan → 2–8 MB). Save a
  compressed copy or compress the open document with Undo/Redo. Compression preserves metadata and attachments;
  when it cannot reduce the size, the CLI still writes the unchanged copy to the requested destination. CLI: `--compress in out [smallest|balanced|high]`.
- Export: Word (.docx), RTF, HTML, text, PNG/JPEG pages (72–300 dpi), copy with a password. These outputs use the committed,
  redacted snapshot used by Save. Image exports account for page rotation. Each rendered image is limited to 16,384 pixels
  per side and 64,000,000 pixels total; the redaction raster fallback has a 32,768-pixel side limit and the same total cap.
- Import: open PNG/JPEG/HEIC/TIFF as PDF, "New PDF from Images…", insert images as pages.
- Bookmarks: the sidebar's Contents tab is editable — ⌘D (Insert › Add Bookmark) adds one for the current view (named
  after the selected text, else "Page N"), double-click or Return renames, Delete removes, drag to reorder and nest,
  indent/outdent buttons, Set Destination to Current View, bold/italic/colour (Insert › Bookmarks › Style); all undoable.
  "Create Bookmarks from Headings…" turns lines set larger than the body text into a 1–3 level tree (optionally with OCR
  for pages without text). PDFKit on this macOS neither writes edited outlines nor correct page references, so the outline
  (with /C and /F styles) is written by PDF42 itself after PDFKit (`DocumentWriter`), readable by Preview, Acrobat, pypdf.
- Page › Crop… (⌥⌘K) / the Crop tool (drag a rectangle): margins in mm on the current, all, odd, even pages or a range,
  "remove white margins automatically", Remove Crop; sets the CropBox, and optionally deletes the content outside on save
  with the redaction engine. Page › Page Size…: A3/A4/A5/Letter/Legal/Tabloid/custom, scale the content to fit or keep it
  and extend/trim the canvas. Custom sizes remain custom when the dialog opens; invalid margins or dimensions leave the
  existing geometry intact. Scaling transforms standard annotation bounds, fonts, ink paths and markup quadrilaterals.
  Pending page-content overlays (edited text, watermark, numbering, header/footer, redaction/whiteout) are baked into the
  resized page; Undo restores the original page and its editable annotations. Paper dimensions are limited to 14,400 points
  per side.
- Links: the Link tool (Insert, markup bar) — drag a rectangle → web address (e-mail becomes mailto:), page (keep zoom,
  whole page, page width) or bookmark, optional visible border; click a link with the tool to edit or delete it.
  Insert › Create Links from URLs finds web and e-mail addresses in the text; View › Show Link Borders. Saved as /Link
  annotations with /URI or /GoTo (PDFKit's broken or wrong page references are repaired on save).
- File › Properties… (⌥⌘I): title, author, subject, keywords (written to the Info dictionary and XMP: dc:title,
  dc:creator, dc:description, pdf:Keywords…), creator, producer, dates, file and page size, PDF version; Initial View
  (page layout, panel, open at page and magnification, fit/centre window, show title, hide toolbar/menu bar); fonts with
  embedded/subset state and pages; encryption, permissions, signatures. PDF42 applies supported page layouts, sidebar
  panels, opening page and whole-page/width/height/percentage magnification when opening. Full-screen and the window/title/
  toolbar/menu-bar preferences are saved for other PDF viewers; they are not applied automatically by PDF42.
- Attachments: sidebar tab (⌥⌘3) listing embedded files (/EmbeddedFiles) and file attachment annotations; add with
  Insert › Attach File… or by dropping files, save, open, drag out to Finder, delete (undoable).
- File › Export as PDF/A-2b… (also in Export As…): fonts embedded (pages with non-embedded fonts are redrawn by Quartz,
  which embeds them; failing that annotations are flattened, last resort an image with OCR text), sRGB output intent,
  XMP with pdfaid part 2 / conformance B and matching Info, document ID; JavaScript, forbidden actions and annotation
  types, attachments, image interpolation, hidden/unprintable annotation flags are removed. Not checked with veraPDF here
  (not installed): DeviceCMYK content, optional content and font program details are not converted.
- Page › Header & Footer…: six positions, codes <<page>> <<pages>> <<label>> <<date>> <<file>> <<bates>>, Bates
  prefix/suffix/digits/start, font, size, colour, margins, page set; optionally the same on other files with continuing
  Bates numbers. Written as real page text in the saved file; the open editor retains removable/undoable overlays
  (Page › Remove Headers and Footers).
- Page › Page Labels…: ranges of logical numbers (1, i, I, a, A, none) with prefix and start (e.g. i–iv, then 1…, "A-1");
  shown in thumbnails, the title bar and bookmarks, accepted by Go to Page; saved as /PageLabels. Overflowing labels are
  rejected or omitted; letter labels requiring more than 4,096 repeated letters display their decimal value.
- Document-level parts that PDFKit drops when pages are replaced (labels, initial view, attachments, outline) are put
  back on every save.
- Command line for these: `--bookmarks in out [clear] [headings[:levels]] "Title@page[:b][:i][:#RRGGBB]" ">Child@page"`,
  `--bookmarks file`, `--crop in out l,t,r,b|auto[:l,t,r,b] [pages] [remove]` (points), `--page-size in out A4|210x297 [scale|extend] [pages]`,
  `--link in out x,y,w,h[@page] https://…|N[:Fit|FitH] …`, `--autolink in out`, `--props in out title=… author=… subject=…
  keywords=… layout=… mode=… openpage=N zoom=… fitwindow=1 …`, `--info file`, `--attach in out file… [desc=…]`,
  `--detach in out name|all`, `--attachments file [dir]`, `--pdfa in out`, `--bates in out PREFIX start [digits] [position]`,
  `--bates-batch outdir PREFIX start a.pdf b.pdf…`, `--header-footer in out "l|c|r" "l|c|r" [pages]`,
  `--labels in out "1-4:r;5-:D;9-:D:A-"`, `--fonts file`.
  `--attachments` lists embedded files and attachment annotations; with a directory it extracts the EmbeddedFiles
  entries, sanitizing filenames and numbering collisions without overwriting existing files. `--detach` removes
  attachments from the output PDF.
- UI checks: `PDF42 file.pdf` with `K42_SNAPSHOT=out.png` (+ `K42_SNAPSHOT_SHEET=1`), `K42_APPEARANCE=dark|light`,
  `K42_MARKUP`, `K42_TOOL=n`, `K42_REDACT="x,y,w,h;…"`, `K42_COMPRESS`, `K42_STAMPS`, `K42_COMPARE=other.pdf`,
  `K42_ACTION=watermarkAction|pageNumbersAction|splitAction|signDocumentAction|applyRedactionsAction|mergeList|textField|…`
  (opens that dialog for the snapshot; also propertiesAction, pageSizeAction, headerFooterAction, pageLabelsAction,
  bookmarksFromHeadingsAction, linkDialog, cropDialog), `K42_SIDEBAR=outline|attachments`, `K42_LINK_BORDERS=1`,
  `K42_DO=action,…` (non-modal actions before the snapshot), `K42_PROPERTIES_TAB=n`. The native audit also exercises markup,
  pages/dialogs, core controls and document lifecycle; a panel screenshot alone does not verify an editing tool.

## XLS42 — spreadsheets (.xlsx, .xls, .ods, .csv)

The capabilities below describe the current source. The regression scenarios below identify their verification scope.

- Opens .xlsx, legacy .xls (Excel 97–2003, formulas decompiled), .ods (LibreOffice/OpenOffice) and CSV/TSV
  (UTF-8/UTF-16/Windows-1251, delimiter detection); saves .xlsx, .ods and CSV. The format is detected by content.
  When Open/Revert supplies an explicit workbook type, mismatched or invalid workbook data is rejected instead
  of being reinterpreted as CSV.
- Displayed like Excel: column widths and row heights, fonts, fills, borders, merged cells, wrapped text,
  overflow into neighbouring cells, `###` for numbers that don't fit, theme colours.
- Number formats: thousands separators, decimals, currency, %, dates and times, `[Red]`, sections `;`, scientific
  notation and elapsed hours. A workbook's 1900/1904 date system is used for date entry, formulas, display, Find,
  fill series and CSV/PDF export; XLSX/ODS round trips retain the epoch and serial values.
- 384 functions (Help › Supported Functions, Insert › Function…) in nine categories: conditional aggregates
  (SUMIFS…MINIFS, AGGREGATE, SUBTOTAL that skips nested subtotals and filtered rows), logic (IFS, SWITCH, XOR, LET,
  LAMBDA through a defined name), lookup (XLOOKUP, XMATCH, LOOKUP, INDEX with whole rows/columns, OFFSET, INDIRECT
  with R1C1, ADDRESS), dynamic arrays (FILTER, SORT, SORTBY, UNIQUE, SEQUENCE, RANDARRAY, TRANSPOSE, VSTACK/HSTACK,
  TOCOL, TAKE/DROP, CHOOSECOLS, EXPAND, WRAPROWS, MMULT, MINVERSE, FREQUENCY, LINEST…), text (TEXTBEFORE/AFTER,
  TEXTSPLIT, TEXTJOIN, NUMBERVALUE, FIXED, LibreOffice's REGEX and Excel's REGEXTEST/EXTRACT/REPLACE), dates
  (NETWORKDAYS/WORKDAY with .INTL weekends and holidays, WEEKNUM, ISOWEEKNUM, YEARFRAC, DAYS360, DATEDIF), maths and
  trigonometry incl. hyperbolic, statistics (percentiles, quartiles, ranks, regression, TREND/GROWTH, normal, t, χ²,
  F, binomial, Poisson, gamma, beta, lognormal, Weibull distributions and their inverses), finance (PMT, IPMT, PPMT,
  NPER, RATE, NPV, XNPV, IRR, XIRR, MIRR, depreciation, CUMIPMT…), information (CELL, INFO, ISFORMULA, TYPE, SHEET…)
  and engineering (CONVERT, base conversions, BIT…). Russian names (СУММ, ЕСЛИ, ВПР) are accepted too. References
  A1/$A$1, ranges, other sheets, whole rows and columns (`1:3`, `A:C`), array constants {1,2;3,4}; recalculation with cycle detection.
- Arrays like Excel 365: operators and scalar functions work element by element (`=SUM(A1:A9*B1:B9)`,
  `=MAX(IF(…))`); dynamic-array formulas spill into the cells below/right (#SPILL! when something is in the way) and are
  saved as array formulas (`.xlsx` with Excel's dynamic-array marking, `.ods` matrix formulas). Edit › Enter Array
  Formula (⌘⇧Return, in the cell or the formula bar) enters a classic Ctrl+Shift+Enter formula over the selected range:
  the formula bar shows `{=…}` in every cell of it, the result fills the range (single rows/columns repeat, extra cells
  #N/A, as in Excel/Calc). Typing into or clearing part of a legacy array is rejected; re-enter the full range with
  ⌘⇧Return. Fill, AutoFill, Series and ordinary/Paste Special cell-content writes also reject partial-array changes,
  including Skip blanks destinations. Replacing a complete array removes its array metadata; Undo restores it.
  Paste Formats can format part of an array. Replace All skips legacy arrays and locked cells.
  Saved as legacy array formulas (`.xlsx` `t="array" ref=…` without the dynamic-array `cm`; `.ods`
  `table:number-matrix-columns/rows-spanned`), and read from Excel and LibreOffice files the same way. `XLS42 --lo-compare` writes ~490 test formulas to `.xlsx` (or `ods`), lets LibreOffice compute them
  headless with a private profile and compares. The historical LibreOffice 25.2 comparison recorded differences for
  functions it lacks, array formulas
  LibreOffice only computes with Ctrl+Shift+Enter, locale formatting (FIXED/DOLLAR) and a few places where XLS42
  follows Excel (WEEKNUM type 1 at year end, MROUND/ATAN2 errors, PERCENTRANK truncating, TYPE(TRUE)=4, CONVERT
  constants).
  This optional comparison requires LibreOffice; it prints `DIFF` for mismatches but does not return a failure exit
  status for them, so it is not a strict regression gate.
- Editing: formula bar, cell references picked with the mouse, copy and paste with formula shifting, fill down/right,
  insert/delete rows and columns with formulas adjusted, sorting, sheets, AutoSum, selection totals.
- Explicit rectangular selections include blank cells. Row/column header selections are bounded by the used data
  along the opposite axis. Copy/Cut, paste/Paste Special (including external text), fill/AutoFill/Series, cell formatting,
  sorting, Text to Columns and Remove Duplicates accept at most **100,000 cells per operation**; this is not a workbook-size limit. Clear Contents and
  Clear Formats visit only stored cells and support larger sparse selections. A Cut rejected by the size limit preserves
  the data and clipboard. Pasting outside the worksheet or across a protected/partial-array destination is rejected
  before changing cells. External TSV paste respects quoted tabs, newlines and quotes, and keeps destination formatting.
- Freeze panes: at the selected cell, top row or first column — frozen rows/columns stay while scrolling (saved to .xlsx
  and .ods; the .ods view settings declare the `ooo:` namespace, without which LibreOffice ignored them).
- Export to PDF: current sheet, all visible sheets or the selection; automatic orientation, fit to width, gridlines;
  pages break at row/column boundaries. Printing uses the same layout. CSV export (delimiter choice, UTF-8), CSV/TSV import as a sheet.
  CSV stores one sheet's computed values without formulas or workbook formatting: Save/Export uses the active sheet,
  while `--convert input output.csv` uses the first sheet. CSV import keeps equals-prefixed fields as literal text;
  it does not execute them as formulas. XLSX/ODS preserve literal strings that resemble formulas, numbers, booleans
  or dates, including embedded tabs, CR and LF. Undecodable legacy XLS formulas retain their cached values.
- Context menu on cells; formatting bar with font, size, colours, alignment, number formats, borders.
- Fill handle (series of numbers/dates, "Item 1 → Item 2", formulas), find and replace, hide/unhide rows and columns,
  AutoFilter with value checklist, defined names, comments, conditional formatting (value rules, text, duplicates,
  colour scales, data bars), pictures and charts (column, bar, line, area, pie, scatter) — all saved to .xlsx.
- Printing: print area, frozen rows repeated on every page, page numbers, options in the print dialog. Fit to width
  scales wide tables to the printable width. If repeated frozen rows would consume 80% or more of the printable body,
  repetition is disabled for that sheet so the body remains printable. Row Autofit grows and shrinks wrapped rows
  and supports Undo/Redo.
- Insert › Function… (also the `fx` button by the formula bar, ⇧⌘F): searchable list of all functions grouped by
  category, with signature, description in the selected interface language, argument fields and a live result;
  while typing a formula the signature of the current function is shown above the grid.
- Data › Validation…: list (values or a range like `=$H$1:$H$5`, drop-down arrow in the cell, ⌥↓), whole number,
  decimal, date, time, text length, custom formula; stop/warning/information messages. Saved to .xlsx and .ods.
- Data › Pivot Table…: row field, optional column field, value field with Sum/Count/Average/Min/Max on a new sheet
  with totals. Saved as a real pivot table — .xlsx pivot cache + pivot table parts (refreshed on load by Excel,
  imported as a DataPilot by LibreOffice), .ods `table:data-pilot-tables` — with the result cells written as usual.
  Pivot tables from Excel/LibreOffice files are read too, so Refresh Pivot Table works after reopening; it rebuilds
  from the source (a source that grew or shrank is replaced by its current region; fields follow their header names).
  One pivot per sheet, one row/column/value field: richer pivots refresh in XLS42's simpler layout.
- Dark mode: cells without a fill are dark with light text (headers, gridlines, gutters, charts, buttons follow);
  fills and colours from the file stay, text on them stays readable, near-black text/borders are lifted. Printing
  and PDF export stay light.
- Data › Group / Ungroup (⌥⇧⌘→/←, up to 7 levels): +/− and level buttons in a gutter beside the headers;
  outline levels and collapsed groups are saved to .xlsx, .ods and read from .xls.
- Data › Protect Sheet… with optional password; Format › Protection › Locked per cell. Locked cells refuse edits
  while the sheet is protected. .xlsx keeps the legacy password hash (and verifies Excel's SHA-512 hashes), .ods uses
  SHA-256 or LibreOffice's Excel-hash key.
- Find and Replace panel (⌥⌘F): Find Next / Replace / Replace All in the sheet or workbook, in formulas or values,
  match case, entire cell; Replace All is one undo step. Edit › Paste Special… (⌥⌘V): all/values/formulas/formats,
  add/subtract/multiply/divide, skip blanks, transpose.
- Data › Sort… (up to three keys, ascending/descending, header detection, case-sensitive, natural order, left to right,
  custom orders: weekdays, months and lists from Data › Sort Lists…; moved formulas keep their row-relative references).
- Data › More Filters: AutoFilter, Standard Filter… (up to four conditions: =, <>, <, >, contains, begins/ends with,
  largest/smallest N or %, empty; AND/OR; case-sensitive; no duplications; copy results to another place), Advanced
  Filter… from a criteria range (rows OR, columns AND, Excel's criteria syntax), Reset Filter. AutoFilter conditions are
  saved in `.xlsx` (customFilters, top10) and `.ods` (database ranges with filter conditions, filtered rows).
- Data › Subtotals…: group by a column, sum/count/average/max/min/product/count numbers/stdev/var, columns to total,
  page break between groups, SUBTOTAL rows plus an outline (levels 1–3); Remove All.
- Data › Text to Columns… (tab/comma/semicolon/space/other, merged delimiters, text delimiter, fixed widths, column
  types), Remove Duplicates… (chosen columns), Statistics › Descriptive Statistics…/Correlation… (formulas on a new
  sheet); Edit › Fill › Down/Right/Up/Left and Series… (linear, growth, dates by day/weekday/month/year, step, end).
- Tools › Goal Seek… (secant method with a bracketing fallback) and Solver… (target max/min/value, variable cells,
  ≤ ≥ = constraints, integer and binary variables, non-negative option; linear models with the simplex method and
  branch and bound, nonlinear ones with Nelder–Mead and penalties). Results are one undo step. The model is kept with
  the sheet and saved the way Excel and LibreOffice keep theirs, so the Solver dialog opens prefilled in all three:
  hidden sheet-scoped names `solver_opt`, `solver_typ` (1 max, 2 min, 3 value) and `solver_val`, `solver_adj`,
  `solver_lhsN/relN/rhsN` (1 ≤, 2 =, 3 ≥, 4 integer, 5 binary), `solver_num`, `solver_neg`, `solver_eng`/`solver_lin`
  (simplex or nonlinear) plus Excel's option defaults — `definedName localSheetId=… hidden="1"` in `.xlsx`, the sheet's
  own `table:named-expressions` in `.ods` (what LibreOffice 24.2+ writes, with its `solver_lo_eng`). Options from other
  programs are kept; the model follows inserted/deleted rows and columns. Checked with LibreOffice 25.2 (its
  `SolverSettings` API reads XLS42's models from both formats and solves them; models LibreOffice wrote open in XLS42).
- Sheet menu: insert, delete, rename, move or copy (to a position, with a new name), tab colour, hide and show,
  protect workbook structure. Tab colours, hidden sheets and structure protection are saved in `.xlsx` and `.ods`.
  Pending in-cell text is committed before inserting, renaming, moving, copying, duplicating or deleting a sheet.
  Validation errors keep the draft open and block the operation. Save, PDF export and Print also commit pending input;
  invalid input stays available for correction. Successful Revert discards the draft and resets obsolete selection;
  a failed Revert preserves the workbook and draft. Renaming a sheet updates its formula references, names and
  supported validation, conditional-format and internal-link references.
- View › Split Window: two or four panes at the active cell, the left/top panes scroll on their own (freeze is turned
  off; Freeze Panes replaces a split). The split is saved per sheet and restored on open: `.xlsx` `sheetView/pane
  state="split"` with xSplit/ySplit in twips (headers included, as Excel measures), `topLeftCell` of both panes and the
  active pane; `.ods` settings `HorizontalSplitMode`/`VerticalSplitMode` = 1 with the positions in pixels and
  `PositionLeft/Right/Top/Bottom`. Splits made by Excel and LibreOffice open as splits. View › Status Bar (⌥⇧⌘S): sum/average/count of the selection (right-click to choose min, max, count numbers),
  sheet number and a zoom slider; hidden with View › Hide All Panels too.
- Insert › Hyperlink… (⌘K): web, mail, file or a place in the workbook (`#Sheet2!A1`); ⌘-click (or a click on the
  selected link) opens it; HYPERLINK() cells work the same. Saved in `.xlsx` (hyperlink relationships/locations) and
  `.ods` (text:a).
- Format › Cells… (⌥⌘1): number categories and custom codes with a preview, font and effects (double underline),
  alignment (indent, rotation, vertically stacked, wrap, shrink to fit), borders (line styles, colour, outer and inner
  lines), background, protection (locked, hidden formula). Format › Cell Styles: Normal, Title, Heading 1–3, Good, Bad,
  Neutral, Accent 1–3, Input, Calculation, Note, Warning Text, Total, Hyperlink, Currency, Percent, Comma, then the
  document's own styles (e.g. LibreOffice's Heading, Text, Footnote, Status, Result). Saved as named cell styles in
  `.xlsx` (cellStyleXfs/cellStyles, cells by xfId) and `.ods` (`style:style` of family table-cell in `office:styles` with
  `style:display-name`, cells' automatic styles with `style:parent-style-name`; Accent1 ↔ LibreOffice's "Accent 1",
  "Excel Built-in Good" from LibreOffice's .xlsx import ↔ Good). Styles read from files stay on the cells, unused
  document styles are kept too. For numeric categories, Decimal places accepts 0–10 using the keyboard or stepper
  and stays synchronized with the format code; custom format codes are not limited to ten decimal places.
  Mixed border selections show a mixed state. Accepting unchanged settings keeps those edges; None removes all edges,
  Outline applies outer edges and clears inner ones, and All applies both. Choosing a font/background colour enables
  that colour; changing border colour or style restyles existing edges. Double borders render as two separate strokes
  in the grid and PDF. Formatting changes support Undo/Redo.
  ODS retains empty formatted/unlocked input cells, empty merged ranges, named-style overrides, named constants and
  expressions, quoted-sheet print areas, and dashed/dotted borders.
- Tests: `build/XLS42.app/Contents/MacOS/XLS42 --selftest` (formulas with fixed expected values,
  formats, data tools, solver, file round trips) and `--uitest` (editing operations in a window; the
  clipboard is restored afterwards). `--feature-demo out.xlsx|ods` writes a file with the sheet features for checks in
  other apps, `--model-demo out.xlsx|ods` one with a Solver model, a split window, named cell styles and array formulas;
  `--info file` prints them (solver, split, styles, arrays) for any file.
  `--convert input output.xlsx|ods|csv` chooses the output format; `--roundtrip input output` always writes XLSX.
  `--lo-compare /absolute/output/directory [/path/to/soffice] [ods] [verbose]` is the optional LibreOffice comparison
  described above. The directory must be absolute; the default executable is
  `/Applications/LibreOffice.app/Contents/MacOS/soffice`. `ods` selects ODS instead of XLSX, and `verbose` prints every
  comparison. This comparison is separate from `--selftest` and is not run by the CLI audit script.
  `--export-pdf input.xlsx output.pdf [all] [landscape] [nogrid] [nofit]` uses the same print layout;
  `all` selects visible sheets. `python3 scripts/audit-spreadsheets-cli.py --binary .build/debug/XLS42` checks real
  XLSX/ODS/CSV conversions and PDF output; see the regression matrix below for its scope.

## DOC42 — documents (.docx, .doc, .rtf, .odt, .md, .txt)

- Page on a grey desk, styles (title, headings, quote), font, size, colour, highlighting, alignment,
  lists, indents, line spacing, tables, images, signatures and stamps from the shared library, PDF export, word count.
- Word styles (Heading 1, Title, docDefaults, theme fonts) and lists are resolved on open, so documents
  look as intended. Saving .docx uses our own writer, which keeps tables and images.
- Opens and saves .docx, .rtf, .odt, .md, .txt, .doc and HTML. DOCX preserves supported pictures, tables, lists,
  styles and page settings; other formats retain only the features they support. Markdown: GitHub tables, nested lists,
  code, links, strikethrough, pictures by relative path. TXT contains plain text. UTF-8/UTF-16/Windows-1251 are detected;
  the original text encoding is retained when all characters fit, otherwise saving uses UTF-8.
- Export: PDF (paginated A4/Letter), Word .docx/.doc, RTF, ODT, HTML, Markdown, text.
- Real pages on screen (text flows across sheets), paper size, orientation, margins, header/footer, page numbers —
  printing and PDF export use the document's page settings. Table rows/columns insert/delete.
  Pictures and Word styles survive .docx round trips.
- Horizontal and vertical rulers (View › Show/Hide Ruler, ⌥⌘R) follow page geometry and zoom. Drag the margin
  boundaries in the horizontal ruler's middle band to change left/right page margins, or the vertical boundaries
  for top/bottom margins. The upper triangle adjusts the first-line indent; lower triangles adjust left/right paragraph
  indents in the active column or header/footer. Dragging the left indent carries the first line. Hold Option to bypass
  the 0.25 cm snap. A click without dragging changes nothing; margin and paragraph changes support Undo/Redo.
  Page margins apply to the corresponding section; the vertical ruler follows the visible page when the caret is offscreen.
  Web layout keeps print settings intact and disables page-margin dragging; paragraph indents remain editable.
  Page Setup rejects non-finite/negative measurements and settings that leave less than 36 pt per column or in height.
- Footnotes (⌥⌘N): numbered automatically, shown at the bottom of their page, click to edit; read from and saved to .docx
  (footnotes.xml; endnotes are read as footnotes), .odt (text:note), Markdown `[^1]`, a "Notes" list in RTF/HTML/text.
- Comments (⌥⌘M): highlighted text with cards in the margin (author, date, edit, resolve, delete); .docx comments.xml
  with resolved state (commentsExtended.xml), .odt office:annotation (resolved state as LibreOffice writes it) and
  RTF Word annotations, read and written; Markdown export as `<!-- comment: … -->`, HTML as `<mark title="…">`.
- Track changes (Edit › Track Changes, ⇧⌘E; Export As… is ⌥⇧⌘E): typed text becomes an insertion (underlined, in the
  author's colour), deleted text stays struck through until accepted. Review menu: Show Markup (off = final view),
  Next/Previous Change, Accept/Reject Change, Accept/Reject All; changes appear as cards next to the comments, with
  accept and reject buttons. Undoable. Saved to .docx as w:ins / w:del (w:delText, paragraph marks) with
  w:trackRevisions, read from Word and LibreOffice files; other formats, PDF and printing get the text with all
  changes accepted. Commands (tables, footnotes, Replace All, …) are not tracked, nor are formatting changes.
- Sections: Insert › Section Break (Next Page); each section has its own paper, orientation, margins and columns
  (Document Setup applies to the cursor's section or the whole document). Columns 1–3 with spacing; the last page of a
  multi-column section balances its columns. Mixed page sizes export to PDF page by page. Saved to .docx as
  sectPr / w:cols and read back (continuous breaks open a new page).
- Dark mode: the window, rulers, comment and change cards follow the system; pages stay white paper with black text.
- Table of contents from Title/Heading 1–3 with dot leaders, page numbers and links to the headings; Update rebuilds it
  in place. Run Update after changing margins or column width to recalculate its tab stops and page numbers.
  Saved as a Word TOC field with bookmarks and hyperlinks.
- Find bar across all pages with replace; Find and Replace dialog (⌥⌘F): match case, whole words, regular expressions,
  Replace / Replace All (undoable).
- Clear Formatting resets selected text to Body appearance while preserving pictures, fields, table/list structure, links, comments,
  tracked changes and section breaks. Character styles inherit parent styles, including underline. Formatting commands
  supported in headers/footers target that editor; drop caps are available only in the body.
- Links (⌘K: insert, edit, remove; saved in docx/odt/rtf/md/html), special characters, Paragraph dialog (indents, spacing,
  exact line height), Clone Formatting (⌥⌘C; double-click keeps it on), Statistics (document and selection).
- Styles panel (⌘T or F11, also Format › Styles): paragraph styles (Body, Title, Heading 1–6, Quote, List, Caption,
  TOC 1–3, Footnote, custom) and character styles (Emphasis, Strong, Code, custom), each shown in its look; click to
  apply; New Style from Selection, Update Style to Match Selection, Modify Style (font, size, bold/italic/underline,
  colour, spacing, line spacing, indents, alignment, based on, next style), delete custom styles. A changed style
  restyles body paragraphs in it (and in styles based on it), keeping what was formatted by hand; undoable. Saved as
  real styles: .docx styles.xml with w:pStyle/w:rStyle (run properties equal to the style are left to it), .odt
  named styles in styles.xml (paragraph styles); read back from Word, LibreOffice and DOC42 files.
- Navigator (F5, side panel): headings as a tree (click to jump, drag a heading to move its chapter), tables, images,
  bookmarks, comments, footnotes, links, sections and fields.
- Bullets and Numbering (Format): bullets, numbers, letters, Roman numerals, text before/after ("(a)"), start value,
  restart numbering, showing higher levels (1.1.1); multi-level lists with Tab / Shift-Tab at an item's start (Format ›
  Demote/Promote One Level); numbers follow edits. Outline Numbering numbers Heading 1–6 (1, 1.1, 1.1.1 or I, A, 1, a).
  Saved as .docx numbering.xml (abstractNum/num, lvl with numFmt, lvlText, start, pStyle for headings) and read back.
- Fields (Insert › Field): page number, page count, date and time (formats, fixed or updating), title, author, file
  name, chapter (current Heading 1); also sequence numbers (captions) and table formulas. Shaded on screen, updated
  with the layout (each page shows its own number), Update Fields (F9). .docx: w:fldSimple (PAGE, NUMPAGES, DATE \@,
  TIME, TITLE, AUTHOR, FILENAME, STYLEREF, SEQ, =SUM(ABOVE); w:fldLock for fixed ones); simple and complex fields are
  read back. Title and author: File › Properties (docProps/core.xml).
- Headers and footers: rich text with fields, tab stops for left/centre/right, a different first page and different
  odd/even pages, per section (or the whole document). Double-click the top or bottom of a page to edit there
  (Esc or a click in the text ends), or Insert › Header and Footer (dialog with page number position).
  .docx header1/2/3.xml (default, first, even) with w:titlePg and w:evenAndOddHeaders, read and written; Word's rule
  that a section without its own headers inherits the previous one's is followed.
- Tables: Table Properties (column widths, borders all/outer/inner/none, line width and colour, cell padding, cell
  background, vertical alignment, repeated heading rows), Merge Cells / Split Cells (merged back, or into columns or
  rows), insert/delete rows and columns with merged cells, Sort (column, ascending/descending, numbers or text, heading
  row kept), Formula (=SUM(ABOVE), LEFT, AVERAGE, MIN, MAX, COUNT, PRODUCT; updates), Convert Text to Table (tabs,
  semicolons, commas, paragraphs) and Table to Text, AutoFormat presets, Tab moves between cells. Rows joined by a merged
  cell are kept on one page when the group fits on a page. .docx: w:tblBorders/w:tcBorders, w:shd, w:gridSpan, w:vMerge, w:tblHeader, w:tblGrid
  widths, w:tblCellMar; read back (the system reader drops spans and shading: DOC42 rebuilds the tables).
- Pictures: click for handles (corners keep proportions; Shift frees them), drag a floating picture to move it; wrap in
  line, square, top and bottom, behind or in front of text (Format › Image › Wrap); anchored to the paragraph, placed on
  its page; border, crop and size in cm in Image Properties; Insert › Caption ("Figure 1: …", numbered automatically,
  above tables). .docx wp:inline / wp:anchor with wrapSquare, wrapTopAndBottom, wrapNone/behindDoc, srcRect, a:ln.
- Paragraph borders and background (box, top, bottom, …; neighbouring paragraphs share a box; w:pBdr/w:shd), drop caps
  (2–5 lines; Word's framed drop-cap paragraph), page background colour and watermark text (Format › Page; w:background,
  Word's VML watermark in the headers), Hyphenation (w:autoHyphenation), Character dialog (font, size, bold/italic,
  colour, highlight, all caps, small caps, superscript/subscript, spacing, underline single/double/thick/dotted/dashed/
  words, strikethrough single/double; w:caps, w:smallCaps, w:dstrike, w:spacing, w:u, w:vertAlign, read back).
- PDF export (File › Export as PDF…): page range, image quality, headings as PDF bookmarks, clickable links (web
  addresses, table of contents and other internal links), comments as notes, title and author in the PDF properties.
  Invalid ranges or ranges containing no document page report an error. Oversized range endpoints are bounded to
  actual pages. Saving/exporting commits pending header/footer edits; Revert discards pending edits and restores
  document settings and styles from the saved file.
- View: single page, book view (two pages), multiple pages, web layout (paper edges and headers/footers hidden;
  explicit page/section breaks remain); the window subtitle shows page X of Y; status bar with words
  (of the selection), language, layout buttons and a zoom slider (View › Hide Status Bar; Hide All Panels hides it too).
  Hide All Panels temporarily hides both rulers and comments without changing their saved visibility preferences;
  restoring panels returns the previous state, and new windows retain the saved settings.
- Command line: `--stats file`, `--dump file`, `--demo out.docx`, `--export in out.pdf|docx|doc|rtf|odt|html|txt|md`,
  `--toc in out`, `--replace in out FIND REPLACEMENT [--case] [--words] [--regex]`,
  `--pdf in out.pdf [--pages 1-2] [--quality 0.5] [--comments] [--no-bookmarks]`.
  Known document extensions are routed directly to their reader, including in a headless process without registered UTIs.
- Known limits: small caps are shown as capitals when the font has no small capitals (saved correctly); heading rows
  repeat on each page in Word/LibreOffice but not on DOC42's screen; .odt keeps paragraph styles but not character
  styles, list definitions, fields or rich headers (plain text there); page-relative positions of floating pictures from
  other programs are approximate.

## IMG42 — images (everything ImageIO reads, plus SVG and PDF)

- Formats: whatever ImageIO decodes on this Mac (asked at runtime) — JPEG, PNG, HEIC/HEIF, AVIF, WebP, GIF, TIFF, BMP,
  ICO/ICNS, PSD, TGA, JPEG 2000, OpenEXR, JPEG XL, camera RAW (CR2/CR3/NEF/ARW/DNG/RAF/ORF…), plus SVG and PDF (pages).
  EXIF orientation is applied. Animated GIF/APNG/WebP/HEICS play; multi-page TIFF and PDF page with ⌥← / ⌥→.
  A broken file shows a message in the window, and navigation carries on. Raster display and edit allocations are
  limited to 16,384 pixels per side and 100,000,000 pixels total. Larger raster files are shown as a reduced preview;
  pixel edits use Save As, so ordinary Save cannot replace the full-resolution original with that preview. SVG display
  is rasterized up to 4,096 pixels on its longest side; PDF pages up to 4,000. Animation loading currently retains at
  most 2,000 frames, rather than streaming arbitrarily long animations.
- Flipping through: opening a file shows its whole folder in Finder name order; several files or a folder make the set.
  → / ← (also Space / ⇧Space, Page Up/Down, a mouse wheel notch or a two-finger swipe when the image fits), ⌘↑ / ⌘↓ first/last;
  position "12 / 87" in the subtitle; View › Loop Through Folder; Sort By name, date modified, date taken, size.
- Viewing: fit (⌥⌘0), actual size (⌘0), ⌘+ / ⌘−, pinch, ⌘-scroll, double-click toggles fit / 100 %, drag to pan,
  checkerboard behind transparency, background Neutral/Dark/Black/Light, full screen (⌃⌘F or F; black, every bar hidden,
  the toolbar slides in at the top), slideshow (⇧⌘F, 3 s, Space pauses, Esc stops).
- Panels (View menu, each with a check mark; state remembered): Markup Toolbar ⇧⌘A, Filmstrip ⌥⌘F (lazy cached
  thumbnails, click to jump), Info Panel ⌘I (file, dimensions, format, colour profile, camera, lens, exposure, ISO,
  GPS with Show in Maps, all EXIF/IPTC/TIFF fields in a collapsible All Metadata section), Adjust Colour ⌥⌘C, Hide All Panels ⇧⌘\.
- Toolbar like Preview's: Previous/Next · Zoom − 1:1 + · Highlight ▾ (colour) · Remove Background, Rotate · Markup ·
  Adjust · Info · Share · Search (Live Text: words are recognised with Vision and highlighted, Return goes to the next
  match; Edit › Copy Text in Image ⇧⌘C).
- Live Text like Preview (VisionKit): text in the image is recognised in the background when it is shown (cached per
  image; Russian, English, Turkish and your preferred languages, plus automatic detection) and can be selected with the
  mouse while viewing or in the Select tool — I-beam over text, drag across words and lines (rotated text too),
  double-click a word, triple-click a line, ⌘A selects all text once some is selected, ⌘C copies, Esc deselects; the
  right-click menu has Look Up, Translate, Search With Google, Copy, Share. Links, phone numbers and e-mail addresses are
  clickable. Drawing tools and marks keep the mouse (no text selection while drawing). Image/query changes invalidate
  pending recognition results so an old search cannot overwrite the current image's highlights.
- Markup bar like Preview's: Select; Selection ▾ (rectangular, elliptical, lasso, Smart Lasso, aspect ratios; Crop to Selection ⌘K,
  Delete Selection ⌫, Copy, Invert ⇧⌘I); Instant Alpha (drag on a colour: farther = wider range, then ⌫); Sketch
  (a rough line, box, circle or star becomes a clean shape); Draw; Shapes ▾ (line, arrow, rectangle, rounded rectangle,
  oval, speech bubble, star, polygon, loupe, spotlight, pixelate); Text; Sign (shared signature library ⇧⌘K);
  Adjust Colour; Adjust Size; Shape Style ▾ (width, dashed, arrowheads, shadow, loupe zoom); Border and Fill colour;
  Text Style ▾ (font, size, colour, bold/italic/underline, alignment); Image Description (alt text, saved in the metadata).
  Marks stay objects — select, move, resize with handles, restyle, ⌫ deletes — until the image is saved.
  Save, Export, Copy, Share and Print include text still being edited. Revert to Saved reads the file before changing
  the editing session: a failed read keeps the draft, a successful Revert discards it, and Undo restores it.
  Escape/tool changes cancel unfinished gestures. Style controls are enabled only for properties the selected mark
  supports. Rotating/flipping moves text-box geometry while keeping its glyphs upright; image stamps rotate/flip their
  actual bitmap. These editable objects are flattened when written.
- Smart Lasso: paint a wide stroke around the object's edge (24 pt on screen at any zoom); the selection snaps to the
  edge inside the stroke — colour edges across the band, a two-region fit and Vision's subject mask, joined into one
  closed path by dynamic programming. Then Crop / Delete / Copy / Invert as with any selection. Excessive or invalid
  strokes are rejected; contour resampling is bounded to 20,000 points.
- Adjust panel (Core Image, live, non-destructive): exposure, brightness, contrast, highlights, shadows, saturation,
  vibrance, temperature, tint, sharpness, vignette; looks (Black & White, Noir, Sepia, Invert); Auto Enhance; double-click
  a slider resets it; hold Show Original to compare; Reset All. One undo step per slider drag; keyboard/accessibility
  changes also create undoable edits. Both positive and negative Highlights values affect rendered pixels.
- Image: Rotate ⌘L / ⌘R and Flip — untouched JPEG/HEIC/PNG/TIFF files get a new orientation tag (no re-encoding, undoable);
  otherwise it becomes an edit. Adjust Size ⌥⌘R (pixels, %, cm, inches, resolution, keep proportions), Remove Background
  (Vision subject lifting), Image Description.
  Switching size units preserves pixel dimensions. With Resample image off, changing resolution changes print size
  without resizing pixels; width/height controls are read-only in that mode. DPI changes support Undo/Redo and are
  written on Save and Export. Invalid size or resolution disables OK. Metadata-only saving attempts to preserve the
  encoded pixels, with ordinary image writing as a fallback where the input is a writable single full-resolution image.
- Files: for a writable single still image, Save ⌘S writes markup + adjustments in the original format
  (JPEG quality 0.92, EXIF/GPS kept, orientation reset), asking once before replacing. Pixel edits to animated,
  multi-page or read-only inputs use Save As to export the displayed frame/page. Save As ⇧⌘S and Export ⇧⌘E offer
  PNG, JPEG with quality, HEIC, TIFF and AVIF when supported, with size presets.
  Save As adopts the chosen file even in a different folder or over an existing member of the image set. Export to
  another file retains the source URL and editable state; writing over the current file establishes a new baseline.
  Render/encoding failures report an error and leave an existing destination intact instead of writing an image with
  missing marks/effects. Transparent pixels are preserved by PNG and composited onto white for JPEG; merely having an
  RGBA buffer does not force an opaque JPEG edit into PNG.
  Leaving an edited image asks Save / Don't Save / Cancel. Move to Trash ⌘⌫ (undo puts it back), Copy ⌘C (image or
  selection), Show in Finder ⇧⌘R, Open With, Share, Print (fit to page), Open Recent.
  File-level Undo/Redo for Trash and lossless rotation/flip targets the original file even after navigation; exporting
  over an already viewed neighbour refreshes its cached image before it is displayed again.
- Command line (headless): `--info files…`, `--edit in out [--brightness N --contrast N … --look sepia --auto --demo-marks
  --rotate right --flip h --resize WxH --crop x,y,w,h --remove-background --description TEXT]`, `--stats file`
  (mean luminance), `--ocr file`, `--rotate file left|right` (lossless), `--types`, `--smart-lasso in "x,y;…" mask.png
  [radius]` (selection mask for a stroke), `--smart-lasso-eval in [--gt mask.png] [--radius r] [--noise a] [--vis out.png]`
  (simulated rough strokes around the subject: IoU of plain vs Smart Lasso). Invalid options, nonfinite/out-of-range
  numbers, missing values and unsupported output extensions return a nonzero exit without replacing the destination.
  CLI `--edit` refuses a reduced-resolution preview rather than silently exporting it as a full-resolution edit.
  `--demo-marks` follow subsequent resize/crop operations; `--description ""` explicitly clears the caption.
- Regression checks: `.build/debug/IMG42 --selftest` (pixels and file output),
  `python3 scripts/audit-images-cli.py --binary .build/debug/IMG42` (independent file/OCR checks), and
  `python3 scripts/audit-ui.py --app IMG42` (native editing, markup, files and navigation). Record the tested binary
  identity and actual results alongside each run.
- UI checks: `K42_SNAPSHOT=out.png` plus `K42_NEXT=n`, `K42_PAGE=n`, `K42_INFO`, `K42_FILMSTRIP`, `K42_ADJUST`, `K42_MARKUP`,
  `K42_DEMO_MARKS`, `K42_TOOL=n`, `K42_SELECT=rect|ellipse|alpha` (+ `K42_CROP`, `K42_DELETE`), `K42_SEARCH=text`,
  `K42_ZOOM=x`, `K42_STAMPS`, `K42_RESIZE`, `K42_DESCRIPTION`, `K42_FULLSCREEN`, `K42_HIDE_ALL`, `K42_SAVE`, `K42_UNDO`,
  `K42_DUMP_MENUS` (prints the markup drop-downs), `K42_APPEARANCE=dark|light`, `K42_EXPORT_ACCESSORY`,
  `K42_DUMP_CANVAS`; Live Text: `K42_LIVETEXT_SELECT="from..to"`, `K42_LIVETEXT_DRAG="x,y;x,y"`, `K42_LIVETEXT_CLICK=x,y,n`,
  `K42_LIVETEXT_ALL`, `K42_LIVETEXT_MENU=x,y`, `K42_LIVETEXT_PROBE`, `K42_LIVETEXT_KEY` (synthesised events, results
  printed); `K42_SMART_LASSO="x,y;…"|@file` (+ `K42_SMART_CROP`).

## PPT42 — presentations (PPTX, ODP)

- Files: PowerPoint .pptx and .potx (PresentationML: slides, master and layouts with theme colours/fonts and placeholder
  positions, text with runs, bullets/numbering and levels, preset and custom shapes with fill/line/shadow and theme style
  references, lines and arrows with arrowhead sizes, pictures with crop, tables, groups, charts, SmartArt, video and
  audio, comments, backgrounds solid/gradient/picture, speaker notes, 16:9 and 4:3, transitions, animations as p:timing)
  and OpenDocument .odp and .otp (pages, master pages as layouts, frames, custom shapes and paths, images, tables,
  groups, chart objects, media plugins, annotations, notes, styles, transitions, anim:* effects). Legacy .ppt opens with
  a message suggesting conversion. Fonts missing on the Mac (Calibri, Aptos…) are replaced by close ones.
- Shapes: all 187 OOXML preset shapes (geometry, shading, text areas and adjust handles from the preset definitions —
  data generated from LibreOffice's oox-drawingml-cs-presets, MPL 2.0, by `Resources/PPT42/make_presets.py`), drawn
  with yellow adjust handles on the canvas; custom geometry (a:custGeom with guide formulas, moveTo/lnTo/arcTo/quad/cubic,
  ODF svg paths) read, drawn and written back; Insert › Shape in sections (basic, arrows, flowchart, stars and banners,
  callouts, brackets and math, lines).
- Charts: column/bar (clustered, stacked, 100 %), line, area, pie, doughnut and scatter drawn natively from the chart part's
  caches (titles, legend, axis titles, data labels, gridlines, theme/series/point colours, number formats); unchanged
  charts are written back with their chart XML and workbook untouched, edited or new ones as chart parts with caches
  and an embedded workbook; Insert › Chart… (⌥⇧⌘C) with a data grid and chart type, chart options in the inspector,
  double-click edits the data. ODP: chart objects (Object N/content.xml) read and written so LibreOffice shows them.
- SmartArt: shown and edited as the shapes of its drawing part; unchanged or text-edited diagrams keep their parts
  (texts written into the drawing and the data model), moved ones too; Format › Convert to Shapes.
- Master view (⌥⌘5): the master and every layout in the sidebar; placeholder position, size and text styles per
  layout, including placeholder shadow, opacity and shape rounding; Slide › Master Layouts: new, duplicate, rename, delete, insert placeholder. Saved to slideLayout parts and,
  in ODP, to one master page per layout.
- Review: Insert › Comment (⌥⌘M), comments pane in the inspector (reply, resolve, delete), markers on the slide;
  pptx comments with threads (modern threaded comments are read), odp annotations.
- Media: Insert › Video or Audio… (mp4/mov/m4a/mp3) with a poster frame; plays in the slide show on click or
  automatically, optionally looped (inspector); pptx videoFile/audioFile + p14:media, odp draw:plugin.
- Animations (inspector › Animation tab): entrance (appear, fade, fly in, zoom, wipe), exit (disappear, fade out,
  fly out, zoom out), emphasis (pulse, spin, grow/shrink, change colour) and motion paths, on click / with previous /
  after previous, delay and duration, reorder; numbers and paths shown on the canvas; played in the slide show.
- Export: PDF (⌥⌘E), notes pages PDF, PNG/JPEG images, outline as RTF or Markdown, movie (.mp4 with slide timings,
  animation steps and cross-fades); printing of slides, handouts or notes pages. File › New from Template… (the six
  themes or a template file); Save As .potx/.otp.
  Notes PDF and printed notes continue onto additional pages when the notes exceed one page; the on-screen Notes Page
  view remains a preview of the first notes page. PDF and image export omit hidden slides unless explicitly included.
  Movies contain rendered slide states and cross-fades without embedded audio/video playback; embedded videos remain
  poster images. MP4 and MOV outputs use their respective containers. Failed movie exports preserve an existing destination.
- Editing like Impress/Keynote: thumbnails sidebar, New Slide with a layout picker, canvas with zoom, handles to move /
  resize / rotate with snapping guides, align/distribute, arrange, group, copy/paste, drag & drop of images; text in
  place with the format bar (formatting while editing is its own undo step); tables; Inspector (⌥⌘I) with Slide,
  Shape, Text, Animation and Comments tabs; six built-in themes; Outline, Slide Sorter and Notes Page views; Find and
  Replace. Find Next/Previous visits each occurrence in text, individual table cells and notes. Replace All can match
  across formatting runs, preserves surrounding formatting, inherits the first matched character's formatting for the
  replacement, and leaves dynamic fields intact. Individual replacement works in table cells; notes replacement updates
  both the document and visible editor and supports Undo/Redo through the notes editor's text undo history.
  Text formatting reaches nested groups and tables. Vertical alignment applies to the edited table cell or all text
  containers of the selected object. Inserting/deleting table rows and columns transforms merged spans and retains
  the content of a merged anchor while part of its span survives. Empty paragraphs retain their chosen formatting.
  Inspector fill/line controls apply to grouped shapes and tables; table borders support colour, width and removal,
  while unsupported dashed-table-border controls are disabled. Geometry inputs reject invalid/nonfinite values.
  Group flip and Ungroup preserve reflected child geometry. Pasting between documents remaps embedded media names
  so a destination image with the same filename cannot replace the pasted image. Undo/Redo restores objects, media
  and selection together.
  Pending in-place and outline text is included in the document's edited state. Save/export flushes outline typing
  immediately; object commands commit in-place text first. An unchanged text edit creates no document Undo step.
  Failed Revert preserves the visible draft; successful Revert discards it, cancels delayed outline updates and refreshes
  the document views. Notes editors clear obsolete local Undo when switching slides; Revert resets notes and outline Undo.
- Slide show: ⌥⌘P / ⇧⌘P, Present in Window; transitions, animations, media; click/→/Space next, ←/⌫ previous,
  number + Return jumps, B/W black/white screen, L laser pointer, P pen (E erases), G slide overview, C hides the
  cursor, Esc ends; presenter view on a second display, or Rehearse (⌥⌘R) in a window. An automatic entrance group on
  the first slide runs at show startup; later navigation cancels stale delayed animation requests.
- Command line: `--info file` (objects, charts, media, animations, comments), `--export in out.pdf [--notes --hidden] | dir
  [--jpeg --width N --hidden] | out.rtf | out.md | out.mp4|mov [--seconds 5 --width N --hidden]`,
  `--convert in.pptx out.odp|.potx|.otp` (and back), `--demo out.pptx|odp` (a representative feature deck),
  `--selftest` (file, layout and malformed-input regression fixtures). `python3 scripts/audit-presentations-cli.py
  --binary .build/debug/PPT42` independently checks conversion, templates, PDF notes/visibility, images, failed outputs
  and movie containers; use a packaged binary path to test that build.
- UI checks: `K42_SNAPSHOT` with `K42_SLIDE=n`, `K42_VIEW=sorter|outline|notes|master`, `K42_SELECT=i,j`,
  `K42_INSPECTOR=slide|shape|text|animation|comments`, `K42_EDIT`, `K42_SHOW=1|window` (+`K42_SHOW_STEPS`,
  `K42_SHOW_KEYS`), `K42_PRESENTER=1`, `K42_LAYOUTS`, `K42_TEMPLATES`, `K42_DO="cmd;cmd"` (scripted commands such as
  chart:, anim:, comment:, media:, newlayout, see Snapshot.swift), `K42_SAVE=path`, `K42_PASTEBOARD=1` (private
  pasteboard), `K42_CANVAS_PNG`. Test runs do not save the window frame. Snapshot runs request app activation before capture.
- Not yet: SmartArt layout (shapes are not re-laid out when text is added; geometry changes save it as shapes),
  hatch and pattern fills (approximated), 3-D chart looks (drawn flat), wipe animations (fade), comment positions in
  LibreOffice's legacy-comment import are approximate. Table borders do not support dash patterns; table text autofit
  is not an inspector command. The Notes Page view previews the first page, while notes PDF/printing paginate all notes.
  Bitmap rendering requires positive finite slide dimensions, a width and computed height of at most 16,384 pixels
  each, and at most 64,000,000 pixels. ODP table expansion is limited to 100,000 cells per table; repeated-space counts
  above 1,000,000 or invalid repeats/spans are rejected. These are explicit resource limits, not compatibility claims.

## SCN42 — scanning (replaces Image Capture)

One window: pages on the left, a big preview in the middle, a few clear settings and one big **Scan** button on the
right. Every scan adds pages to the document; save them as one searchable PDF when you are done.

- Scanners: devices made available by Image Capture (ImageCaptureCore: USB, network/Bonjour, AirScan/eSCL), picked in the
  toolbar and remembered; status in plain words (connecting, warming up, busy, feeder empty or jammed, needs a driver…).
  Flatbed, document feeder with duplex, transparency unit, subject to the device/driver's capabilities. **Scan with iPhone or iPad** (Continuity Camera: Take Photo,
  Scan Documents) from the toolbar, File menu or welcome window — no scanner needed.
- Settings: presets Document, Photo, Receipt, Draft plus your own (Scan › Save Settings as Preset…, ⌃⌘1…); source, colour
  (colour / gray / B&W), resolution, paper size (Auto, A4, Letter, Legal, A5, A6, business card, custom). More Options:
  bit depth, brightness, contrast, B&W threshold, descreen. Device source selection completes before using that source's
  resolution, duplex and paper capabilities; pending selection preserves the requested DPI/duplex. Switching source
  during a job prepares the next source when that job finishes. Invalid custom paper sizes and nonfinite values cannot start a scan.
- Overview (⇧⌘B) shows the glass; drag to mark one or several scan areas (move, resize by the corners, ⌫ deletes; hand-drawn
  areas are remembered). **Separate photos on the glass**: the photos are found on the overview (glass background +
  Vision rectangles) and each one becomes its own straight image. Areas stay within the glass; changing source or device
  clears an obsolete overview. Stop Scanning rejects late scanner callbacks while already received pages can finish
  processing. Cancelling a Copy job does not print the partially received document.
- After scanning (Core Image + Vision, in the background while the scanner goes on): crop to the page and straighten
  (perspective correction; text-line deskew for feeder pages), turn sideways / upside-down pages upright (text direction),
  remove blank pages (ink coverage + tone), document clean-up (paper made white: shadows, yellowing and uneven light
  divided out; sharper text; adaptive B&W; border clean-up), photo auto-enhance. Automatic B&W retains existing black
  ink even on an already binary page. Invalid geometry, failed processing or failed OCR reports an error; it is not
  recorded as successfully processed pixels or successfully recognized empty text.
- Import: File › Import and dropping files add raster images and every page of TIFF/PDF, in source order. UI raster
  import applies EXIF orientation and bounds the decoded image to 6,000 pixels on the longest side; usable source DPI
  is reduced proportionally to preserve physical page size. PDFs are rasterized at up to 250 dpi, also bounded to
  6,000 pixels on the longest side with matching effective DPI. Imports reject locked, corrupt or invalid pages instead
  of silently adding only the readable subset. A multi-page input is limited to 2,000 pages. An animated image imported
  through the UI contributes its first frame.
- Pages: thumbnails with drag-to-reorder, drop images/PDFs to insert, rotate ⌘L/⌘R, delete ⌘⌫, move ⌥⌘↑/↓, Rescan Page ⌥⌘B,
  Insert Next Scan After This Page, Split Document Here (rest goes to a new window), Enhance ⌥⌘E, colour per page,
  Crop and Straighten Again, Remove Blank Pages. Rotation, reordering, deletion, rescan and image adjustments support Undo;
  OCR does not. Dropping before the first thumbnail inserts at the start. Rescan replaces its selected page after
  processing and cannot start while another scan or its image processing is still running. Selection and insertion
  anchors follow page identity through reordering. Undo restores page pixels, processing options, text, dimensions and
  selection; returning to the saved content clears the unsaved marker. Unchanged commands do not add an Undo entry.
  Pages created by Split are independent from those retained in the original window.
- Text: Recognize Text ⌥⌘R (Vision, Russian/English/Turkish + your languages): invisible text layer in PDFs (searchable,
  selectable), Copy Text ⇧⌘C, Export Text (.txt, .rtf). Text exports use top-to-bottom, then left-to-right reading order.
  Pixel edits invalidate old OCR. Delayed recognition or cleanup results cannot overwrite edited, removed or replaced
  pages; failed recognition does not mark an empty result as cached text.
- Saving ⌘S: PDF (one file or one per page), PDF/A-style (XMP pdfaid 2B, sRGB output intent), multi-page TIFF (B&W as 1-bit
  CCITT G4), JPEG/HEIC (quality), PNG. File size presets (lossless / 300 / 200 / 150 dpi with JPEG) with the expected size
  shown before saving; the prepared JPEG stream is embedded without another encoding pass (the source may already
  have been processed/resampled/recompressed), and B&W pages stay 1-bit. Name templates (`Scan YYYY-MM-DD HH-mm`,
  counter `{n}`, first line of text `{title}`), folder (default ~/Documents/Scans), then open in PDF42, IMG42, Finder or the
  default app. Export… (any name/place/format), Share, Send by Mail, Print, **Copy** ⌥⌘P (scan and print N copies
  on the default printer).
  Saving writes a snapshot: later scans, edits or OCR results remain unsaved. Undo during a write is compared to the
  actual saved snapshot on completion; Redo to that snapshot becomes clean. Close is blocked while scanning or image
  processing is pending; after that, Close → Save closes only after a successful save with no remaining changes. Shared
  busy state disables conflicting scan/import/save operations while the active job finishes. Saved history records the
  actual exported snapshot. PDF compression keeps the original physical page size and OCR placement when resampling.
  Each output file is replaced only after successful encoding; a multi-file export is not one atomic transaction.
- Signature or Stamp ⇧⌘K (Tools menu, optional toolbar item): pick a signature or stamp from the shared library or
  generator (each insertion is a new impression), place it over the shown page — drag to move, drag a corner to resize
  keeping proportions, arrows nudge — and press Return or **Print into Page** (Esc cancels). The mark is printed into
  the page pixels with multiply blending, like ink on paper: text under it stays visible, and it becomes part of the
  scan in every saved format, including PDF (no annotation or separate layer). Both the shown page and the original
  behind clean-up get it, so changing clean-up/colour or Crop and Straighten Again keeps the mark; gray pages stay gray
  and B&W pages stay 1-bit. One Undo step removes it. Showing another page drops an unfinished placement.
- History (⌥⌘3): saved scans with thumbnails; double-click opens, right-click reveals or removes the history entry.
  Removing an entry or clearing History does not delete the saved document.
- Command line (headless, nothing scans for real): `--make-test-inputs dir` (skewed page on a grey glass with a shadow,
  three photos on the glass, feeder pages: blank with noise, skewed, upside down, yellowed, sideways), `--process in out.png|out.jpg
  [--crop --deskew --rotate --enhance --photo --gray --bw --dpi n]`, `--pipeline dir|files… out.pdf [--ocr --text out.txt|out.rtf --remove-blank --compress
  original|high|balanced|smallest --bw --gray --pdfa --format tiff|jpeg|png|heic]` (prints crop angle and box, deskew,
  rotation, ink, blank, OCR sample, sizes), `--detect-photos glass.png [outdir]`, `--fake-scan dir out.pdf [--feeder --duplex
  --photos --dpi n]`, `--list-devices [s]` (read-only discovery), `--name template`.
  Raster CLI input keeps its full resolution within 32,768 pixels per side and 256,000,000 pixels; oversized input is
  rejected. PDF CLI input uses the bounded PDF rasterizer described above. `--process` accepts one page; `--pipeline`
  handles multi-page inputs. Unknown, duplicate, incompatible or incomplete flags and invalid values fail before writing.
  Without `--format`, an unsupported output extension is rejected; an explicit pipeline `--format` chooses the output
  format and its actual extension. Explicit `--dpi` accepts finite values from 1 to 2,400 (whole numbers for fake scans).
  Processing and rendering have the same 32,768-side/256-million-pixel allocation bound, which includes standard Legal
  paper at 1,200 dpi. Cleanup and geometric rendering use 8-bit channels; B&W file output can be packed to 1-bit. Scanner
  acquisition bit depth does not promise high-bit-depth preservation through those operations. The 2,000-page bound
  applies to each multi-page input; batches remain in memory without an aggregate page or memory budget.
- Virtual scanner: `K42_FAKE_SCANNER=dir` replaces Image Capture with a scanner that "scans" the images in `dir` (`glass*`
  for the flatbed in turn, `feed*` for the feeder; areas, resolution, colour, duplex, `K42_FAKE_WARMUP`, `K42_FAKE_SPEED`).
  UI checks: `K42_SNAPSHOT` with `K42_AUTOSCAN=flatbed|feeder|overview|photos`, `K42_DEMO_PAGES=n`, `K42_SHOW=history|settings|more|glass`,
  `K42_SOURCE=feeder` (+ `K42_DUPLEX`, `K42_REMOVE_BLANK`), `K42_PRESET=n`, `K42_SAVE_NOW` (+ `K42_SAVE_FORMAT`, `K42_SAVE_OCR`),
  `K42_SAVE_SHEET`, `K42_ACTIONS="select:2,rotateRight,moveUp,undo,…"`, `K42_CONTINUITY_TEST=file` (private pasteboard),
  `K42_PRINT_TEST`, `K42_DUMP_MENUS`, `K42_WELCOME`, `K42_SCAN_DIR` (saves and history go there), `K42_WATCHDOG=s`.
  `--selftest` exercises deterministic page, processing and output invariants. Native control scenarios and real Vision
  fixtures run through `python3 scripts/audit-ui.py --app SCN42`; independent saved-file checks run through
  `python3 scripts/audit-scans-cli.py --binary .build/debug/SCN42` (Pillow and pypdf required). Record the tested executable and results. A virtual scan, a
  successful model fixture or a generated Print-to-PDF file does not verify physical scanner drivers, duplex hardware,
  Continuity Camera, Share/Mail delivery or a physical printer. PDF/A-style output is not a certified conformance claim.
  Unsaved scans still have no document recovery autosave.

## Common

- Signature and stamp generator (in the Signatures and Stamps popover of PDF42, DOC42, IMG42, PPT42 and XLS42; SCN42 prints marks into the page pixels).
  **Create Signature…**: one or more names (one per line), a form — full name, initial and surname, surname,
  initials and a scribble, capital and a scribble, or any — and three styles out of 1320 (plan × flourish × capital variant × how
  the initial joins), New Variants (⌘R) for three more, and a pen: ballpoint, gel, fountain or felt-tip.
  Signatures are written by a model of a fast hand: hand-drawn centre lines for Latin, Cyrillic and Turkish letters
  with alternative forms of many lowercase letters, a smooth line through the letters, and the pen's speed along it
  from the sigma-lognormal model of rapid movements. Delayed strokes come after the word as real writers add them:
  i-dots, t-bars, the breve of й and ğ, the two dots (or one dash) of ё, ö, ü — some styles leave them out. Letters
  get smaller, flatter and simpler towards the end of a word; most styles climb 2–8°, about one in eight runs level.
  Between copies the letter shapes stay close while the layout varies (how far the line climbs, run-out and flourish
  length). A strike-through flourish crosses only the foot of the capital. Speed and direction set the ink: a dense
  ballpoint line, wider and darker on downstrokes, a full-width touch-down and a tapering lift, rare short skips at
  the fastest parts, blobs where the pen is slow and fine striations; for a fountain pen, width follows the stroke
  angle; paper fibres under every pen.
  **My Signature…** learns your own signature: sign in the box a few times (trackpad or mouse, recorded with time and
  pressure), open a scan or photo, or take a sketch or photo from an iPhone or iPad (Continuity). New copies combine
  compatible samples with small variations in movement and ink. Pad input supplies stroke timing, and copies are
  re-drawn from it by the pen model. Copies of a signature learned from pictures are made from the ink of the samples
  themselves: one sample (larger, sharper ones more often) is bent the way a hand varies — slant, width, a slight
  turn, a smooth bend — inked again in the chosen colour with the pen's texture, so tips, loops and dense crossings
  stay as written. The sample pictures are kept with the recipe in the local library (grey, at most 400 px tall). A
  copy cannot be sharper than its sample: a signature cut from a photo of a whole sheet looks soft when placed
  large; scans or close-up photos work best, and 5–10 different samples give the variety. A photo of a sheet
  signed many times is split into its signatures automatically (ink found against the local paper, only on the
  paper; fragments, two signatures written into each other and ones cut by the edge are left out), and each becomes
  a sample. Samples and the chosen pen are stored locally in the library recipe.
  Horizontal and nearly horizontal strokes retain their proportions and are scaled before rendering. Invalid or
  excessive geometry/timing is rejected before allocating stroke samples; a failed recipe does not produce an image.
  `PDF42 --signature-bench DIR [designs copies] [--check]` measures the generator with metrics from handwriting
  research — normalized jerk, velocity peaks, hesitation (near-stops), line wobble, DTW distance between copies and
  between styles, and whether Vision reads legible forms — against fluent, laboured and shaky reference trajectories;
  `--check` exits 1 when timing, wobble or variability leave the recorded thresholds.
  `PDF42 --signature-blindtest REAL_DIR OUT_DIR [copies]` prepares a blind test for people: each real signature
  picture in REAL_DIR (your own, or with the owner's consent) is learned and generated `copies` times, and real and
  generated are shuffled into numbered cells of `sheet.pdf` with the same grey ink, size and darkness; `key.json`
  keeps the answers. `PDF42 --signature-blindtest-score key.json answers.txt` scores the numbers a tester marked as
  generated: accuracy, hits, false alarms and d′ (0 = no measured separation in this test). Results do not establish realism for people generally.
  `PDF42 --signature-fidelity INPUT… OUT [--check]` measures learning on a person's sample sheet(s): every third
  signature is held out, the rest are learned, and copies are compared with the held-out real ones after the same
  tracing at the same ink size — shape ratio (copies vs real distance to unseen signatures), variation ratio, outliers
  unlike any real signature, and proportions, slant, line width, path length, strokes and sharp turns in units of the
  real spread (wobble is reported only: on photos it is dominated by the camera). It writes the split signatures,
  the copies, a blind-test sheet and `report.md`. `PDF42 --signature-fidelity-synthetic OUT` runs the same on a
  generated sheet with traps (no real person's data) and is part of `audit-ui.py --mode tests --app PDF42`.
  **Create Stamp…**: 132 stamp and seal styles in 15 categories (Russia and CIS, Europe, UK and Ireland, Turkey,
  USA and Canada, Hong Kong and Taiwan — including the company chop with a signature line and the small round
  chop —, mainland China, Japan, Korea, India, Middle East, Latin America, office stamps, date and registration
  stamps, personal stamps); every text is editable, an empty date means today. Generic designs only: no state
  emblems and no notary, court, customs, police or prescription seals. Ink: blue, violet, black, red, green or any
  colour. **Add to Library…** also imports your own stamp (SVG, PDF, PNG, JPEG, HEIC, TIFF) either as a stamp or as a
  fixed picture.
  Generated and imported stamps are saved as a recipe, so **every insertion is a new impression**: the same
  signature or stamp with different pen pressure and ink shade, slightly different size, slant and angle (signatures
  ±2°; round stamps 2–8°, rectangular 0.3–2°), uneven pad pressure, grain, skips and sometimes a faint double
  strike. `PDF42 --mark-samples DIR` writes a sample sheet of every form, style and an imported SVG.
- Interface language is chosen from 20 native language names on the welcome screen and shared by all apps.
  Changing it restarts the current app; already running sister apps pick up the choice at their next launch.
  Translations are bundled and work offline. System panels (Print, Save, toolbar items) use the chosen language
  where macOS provides it; Quick Look follows the system language. Arabic text is supported, while document
  canvases and spreadsheet geometry retain their existing layout. Interface language does not add OCR languages,
  translate document content or change the accepted formula syntax.
- Any panel can be hidden: View › Hide All Panels (⇧⌘\\) leaves only the document; View › Show Toolbar (⌥⌘T),
  Show Toolbar Labels (icons only makes the toolbar half as tall) and each app's own bars. Default toolbars carry
  frequent actions only; View › Customize Toolbar offers the rest (for example DOC42's Footnote, Track Changes, Zoom,
  Print and Export as PDF, SCN42's Overview, Delete and Enhance). In full screen (⌃⌘F) the
  bars slide in only when the pointer reaches the top edge.
- PDF42, DOC42, XLS42 and PPT42 use AppKit autosave with a 30-second delay to a backup location. This does not save
  every keystroke immediately. IMG42 edits and unsaved SCN42 scans remain in memory until saved.
- File › Open Recent refreshes from the app's document history when opened; Clear Menu is disabled for an empty list.
  IMG42 routes opening through its image controller. SCN42 opens recent saved scans in the system's default app;
  File › Open imports pages into a scan document.
- Dark mode everywhere; `K42_APPEARANCE=dark` (or `light`) forces it for one run, e.g. with `K42_SNAPSHOT`.
- `scripts/check_strings.py` — checks all 20 languages for missing strings, duplicate/empty catalog entries,
  unsafe format substitutions, changed shortcuts and broken template/formula/command examples.
- Distribution to other Macs: `DEVELOPER_ID="Developer ID Application: …" ./scripts/sign_and_notarize.sh PDF42 DOC42 XLS42 IMG42 PPT42 SCN42`
  (needs an Apple Developer account and stored notarization credentials; see the script header).
  Pass the app names explicitly: the script's current no-argument fallback forms one combined name and is not usable.

## Known limitations

- PDF42 signatures: signing and LTV need a file with a classic cross-reference table (any file saved by PDF42; files
  with cross-reference streams are re-saved before the first signature, which is impossible once they are signed).
  The PAdES level is reported from the file's structure (attributes, timestamps, DSS order, revocation coverage); it is
  not an eIDAS qualification check: PDF42 does not consult the EU trusted lists, and its trust decision is the macOS
  trust store. CSC remote signing has been tested only against a local mock service, not a real QTSP. RSA-PSS keys,
  OCSP nonces and asynchronous (multi-signature) CSC operations are not supported. Network calls block the window
  while they run (the OAuth wait keeps the run loop alive).
- GOST signatures need the user's own CryptoPro CSP. The GOST path has been exercised only against stand-in tools
  (`scripts/fake-cryptopro`), not against a real CSP install, so `cryptcp` option compatibility (`-cadesbes`,
  `-cadest`) and interoperability with CryptoPro PDF/Acrobat are not yet confirmed.
  МЧД revocation is not checked online, and the МЧД XML is not validated against the FNS schema.
- Tracked formatting changes in DOC42, saving to legacy .xls, Excel 5.0/95 files.
- XLS42 implements a subset of Excel/LibreOffice behavior; the function list and passing fixtures do not establish
  full format or calculation compatibility. Richer pivot tables refresh in its simpler one-pivot-per-sheet layout.
  CSV retains one sheet's computed values and automatically detects numeric/date fields on import, so it cannot
  preserve workbook types, formulas or formatting.
- XLS42's 100,000-cell edit limit applies per checked operation, not per workbook. Formula operations requiring a
  dense matrix are limited to 2,000,000 cells and return `#NUM!` when that allocation would be exceeded; unsupported
  spill size/placement returns `#SPILL!`. Sparse aggregates can inspect larger references without allocating blank
  positions. Bounded statistical/financial loops reject workloads over 1,000,000 iterations with `#NUM!`.
  ODS import rejects invalid repeat/span dimensions and expansion beyond 1,000,000 materialized cells across a
  workbook, instead of silently clipping repeated content. Large empty repeated regions remain sparse.
  CSV export rejects a rectangle larger than 2,000,000 positions from A1 to the last used cell, including intervening
  blanks; a single far-away cell can therefore exceed the export limit even in a sparse workbook.
- IMG42 and SCN42 have no document recovery autosave. Image codec support depends on the Mac; pixel edits to animated,
  multi-page or read-only inputs export the displayed frame/page through Save As. AVIF export is offered only when supported.
- PDF/A export has not been checked with a conformance validator in this audit; see each app's format notes above.
- The audit's scanner tests use a virtual source. Physical scanners, Continuity Camera and the minimum supported macOS
  version were not validated in this run. The regression scenarios below state their individual coverage.

## Regression checks

These checks cover the macOS implementation. Run them from the repository root after installing the Xcode Command
Line Tools and Python 3.
The document CLI check also needs Poppler's `pdfinfo` and `pdftotext` on PATH. The spreadsheet CLI check uses them for
PDF checks and records an explicit SKIP when either is missing. The checks use generated fixtures,
do not need a physical scanner or XCTest, and do not open interactive windows:

```bash
swift build
.build/debug/PDF42 --selftest
.build/debug/XLS42 --selftest
.build/debug/PPT42 --selftest
.build/debug/IMG42 --selftest
.build/debug/SCN42 --selftest
python3 scripts/audit-media-previews.py
python3 scripts/audit-documents-video.py
python3 scripts/audit-documents-cli.py
python3 scripts/audit-pdf-cli.py --binary .build/debug/PDF42
python3 scripts/audit-signing-cli.py --binary .build/debug/PDF42
python3 scripts/audit-spreadsheets-cli.py --binary .build/debug/XLS42
python3 scripts/audit-presentations-cli.py --binary .build/debug/PPT42
python3 scripts/audit-images-cli.py --binary .build/debug/IMG42
python3 scripts/audit-scans-cli.py --binary .build/debug/SCN42
swiftc -module-cache-path .build/ModuleCache Sources/Kit42/Zip.swift Sources/Kit42/TextDecoding.swift \
  scripts/audit-documents-regressions.swift -o .build/audit-documents-regressions
.build/audit-documents-regressions
python3 scripts/check_strings.py
python3 scripts/audit-gost-cli.py
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
.build/debug/localization42audit
```

`audit-gost-cli.py` runs PDF42's GOST commands against `scripts/fake-cryptopro`, OpenSSL (RSA) stand-ins for
`cryptcp`/`certmgr`/`csptest` that need Homebrew `openssl@3`. It covers process plumbing, PDF embedding,
verdicts and МЧД matching, but not GOST cryptography. With a real CSP and a certificate from the CryptoPro test CA,
run `python3 scripts/audit-gost-cli.py --real --thumbprint SHA1 [--pin PIN] --snils <SNILS in the certificate>`.

The PDF CLI runner requires `pypdf` and uses `openssl` for its default isolated signature checks. It writes fixtures,
command logs, the tested binary's SHA-256 and `result.json` to `build/pdf-cli-audit`; use `--output-dir` to change the
location or `--binary build/PDF42.app/Contents/MacOS/PDF42` to test a packaged build. `--skip-signing` records an explicit
SKIP. Signing fixtures create temporary local keys/certificates and remove their private keys afterwards; they do not
modify the Keychain or contact a timestamp/network service. They are not a certificate-trust or full PDF/A conformance test.

These cover malformed PDF/ZIP input, attachment extraction, spreadsheet references and numeric precision,
text encodings, CSV/ODS previews, scan save state, and preservation of existing files when exports fail.
The document CLI check verifies DOCX round trips and PDF text. It accepts a positional executable path, for example
`python3 scripts/audit-documents-cli.py build/DOC42.app/Contents/MacOS/DOC42`. The spreadsheet CLI check writes its fixtures,
converted files, PDFs, command log and `result.json` to `build/xls-cli-audit` (`--output-dir` changes that path).
It accepts `--binary build/XLS42.app/Contents/MacOS/XLS42` for testing a packaged bundle and `--skip-pdf` for an
explicitly limited run. The presentation CLI runner writes corresponding artifacts and `result.json` to
`build/ppt-cli-audit`; it accepts `--binary build/PPT42.app/Contents/MacOS/PPT42`, `--output-dir`, `--skip-pdf` and
`--skip-movie`. PDF checks require Poppler and report explicit SKIP results when unavailable; an explicitly skipped
movie check also remains visible in the report. Movie checks exercise successful creation and replacement.
The SCN42 CLI runner requires Pillow and pypdf. It writes generated input/output files to `build/scn-cli-audit`;
`result.json` records command results and the tested executable's SHA-256. Use `--output-dir` to choose an isolated directory
and `--binary build/SCN42.app/Contents/MacOS/SCN42` to test a packaged build. It checks output geometry, compression,
multi-page files, EXIF orientation, B&W encoding, failures and actual OCR/TXT/RTF; successful virtual acquisition
is covered by the native scanner scenario, and physical scanner hardware is a separate check.

PDF42's matrix lists the scope of its regression scenarios. It does not assert that all scenarios pass on an arbitrary
binary; each run must record its observed results, tested executable and unexercised areas.

| Common PDF workflow | Regression coverage | Boundary of the check |
|---|---|---|
| Use toolbar and navigation | Tool selection, search, page navigation, view modes, zoom and responder routing | Native controls/events and generated fixtures; no exhaustive physical shortcut/device coverage |
| Mark up and edit text | Shape/ink/text/form interactions, drag cancellation, colour/width, stacking, Undo/Redo and pending edits | Representative native gestures and annotation state; saved output checked separately |
| Rearrange and format pages | Multi-page move/rotate/delete/insert, crop/size dialogs, watermarks/numbers/headers, destinations and metadata | Undo/Redo plus emitted text/destinations; not every external PDF's appearance stream |
| Fill and save forms | Text, checkbox/dropdown creation, filled/empty values, AcroForm persistence | Supported AcroForm widgets; no XFA or form-script compatibility claim |
| Save, Revert and process | Pending draft, successful/failed Revert, annotation stack, compression/redaction model, OCR result freshness | Native lifecycle and synthetic PDFs; scanner hardware is not involved |
| Export and combine | TXT/RTF/HTML/DOCX, images, crop/scale, split/merge, metadata, links, attachments and failure preservation | Independent CLI output inspection and native snapshot checks |
| Redact sensitive content | Source text removal, moved edited text, covered overlays, bounds and failure without a fallback | Chosen content/annotation fixtures, not exhaustive malformed-PDF or third-party conformance verification |
| Sign digitally | Generated temporary identity, visible/incremental signing and tamper cases | Local self-signed fixture; Keychain import, trusted PKI, real timestamp services and external-viewer trust remain separate |

XLS42's regression scenarios are listed below as coverage, not as a claim that every scenario has passed on the
current binary. Preserve actual run results and binary identities; none establishes complete Excel compatibility.

| Common workflow | Regression coverage | Boundary of the check |
|---|---|---|
| Enter and navigate cells | Native editing: selection traversal, validation rejection/correction, pending draft, Undo/Redo | Native actions/event dispatch, not physical keyboard or IME testing |
| Copy, paste and fill | Internal/external quoted text, Paste Special, Skip blanks, protected cells, partial/full legacy arrays, fill directions and date/growth series | Listed operations and fixtures; rejected writes must leave cells/undo history unchanged |
| Format a selected table | Native Format Cells controls, mixed borders, None/Outline/All, colours, wrap, Autofit, Undo/Redo and border file round trips | Representative controls/fixtures; screenshots alone do not prove behavior |
| Save and reopen | Pending edits, validation failure, successful/failed Revert; XLSX→ODS→XLSX values, empty input styles, borders, merges, names, panes and visibility | Generated fixtures; unsupported third-party workbook features are not certified |
| Exchange CSV/TSV | Unicode, delimiter/quote escaping, embedded CR/LF, equals-prefixed literal text, displayed 1904 dates | CSV types and workbook formatting are inherently unavailable |
| Recalculate ordinary formulas | Existing function cases plus bounds, sparse aggregates, blank range tails, named ranges, reference changes, error propagation and spill collisions | Formula subset; historical LibreOffice comparisons are separate and optional |
| Use dates and number formats | 1900/1904 entry, functions, file round trips, Find/Series, CSV/PDF, scientific and elapsed-time formats | Targeted dates/formats, not every locale or custom format code |
| Rename and restructure sheets | Native rename/reference cases plus existing sheet, row/column and data-tool tests | Supported metadata and formulas; not every external-link format |
| Print or export PDF | Current sheet/selection/visible workbook, print area, landscape, wide fit, oversized frozen titles and extracted PDF text | Native layout plus CLI PDFs; no physical printer validation |
| Open malformed or oversized data | Invalid XML indices/repeats/spans, worksheet limits, dense allocation and expensive numeric arguments | Explicit fixtures and resource limits, not an exhaustive fuzzing campaign |

PPT42's scenarios define the scope of its regression suites. Run results, binary identity and installation state are
recorded with each tested build; a passing fixture does not establish a usage percentage or full PowerPoint/Impress
compatibility.

| Common presentation workflow | Regression coverage | Boundary of the check |
|---|---|---|
| Move, resize and arrange objects | Native canvas mouse events, selection, snapping/rotation, stack order, align/distribute, Group/Ungroup and Undo/Redo | Generated objects and native event dispatch; not physical device testing |
| Format text and shapes | Selected text Undo/Redo, empty paragraphs, nested-group formatting, inspector geometry/fill/line controls, master appearance and rendered border/fill pixels | Representative controls; unsupported table dash/autofit controls stay disabled |
| Find and replace | Every occurrence, cross-run text, table cells, notes, dynamic fields, formatting preservation and Undo/Redo | Supported text model; no guarantee for unsupported imported document parts |
| Edit tables | Cell alignment, insert/delete through merged rows/columns, spans, retained anchor text, Undo/Redo and formatting round trips | Supported rectangular table model; no full PowerPoint table-style parity claim |
| Copy between presentations | Object/slide paste, media filename collisions, pending text, Undo/Redo and restored selection | Generated image/media fixtures, not every embedded object format |
| Save and reopen | In-place/outline drafts, notes Undo after slide changes, successful/failed Revert, PPTX↔ODP and POTX/OTP templates, notes/text whitespace, visibility and styles | Generated files; external Office/LibreOffice interoperability is a separate check |
| Export and print | Visible/hidden PDF scope, complete multi-page notes, image dimensions, output preservation on failure, MP4/MOV containers | Exported artifacts and PDF text; no physical printer or embedded-media movie playback testing |
| Present slides | First automatic build, click builds, hidden-slide navigation, black screen and Previous/End | Native show controller; multiple physical displays, external media and timing under load remain separate checks |
| Reject oversized or malformed input | Bitmap dimensions, ODP repeats/spans, empty/corrupt structures and explicit failures | Resource limits and chosen fixtures, not exhaustive fuzzing |

These checks still use macOS frameworks: restrictive execution sandboxes can block AppKit or the video encoder even
without visible windows. Run them in a normal logged-in macOS session when those services are required.

Native interface checks require a logged-in macOS desktop and access to WindowServer:

```bash
swift build
python3 scripts/audit-ui.py
# Optional: only the behavioral checks, or screenshots for one app.
python3 scripts/audit-ui.py --mode tests
python3 scripts/audit-ui.py --mode tests --app IMG42
python3 scripts/audit-ui.py --app IMG42 --binary build/IMG42.app/Contents/MacOS/IMG42
python3 scripts/audit-ui.py --mode tests --app SCN42
python3 scripts/audit-ui.py --app SCN42 --binary build/SCN42.app/Contents/MacOS/SCN42
python3 scripts/audit-ui.py --app DOC42
python3 scripts/audit-ui.py --app XLS42
python3 scripts/audit-ui.py --app PPT42
python3 scripts/audit-ui.py --app PPT42 --binary build/PPT42.app/Contents/MacOS/PPT42
python3 scripts/audit-ui.py --app PDF42
python3 scripts/audit-ui.py --app PDF42 --binary build/PDF42.app/Contents/MacOS/PDF42
python3 scripts/audit-ui.py --mode tests --app PDF42 --case pdf-markup --case pdf-files
python3 scripts/audit-ui.py --mode screenshots --app PDF42
python3 scripts/audit-ui.py --app DOC42 --output build/doc-check # separate evidence directory
python3 scripts/audit-localization.py --app DOC42 # native picker/menu/label-fit checks and 20 welcome captures
python3 scripts/audit-localization.py --app PDF42 --language ar --language zh-Hant --appearance dark
```

The runner creates separate test app bundles in `build/ui-audit/apps`. Separate bundle identifiers isolate
app-specific preferences; the suite-wide interface language remains shared. Test windows may activate the app.
The localization runner uses process-only `-Suite42Language <code>` overrides and writes native-check reports and
screenshots to `build/localization-audit`; it does not change the saved suite language. It checks welcome screens,
picker contents, menu translation and label sizing, not document editing or every translated dialog.
PDF42 window checks launch through macOS LaunchServices so native field editors and menu routing have an active window.
`--case` can be repeated to select behavioral scenarios with `--mode tests`; a subset run does not replace the full audit.
Generator-popover captures (`mark-signature`, `mark-seal`, `mark-learn`) belong to `--mode screenshots`, not behavioral
checks. Reports label behavioral and screenshot runs separately. SCN42 mark placement runs once in its dedicated
`scn-marks` scenario; it is not repeated by the legacy lifecycle smoke test.
Logs and screenshots go to `build/ui-audit`. It checks recent-file menus,
spreadsheet selection and formatting, document panels, note replacement, image resizing/DPI and undo,
and scanner page ordering, rescan and close/save behavior. Scanner checks use generated images.
SCN42 adds settings/presets, delayed source preparation, DPI/duplex preservation, Scan/Overview/Stop callbacks,
page/selection gestures, complete Undo/Redo and saved-snapshot lifecycle. Copy exercises controlled acquisition,
processing and print-to-file parameters. A separate real Vision scenario checks OCR, four source orientations,
document crop and photo separation. These checks do not establish physical scanner, Continuity Camera, Mail/Share
delivery or printer interoperability; pair them with `SCN42 --selftest` and `audit-scans-cli.py`.
XLS42 adds editing/protection/array checks, native Format Cells controls, row Autofit Undo/Redo, pending-edit and
Revert lifecycle, sheet-reference changes, sparse conditional formatting and actual PDF scope/layout assertions.
PDF42 adds native markup, page operations and dialogs, toolbar/navigation, pending drafts and Revert, forms, OCR
snapshot handling, output checks and regression scenarios for document mutations and redaction.
IMG42 adds native drawing/selection/style controls, pixel edits and colour/size controls, navigation/search/panel state,
Save/Save As/Export/Revert lifecycle, Trash/Undo/Redo, export format/quality/size controls and pending text in Copy/Share/Print.
Additional scenarios exercise real full-screen Spaces and slideshow transitions, VisionKit mouse text selection and
Escape, and real Vision background removal and Smart Lasso masks on a generated subject. Vision errors fail their
scenario; the deterministic `--selftest` remains independent of model availability. File checks use disposable images,
a private native pasteboard and Print's save-to-PDF disposition; no job is sent to a physical printer. Run
`IMG42 --selftest` and the independent image CLI suite alongside these native scenarios; observed results and limitations
must accompany each tested build.
PPT42 adds canvas manipulation, media-safe copy/paste, text/Find/table commands, document lifecycle, inspector
controls with rendered fill/border checks, and slideshow navigation/builds.
DOC42 checks dispatch native mouse events for ruler drags at 25–200% zoom, exercise section/column/header targeting,
and check repeated Undo/Redo, formatting, Find/Replace, tables, comments, picture resize, page geometry, pending header
edits, Revert, format round trips and PDF page ranges. Complex documents are checked for complete text/glyph coverage
after changes to view modes, columns and font size; export isolation and dynamic field widths are also exercised.
Fixtures use disposable documents. These checks exercise native
actions and event dispatch; they do not replace testing physical input devices or every third-party document.
Screenshots cover all six apps in light and dark appearances at different window sizes.
DOC42 additionally captures book, columns and web layouts.
The snapshots render AppKit views; native material layers may require checking the live window as well.

## Structure

- `rust/` — the separate native Windows workspace; see [its README](rust/README.md) for crates, build instructions and limits.
- `Sources/Kit42` — shared macOS components: commands, menu bar, toolbar, tool strip, banners, welcome window, localisation,
  signature library, certificates, ZIP.
- `Sources/Previews42` — Quick Look rendering (Markdown → HTML/page, ODS/TSV → table); `Sources/QLPreview42` and
  `Sources/QLThumbnail42` are the extension binaries that `build.sh` packs into `.appex` bundles
  (Info.plist generated, ad-hoc signed with `Resources/QuickLook/Extension.entitlements`); `ql42render` renders the
  same output to files for testing (`ql42render html file.md out.html`, `ql42render thumb file.ods 512 out.png`).
- `Sources/PDF42`, `Sources/XLS42`, `Sources/DOC42`, `Sources/IMG42`, `Sources/PPT42`, `Sources/SCN42` — the macOS apps. Each has `Commands.swift` (all commands, menus, toolbar)
  and `Strings.swift` (translations).
- `DESIGN.md` — the design system: menu order, reserved shortcuts, window layout, how to add an app.

## License

MIT — see [LICENSE](LICENSE).
