// Headless browser test of the production build: serves it at /app/ with the headers off42.cc sends
// (website/public/_headers, including the site's Content-Security-Policy), drives the page in headless Chromium
// (Playwright) and checks that files open, conversions download, generators draw, the interface follows the
// language, and that the page makes no request outside its own origin. Run after `npm run build`; pass
// `--dir ../website/public/app` to test the copy published on the site.
// CHROMIUM_PATH may point at an existing headless Chromium; otherwise Playwright's own build is used
// (`npx playwright install chromium`).
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';
import assert from 'node:assert/strict';
import { serveApp } from './serve.mjs';
import { chromium } from 'playwright';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const fixtures = join(root, 'test', 'fixtures');
// --url tests an app that is already served elsewhere, e.g. `wrangler dev` in website/ (http://127.0.0.1:8787/app/).
const arg = (name) => (process.argv.includes(name) ? process.argv[process.argv.indexOf(name) + 1] : undefined);
const dir = resolve(arg('--dir') ?? join(root, 'dist'));
const server = arg('--url') ? { origin: new URL(arg('--url')).origin, close: async () => {} }
  : await serveApp(dir, join(root, '..', 'website', 'public', '_headers'));
const origin = server.origin;
const app = arg('--url') ?? `${origin}/app/`;
console.log(`testing ${app}${arg('--url') ? '' : ` (serving ${dir})`}`);
const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROMIUM_PATH || undefined });
let failed = false;

