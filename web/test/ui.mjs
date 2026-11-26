// Headless browser test of the production build: serves it at /app/ with the headers off42.cc sends
// (website/public/_headers, including the site's Content-Security-Policy), drives the page in headless Chromium
// (Playwright) and checks the loading screen, every section (documents, spreadsheets, presentations, PDF, labels &
// codes, signatures & stamps), conversions and downloads, language selection and right-to-left layout, and that
// the page makes no request outside its own origin. Run after `npm run build`; pass `--dir ../website/public/app`
// to test the copy published on the site, `--url <app URL>` to test an app served elsewhere (for example
// `npx wrangler dev` in website/), and `--screenshots <dir>` to save screenshots of the main screens.
// CHROMIUM_PATH may point at an existing headless Chromium; otherwise Playwright's own build is used
// (`npx playwright install chromium`).
import { mkdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';
import assert from 'node:assert/strict';
import { serveApp } from './serve.mjs';
import { chromium } from 'playwright';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const fixtures = join(root, 'test', 'fixtures');
const arg = (name) => (process.argv.includes(name) ? process.argv[process.argv.indexOf(name) + 1] : undefined);
const dir = resolve(arg('--dir') ?? join(root, 'dist'));
const shots = arg('--screenshots') ? resolve(arg('--screenshots')) : undefined;
if (shots) mkdirSync(shots, { recursive: true });
const server = arg('--url') ? { origin: new URL(arg('--url')).origin, close: async () => {} }
  : await serveApp(dir, join(root, '..', 'website', 'public', '_headers'));
const origin = server.origin;
const app = arg('--url') ?? `${origin}/app/`;
console.log(`testing ${app}${arg('--url') ? '' : ` (serving ${dir})`}`);
const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM_PATH || undefined });
let failed = false;
const foreign = [];
const errors = [];

async function newPage(options = {}) {
  const context = await browser.newContext({ acceptDownloads: true, viewport: { width: 1360, height: 860 }, ...options });
  const page = await context.newPage();
  page.on('request', (request) => {
    const url = request.url();
    if (!url.startsWith(origin) && !/^(data|blob|about):/.test(url)) foreign.push(url);
  });
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('console', (msg) => msg.type() === 'error' && errors.push(msg.text()));
  return page;
}

// Waits until the app is ready and returns what the loading screen shows at that moment (it fades out afterwards).
const ready = async (page) => (await page.waitForFunction(() => document.body.dataset.ready === 'true' && {
  text: document.getElementById('splash-text')?.textContent, value: document.getElementById('splash')?.getAttribute('aria-valuenow'),
}, null, { timeout: 90_000 })).jsonValue();
const shoot = async (page, name) => {
  if (!shots) return;
  await page.waitForSelector('#splash', { state: 'detached' });
  await page.screenshot({ path: join(shots, `${name}.png`) });
};
const step = async (name, fn) => {
  await fn();
  console.log(`ok   ${name}`);
};

