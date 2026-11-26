// Headless browser test of the production build: serves dist/ with `vite preview`, drives the page in headless
// Chromium (Playwright) and checks that files open, conversions download, generators draw, and that the page
// makes no request outside its own origin. Run after `npm run build`.
// CHROMIUM_PATH may point at an existing headless Chromium; otherwise Playwright's own build is used
// (`npx playwright install chromium`).
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import assert from 'node:assert/strict';
import { preview } from 'vite';
import { chromium } from 'playwright';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const fixtures = join(root, 'test', 'fixtures');
const server = await preview({ root, preview: { port: 0, host: '127.0.0.1', open: false }, logLevel: 'warn' });
const origin = server.resolvedUrls.local[0].replace(/\/$/, '');
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

  await step('engine loads', async () => {
    await page.goto(`${origin}/`);
    await page.waitForSelector('#engine-version:not(:empty)', { timeout: 60_000 });
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

  assert.deepEqual(foreign, [], 'requests outside the app origin');
  assert.deepEqual(errors, [], 'page errors');
  console.log('ok   no requests left the app origin; no page errors');
} catch (error) {
  failed = true;
  console.error(`FAIL ${error?.stack ?? error}`);
} finally {
  await browser.close();
  await new Promise((resolve) => server.httpServer.close(resolve));
}
process.exit(failed ? 1 : 0);
