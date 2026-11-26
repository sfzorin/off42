// The OFF42 web app: a browser shell over the off42 Rust cores (crates/off42-wasm), which run in a Web Worker
// (engine.worker.ts). Files are read from the user's disk, processed in this tab and offered back as downloads;
// nothing is sent over the network.
import { call, json, startEngine, type Progress } from './engine';
import { icon, placeIcons } from './icons';
import { chooseLanguage, language, languagePicker, loadLanguage, t, translatePage } from './i18n';

// MARK: - Helpers

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

type Props<K extends keyof HTMLElementTagNameMap> = Partial<Omit<HTMLElementTagNameMap[K], 'dataset'>> & { dataset?: Record<string, string> };

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K, props: Props<K> = {}, ...children: (Node | string | false | null | undefined)[]
): HTMLElementTagNameMap[K] {
  const { dataset, ...rest } = props;
  const node = Object.assign(document.createElement(tag), rest);
  if (dataset) Object.assign(node.dataset, dataset);
  node.append(...children.filter((c): c is Node | string => c !== false && c !== null && c !== undefined));
  return node;
}

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

function stem(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(0, dot) : name;
}

function extension(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : '';
}

/** A file size in the interface language (Intl unit formatting needs no catalog strings). */
function bytesText(bytes: number): string {
  const [value, unit] = bytes >= 1024 * 1024 ? [bytes / 1024 / 1024, 'megabyte'] : [Math.max(bytes / 1024, 0.1), 'kilobyte'];
  return new Intl.NumberFormat(language, { style: 'unit', unit, unitDisplay: 'short', maximumFractionDigits: 1 }).format(value);
}

const number = (n: number) => new Intl.NumberFormat(language).format(n);

/** A random 64-bit seed as a decimal string (Web Crypto; the engine has no clock or entropy of its own). */
function seed(): string {
  const value = crypto.getRandomValues(new BigUint64Array(1))[0];
  return (value === 0n ? 1n : value).toString();
}

function today(): string {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(d.getDate())}/${pad(d.getMonth() + 1)}/${d.getFullYear()}`;
}

function debounce<A extends unknown[]>(fn: (...args: A) => void, ms: number) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return (...args: A) => {
    clearTimeout(timer);
    timer = setTimeout(() => fn(...args), ms);
  };
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

function download(data: Uint8Array | string, name: string, type: string, announce = true) {
  const url = URL.createObjectURL(new Blob([data as BlobPart], { type }));
  const link = el('a', { href: url, download: name });
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
  if (announce) toast(t('files.saved', { name }), 'check');
}

function imageFrom(data: Uint8Array | string, type: string, alt: string): HTMLImageElement {
  const url = URL.createObjectURL(new Blob([data as BlobPart], { type }));
  const image = el('img', { src: url, alt, draggable: false });
  image.addEventListener('load', () => URL.revokeObjectURL(url), { once: true });
  return image;
}

function toast(text: string, kind: 'check' | 'warn' = 'check') {
  const node = el('div', { className: `toast ${kind}` }, icon(kind), el('span', { textContent: text }));
  $('toasts').append(node);
  while ($('toasts').childElementCount > 3) $('toasts').firstElementChild!.remove();
  setTimeout(() => node.classList.add('leaving'), kind === 'warn' ? 7000 : 3200);
  setTimeout(() => node.remove(), kind === 'warn' ? 7400 : 3600);
}

function errorNote(text: string): HTMLElement {
  return el('div', { className: 'callout error' }, icon('warn'), el('span', { textContent: text }));
}

/** A button with an optional icon; async handlers show a busy state and report errors as a toast. */
function button(label: string, onClick: () => unknown, options: { primary?: boolean; icon?: string; title?: string; className?: string } = {}) {
  const b = el('button', { type: 'button', className: [options.primary ? 'primary' : '', options.className ?? ''].join(' ').trim() });
  if (options.icon) b.append(icon(options.icon));
  if (label) b.append(el('span', { textContent: label }));
  if (options.title) {
    b.title = options.title;
    b.setAttribute('aria-label', options.title);
  }
  b.addEventListener('click', () => busy(b, onClick));
  return b;
}

async function busy(b: HTMLButtonElement, action: () => unknown) {
  if (b.classList.contains('busy')) return;
  b.classList.add('busy');
  b.setAttribute('aria-busy', 'true');
  try {
    await action();
  } catch (error) {
    toast(message(error), 'warn');
  } finally {
    b.classList.remove('busy');
    b.removeAttribute('aria-busy');
  }
}

function ask(text: string, ok: string): Promise<boolean> {
  const dialog = $<HTMLDialogElement>('confirm');
  $('confirm-text').textContent = text;
  $('confirm-ok').textContent = ok;
  dialog.showModal();
  return new Promise((resolve) => {
    const done = (value: boolean) => {
      dialog.close();
      $('confirm-ok').removeEventListener('click', yes);
      $('confirm-cancel').removeEventListener('click', no);
      dialog.removeEventListener('cancel', no);
      resolve(value);
    };
    const yes = () => done(true);
    const no = () => done(false);
    $('confirm-ok').addEventListener('click', yes);
    $('confirm-cancel').addEventListener('click', no);
    dialog.addEventListener('cancel', no);
  });
}

function chips(container: HTMLElement, options: { value: string; label: string }[], selected: string, onChange: (value: string) => void) {
  container.replaceChildren(...options.map((o) => el('button', { type: 'button', textContent: o.label, dataset: { value: o.value } })));
  setupChips(container, selected, onChange);
}

function setupChips(container: HTMLElement, selected: string, onChange: (value: string) => void) {
  const buttons = [...container.querySelectorAll<HTMLButtonElement>('button')];
  const select = (value: string) => {
    container.dataset.value = value;
    buttons.forEach((b) => b.setAttribute('aria-checked', String(b.dataset.value === value)));
  };
  buttons.forEach((b) => {
    b.setAttribute('role', 'radio');
    b.type = 'button';
    b.addEventListener('click', () => {
      select(b.dataset.value!);
      onChange(b.dataset.value!);
    });
  });
  select(selected);
}

function setupSegments(root: HTMLElement) {
  const tabs = [...root.querySelectorAll<HTMLButtonElement>('.segmented [data-pane]')];
  const panes = [...root.querySelectorAll<HTMLElement>('.pane[data-pane]')];
  const show = (name: string) => {
    tabs.forEach((b) => b.setAttribute('aria-selected', String(b.dataset.pane === name)));
    panes.forEach((p) => (p.hidden = p.dataset.pane !== name));
    root.dispatchEvent(new CustomEvent('pane', { detail: name }));
  };
  tabs.forEach((b) => b.addEventListener('click', () => show(b.dataset.pane!)));
  show(tabs[0].dataset.pane!);
}

// MARK: - Sections and routing

type OpenFile = { name: string; bytes: Uint8Array };
type View = { actions: HTMLElement[]; body: HTMLElement; status: (Node | string)[]; keydown?: (e: KeyboardEvent) => void };
type FileSection = {
  id: string; app: string; title: string; extensions: string[]; formats: string;
  view: (file: OpenFile, kind: string) => Promise<View>;
  open?: (file: OpenFile) => Promise<void>;
  keydown?: (e: KeyboardEvent) => void;
};

const SECTION_IDS = ['documents', 'spreadsheets', 'presentations', 'pdf', 'labels', 'signatures'];
const fileSections: FileSection[] = [];
const toolSetup: Record<string, () => void> = { labels: setupLabels, signatures: setupSignatures };
const initialised = new Set<string>();
let current = '';

function route() {
  const requested = location.hash.slice(1);
  const id = SECTION_IDS.includes(requested) ? requested : 'documents';
  current = id;
  document.querySelectorAll<HTMLElement>('.nav-item').forEach((a) => {
    if (a.dataset.section === id) a.setAttribute('aria-current', 'page');
    else a.removeAttribute('aria-current');
  });
  document.querySelectorAll<HTMLElement>('main > .section').forEach((s) => (s.hidden = s.id !== id));
  if (!initialised.has(id) && toolSetup[id]) {
    initialised.add(id);
    toolSetup[id]();
  }
  $('main').scrollTop = 0;
}

function sectionFor(name: string): FileSection | undefined {
  const ext = extension(name);
  return fileSections.find((s) => s.extensions.includes(ext));
}

async function openFile(file: OpenFile, section = sectionFor(file.name)) {
  if (!section) {
    toast(t('files.unsupported', { extension: extension(file.name) }), 'warn');
    return;
  }
  if (location.hash !== `#${section.id}`) location.hash = section.id;
  else route();
  remember(file, section);
  await section.open!(file);
}