try {
  const page = await newPage();
  const open = async (section, file, buffer) => {
    const input = `#file-input-${section}`;
    await page.setInputFiles(input, buffer ? { name: file, mimeType: 'application/octet-stream', buffer } : join(fixtures, file));
  };
  const opened = (section, file) => page.waitForSelector(`#${section} .section-title h1:text-is("${file}")`);
  const downloadBytes = async (click) => {
    const [download] = await Promise.all([page.waitForEvent('download'), click()]);
    return { name: download.suggestedFilename(), bytes: readFileSync(await download.path()) };
  };
  const save = (section, format) => downloadBytes(() => page.click(`#${section} .toolbar button:has(span:text-is("${format}"))`));

  await step('loading screen with progress, then the engine runs in a worker under /app/ with the site headers', async () => {
    const wasm = page.waitForResponse((r) => r.url().endsWith('.wasm'));
    const response = await page.goto(app);
    assert.match(response.headers()['content-security-policy'] ?? '', /script-src 'self' 'wasm-unsafe-eval'/);
    const splash = await ready(page);
    assert.deepEqual(splash, { text: 'Loading the OFF42 engine… 100%', value: '100' });
    assert.equal((await wasm).headers()['content-type'], 'application/wasm');
    assert.ok(page.workers().length >= 1, 'the engine runs in a Web Worker');
    assert.match(await page.textContent('#engine-version'), /\d+\.\d+\.\d+/);
    assert.equal(await page.getAttribute('html', 'lang'), 'en');
    await page.waitForSelector('#splash', { state: 'detached' });
    // Documents is the default section, with an empty state.
    await page.waitForSelector('#documents .dropzone h2:text-is("Open a document")');
    assert.equal(await page.getAttribute('.nav-item[data-section="documents"]', 'aria-current'), 'page');
    await shoot(page, '01-documents-empty');
  });

  await step('Documents: Markdown as a page with counts, saved as DOCX; DOCX → ODT asks before dropping parts', async () => {
    await open('documents', 'sample.md');
    await opened('documents', 'sample.md');
    // The preview frame runs no scripts (sandboxed, same origin), so it is inspected from the page.
    await page.waitForFunction(() => document.querySelector('#documents iframe.paper')?.contentDocument?.body
      ?.textContent.includes('Quarterly notes'));
    assert.ok((await page.locator('#documents iframe.paper').boundingBox()).height >= 1000, 'A4 page height');
    assert.match(await page.textContent('#documents .statusbar'), /\d+ words · \d+ characters/);
    // Links in the document cannot navigate the frame away from the preview.
    assert.equal(await page.evaluate(() => getComputedStyle(document.querySelector('#documents iframe.paper')
      .contentDocument.querySelector('a')).pointerEvents), 'none');
    await shoot(page, '02-document');
    const docx = await save('documents', 'DOCX');
    assert.equal(docx.name, 'sample.docx');
    assert.equal(docx.bytes.subarray(0, 2).toString(), 'PK');
    await page.waitForSelector('.toast:has-text("Saved sample.docx")');
    for (const format of ['ODT', 'RTF', 'MD', 'HTML']) {
      assert.ok((await save('documents', format)).bytes.length > 0, format);
    }
    await open('documents', 'sample.docx', docx.bytes);
    await opened('documents', 'sample.docx');
    await page.click('#documents .toolbar button:has(span:text-is("ODT"))');
    await page.waitForSelector('#confirm[open]');
    assert.match(await page.textContent('#confirm-text'), /package parts/);
    const [odt] = await Promise.all([page.waitForEvent('download'), page.click('#confirm-ok')]);
    assert.equal(readFileSync(await odt.path()).subarray(0, 2).toString(), 'PK');
    assert.equal(await page.locator('#recent-list li').count(), 2, 'recent files in the sidebar');
  });

  await step('Spreadsheets: CSV in a grid with headers, cell selection and sheet tabs; saved as XLSX and reopened', async () => {
    await page.click('.nav-item[data-section="spreadsheets"]');
    await page.waitForSelector('#spreadsheets .dropzone');
    await open('spreadsheets', 'sample.csv');
    await opened('spreadsheets', 'sample.csv');
    await page.waitForSelector('#spreadsheets .grid td:text-is("12")');
    assert.equal(await page.textContent('#spreadsheets .grid thead th:nth-child(2)'), 'A');
    assert.ok(await page.locator('#spreadsheets .grid td.num').count() > 0, 'numbers are right-aligned');
    const cell = page.locator('#spreadsheets .grid td:text-is("12")');
    await cell.click();
    assert.match(await page.textContent('#spreadsheets .namebox'), /^[A-Z]+\d+$/);
    assert.equal(await page.textContent('#spreadsheets .valuebox'), '12');
    assert.equal(await page.locator('#spreadsheets .sheet-tabs button').count(), 1);
    await shoot(page, '03-spreadsheet');
    const xlsx = await save('spreadsheets', 'XLSX');
    assert.equal(xlsx.bytes.subarray(0, 2).toString(), 'PK');
    assert.ok((await save('spreadsheets', 'ODS')).bytes.length > 0);
    await open('spreadsheets', 'sample.xlsx', xlsx.bytes);
    await opened('spreadsheets', 'sample.xlsx');
    await page.waitForSelector('#spreadsheets .grid td:text-is("12")');
    const csv = await downloadBytes(() => page.click('#spreadsheets .toolbar button:has-text("Sheet values as CSV")'));
    assert.match(csv.bytes.toString(), /12/);
  });

  await step('a file opened in the wrong section goes to its own (CSV from Documents → Spreadsheets)', async () => {
    await page.click('.nav-item[data-section="documents"]');
    await open('documents', 'sample.csv');
    await page.waitForURL(/#spreadsheets$/);
    await opened('spreadsheets', 'sample.csv');
  });

  await step('Presentations: thumbnails, 16:9 slide, notes, keyboard navigation; PPTX → ODP → reopened', async () => {
    await page.click('.nav-item[data-section="presentations"]');
    await open('presentations', 'sample.pptx');
    await opened('presentations', 'sample.pptx');
    assert.equal(await page.locator('#presentations .thumb').count(), 2);
    await page.waitForSelector('#presentations .slide-stage h2:text-is("Quarterly review")');
    assert.equal(await page.textContent('#presentations .notes-text'), 'Thank the team.');
    const box = await page.locator('#presentations .slide-stage .slide-card').boundingBox();
    assert.ok(Math.abs(box.width / box.height - 16 / 9) < 0.02, 'slides are 16:9');
    await shoot(page, '04-presentation');
    await page.keyboard.press('ArrowDown');
    await page.waitForSelector('#presentations .slide-stage h2:text-is("Next steps")');
    assert.match(await page.textContent('#presentations .statusbar'), /Slide 2 of 2/);
    const odp = await save('presentations', 'ODP');
    assert.equal(odp.bytes.subarray(0, 2).toString(), 'PK');
    await open('presentations', 'sample.odp', odp.bytes);
    await opened('presentations', 'sample.odp');
    assert.equal(await page.locator('#presentations .thumb').count(), 2);
  });

  await step('PDF: pages, properties, bookmark tree, attachments and form fields; page view marked as coming', async () => {
    await page.click('.nav-item[data-section="pdf"]');
    await open('pdf', 'sample.pdf');
    await opened('pdf', 'sample.pdf');
    await page.waitForSelector('#pdf .facts dd:text-is("Quarterly report")');
    assert.equal(await page.textContent('#pdf .facts dd'), '2');
    assert.equal(await page.textContent('#pdf .tree > li > ul > li'), 'Revenue', 'nested bookmark');
    await page.waitForSelector('#pdf .plain-list li:text-is("notes.txt")');
    await page.waitForSelector('#pdf .fields td:text-is("Approved by")');
    await page.waitForSelector('#pdf .coming .pill:text-is("Coming soon")');
    assert.equal(await page.locator('#pdf button:has-text("Save")').count(), 0, 'no save buttons for PDF');
    await shoot(page, '05-pdf');
    await page.click('#pdf button[aria-label="Close"]');
    await page.waitForSelector('#pdf .dropzone h2:text-is("Open a PDF")');
    await page.waitForSelector('#pdf .recent-card:has-text("sample.pdf")');
  });

  await step('Labels & codes: live code preview, PNG/SVG, label sheet preview and PDF, product marks', async () => {
    await page.click('.nav-item[data-section="labels"]');
    await page.waitForSelector('#code-preview img');
    const first = await page.getAttribute('#code-preview img', 'src');
    await page.fill('#code-value', 'OFF42-0001');
    await page.waitForFunction((src) => document.querySelector('#code-preview img')?.getAttribute('src') !== src, first);
    await page.click('#code-format button[data-value="code128"]');
    await page.waitForFunction(() => document.querySelector('#code-format [aria-checked="true"]')?.dataset.value === 'code128');
    await shoot(page, '06-codes');
    const png = await downloadBytes(() => page.click('#code-png'));
    assert.equal(png.bytes.subarray(1, 4).toString(), 'PNG');
    const svg = await downloadBytes(() => page.click('#code-svg'));
    assert.match(svg.bytes.toString(), /<svg/);
    await page.fill('#code-value', '');
    await page.waitForSelector('#code-preview .callout.error');

    await page.click('[data-tabs="labels"] [data-pane="sheets"]');
    await page.waitForSelector('#sheet-preview .slot');
    assert.equal(await page.locator('#sheet-preview .slot.filled').count(), 4, 'Box 1 ×2, Box 2, Spare parts');
    await page.fill('#sheet-first', '3');
    await page.waitForFunction(() => document.querySelectorAll('#sheet-preview .slot.used').length === 2);
    await shoot(page, '07-label-sheet');
    const pdf = await downloadBytes(() => page.click('#sheet-pdf'));
    assert.equal(pdf.bytes.subarray(0, 4).toString(), '%PDF');
    assert.match(await page.textContent('#sheet-status'), /4 labels on 1 page/);

    await page.click('[data-tabs="labels"] [data-pane="marks"]');
    await page.waitForFunction(() => document.querySelectorAll('.mark-tile img').length > 50);
    const all = await page.locator('.mark-tile').count();
    await page.locator('#marks-category button').nth(1).click();
    assert.ok((await page.locator('.mark-tile').count()) < all, 'category filter');
    await page.locator('#marks-category button').first().click();
    await page.fill('#marks-search', 'CE');
    await page.locator('.mark-tile').first().click();
    await page.waitForSelector('#marks-detail h3');
    await page.waitForSelector('#marks-detail .mark-preview img');
    await shoot(page, '08-marks');
    const markPng = await downloadBytes(() => page.click('#marks-detail button:has-text("Download PNG")'));
    assert.equal(markPng.bytes.subarray(1, 4).toString(), 'PNG');
    await page.fill('#marks-search', 'zzzz-no-such-mark');
    await page.waitForSelector('#marks-grid .empty-line');
  });

  await step('Signatures & stamps: six variants, pen and ink, new impressions, live stamp editor, PNG downloads', async () => {
    await page.click('.nav-item[data-section="signatures"]');
    await page.waitForFunction(() => document.querySelectorAll('#sig-results .signature-paper img').length === 6);
    const before = await page.getAttribute('#sig-results .signature-paper img', 'src');
    await page.click('#sig-ink button[data-value="green"]');
    await page.click('#sig-pen button[data-value="2"]');
    await page.waitForFunction((src) => document.querySelector('#sig-results .signature-paper img')?.getAttribute('src') !== src, before);
    await page.waitForFunction(() => document.querySelectorAll('#sig-results .signature-paper img').length === 6);
    await shoot(page, '09-signatures');
    const one = await page.getAttribute('#sig-results .signature-paper img', 'src');
    await page.locator('#sig-results .variant').first().locator('button[aria-label="New impression"]').click();
    await page.waitForFunction((src) => document.querySelector('#sig-results .signature-paper img')?.getAttribute('src') !== src, one);
    const sig = await downloadBytes(() => page.locator('#sig-results .variant').nth(2).locator('button[aria-label="Download PNG"]').click());
    assert.equal(sig.bytes.subarray(1, 4).toString(), 'PNG');
    await page.click('#sig-generate');
    await page.waitForFunction(() => document.querySelectorAll('#sig-results .signature-paper img').length === 6);

    await page.click('[data-tabs="signatures"] [data-pane="stamp"]');
    await page.waitForSelector('#stamp-preview img');
    const stamp = await page.getAttribute('#stamp-preview img', 'src');
    await page.locator('#stamp-fields input, #stamp-fields textarea').first().fill('OFF42 TEST');
    await page.waitForFunction((src) => document.querySelector('#stamp-preview img')?.getAttribute('src') !== src, stamp);
    await shoot(page, '10-stamp');
    const png = await downloadBytes(() => page.click('#stamp-download'));
    assert.equal(png.bytes.subarray(1, 4).toString(), 'PNG');
  });

  await step('German from ?lang=: no English before the catalog, translated sections and options, picker', async () => {
    // Hold the German catalog: until it arrives the loading screen shows no text and the app stays hidden.
    let release;
    const held = new Promise((resolve) => (release = resolve));
    await page.route(/\/assets\/de-[\w-]+\.js$/, async (route) => {
      await held;
      await route.continue();
    });
    await page.goto(`${app}?lang=de#signatures`);
    await page.waitForSelector('#splash');
    await page.waitForTimeout(300);
    assert.equal(await page.textContent('#splash-text'), '');
    assert.ok(await page.locator('#app').isHidden());
    release();
    assert.equal((await ready(page)).text, 'Die OFF42-Engine wird geladen… 100 %');
    await page.unrouteAll();
    assert.equal(await page.getAttribute('html', 'lang'), 'de');
    assert.equal(await page.textContent('.nav-item[data-section="documents"] span:last-child'), 'Dokumente');
    assert.equal(await page.inputValue('#language'), 'de');
    await page.waitForSelector('#sig-pen button');
    assert.notEqual(await page.textContent('#sig-pen button'), 'Ballpoint pen');
    await Promise.all([page.waitForURL(/lang=fr/), page.selectOption('#language', 'fr')]);
    await ready(page);
    assert.equal(await page.getAttribute('html', 'lang'), 'fr');
    assert.match(page.url(), /#signatures$/, 'the section survives a language change');
  });

  await step('Arabic is right-to-left; Japanese mark names and Russian stamp names come from the cores', async () => {
    await page.goto(`${app}?lang=ar`);
    await ready(page);
    assert.equal(await page.getAttribute('html', 'dir'), 'rtl');
    const sidebar = await page.locator('.sidebar').boundingBox();
    assert.ok(sidebar.x > 600, 'the sidebar is on the right');
    await open('spreadsheets', 'sample.csv');
    await opened('spreadsheets', 'sample.csv');
    await shoot(page, '11-arabic-rtl');
    await page.goto(`${app}?lang=ja#labels`);
    await ready(page);
    await page.click('[data-tabs="labels"] [data-pane="marks"]');
    await page.waitForSelector('.mark-tile');
    assert.ok(await page.locator('.mark-name', { hasText: /[぀-ヿ一-鿿]/ }).count() > 10, 'Japanese mark names');
    await page.goto(`${app}?lang=ru#signatures`);
    await ready(page);
    await page.click('[data-tabs="signatures"] [data-pane="stamp"]');
    await page.waitForSelector('#stamp-preview img');
    assert.match(await page.textContent('#stamp-template option'), /[а-яё]/i);
    await page.context().close();
  });

  await step('browser language is the default (zh-TW → Traditional Chinese)', async () => {
    const zh = await newPage({ locale: 'zh-TW' });
    await zh.goto(app);
    await ready(zh);
    assert.equal(await zh.getAttribute('html', 'lang'), 'zh-Hant');
    await zh.context().close();
  });

  await step('dark theme follows the system; phone width uses a top app bar', async () => {
    const dark = await newPage({ colorScheme: 'dark' });
    await dark.goto(`${app}#presentations`);
    await ready(dark);
    const background = await dark.evaluate(() => getComputedStyle(document.body).backgroundColor);
    assert.notEqual(background, 'rgb(247, 246, 242)', 'dark background');
    await dark.setInputFiles('#file-input-presentations', join(fixtures, 'sample.pptx'));
    await dark.waitForSelector('#presentations .slide-stage h2');
    await shoot(dark, '12-dark-presentation');
    await dark.context().close();

    const phone = await newPage({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    await phone.goto(`${app}#documents`);
    await ready(phone);
    const bar = await phone.locator('.sidebar').boundingBox();
    assert.ok(bar.width >= 380 && bar.height < 160, `top bar ${JSON.stringify(bar)}`);
    assert.ok(await phone.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), 'no horizontal scrolling');
    await shoot(phone, '13-phone-empty');
    await phone.setInputFiles('#file-input-documents', join(fixtures, 'sample.md'));
    await phone.waitForSelector('#documents iframe.paper');
    await shoot(phone, '14-phone-document');
    await phone.context().close();
  });

  assert.deepEqual(foreign, [], 'requests outside the app origin');
  assert.deepEqual(errors, [], 'page errors');
  console.log('ok   no requests left the app origin; no page errors');
} catch (error) {
  failed = true;
  console.error(`FAIL ${error?.stack ?? error}`);
  if (errors.length) console.error(`page errors: ${errors.join('\n')}`);
} finally {
  await browser.close();
  await server.close();
}
process.exit(failed ? 1 : 0);
