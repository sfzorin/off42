// Headless browser test of the production build: serves it at /app/ with the headers off42.cc sends
// (website/public/_headers, including the site's Content-Security-Policy), drives the page in headless Chromium
// (Playwright) and checks the loading screen, the app shell (drawer, language menu, help, home screens with recent
// files), every editor through its menus and toolbar (documents, spreadsheets, presentations, PDF), conversions and
// downloads, printing, creating and editing documents and spreadsheets (typing, styles, formulas, rows, Undo/Redo,
// Edited/Saved, downloads checked by reopening them through the engine), the tool screens (labels & codes, signatures & stamps), languages and right-to-left layout,
// the dark theme and the phone layout, and that the page makes no request outside its own origin.
// Run after `npm run build`; pass `--dir ../website/public/app` to test the copy published on the site,
// `--url <app URL>` to test an app served elsewhere (for example `npx wrangler dev` in website/), and
// `--screenshots <dir>` to save screenshots of the main screens.
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
  const context = await browser.newContext({ acceptDownloads: true, viewport: { width: 1440, height: 900 }, ...options });
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
  await page.waitForTimeout(250); // let entrance animations settle
  await page.screenshot({ path: join(shots, `${name}.png`) });
};
/** Asserts that two visible elements do not overlap. */
const clearOf = async (page, a, b) => {
  const [boxA, boxB] = [await page.locator(a).boundingBox(), await page.locator(b).boundingBox()];
  assert.ok(boxA && boxB, `${a} and ${b} are visible`);
  const overlap = boxA.x < boxB.x + boxB.width && boxB.x < boxA.x + boxA.width
    && boxA.y < boxB.y + boxB.height && boxB.y < boxA.y + boxA.height;
  assert.ok(!overlap, `${a} ${JSON.stringify(boxA)} overlaps ${b} ${JSON.stringify(boxB)}`);
};
const step = async (name, fn) => {
  await fn();
  console.log(`ok   ${name}`);
};

/** Helpers bound to one page. */
function driver(page) {
  const open = (section, file, buffer) => page.setInputFiles(`#file-input-${section}`,
    buffer ? { name: file, mimeType: 'application/octet-stream', buffer } : join(fixtures, file));
  // The editor is shown with the file's name (without extension) in the title row, its format badge and menus.
  const opened = (section, file) => page.waitForFunction(([section, file]) => {
    const editor = document.querySelector(`#${section} .editor:not([hidden])`);
    const dot = file.lastIndexOf('.');
    return editor && editor.querySelector('.file-name')?.value === file.slice(0, dot)
      && editor.querySelector('.format-badge')?.textContent === file.slice(dot)
      && editor.querySelector('.menubar .menubar-item') && !editor.querySelector('.editor-body > .loading');
  }, [section, file]);
  const downloadBytes = async (click) => {
    const [download] = await Promise.all([page.waitForEvent('download'), click()]);
    return { name: download.suggestedFilename(), bytes: readFileSync(await download.path()) };
  };
  /** Clicks through the menu bar: menu(section, ['File', 'Download as', 'Markdown']). */
  const menu = async (section, path) => {
    await page.click(`#${section} .menubar-item:text-is("${path[0]}")`);
    for (const label of path.slice(1)) {
      await page.locator(`.menu .menu-item:has(.menu-label:text-is("${label}"))`).last().click();
    }
  };
  const downloadAs = (section, label) => downloadBytes(() => menu(section, ['File', 'Download as', label]));
  const go = async (id) => {
    await page.evaluate((id) => (location.hash = id), id);
    await page.waitForSelector(`#${id}:not([hidden])`);
  };
  return { open, opened, downloadBytes, menu, downloadAs, go };
}