async function openPicked(input: HTMLInputElement, section?: FileSection) {
  const picked = input.files?.[0];
  if (!picked) return;
  const file = { name: picked.name, bytes: new Uint8Array(await picked.arrayBuffer()) };
  input.value = '';
  await openFile(file, sectionFor(file.name) ?? section);
}

// Files opened in this tab, newest first; they stay in memory only.
const recent: { file: OpenFile; section: FileSection }[] = [];

function remember(file: OpenFile, section: FileSection) {
  const at = recent.findIndex((r) => r.file.name === file.name && r.file.bytes.length === file.bytes.length);
  if (at >= 0) recent.splice(at, 1);
  recent.unshift({ file, section });
  recent.splice(8);
  renderRecent();
}

function recentButton(entry: { file: OpenFile; section: FileSection }, className: string) {
  const b = el('button', { type: 'button', className: `${className} app-${entry.section.app}` },
    el('span', { className: 'app-icon small' }, icon(entry.section.app)),
    el('span', { className: 'recent-name', textContent: entry.file.name, dir: 'auto' }),
    el('span', { className: 'recent-size', textContent: bytesText(entry.file.bytes.length) }));
  b.addEventListener('click', () => openFile(entry.file, entry.section));
  return b;
}

function renderRecent() {
  $('recent').hidden = recent.length === 0;
  $('recent-list').replaceChildren(...recent.map((entry) => el('li', {}, recentButton(entry, 'recent-item'))));
  for (const section of fileSections) {
    const list = document.querySelector<HTMLElement>(`#${section.id} .empty-recent`);
    if (!list) continue;
    const mine = recent.filter((r) => r.section === section);
    list.hidden = mine.length === 0;
    list.querySelector('.recent-cards')!.replaceChildren(...mine.map((entry) => recentButton(entry, 'recent-card')));
  }
}

function buildFileSection(section: FileSection) {
  const input = el('input', { type: 'file', id: `file-input-${section.id}`, className: 'visually-hidden',
    accept: section.extensions.map((e) => `.${e}`).join(',') });
  input.tabIndex = -1;
  input.addEventListener('change', () => openPicked(input, section));
  const pick = () => input.click();

  const title = el('h1', { textContent: section.title });
  const subtitle = el('p', { textContent: t(`${section.id}.tagline`) });
  const headActions = el('div', { className: 'head-actions' });
  const toolbar = el('div', { className: 'toolbar', hidden: true });
  const content = el('div', { className: 'content' });
  const status = el('footer', { className: 'statusbar', hidden: true });

  const empty = el('div', { className: 'empty' },
    el('div', { className: 'dropzone' },
      el('span', { className: 'app-icon xl' }, icon(section.app)),
      el('h2', { textContent: t(`${section.id}.empty`) }),
      el('p', { textContent: t(`${section.id}.emptyText`) }),
      el('div', { className: 'actions center' }, button(t('files.openFile'), pick, { primary: true, icon: 'open' })),
      el('p', { className: 'hint', textContent: t('files.dropHere') }),
      el('p', { className: 'formats', dir: 'ltr' }, ...section.formats.split(' ').map((f) => el('span', { textContent: f })))),
    el('div', { className: 'empty-recent', hidden: true },
      el('p', { className: 'nav-heading' }, icon('clock'), el('span', { textContent: t('recent.title') })),
      el('div', { className: 'recent-cards' })));

  const showEmpty = () => {
    title.textContent = section.title;
    subtitle.textContent = t(`${section.id}.tagline`);
    headActions.replaceChildren();
    toolbar.hidden = true;
    status.hidden = true;
    content.replaceChildren(empty);
    section.keydown = undefined;
  };

  section.open = async (file) => {
    const kind = extension(file.name);
    title.textContent = file.name;
    subtitle.textContent = bytesText(file.bytes.length);
    headActions.replaceChildren(
      button(t('files.openFile'), pick, { icon: 'open' }),
      button('', showEmpty, { icon: 'close', title: t('files.close'), className: 'icon-only' }));
    toolbar.hidden = true;
    status.hidden = true;
    content.replaceChildren(el('div', { className: 'loading' }, el('span', { className: 'spinner' }),
      el('span', { textContent: t('files.opening', { name: file.name }) })));
    try {
      const view = await section.view(file, kind);
      toolbar.replaceChildren(...view.actions);
      toolbar.hidden = view.actions.length === 0;
      status.replaceChildren(...view.status);
      status.hidden = view.status.length === 0;
      content.replaceChildren(view.body);
      section.keydown = view.keydown;
    } catch (error) {
      content.replaceChildren(el('div', { className: 'empty' }, el('div', { className: 'dropzone failed' },
        el('span', { className: 'app-icon xl' }, icon('warn')),
        el('h2', { textContent: t('files.failed') }),
        el('p', { className: 'error-text', textContent: message(error) }),
        el('div', { className: 'actions center' }, button(t('files.openFile'), pick, { primary: true, icon: 'open' })))));
    }
  };

  const node = el('section', { id: section.id, className: `section file-section app-${section.app}`, hidden: true },
    el('header', { className: 'section-head' },
      el('span', { className: 'app-icon large' }, icon(section.app)),
      el('div', { className: 'section-title' }, title, subtitle),
      headActions),
    toolbar, content, status, input);
  $('main').append(node);
  showEmpty();
}

