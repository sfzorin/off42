# Product marks used by the Windows apps

`marks.json` and the 97 artworks are the same catalogue the macOS apps ship in
`Sources/Kit42/Resources/Technical/` (imported from the Lably project with
`scripts/import-lably-marks/`; copy both together when it changes). Ids are file
names, so recipes made on either platform resolve to the same mark; the 14 symbol
ids shipped earlier are unchanged and byte-identical.

87 marks are SVG and are placed in PDF as vector paths. Ten are raster (OSHA GHS
pictograms copied byte-for-byte from OSHA, and a WebKit render of Lably's ETL file)
and are placed as images. Names, descriptions, regions and conditions of use come
from Lably in 11 languages; other interface languages show English.

Inserting a mark is artwork only: it does not certify a product, establish a safety
class, or grant the right to use a certification mark or trademark. Entries Lably
withheld are included at the owner's request and flagged `unverified`; the insert
command shows each mark's conditions of use before placing it. Provenance and
official-source checksums are in `LABLY-SOURCES.md`.
