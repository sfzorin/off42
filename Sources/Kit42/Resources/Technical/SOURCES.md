# Technical assets used by Suite42

## Product marks

`marks.json` and the 97 mark artworks (SVG, and OSHA GHS pictograms as PNG) were imported on 2026-11-25 from the
local Lably project (`public/marks/`, registry `src/lib/compliance/marks.ts` and `localization.ts`, commit 446aafb).
Files are copied byte-for-byte. Lably entries that shared one drawing were merged, so each `id` is the file name;
the 14 ids shipped earlier (protective-earth … fan-cooled) are unchanged and existing documents keep resolving.
Names, descriptions, regions and warnings come from Lably in en, ru, tr, de, fr, es, it, zh-Hans, zh-Hant, ko, ja;
other interface languages show English. Provenance, checksums and source pages for official artwork are in
`LABLY-SOURCES.md` (Lably's `SOURCES.md`).

All marks are insertable, including 10 drawings Lably withheld (`unverified: true`: CSA, PSE, ISO 9001,
Green Dot, USB, Class III, and others). For those, and for trademarks, conformity marks and marks that need a
registration/licence number, the dialog shows the use conditions under the preview. Inserting a mark is artwork
only: it does not certify a product, establish its safety class, or grant the right to use a certification mark
or trademark. Some withheld drawings are known not to match the official artwork (see `LABLY-SOURCES.md`).

Regenerate the catalogue from Lably with `scripts/import-lably-marks/` (see its README). Three Lably drawings that
are not the mark at all are left out (solid square “Li-ion warning”, bare oval “Sterile”, filled triangle “MR
Conditional”); `etl.png` is a WebKit render of Lably's `etl.svg`, whose clip paths NSImage cannot draw.

## Barcode encoder

`bwip-js-min.js` is bwip-js 4.11.4 (2026-08-19), copied from Lably's installed
package. The script includes Barcode Writer in Pure PostScript 2026-05-28 and
its original copyright/license banner. It executes locally in Apple's
JavaScriptCore, with no browser, network access or external service. Only its
raw encoder output is consumed; text and vectors are drawn by AppKit/CoreGraphics.
`LICENSE.bwip-js` and `LICENSE.INCONSOLATA` preserve the package/font notices.
