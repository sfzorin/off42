// A thin web shell over the off42 Rust cores (crates/off42-wasm). All work happens in this tab; the engine
// receives bytes and returns bytes, and nothing is sent over the network.
import init, * as off42 from './wasm/off42_wasm.js';
import { chooseLanguage, language, languagePicker, loadLanguage, t, translatePage } from './i18n';

// MARK: - Helpers

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]> & { className?: string } = {},
  ...children: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children);
  return node;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function download(data: Uint8Array | string, name: string, type: string) {
  const blob = new Blob([data as BlobPart], { type });
  const url = URL.createObjectURL(blob);
  const link = el('a', { href: url, download: name });
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

function imageFrom(data: Uint8Array | string, type: string, alt: string): HTMLImageElement {
  const url = URL.createObjectURL(new Blob([data as BlobPart], { type }));
  const image = el('img', { src: url, alt });
  image.addEventListener('load', () => URL.revokeObjectURL(url), { once: true });
  return image;
}

/** A random 64-bit seed as a decimal string (Web Crypto; the engine has no clock or entropy of its own). */
function seed(): string {
  const value = crypto.getRandomValues(new BigUint64Array(1))[0];
  return (value === 0n ? 1n : value).toString();
}

function stem(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(0, dot) : name;
}

function extension(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : '';
}

function errorBox(text: string): HTMLElement {
  return el('p', { className: 'error', textContent: text });
}

function button(label: string, onClick: () => void, primary = false): HTMLButtonElement {
  const b = el('button', { textContent: label, className: primary ? 'primary' : '' });
  b.addEventListener('click', () => {
    try {
      onClick();
    } catch (error) {
      alert(message(error));
    }
  });
  return b;
}

const MIME: Record<string, string> = {
  docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  odt: 'application/vnd.oasis.opendocument.text',
  rtf: 'application/rtf',
  md: 'text/markdown',
  html: 'text/html',
  txt: 'text/plain',
  xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  ods: 'application/vnd.oasis.opendocument.spreadsheet',
  csv: 'text/csv',
  pptx: 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
  odp: 'application/vnd.oasis.opendocument.presentation',
  pdf: 'application/pdf',
  png: 'image/png',
  svg: 'image/svg+xml',
};

// MARK: - Tabs

function setupTabs() {
  const tabs = document.querySelectorAll<HTMLButtonElement>('[data-tab]');
  const opened = new Set<string>();
  const show = (name: string) => {
    tabs.forEach((t) => t.setAttribute('aria-selected', String(t.dataset.tab === name)));
    document.querySelectorAll<HTMLElement>('.panel').forEach((p) => (p.hidden = p.id !== name));
    if (!opened.has(name)) {
      opened.add(name);
      if (name === 'labels') setupLabels();
      if (name === 'marks') setupSignatures();
    }
  };
  tabs.forEach((t) => t.addEventListener('click', () => show(t.dataset.tab!)));
  show('files');
}

// MARK: - Files

const DOCUMENTS = new Set(['docx', 'odt', 'rtf', 'md', 'markdown', 'html', 'htm', 'txt']);
const SHEETS = new Set(['xlsx', 'ods', 'xls', 'csv']);
const SLIDES = new Set(['pptx', 'odp']);

function setupFiles() {
  const input = $<HTMLInputElement>('file-input');
  const drop = $('drop');
  input.addEventListener('change', () => input.files?.[0] && openFile(input.files[0]));
  drop.addEventListener('dragover', (e) => {
    e.preventDefault();
    drop.classList.add('over');
  });
  drop.addEventListener('dragleave', () => drop.classList.remove('over'));
  drop.addEventListener('drop', (e) => {
    e.preventDefault();
    drop.classList.remove('over');
    const file = e.dataTransfer?.files[0];
    if (file) openFile(file);
  });
}

async function openFile(file: File) {
  const view = $('file-view');
  view.replaceChildren(el('p', { className: 'notice', textContent: t('files.opening', { name: file.name }) }));
  const bytes = new Uint8Array(await file.arrayBuffer());
  const kind = extension(file.name);
  // Let the notice paint before the synchronous engine call.
  await new Promise((resolve) => setTimeout(resolve, 0));
  const header = el('div', { className: 'file-header' },
    el('h2', { textContent: file.name }),
    el('span', { className: 'hint', textContent: t('files.size', { size: (bytes.length / 1024).toLocaleString(language, { maximumFractionDigits: 1 }) }) }));
  try {
    let body: HTMLElement;
    if (DOCUMENTS.has(kind)) body = showDocument(bytes, kind, file.name);
    else if (SHEETS.has(kind)) body = showSpreadsheet(bytes, kind, file.name);
    else if (SLIDES.has(kind)) body = showPresentation(bytes, kind, file.name);
    else if (kind === 'pdf') body = showPdf(bytes);
    else body = errorBox(t('files.unsupported', { extension: kind }));
    view.replaceChildren(header, body);
  } catch (error) {
    view.replaceChildren(header, errorBox(message(error)));
  }
}

function documentKind(kind: string): string {
  return kind === 'markdown' ? 'md' : kind === 'htm' ? 'html' : kind;
}

function showDocument(bytes: Uint8Array, rawKind: string, name: string): HTMLElement {
  const kind = documentKind(rawKind);
  const html = off42.documentHtml(bytes, kind);
  const info = JSON.parse(off42.documentInfo(bytes, kind));
  // The preview runs no scripts and has no access to this page (empty sandbox).
  const frame = el('iframe', { className: 'doc-frame', title: t('doc.preview') });
  frame.setAttribute('sandbox', '');
  frame.srcdoc = html;
  const save = (to: string) => {
    let output: Uint8Array;
    try {
      output = off42.documentConvert(bytes, kind, to, false);
    } catch (error) {
      const text = message(error);
      if (!/package parts/.test(text)) throw error;
      const ok = confirm(t('doc.convertAnyway', { error: text, format: to.toUpperCase() }));
      if (!ok) return;
      output = off42.documentConvert(bytes, kind, to, true);
    }
    download(output, `${stem(name)}.${to}`, MIME[to]);
  };
  const actions = el('div', { className: 'actions' }, el('span', { className: 'hint', textContent: t('common.saveAs') }),
    ...['docx', 'odt', 'rtf', 'md', 'html', 'txt'].map((to) => button(to.toUpperCase(), () => save(to))));
  const stats = el('p', { className: 'hint',
    textContent: t('doc.stats', { words: info.words, characters: info.characters, paragraphs: info.paragraphs }) });
  return el('div', {}, actions, stats, frame);
}

type Sheet = { name: string; rows: number; cols: number; truncated: boolean; cells: [number, number, string][] };
const MAX_ROWS = 1000;
const MAX_COLS = 100;

function columnName(index: number): string {
  let name = '';
  for (let n = index + 1; n > 0; n = Math.floor((n - 1) / 26)) name = String.fromCharCode(65 + ((n - 1) % 26)) + name;
  return name;
}

function showSpreadsheet(bytes: Uint8Array, kind: string, name: string): HTMLElement {
  const book: { readOnly: boolean; sheets: Sheet[] } = JSON.parse(off42.spreadsheetJson(bytes, kind, MAX_ROWS, MAX_COLS));
  let current = 0;
  const table = el('div', { className: 'sheet-scroll' });
  const tabs = el('div', { className: 'sheet-tabs' });
  const note = el('p', { className: 'hint' });
  const render = () => {
    const sheet = book.sheets[current];
    const rows = Math.min(Math.max(sheet.rows, ...sheet.cells.map((c) => c[0] + 1), 1), MAX_ROWS);
    const cols = Math.min(Math.max(sheet.cols, ...sheet.cells.map((c) => c[1] + 1), 1), MAX_COLS);
    const grid: string[][] = Array.from({ length: rows }, () => Array(cols).fill(''));
    for (const [r, c, text] of sheet.cells) grid[r][c] = text;
    const head = el('tr', {}, el('th'), ...grid[0].map((_, c) => el('th', { textContent: columnName(c) })));
    const body = grid.map((row, r) => el('tr', {}, el('th', { textContent: String(r + 1) }),
      ...row.map((text) => el('td', { textContent: text, className: /^-?[\d.,\s%]+$/.test(text) ? 'num' : '' }))));
    table.replaceChildren(el('table', {}, el('thead', {}, head), el('tbody', {}, ...body)));
    tabs.querySelectorAll('button').forEach((b, i) => b.setAttribute('aria-selected', String(i === current)));
    note.textContent = sheet.truncated ? t('sheet.truncated', { rows: MAX_ROWS, columns: MAX_COLS }) : '';
  };
  book.sheets.forEach((sheet, i) => tabs.append(button(sheet.name, () => { current = i; render(); })));
  render();
  const actions = el('div', { className: 'actions' }, el('span', { className: 'hint', textContent: t('common.saveAs') }),
    ...['xlsx', 'ods', 'csv'].map((to) =>
      button(to.toUpperCase(), () => download(off42.spreadsheetConvert(bytes, kind, to), `${stem(name)}.${to}`, MIME[to]))),
    button(t('sheet.valuesCsv'), () => download(off42.spreadsheetValuesCsv(bytes, kind, current),
      `${stem(name)} - ${book.sheets[current].name}.csv`, MIME.csv)));
  const readOnly = book.readOnly
    ? el('p', { className: 'hint', textContent: t('sheet.readOnly') }) : '';
  return el('div', {}, actions, readOnly, tabs, note, table);
}

type SlideOutline = {
  title: string; body: string; notes: string; hidden: boolean;
  objects: { kind: string; role: 'title' | 'body' | 'footer' | null; text: string; cells: string[][] }[];
};

function lines(text: string): HTMLElement {
  return el('ul', {}, ...text.split('\n').filter(Boolean).map((line) => el('li', { textContent: line })));
}

function showPresentation(bytes: Uint8Array, kind: string, name: string): HTMLElement {
  const outline: { slides: SlideOutline[] } = JSON.parse(off42.presentationOutline(bytes, kind));
  const slides = outline.slides.map((slide, i) => {
    // Imported slides carry their title in a placeholder object with the title role; slides without
    // placeholders (e.g. written by off42 itself) start with their title text box.
    const titleObject = slide.title ? undefined
      : slide.objects.find((o) => o.role === 'title')
        ?? (slide.objects.every((o) => o.role === null) ? slide.objects.find((o) => o.kind === 'Text') : undefined);
    const card = el('article', { className: `slide${slide.hidden ? ' hidden-slide' : ''}` },
      el('div', { className: 'slide-number', textContent: `${i + 1}${slide.hidden ? ` · ${t('slides.hidden')}` : ''}` }),
      el('h3', { textContent: slide.title || titleObject?.text || t('slides.untitled') }));
    if (slide.body) card.append(lines(slide.body));
    for (const object of slide.objects) {
      if (object === titleObject) continue;
      if (object.cells.length) {
        card.append(el('table', { className: 'mini' }, ...object.cells.map((row) =>
          el('tr', {}, ...row.map((text) => el('td', { textContent: text }))))));
      } else if (object.kind === 'Picture') {
        card.append(el('p', { className: 'hint', textContent: t('slides.picture') }));
      } else if (object.text) {
        card.append(object.role === 'body' ? lines(object.text) : el('p', { textContent: object.text }));
      }
    }
    if (slide.notes) card.append(el('p', { className: 'notes', textContent: t('slides.notes', { text: slide.notes }) }));
    return card;
  });
  const actions = el('div', { className: 'actions' }, el('span', { className: 'hint', textContent: t('common.saveAs') }),
    ...['pptx', 'odp'].map((to) =>
      button(to.toUpperCase(), () => download(off42.presentationConvert(bytes, kind, to), `${stem(name)}.${to}`, MIME[to]))));
  return el('div', {}, actions, el('p', { className: 'hint', textContent: t('slides.outline', { count: slides.length }) }),
    el('div', { className: 'slides' }, ...slides));
}

function showPdf(bytes: Uint8Array): HTMLElement {
  const info = JSON.parse(off42.pdfInfo(bytes));
  if (info.encrypted) return el('p', { className: 'notice', textContent: t('pdf.encrypted') });
  const rows: [string, string][] = [[t('pdf.pages'), String(info.pages)]];
  for (const [key, value] of Object.entries(info.metadata as Record<string, string>)) rows.push([key, value]);
  if (info.bookmarks.length) rows.push([t('pdf.bookmarks'), info.bookmarks.join(' · ')]);
  if (info.attachments.length) rows.push([t('pdf.attachments'), info.attachments.join(', ')]);
  if (info.formFields.length) rows.push([t('pdf.formFields'), info.formFields.map((f: { name: string }) => f.name).join(', ')]);
  return el('div', {},
    el('table', { className: 'info' }, ...rows.map(([k, v]) => el('tr', {}, el('th', { textContent: k }), el('td', { textContent: v })))),
    el('p', { className: 'hint', textContent: t('pdf.noRendering') }));
}

// MARK: - Labels & codes

type Stock = { id: string; name: string; width: number; height: number; columns: number; rows: number };
type Mark = { id: string; name: string; category: string; purpose: string; warning?: string; unverified: boolean; needsNumber: boolean };

function setupLabels() {
  // Codes
  const format = $<HTMLSelectElement>('code-format');
  const value = $<HTMLInputElement>('code-value');
  const width = $<HTMLInputElement>('code-width');
  const caption = $<HTMLInputElement>('code-caption');
  const preview = $('code-preview');
  const renderCode = () => {
    try {
      const svg = off42.barcodeSvg(format.value, value.value, Number(width.value), caption.checked);
      preview.replaceChildren(imageFrom(svg, MIME.svg, t('code.preview')));
    } catch (error) {
      preview.replaceChildren(errorBox(message(error)));
    }
  };
  [format, value, width, caption].forEach((c) => c.addEventListener('input', renderCode));
  renderCode();
  $('code-png').addEventListener('click', () => guarded(() =>
    download(off42.barcodePng(format.value, value.value, Number(width.value)), `${format.value}.png`, MIME.png)));
  $('code-svg').addEventListener('click', () => guarded(() =>
    download(off42.barcodeSvg(format.value, value.value, Number(width.value), caption.checked), `${format.value}.svg`, MIME.svg)));

  // Label sheets
  const stocks: Stock[] = JSON.parse(off42.labelStocks());
  const stock = $<HTMLSelectElement>('sheet-stock');
  stock.append(...stocks.map((s) => el('option', { value: s.id,
    textContent: `${s.name} — ${s.columns}×${s.rows}, ${s.width}×${s.height} mm` })),
  el('option', { value: 'custom', textContent: t('labels.custom') }));
  stock.addEventListener('change', () => ($('sheet-custom').hidden = stock.value !== 'custom'));
  $('sheet-pdf').addEventListener('click', () => guarded(() => {
    const labels = $<HTMLTextAreaElement>('sheet-lines').value.split('\n').map((l) => l.trim()).filter(Boolean).map((line) => {
      const [text, code, copies] = line.split('|').map((p) => p.trim());
      return { text: text ?? '', code: code || undefined, copies: copies ? Number(copies) : 1 };
    });
    const num = (id: string) => Number($<HTMLInputElement>(id).value);
    const pdf = off42.labelSheetPdf(JSON.stringify({
      stock: stock.value, width: num('sheet-width'), height: num('sheet-height'), gapX: num('sheet-gapx'),
      gapY: num('sheet-gapy'), firstSlot: num('sheet-first'), codeFormat: $<HTMLSelectElement>('sheet-code').value || undefined,
      labels,
    }));
    const pages = JSON.parse(off42.pdfInfo(pdf)).pages;
    $('sheet-status').textContent = t('labels.status', { labels: labels.reduce((n, l) => n + l.copies, 0), pages });
    download(pdf, 'labels.pdf', MIME.pdf);
  }, $('sheet-status')));

  // Product marks
  // label42 has mark names in 11 of the 20 languages (English otherwise), keyed by the same codes.
  const marks: Mark[] = JSON.parse(off42.productMarks(language));
  const category = $<HTMLSelectElement>('marks-category');
  category.append(...[...new Set(marks.map((m) => m.category))].sort().map((c) => el('option', { value: c, textContent: c })));
  const search = $<HTMLInputElement>('marks-search');
  const markWidth = $<HTMLInputElement>('marks-width');
  const grid = $('marks-grid');
  const detail = $('marks-detail');
  const thumbnails = new Map<string, HTMLElement>();
  for (const mark of marks) {
    const tile = el('button', { className: 'mark', title: mark.name });
    try {
      tile.append(imageFrom(off42.productMarkSvg(mark.id, 20), MIME.svg, mark.name));
    } catch {
      tile.append(el('span', { textContent: '?' }));
    }
    tile.append(el('span', { textContent: mark.name }));
    tile.addEventListener('click', () => {
      detail.hidden = false;
      const w = () => Number(markWidth.value);
      detail.replaceChildren(
        el('h3', { textContent: mark.name }),
        el('p', { textContent: mark.purpose }),
        ...(mark.warning ? [el('p', { className: 'warning', textContent: mark.warning })] : []),
        ...(mark.unverified ? [el('p', { className: 'warning', textContent: t('marks.unverified') })] : []),
        ...(mark.needsNumber ? [el('p', { className: 'hint', textContent: t('marks.needsNumber') })] : []),
        el('div', { className: 'actions' },
          button(t('common.downloadSvg'), () => download(off42.productMarkSvg(mark.id, w()), `${mark.id}.svg`, MIME.svg)),
          button(t('common.downloadPng'), () => download(off42.productMarkPng(mark.id, w()), `${mark.id}.png`, MIME.png))));
    });
    thumbnails.set(mark.id, tile);
  }
  const filter = () => {
    const q = search.value.trim().toLowerCase();
    grid.replaceChildren(...marks
      .filter((m) => (!category.value || m.category === category.value) &&
        (!q || `${m.name} ${m.purpose} ${m.id}`.toLowerCase().includes(q)))
      .map((m) => thumbnails.get(m.id)!));
  };
  search.addEventListener('input', filter);
  category.addEventListener('change', filter);
  filter();
}

function guarded(action: () => void, status?: HTMLElement) {
  try {
    if (status) status.textContent = '';
    action();
  } catch (error) {
    if (status) status.textContent = message(error);
    else alert(message(error));
  }
}

// MARK: - Signatures & stamps

type Option = { value: number | string; title: string };
type Field = { key: string; label: string; example: string; multiline: boolean; date: boolean };
type Template = { id: string; title: string; category: string; round: boolean; ink: string; fields: Field[] };

function fillSelect(select: HTMLSelectElement, options: Option[], selected?: string) {
  select.replaceChildren(...options.map((o) => el('option', { value: String(o.value), textContent: o.title })));
  if (selected) select.value = selected;
}

function today(): string {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(d.getDate())}/${pad(d.getMonth() + 1)}/${d.getFullYear()}`;
}

function setupSignatures() {
  const options: { forms: Option[]; pens: Option[]; inks: Option[] } = JSON.parse(off42.signatureOptions());
  // The core names these in English; the interface translates the fixed option sets.
  const forms = options.forms.map((o) => ({ ...o, title: t(`sig.form.${o.value}`) }));
  const pens = options.pens.map((o) => ({ ...o, title: t(`sig.pen.${o.value}`) }));
  const inks = options.inks.map((o) => ({ ...o, title: t(`ink.${o.value}`) }));
  fillSelect($('sig-form'), forms);
  fillSelect($('sig-pen'), pens);
  fillSelect($('sig-ink'), inks, 'blue');
  fillSelect($('stamp-ink'), [{ value: '', title: t('stamp.defaultInk') }, ...inks]);

  $('sig-generate').addEventListener('click', () => {
    const results = $('sig-results');
    const name = $<HTMLInputElement>('sig-name').value;
    const form = Number($<HTMLSelectElement>('sig-form').value);
    const pen = Number($<HTMLSelectElement>('sig-pen').value);
    const ink = $<HTMLSelectElement>('sig-ink').value;
    try {
      const designs: string[] = JSON.parse(off42.signatureDesigns(3, form, seed()));
      results.replaceChildren(...designs.map((design) => {
        const frame = el('div', { className: 'preview checker' });
        let png: Uint8Array = new Uint8Array();
        const draw = () => {
          png = off42.signaturePng(name, design, seed(), ink, pen, 130);
          frame.replaceChildren(imageFrom(png, MIME.png, t('sig.alt', { name })));
        };
        draw();
        return el('div', { className: 'result' }, frame, el('div', { className: 'actions' },
          button(t('common.newImpression'), draw),
          button(t('common.downloadPng'), () => download(png, `signature-${stem(name.replace(/\s+/g, '-'))}.png`, MIME.png))));
      }));
    } catch (error) {
      results.replaceChildren(errorBox(message(error)));
    }
  });

  const templates: Template[] = JSON.parse(off42.stampTemplates(language));
  const select = $<HTMLSelectElement>('stamp-template');
  const groups = new Map<string, HTMLOptGroupElement>();
  for (const t of templates) {
    if (!groups.has(t.category)) groups.set(t.category, el('optgroup', { label: t.category }));
    groups.get(t.category)!.append(el('option', { value: t.id, textContent: t.title }));
  }
  select.replaceChildren(...groups.values());
  const fields = $('stamp-fields');
  const preview = $('stamp-preview');
  let impression = seed();
  const inputs = new Map<string, HTMLInputElement | HTMLTextAreaElement>();
  const current = () => templates.find((t) => t.id === select.value)!;
  // The impression (tilt, pressure, grain) is the same at every scale: preview small, download large.
  const render = (scale: number) => {
    const values = Object.fromEntries([...inputs].map(([k, input]) => [k, input.value]));
    return off42.stampPng(select.value, JSON.stringify(values), $<HTMLSelectElement>('stamp-ink').value, impression, scale);
  };
  const showFields = () => {
    inputs.clear();
    fields.replaceChildren(...current().fields.map((f) => {
      const input = f.multiline ? el('textarea', { rows: 2 }) : el('input');
      input.value = f.date ? today() : f.example;
      inputs.set(f.key, input);
      return el('label', {}, f.label, input);
    }));
  };
  const draw = () => {
    try {
      preview.replaceChildren(imageFrom(render(0.5), MIME.png, current().title));
    } catch (error) {
      preview.replaceChildren(errorBox(message(error)));
    }
  };
  select.addEventListener('change', () => { showFields(); draw(); });
  $('stamp-render').addEventListener('click', draw);
  $('stamp-again').addEventListener('click', () => { impression = seed(); draw(); });
  $('stamp-download').addEventListener('click', () => guarded(() => download(render(2), `stamp-${select.value}.png`, MIME.png)));
  showFields();
  draw();
}

// MARK: - Start

async function start() {
  await loadLanguage(chooseLanguage());
  translatePage();
  languagePicker($<HTMLSelectElement>('language'));
  $<HTMLAnchorElement>('site-link').href = `../${language}/`;
  try {
    await init();
  } catch (error) {
    $('loading').replaceWith(errorBox(t('app.engineFailed', { error: message(error) })));
    return;
  }
  $('loading').remove();
  $('engine-version').textContent = t('app.engine', { version: off42.version() });
  setupTabs();
  setupFiles();
}

start();