try {
  const page = await newPage();
  const { open, opened, downloadBytes, menu, downloadAs, go } = driver(page);

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
  });

  await step('Documents home: top bar, open strip with the Blank and Open tiles, empty recent list, privacy line', async () => {
    await page.waitForSelector('#documents .home .app-title h1:text-is("Documents")');
    await page.waitForSelector('#documents .strip-head h2:text-is("Open a document")');
    assert.deepEqual(await page.locator('#documents .tile').evaluateAll((tiles) => tiles.map((t) => t.getAttribute('aria-label'))),
      ['Blank document', 'Open from device'], 'Blank document first, then Open from device; no other tiles');
    assert.deepEqual(await page.locator('#documents .tile-label').allTextContents(), ['Blank document', 'Open from device']);
    await page.waitForSelector('#documents .recent-empty:text-is("Files you open appear here.")');
    await page.waitForSelector('#documents .privacy-line');
    assert.equal(await page.locator('.sidebar').count(), 0, 'no permanent sidebar');
    // The tile opens the file picker.
    const [chooser] = await Promise.all([page.waitForEvent('filechooser'), page.click('#documents .tile.open-tile')]);
    assert.equal(chooser.isMultiple(), false);
    await shoot(page, '01-documents-home');
  });

  await step('drawer: opens from ☰, traps focus, closes with Esc and with the scrim, switches apps', async () => {
    const drawer = page.locator('#drawer');
    await page.click('#documents .menu-button');
    await page.waitForSelector('#drawer[open]');
    assert.equal(await page.locator('#drawer .drawer-app').count(), 6);
    assert.equal(await page.getAttribute('#drawer .drawer-app[data-app="documents"]', 'aria-current'), 'page');
    assert.match(await page.getAttribute('#drawer-download', 'href'), /\.\.\/en\/download\/$/);
    assert.match(await page.textContent('#drawer .engine-version'), /OFF42 engine \d+\.\d+\.\d+ · WebAssembly/);
    for (let i = 0; i < 12; i++) await page.keyboard.press('Tab');
    assert.ok(await page.evaluate(() => document.getElementById('drawer').contains(document.activeElement)), 'focus stays in the drawer');
    await page.keyboard.press('Shift+Tab');
    assert.ok(await page.evaluate(() => document.getElementById('drawer').contains(document.activeElement)), 'focus stays in the drawer (backwards)');
    await shoot(page, '02-drawer');
    await page.keyboard.press('Escape');
    await drawer.waitFor({ state: 'hidden' });
    await page.click('#documents .menu-button');
    await page.waitForSelector('#drawer[open]');
    await page.mouse.click(1000, 450); // the scrim
    await drawer.waitFor({ state: 'hidden' });
    await page.click('#documents .menu-button');
    await page.click('#drawer .drawer-app[data-app="spreadsheets"]');
    await page.waitForURL(/#spreadsheets$/);
    await drawer.waitFor({ state: 'hidden' });
    await page.waitForSelector('#spreadsheets .home .app-title h1:text-is("Spreadsheets")');
    assert.ok(await page.locator('#documents').isHidden());
    await page.click('#spreadsheets .menu-button');
    await page.click('#drawer .drawer-app[data-app="documents"]');
    await page.waitForURL(/#documents$/);
  });

  await step('help opens About with the engine version; the language menu lists 20 languages with a check', async () => {
    await page.click('#documents .help-button');
    await page.waitForSelector('#about[open]');
    assert.match(await page.textContent('#about-version'), /\d+\.\d+\.\d+/);
    await page.keyboard.press('Escape');
    await page.waitForSelector('#about', { state: 'hidden' });
    await page.click('#documents .language-button');
    const items = page.locator('.menu [role="menuitemradio"]');
    assert.equal(await items.count(), 20);
    assert.equal(await page.textContent('.menu [role="menuitemradio"][aria-checked="true"]'), 'English');
    assert.equal(await page.evaluate(() => document.activeElement.textContent), 'English', 'the current language has focus');
    await page.keyboard.press('ArrowDown');
    assert.equal(await page.evaluate(() => document.activeElement.textContent), '简体中文');
    await shoot(page, '03-language-menu');
    await page.keyboard.press('Escape');
    assert.equal(await page.locator('.menu').count(), 0);
    assert.equal(await page.evaluate(() => document.activeElement.classList.contains('language-button')), true, 'focus returns to the button');
  });

  await step('Documents: Markdown on a page; Download keeps the original; Download as ▸ DOCX/ODT/RTF/Markdown/HTML', async () => {
    await open('documents', 'sample.md');
    await opened('documents', 'sample.md');
    // The document is drawn by the engine on an editable page.
    await page.waitForFunction(() => document.querySelector('#documents .doc-page[contenteditable="true"]')?.textContent
      .includes('Quarterly notes'));
    assert.ok((await page.locator('#documents .doc-page').boundingBox()).height >= 1000, 'A4 page height');
    // Links in the document cannot navigate away from the page.
    assert.equal(await page.evaluate(() => {
      const link = document.querySelector('#documents .doc-page a');
      return `${getComputedStyle(link).pointerEvents} ${link.hasAttribute('href')}`;
    }), 'none false');
    assert.deepEqual(await page.locator('#documents .menubar-item').allTextContents(), ['File', 'Edit', 'View', 'Insert', 'Format', 'Help']);
    await shoot(page, '04-document');
    const original = await downloadBytes(() => page.click('#documents .download-pill'));
    assert.equal(original.name, 'sample.md');
    assert.deepEqual(original.bytes, readFileSync(join(fixtures, 'sample.md')), 'Download gives back the file as opened');
    await page.click('#documents .menubar-item:text-is("File")');
    await page.hover('.menu .menu-item:has-text("Download as")');
    await page.waitForSelector('.menu >> nth=1');
    assert.deepEqual(await page.locator('.menu').nth(1).locator('.menu-hint').allTextContents(),
      ['.docx', '.odt', '.rtf', '.md', '.html', '.txt']);
    await shoot(page, '05-file-menu');
    await page.keyboard.press('Escape');
    await page.keyboard.press('Escape');
    const docx = await downloadAs('documents', 'Microsoft Word');
    assert.equal(docx.name, 'sample.docx');
    assert.equal(docx.bytes.subarray(0, 2).toString(), 'PK');
    await page.waitForSelector('#snackbar:has-text("Saved sample.docx")');
    const odt = await downloadAs('documents', 'OpenDocument text');
    assert.equal(odt.name, 'sample.odt');
    assert.equal(odt.bytes.subarray(0, 2).toString(), 'PK');
    assert.match(odt.bytes.toString('latin1'), /application\/vnd\.oasis\.opendocument\.text/);
    const rtf = await downloadAs('documents', 'Rich Text');
    assert.equal(rtf.name, 'sample.rtf');
    assert.match(rtf.bytes.toString(), /^\{\\rtf/);
    const md = await downloadAs('documents', 'Markdown');
    assert.equal(md.name, 'sample.md');
    assert.match(md.bytes.toString(), /Quarterly notes/);
    const html = await downloadAs('documents', 'Web page');
    assert.equal(html.name, 'sample.html');
    assert.match(html.bytes.toString(), /<h1[^>]*>(<[^>]+>)*Quarterly notes</);
  });

  await step('one snackbar at the bottom start: the next message replaces the previous one', async () => {
    await page.waitForSelector('#snackbar:has-text("Saved sample.html")');
    assert.equal(await page.locator('#snackbar .snack-text').count(), 1);
    assert.equal(await page.locator('.toast, .toasts').count(), 0);
    const box = await page.locator('#snackbar').boundingBox();
    assert.ok(box.x < 60 && box.y + box.height > 820, `bottom-left ${JSON.stringify(box)}`);
  });

  await step('Documents: statistics dialog, rename for downloads, print shows just the page, keyboard menus', async () => {
    const words = Number((await page.textContent('#documents .toolbar button[aria-label="Document statistics"]')).match(/\d+/)[0]);
    assert.ok(words > 10);
    await page.click('#documents .toolbar button[aria-label="Document statistics"]');
    await page.waitForSelector('#stats[open]');
    const stats = Object.fromEntries(await page.$$eval('#stats-list dt', (dts) => dts.map((dt) => [dt.textContent, dt.nextElementSibling.textContent])));
    assert.equal(Number(stats.Words), words);
    assert.ok(Number(stats.Characters.replace(/\D/g, '')) > words);
    assert.ok(Number(stats.Paragraphs) >= 4);
    assert.equal(stats.Format, 'MD');
    await shoot(page, '06-statistics');
    await page.click('#stats button[data-close]');
    await page.waitForSelector('#stats', { state: 'hidden' });

    await menu('documents', ['File', 'Rename']);
    assert.equal(await page.evaluate(() => document.activeElement.className), 'file-name');
    await page.keyboard.type('Quarterly notes');
    await page.keyboard.press('Enter');
    assert.equal((await downloadAs('documents', 'Microsoft Word')).name, 'Quarterly notes.docx');

    // Print: the page's own content goes to #print-root; print CSS hides the app.
    await page.evaluate(() => {
      window.print = () => {
        const host = document.querySelector('#print-root .print-document');
        window.__printed = { text: host?.shadowRoot?.textContent ?? '', printing: document.documentElement.classList.contains('printing') };
      };
    });
    await menu('documents', ['File', 'Print']);
    const printed = await page.evaluate(() => window.__printed);
    assert.ok(printed.printing && printed.text.includes('Quarterly notes') && printed.text.includes('Hire two engineers'), JSON.stringify(printed));
    await page.emulateMedia({ media: 'print' });
    assert.equal(await page.evaluate(() => getComputedStyle(document.getElementById('app')).display), 'none');
    assert.equal(await page.evaluate(() => getComputedStyle(document.getElementById('print-root')).display), 'block');
    await page.emulateMedia({ media: 'screen' });
    await page.evaluate(() => window.dispatchEvent(new Event('afterprint')));
    assert.equal(await page.locator('#print-root > *').count(), 0);

    // Zoom from the toolbar menu scales the page.
    const before = (await page.locator('#documents .doc-page').boundingBox()).width;
    await page.click('#documents .zoom-button');
    await page.click('.menu .menu-item:has-text("150%")');
    const after = (await page.locator('#documents .doc-page').boundingBox()).width;
    assert.ok(Math.abs(after / before - 1.5) < 0.02, `zoom ${before} → ${after}`);
    await menu('documents', ['View', 'Actual size']);

    // Keyboard: ArrowDown opens File with its first item, ArrowRight moves to Edit, Escape returns to the bar.
    await page.focus('#documents .menubar-item:text-is("File")');
    await page.keyboard.press('ArrowDown');
    assert.equal(await page.evaluate(() => document.activeElement.querySelector('.menu-label')?.textContent), 'New document');
    await page.keyboard.press('ArrowDown');
    assert.equal(await page.evaluate(() => document.activeElement.querySelector('.menu-label')?.textContent), 'Open…');
    await page.keyboard.press('ArrowDown');
    assert.equal(await page.evaluate(() => document.activeElement.querySelector('.menu-label')?.textContent), 'Download (.md)');
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('ArrowRight'); // into the Download as submenu
    assert.equal(await page.evaluate(() => document.activeElement.querySelector('.menu-label')?.textContent), 'Microsoft Word');
    await page.keyboard.press('ArrowLeft');
    assert.equal(await page.evaluate(() => document.activeElement.querySelector('.menu-label')?.textContent), 'Download as');
    assert.equal(await page.locator('.menu').count(), 1, 'the submenu closed');
    await page.keyboard.press('Escape');
    assert.equal(await page.locator('.menu').count(), 0);
    assert.equal(await page.evaluate(() => document.activeElement.textContent), 'File', 'focus back on the menu bar');
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('Enter');
    assert.equal(await page.getAttribute('.menu', 'aria-label'), 'Edit');
    await page.keyboard.press('ArrowRight');
    assert.equal(await page.getAttribute('.menu', 'aria-label'), 'View', 'ArrowRight in an open menu moves to the next menu');
    await page.keyboard.press('Escape');
  });

  await step('DOCX → ODT asks before dropping package parts; recent files reopen from memory and from the list', async () => {
    await open('documents', 'sample.docx', (await downloadAs('documents', 'Microsoft Word')).bytes);
    await opened('documents', 'sample.docx');
    await menu('documents', ['File', 'Download as', 'OpenDocument text']);
    await page.waitForSelector('#confirm[open]');
    assert.match(await page.textContent('#confirm-text'), /package parts/);
    const [odt] = await Promise.all([page.waitForEvent('download'), page.click('#confirm-ok')]);
    assert.equal(readFileSync(await odt.path()).subarray(0, 2).toString(), 'PK');
    // Home (app logo), recent list with both files, search, reopening from memory.
    await page.click('#documents .logo-button');
    await page.waitForSelector('#documents .home:not([hidden])');
    assert.deepEqual(await page.locator('#documents .recent-row .name').allTextContents(), ['sample.docx', 'sample.md']);
    await page.fill('#documents .search-field input', '.md');
    assert.deepEqual(await page.locator('#documents .recent-row .name').allTextContents(), ['sample.md']);
    await page.fill('#documents .search-field input', 'nothing-like-this');
    await page.waitForSelector('#documents .recent-empty:has-text("No recent files match")');
    await page.fill('#documents .search-field input', '');
    await shoot(page, '20-documents-recent');
    await page.click('#documents .recent-row:has-text("sample.md") .recent-open');
    await opened('documents', 'sample.md');
    await menu('documents', ['File', 'Close']);
    // After a reload only names, sizes and times remain: the entry asks for the file again.
    await page.reload();
    await ready(page);
    assert.deepEqual(await page.locator('#documents .recent-row .name').allTextContents(), ['sample.md', 'sample.docx']);
    assert.match(await page.getAttribute('#documents .recent-row:has-text("sample.md") .recent-open', 'title'), /Choose sample\.md again/);
    const [chooser] = await Promise.all([page.waitForEvent('filechooser'),
      page.click('#documents .recent-row:has-text("sample.md") .recent-open')]);
    await page.waitForSelector('#snackbar:has-text("Choose sample.md again")');
    await chooser.setFiles(join(fixtures, 'sample.md'));
    await opened('documents', 'sample.md');
    await page.click('#documents .logo-button');
    await page.click('#documents .recent-row:has-text("sample.docx") .recent-more');
    await page.click('.menu .menu-item:has-text("Remove from list")');
    assert.deepEqual(await page.locator('#documents .recent-row .name').allTextContents(), ['sample.md']);
  });

  await step('Spreadsheets: CSV in a full grid, selection drives the formula bar, headers and sum; conversions', async () => {
    await go('spreadsheets');
    await open('spreadsheets', 'sample.csv');
    await opened('spreadsheets', 'sample.csv');
    await page.waitForSelector('#spreadsheets .grid td:text-is("12")');
    assert.equal(await page.textContent('#spreadsheets .grid thead th:nth-child(2)'), 'A');
    assert.ok(await page.locator('#spreadsheets .grid td.num').count() > 0, 'numbers are right-aligned');
    assert.deepEqual(await page.locator('#spreadsheets .menubar-item').allTextContents(),
      ['File', 'Edit', 'View', 'Insert', 'Format', 'Data', 'Sheet', 'Help']);
    await page.locator('#spreadsheets .grid td:text-is("12")').click();
    assert.equal(await page.textContent('#spreadsheets .namebox'), 'C5');
    // The cell shows the calculated value; the formula bar shows (and edits) the formula behind it.
    assert.equal(await page.textContent('#spreadsheets td[data-ref="C5"]'), '12');
    assert.equal(await page.textContent('#spreadsheets .valuebox'), '=SUM(C2:C4)*B3+B2');
    assert.deepEqual(await page.locator('#spreadsheets .grid th.hl').allTextContents(), ['C', '5'], 'row and column headers highlighted');
    // Keyboard and range selection: the status chip sums the numbers.
    await page.keyboard.press('ArrowUp');
    assert.equal(await page.textContent('#spreadsheets .namebox'), 'C4');
    assert.equal(await page.textContent('#spreadsheets .valuebox'), '2.5');
    await page.click('#spreadsheets td[data-ref="B2"]');
    await page.click('#spreadsheets td[data-ref="C4"]', { modifiers: ['Shift'] });
    assert.equal(await page.textContent('#spreadsheets .namebox'), 'B2:C4');
    assert.equal(await page.textContent('#spreadsheets .status-chip'), 'Sum: 16 · Count: 6');
    assert.equal(await page.locator('#spreadsheets .grid td.in-range').count(), 6);
    assert.ok((await page.locator('#spreadsheets .grid-scroll').boundingBox()).width > 1400, 'full-width grid');
    assert.equal(await page.locator('#spreadsheets .sheet-tab').count(), 1);
    const xlsx = await downloadAs('spreadsheets', 'Microsoft Excel');
    assert.equal(xlsx.name, 'sample.xlsx');
    assert.equal(xlsx.bytes.subarray(0, 2).toString(), 'PK');
    const ods = await downloadAs('spreadsheets', 'OpenDocument spreadsheet');
    assert.equal(ods.name, 'sample.ods');
    assert.match(ods.bytes.toString('latin1'), /application\/vnd\.oasis\.opendocument\.spreadsheet/);
    const csv = await downloadAs('spreadsheets', 'CSV');
    assert.match(csv.bytes.toString(), /Tea/);
    await open('spreadsheets', 'sample.xlsx', xlsx.bytes);
    await opened('spreadsheets', 'sample.xlsx');
    await page.waitForSelector('#spreadsheets .grid td:text-is("12")');
    const values = await downloadAs('spreadsheets', 'Sheet values as CSV');
    assert.equal(values.name, 'sample - Sheet1.csv');
    assert.match(values.bytes.toString(), /12/);
  });

  await step('Spreadsheets: sheet tabs at the bottom, the Sheet menu and the all-sheets list switch sheets; print', async () => {
    await open('spreadsheets', 'sheets.xlsx');
    await opened('spreadsheets', 'sheets.xlsx');
    assert.deepEqual(await page.locator('#spreadsheets .sheet-tab').allTextContents(), ['November', 'October']);
    await page.waitForSelector('#spreadsheets .grid td:text-is("1000")');
    await page.click('#spreadsheets .grid td:text-is("1000")');
    assert.equal(await page.textContent('#spreadsheets .namebox'), 'C5');
    await shoot(page, '07-spreadsheet');
    await page.click('#spreadsheets .sheet-tab:text-is("October")');
    await page.waitForSelector('#spreadsheets .grid td:text-is("North")');
    assert.equal(await page.getAttribute('#spreadsheets .sheet-tab:text-is("October")', 'aria-selected'), 'true');
    assert.equal(await page.textContent('#spreadsheets .namebox'), 'A1');
    await menu('spreadsheets', ['Sheet', 'Next sheet']);
    await page.waitForSelector('#spreadsheets .grid td:text-is("Tea")');
    await page.click('#spreadsheets .sheet-bar button[aria-label="All sheets"]');
    await page.click('.menu .menu-item:has-text("October")');
    await page.waitForSelector('#spreadsheets .grid td:text-is("South")');
    const values = await downloadAs('spreadsheets', 'Sheet values as CSV');
    assert.equal(values.name, 'sheets - October.csv');
    assert.match(values.bytes.toString(), /North,120/);
    // The snackbar reporting the download stays above the sheet-tab bar (all-sheets button and first tab).
    await page.waitForSelector('#snackbar:not([hidden])');
    await clearOf(page, '#snackbar', '#spreadsheets .sheet-bar');
    await shoot(page, '07-spreadsheet-snackbar');
    await page.evaluate(() => (window.print = () => (window.__printed = document.querySelector('#print-root table.grid')?.textContent ?? '')));
    await page.click('#spreadsheets .toolbar button[aria-label="Print"]');
    assert.match(await page.evaluate(() => window.__printed), /North120/);
    await page.evaluate(() => window.dispatchEvent(new Event('afterprint')));
  });

  await step('a file opened in the wrong app goes to its own (CSV from Documents → Spreadsheets)', async () => {
    await go('documents');
    await open('documents', 'sample.csv');
    await page.waitForURL(/#spreadsheets$/);
    await opened('spreadsheets', 'sample.csv');
  });

  await step('Documents: a blank document — typing, Enter, heading, bold, list, Undo/Redo, Edited/Saved, DOCX reopened', async () => {
    await go('documents');
    await page.click('#documents .tile.blank-tile');
    await page.waitForSelector('#documents .doc-page[contenteditable="true"]');
    assert.equal(await page.inputValue('#documents .file-name'), 'Untitled document');
    assert.equal(await page.textContent('#documents .format-badge'), '.docx');
    assert.equal(await page.locator('#documents .save-state:not([hidden])').count(), 0, 'no Edited/Saved state before an edit');
    assert.ok(await page.locator('#documents .toolbar button[aria-label^="Undo"]').isDisabled());
    const paragraphs = () => page.$$eval('#documents .doc-page [data-para]', (ps) => ps.map((p) => p.textContent));
    const settle = (texts) => page.waitForFunction((texts) => JSON.stringify([...document.querySelectorAll('#documents .doc-page [data-para]')]
      .map((p) => p.textContent)) === JSON.stringify(texts), texts);
    await page.click('#documents .doc-page');
    await page.keyboard.type('Quarterly plan');
    await page.keyboard.press('Enter');
    await page.keyboard.type('We grow fast this year.');
    await page.keyboard.press('Enter');
    await page.keyboard.type('Hire people');
    await page.keyboard.press('Enter');
    await page.keyboard.type('Ship the web app');
    await settle(['Quarterly plan', 'We grow fast this year.', 'Hire people', 'Ship the web app']);
    await page.waitForSelector('#documents .save-state:text-is("Edited")');
    // Backspace and typing go through the engine too.
    await page.keyboard.press('Backspace');
    await page.keyboard.type('p!');
    await settle(['Quarterly plan', 'We grow fast this year.', 'Hire people', 'Ship the web app!']);
    // Heading 1 from the style menu on the first paragraph.
    await page.click('#documents .doc-page [data-para] >> nth=0');
    await page.click('#documents .style-button');
    await page.click('.menu .menu-item:has-text("Heading 1")');
    await page.waitForSelector('#documents .doc-page h1:has(span:text-is("Quarterly plan"))');
    assert.equal(await page.textContent('#documents .style-label'), 'Heading 1');
    // Bold on a selected word with the keyboard shortcut.
    const selectText = (needle) => page.evaluate((needle) => {
      const walker = document.createTreeWalker(document.querySelector('#documents .doc-page'), NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const at = node.data.indexOf(needle);
        if (at >= 0) return document.getSelection().setBaseAndExtent(node, at, node, at + needle.length);
      }
      throw new Error(`no ${needle}`);
    }, needle);
    await selectText('fast');
    await page.keyboard.press('ControlOrMeta+b');
    await page.waitForFunction(() => [...document.querySelectorAll('#documents .doc-page span')]
      .some((s) => s.textContent === 'fast' && s.style.fontWeight === '700'));
    assert.equal(await page.getAttribute('#documents .toolbar button[aria-label^="Bold"]', 'aria-pressed'), 'true');
    // A bulleted list over the last two paragraphs from the toolbar.
    await page.evaluate(() => {
      const ps = document.querySelectorAll('#documents .doc-page [data-para]');
      document.getSelection().setBaseAndExtent(ps[2].firstChild.firstChild, 0, ps[3].firstChild.firstChild, 4);
    });
    await page.click('#documents .toolbar button[aria-label="Bulleted list"]');
    await page.waitForFunction(() => document.querySelectorAll('#documents .doc-page ul > li').length === 2);
    await shoot(page, '22-document-editing');
    // Undo and Redo: each action is one step.
    await page.click('#documents .toolbar button[aria-label^="Undo"]');
    await page.waitForFunction(() => document.querySelectorAll('#documents .doc-page ul > li').length === 0);
    await page.click('#documents .doc-page [data-para] >> nth=1');
    await page.keyboard.press('ControlOrMeta+z');
    await page.waitForFunction(() => ![...document.querySelectorAll('#documents .doc-page span')].some((s) => s.style.fontWeight === '700'));
    await menu('documents', ['Edit', 'Redo']);
    await page.waitForFunction(() => [...document.querySelectorAll('#documents .doc-page span')].some((s) => s.textContent === 'fast' && s.style.fontWeight === '700'));
    await page.click('#documents .doc-page [data-para] >> nth=1');
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+Shift+z' : 'Control+y');
    await page.waitForFunction(() => document.querySelectorAll('#documents .doc-page ul > li').length === 2);
    // Download saves the edited document; the state turns to Saved.
    const saved = await downloadBytes(() => page.click('#documents .download-pill'));
    assert.equal(saved.name, 'Untitled document.docx');
    assert.equal(saved.bytes.subarray(0, 2).toString(), 'PK');
    await page.waitForSelector('#documents .save-state:text-is("Saved")');
    // Reopened through the engine: text, heading, bold and list survived.
    await open('documents', 'plan.docx', saved.bytes);
    await opened('documents', 'plan.docx');
    await page.waitForSelector('#documents .doc-page h1:has(span:text-is("Quarterly plan"))');
    assert.deepEqual(await paragraphs(), ['Quarterly plan', 'We grow fast this year.', 'Hire people', 'Ship the web app!']);
    assert.ok(await page.evaluate(() => [...document.querySelectorAll('#documents .doc-page span')]
      .some((s) => s.textContent === 'fast' && s.style.fontWeight === '700')), 'bold kept');
    assert.deepEqual(await page.locator('#documents .doc-page ul > li').allTextContents(), ['Hire people', 'Ship the web app!']);
    // A Word file keeps Word package parts that only DOCX can hold: HTML asks before leaving them out.
    const [download] = await Promise.all([page.waitForEvent('download'), (async () => {
      await menu('documents', ['File', 'Download as', 'Web page']);
      await page.waitForSelector('#confirm[open]');
      assert.match(await page.textContent('#confirm-text'), /package parts/);
      await page.click('#confirm-ok');
    })()]);
    assert.match(readFileSync(await download.path()).toString(), /<h1[^>]*>(<[^>]+>)*Quarterly plan/);
    // Changes that were not downloaded: closing asks first; Undo back to the downloaded state needs no question.
    await page.click('#documents .doc-page [data-para] >> nth=1');
    await page.keyboard.press('End');
    await page.keyboard.type(' Really.');
    await page.waitForSelector('#documents .save-state:text-is("Edited")');
    await menu('documents', ['File', 'Close']);
    await page.waitForSelector('#confirm[open]');
    assert.match(await page.textContent('#confirm-text'), /plan\.docx.*not downloaded/);
    await page.click('#confirm-cancel');
    assert.ok(await page.locator('#documents .editor').isVisible(), 'still open');
    await page.click('#documents .toolbar button[aria-label^="Undo"]');
    await page.waitForSelector('#documents .save-state:text-is("Saved")');
    await menu('documents', ['File', 'Close']);
    await page.waitForSelector('#documents .home:not([hidden])');
    assert.equal(await page.locator('#confirm[open]').count(), 0);
  });

  await step('Spreadsheets: a blank workbook — values, SUM, F2, formula bar, inserted row shifts the formula, Undo, XLSX reopened', async () => {
    await go('spreadsheets');
    await menu('spreadsheets', ['File', 'New spreadsheet']);
    await page.waitForFunction(() => document.querySelector('#spreadsheets .file-name')?.value === 'Untitled spreadsheet');
    assert.equal(await page.textContent('#spreadsheets .format-badge'), '.xlsx');
    const cellText = (ref) => page.textContent(`#spreadsheets td[data-ref="${ref}"]`);
    const settled = (ref, text) => page.waitForSelector(`#spreadsheets td[data-ref="${ref}"]:text-is("${text}")`);
    await page.click('#spreadsheets td[data-ref="A1"]');
    for (const value of ['10', '20', '30', '=SUM(A1:A3)']) {
      await page.keyboard.type(value);
      await page.keyboard.press('Enter');
    }
    await settled('A4', '60');
    assert.equal(await page.textContent('#spreadsheets .namebox'), 'A5', 'Enter moves down');
    await page.click('#spreadsheets td[data-ref="A4"]');
    assert.equal(await page.textContent('#spreadsheets .valuebox'), '=SUM(A1:A3)', 'the formula bar shows the formula');
    // Insert a row above row 2: the formula moves to A5 and its range grows.
    await page.click('#spreadsheets td[data-ref="A2"]');
    await page.click('#spreadsheets .toolbar button[aria-label="Row above"]');
    await settled('A5', '60');
    assert.equal(await cellText('A2'), '');
    await page.click('#spreadsheets td[data-ref="A5"]');
    assert.equal(await page.textContent('#spreadsheets .valuebox'), '=SUM(A1:A4)');
    await page.click('#spreadsheets td[data-ref="A2"]');
    await page.keyboard.type('5');
    await page.keyboard.press('Tab');
    await settled('A5', '65');
    assert.equal(await page.textContent('#spreadsheets .namebox'), 'B2', 'Tab moves right');
    // Undo twice: the value, then the row; Redo brings both back.
    await page.keyboard.press('ControlOrMeta+z');
    await settled('A5', '60');
    await page.keyboard.press('ControlOrMeta+z');
    await settled('A4', '60');
    await page.click('#spreadsheets td[data-ref="A4"]');
    assert.equal(await page.textContent('#spreadsheets .valuebox'), '=SUM(A1:A3)');
    await page.click('#spreadsheets .toolbar button[aria-label^="Redo"]');
    await page.click('#spreadsheets .toolbar button[aria-label^="Redo"]');
    await settled('A5', '65');
    // F2 edits the input in place; Delete clears; the formula bar edits the active cell.
    await page.click('#spreadsheets td[data-ref="A1"]');
    await page.keyboard.press('F2');
    await page.keyboard.type('0');
    await page.keyboard.press('Enter');
    await settled('A5', '155');
    await page.click('#spreadsheets td[data-ref="A2"]');
    await page.keyboard.press('Delete');
    await settled('A5', '150');
    await page.click('#spreadsheets td[data-ref="A3"]');
    await page.click('#spreadsheets .valuebox');
    await page.keyboard.press('ControlOrMeta+a');
    await page.keyboard.type('25');
    await page.keyboard.press('Enter');
    await settled('A5', '155');
    // Bold and the currency format of the interface language (English: dollars).
    await page.click('#spreadsheets td[data-ref="A1"]');
    await page.keyboard.press('ControlOrMeta+b');
    await page.waitForSelector('#spreadsheets td[data-ref="A1"].b');
    await page.click('#spreadsheets td[data-ref="A5"]');
    await page.click('#spreadsheets .toolbar button[aria-label="Currency"]');
    await settled('A5', '$155.00');
    await page.waitForSelector('#spreadsheets .save-state:text-is("Edited")');
    await shoot(page, '23-spreadsheet-editing');
    const saved = await downloadBytes(() => page.click('#spreadsheets .download-pill'));
    assert.equal(saved.name, 'Untitled spreadsheet.xlsx');
    await page.waitForSelector('#spreadsheets .save-state:text-is("Saved")');
    // Reopened through the engine: values, formula, format and bold.
    await open('spreadsheets', 'budget.xlsx', saved.bytes);
    await opened('spreadsheets', 'budget.xlsx');
    await settled('A5', '$155.00');
    assert.deepEqual(await Promise.all(['A1', 'A2', 'A3', 'A4'].map(cellText)), ['100', '', '25', '30']);
    await page.click('#spreadsheets td[data-ref="A5"]');
    assert.equal(await page.textContent('#spreadsheets .valuebox'), '=SUM(A1:A4)');
    assert.equal(await page.locator('#spreadsheets td[data-ref="A1"].b').count(), 1);
    const csv = await downloadAs('spreadsheets', 'Sheet values as CSV');
    assert.match(csv.bytes.toString(), /\$155\.00/);
  });

  await step('Presentations: slide strip with selection ring, 16:9 slide, notes, arrow keys; PPTX → ODP → reopened', async () => {
    await go('presentations');
    await open('presentations', 'sample.pptx');
    await opened('presentations', 'sample.pptx');
    assert.equal(await page.locator('#presentations .thumb').count(), 2);
    await page.waitForSelector('#presentations .slide-stage h2:text-is("Quarterly review")');
    assert.equal(await page.textContent('#presentations .notes-text'), 'Thank the team.');
    assert.equal(await page.getAttribute('#presentations .thumb >> nth=0', 'aria-current'), 'true');
    const box = await page.locator('#presentations .slide-stage .slide-card').boundingBox();
    assert.ok(Math.abs(box.width / box.height - 16 / 9) < 0.02, 'slides are 16:9');
    assert.ok(box.width > 800, `the slide fills the stage (${box.width})`);
    await shoot(page, '08-presentation');
    await page.keyboard.press('ArrowRight');
    await page.waitForSelector('#presentations .slide-stage h2:text-is("Next steps")');
    assert.equal(await page.getAttribute('#presentations .thumb >> nth=1', 'aria-current'), 'true');
    assert.equal(await page.textContent('#presentations .slide-position'), 'Slide 2 of 2');
    await page.keyboard.press('ArrowLeft');
    await page.waitForSelector('#presentations .slide-stage h2:text-is("Quarterly review")');
    await page.keyboard.press('End');
    await page.waitForSelector('#presentations .slide-stage h2:text-is("Next steps")');
    await menu('presentations', ['Slide', 'First slide']);
    await page.waitForSelector('#presentations .slide-stage h2:text-is("Quarterly review")');
    await page.click('#presentations .toolbar button[aria-label="Next slide"]');
    await page.waitForSelector('#presentations .slide-stage h2:text-is("Next steps")');
    await menu('presentations', ['View', 'Speaker notes']);
    await page.waitForSelector('#presentations .slide-notes', { state: 'hidden' });
    await page.click('#presentations .toolbar button[aria-label="Speaker notes"]');
    await page.waitForSelector('#presentations .slide-notes');
    const odp = await downloadAs('presentations', 'OpenDocument presentation');
    assert.equal(odp.name, 'sample.odp');
    assert.equal(odp.bytes.subarray(0, 2).toString(), 'PK');
    const pptx = await downloadAs('presentations', 'Microsoft PowerPoint');
    assert.equal(pptx.bytes.subarray(0, 2).toString(), 'PK');
    await open('presentations', 'sample.odp', odp.bytes);
    await opened('presentations', 'sample.odp');
    assert.equal(await page.locator('#presentations .thumb').count(), 2);
    await page.evaluate(() => (window.print = () => (window.__printed = document.querySelectorAll('#print-root .print-slide').length)));
    await menu('presentations', ['File', 'Print']);
    assert.equal(await page.evaluate(() => window.__printed), 2, 'every slide is printed');
    await page.evaluate(() => window.dispatchEvent(new Event('afterprint')));
  });

  await step('PDF: a summary page instead of a placeholder, a one-line note and a toggleable side panel', async () => {
    await go('pdf');
    await open('pdf', 'sample.pdf');
    await opened('pdf', 'sample.pdf');
    await page.waitForSelector('#pdf .summary-page h1:text-is("Quarterly report")');
    assert.equal(await page.textContent('#pdf .summary-page .facts dd'), '2');
    assert.equal(await page.textContent('#pdf .summary-page .tree > li > ul > li'), 'Revenue', 'nested bookmark');
    assert.equal(await page.locator('#pdf .canvas-note').count(), 1);
    assert.equal(await page.locator('text=Coming soon').count(), 0);
    await page.waitForSelector('#pdf .pdf-panel .facts dd:text-is("OFF42 test")');
    await shoot(page, '09-pdf');
    await page.click('#pdf .panel-tab[data-section="bookmarks"]');
    assert.equal(await page.textContent('#pdf .pdf-panel .tree > li > ul > li'), 'Revenue');
    await page.click('#pdf .toolbar button:has-text("Attachments")');
    await page.waitForSelector('#pdf .pdf-panel .plain-list li:text-is("notes.txt")');
    await page.click('#pdf .panel-tab[data-section="fields"]');
    await page.waitForSelector('#pdf .pdf-panel .fields td:text-is("Approved by")');
    await page.click('#pdf .panel-toggle');
    await page.waitForSelector('#pdf .pdf-panel', { state: 'hidden' });
    assert.equal(await page.getAttribute('#pdf .panel-toggle', 'aria-pressed'), 'false');
    await menu('pdf', ['View', 'Document details']);
    await page.waitForSelector('#pdf .pdf-panel');
    await page.click('#pdf .menubar-item:text-is("File")');
    assert.equal(await page.locator('.menu .menu-item:has-text("Download as")').isDisabled(), true, 'no conversions for PDF');
    assert.equal(await page.locator('.menu .menu-item:has-text("Print")').count(), 0, 'no print without page rendering');
    await page.keyboard.press('Escape');
    const pdf = await downloadBytes(() => page.click('#pdf .download-pill'));
    assert.equal(pdf.name, 'sample.pdf');
    assert.equal(pdf.bytes.subarray(0, 4).toString(), '%PDF');
    await menu('pdf', ['File', 'Close']);
    await page.waitForSelector('#pdf .recent-row:has-text("sample.pdf")');
  });

  await step('Labels & codes: flat settings/canvas layout, live code preview, PNG/SVG, label sheet PDF, marks', async () => {
    await go('labels');
    await page.waitForSelector('#labels .topbar .app-title h1:text-is("Labels & codes")');
    await page.waitForSelector('#code-preview img');
    const settings = await page.locator('#labels .pane[data-pane="codes"] .settings').boundingBox();
    const canvas = await page.locator('#labels .pane[data-pane="codes"] .canvas').boundingBox();
    assert.ok(canvas.x >= settings.x + settings.width - 1, 'settings left, canvas right');
    assert.equal(await page.locator('#labels .card').count(), 0, 'no cards');
    const first = await page.getAttribute('#code-preview img', 'src');
    await page.fill('#code-value', 'OFF42-0001');
    await page.waitForFunction((src) => document.querySelector('#code-preview img')?.getAttribute('src') !== src, first);
    await page.click('#code-format button[data-value="code128"]');
    await page.waitForFunction(() => document.querySelector('#code-format [aria-checked="true"]')?.dataset.value === 'code128');
    await shoot(page, '10-codes');
    const png = await downloadBytes(() => page.click('#code-png'));
    assert.equal(png.bytes.subarray(1, 4).toString(), 'PNG');
    const svg = await downloadBytes(() => page.click('#code-svg'));
    assert.match(svg.bytes.toString(), /<svg/);
    await page.fill('#code-value', '');
    await page.waitForSelector('#code-preview .error-note');

    await page.click('[data-tabs="labels"] [data-pane="sheets"]');
    await page.waitForSelector('#sheet-preview .slot');
    assert.equal(await page.locator('#sheet-preview .slot.filled').count(), 4, 'Box 1 ×2, Box 2, Spare parts');
    await page.fill('#sheet-first', '3');
    await page.waitForFunction(() => document.querySelectorAll('#sheet-preview .slot.used').length === 2);
    await shoot(page, '11-label-sheet');
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
    await shoot(page, '12-marks');
    const markPng = await downloadBytes(() => page.click('#marks-detail button:has-text("Download PNG")'));
    assert.equal(markPng.bytes.subarray(1, 4).toString(), 'PNG');
    await page.fill('#marks-search', 'zzzz-no-such-mark');
    await page.waitForSelector('#marks-grid .empty-line');
  });

  await step('Signatures & stamps: six variants, pen and ink, new impressions, live stamp editor, PNG downloads', async () => {
    await go('signatures');
    await page.waitForFunction(() => document.querySelectorAll('#sig-results .signature-paper img').length === 6);
    const before = await page.getAttribute('#sig-results .signature-paper img', 'src');
    await page.click('#sig-ink button[data-value="green"]');
    await page.click('#sig-pen button[data-value="2"]');
    await page.waitForFunction((src) => document.querySelector('#sig-results .signature-paper img')?.getAttribute('src') !== src, before);
    await page.waitForFunction(() => document.querySelectorAll('#sig-results .signature-paper img').length === 6);
    await shoot(page, '13-signatures');
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
    await shoot(page, '14-stamp');
    const png = await downloadBytes(() => page.click('#stamp-download'));
    assert.equal(png.bytes.subarray(1, 4).toString(), 'PNG');
  });

  await step('German from ?lang=: no English before the catalog, translated shell; the language menu switches', async () => {
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
    await page.waitForSelector('#sig-pen button');
    assert.notEqual(await page.textContent('#sig-pen button'), 'Ballpoint pen');
    await page.click('#signatures .menu-button');
    assert.equal(await page.textContent('#drawer .drawer-app[data-app="documents"]'), 'Dokumente');
    assert.notEqual(await page.textContent('#drawer-download'), 'Download for Mac, Windows, Linux');
    await page.keyboard.press('Escape');
    await page.click('#signatures .language-button');
    assert.equal(await page.textContent('.menu [role="menuitemradio"][aria-checked="true"]'), 'Deutsch');
    await Promise.all([page.waitForURL(/lang=fr/), page.click('.menu .menu-item:has-text("Français")')]);
    await ready(page);
    assert.equal(await page.getAttribute('html', 'lang'), 'fr');
    assert.match(page.url(), /#signatures$/, 'the app survives a language change');
    await open('documents', 'sample.md');
    await page.waitForSelector('#documents .menubar-item:text-is("Fichier")');
  });

  await step('Arabic is right-to-left (drawer, bar, menus); Japanese mark and Russian stamp names come from the cores', async () => {
    await page.goto(`${app}?lang=ar`);
    await ready(page);
    assert.equal(await page.getAttribute('html', 'dir'), 'rtl');
    assert.ok((await page.locator('#documents .menu-button').boundingBox()).x > 1300, '☰ at the right');
    await page.click('#documents .menu-button');
    const drawer = await page.locator('#drawer').boundingBox();
    assert.ok(drawer.x + drawer.width > 1430 && drawer.x > 1000, `the drawer slides in from the right ${JSON.stringify(drawer)}`);
    await page.keyboard.press('Escape');
    await open('spreadsheets', 'sample.csv');
    await opened('spreadsheets', 'sample.csv');
    const file = await page.locator('#spreadsheets .menubar-item >> nth=0').boundingBox();
    const view = await page.locator('#spreadsheets .menubar-item >> nth=1').boundingBox();
    assert.ok(file.x > view.x, 'menus run right to left');
    await page.locator('#spreadsheets .menubar-item >> nth=0').click();
    const list = await page.locator('.menu').boundingBox();
    assert.ok(Math.abs(list.x + list.width - (file.x + file.width)) < 2, 'the menu is aligned to the right edge of its title');
    await page.keyboard.press('Escape');
    await shoot(page, '15-arabic-rtl');
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

  await step('dark theme follows the system: dark chrome, white slide', async () => {
    const dark = await newPage({ colorScheme: 'dark' });
    await dark.goto(`${app}#presentations`);
    await ready(dark);
    const background = await dark.evaluate(() => getComputedStyle(document.body).backgroundColor);
    assert.equal(background, 'rgb(27, 28, 31)', 'dark background');
    await dark.setInputFiles('#file-input-presentations', join(fixtures, 'sample.pptx'));
    await dark.waitForSelector('#presentations .slide-stage h2');
    assert.equal(await dark.evaluate(() => getComputedStyle(document.querySelector('#presentations .slide-stage .slide-card')).backgroundColor),
      'rgb(255, 255, 255)');
    assert.equal(await dark.evaluate(() => getComputedStyle(document.querySelector('#presentations .editor')).backgroundColor), 'rgb(31, 32, 35)');
    await shoot(dark, '16-dark-presentation');
    await dark.context().close();
  });

  await step('phone width: compact bars, no menu bar but a ⋮ menu with the same commands, 44 px targets', async () => {
    const phone = await newPage({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    const d = driver(phone);
    await phone.goto(`${app}#documents`);
    await ready(phone);
    const bar = await phone.locator('#documents .topbar').boundingBox();
    assert.ok(bar.width >= 380 && bar.height <= 56, `top bar ${JSON.stringify(bar)}`);
    assert.ok(await phone.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), 'no horizontal scrolling');
    for (const selector of ['#documents .menu-button', '#documents .language-button', '#documents .help-button']) {
      const box = await phone.locator(selector).boundingBox();
      assert.ok(box.width >= 44 && box.height >= 44, `${selector} ${JSON.stringify(box)}`);
    }
    await shoot(phone, '17-phone-home');
    await d.open('documents', 'sample.md');
    await d.opened('documents', 'sample.md');
    await phone.waitForSelector('#documents .doc-page');
    assert.ok(await phone.locator('#documents .menubar').isHidden(), 'no menu bar');
    const more = await phone.locator('#documents .overflow-button').boundingBox();
    assert.ok(more.width >= 44 && more.height >= 44);
    const toolbar = await phone.locator('#documents .toolbar').boundingBox();
    assert.ok(toolbar.y + toolbar.height >= 843 && toolbar.height >= 56, `toolbar at the bottom ${JSON.stringify(toolbar)}`);
    assert.ok(await phone.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), 'no horizontal scrolling');
    await shoot(phone, '18-phone-document');
    await phone.click('#documents .overflow-button');
    assert.deepEqual(await phone.locator('.menu .menu-heading').allTextContents(), ['File', 'Edit', 'View', 'Insert', 'Format', 'Help']);
    const item = await phone.locator('.menu .menu-item >> nth=0').boundingBox();
    assert.ok(item.height >= 44);
    await shoot(phone, '19-phone-menu');
    // Hovering opens the submenu; with no room beside the item it opens below it and the item stays tappable.
    await phone.hover('.menu .menu-item:has-text("Download as")');
    await phone.click('.menu .menu-item:has-text("Download as")');
    const parent = await phone.locator('.menu .menu-item:has-text("Download as")').boundingBox();
    const submenu = await phone.locator('.menu').nth(1).boundingBox();
    assert.ok(submenu.y >= parent.y + parent.height - 1 || submenu.x >= parent.x + parent.width - 4,
      `the submenu does not cover its item ${JSON.stringify({ parent, submenu })}`);
    const [download] = await Promise.all([phone.waitForEvent('download'), phone.click('.menu .menu-item:has-text("Rich Text")')]);
    assert.equal(download.suggestedFilename(), 'sample.rtf');
    assert.match(readFileSync(await download.path()).toString(), /^\{\\rtf/);
    const snack = await phone.locator('#snackbar').boundingBox();
    assert.ok(snack.y + snack.height <= toolbar.y, 'the snackbar stays above the toolbar');
    await phone.click('#documents .back-button');
    await phone.waitForSelector('#documents .home:not([hidden])');
    // Spreadsheets on a phone: the snackbar clears both the sheet-tab bar and the bottom toolbar.
    await phone.goto(`${app}#spreadsheets`);
    await ready(phone);
    await d.open('spreadsheets', 'sheets.xlsx');
    await d.opened('spreadsheets', 'sheets.xlsx');
    await phone.click('#spreadsheets .overflow-button');
    await phone.hover('.menu .menu-item:has-text("Download as")');
    await phone.click('.menu .menu-item:has-text("Download as")');
    await Promise.all([phone.waitForEvent('download'), phone.locator('.menu .menu-item:has(.menu-label:text-is("Sheet values as CSV"))').last().click()]);
    await phone.waitForSelector('#snackbar:not([hidden])');
    await clearOf(phone, '#snackbar', '#spreadsheets .sheet-bar');
    await clearOf(phone, '#snackbar', '#spreadsheets .toolbar');
    await shoot(phone, '21-phone-spreadsheet-snackbar');
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