try {
  const context = await browser.newContext({ acceptDownloads: true });
  const page = await context.newPage();
  const foreign = [];
  const errors = [];
  page.on('request', (request) => {
    const url = request.url();
    if (!url.startsWith(origin) && !/^(data|blob|about):/.test(url)) foreign.push(url);
  });
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('console', (msg) => msg.type() === 'error' && errors.push(msg.text()));
  page.on('dialog', (dialog) => dialog.accept());

  const step = async (name, fn) => {
    await fn();
    console.log(`ok   ${name}`);
  };
  const open = async (file) => {
    await page.setInputFiles('#file-input', join(fixtures, file));
    await page.waitForSelector(`#file-view h2:text-is("${file}")`);
  };
  const downloadBytes = async (click) => {
    const [download] = await Promise.all([page.waitForEvent('download'), click()]);
    return readFileSync(await download.path());
  };

  await step('engine loads under /app/ with the site headers (CSP allows wasm, served as application/wasm)', async () => {
    const wasm = page.waitForResponse((r) => r.url().endsWith('.wasm'));
    const response = await page.goto(app);
    assert.match(response.headers()['content-security-policy'] ?? '', /script-src 'self' 'wasm-unsafe-eval'/);
    await page.waitForSelector('#engine-version:not(:empty)', { timeout: 60_000 });
    assert.equal((await wasm).headers()['content-type'], 'application/wasm');
    assert.equal(await page.getAttribute('html', 'lang'), 'en');
  });

  await step('Markdown opens as a preview and saves as DOCX', async () => {
    await open('sample.md');
    const frame = page.frameLocator('iframe.doc-frame');
    await frame.locator('text=Quarterly notes').first().waitFor();
    const docx = await downloadBytes(() => page.click('#file-view button:text-is("DOCX")'));
    assert.equal(docx.subarray(0, 2).toString(), 'PK');
  });

  await step('CSV opens as a table with calculated values and saves as XLSX', async () => {
    await open('sample.csv');
    await page.waitForSelector('.sheet-scroll td:text-is("12")');
    const xlsx = await downloadBytes(() => page.click('#file-view button:text-is("XLSX")'));
    assert.equal(xlsx.subarray(0, 2).toString(), 'PK');
  });

  await step('PPTX opens as a slide outline and saves as ODP', async () => {
    await open('sample.pptx');
    await page.waitForSelector('.slide h3:text-is("Quarterly review")');
    assert.equal(await page.locator('.slide').count(), 2);
    const odp = await downloadBytes(() => page.click('#file-view button:text-is("ODP")'));
    assert.equal(odp.subarray(0, 2).toString(), 'PK');
  });

  await step('Labels & codes: code preview, label sheet PDF, product marks', async () => {
    await page.click('[data-tab="labels"]');
    await page.waitForSelector('#code-preview img');
    const pdf = await downloadBytes(() => page.click('#sheet-pdf'));
    assert.equal(pdf.subarray(0, 4).toString(), '%PDF');
    assert.ok((await page.locator('.mark').count()) > 50);
    await page.fill('#marks-search', 'CE');
    await page.locator('.mark').first().click();
    await page.waitForSelector('#marks-detail h3');
    const png = await downloadBytes(() => page.click('#marks-detail button:text-is("Download PNG")'));
    assert.equal(png.subarray(1, 4).toString(), 'PNG');
  });

  await step('Signatures & stamps: three signatures and a stamp', async () => {
    await page.click('[data-tab="marks"]');
    await page.waitForSelector('#stamp-preview img');
    await page.click('#sig-generate');
    await page.waitForFunction(() => document.querySelectorAll('#sig-results img').length === 3);
    const png = await downloadBytes(() => page.click('#stamp-download'));
    assert.equal(png.subarray(1, 4).toString(), 'PNG');
  });

  await step('language from ?lang=: German interface, language picker, translated options', async () => {
    await page.goto(`${app}?lang=de`);
    await page.waitForSelector('#engine-version:not(:empty)', { timeout: 60_000 });
    assert.equal(await page.getAttribute('html', 'lang'), 'de');
    assert.equal(await page.textContent('[data-tab="files"]'), 'Dateien');
    assert.equal(await page.inputValue('#language'), 'de');
    await page.click('[data-tab="marks"]');
    await page.waitForSelector('#stamp-preview img');
    assert.notEqual(await page.textContent('#sig-pen option'), 'Ballpoint pen');
    await Promise.all([page.waitForURL(/lang=fr/), page.selectOption('#language', 'fr')]);
    await page.waitForSelector('#engine-version:not(:empty)', { timeout: 60_000 });
    assert.equal(await page.getAttribute('html', 'lang'), 'fr');
  });

  await step('Arabic is right-to-left; Japanese mark names and Russian stamp names come from the cores', async () => {
    await page.goto(`${app}?lang=ar`);
    await page.waitForSelector('#engine-version:not(:empty)', { timeout: 60_000 });
    assert.equal(await page.getAttribute('html', 'dir'), 'rtl');
    await page.goto(`${app}?lang=ja`);
    await page.waitForSelector('#engine-version:not(:empty)', { timeout: 60_000 });
    await page.click('[data-tab="labels"]');
    await page.waitForSelector('.mark');
    assert.ok(await page.locator('.mark span', { hasText: /[\u3040-\u30ff\u4e00-\u9fff]/ }).count() > 10, 'Japanese mark names');
    await page.goto(`${app}?lang=ru`);
    await page.waitForSelector('#engine-version:not(:empty)', { timeout: 60_000 });
    await page.click('[data-tab="marks"]');
    await page.waitForSelector('#stamp-preview img');
    assert.match(await page.textContent('#stamp-template option'), /[а-яё]/i);
  });

  await step('browser language is the default (zh-TW → Traditional Chinese)', async () => {
    const zh = await browser.newContext({ locale: 'zh-TW' });
    const zhPage = await zh.newPage();
    zhPage.on('pageerror', (error) => errors.push(error.message));
    await zhPage.goto(app);
    await zhPage.waitForSelector('#engine-version:not(:empty)', { timeout: 60_000 });
    assert.equal(await zhPage.getAttribute('html', 'lang'), 'zh-Hant');
    await zh.close();
  });

  assert.deepEqual(foreign, [], 'requests outside the app origin');
  assert.deepEqual(errors, [], 'page errors');
  console.log('ok   no requests left the app origin; no page errors');
} catch (error) {
  failed = true;
  console.error(`FAIL ${error?.stack ?? error}`);
} finally {
  await browser.close();
  await server.close();
}
process.exit(failed ? 1 : 0);
