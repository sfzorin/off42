# DOC42 layout feasibility probe

This offline experiment shapes a fixed fixture with cosmic-text and rasterizes it to PNG pages. It is not a DOCX importer, an editor, or a complete pagination engine. It lays out Latin/Cyrillic, combining marks and ligatures, followed by a two-column table and a final paragraph. Table rows remain together; a row that cannot fit is rejected. Unsupported glyphs and ink outside a line box are errors. The trace records UTF-8 byte ranges relative to each named paragraph or cell, including spaces consumed by wrapping. Layout rejects non-contiguous ranges. Width and font-size changes exercise reflow; page dimensions and margins are fixed.

Run from the Rust workspace:

```
cargo run -p doc42-layout-probe -- /tmp/doc42-pages 450 18
```

Arguments are output directory, content width (160–536 pixels), and font size (10–40 pixels). The output directory must be empty or absent. The fixture is bounded to 48,000 bytes and 64 pages. Trace coordinates and images use pixel units. Output is deterministic for the pinned dependency versions and fixture font; exact pixels across different platform rasterizers are not promised. This experiment supports only nonempty, single-line source paragraphs and two-cell rows; it does not support arbitrary input, bidi paragraph pagination, splitting table rows, font fallback, headers, footers, editing, printing, or DOCX. A missing glyph is an error, not coverage.

`fixtures/NotoSans.ttf` is Noto Sans from <https://github.com/google/fonts/tree/main/ofl/notosans>, downloaded as `NotoSans[wdth,wght].ttf`. It is distributed under the SIL Open Font License 1.1; see `fixtures/OFL.txt`. The font is embedded to avoid dependence on installed fonts.