function saveGroup(formats: string[], save: (to: string) => Promise<void>): HTMLElement {
  return el('div', { className: 'save-group' },
    el('span', { className: 'toolbar-label' }, icon('download'), el('span', { textContent: t('common.saveAs') })),
    el('div', { className: 'segmented-buttons' }, ...formats.map((to) => button(to.toUpperCase(), () => save(to)))));
}

// MARK: - Documents

function documentKind(kind: string): string {
  return kind === 'markdown' ? 'md' : kind === 'htm' ? 'html' : kind;
}

// The page around the document's own HTML: A4 paper with margins. Links cannot navigate (the frame is sandboxed
// without popups or top navigation) and the frame runs no scripts.
const PAPER_STYLE = `<base target="_blank"><style>
html{background:#fff;color:#1d211e;color-scheme:light}
body{margin:0;padding:72px 84px 96px;font:16px/1.5 "Helvetica Neue",Arial,"Noto Sans",sans-serif;overflow:hidden;word-wrap:break-word}
img{max-width:100%;height:auto}table{border-collapse:collapse;max-width:100%}td,th{vertical-align:top}
a{color:#2f5f86;pointer-events:none}
@media (max-width:600px){body{padding:32px 24px 48px}}
</style>`;

async function viewDocument(file: OpenFile, rawKind: string): Promise<View> {
  const kind = documentKind(rawKind);
  const [html, info] = await Promise.all([
    call<string>('documentHtml', file.bytes, kind),
    json<{ words: number; characters: number; paragraphs: number }>('documentInfo', file.bytes, kind),
  ]);
  const frame = el('iframe', { className: 'paper', title: t('doc.preview') });
  // Same origin (so the page can size the frame to its content) but no scripts, forms, popups or navigation.
  frame.setAttribute('sandbox', 'allow-same-origin');
  frame.srcdoc = /<head[^>]*>/i.test(html) ? html.replace(/<head[^>]*>/i, (m) => m + PAPER_STYLE) : PAPER_STYLE + html;
  const fit = () => {
    const doc = frame.contentDocument;
    if (doc?.documentElement) frame.style.height = `${Math.max(doc.documentElement.scrollHeight, 400)}px`;
  };
  frame.addEventListener('load', () => {
    fit();
    new ResizeObserver(fit).observe(frame);
  });
  const save = async (to: string) => {
    let output: Uint8Array;
    try {
      output = await call<Uint8Array>('documentConvert', file.bytes, kind, to, false);
    } catch (error) {
      const text = message(error);
      if (!/package parts/.test(text)) throw error;
      if (!(await ask(t('doc.convertAnyway', { error: text, format: to.toUpperCase() }), t('common.convertAnyway')))) return;
      output = await call<Uint8Array>('documentConvert', file.bytes, kind, to, true);
    }
    download(output, `${stem(file.name)}.${to}`, MIME[to]);
  };
  return {
    actions: [saveGroup(['docx', 'odt', 'rtf', 'md', 'html', 'txt'], save)],
    body: el('div', { className: 'desk' }, frame),
    status: [
      el('span', { textContent: t('doc.stats', { words: number(info.words), characters: number(info.characters), paragraphs: number(info.paragraphs) }) }),
      el('span', { className: 'status-right', textContent: kind.toUpperCase() }),
    ],
  };
}

// MARK: - Spreadsheets

