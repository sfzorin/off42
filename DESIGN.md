# off42 — design system

off42 has native macOS and Windows implementations of PDF42, DOC42, XLS42, IMG42, PPT42 and SCN42.
The Swift/macOS apps share **Kit42** (`Sources/Kit42`); the Rust/Windows apps share native Windows infrastructure.
The Kit42, AppKit and Swift contracts below describe macOS; the Windows workspace has its own implementation
and feature scope. Shared product names do not imply identical capabilities.
This describes the source as of 2026-11-07. [README.md](README.md#regression-checks) documents verification commands
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
- On Windows, accepting the modal speaker-notes editor adds one slide-scoped history entry. Office overlays patch
  supported note text and preserve other shapes and parts. Adding a PPTX note reuses a compatible notes master or
  adds a standard master using the slide theme. Unsupported layouts retain their original package data.
- Windows rehearsal reuses the normal slide-show order, transitions and navigation. The host lays out the current
  canvas alongside an independent next-slide preview and a read-only, scrollable notes control. Blanking affects
  the current canvas alone. Ending rehearsal destroys its controls and restores the editor window.
- Windows slide image exports render the presentation scene in a bounded, opaque offscreen buffer. Batch output
  preserves original slide numbering and stages every file before publication. A destination conflict leaves
  existing files intact and rollback removes only outputs whose identities still match the published files.
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



Windows spreadsheet pictures use shared grid object hooks for immutable decoded pixels, selection and a
single movement commit at pointer release. Drag previews stay in the host; document history stores cell
anchors, point offsets and physical size. Frozen panes clip objects using the same geometry as cells.
XLS42 preserves original PNG bytes (including technical recipes) in projects and Office exports; imported
Office pictures retain provenance so only the corresponding original drawing node is replaced.

Shared Windows technical-object commands pass an immutable rendered object to an app adapter. Each
adapter inserts or replaces the object in one document-history transaction and retains the portable
recipe. Object choice uses current document identities; PDF recipes share the macOS annotation key.

PDF technical appearances contain only bounded path operators, with glyph outlines and independent
physical bounds. The core validates the operator allowlist and graphics-state balance before recording
a vector stamp. Raster pixels are transient for vector placement and are not retained in its history
or emitted as an image resource. Standard image signatures continue to use their alpha image appearance.

Label inputs are immutable source snapshots. Apps can enumerate named sources; the source chooser only
appears when there is a choice. DOC tables are explicitly named because the current linear selection
API does not cover table cells. Label preferences separate tabular and text workflows, hash source
headers before restoring mappings, and exclude document contents and barcode payloads.

The Windows host defers real menu/toolbar commands received while refreshing native controls. RichEdit
and TSF can pump messages inside a refresh; generated control notifications remain suppressed, while
user commands are posted back in order after the refresh releases the model. Deferred commands are also
flushed after smaller guarded control updates and normal command dispatch.

## Windows language host

The Rust host bundles exported macOS catalogs (reproduce with `rust/scripts/export-localizations.py`).
Windows-specific labels are supplemented by `win42/resources/windows-translations.json`; these override imported
keys without changing their source catalogs. `win42::localization` owns the twenty-language set, matching, process overrides and bounded atomic preferences
in `%LOCALAPPDATA%/off42/Preferences/language.json`. View → Language confirms unsaved changes and restarts
with the current saved path. Only UI keys pass through lookup; field data and document strings stay untouched.
Choice prompts retain original values/indices independently from translated display labels. Prompt label heights
are measured with the GUI font; Arabic labels align right without mirroring document geometry. Missing Windows
keys use English. This does not establish full translation or proofing coverage.

## Windows recent documents

Each app has a bounded atomic JSON path list in `%LOCALAPPDATA%/off42/Recent Documents/`. Only successful
normal session opens and saved snapshots update it. Asynchronous opens enter the list after identity validation
and commit. File → Open Recent uses stable command IDs and the same `open_paths`/unsaved confirmation as Open.
Menu display escapes filename mnemonics and never translates paths. Clear Menu changes preferences only.
There is no document content in this file and no cloud synchronization. Import-only scan sources are excluded.

## Windows exports with settings

`App::file_input_request` optionally collects export settings before the file picker. Neither Cancel invokes the app.
`file_action_with_input` receives both confirmed values and destination, leaving ordinary `file_action` unchanged.
File action menu states now use the same app availability checks as their dispatch. PDF42 page export captures
`PageExtraction` once; each worker output clones/prunes its committed graph and remaps effective page labels.
Source state/history stay untouched. Split outputs stream into one temporary ZIP, with bounded total data,
progress, cancellation and final no-clobber publication. No partial destination is exposed.

PDF page-label overrides live in the core snapshot state, keyed by page object identity. A range edit expands only
its effective rules and commits one history transaction; unaffected labels retain their values. Clear uses an explicit
empty override to remove the catalog tree. Saving, reordering and extraction rebuild rules in the current page order.
Roman/letter display matches the macOS bounded implementation. `App::input_choices` supplies stable underlying
values for translated native choice controls, reused by page-label numbering styles.

Windows Save As uses a host-only ID outside application tool IDs. Earlier ID 18 collided with IMG/SCN crop and PDF
rectangle tools; those app tool IDs remain stable, while Save As now uses 65507. The audit detects old/new Save As
IDs so it can still inspect older downloadable previews. Canvas pointer moves update tools/redraw only during a
captured gesture; current tools have no passive-hover state. This keeps idle synthesized moves out of caption/
paint updates and lets background timers run. Native regressions check full crop pixels/history and drawn PDF
annotations, alongside the pending asynchronous-open scenario.

## Windows engineering calculations

The formula parser recognizes a complete function token (including digits/dots and Excel compatibility prefixes)
before considering A1 references; reference parsing remains separate. Scalar engineering arguments reuse bounded
expression parsing. Signed radix conversions and 48-bit operations enforce bounds before integer conversion/shifting.
Unit constants are exported reproducibly from the macOS source by `rust/scripts/export-unit-conversions.py`; the
bundled catalog is lazy and case sensitive. Dimensions, SI powers, binary-information prefixes and temperature
scale/offset handling remain separate. Explicit scalar Boolean/numeric-text coercion does not affect range filtering.

Office export obtains typed formula results from one evaluator/cache per sheet. The XLSX writer infers types from
string values, so generated text formula caches are corrected to `t="str"` before computing the embedded project
digest. The patch retains formula XML, other cell attributes and unrelated ZIP entries. Spreadsheet string escapes
preserve literal `_xHHHH_` text and encode invalid XML characters. ODS gets typed scalar values directly; error
cache display is textual because that writer has no error variant. External imports are tested after changing an
input, ensuring recomputation cannot pass solely through a saved cached result.

## Windows runtime dependency gate

The explicit MSVC target uses `+crt-static`; Cargo host tools/proc macros remain separate. CI stages binaries from
the target-specific output directory and tests with PDFium beside target-specific test executables. A bounded Python
PE reader examines import and delay-import tables, requires x64 PE32+ images and rejects redistributable C++ runtimes
and unknown libraries. OS/API-set imports and bundled PDFium are allowed. Reports record each binary's SHA-256.
The archive is created only after this gate passes. The source handoff includes the target configuration.


## PDF text export and configured file actions

A file action can resolve its extension/filter after its settings prompt and before the system picker. The
`configured_file_action` hook is read-only; prompt cancellation, invalid settings and picker cancellation start
no work. Choice controls use the same stable underlying values as ordinary command prompts.

PDF text export takes the same committed `save_snapshot` as other outputs. A worker owns the immutable bytes,
serializes PDFium use with existing search/edit sessions, and closes every native handle through RAII. It reads
pages in the requested order, coalesces equal character styles, normalizes line endings, and preserves blank page
boundaries. DOC42 adapters serialize RTF/HTML/DOCX; the HTML adapter now round-trips explicit page breaks. Font
names are sanitized before entering rich-text output. Paragraph/text/style-run limits bound allocation, and
cancellation is checked between pages and during glyph iteration. Final publication is atomic and no-clobber.


## XLS scalar text arguments and parity reporting

The extended text dispatcher preserves omitted arguments separately from explicit values. Text aggregators read
bounded row-major ranges and preserve truly blank cells as empty strings; ordinary numeric aggregation retains
its existing type-filtering rules. Existing scalar formula values remain authoritative, and Office caches use
the previously established typed result path. Text functions live behind one dispatcher with explicit argument,
output and search-work limits. Wildcard SEARCH uses bounded dynamic programming rather than repeated regex or
quadratic suffix scans. Tests strip internal Office project data and edit inputs before checking recalculation.

The reference-case exporter reads only the macOS fixed fixture list and records its SHA-256. A standalone report
compares actual portable calculation results with those examples and retains all gaps. It is a diagnostic report,
not an all-pass test or a feature-count claim; locale-dependent examples are identified separately.


The math dispatcher shares numeric coercion with the established evaluator and receives both flattened numeric
values and the original argument count. This keeps range filtering intact while rejecting invalid arities even
when ranges contain no numbers. GCD/LCM use bounded exact integers and checked products; combinatorial loops stop
when no finite f64 result is possible. Stable reciprocal/asymptotic forms avoid needless overflow for ACOTH and
SQRTPI. Calculation never substitutes NaN/Inf for a visible formula error.


## Independent native windows

The Windows host keeps one document session per process. File → New opens a blank sibling process for apps that
create documents, matching the macOS document model; it does not replace the current session, ask to discard it
or cancel its asynchronous open. File → New Window (Ctrl+Shift+N) is available in all six apps, including viewers.
Both blank windows and additional dropped/opened documents use the same spawn helper, forwarding the current
language override and a `--` path separator. Save paths, histories and native handles remain process-local. Shared
library/preferences/recents retain their established local-file behavior.


Window placement is a separate bounded preference per app: normal workspace rectangle and maximized state only.
GetWindowPlacement/SetWindowPlacement are paired so taskbar/workspace coordinates are not mixed with screen
coordinates. Windows relocates entirely offscreen rectangles after display changes. Fullscreen slideshow windows
save their pre-fullscreen placement; minimized state is never restored. Preferences are validated and published
atomically only after close is confirmed, and preference failures do not block document closure. No document path
or text is stored in layout files.


## Rich document search and anchored replacement

Native Rich Edit Find searches its actual projection through EM_FINDTEXTEXW, using native character ranges and
wraparound. A plain model projection cannot represent table/image offsets. The host retains the existing app
adapter for plain editors and uses Rich Edit selection APIs for rich results. This fixes literal search through
structured documents without claiming new regex/whole-word behavior.

Literal body replacement is isolated in DOC42's search module. It plans bounded nonoverlapping matches before
mutating a disposable transaction, includes table paragraphs and skips images/section separators, indexes run
boundaries to retain formatting/links without rescanning all runs per match, and remaps bookmarks/comments/notes
from right to left. Matches covering invisible note markers are refused so a plain text query cannot silently remove a note. Revision/field
anchors and cached TOC content remain guarded. One history entry contains the entire operation; invalid/oversized
results commit nothing. The ODT reader isolates outer bookmark markers while parsing inline note paragraphs.


An additional preflight bounds matches multiplied by document annotation/field/revision/TOC contexts. The generic
anchor remapper is deliberately reused for consistent gravity, and excessive aggregate work is refused before
any rewrite. This prevents the independent match/document caps from multiplying into a long UI-blocking operation.

### Windows vector paths

`win42::SceneItem::VectorPath` carries compound Move/Line/Cubic/Close paths with independent optional fill
and stroke. The GDI host uses an even-odd path at the destination resolution for canvas, show and printer
output, with saved/restored DC state. PPT42 evaluates the shared preset catalog at the current shape size,
then applies the existing object transform. Yellow handles edit a temporary drag object; pointer-up commits
one model history step and cancellation drops the preview. Text uses the preset's evaluated text rectangle.
Preset identity and adjustment values remain editable across project/PPTX/ODP output. Generated MPL data
ships with source, notice and license in the Windows archive.

### Windows PDF page decorations

PDF decorations live in the PDF snapshot history and are keyed by stable page identity. A selection is validated
before committing a single multi-page operation. Header/footer and Bates tokens resolve in document order at insertion;
future page reorder retains the assigned text. Rendering and export use the current crop/quarter-turn transform.
Save snapshots add isolated Form XObjects, preserve inherited/shared page resources, and wrap original content state.
Repeated watermark forms/images and font character sets are shared. Watermarks use glyph outlines; headers and numbers
carry embedded Unicode text. Reopening leaves baked page content. Filename tokens follow only successful save callbacks.
The shared configured file action validates image-watermark settings before showing its PNG/JPEG picker.


### Windows read-only statistics and typed statistical arguments

`DialogRequest::Information` displays a localized read-only message without producing a model callback.
DOC42 statistics traverse rich body paragraphs and table cells directly; they do not flatten the Rich Edit
caret projection or alter history. Style runs are joined before Unicode word/grapheme segmentation.

XLS extended arguments preserve scalar/reference provenance and actual blank flags alongside evaluated values.
Statistical families use that information for standard and A-variant coercion and paired observations.
Distribution iterations charge the evaluator's shared step budget; argument text charges a separate cumulative
8 MiB budget before accumulation, including cache hits. Tail and inverse-tail functions avoid `1-p` cancellation.
The independent high-precision numeric fixture is QA data, not an installed runtime dependency.

Office snapshot embedding rebuilds fresh ZIP containers, preserves unrelated parts and declares the JSON part
in standard package metadata before hashing. Re-embedding removes prior private declarations/parts. ODS import
repairs only a dangling own optional snapshot declaration in its working copy. Formula token mapping operates
outside quoted literals and distinguishes ODF native F left tails from legacy Excel right-tail names.


### Windows spreadsheet financial and calendar arithmetic

Financial functions reuse typed scalar/reference arguments and the global calculation budget. IRR and RATE
solve in log-rate space, normalize residuals and use bounded bracket fallback; cash-flow sign and date-shape
checks precede solving. Loan interest discounts remaining payments rather than advancing a rounded payment
through many periods. Long-term future balances retain compensated cancellation terms.

Calendar functions work in absolute Excel serial days with a workbook offset. Explicit components preserve
day 60 without constructing an invalid Gregorian date. Business-day intervals count whole weeks and holiday
sets; WORKDAY finds the first distance reaching the requested count with bounded monotone search. Real ISO
entry values are typed dates. Native XLSX uses serial values/date formatting; ODS uses date-time values, and
external XLSX edits use the standard ISO date cell kind. Explicit imported text remains text.


The plain Windows EDIT subclass brackets native text replacement, paste/cut/clear and character operations.
Intermediate EN_CHANGE notifications are suppressed until the outer native operation returns; the host then
commits its final text once. REFRESHING notifications remain ignored, and Rich Edit keeps its existing path.
This preserves one history step when replacing a selected cell value instead of first committing its deletion.


### Windows conditions and reference information

The scalar evaluator retains the original formula-cell coordinates through nested/lazy/fallback parsers.
Reference metadata operations inspect reference syntax and cell kinds without calculating the referenced formula
when that is unnecessary. Error literals and imported error cells feed the same typed error path; error-looking
text remains text. Typed logical ranges ignore reference text/blank values and propagate errors across all operands.
IF/IFS/SWITCH/CHOOSE retain selected-branch evaluation; native result caches preserve Boolean/text/error types.


### Windows native reports and PDF comparison

`ReportRequest` presents a resizable native list, a copyable read-only detail field and explicit keep/clear
controls. Stable row indices produce `ReportEvent` callbacks; the owning app retains the data and performs
navigation/model transactions. The owner is disabled while the report is open, background completion/rendering
continues, and pending print dialogs wait until it closes. A pending report is consumed once on the UI thread.
The report does not add toolbar controls to every app.

PDF comparison reads two committed snapshots in a worker, keeps UTF-8 positions and PDF-space glyph bounds,
and compares Unicode words per page using bounded linear-memory LCS with common-prefix/suffix/disjoint fast
paths. It groups removed/replaced words into line rectangles. Temporary highlights multiply into disposable
view pixels; explicit keeping adds colored highlight annotations in one validated history transaction. Tagged
live marks can be cleared without affecting ordinary annotations; saved/reopened marks become ordinary PDF
annotations. Source/document and history identities invalidate stale results while allowing navigation and Save.

`SnapshotHistory::current_id` is opaque and follows the exact state through Undo/Redo; new branches and unequal
replacement get new identities. Saving leaves the identity unchanged. PDF snapshot guards combine it with a
unique source identity, so PDFs with matching object IDs cannot accept one another's asynchronous results.


### Windows file batches and separate results

`DialogRequest::CombineFiles` uses a native multi-select picker and a new save destination, then returns typed
paths to the app. Cancellation dispatches no model transaction. A completed background result can request a new
independent document window; its originating document/session is retained. Print/report modal work remains ordered.
Application command IDs are checked against host menus and native control-notification IDs before building UI.

PDF batch merge reads bounded inputs into disposable documents and shares the existing structural insertion
adapter. Its temporary model retains no user Undo history. Imported page-label rules resolve indirect standard
values before object renumbering and map old/new page identities; existing labels and default numbering remain
separate. Bookmarks/forms retain the existing namespace/resource validation. A fully prepared output is written to
a same-directory temporary file, checked for cancellation and published without clobbering. No input is rewritten.

Windows PDF header batches use the shared multi-file/folder dialog and return per-file outcomes in the native
report component. Report clients choose whether highlight controls and a compact first column are relevant.
App-requested continuation commands and output windows are delivered after modal owners become enabled;
settings can therefore continue into another picker without re-entering the previous input dialog. Large
settings forms use monitor-aware column placement. Batch files publish individually without replacing existing
paths, and cancellation preserves the copies already completed.


### Windows calculated spreadsheet arrays

A calculation owns disposable typed values and spill rectangles; authoritative worksheet cells own user input.
Formula anchors retain their source and calculated children never enter document history. An occupied input,
protected array span, merge or competing spill produces a blocked anchor. Matrix shape, work and cumulative text
are checked before replication. Parser-local LET bindings are scoped to the current expression. Scalar operations
lift through the existing evaluator, while conditional operations select branches before broadcasting.

Display, clipboard and export consume a single calculation per operation. Plain clipboard text uses formatted
results; private payloads preserve formulas and source coordinates. Partial spills become typed values, while
copied anchors keep calculated children empty. Export adapters write standard XLSX dynamic ranges/typed caches
and ODS matrix spans independently of the optional native project payload. External array structure preservation
remains a separate adapter responsibility; existing legacy-array edit guards are retained.


External array adapters distinguish authoritative input from cached children using standard format metadata.
Only pure, explicitly dynamic XLSX metadata records unlock caches; mixed records and legacy ranges remain guarded.
ODS matrix anchors use supported dynamic functions. Import validates overlap and bounded total range work before
clearing child inputs. Modern XLSX errors are sanitized in a disposable reader package and restored from original XML.

External scalar and array exports share one calculation/clock per sheet. Surgical XML updates preserve styles,
unknown attributes and objects, materialize only required ODS repeat runs, and clear old cache ranges without erasing
explicit inputs. XLSX registration preserves existing metadata records. Core ODS preservation checks remain strict;
a writer-injected default style is removed only where an unchanged source cell had no explicit style. Array metadata
updates occur separately from the general opaque-content preservation pass.


A spreadsheet calculation keeps full precision. Presentation formats only typed numeric results and never feeds
rounded grid strings back into arithmetic or typed Office caches. `calculated_cells` supplies sparse data-processing text;
`display_grid` and plain clipboard text share the presentation projection, while private clipboard payloads retain
source formulas/types/styles. The common cell formatter accepts an explicit sheet identity so filter/summary
references across sheets use the owning sheet's format. XLSX visibility uses the same calculation and disables the
writer's independent filter pass, preventing it from hiding formula rows that passed the application filter.

Windows PDF/scan recognition shares borrowed RGBA input and source-pixel word rectangles. The native OCR
SDK is pinned separately from Cargo, statically linked with the CRT, and ships verified offline models and
licenses. PDF workers retain only text between pages and commit against an exact source/history token;
selected-page capture normalizes disposable render geometry while the saved page retains original boxes,
rotation and annotations. Invisible font CIDs retain Unicode mappings even where a visible outline is absent.
Native backend and GUI verification is pending; the package remains a development build.

Image analysis captures an immutable shared raster and checks its pixel identity before displaying/copying
recognized words. Viewing sessions reset on file/frame navigation; drawing changes invalidate the cache
before another worker result can be accepted. Selection tint is a disposable display buffer. Applications
may omit infrequent command specifications from the toolbar while retaining menu items and accelerators.

PDF label artwork is an independent single-page snapshot. Native PDFium flattening runs under the shared
PDFium lock; the portable label renderer clips and normalizes page geometry inside one reusable form and
prunes unrelated source objects. Layout preferences retain stock/copies only. Source choices can carry
individual capture failures, so unavailable artwork does not disable other label inputs.

Document picture label sources carry encoded images and displayed dimensions; decoding is deferred until
the user chooses that source. The bounded bitmap wrapper preserves aspect ratio and composites alpha on
white paper, then reuses the PDF artwork assembly path. Other document content never enters that snapshot.

A shared PDFium mutex now covers search/edit/preview/printing initialization and teardown. Disposable
preview documents live on workers; only one BGRA page buffer enters the modal preview. UI callbacks run
outside window-state borrows, allowing standard save/print/settings dialogs without retaining engine locks.
Actual-size PDF print jobs retain immutable bytes and native document/font resources until spooling ends.

A document gesture may queue the shared label action and provide one ephemeral source override. The host
consumes it once, outside document-state borrows and after normal mouse release. PDF area selection
checks document epoch, save-state identity and current page before extracting an immutable single-page
snapshot. Selection outlines are display items only; cancellation or rerender clears the queued area.

Native rich-content label capture streams only the current selection, then formats an isolated editor.
A temporary EMF measures actual ink before allocating one bounded bitmap; the PDF stores only the
resulting selected pixels. No clipboard round trip or live-editor layout mutation is needed. Native
selected-text label sources use the same editor's UTF-16 projection rather than model-only offsets.
The initial mixed-content output is raster artwork; vector text fidelity remains a separate parity gap.

Recovery uses a readonly app snapshot contract, separate from user-save revision capture. Imported Office
containers remain in their original format; an inline presentation draft is applied only to an independent
state copy. The shared host tracks a recovered document's unsaved state independently of its newly decoded
model, so it cannot silently close or save over the original. Protected PDF sessions do not produce automatic
plaintext copies. File-menu/startup recovery is limited to inactive sessions holding no exclusive file lock.

Each journal writes one bounded, length/hash-checked atomic record on a serial worker. A failed write leaves
the previous record intact; a later Save/Discard still queues its cleanup. Claimed copies stay locked until
saved/discarded. Recovery storage is an owned cache, never an allowed document-save destination; cleanup
removes only known journal/materialization files and preserves unrelated files. Errors appear in status.

Each app exposes one ordinary print command to the shared host. The host places it in File, registers
Ctrl+P once, and retains normal command-state checks. PDF jobs distinguish actual-size label sheets from
centered/shrink-only document pages. Pure placement math uses physical sheet dimensions, separate axis
DPI and hard-margin offsets, then applies printer calibration. PDF/font resources survive through EndDoc;
queued background print jobs wait until the owner is enabled. Raster printing remains an explicit fallback.

Presentation scenes read an active inline text draft for both display and print while keeping the model's
history unchanged. Recovery deletion takes an exclusive claim and validates the record envelope but does
not require decoding/checksumming a damaged payload; recovery itself still verifies the full payload hash.

Apps may opt individual character commands into native Rich Edit formatting. The host derives command
states from the current native range and applies only the requested character mask, preserving mixed
attributes elsewhere. Empty character selections set typing attributes without a model/history mutation;
paragraph alignment changes its paragraphs even with a collapsed caret. Content changes pass through the
existing guarded projection adapter exactly once; generated control notifications
are suppressed during this operation, and a rejected projection restores the native snapshot and range.
Successful character formatting avoids a model-to-control reload, which would erase insertion attributes.

Image recovery uses an internal versioned sample/metadata record per frame, wrapped in an animation/session
record and the common hashed journal. It bypasses export codecs, retains 16-bit samples and conversion guards,
and captures readonly references to all active/inactive frames without consuming a pending save. Decoders
bound frames, dimensions, lengths, metadata counts and aggregate allocation; normal recovery produces a fresh
model and the shared host supplies the unsaved-copy lifecycle. Original paths are not output destinations.

Goal Seek freezes a workbook task before starting a cancelable worker. Numerical trials use a private model
with the ordinary formula evaluator and frozen calculation time; no trial writes enter live history or export
caches. A sealed result carries its source state/history identity and can be committed only against that state
through normal cell validation. The host's confirmation contract delivers an explicit boolean result; closing
or declining it does not apply the proposed change. Worker/session disposal signals cancellation without a UI join.

Rich Edit's projection may inject fixed view defaults and re-encode PNG resources. Only known default
padding and inline-picture properties are accepted in projection mode; regular imports stay strict.
The picture matcher tries exact resources first, then compares bounded decoded sample hashes and geometry,
restoring original compressed data/MIME/metadata on a match. It caches each source digest and bounds aggregate
pixel work; any removed or visually changed source picture still rejects the transaction. Unknown placement
properties are not silently discarded. Regression fixtures are the exact isolated native-audit source/RTF.

SCN acquisition captures generation plus an optional rescan page identity. A replacement validates the entire
batch against a private candidate that excludes the target's current slot/allocation, then replaces the target
with a fresh page and appends further feeder images in one history commit. Neither selection changes nor
index reuse redirect that callback; project generation or missing identity refuses it. Ordinary WIA acquisition
uses the same STA/result channel. Only explicit audit environment variables enable a WIC image fixture with
a bounded file barrier for deterministic cancellation/delayed callbacks; it never establishes hardware coverage.

Toolbar commands may declare toggle semantics. Native pushlike check controls receive the same checked state
as menus, while dialog/action buttons retain ordinary button semantics. Control updates occur outside model
borrows to avoid synchronous native notification reentrancy.

Windows constrained optimization runs through a sealed XLS42 task/result boundary. Tasks own a frozen private
workbook, share one evaluator per trial point and carry the source history identity; trial writes bypass live
history. Accepted values are first validated in a private candidate workbook, then the sheet model and all
values enter live history together. Discard keeps only the model. The native form declares its multiline
field indices through `input_multiline_fields`; existing first-field multiline forms retain their behavior.

Windows explicit proofing uses the installed `ISpellChecker` providers in the UI's existing COM apartment. It
checks an immutable GT_RAWTEXT UTF-16 projection, masks structural delimiters without changing positions and
validates returned ranges. Dialogs expose only installed languages and readonly source words. A correction
maps the captured UTF-16 text to the authoritative paragraph/table runs and prepares a sealed model change;
font substitutions from exported native RTF are not imported. The host loads and validates the new projection
before committing the prepared change, suppressing generated control notifications. Refusal discards it and
restores the prior native projection and selection. DOC retains source run styles/links; PPT retains its exact
style precision and object geometry. No dictionary writes or automatic correction are performed; Skip All is
limited to that checker session.

Shared Windows panel visibility belongs to each native window, separately from document history. It combines
application availability with user visibility, and the existing DPI layout reclaims sidebar/toolbar/status/ruler
space. `Window Layout` preferences persist the visibility flags with the normal placement; default flags are
omitted to retain the old placement representation. Generated UI controls still exist while hidden, so their
commands and document models remain available. Menu checks reflect availability and current visibility.

PDF header/footer font programs are bounded immutable values, shared across decoration history and batch
workers. Windows GDI resolves the requested family/style to its actual TrueType bytes; substitution to a
missing family is refused. The shared PDF font layer validates outline-embedding permissions and preserves
full programs when subsetting is prohibited. Embedded metrics and Unicode maps come from that face. A
process-local program identity separates glyph-subset caches; actual data equality defines settings equality.
The default Noto Sans path stays available without system-font access. This font-program boundary does not
implement new shaping or font fallback behavior. Machine-readable embedding rules follow the
[OpenType OS/2 specification](https://learn.microsoft.com/typography/opentype/spec/os2).

### Windows sparse worksheet projection

`Workbook::display_cells` and `calculated_cells` calculate once and return occupied/spilled coordinates;
formatting, safe external caches and typed booleans keep the existing display semantics. Filtering, clipboard,
labels, charts, pivots, validation and printing consume sparse maps. Clipboard rectangles retain an independent
100,000-cell operation bound. Dense projections are test-only helpers; production consumers use sparse maps.
`App::sparse_table` supplies an immutable `SparseTable` with shared text storage and explicit dimensions;
legacy `table` adapters remain available. The host caches the chosen source on refresh, shows/layouts the grid
for either representation, and painting reads missing coordinates as empty text. The XLS physical row map is
shared, sorted and binary searched; hidden/filtered rows keep their physical editing/clipboard coordinates.
This removes empty-cell text allocation; the shared sparse axes below handle native grid geometry.

`App::grid_axes` optionally supplies sparse `GridAxis` geometry: default length plus overrides and cumulative
64-bit deltas in shared immutable storage. Prefix/hit queries binary search; visible traversal skips zero-length
runs and visits only cells intersecting the frozen/scrolled viewport. XLS translates physical size overrides
through its existing row map. Row labels are requested only while painting visible headers. The grid keeps a
64-bit origin, uses clipped 32-bit GDI pixels, and scales only the native scrollbar thumb when total geometry
exceeds the OS range; both scrollbar endpoints retain exact logical coordinates. Existing custom grids can use
the bounded legacy length adapter. Geometry/hit/merge/resize/editor/picture paths use the same axes.

External XLSX values and shared-formula text are read through Calamine's pinned `worksheet_cells_reader`
stream, one actual XML cell record at a time. No worksheet-range matrix is constructed. Types, dates,
modern error substitutions, per-cell formula caches, layout imports and original-package preservation retain
existing policy; record count and cumulative input text are checked before model insertion. XLS retains its legacy reader; ODS uses the sparse record import described below.

ODS import visits namespace-qualified XML cell records and expands only meaningful bounded repetitions
(up to one million records, four MB cumulative text). Empty repeated rows/columns advance coordinates.
ODF text spaces, tabs, line breaks and paragraphs are decoded explicitly with a bounded depth. Style/merge
projection and unedited-style verification visit actual coordinates; structural style shifts visit explicit styles.
Value-only ODS changes with unchanged sheet layout/styles and no active filter use the existing surgical XML
patcher extended with typed inputs. Unedited rich text, metadata and ZIP parts remain byte-for-byte; scalar
and dynamic-array caches still refresh through the common pipeline. Layout/style/structural/filter edits retain
the existing reconstruction and preservation guards. Missing XML attribute namespaces are introduced locally,
and nonexistent attribute removals need no namespace declaration. Newly materialized gaps use ODF repetition.


Worksheet capacity is now 1,048,576 by 16,384. It is independent of one-million stored-cell/four-MB text
budgets and the 100,000-cell rectangle-operation budget. Copy/paste/style/fill/CSV and native print
check operation area before expansion; native merge encoders have the same bound. Full-range validation
visits stored coordinates, checking implicit blanks algebraically. Chart series retain the old 1,000-point
maximum independently of sheet dimensions. Dense text APIs compile only for core tests; examples and app
consumers use sparse projections. Grid refresh brings a changed focus into view without snapping ordinary
scrolling back to an unchanged focus; headers fit seven-digit row numbers. Viewport expansion and full-column formula aggregation are described below.

The Windows XLS open boundary validates stored editor inputs from `Workbook::cell_inputs` across all
sheets before replacing the live session. It does not iterate the logical worksheet rectangle; implicit
blanks cannot contain unsupported control characters. Refusal preserves the previous document and caches.

Reference aggregates traverse the union of stored nonempty coordinates and ordered spill entries, in row-major
order, charging stored-data scans against the common calculation budget. SUM/AVERAGE/MIN/MAX/COUNT/COUNTA/
PRODUCT and aggregate math retain their typed range rules without visiting implicit blanks. COUNTBLANK uses
reference area minus calculated nonempty coordinates, including empty-string formula results and spill values.
Other operations still enforce their materialization budgets. This does not add whole-column reference syntax.

Native XLSX literal error inputs are emitted as formatted placeholders and corrected through the existing typed
cache XML writer to standard `t="e"` cells without formulas. Supported legacy/modern codes retain their exact
raw errors; unrecognized literal XLSX error codes are refused. Native ODS literal errors are tagged by the shared
surgical typed-input patcher. Ordinary error-looking strings stay strings. The disposable Calamine working copy
also normalizes GETTING_DATA before restoring the original code, and the evaluator recognizes the supported
modern errors without changing ERROR.TYPE's explicit mappings.

Grid selection/frozen-row queries binary-search the shared physical-row map. Picture moves use the same
sparse 64-bit axes as painting and hit testing for both prefix positions and destination lookup; no per-row
walk is needed to move an object near the bottom of a large sheet. The existing visible-anchor/point-unit
policy and one-transaction history remain intact.

Whole-axis reference parsing accepts optional absolute markers, quoted sheet qualifiers and global Excel
row/column endpoints. Valid formula references into unallocated blank area evaluate as blanks/zero without
changing sheet data/extent; past-capacity references still fail. INDEX's reference form resolves its source
rectangle before materializing only the chosen result; array-valued first arguments retain the array path.
INDIRECT accepts whole-axis address text while still refusing arbitrary expressions. ODF conversion retains
whole-axis operands in bracket notation, including row-only endpoints. Native Office writers and importers
use the standard activeTab/ActiveTable metadata, independently of the private snapshot. External tab-state
saves surgically update the first workbook view and its sheet selection flags; ODS reads/writes ActiveTable
by namespace URI and creates missing settings with a manifest entry. Other views, cell XML, styles and
unknown ZIP members remain unchanged. Same-active-sheet reordering preserves existing grouping flags.
Existing allocated-coordinate editing bounds and array budgets remain.

Conditional reference aggregates collect the union of occupied relative offsets from all criteria and the
optional target, including spill coordinates. Counting adds the area of omitted offsets only when all
criteria accept a physical blank. Summing/averaging/extrema inspect typed occupied target values; numeric
counts exclude blank/text/logical targets, while selected errors propagate. Criteria use computed emptiness,
so formula-empty strings and nonempty spill cells are distinguished. Work for stored traversal, predicates
and wildcard matching shares the existing evaluation budget. SUMIF/AVERAGEIF target anchoring follows the
criteria dimensions with global bounds; IFS validates equal shapes before traversal.

Windows blank-grid growth is per-document viewport state, keyed by sheet identity. Scroll/keyboard edges
request extra rows/columns through App::extend_grid_view; the native host refreshes its sparse projection
without mutating document data or history. Out-of-allocation selections remain in the viewport until edited.
Workbook::edit_expanding runs an edit against a private bounded state and commits dimensions/selection/data
as one history step. Errors and no-ops preserve the original state and redo branch. Mapped typed/TSV paste
validates global mapping axes, then grows to the actually used destinations in its existing atomic edit.
External ODS positive blank allocation growth does not shift references; value-only saves normalize this
allocation difference when choosing the surgical XML path. Shrinking/structural changes keep their guards.

Native grid snapshots now carry a document/sheet identity independent of ordinary edits and focus
coordinates. The grid resets scroll and transient drag state on identity changes before extending blank
space or revealing the selected cell. Successful Open/New advance the document generation; rejected Open
preserves it, and sheet identity distinguishes same-focus tab changes. A small sheet's scroll clamping is
not relied on as a document-reset mechanism. Range commands use bounded growth transactions; mapped
serial output grows only its actual destinations.

A lexical formula normalization layer shares the macOS Russian alias table. It supplies disposable
canonical text to evaluation, spill discovery and standard Office writers without replacing authoritative
editor input. Semicolons become argument separators only outside inline arrays; quoted strings/sheets and
structured headers are copied intact. Unicode unquoted sheet prefixes are quoted for standard output.
Error constants are normalized before ODF reference tokenization (OpenFormula 1.3 §5.12). Tests inspect
independent packages and retain exact original-state Undo for external edits.

Stable sorting uses typed keys from one bounded evaluation session. Nonempty-key line indexes are ordered
explicitly; omitted all-blank lines map algebraically into the stable trailing group. Only stored cells move,
including styles and relative formula inputs. This removes rectangular materialization from sorting. Rows
and columns share the same permutation, with optional headers, multiple directions, natural ASCII number
segments and custom priorities. Key slots and evaluation work are independently bounded; failed rules,
merged/array/spill blocks and unsupported external sources leave the original history untouched. The basic
sort dialog stays compact; advanced options have their own menu dialog.

Common ODS numeric formats are encoded as standard number-style/percentage-style definitions with
explicit decimal/minimum digit/grouping and en-US punctuation. Native cell styles reference deduplicated
definitions; numeric values remain unrounded while numeric text caches are formatted independently.
Import resolves namespace-qualified definitions and parent number styles. Surgical input/array/scalar cache
updates carry number-format context, keeping percentage type and display text separate from numeric data.
Formatting changes are bounded by XML size and unique-style counts; unsupported code families remain
guarded. Unchanged style definitions and unknown ZIP parts survive value-only external saves.
