# Import product marks from Lably

Regenerates `Sources/Kit42/Resources/Technical/marks.json` and the mark artwork from a Lably checkout at
the directory named by `LABLY_DIR`.

```sh
export LABLY_DIR=/path/to/lably   # a Lably checkout
cd scripts/import-lably-marks
node --experimental-strip-types --no-warnings --import ./register.mjs export.ts > marks.json   # Lably registry + copy in 11 languages
node build-kit42.mjs                                                                             # one entry per artwork file
cp "$LABLY_DIR/public/marks/SOURCES.md" ../../Sources/Kit42/Resources/Technical/LABLY-SOURCES.md
```

`build-kit42.mjs` keeps every Lably entry (including the ones Lably withheld, flagged `unverified`), merges entries
that share a drawing, leaves out three drawings that are not the mark at all (solid square “Li-ion warning”, bare
oval “Sterile”, filled triangle “MR Conditional”), and ships `etl.png` (a Quick Look/WebKit render of Lably's
`etl.svg`, whose clip paths NSImage cannot draw). Then run `production42audit --ui` and the XLS42/DOC42/PDF42 audits.