type Sheet = { name: string; rows: number; cols: number; truncated: boolean; cells: [number, number, string][] };
const MAX_ROWS = 1000;
const MAX_COLS = 100;
const NUMERIC = /^[-−+(]?[$€£¥₽₹]?\s?\d[\d\s.,'’ ]*(%|[$€£¥₽₹]|\s?[A-Z]{3})?\)?$/;

function columnName(index: number): string {
  let name = '';
  for (let n = index + 1; n > 0; n = Math.floor((n - 1) / 26)) name = String.fromCharCode(65 + ((n - 1) % 26)) + name;
  return name;
}

async function viewSpreadsheet(file: OpenFile, kind: string): Promise<View> {
  const book = await json<{ readOnly: boolean; sheets: Sheet[] }>('spreadsheetJson', file.bytes, kind, MAX_ROWS, MAX_COLS);
  let currentSheet = 0;
  const nameBox = el('span', { className: 'namebox', textContent: 'A1' });
  const valueBox = el('span', { className: 'valuebox', dir: 'auto' });
  const scroll = el('div', { className: 'grid-scroll', dir: 'ltr', tabIndex: 0 });
  const tabs = el('div', { className: 'sheet-tabs', role: 'tablist' });
  const size = el('span');
  const note = el('span', { className: 'status-right' });
  let selected: HTMLElement | null = null;

  const select = (cell: HTMLElement) => {
    selected?.classList.remove('selected');
    selected = cell;
    cell.classList.add('selected');
    nameBox.textContent = cell.dataset.ref!;
    valueBox.textContent = cell.textContent;
  };

  const render = () => {
    const sheet = book.sheets[currentSheet];
    const used = sheet.cells.reduce(([r, c], [row, col]) => [Math.max(r, row + 1), Math.max(c, col + 1)], [0, 0]);
    const rows = Math.min(Math.max(sheet.rows, used[0], 30), MAX_ROWS);
    const cols = Math.min(Math.max(sheet.cols, used[1], 12), MAX_COLS);
    const grid: string[][] = Array.from({ length: rows }, () => Array(cols).fill(''));
    for (const [r, c, text] of sheet.cells) if (r < rows && c < cols) grid[r][c] = text;
    const head = el('tr', {}, el('th', { className: 'corner' }),
      ...Array.from({ length: cols }, (_, c) => el('th', { textContent: columnName(c), scope: 'col' })));
    const body = grid.map((row, r) => el('tr', {}, el('th', { textContent: String(r + 1), scope: 'row' }),
      ...row.map((text, c) => el('td', { textContent: text, className: NUMERIC.test(text.trim()) ? 'num' : '',
        dataset: { ref: `${columnName(c)}${r + 1}` } }))));
    const table = el('table', { className: 'grid' }, el('thead', {}, head), el('tbody', {}, ...body));
    table.addEventListener('click', (e) => {
      const cell = (e.target as HTMLElement).closest('td');
      if (cell) select(cell);
    });
    scroll.replaceChildren(table);
    scroll.scrollTo(0, 0);
    select(table.querySelector('td')!);
    tabs.querySelectorAll('button').forEach((b, i) => b.setAttribute('aria-selected', String(i === currentSheet)));
    size.textContent = t('sheet.size', { rows: number(used[0]), columns: number(used[1]) });
    note.textContent = sheet.truncated ? t('sheet.truncated', { rows: MAX_ROWS, columns: MAX_COLS }) : '';
  };
  book.sheets.forEach((sheet, i) => {
    const b = el('button', { type: 'button', role: 'tab', textContent: sheet.name, dir: 'auto' });
    b.addEventListener('click', () => { currentSheet = i; render(); });
    tabs.append(b);
  });
  render();

  const save = async (to: string) =>
    download(await call<Uint8Array>('spreadsheetConvert', file.bytes, kind, to), `${stem(file.name)}.${to}`, MIME[to]);
  const valuesCsv = async () => download(await call<Uint8Array>('spreadsheetValuesCsv', file.bytes, kind, currentSheet),
    `${stem(file.name)} - ${book.sheets[currentSheet].name}.csv`, MIME.csv);
  const actions = book.readOnly
    ? [el('span', { className: 'toolbar-note' }, icon('lock'), el('span', { textContent: t('sheet.readOnly') }))]
    : [saveGroup(['xlsx', 'ods', 'csv'], save)];
  actions.push(button(t('sheet.valuesCsv'), valuesCsv, { icon: 'download' }));
  return {
    actions,
    body: el('div', { className: 'sheet-view' }, el('div', { className: 'formula-bar' }, nameBox, valueBox), scroll, tabs),
    status: [size, note],
  };
}

// MARK: - Presentations

type SlideObject = { kind: string; role: 'title' | 'body' | 'footer' | null; text: string; cells: string[][] };
type SlideOutline = { title: string; body: string; notes: string; hidden: boolean; objects: SlideObject[] };

function bulletList(text: string): HTMLElement {
  return el('ul', {}, ...text.split('\n').filter((l) => l.trim()).map((line) => el('li', { textContent: line, dir: 'auto' })));
}

function slideParts(slide: SlideOutline) {
  // Imported slides carry their title in a placeholder object with the title role; slides without placeholders
  // (e.g. written by off42 itself) start with their title text box.
  const titleObject = slide.title ? undefined
    : slide.objects.find((o) => o.role === 'title')
      ?? (slide.objects.every((o) => o.role === null) ? slide.objects.find((o) => o.kind === 'Text') : undefined);
  return { title: slide.title || titleObject?.text || '', rest: slide.objects.filter((o) => o !== titleObject) };
}

function slideCard(slide: SlideOutline, large: boolean): HTMLElement {
  const { title, rest } = slideParts(slide);
  const card = el('div', { className: `slide-card${large ? ' large' : ''}` },
    el('h2', { className: title ? '' : 'untitled', textContent: title || t('slides.untitled'), dir: 'auto' }));
  const body = el('div', { className: 'slide-body' });
  if (slide.body) body.append(bulletList(slide.body));
  for (const object of rest) {
    if (object.cells.length) {
      body.append(el('table', { className: 'slide-table' }, ...object.cells.map((row) =>
        el('tr', {}, ...row.map((text) => el('td', { textContent: text, dir: 'auto' }))))));
    } else if (object.kind === 'Picture') {
      body.append(el('div', { className: 'slide-picture' }, icon('picture'), large ? el('span', { textContent: t('slides.pictureLabel') }) : ''));
    } else if (object.text) {
      if (object.role === 'footer') card.append(el('p', { className: 'slide-footer', textContent: object.text, dir: 'auto' }));
      else body.append(object.role === 'body' ? bulletList(object.text) : el('p', { textContent: object.text, dir: 'auto' }));
    }
  }
  card.append(body);
  return card;
}

async function viewPresentation(file: OpenFile, kind: string): Promise<View> {
  const outline = await json<{ slides: SlideOutline[] }>('presentationOutline', file.bytes, kind);
  const slides = outline.slides;
  let index = 0;
  const list = el('ol', { className: 'thumbs' });
  const stage = el('div', { className: 'slide-stage' });
  const notes = el('div', { className: 'slide-notes' });
  const position = el('span');
  const thumbs = slides.map((slide, i) => {
    const b = el('button', { type: 'button', className: `thumb${slide.hidden ? ' hidden-slide' : ''}` },
      el('span', { className: 'thumb-number', textContent: String(i + 1) }), slideCard(slide, false));
    b.addEventListener('click', () => show(i));
    list.append(el('li', {}, b));
    return b;
  });
  const show = (i: number) => {
    if (!slides.length) return;
    index = Math.max(0, Math.min(slides.length - 1, i));
    thumbs.forEach((b, n) => b.setAttribute('aria-current', String(n === index)));
    thumbs[index].scrollIntoView({ block: 'nearest' });
    const slide = slides[index];
    stage.replaceChildren(slideCard(slide, true));
    notes.replaceChildren(el('p', { className: 'notes-title' }, icon('notes'), el('span', { textContent: t('slides.notesTitle') })),
      el('p', { className: slide.notes ? 'notes-text' : 'notes-text muted', textContent: slide.notes || t('slides.noNotes'), dir: 'auto' }));
    position.textContent = t('slides.position', { index: index + 1, count: slides.length }) +
      (slide.hidden ? ` · ${t('slides.hidden')}` : '');
  };
  show(0);
  const save = async (to: string) =>
    download(await call<Uint8Array>('presentationConvert', file.bytes, kind, to), `${stem(file.name)}.${to}`, MIME[to]);
  return {
    actions: [saveGroup(['pptx', 'odp'], save)],
    body: slides.length
      ? el('div', { className: 'slides-view' }, list, el('div', { className: 'slide-main' }, stage, notes))
      : el('p', { className: 'callout', textContent: t('slides.none') }),
    status: [position, el('span', { className: 'status-right' }, icon('info'), el('span', { textContent: t('slides.outlineNote') }))],
    keydown: (e) => {
      if (['ArrowDown', 'ArrowRight', 'PageDown'].includes(e.key)) show(index + 1);
      else if (['ArrowUp', 'ArrowLeft', 'PageUp'].includes(e.key)) show(index - 1);
      else if (e.key === 'Home') show(0);
      else if (e.key === 'End') show(slides.length - 1);
      else return;
      e.preventDefault();
    },
  };
}

// MARK: - PDF

type PdfInfo = {
  encrypted: boolean; pages: number; metadata: Record<string, string>; bookmarks: string[]; attachments: string[];
  formFields: { name: string; kind: string; value: string }[];
};

function infoCard(iconName: string, title: string, count: number | null, ...body: Node[]): HTMLElement {
  return el('section', { className: 'card info-card' },
    el('h3', {}, icon(iconName), el('span', { textContent: title }), count === null ? '' : el('span', { className: 'badge', textContent: number(count) })),
    ...body);
}

function bookmarkTree(lines: string[]): HTMLElement {
  // pdf42 reports the outline depth as two spaces per level.
  const root = el('ul', { className: 'tree' });
  const stack: HTMLElement[] = [root];
  for (const line of lines) {
    const depth = Math.floor((line.length - line.trimStart().length) / 2);
    while (stack.length > depth + 1) stack.pop();
    while (stack.length < depth + 1) {
      const parent = stack[stack.length - 1];
      const nested = el('ul');
      (parent.lastElementChild ?? parent).append(nested);
      stack.push(nested);
    }
    stack[stack.length - 1].append(el('li', {}, el('span', { textContent: line.trim(), dir: 'auto' })));
  }
  return root;
}

async function viewPdf(file: OpenFile): Promise<View> {
  const info = await json<PdfInfo>('pdfInfo', file.bytes);
  if (info.encrypted) {
    return {
      actions: [],
      body: el('div', { className: 'empty' }, el('div', { className: 'dropzone' },
        el('span', { className: 'app-icon xl' }, icon('lock')),
        el('h2', { textContent: t('pdf.encrypted') }),
        el('p', { textContent: t('pdf.encryptedText') }))),
      status: [],
    };
  }
  const metaKeys = ['Title', 'Author', 'Subject', 'Keywords', 'Creator', 'Producer'].filter((key) => info.metadata[key]);
  const details = el('dl', { className: 'facts' },
    el('dt', { textContent: t('pdf.pages') }), el('dd', { textContent: number(info.pages) }),
    el('dt', { textContent: t('pdf.fileSize') }), el('dd', { textContent: bytesText(file.bytes.length) }),
    ...metaKeys.flatMap((key) => [el('dt', { textContent: t(`pdf.meta.${key}`) }), el('dd', { textContent: info.metadata[key], dir: 'auto' })]));
  const pages = el('div', { className: 'page-stack' },
    ...Array.from({ length: Math.min(info.pages, 3) }, () => el('span', { className: 'page-sheet' })));
  pages.setAttribute('aria-hidden', 'true');
  const preview = el('section', { className: 'card coming' }, pages,
    el('h3', { textContent: t('pdf.comingTitle') }),
    el('p', { textContent: t('pdf.noRendering') }),
    el('span', { className: 'pill', textContent: t('common.comingSoon') }));
  const cards: HTMLElement[] = [infoCard('info', t('pdf.document'), null, details)];
  cards.push(infoCard('bookmark', t('pdf.bookmarks'), info.bookmarks.length,
    info.bookmarks.length ? bookmarkTree(info.bookmarks) : el('p', { className: 'muted', textContent: t('pdf.noBookmarks') })));
  if (info.attachments.length) {
    cards.push(infoCard('attach', t('pdf.attachments'), info.attachments.length,
      el('ul', { className: 'plain-list' }, ...info.attachments.map((a) => el('li', { textContent: a, dir: 'auto' })))));
  }
  if (info.formFields.length) {
    cards.push(infoCard('field', t('pdf.formFields'), info.formFields.length,
      el('table', { className: 'fields' },
        el('thead', {}, el('tr', {}, ...['pdf.fieldName', 'pdf.fieldKind', 'pdf.fieldValue'].map((k) => el('th', { textContent: t(k) })))),
        el('tbody', {}, ...info.formFields.map((f) => el('tr', {},
          el('td', { textContent: f.name, dir: 'auto' }), el('td', { textContent: f.kind }), el('td', { textContent: f.value, dir: 'auto' })))))));
  }
  return {
    actions: [],
    body: el('div', { className: 'pdf-view' }, preview, el('div', { className: 'info-column' }, ...cards)),
    status: [el('span', { textContent: t('pdf.pageCount', { pages: number(info.pages) }) }),
      el('span', { className: 'status-right', textContent: `PDF · ${bytesText(file.bytes.length)}` })],
  };
}

// MARK: - Labels & codes

type Stock = { id: string; name: string; width: number; height: number; columns: number; rows: number;
  marginLeft: number; marginTop: number; gapX: number; gapY: number };
type Mark = { id: string; name: string; category: string; purpose: string; warning?: string; unverified: boolean; needsNumber: boolean };

async function setupLabels() {
  const root = $('labels');
  setupSegments(root);

  // Codes: the preview follows every keystroke.
  const format = $('code-format');
  const value = $<HTMLTextAreaElement>('code-value');
  const width = $<HTMLInputElement>('code-width');
  const caption = $<HTMLInputElement>('code-caption');
  const preview = $('code-preview');
  let generation = 0;
  const renderCode = async () => {
    const mine = ++generation;
    $('code-width-out').textContent = width.value;
    $('code-size').textContent = t('code.size', { width: width.value });
    try {
      const svg = await call<string>('barcodeSvg', format.dataset.value, value.value, Number(width.value), caption.checked);
      if (mine === generation) preview.replaceChildren(imageFrom(svg, MIME.svg, t('code.preview')));
    } catch (error) {
      if (mine === generation) preview.replaceChildren(errorNote(message(error)));
    }
  };
  setupChips(format, 'qr', renderCode);
  [value, width, caption].forEach((c) => c.addEventListener('input', renderCode));
  renderCode();
  $('code-png').addEventListener('click', (e) => busy(e.currentTarget as HTMLButtonElement, async () =>
    download(await call<Uint8Array>('barcodePng', format.dataset.value, value.value, Number(width.value)), `${format.dataset.value}.png`, MIME.png)));
  $('code-svg').addEventListener('click', (e) => busy(e.currentTarget as HTMLButtonElement, async () =>
    download(await call<string>('barcodeSvg', format.dataset.value, value.value, Number(width.value), caption.checked), `${format.dataset.value}.svg`, MIME.svg)));

  // Label sheets: a schematic of the first sheet, drawn from the stock geometry.
  const stocks = await json<Stock[]>('labelStocks');
  const stock = $<HTMLSelectElement>('sheet-stock');
  stock.append(...stocks.map((s) => el('option', { value: s.id,
    textContent: `${s.name} — ${s.columns}×${s.rows}` })),
  el('option', { value: 'custom', textContent: t('labels.custom') }));
  const num = (id: string) => Number($<HTMLInputElement>(id).value) || 0;
  const labelLines = () => $<HTMLTextAreaElement>('sheet-lines').value.split('\n').map((l) => l.trim()).filter(Boolean).map((line) => {
    const [text, code, copies] = line.split('|').map((p) => p.trim());
    return { text: text ?? '', code: code || undefined, copies: copies ? Math.max(1, Math.floor(Number(copies)) || 1) : 1 };
  });
  const geometry = (): Stock => {
    const chosen = stocks.find((s) => s.id === stock.value);
    if (chosen) return chosen;
    const w = Math.max(num('sheet-width'), 5);
    const h = Math.max(num('sheet-height'), 5);
    const gapX = num('sheet-gapx');
    const gapY = num('sheet-gapy');
    const columns = Math.max(1, Math.floor((210 + gapX) / (w + gapX)));
    const rows = Math.max(1, Math.floor((297 + gapY) / (h + gapY)));
    return { id: 'custom', name: '', width: w, height: h, columns, rows, gapX, gapY,
      marginLeft: (210 - columns * w - (columns - 1) * gapX) / 2, marginTop: (297 - rows * h - (rows - 1) * gapY) / 2 };
  };
  const drawSheet = () => {
    $('sheet-custom').hidden = stock.value !== 'custom';
    const g = geometry();
    const labels = labelLines().flatMap((l) => Array.from({ length: Math.min(l.copies, 1000) }, () => l));
    const first = Math.max(1, Math.floor(num('sheet-first')) || 1);
    const slots = g.columns * g.rows;
    const withCode = $<HTMLSelectElement>('sheet-code').value !== '';
    const sheet = $('sheet-preview');
    sheet.replaceChildren(...Array.from({ length: slots }, (_, i) => {
      const label = labels[i - (first - 1)];
      const slot = el('div', { className: `slot${label ? ' filled' : ''}${i < first - 1 ? ' used' : ''}` });
      const col = i % g.columns;
      const row = Math.floor(i / g.columns);
      Object.assign(slot.style, {
        left: `${((g.marginLeft + col * (g.width + g.gapX)) / 210) * 100}%`,
        top: `${((g.marginTop + row * (g.height + g.gapY)) / 297) * 100}%`,
        width: `${(g.width / 210) * 100}%`, height: `${(g.height / 297) * 100}%`,
      });
      if (label) {
        if (withCode && label.code) slot.append(icon('labels', 'icon slot-code'));
        slot.append(el('span', { textContent: label.text }));
      }
      return slot;
    }));
    const pages = Math.max(1, Math.ceil((first - 1 + labels.length) / slots));
    $('sheet-summary').textContent = t('labels.summary', { labels: number(labels.length), slots: number(slots), pages: number(pages),
      size: `${g.width} × ${g.height} mm` });
  };
  root.querySelectorAll('#sheet-stock, #sheet-code, .pane[data-pane="sheets"] input, #sheet-lines')
    .forEach((c) => c.addEventListener('input', drawSheet));
  drawSheet();
  $('sheet-pdf').addEventListener('click', (e) => busy(e.currentTarget as HTMLButtonElement, async () => {
    const statusLine = $('sheet-status');
    statusLine.textContent = '';
    const labels = labelLines();
    try {
      const pdf = await call<Uint8Array>('labelSheetPdf', JSON.stringify({
        stock: stock.value, width: num('sheet-width'), height: num('sheet-height'), gapX: num('sheet-gapx'),
        gapY: num('sheet-gapy'), firstSlot: num('sheet-first'), codeFormat: $<HTMLSelectElement>('sheet-code').value || undefined,
        labels,
      }));
      const pages = (await json<PdfInfo>('pdfInfo', pdf)).pages;
      statusLine.textContent = t('labels.status', { labels: labels.reduce((n, l) => n + l.copies, 0), pages });
      download(pdf, 'labels.pdf', MIME.pdf);
    } catch (error) {
      statusLine.textContent = message(error);
      throw error;
    }
  }));

  // Product marks: label42 has mark names in 11 of the 20 languages (English otherwise).
  const marks = await json<Mark[]>('productMarks', language);
  const categories = [...new Set(marks.map((m) => m.category))].sort();
  const search = $<HTMLInputElement>('marks-search');
  const grid = $('marks-grid');
  const detail = $('marks-detail');
  const categoryChips = $('marks-category');
  const tiles = new Map<string, HTMLElement>();
  for (const mark of marks) {
    const art = el('span', { className: 'mark-art' });
    const tile = el('button', { type: 'button', className: 'mark-tile', title: mark.name }, art,
      el('span', { className: 'mark-name', textContent: mark.name, dir: 'auto' }));
    tile.addEventListener('click', () => showMark(mark, tile));
    tiles.set(mark.id, tile);
    call<string>('productMarkSvg', mark.id, 20).then(
      (svg) => art.append(imageFrom(svg, MIME.svg, '')),
      () => art.append(icon('warn')));
  }
  const showMark = (mark: Mark, tile: HTMLElement) => {
    tiles.forEach((t) => t.removeAttribute('aria-current'));
    tile.setAttribute('aria-current', 'true');
    detail.hidden = false;
    const widthInput = el('input', { type: 'number', min: '3', max: '250', value: '20' });
    const w = () => Number(widthInput.value) || 20;
    const art = el('div', { className: 'stage paper-stage mark-preview' });
    call<string>('productMarkSvg', mark.id, 40).then((svg) => art.replaceChildren(imageFrom(svg, MIME.svg, mark.name)), () => {});
    detail.replaceChildren(
      el('div', { className: 'detail-head' },
        el('div', {}, el('p', { className: 'eyebrow', textContent: mark.category }), el('h3', { textContent: mark.name, dir: 'auto' })),
        button('', () => { detail.hidden = true; tile.removeAttribute('aria-current'); }, { icon: 'close', title: t('files.close'), className: 'icon-only' })),
      art,
      el('p', { textContent: mark.purpose, dir: 'auto' }),
      mark.warning ? el('p', { className: 'callout warning' }, icon('warn'), el('span', { textContent: mark.warning, dir: 'auto' })) : '',
      mark.unverified ? el('p', { className: 'callout warning' }, icon('warn'), el('span', { textContent: t('marks.unverified') })) : '',
      mark.needsNumber ? el('p', { className: 'hint', textContent: t('marks.needsNumber') }) : '',
      el('label', { className: 'field' }, el('span', { className: 'field-label', textContent: t('common.widthMm') }), widthInput),
      el('div', { className: 'actions' },
        button(t('common.downloadSvg'), async () => download(await call<string>('productMarkSvg', mark.id, w()), `${mark.id}.svg`, MIME.svg), { icon: 'download' }),
        button(t('common.downloadPng'), async () => download(await call<Uint8Array>('productMarkPng', mark.id, w()), `${mark.id}.png`, MIME.png), { icon: 'download', primary: true })));
    detail.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  };
  const filter = () => {
    const q = search.value.trim().toLowerCase();
    const category = categoryChips.dataset.value ?? '';
    const shown = marks.filter((m) => (!category || m.category === category) &&
      // Short queries ("CE") match names and ids only; longer ones also the description.
      (!q || `${m.name} ${m.id} ${m.category}${q.length > 3 ? ` ${m.purpose}` : ''}`.toLowerCase().includes(q)));
    grid.replaceChildren(...shown.map((m) => tiles.get(m.id)!));
    if (!shown.length) grid.append(el('p', { className: 'muted empty-line', textContent: t('marks.noMatch') }));
    $('marks-count').textContent = t('marks.count', { count: number(shown.length) });
  };
  chips(categoryChips, [{ value: '', label: t('marks.all') }, ...categories.map((c) => ({ value: c, label: c }))], '', filter);
  search.addEventListener('input', filter);
  filter();
}

// MARK: - Signatures & stamps

type Option = { value: number | string; title: string };
type Field = { key: string; label: string; example: string; multiline: boolean; date: boolean };
type Template = { id: string; title: string; category: string; round: boolean; ink: string; fields: Field[] };

async function setupSignatures() {
  setupSegments($('signatures'));
  const options = await json<{ forms: Option[]; pens: Option[]; inks: Option[] }>('signatureOptions');
  // The core names these in English; the interface translates the fixed option sets.
  const formSelect = $<HTMLSelectElement>('sig-form');
  formSelect.replaceChildren(...options.forms.map((o) => el('option', { value: String(o.value), textContent: t(`sig.form.${o.value}`) })));
  const pen = $('sig-pen');
  const ink = $('sig-ink');
  const name = $<HTMLInputElement>('sig-name');
  const results = $('sig-results');

  type Variant = { design: string; impression: string; frame: HTMLElement; png?: Uint8Array };
  let variants: Variant[] = [];
  let generation = 0;
  const draw = async (variant: Variant, mine = generation) => {
    try {
      const png = await call<Uint8Array>('signaturePng', name.value, variant.design, variant.impression, ink.dataset.value,
        Number(pen.dataset.value), 130);
      if (mine !== generation) return;
      variant.png = png;
      variant.frame.replaceChildren(imageFrom(png, MIME.png, t('sig.alt', { name: name.value })));
    } catch (error) {
      if (mine === generation) variant.frame.replaceChildren(errorNote(message(error)));
    }
  };
  const redraw = () => {
    const mine = ++generation;
    return Promise.all(variants.map((v) => draw(v, mine)));
  };
  const generate = async () => {
    const designs = await json<string[]>('signatureDesigns', 6, Number(formSelect.value), seed());
    variants = designs.map((design) => ({ design, impression: seed(), frame: el('div', { className: 'signature-paper' }, el('span', { className: 'spinner' })) }));
    results.replaceChildren(...variants.map((variant, i) => el('article', { className: 'variant card' },
      variant.frame,
      el('div', { className: 'variant-foot' },
        el('span', { className: 'variant-number', textContent: t('sig.variant', { number: i + 1 }) }),
        button('', async () => { variant.impression = seed(); await draw(variant); }, { icon: 'refresh', title: t('common.newImpression'), className: 'icon-only' }),
        button('', () => variant.png && download(variant.png, `signature-${stem(name.value.trim().replace(/\s+/g, '-')) || 'off42'}-${i + 1}.png`, MIME.png),
          { icon: 'download', title: t('common.downloadPng'), className: 'icon-only' })))));
    await redraw();
  };
  chips(pen, options.pens.map((o) => ({ value: String(o.value), label: t(`sig.pen.${o.value}`) })), String(options.pens[0].value), redraw);
  ink.replaceChildren(...options.inks.map((o) => el('button', { type: 'button', className: 'swatch', title: t(`ink.${o.value}`),
    dataset: { value: String(o.value) } }, el('span', { className: `ink-dot ink-${o.value}` }))));
  ink.querySelectorAll('button').forEach((b) => b.setAttribute('aria-label', b.title));
  setupChips(ink, 'blue', redraw);
  formSelect.addEventListener('change', generate);
  name.addEventListener('input', debounce(redraw, 250));
  $('sig-generate').addEventListener('click', (e) => busy(e.currentTarget as HTMLButtonElement, generate));
  generate();

  // Stamps
  const templates = await json<Template[]>('stampTemplates', language);
  $<HTMLSelectElement>('stamp-ink').replaceChildren(el('option', { value: '', textContent: t('stamp.defaultInk') }),
    ...options.inks.map((o) => el('option', { value: String(o.value), textContent: t(`ink.${o.value}`) })));
  const select = $<HTMLSelectElement>('stamp-template');
  const groups = new Map<string, HTMLOptGroupElement>();
  for (const template of templates) {
    if (!groups.has(template.category)) groups.set(template.category, el('optgroup', { label: template.category }));
    groups.get(template.category)!.append(el('option', { value: template.id, textContent: template.title }));
  }
  select.replaceChildren(...groups.values());
  // A template from the user's region; stamps draw Latin, Cyrillic and Greek text only.
  const preferred = ({ en: 'us.corporate', ru: 'ru.ooo', tr: 'tr.kase', de: 'de.firm', fr: 'fr.tampon', it: 'it.timbro',
    es: 'es.sello', pt: 'pt.carimbo', pl: 'pl.pieczatka', hu: 'eu.round' } as Record<string, string>)[language] ?? 'office.approved';
  if (templates.some((tpl) => tpl.id === preferred)) select.value = preferred;
  const fields = $('stamp-fields');
  const preview = $('stamp-preview');
  let impression = seed();
  let stampGeneration = 0;
  const inputs = new Map<string, HTMLInputElement | HTMLTextAreaElement>();
  const currentTemplate = () => templates.find((tpl) => tpl.id === select.value)!;
  // The impression (tilt, pressure, grain) is the same at every scale: preview small, download large.
  const render = (scale: number) => {
    const values = Object.fromEntries([...inputs].map(([k, input]) => [k, input.value]));
    return call<Uint8Array>('stampPng', select.value, JSON.stringify(values), $<HTMLSelectElement>('stamp-ink').value, impression, scale);
  };
  const drawStamp = async () => {
    const mine = ++stampGeneration;
    preview.classList.add('updating');
    try {
      const png = await render(0.6);
      if (mine === stampGeneration) preview.replaceChildren(imageFrom(png, MIME.png, currentTemplate().title));
    } catch (error) {
      if (mine === stampGeneration) preview.replaceChildren(errorNote(message(error)));
    } finally {
      if (mine === stampGeneration) preview.classList.remove('updating');
    }
  };
  const live = debounce(drawStamp, 200);
  const showFields = () => {
    inputs.clear();
    fields.replaceChildren(...currentTemplate().fields.map((f) => {
      const input = f.multiline ? el('textarea', { rows: 3 }) : el('input');
      input.value = f.date ? today() : f.example;
      input.addEventListener('input', live);
      inputs.set(f.key, input);
      return el('label', { className: 'field' }, el('span', { className: 'field-label', textContent: f.label }), input);
    }));
  };
  select.addEventListener('change', () => { showFields(); drawStamp(); });
  $('stamp-ink').addEventListener('change', drawStamp);
  $('stamp-again').addEventListener('click', () => { impression = seed(); drawStamp(); });
  $('stamp-download').addEventListener('click', (e) => busy(e.currentTarget as HTMLButtonElement, async () =>
    download(await render(2), `stamp-${select.value}.png`, MIME.png)));
  showFields();
  drawStamp();
}

// MARK: - Start

function setupDrop() {
  const overlay = $('drop-overlay');
  let depth = 0;
  const hasFiles = (e: DragEvent) => [...(e.dataTransfer?.types ?? [])].includes('Files');
  window.addEventListener('dragenter', (e) => {
    if (!hasFiles(e)) return;
    depth += 1;
    overlay.hidden = false;
  });
  window.addEventListener('dragleave', () => {
    depth = Math.max(0, depth - 1);
    if (!depth) overlay.hidden = true;
  });
  window.addEventListener('dragover', (e) => hasFiles(e) && e.preventDefault());
  window.addEventListener('drop', async (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    depth = 0;
    overlay.hidden = true;
    const dropped = e.dataTransfer?.files[0];
    if (!dropped) return;
    const section = fileSections.find((s) => s.id === current);
    await openFile({ name: dropped.name, bytes: new Uint8Array(await dropped.arrayBuffer()) }, sectionFor(dropped.name) ?? section);
  });
}

function splashProgress({ loaded, total }: Progress, translated: boolean) {
  const percent = total ? Math.min(100, Math.round((loaded / total) * 100)) : 0;
  $('splash-fill').style.width = `${percent}%`;
  $('splash').setAttribute('aria-valuenow', String(percent));
  if (!translated) return;
  $('splash-text').textContent = t('app.loadingPercent', { percent });
  $('splash-note').textContent = total ? t('app.loadingBytes', { loaded: bytesText(loaded), total: bytesText(total) }) : '';
}

async function start() {
  let translated = false;
  let last: Progress = { loaded: 0, total: 0 };
  // The engine download starts at once, in parallel with the interface catalog.
  const engine = startEngine((progress) => {
    last = progress;
    splashProgress(progress, translated);
  });
  await loadLanguage(chooseLanguage());
  translatePage();
  translated = true;
  splashProgress(last, true);
  placeIcons();
  languagePicker($<HTMLSelectElement>('language'));
  $<HTMLAnchorElement>('site-link').href = `../${language}/`;
  $<HTMLAnchorElement>('brand-link').href = `../${language}/`;

  let version: string;
  try {
    version = await engine;
  } catch (error) {
    $('splash').classList.add('failed');
    $('splash-text').textContent = t('app.engineFailed', { error: message(error) });
    $('splash-note').replaceChildren(button(t('app.reload'), () => location.reload(), { primary: true, icon: 'refresh' }));
    return;
  }
  $('engine-version').textContent = t('app.engine', { version });

  fileSections.push(
    { id: 'documents', app: 'doc', title: t('nav.documents'), extensions: ['docx', 'odt', 'rtf', 'md', 'markdown', 'html', 'htm', 'txt'],
      formats: 'DOCX ODT RTF MD HTML TXT', view: viewDocument },
    { id: 'spreadsheets', app: 'xls', title: t('nav.spreadsheets'), extensions: ['xlsx', 'ods', 'xls', 'csv'],
      formats: 'XLSX ODS XLS CSV', view: viewSpreadsheet },
    { id: 'presentations', app: 'ppt', title: t('nav.presentations'), extensions: ['pptx', 'odp'],
      formats: 'PPTX ODP', view: viewPresentation },
    { id: 'pdf', app: 'pdf', title: 'PDF', extensions: ['pdf'], formats: 'PDF', view: viewPdf },
  );
  const tools = $('labels');
  fileSections.forEach(buildFileSection);
  $('main').append(tools, $('signatures'));
  setupDrop();
  window.addEventListener('hashchange', route);
  document.querySelectorAll<HTMLElement>('.nav-item').forEach((item) =>
    item.addEventListener('click', () => (location.hash = item.dataset.section!)));
  document.addEventListener('keydown', (e) => {
    const target = e.target as HTMLElement;
    if (e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName)) return;
    if ($<HTMLDialogElement>('confirm').open) return;
    fileSections.find((s) => s.id === current)?.keydown?.(e);
  });
  route();
  $('app').hidden = false;
  $('splash').classList.add('done');
  setTimeout(() => $('splash').remove(), 400);
  document.body.dataset.ready = 'true';
}

start();
