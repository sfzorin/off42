// Headless smoke test: loads the generated wasm package in Node and exercises every exported operation on
// fixtures. Run after `npm run build:wasm` (see README.md). Exits non-zero on the first failure.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import assert from 'node:assert/strict';
import * as off42 from '../src/wasm/off42_wasm.js';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
off42.initSync({ module: readFileSync(join(here, '..', 'src', 'wasm', 'off42_wasm_bg.wasm')) });

const fixture = (name) => new Uint8Array(readFileSync(join(here, 'fixtures', name)));
const repoFile = (path) => new Uint8Array(readFileSync(join(repo, path)));
const starts = (bytes, prefix) => prefix.split('').every((c, i) => bytes[i] === c.charCodeAt(0));
const text = (bytes) => new TextDecoder().decode(bytes);

let passed = 0;
function check(name, fn) {
  try {
    fn();
    passed += 1;
    console.log(`ok   ${name}`);
  } catch (error) {
    console.error(`FAIL ${name}\n${error?.stack ?? error}`);
    process.exit(1);
  }
}

check('version', () => assert.match(off42.version(), /^\d+\.\d+\.\d+$/));

// Documents
const markdown = fixture('sample.md');
let docx;
check('Markdown → DOCX', () => {
  docx = off42.documentConvert(markdown, 'md', 'docx', false);
  assert.ok(starts(docx, 'PK'));
});
check('DOCX → HTML preview', () => {
  const html = off42.documentHtml(docx, 'docx');
  assert.ok(html.includes('Quarterly notes') && html.includes('Revenue'), html);
});
check('DOCX → ODT is refused without consent, allowed when dropping package parts', () => {
  assert.throws(() => off42.documentConvert(docx, 'docx', 'odt', false), /package parts/);
  assert.ok(starts(off42.documentConvert(docx, 'docx', 'odt', true), 'PK'));
});
check('Markdown → RTF and statistics', () => {
  assert.ok(text(off42.documentConvert(markdown, 'md', 'rtf', false)).startsWith('{\\rtf'));
  const info = JSON.parse(off42.documentInfo(markdown, 'md'));
  assert.ok(info.words >= 10, JSON.stringify(info));
});
check('signed LibreOffice DOCX is refused rather than silently unsigned', () => {
  assert.throws(() => off42.documentHtml(repoFile('scripts/office-signatures/fixtures/libreoffice-25.2-signed.docx'), 'docx'), /_xmlsignatures/);
});
check('RTF → HTML preview and Markdown', () => {
  const rtf = off42.documentConvert(markdown, 'md', 'rtf', false);
  assert.ok(off42.documentHtml(rtf, 'rtf').includes('Next steps'));
  assert.ok(text(off42.documentConvert(rtf, 'rtf', 'md', false)).includes('Ship the web version'));
});
check('unsupported document target is an error', () => {
  assert.throws(() => off42.documentConvert(markdown, 'md', 'pdf', false), /Unsupported document format/);
});

// Spreadsheets
const csv = fixture('sample.csv');
let xlsx;
check('CSV → table JSON with calculated formula', () => {
  const book = JSON.parse(off42.spreadsheetJson(csv, 'csv', 1000, 100));
  const total = book.sheets[0].cells.find(([r, c]) => r === 4 && c === 2);
  assert.equal(total?.[2], '12');
});
check('CSV → XLSX → table JSON and values CSV', () => {
  xlsx = off42.spreadsheetConvert(csv, 'csv', 'xlsx');
  assert.ok(starts(xlsx, 'PK'));
  const book = JSON.parse(off42.spreadsheetJson(xlsx, 'xlsx', 1000, 100));
  assert.equal(book.sheets[0].cells.find(([r, c]) => r === 4 && c === 2)?.[2], '12');
  const values = text(off42.spreadsheetValuesCsv(xlsx, 'xlsx', 0)).split('\r\n');
  assert.equal(values[4], 'Total,,12');
});
check('CSV → ODS → table JSON', () => {
  const ods = off42.spreadsheetConvert(csv, 'csv', 'ods');
  const book = JSON.parse(off42.spreadsheetJson(ods, 'ods', 1000, 100));
  assert.equal(book.sheets[0].cells.find(([r, c]) => r === 4 && c === 2)?.[2], '12');
});
check('LibreOffice XLSX → table JSON', () => {
  const book = JSON.parse(off42.spreadsheetJson(repoFile('scripts/office-signatures/fixtures/libreoffice-25.2-signed.xlsx'), 'xlsx', 1000, 100));
  assert.ok(book.sheets.length >= 1);
});

