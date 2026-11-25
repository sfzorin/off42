# off42 — design system

off42 has native macOS and Windows implementations of PDF42, DOC42, XLS42, IMG42, PPT42 and SCN42.
The Swift/macOS apps share **Kit42** (`Sources/Kit42`); the Rust/Windows apps share native Windows infrastructure.
The Kit42, AppKit and Swift contracts below describe macOS; the Windows workspace has its own implementation
and feature scope. Shared product names do not imply identical capabilities.
This describes the source as of 2026-11-06. [README.md](README.md#regression-checks) documents verification commands
and scope; detailed build logs, executable identities and installation history are retained with each local audit.

## Principles

- **Native on each platform.** macOS uses AppKit, system fonts, SF Symbols and system colours (light/dark).
  Windows uses Rust and native Windows controls. Core editing works offline; each implementation documents its
  own framework and runtime dependencies.
- **Quiet main toolbar.** Only frequent actions; tools for the current mode live in the tool strip.
- **One command — one definition.** A `Command` (title, SF Symbol, shortcut, selector) builds the menu item,
  toolbar button and strip button, keeping their labels and shortcuts in one place.
- **Undo follows the active editor.** Document changes use the owning document/controller's `UndoManager`;
  text views can have a separate native text undo history (for example PPT42 notes). Commit pending field/text edits
  before saving or switching context. Not every background operation is undoable; SCN42 OCR is one exception.
- **Use banners for ongoing status.** Signatures, read-only and compatibility information use `BannerView`;
  confirmations and errors can use native alerts or sheets.
- **English text is British** (Colour, Grey, Centre). Title case for menu items, buttons, toolbar labels and undo
  names; sentence case for tooltips, field labels and checkboxes. One command keeps one label in every app.
- **Twenty interface languages, English by default.** The picker follows emshandbook.com's language set and order:
  `en`, `zh-Hans`, `zh-Hant`, `es`, `ar`, `fr`, `pt`, `id`, `ru`, `de`, `ja`, `vi`, `tr`, `fil`, `ko`, `th`,
  `it`, `pl`, `ms`, `hu`. Labels use native language names. Explicit Chinese script tags take precedence over regions;
  `zh-TW/HK/MO` map to Traditional, `zh-CN/SG` to Simplified, and `tl` maps to Filipino.
  Every string in code is English inside `L("…")`. Existing `[en, ru, tr]` tables remain in
  `Kit42/Strings.swift` and `<App>/Strings.swift`, registered in `main.swift`; the other 17 languages use shared
  English-keyed JSON catalogs in `Kit42/Resources/Translations`. Registering legacy rows merges translations
  without discarding other languages. Missing or empty translations fall back to the English key.
  The language is picked on the welcome screen and is shared by all six apps (`com.sfzorin.suite42` › `language`);
  switching restarts the app. `-Suite42Language <code>` overrides only the current process for audits.
  `AppleLanguages` is set in the volatile argument domain before AppKit loads its strings. Arabic labels use native
  Unicode text rendering; document geometry is not globally mirrored. An ellipsis "…" marks commands that open a dialog.
  `Suite42_Kit42.bundle` is copied into each app and Quick Look extension's `Contents/Resources` before signing;
  runtime lookup checks that location before SwiftPM's development bundle. `check_strings.py` checks all catalog
  keys, nonempty values and printf argument identity/type/width/precision, including positional reordering;
  document template fields, shortcut symbols and executable command/formula syntax must also remain intact.
  The formula audit accepts XLS42's existing Russian SUM alias and localized sheet names in reference examples.
  Welcome action buttons share the width required by their longest translated title; the identity pane and window
  grow to fit that width and the translated tagline. `audit-localization.py` launches isolated copies and checks the
  native picker, current language, translated File menu, catalog loading and visible label/action widths before
  capturing each welcome window. It does not establish editing behavior or full dialog coverage.
  Interface language does not translate document content, add OCR languages or alter the accepted formula syntax.

## Rust Windows workspace

The separate Windows apps use `windows-rs` and shared `win42` native windows, command registry, menus, toolbars,
dialogs and controls. App models own history and document state; shared save snapshots detect conflicts and use atomic
replacement. Multi-file saves stage sidecars first with verified reuse and best-effort rollback, not OS-atomic
replacement of the whole bundle. Async open retains the existing document/session through verification, cancellation
and progress. The UI provides canvas drawing, rich text and scaled images, asynchronous jobs with generation-based
cancellation, and printing using twips and DPI. DOC42 separates its editable RichEdit model from the read-only
paginated preview. PDF password retries use the captured source bytes; async open rechecks identity before replacing the prior session.
DOC42 reserves note space in preview/print and explicitly refreshes linked TOC entries, invalidating cached page numbers.
XLS42 maps visible grid positions to physical cells for filtered, hidden and frozen areas. Typed clipboard preserves
source cells and styles, translates formulas per destination and validates the whole sparse paste as one history step.
Its registered payload is bound to the published Unicode text; invalid or mismatched payloads fall back to text.
SCN42 shares immutable pixel/OCR data across history, bounds retained snapshots and OCR allocations, and commits a
successful batch OCR as one undo step. Scene transforms rotate objects
and groups; where a printer lacks `SB_PIXEL_ALPHA`, scene printing uses a bounded white raster fallback at up to
200 DPI. Common controls v6 and PerMonitorV2 are declared in the application manifest.

These are implementation contracts, not evidence of native event dispatch, rendering, printing or lifecycle behavior.
See [rust/README.md](rust/README.md) for formats, dependencies and verification limits.

## Window layout (macOS)

Typical PDF42 layout; other apps add their own formula bar, notes, inspectors or scan settings:

```
┌───────────────┬─────────────────────────────────────────────────────────────┐
│ ● ● ●  [▤]    │ Name.ext            [− +] [↺] [✎] [✍] [✓]    [ Поиск       ] │  toolbar (unified)
│               │ subtitle: status                                            │
│               ├─────────────────────────────────────────────────────────────┤
│ sidebar       │          tool strip (ToolStrip, toggle ⇧⌘A in PDF42)         │  titlebar accessory
│ (NSSplitView  ├─────────────────────────────────────────────────────────────┤
│  sidebar)     │ ⚠ banner (BannerView, when there is something to say)       │
│               │                                                             │
│               │                 document                                    │
└───────────────┴─────────────────────────────────────────────────────────────┘
```

- App windows use native toolbar/titlebar controls. Split-view sidebars use `NSSplitViewItem(sidebarWithViewController:)`;
  not every app has the same split-view structure.
- Toolbar order: `toggleSidebar · sidebarTrackingSeparator · flexibleSpace · view group · document actions · mode toggle · search`.
- Default toolbars hold only frequent actions. A command that already has a home elsewhere (zoom when the window has
  a zoom slider, print/export in File, page tools in a menu) goes to `ToolbarController(optional:)`: it stays in
  Customize Toolbar but is not placed by default. Never put the same action twice in one window (toolbar and inspector).
- Subtitle: position and state only ("Стр. 3 из 12", "Лист 2 · A1:C4"). Counts (words, characters) belong in the
  status bar; the two never repeat each other.
- Status bar: only where the document has counts or a view-layout/zoom control that the toolbar does not carry
  (DOC42, XLS42; SCN42 shows device state). PDF42, PPT42 and IMG42 have none by design.
- Apps can show `WelcomeWindowController` (icon, actions, recent files) when no document is open;
  SCN42 opens a new scan window by default.
- `Panels.toggleAll` saves the current visibility per window and restores it. App-specific panels must do the same;
  temporary hiding must not overwrite preferences for later windows (DOC42 ruler/comments use non-persisting setters).
- At narrow widths, `ToolStrip` moves groups into its More menu; native toolbars use their overflow menu.
- DOC42 has a 30 pt horizontal ruler above the document viewport and a 30 pt vertical ruler to its left. One View
  command shows/hides both. Horizontal page-margin grips occupy the middle band, separately from paragraph-indent triangles;
  native down/drag/up events preview the value and commit one undoable change on release. Geometry tracks scroll,
  zoom, sections and the active column/header. Web layout disables print-margin grips. Test actions as well as
  appearance: a visible ruler is not evidence that its controls work.

## PDF42 editing and output contracts

- Pending in-place text participates in dirty state. Save/export commits the visible draft before taking a snapshot;
  unchanged edits do not create Undo. Revert parses first: failed reads preserve the live model/draft; successful reads
  cancel interactions and restore document metadata and views with stale Undo cleared.
- Save and export render a separate snapshot. They preserve live annotation order, rotation and editability and fail
  if redaction/rendering cannot complete. They must not substitute the original unredacted document on failure.
  Apply Redactions and Compress This Document replace the same model that Save uses, with complete Undo/Redo.
- Markup selection drives the colour/line-width controls. Width targets rectangle/ellipse/ink/widget borders; it is
  disabled for text boxes, stamps, text markup and page overlays. Form tools select widgets; other markup cannot be moved by
  mistake through a form tool. Escape or a tool switch rolls back an unfinished gesture. Undo after deletion restores
  the annotation's stacking position. Edited text retains its source cover even when moved.
- Page changes preserve page identity, current page and thumbnail selection. Multi-selection commands operate on all
  selected pages. Destinations, attachment lookup and per-link magnification metadata follow reordered/replaced pages;
  Undo restores these together. Crop/size/numbering controls validate before mutation and reject nonfinite/overflowing input.
- Page-size scaling transforms annotation bounds, fonts and local drawing geometry. Pending page-content overlays are
  safely baked into a resized page while Undo retains the original editable annotations. Custom annotation drawing
  classes must be copied explicitly when retained; generic PDFAnnotation copying alone does not preserve their appearance.
- Initial View metadata and the app's opening behavior are separate. PDF42 applies supported layout/sidebar/page and
  magnification settings; window/title/tool/menu-bar and full-screen preferences remain serialized metadata for other viewers.
- OCR operates on captured input and applies only to the document state that requested it. It must not mutate source
  annotation/rotation state while rendering or overwrite intervening edits with an obsolete result.
- Outputs validate allocation bounds and replace each destination only after successful generation. Multi-file split/
  batch output is not an atomic group. Missing merge inputs are errors; a no-reduction compression still writes the
  requested unchanged copy. Unsupported formats and invalid ranges must not produce a success-shaped no-op.

These contracts describe the current source. README lists scenario boundaries; each audit records the actual checked
binary, failures/skips, installation and publication state. Native UI assertions complement saved-file inspection.

## XLS42 editing and output contracts

- Commit the visible cell/formula draft before Save, Print, export, Find or a sheet/context change. The commit result
  is part of the command contract: a validation failure retains the text, focus and selection and stops the command.
  Pending text participates in the document's edited state. Successful Revert discards the draft and restores a valid
  sheet/selection; a failed read must leave both draft and workbook intact.
- Cell-content mutations preflight the actual destination, including Skip blanks and stopped fill series. Reject
  writes crossing protection, worksheet bounds, the 100,000-cell operation limit or part of a legacy array before
  changing cells or undo history. A complete array replacement removes its metadata in the same undo transaction;
  format-only paste can change a subset without changing array formulas. Large sparse clears visit stored cells.
- Format Cells represents mixed border selections explicitly. Opening and accepting the dialog without changing a
  border preserves every edge, including imported line styles. None, Outline and All are explicit actions over the
  complete selection; Outline clears inner lines. Changing a border style/colour changes existing lines without
  inventing new ones. Native font/background colour wells turn off Automatic/No fill when a colour is selected.
  Number-format controls and their preview stay synchronized with typed values and category changes.
- Row Autofit is a document mutation with Undo/Redo, including shrinking a previously tall row. Formatting controls
  and preview rendering are separate from applying the final selection-wide change.
- Date serials retain the workbook's 1900/1904 epoch. Rendering, editing, Find, fill series and CSV/PDF output receive
  that context; import/export must not silently shift raw values. Text typed as a forced string remains a string after
  save/reopen even when it resembles a formula or date.
- Print and PDF use the same layout and explicit scope. Workbook scope contains visible sheets; selection scope
  contains the selection. Fit to width uses the required scale even for very wide tables. Frozen title repetition is
  disabled when it would consume at least 80% of the printable body. PDF export does not substitute a screenshot for
  complete sheet layout or mutate the user's print settings.

These describe the source implementation. Keep scenario coverage in README and
dated binary/test/installation evidence with each audit; native action tests and PDFs establish more than panel snapshots.

## PPT42 editing and output contracts

- Visible in-place and outline text drafts participate in document dirty state. Commit in-place text before object
  commands; Save/export also flushes pending outline typing synchronously. Successful Revert discards drafts and delayed
  outline updates only after the replacement file has parsed. Failed reads retain the original model and draft. Opening
  and leaving unchanged text must not create a document Undo step.
- Text editing owns a text UndoManager; format changes while editing separate typing bursts. Object/document actions
  restore model and selection together. A command that commits a pending draft and changes an object can form one
  native undo group; regressions must check the complete before/after state of that group. Notes editors clear their
  local Undo when the slide/content changes; Revert clears local notes/outline histories so they cannot edit stale content.
- Character/paragraph commands visit visible text containers in nested groups and table cells. Vertical alignment
  targets the edited table cell when one is active, otherwise all selected text containers. Empty-paragraph properties
  are data and must survive commit. The inspector reports effective styles from the same text-container traversal.
- Find navigates individual occurrences in objects, cells and notes. Replacement can span run boundaries, retains
  surrounding formatting, and uses the first matched character's properties for inserted text. Dynamic fields stay
  fields. Notes replacement uses the notes text view's Undo path; whole-deck replacement is a document transaction.
- Insert/delete table rows and columns transforms merged rectangles and their covered cells together. Removing an
  anchor from a surviving span retains its content at the surviving anchor. Fill/line inspector controls mutate the
  actual table style and grouped child shapes and reflect that effective style in the panel. Table No Fill is explicit
  transparency, not absent data that inherits the header theme. Unsupported table dash/autofit controls are disabled.
  Master/layout placeholder commits retain shadow, opacity and shape adjustment values as well as fill/line/text.
- Canvas transformations and Ungroup preserve the rendered child geometry, including reflections. Cross-document
  paste imports media under noncolliding names and remaps references in the same Undo transaction as inserted objects.
  Inspector numeric fields reject invalid/nonfinite geometry before a mutation, and controls re-enable when their
  mode/selection becomes eligible again.
- Notes PDF and printed notes paginate the complete text. The on-screen Notes Page is a first-page preview. PDF/image
  scope omits hidden slides unless included explicitly. Validate bitmap dimensions before allocating; export failures
  must preserve existing destination files where the exporter promises replacement safety.
- Show startup plays its first automatic animation group. Delayed animation callbacks are tied to the navigation that
  scheduled them; moving elsewhere must not reveal a build on the wrong slide.

These are local implementation contracts. Record exercised scenarios, limitations and the exact build's results in
README and the audit artifacts; passing model tests do not replace native actions, rendered-pixel assertions or saved-file checks.

## IMG42 editing and output contracts

- Pixel edits, markup, adjustments, image description and print resolution belong to one editing state. Undo/Redo
  restores the source pixels, markup, pixel selection, selected mark and effective inspector/markup controls together.
  A click, unchanged style or same-size resize must not create a phantom Undo entry. Native slider keyboard actions
  have the same Undo boundary as mouse drags; Reset and Look changes preserve the complete previous adjustment state.
- In-place text reads the active field editor. Only changed drafts participate in dirty state; Save/export/Print
  commits visible text. Escape discards the draft, and restoring/resetting another state cancels stale editors and
  unfinished gestures. Tool changes and Escape roll back unfinished mark movement, resizing and selection gestures.
- The displayed animated frame is the frame frozen for editing/output. Pixel edits to animated or multipage input
  produce a still image of the displayed frame/page; a successful output must not silently switch to the first frame.
  File navigation/loading clears obsolete editing sources so a new destination cannot receive a previous image's pixels.
  File-level Undo/Redo (lossless orientation and Trash) remains bound to its original URL across navigation; changing
  another file must preserve the current image. Exporting over an existing image invalidates its decoded cache and thumbnail.
- Show Original is a temporary view of the original bitmap at its original geometry, including after Crop/Resize.
  Releasing it restores the edited geometry. Delayed full-resolution adjustment renders must not overwrite that preview.
  Mutable Core Image filter instances are not shared between concurrent preview/full-resolution renders.
- Background removal, Smart Lasso, Instant Alpha and text recognition apply only to their captured image/request
  state. Revert, navigation, replacement, a newer request or intervening relevant edits invalidate older work. A stale
  asynchronous result must not clear a newer selection, overwrite edits or act on another file.
- Pixel masks match the image dimensions. Nonrectangular Crop retains transparency outside the selection; mask/render
  failure aborts output rather than returning an unmasked rectangle. Instant Alpha follows a connected colour component.
  Finite positive size/allocation checks precede bitmap creation; Smart Lasso also bounds contour resampling work.
- Resize unit switches preserve exact pixel dimensions. Proportional resize updates both dimensions; invalid dimensions
  stay invalid when units change. With Resample off, pixel dimensions are read-only and resolution changes print size.
  Mark geometry and image stamps follow pixel transforms; text annotations retain their established upright orientation.
- Output commits visible edits and checks adjustment/markup rendering. Failed rendering cannot save original pixels
  without the requested effects. Per-file destination replacement is atomic after successful encoding; a batch is not
  one atomic transaction. Do not mark the document clean or switch its file identity before a write succeeds.

These are source contracts; each audit records the exact tested binary, exercised scenarios and remaining limits.

## SCN42 editing and output contracts

- Pages have stable identities. Selection, insertion anchors, rescan targets and queued work refer to those identities,
  not rows that may have moved. Undo/Redo restores the complete page snapshot (base/output pixels, DPI, processing
  options/report and text) plus selection. Empty or unchanged operations do not register Undo. Dirty state compares
  complete current contents with the saved snapshot. Save completion installs the exact written snapshot as the new
  baseline, including when Undo ran during the write; Redo to that written content becomes clean again.
- Cleanup, straightening and OCR run against captured immutable pixels/options. Applying results requires the original
  page object and content revision to remain current. Pixel changes invalidate OCR; failed recognition is not cached
  as an empty page. Rendering/processing failures report an error without publishing an unprocessed fallback as success.
- Scanning, overview, source preparation, processing, OCR and saving share the controller's busy/availability state.
  Device/source changes, cancellation and window closure invalidate obsolete callbacks. Stop prevents additional scan
  callbacks from adding pages; pages already handed to processing may finish. A cancelled scan-and-print job must not
  send a partial job to the printer. Received-page processing has a separate generation from edits/OCR on existing
  pages, so starting an edit cannot discard an already acquired page. Close preserves pending work and closes after
  Save only when the document is clean.
- Scan areas are finite, bounded to the glass and nondegenerate. Source capabilities are read after the functional unit
  is selected; saved duplex/resolution settings survive the period before those capabilities are known. Bitmap/render
  scratch allocations are bounded to 32,768 pixels per side and 256,000,000 pixels; invalid
  numbers and geometry are rejected before integer conversion, allocation or Core Image operations. B&W thresholding
  preserves the dark Otsu class, including an already black/white original; packed rows account for padding bits.
- UI image import applies orientation and uses at most 6,000 pixels on the longest side. Valid source DPI is scaled
  with any pixel reduction. PDF import uses up to 250 dpi with the same pixel bound and preserves page rotation and
  effective physical size. All TIFF/PDF pages are imported, with a 2,000-page bound per input and an error for an unreadable
  page. Batches remain in memory without an aggregate page/memory budget. Raster CLI input uses full resolution within
  the larger processing bound; PDF CLI input shares the PDF rasterizer.
- Save/export capture one immutable output snapshot. OCR must finish successfully when requested. Clean state, history
  counts/thumbnails and the completion callback refer to the saved snapshot, not later pages. PDF compression preserves
  physical page size and corresponding OCR geometry. Single-page encoders cannot silently drop extra pages. Each output
  file is atomically replaced after successful encoding; a multi-file batch is not one atomic transaction. History
  removal deletes its entry/thumbnail, not the saved document.
- Signature/stamp placement belongs to one page identity. Return or Print into Page commits the mark into both the
  displayed pixels and the geometry-corrected base with multiply blending; Escape, changing the shown page or its
  pixel revision, and closing the window cancel unfinished placement. Existing OCR text/geometry is preserved because
  page geometry is unchanged; pending results from the previous pixel revision are discarded. The mutation is one
  complete Undo step. Preserve grayscale/1-bit page
  kinds. Saved output contains the resulting page pixels, including PDF; no independent PDF annotation is created.
- Native regression scenarios must exercise actions, mouse/key controls, complete Undo/Redo and rendered/saved data.
  Keep deterministic pixel/document checks separate from real Vision recognition/detection and virtual-scanner jobs.
  None of these substitutes for physical scanner, Continuity Camera, external sharing or printer testing. Each audit records
  the tested binary, actual results, hardware used or absent, publication state and remaining limits.

## Menu bar (`MenuBarSpec` → `MenuBar.build`)

Same skeleton and order in every app; empty Insert and Format menus are omitted. Applications can adapt or disable
standard commands to match their document model:

| Menu | Kit42 provides | App adds |
|---|---|---|
| **App** | About, Settings (optional), Services, Hide, Quit | — |
| **Файл** | New, Open, Open Recent, Close, Save, Save As, Duplicate, Revert, Page Setup, Print | `fileExtras`: export, password, … |
| **Правка** | Undo/Redo, Cut/Copy/Paste, Select All, Find ▸ | `editExtras`, `find` |
| **Вид** | Sidebar ⌃⌘S, Toolbar ⌥⌘T, Toolbar Labels, Customize, Hide All Panels ⇧⌘\, Full Screen ⌃⌘F | `view`: zoom, modes, navigation |
| **Вставка** | — | images, signatures, pages / tables / rows |
| **Формат** | — | font, paragraph / cell format |
| *App menus* | — | See the application menu list below |
| **Окно** | Minimise, Zoom, Bring All to Front | — |
| **Справка** | system search | shortcuts, welcome window |

Custom top-level menus (`Commands.swift`, localized in the app):

- PDF42: Markup, Form, Page, Signature.
- DOC42: Review, Table.
- XLS42: Table, Sheet, Data, Tools.
- IMG42: Image, Markup.
- PPT42: Slide, Slide Show.
- SCN42: Scan, Page, Tools.

`RecentDocumentsMenu` rebuilds Open Recent from `NSDocumentController.shared.recentDocumentURLs` when opened.
Entries route through the app's document controller; Clear Menu is disabled when empty. IMG42 and SCN42 use custom
controllers, so recent-file actions must not assume every app has an `NSDocument` editor.

### Shortcuts reserved across the suite

| Shortcut | Shared meaning where supported |
|---|---|
| ⌘N ⌘O ⌘S ⇧⌘S ⌘W ⌘P | standard file commands |
| ⌘Z ⇧⌘Z ⌘X ⌘C ⌘V ⌘A | standard edit commands |
| ⌘F ⌘G ⇧⌘G | find, next, previous |
| ⌘+ ⌘- ⌘0 ⌥⌘0 | zoom in, out, actual size, fit |
| ⌃⌘S ⌥⌘T ⌃⌘F | sidebar, toolbar, full screen |
| ⇧⌘\ | temporarily hide/restore all panels |
| ⌘1…⌘9 | tools / modes of the current tool strip |
| ⇧⌘K | insert signature or seal (shared library) |
| ⇧⌘I | insert image |
| ⌘B ⌘I ⌘U | bold, italic, underline in document/text editing contexts; SCN42 uses ⌘B for Scan |
| ⌥⌘D / ⌥⌘V | digital signature: sign / verify |
| ⇧⌘0 | welcome window |

Prefer ⌥⌘ or ⇧⌘ for app-specific shortcuts and check the assembled menu for collisions. Deliberate exceptions are
documented by the app: PPT42 uses ⇧⌘P for Play from Current Slide and removes that shortcut from Page Setup.

## Signature and stamp library

- PDF42, DOC42, XLS42, IMG42, PPT42 and SCN42 use the same local library and generator. A generated item stores a preview
  and a JSON recipe; an imported stamp also keeps its original source. Insert renders a fresh impression from the
  recipe. Importing a fixed picture retains a fixed image. Visual signatures and stamps are separate from the
  certificate-based digital-signature commands.
- **My Signature…** accepts strokes from `SignaturePad` or traces an imported picture. The pad records points,
  time and available event pressure; picture samples have no measured timing. Rendering uses stroke geometry and
  captured or modelled durations, blends structurally compatible samples, and varies each copy. The recipe keeps
  the samples, ink and chosen pen locally. Picture tracing also stores an estimated stroke-width ratio, used within
  the renderer's bounds when making copies. It does not send samples to a training service.
- Generated and learned signatures share the renderer. A smooth path defines geometry; overlapping lognormal
  movement impulses define speed along it. Ballpoint, gel, fountain and felt-tip pens convert that motion into
  different widths and ink textures. Recipes without a pen field retain the ballpoint default.
- Learned samples use one scale for both axes, with a width-aware floor for very flat strokes. Raw input, spline,
  spatial resampling, timing integration and output bitmaps have explicit work/size bounds. Reject invalid/nonfinite
  recipes before those operations; keep pixel and JSON-round-trip regressions for valid flat and wide samples.
- The signature blind-test commands create a numbered, shuffled sheet and score a tester's selected cells against its
  key. They normalize ink presentation and retain measured answers separately from generated fixtures. Correct/inverted
  fixture answers validate scoring mechanics only; a human comparison requires real reference images and independent judgments.

## Kit42 components

| Type | Purpose |
|---|---|
| `Command`, `Shortcut`, `commandTag(of:)` | action description; the command tag (tool, mode) arrives in `sender` |
| `MenuBarSpec`, `MenuBar.build` | menu bar |
| `RecentDocumentsMenu` | live recent-file list with custom-controller routing and Clear validation |
| `Panels`, `PanelHiding`, `ToolbarLabelsToggle` | temporary panel hiding/restoration, toolbar labels and full-screen behavior |
| `ToolbarController`, `ToolbarElement` | toolbar from commands (button, group, search, view) |
| `ToolStrip` | slim bar under the toolbar; controls in rounded groups (`.group`, `.tools` exclusive, `.buttons`, `.view`) |
| `StripButton` | borderless icon button: soft hover, accent when on, optional drop-down menu (alignment, lists, borders) |
| `ColorButton` + palette | icon with a colour bar; click opens a curated palette with "No colour" and "More colours…" |
| `SizeStepper`, `stripPopUp` | compact "− 12 +" for sizes; borderless pop-ups for font/style/format |
| `PaginatedPDF` | paginated PDF of any view through the print system (exports) |
| `DocxWriter` | .docx writer (paragraphs, runs, tables, images) — the system writer drops tables |
| `BannerView` | status bar (`info / success / warning / error`) |
| `WelcomeWindowController` | start window |
| `StampLibrary`, `StampLibraryViewController`, `ImageTools` | shared library of signatures and seals, background removal |
| `SigmaLognormal`, `PenRenderer`, `PenType` | sigma-lognormal timing along a smooth line (geometry and time separated); ink from speed and direction for ballpoint, gel, fountain and felt-tip pens, paper fibres |
| `SignatureBench` | measurement bench: fluency (normalized jerk, velocity peaks, hesitation), line wobble, DTW variability, Vision legibility, calibration references, `--check` thresholds |
| `SignatureLearning`, `LearnedSignature`, `SignaturePad` | user's samples: timed pad strokes or traced picture strokes (threshold, Zhang–Suen thinning, good-continuation tracing); RDP via-points with captured or modelled durations; compatible samples blended into varied copies |
| `SignatureGenerator`, `PenAlphabet` | hand-drawn letter centre lines, centripetal spline and low-pass hand; sigma-lognormal speed along that smooth path; 1320 styles (design seed low bits); impression seeds vary drift, letter position/scale and flourishes |
| `SealGenerator`, `SealCatalog` | 132 stamp templates in 15 categories built from layout families (round, oval, rect rows, office word, CJK grid, dater, table, triangle); `impress` gives imported pictures the same impression |
| `MarkGeneratorController`, `GeneratedMark`, `InkTexture` | generator window; library items keep a JSON recipe (and imported originals in `Stamps/Sources`) so each insertion renders a new impression |
| `InkColorPopUp` | ink colour presets with swatches plus Other Colour… |
| `CertificateStore`, `SigningIdentity` | certificates from the Keychain, .p12 import, self-signed creation |
| `Theme42` | sizes, app accent colours, `NSAlert.form` |
| `DebugSnapshot` | `K42_SNAPSHOT=out.png` renders a window, requests app activation, then exits unless kept open; `K42_APPEARANCE=dark\|light` forces appearance; `K42_AUDIT_RECENT=1` checks recent-menu behavior |

## Visual parameters

- App accents (icon, details; `Theme42.App.colors` = `make_icon.swift` palette): PDF red `#FF6A5C→#D81E35`,
  DOC blue `#4FA3FF→#1659D6`, XLS green `#3FD486→#0E8A4A`, IMG purple `#B57CFF→#6A2BD9`,
  PPT orange `#FFB547→#E5671A`, SCN cyan `#3FD8E0→#0B8FA6`.
- Icons are drawn in code (`scripts/make_icon.swift`, redrawn by `build.sh` when the script changes, `./build.sh icons`
  forces it; the `.icns` files are committed). Apple's grid: 1024 px canvas, 824 px squircle body (superellipse n = 5)
  with 100 px margins and a soft drop shadow; body = vertical gradient of the app colour with a top light.
  On it the same white sheet with a folded top-right corner; the sheet carries the app's mark: PDF — text lines and a
  red "PDF" label, DOC — blue heading bar and text lines, XLS — green grid with header row, IMG — landscape photo,
  PPT — slide/chart, SCN — page crossed by a scanning beam.
  ≤ 32 px use a simplified drawing (fewer, thicker marks).
- Dark mode: only semantic colours (`labelColor`, `separatorColor`, `controlAccentColor` …) in Kit42 drawing; layer
  colours (`cgColor`) are resolved again in `viewDidChangeEffectiveAppearance`. Paper-like things (stamp tiles,
  document pages) stay white on purpose.
- Shared defaults (`Theme.swift`): tool strip 42 pt, banner 36 pt, sidebar 180 pt, spacing 8 pt, corner radius 8 pt.
  Individual window controllers set their own sidebar/inspector minimums and maximums.
- Strip: capsules fit their controls with 3 pt vertical / 4 pt horizontal padding (radius 8, 4.5 % label fill,
  hairline border), 8 pt between groups and 2 pt between controls. Buttons are 30×26 pt, or 38×26 with a menu;
  symbols 13 pt medium, hover 7 % fill, "on" = accent 18 % with accent icon.
- Group outline is a `labelColor` 8 % hairline: `separatorColor` resolves much darker on non-vibrant backgrounds.
- Related choices collapse into one drop-down that shows the current state (alignment, lists, borders, line width).
- Colours are never big wells: icon + colour bar, palette in a popover.
- Thumbnails (PDF42 sidebar, IMG42 filmstrip): one centred column in sidebars; selection is an accent outline around
  the page or picture, never a filled cell.
- Segmented tab bars use either all text or all icons with translated tooltips (PPT42 inspector uses icons because
  five translated text labels do not fit its 290 pt width). Check intrinsic label widths across all 20 languages;
  long translations must not be clipped to the English width.
- Prominent buttons use the system accent (`bezelColor` unset); app colours are for icons only.
- Menu items carry the command's SF Symbol; cells/pages have context menus built from the same commands.
- For newly generated document text, prefer document fonts such as Helvetica Neue instead of SF, whose Cyrillic
  characters have caused PDF extraction problems. Preserve fonts from imported documents; UI text uses system fonts.

## Adding a macOS app

1. Add `Sources/<App>/`, an executable product/target depending on Kit42 in `Package.swift`, and `Resources/<App>/Info.plist`.
2. `Commands.swift`: every `Command` plus `menuBar: MenuBarSpec` and `toolbar: [ToolbarElement]`.
3. Window controller: actions `@objc func …(_ sender: Any?)`, validation via `validateMenuItem` / `validateToolbarItem`.
4. Add the app to `Theme42.App`, `scripts/make_icon.swift` and `build.sh`'s `ALL` list; register its translations in `main.swift`.
5. Add Quick Look type mappings only if needed, and include the app in `scripts/audit-ui.py` with representative fixtures.
6. Run `./build.sh <App>` and verify both appearances and a narrow window. Update README and the dated status report.

## Interface verification (macOS)

`python3 scripts/audit-ui.py` builds isolated test bundles from already-built debug executables and runs native
controller checks plus light/dark snapshots. It needs a logged-in desktop; bundle identifiers isolate app-specific
defaults, while the suite language remains shared. See [README.md](README.md#regression-checks) for commands.
Snapshots use AppKit view rendering, which can omit native composited material layers. Suspect artifacts must be
checked in a live window before changing layout. For example, the XLS42 Format Cells tabs were black in a cached-view
snapshot but rendered correctly in the live dialog. Responder-action tests do not simulate physical keyboard input.
For PDF42, `--app PDF42` runs markup, pages/dialogs, core controls and lifecycle regressions. Pair it with
`PDF42 --selftest` and `scripts/audit-pdf-cli.py --binary …`; keep skipped signing checks and limits of the local
self-signed fixtures explicit. Use `--binary` with `--app` to audit a packaged release executable.
For XLS42, `--app XLS42` runs the editing/formatting/lifecycle and print-layout regressions as well as snapshots.
Pair it with `XLS42 --selftest` for calculation/model checks and `scripts/audit-spreadsheets-cli.py --binary …` for
independent XLSX/ODS/CSV fixture conversions and PDF text/page checks. A fixture's scope and any skipped PDF checks
must remain visible in the recorded result.
For PPT42, `--app PPT42` adds canvas/editing, text/table/show, lifecycle and inspector suites. Pair those with
`PPT42 --selftest` and `scripts/audit-presentations-cli.py --binary …`; record skipped PDF/movie checks explicitly.
Native control tests include rendered fill/border samples and complete-document output assertions where relevant.

For IMG42, use `IMG42 --selftest`, `scripts/audit-ui.py --app IMG42` and
`scripts/audit-images-cli.py --binary …` together. Pixel fixtures cover actual masks/transforms/adjustments, native
scenarios cover controls and Undo/lifecycle, and independent saved-file checks cover encoding and metadata. Record
codec- or Vision-dependent skips explicitly; successful panel toggles are not editing/output verification.
The IMG42 native suite also exercises disposable-file Trash/Undo/Redo across navigation, actual full-screen Spaces,
VisionKit text-selection gestures and Escape, and real Vision subject segmentation. The model-dependent scenario
preserves its generated source, alpha output and masks; it must fail visibly if recognition or segmentation fails.
Native audit bundles use isolated identifiers and are re-signed. Reports distinguish original executable hashes from
the launched copies and verify unsigned Mach-O equivalence, resource manifests and unchanged inputs after the run.
For SCN42, pair `SCN42 --selftest` with `scripts/audit-scans-cli.py --binary …` and
`scripts/audit-ui.py --app SCN42 --binary …`. Native core/scanner/editing/files scenarios exercise settings, cancellation,
page gestures, complete Undo/Redo, saved snapshots and Copy-to-PDF. A separate real Vision scenario covers OCR,
orientation, document crop and photo separation. The runner also retains legacy checks and light/dark captures.
Record physical-device absence explicitly; virtual backend and print-to-file checks do not verify scanner drivers,
hardware duplex, Continuity Camera or printer output.

`localization42audit` checks the language set and aliases, process-only overrides, bundled lookup, registration
merging, formatting and English fallback without XCTest. `scripts/check_strings.py` checks catalog structure and
protected literals; `scripts/audit-localization.py` checks native welcome screens and menu language in each app.
These complement the behavioral suites and must not be reported as editing or complete linguistic review.

## Quick Look

- Only formats macOS shows as plain text or not at all get extensions (checked with `qlmanage`): Markdown
  (hosted by DOC42), `.ods` and `.tsv` (XLS42). `.docx/.xlsx/.xls/.csv/.rtf/.odt` are left to the system.
- Previews are data-based HTML (`QLIsDataBasedPreview`) with `prefers-color-scheme` light/dark CSS; thumbnails are drawn
  on white paper (portrait page for documents, zoomed grid for spreadsheets).
- One binary per kind (`QLPreview42`, `QLThumbnail42`); `build.sh` writes each host's Info.plist with its
  `QLSupportedContentTypes` (`QL_TYPES` in build.sh), bundle id `<app id>.preview|thumbnail`.
- Extensions are sandboxed (`Resources/QuickLook/Extension.entitlements`) and signed before the app; the app is
  signed without `--deep`, which would strip their entitlements.
- Extension strings use `PL("…")` (legacy rows in `Previews42/Strings.swift`, additional languages in the Kit42
  catalogs) and follow the first supported system language: the sandbox cannot read the suite's language choice.

## OFF42 website contracts

The public product website lives in `website/`, independently of the macOS and Windows app builds.
A dependency-free Node generator renders 14 route types for the same 20 language
codes as `Kit42.Language`. Routes use `/<locale>/`; Arabic sets `dir=rtl`. Shared
header, footer, feature illustrations, download cards and language navigation use
one renderer and stylesheet. Language changes preserve the current route. Navigation
and page content work without JavaScript; the small script closes menus on Escape/outside click.

The visual system uses warm ivory, dark green, accessible burnt-orange buttons,
large headings, a generated paper still life and clearly identified CSS/SVG workflow
illustrations. These illustrations are not screenshots or evidence of app behavior.
Responsive grids collapse for small screens; screen-reader labels must use clipping,
not negative horizontal positioning (which creates RTL overflow). Normal text contrast
must remain at least 4.5:1. `website/QA.md` retains the reproduced regression scenarios.

Every page has localized HTML, a unique title/canonical, 20 hreflang links plus
x-default, social metadata and a single H1. Feature FAQs have matching JSON-LD. Each localized home page
embeds that language's demo video (MP4 with a JPEG poster); the build check requires both for all 20 languages.
The sitemap enumerates all 280 canonical localized routes. The root selects English
through an HTTP redirect; the site does not silently change language based on location.

`website/site.config.json` owns the GitHub destination and nullable platform release
entries. A null entry renders an unavailable status and real source/release links.
Direct downloads require a real HTTPS installer URL, version, architecture and system
requirements; SHA-256 can be shown. Publishing the website never builds, installs,
signs or notarises applications. Platform-specific capabilities and installer availability are user-visible.
Copy for both macOS and Windows must follow the implemented scope and configured releases; changes to either
platform require explicit content updates.

## Technical elements and label output (macOS)

`Kit42.TechnicalElement` is the shared editable recipe. The offline encoder produces integral-module PNG
and vector PDF with quiet zones. PNG Description metadata stores the bounded versioned JSON recipe;
PDF42 annotations store the same recipe in an annotation key and draw a normal-blend vector appearance.
DOCX/XLSX/ODS preserve original technical PNG bytes, including metadata. Code values and physical sizes
are validated before insertion; editing replaces one object with normal Undo/Redo. There is no cell-link
expression or remote rendering service. Third-party resource licences/provenance live alongside assets.

Marks come from `Technical/marks.json` (imported from Lably, 2026-11-25): 97 artworks, one per file, whose id is
the file name so the earlier 14 symbol ids keep resolving. The symbol popup lists category headings (disabled
items) with indented marks carrying their id as `representedObject`; search ranks exact id/name matches first and
selects the best match. Under the preview a notice shows the mark's purpose and, in orange, its conditions of use
(conformity marks, trademarks, required registration numbers, artwork Lably had withheld as unverified). All marks
are insertable; the notice informs, it does not block. Mark names follow the interface language where Lably has
copy (11 languages), otherwise English. Raster marks (OSHA GHS PNGs, a WebKit render of the ETL file that
NSImage cannot draw) are loaded like SVGs by file extension.

Apps adapt their selected content into immutable `LabelSource` snapshots. Table rows retain original row
identities and visibility, with the header row still present for a reversible header toggle. PDF regions
come from `exportSnapshot()` so pending edits and redactions are included. DOC rich selections become
one-page PDF snapshots. `LabelSheetRenderer` owns bounded copy expansion, physical stock layout,
pagination, vector placement and fit errors. The shared transient sheet owns mapping, templates and
preview; changing inputs invalidates output immediately, and generation tokens reject stale results.
Saved templates rebind table fields by unique names, not by silently reusing column indexes.

`ProductionPrint` keeps calibration in NSPrintInfo, keyed by printer and paper for saved profiles.
It transforms each complete printed page, including DOC subviews and XLS images, with positive Y
meaning down on paper. Export renderers never read calibration. PDF and mixed-size DOC print paths
use a shared vector print view; the existing XLS print options remain available in their accessory.
There are no permanent sidebars or new default toolbar buttons. These Kit42 technical-element, label and
calibration contracts apply to macOS; the Windows apps have their own barcode and label-sheet tools, described in
[rust/README.md](rust/README.md#labels-barcodes-and-signatures).

`production42audit` verifies actual code decoding, vector/PNG output, table/copy/visibility rules,
label pagination and print-to-file geometry. App native technical scenarios additionally cover
commands, controls, complete Undo/Redo and disk save/reopen. The shared UI test is opt-in with `--ui`;
coordinate it with other desktop audits. These checks do not certify physical printer alignment.
