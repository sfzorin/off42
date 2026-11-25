// Converts the Lably registry (marks.json from export.ts) into Kit42's Technical/marks.json and copies the artwork.
import {readFileSync, writeFileSync, copyFileSync} from 'node:fs';
const all = JSON.parse(readFileSync(new URL('./marks.json', import.meta.url)));
const SRC = process.env.LABLY_DIR + '/public/marks/', DST = new URL('../../Sources/Kit42/Resources/Technical/', import.meta.url).pathname;
const byFile = new Map();
// Drawings that are not the mark at all (solid square, bare oval, filled triangle) are left out.
const BROKEN = new Set(['li-ion-warning.svg', 'sterile.svg', 'mr-conditional.svg']);
// NSImage cannot draw this Inkscape file's clip paths: ship a WebKit (Quick Look) render at ~1000 px instead.
const RASTER = {'etl.svg': 'etl.png'};
// One entry per artwork file; ids are file names, so the 14 earlier symbol ids keep working.
// Several Lably entries can share one drawing. Prefer verified (selectable) entries, then the precise name for the
// drawing (Class II for insulation-class.svg), then the entry whose id names the file (Protective_Earth, not Earth_ground).
const PREFERRED = new Set(['Class_II']);
const named = m => m.id.toLowerCase().replace(/_/g, '-') === m.url.split('/').pop().replace(/\.(svg|png)$/, '');
const rank = m => (m.selectable === false ? 4 : 0) + (PREFERRED.has(m.id) ? 0 : 2) + (named(m) ? 0 : 1);
for (const m of [...all].sort((a, b) => rank(a) - rank(b))) {
  const file = m.url.split('/').pop(); if (byFile.has(file) || BROKEN.has(file)) continue;
  byFile.set(file, m);
}
const out = [];
for (let [file, m] of byFile) {
  if (RASTER[file]) { copyFileSync(new URL('./' + RASTER[file], import.meta.url), DST + RASTER[file]); file = RASTER[file]; }
  else copyFileSync(SRC + file, DST + file);
  const copy = {};
  for (const [lang, c] of Object.entries(m.copy)) copy[lang] = Object.fromEntries(Object.entries({name: c.label, description: c.description, warning: c.warning, category: c.category, region: c.region}).filter(([, v]) => v));
  out.push({id: file.replace(/\.(svg|png)$/, ''), file, lablyID: m.id, category: m.category, legal: m.legal,
            registrationNumber: !!m.regNumberRequired, unverified: m.selectable === false, aspect: +(m.w / m.h).toFixed(4), copy});
}
const order = ['certification', 'safety', 'environmental', 'hazardous', 'handling', 'wireless', 'medical', 'quality'];
out.sort((a, b) => order.indexOf(a.category) - order.indexOf(b.category));
writeFileSync(DST + 'marks.json', JSON.stringify(out, null, 1) + '\n');
console.log(out.length, 'marks;', out.filter(m => m.unverified).length, 'unverified;', Object.entries(out.reduce((c, m) => (c[m.category] = (c[m.category] || 0) + 1, c), {})).join(' '));