// Presentations (sample.pptx was written by ppt42-core)
const pptx = fixture('sample.pptx');
check('PPTX → slide outline', () => {
  const outline = JSON.parse(off42.presentationOutline(pptx, 'pptx'));
  assert.equal(outline.slides.length, 2);
  assert.ok(JSON.stringify(outline).includes('Quarterly review'));
  assert.ok(JSON.stringify(outline).includes('Ship the web version'));
});
check('PPTX → ODP → outline and back to PPTX', () => {
  const odp = off42.presentationConvert(pptx, 'pptx', 'odp');
  const outline = JSON.parse(off42.presentationOutline(odp, 'odp'));
  assert.equal(outline.slides.length, 2);
  assert.ok(starts(off42.presentationConvert(odp, 'odp', 'pptx'), 'PK'));
  assert.throws(() => off42.presentationOutline(new Uint8Array([1, 2, 3]), 'pptx'));
});

// PDF and labels
check('PDF fixture info', () => {
  const info = JSON.parse(off42.pdfInfo(repoFile('rust/apps/pdf42/fixtures/page-export.pdf')));
  assert.equal(info.encrypted, false);
  assert.ok(info.pages >= 1);
});
check('codes: QR PNG, Code 128 SVG', () => {
  assert.ok(starts(off42.barcodePng('qr', 'https://off42.cc', 30), '\x89PNG'));
  assert.ok(off42.barcodeSvg('code128', 'OFF42-001', 50, true).startsWith('<svg'));
  assert.throws(() => off42.barcodePng('ean13', '1', 30), /Choose QR/);
});
check('label sheet PDF from a catalogue stock', () => {
  const stocks = JSON.parse(off42.labelStocks());
  assert.ok(stocks.length > 0);
  const pdf = off42.labelSheetPdf(JSON.stringify({
    stock: stocks[0].id, codeFormat: 'datamatrix', labels: [{ text: 'Box 1', code: 'BOX-1', copies: 2 }],
  }));
  assert.ok(starts(pdf, '%PDF'));
  assert.equal(JSON.parse(off42.pdfInfo(pdf)).pages, 1);
});
check('product marks', () => {
  const marks = JSON.parse(off42.productMarks('en'));
  assert.ok(marks.find((m) => m.id === 'ce'));
  assert.ok(off42.productMarkSvg('ce', 20).startsWith('<svg'));
  assert.ok(starts(off42.productMarkPng('ce', 20), '\x89PNG'));
});

// Signatures and stamps
check('signature designs and PNG', () => {
  const options = JSON.parse(off42.signatureOptions());
  assert.ok(options.forms.length > 0 && options.pens.length > 0 && options.inks.length > 0);
  const designs = JSON.parse(off42.signatureDesigns(3, 0, '123456789'));
  assert.equal(designs.length, 3);
  const png = off42.signaturePng('Anna Smith', designs[0], '42', 'blue', 0, 130);
  assert.ok(starts(png, '\x89PNG'));
});
check('stamp templates and PNG (date supplied by the host)', () => {
  const templates = JSON.parse(off42.stampTemplates());
  assert.ok(templates.length > 10);
  let drawn = 0;
  for (const t of templates) {
    const values = Object.fromEntries(t.fields.filter((f) => f.date).map((f) => [f.key, '26/11/2026']));
    const png = off42.stampPng(t.id, JSON.stringify(values), '', '7', 0.5);
    assert.ok(starts(png, '\x89PNG'), t.id);
    drawn += 1;
  }
  assert.equal(drawn, templates.length);
});

console.log(`\n${passed} checks passed`);
