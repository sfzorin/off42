// The OFF42 web app: a browser shell over the off42 Rust cores (crates/off42-wasm), which run in a Web Worker
// (engine.worker.ts). Files are read from the user's disk, processed in this tab and offered back as downloads;
// nothing is sent over the network.
//
// Layout: one screen per app (Documents, Spreadsheets, Presentations, PDF, Labels & codes, Signatures & stamps),
// switched with the ☰ drawer and the URL hash. A file app shows its home screen (open strip, recent files) until a
// file is open, then an editor: title row, menu bar, one toolbar and the content on a grey canvas. Menus and the
// toolbar carry only operations that work today.
import { call, json, startEngine, type Progress } from './engine';
import { appLogo, icon, placeIcons } from './icons';
import { chooseLanguage, language, LANGUAGES, loadLanguage, switchLanguage, t, translatePage } from './i18n';

// MARK: - Helpers

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

type Props<K extends keyof HTMLElementTagNameMap> = Partial<Omit<HTMLElementTagNameMap[K], 'dataset' | 'role'>> & {
  dataset?: Record<string, string>; role?: string; aria?: Record<string, string>;
};

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K, props: Props<K> = {}, ...children: (Node | string | false | null | undefined)[]
): HTMLElementTagNameMap[K] {
  const { dataset, role, aria, ...rest } = props;
  const node = Object.assign(document.createElement(tag), rest);
  if (dataset) Object.assign(node.dataset, dataset);
  if (role) node.setAttribute('role', role);
  if (aria) for (const [k, v] of Object.entries(aria)) node.setAttribute(`aria-${k}`, v);
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

const number = (n: number) => new Intl.NumberFormat(language, { maximumFractionDigits: 10 }).format(n);
const percent = (n: number) => new Intl.NumberFormat(language, { style: 'percent' }).format(n / 100);

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

const MAC = /Mac|iPhone|iPad/.test(navigator.platform);
const shortcut = (key: string) => (MAC ? `⌘${key}` : `Ctrl+${key}`);
const rtl = () => document.documentElement.dir === 'rtl';

const MIME: Record<string, string> = {
  docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  odt: 'application/vnd.oasis.opendocument.text',
  rtf: 'application/rtf',
  md: 'text/markdown',
  html: 'text/html',
  txt: 'text/plain',
  xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  xls: 'application/vnd.ms-excel',
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
  if (announce) notify(t('files.saved', { name }));
}

function imageFrom(data: Uint8Array | string, type: string, alt: string): HTMLImageElement {
  const url = URL.createObjectURL(new Blob([data as BlobPart], { type }));
  const image = el('img', { src: url, alt, draggable: false });
  image.addEventListener('load', () => URL.revokeObjectURL(url), { once: true });
  return image;
}

// MARK: - Snackbar: one message at a time, bottom start, replaced by the next one.

let snackTimer: ReturnType<typeof setTimeout> | undefined;

function notify(text: string, kind: 'info' | 'warn' = 'info') {
  const bar = $('snackbar');
  const close = el('button', { type: 'button', className: 'snack-close', title: t('files.close'), aria: { label: t('files.close') } }, icon('close'));
  close.addEventListener('click', () => (bar.hidden = true));
  bar.replaceChildren(kind === 'warn' ? icon('warn') : '', el('span', { className: 'snack-text', textContent: text }), close);
  bar.className = `snackbar ${kind}`;
  bar.hidden = false;
  void bar.offsetWidth; // restart the entrance animation
  bar.classList.add('show');
  clearTimeout(snackTimer);
  snackTimer = setTimeout(() => (bar.hidden = true), kind === 'warn' ? 9000 : 4500);
}

function errorNote(text: string): HTMLElement {
  return el('div', { className: 'error-note' }, icon('warn'), el('span', { textContent: text }));
}

/** A button with an optional icon; async handlers show a busy state and report errors in the snackbar. */
function button(label: string, onClick: () => unknown, options: { icon?: string; title?: string; className?: string } = {}) {
  const b = el('button', { type: 'button', className: options.className ?? 'outline-button' });
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
    notify(message(error), 'warn');
  } finally {
    b.classList.remove('busy');
    b.removeAttribute('aria-busy');
  }
}

async function attempt(action: () => unknown) {
  try {
    await action();
  } catch (error) {
    notify(message(error), 'warn');
  }
}

/** A toolbar button: icon (and optional text), with its name as tooltip and accessible label. */
function tool(iconName: string, label: string, action: () => unknown, text?: string | HTMLElement): HTMLButtonElement {
  const b = el('button', { type: 'button', className: `tool${text ? ' with-text' : ''}`, title: label, aria: { label } }, icon(iconName));
  if (text) b.append(typeof text === 'string' ? el('span', { textContent: text }) : text);
  b.addEventListener('click', () => busy(b, action));
  return b;
}

const separator = () => el('span', { className: 'tool-sep', role: 'separator' });

// MARK: - Dialogs

function setupDialog(dialog: HTMLDialogElement) {
  dialog.querySelectorAll('[data-close]').forEach((b) => b.addEventListener('click', () => dialog.close()));
  // A click on the backdrop (outside the dialog box) closes it.
  dialog.addEventListener('click', (e) => {
    if (e.target !== dialog) return;
    const r = dialog.getBoundingClientRect();
    if (e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom) dialog.close();
  });
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

function showStats(rows: [string, string][]) {
  $('stats-list').replaceChildren(...rows.flatMap(([label, value]) =>
    [el('dt', { textContent: label }), el('dd', { textContent: value, dir: 'auto' })]));
  $<HTMLDialogElement>('stats').showModal();
}

const showAbout = () => $<HTMLDialogElement>('about').showModal();

// MARK: - Menus
//
// Menus are plain buttons in a fixed-position list. Keyboard: arrows move (and open submenus or the neighbouring
// menu-bar menu), Home/End, Enter/Space activate, Escape closes one level, Tab closes all, a letter jumps.

type Command = {
  label: string; icon?: string; hint?: string; shortcut?: string; run?: () => unknown; submenu?: () => MenuItem[];
  checked?: boolean; radio?: boolean; disabled?: boolean; lang?: string;
};
type MenuItem = Command | { separator: true } | { heading: string };
type MenuSpec = { label: string; items: () => MenuItem[] };

const SEP: MenuItem = { separator: true };
// Where the pointer was when the last menu opened: a menu appearing under a resting pointer must not react to it
// (keyboard users would see submenus open by themselves).
const pointer = { x: -1, y: -1 };
let restingPointer = { x: -1, y: -1 };
document.addEventListener('pointermove', (e) => Object.assign(pointer, { x: e.clientX, y: e.clientY }), true);
const openMenus: { menu: HTMLElement; anchor: HTMLElement }[] = [];
let menubarContext: { open: (index: number) => void; index: number; count: number } | null = null;

function closeMenus(level = 0, focusAnchor = false) {
  let lowest: HTMLElement | undefined;
  while (openMenus.length > level) {
    const { menu, anchor } = openMenus.pop()!;
    menu.remove();
    anchor.setAttribute('aria-expanded', 'false');
    anchor.classList.remove('open');
    lowest = anchor;
  }
  if (level === 0) menubarContext = null;
  if (focusAnchor && lowest) lowest.focus();
}

function placeMenu(menu: HTMLElement, anchor: HTMLElement, side: boolean, alignEnd: boolean) {
  const r = anchor.getBoundingClientRect();
  const vw = document.documentElement.clientWidth;
  const vh = window.innerHeight;
  const w = menu.offsetWidth;
  const h = menu.offsetHeight;
  let left: number;
  let top: number;
  if (side) {
    const after = rtl() ? r.left - w + 2 : r.right - 2;
    const before = rtl() ? r.right - 2 : r.left - w + 2;
    const fits = (x: number) => x >= 8 && x + w <= vw - 8;
    top = r.top - 6;
    if (fits(after)) left = after;
    else if (fits(before)) left = before;
    else {
      // No room beside the item (phones): open below it, so the item itself stays visible and tappable.
      left = rtl() ? r.right - w - 16 : r.left + 16;
      top = r.bottom + 2;
    }
  } else {
    left = alignEnd !== rtl() ? r.right - w : r.left;
    top = r.bottom + 4;
    if (top + h > vh - 8 && r.top - h - 4 > 8) top = r.top - h - 4;
  }
  left = Math.min(Math.max(8, left), vw - w - 8);
  top = Math.max(8, Math.min(top, vh - h - 8));
  menu.style.left = `${Math.max(8, left)}px`;
  menu.style.top = `${top}px`;
}

function showMenu(items: MenuItem[], anchor: HTMLElement, level = 0,
  options: { focus?: 'first' | 'last' | false; alignEnd?: boolean; label?: string } = {}) {
  closeMenus(level);
  restingPointer = { ...pointer };
  const menu = el('div', { className: 'menu', role: 'menu' });
  if (options.label) menu.setAttribute('aria-label', options.label);
  menu.style.maxHeight = `${window.innerHeight - 16}px`;
  const entries: { button: HTMLButtonElement; item: Command }[] = [];
  for (const item of items) {
    if ('separator' in item) {
      menu.append(el('div', { className: 'menu-sep', role: 'separator' }));
      continue;
    }
    if ('heading' in item) {
      menu.append(el('div', { className: 'menu-heading', textContent: item.heading, role: 'presentation' }));
      continue;
    }
    const b = el('button', { type: 'button', className: 'menu-item', disabled: !!item.disabled, tabIndex: -1,
      role: item.checked === undefined ? 'menuitem' : item.radio ? 'menuitemradio' : 'menuitemcheckbox' });
    if (item.checked !== undefined) b.setAttribute('aria-checked', String(item.checked));
    const lead = el('span', { className: 'menu-icon' });
    if (item.checked) lead.append(icon('check'));
    else if (item.icon) lead.append(icon(item.icon));
    const label = el('span', { className: 'menu-label', textContent: item.label });
    if (item.lang) label.lang = item.lang;
    b.append(lead, label);
    if (item.hint) b.append(el('span', { className: 'menu-hint', textContent: item.hint, dir: 'ltr' }));
    if (item.shortcut) b.append(el('span', { className: 'menu-hint', textContent: item.shortcut, dir: 'ltr' }));
    if (item.submenu) {
      b.setAttribute('aria-haspopup', 'menu');
      b.setAttribute('aria-expanded', 'false');
      b.append(el('span', { className: 'menu-chevron' }, icon(rtl() ? 'prev' : 'next')));
    }
    b.addEventListener('click', () => activate(item, b, level));
    b.addEventListener('pointerenter', (e) => {
      if (b.disabled || (e.clientX === restingPointer.x && e.clientY === restingPointer.y)) return;
      b.focus({ preventScroll: true });
      if (item.submenu) {
        if (openMenus[level + 1]?.anchor !== b) showMenu(item.submenu(), b, level + 1, { focus: false });
      } else closeMenus(level + 1);
    });
    entries.push({ button: b, item });
    menu.append(b);
  }
  menu.addEventListener('keydown', (e) => menuKey(e, level, entries));
  document.body.append(menu);
  placeMenu(menu, anchor, level > 0, !!options.alignEnd);
  anchor.setAttribute('aria-expanded', 'true');
  anchor.classList.add('open');
  openMenus.push({ menu, anchor });
  const enabled = entries.filter((e) => !e.item.disabled);
  const checked = enabled.find((e) => e.item.checked);
  if (options.focus) (options.focus === 'last' ? enabled.at(-1) : checked ?? enabled[0])?.button.focus({ preventScroll: true });
  if (checked) checked.button.scrollIntoView({ block: 'nearest' });
}

function activate(item: Command, b: HTMLButtonElement, level: number) {
  if (item.disabled) return;
  if (item.submenu) {
    showMenu(item.submenu(), b, level + 1, { focus: 'first' });
    return;
  }
  closeMenus(0, true);
  if (item.run) attempt(item.run);
}

function menuKey(e: KeyboardEvent, level: number, entries: { button: HTMLButtonElement; item: Command }[]) {
  const enabled = entries.filter((x) => !x.item.disabled);
  const index = enabled.findIndex((x) => x.button === document.activeElement);
  const current = enabled[index];
  const forward = rtl() ? 'ArrowLeft' : 'ArrowRight';
  const back = rtl() ? 'ArrowRight' : 'ArrowLeft';
  const focusAt = (i: number) => enabled[(i + enabled.length) % enabled.length]?.button.focus();
  if (e.key === 'ArrowDown') focusAt(index + 1);
  else if (e.key === 'ArrowUp') focusAt(index < 0 ? -1 : index - 1);
  else if (e.key === 'Home') focusAt(0);
  else if (e.key === 'End') focusAt(-1);
  else if (e.key === forward) {
    if (current?.item.submenu) showMenu(current.item.submenu(), current.button, level + 1, { focus: 'first' });
    else if (menubarContext) menubarContext.open(menubarContext.index + 1);
  } else if (e.key === back) {
    if (level > 0) closeMenus(level, true);
    else if (menubarContext) menubarContext.open(menubarContext.index - 1);
  } else if (e.key === 'Escape') closeMenus(level > 0 ? level : 0, true);
  else if (e.key === 'Tab') {
    closeMenus(0, true);
    return;
  } else if (e.key === 'Enter' || e.key === ' ') {
    if (current) activate(current.item, current.button, level);
  } else if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
    const key = e.key.toLocaleLowerCase();
    const start = index + 1;
    const order = [...enabled.slice(start), ...enabled.slice(0, start)];
    order.find((x) => x.item.label.toLocaleLowerCase().startsWith(key))?.button.focus();
  } else return;
  e.preventDefault();
  e.stopPropagation();
}

/** A dropdown opened from `anchor` (language menu, ⋮ menus, zoom): toggles, opens with the first item focused. */
function dropdown(anchor: HTMLElement, items: () => MenuItem[], options: { alignEnd?: boolean; label?: string } = {}) {
  anchor.setAttribute('aria-haspopup', 'menu');
  anchor.setAttribute('aria-expanded', 'false');
  anchor.addEventListener('click', () => {
    if (openMenus[0]?.anchor === anchor) closeMenus(0);
    else showMenu(items(), anchor, 0, { focus: 'first', ...options });
  });
}

/** The menu bar of an editor (role menubar, roving tab stop, hover switches between open menus). */
function menubar(specs: MenuSpec[]): HTMLElement {
  const bar = el('nav', { className: 'menubar', role: 'menubar', aria: { label: t('editor.menu') } });
  const buttons = specs.map((spec, i) => el('button', { type: 'button', className: 'menubar-item', textContent: spec.label,
    role: 'menuitem', tabIndex: i === 0 ? 0 : -1, aria: { haspopup: 'menu', expanded: 'false' } }));
  const open = (i: number, focus: 'first' | 'last' | false = 'first') => {
    const index = (i + specs.length) % specs.length;
    buttons.forEach((b, n) => (b.tabIndex = n === index ? 0 : -1));
    showMenu(specs[index].items(), buttons[index], 0, { focus, label: specs[index].label });
    menubarContext = { open: (n) => open(n), index, count: specs.length };
  };
  buttons.forEach((b, i) => {
    b.addEventListener('click', () => (openMenus[0]?.anchor === b ? closeMenus(0) : open(i)));
    b.addEventListener('pointerenter', () => {
      if (menubarContext && openMenus.length && openMenus[0].anchor !== b && buttons.includes(openMenus[0].anchor as HTMLButtonElement)) open(i);
    });
    b.addEventListener('keydown', (e) => {
      const forward = rtl() ? 'ArrowLeft' : 'ArrowRight';
      const back = rtl() ? 'ArrowRight' : 'ArrowLeft';
      let next = -1;
      if (e.key === forward) next = (i + 1) % buttons.length;
      else if (e.key === back) next = (i - 1 + buttons.length) % buttons.length;
      else if (e.key === 'ArrowDown' || e.key === 'Enter' || e.key === ' ') open(i, 'first');
      else if (e.key === 'ArrowUp') open(i, 'last');
      else return;
      e.preventDefault();
      if (next >= 0) {
        buttons.forEach((x, n) => (x.tabIndex = n === next ? 0 : -1));
        buttons[next].focus();
      }
    });
  });
  bar.append(...buttons);
  return bar;
}

document.addEventListener('pointerdown', (e) => {
  if (!openMenus.length) return;
  const target = e.target as Node;
  if (openMenus.some(({ menu, anchor }) => menu.contains(target) || anchor.contains(target))) return;
  closeMenus(0);
}, true);
window.addEventListener('resize', () => closeMenus(0));

// MARK: - Apps, drawer and routing

type AppId = 'documents' | 'spreadsheets' | 'presentations' | 'pdf' | 'labels' | 'signatures';
const APPS: { id: AppId; logo: string; name: () => string }[] = [
  { id: 'documents', logo: 'doc', name: () => t('nav.documents') },
  { id: 'spreadsheets', logo: 'xls', name: () => t('nav.spreadsheets') },
  { id: 'presentations', logo: 'ppt', name: () => t('nav.presentations') },
  { id: 'pdf', logo: 'pdf', name: () => 'PDF' },
  { id: 'labels', logo: 'labels', name: () => t('tab.labels') },
  { id: 'signatures', logo: 'signatures', name: () => t('tab.marks') },
];
const appInfo = (id: AppId) => APPS.find((a) => a.id === id)!;
const toolSetup: Partial<Record<AppId, () => void>> = { labels: setupLabels, signatures: setupSignatures };
const initialised = new Set<string>();
let current: AppId = 'documents';

function route() {
  const requested = location.hash.slice(1) as AppId;
  const id = APPS.some((a) => a.id === requested) ? requested : 'documents';
  current = id;
  closeMenus(0);
  document.querySelectorAll<HTMLElement>('#app > .screen').forEach((s) => (s.hidden = s.id !== id));
  document.querySelectorAll<HTMLElement>('.drawer-app').forEach((a) => {
    if (a.dataset.app === id) a.setAttribute('aria-current', 'page');
    else a.removeAttribute('aria-current');
  });
  document.title = `${appInfo(id).name()} – OFF42`;
  if (!initialised.has(id) && toolSetup[id]) {
    initialised.add(id);
    toolSetup[id]!();
  }
}

function setupDrawer() {
  const drawer = $<HTMLDialogElement>('drawer');
  $('drawer-apps').replaceChildren(...APPS.map((app) => {
    const link = el('a', { className: 'drawer-app', href: `#${app.id}`, dataset: { app: app.id } }, appLogo(app.logo, 22),
      el('span', { textContent: app.name() }));
    link.addEventListener('click', () => drawer.close());
    return link;
  }));
  const site = `../${language}/`;
  $<HTMLAnchorElement>('drawer-download').href = `${site}download/`;
  $<HTMLAnchorElement>('drawer-about').href = `${site}about/`;
  $<HTMLAnchorElement>('drawer-privacy').href = `${site}privacy/`;
  $<HTMLAnchorElement>('about-site').href = site;
  $<HTMLAnchorElement>('about-privacy-link').href = `${site}privacy/`;
  $<HTMLAnchorElement>('about-download').href = `${site}download/`;
  setupDialog(drawer);
  // Keep Tab inside the drawer while it is open.
  drawer.addEventListener('keydown', (e) => {
    if (e.key !== 'Tab') return;
    const focusable = [...drawer.querySelectorAll<HTMLElement>('a[href], button')].filter((n) => n.offsetParent);
    const first = focusable[0];
    const last = focusable.at(-1)!;
    if (e.shiftKey && document.activeElement === first) {
      last.focus();
      e.preventDefault();
    } else if (!e.shiftKey && document.activeElement === last) {
      first.focus();
      e.preventDefault();
    }
  });
}

function openDrawer() {
  closeMenus(0);
  const drawer = $<HTMLDialogElement>('drawer');
  drawer.showModal();
  (drawer.querySelector<HTMLElement>('.drawer-app[aria-current]') ?? drawer.querySelector<HTMLElement>('.drawer-app'))?.focus();
}

function languageItems(): MenuItem[] {
  return LANGUAGES.map(([code, name]) => ({ label: name, lang: code, checked: code === language, radio: true,
    run: () => switchLanguage(code) }));
}

/** The top bar of a home or tool screen: ☰, app logo and name, optional search, language, help and the wordmark. */
function topBar(id: AppId, search?: HTMLElement): HTMLElement {
  const app = appInfo(id);
  const menuButton = el('button', { type: 'button', className: 'icon-button menu-button', title: t('bar.menu'),
    aria: { label: t('bar.menu') } }, icon('menu'));
  menuButton.addEventListener('click', openDrawer);
  const languageButton = el('button', { type: 'button', className: 'icon-button language-button', title: t('app.language'),
    aria: { label: t('app.language') } }, icon('globe'));
  dropdown(languageButton, languageItems, { alignEnd: true, label: t('app.language') });
  const helpButton = el('button', { type: 'button', className: 'icon-button help-button', title: t('bar.help'),
    aria: { label: t('bar.help') } }, icon('help'));
  helpButton.addEventListener('click', showAbout);
  const wordmark = el('a', { className: 'wordmark small', href: `../${language}/`, dir: 'ltr', aria: { label: 'OFF42' } });
  wordmark.innerHTML = 'off<span class="wm-n">42<svg class="wm-star" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 2v20M2 12h20M4.9 4.9l14.2 14.2M19.1 4.9 4.9 19.1"/></svg></span>';
  return el('header', { className: 'topbar' },
    menuButton,
    el('div', { className: 'app-title' }, appLogo(app.logo, 32), el('h1', { textContent: app.name() })),
    el('div', { className: 'topbar-center' }, search ?? ''),
    el('div', { className: 'topbar-end' }, languageButton, helpButton, wordmark));
}

// MARK: - Recent files
//
// Only names, sizes and times are stored (localStorage, this browser only). The bytes of files opened in this tab
// stay in memory, so those reopen at once; older entries ask for the file again.

type Recent = { app: AppId; name: string; size: number; at: number };
const RECENT_KEY = 'off42.recent';
const sessionFiles = new Map<string, Uint8Array>();
const recentKey = (r: { app: string; name: string; size: number }) => `${r.app}\u0000${r.name}\u0000${r.size}`;

function loadRecent(): Recent[] {
  try {
    const list = JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]');
    return Array.isArray(list) ? list.filter((r) => r && typeof r.name === 'string' && typeof r.app === 'string'
      && Number.isFinite(r.size) && Number.isFinite(r.at)) : [];
  } catch {
    return [];
  }
}

function saveRecent(list: Recent[]) {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(list.slice(0, 40)));
  } catch {
    // Storage may be unavailable (private mode, blocked site data): the list then lasts for this tab only.
  }
}

let memoryRecent: Recent[] | null = null;
const recentList = () => memoryRecent ?? loadRecent();

function remember(app: AppId, file: OpenFile) {
  const entry: Recent = { app, name: file.name, size: file.bytes.length, at: Date.now() };
  const list = [entry, ...recentList().filter((r) => recentKey(r) !== recentKey(entry))];
  saveRecent(list);
  if (loadRecent().length === 0) memoryRecent = list;
  sessionFiles.delete(recentKey(entry));
  sessionFiles.set(recentKey(entry), file.bytes);
  while (sessionFiles.size > 12) sessionFiles.delete(sessionFiles.keys().next().value!);
}

function forget(entry: Recent) {
  const list = recentList().filter((r) => recentKey(r) !== recentKey(entry));
  saveRecent(list);
  if (memoryRecent) memoryRecent = list;
  sessionFiles.delete(recentKey(entry));
}

function whenText(at: number): string {
  const date = new Date(at);
  const now = new Date();
  const midnight = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((midnight(now) - midnight(date)) / 86_400_000);
  if (days === 0 || days === 1) {
    const day = new Intl.RelativeTimeFormat(language, { numeric: 'auto' }).format(-days, 'day');
    const time = new Intl.DateTimeFormat(language, { hour: 'numeric', minute: '2-digit' }).format(date);
    return t('home.when', { day: day.charAt(0).toLocaleUpperCase(language) + day.slice(1), time });
  }
  return new Intl.DateTimeFormat(language, { day: 'numeric', month: 'short',
    year: date.getFullYear() === now.getFullYear() ? undefined : 'numeric' }).format(date);
}

// MARK: - File apps: home screen and editor

type OpenFile = { name: string; bytes: Uint8Array };
type Context = {
  file: OpenFile; kind: string;
  /** The file name without extension as currently shown in the title row (Rename changes it). */
  stem: () => string;
  save: (data: Uint8Array | string, ext: string) => void;
  print: () => void;
};
type Editor = {
  body: HTMLElement;
  toolbar: (HTMLElement | 'sep')[];
  /** Download as ▸ entries. */
  exports: { label: string; ext: string; run: () => Promise<void> }[];
  extraExports?: { label: string; run: () => Promise<void> }[];
  fileItems?: MenuItem[];
  menus?: MenuSpec[];
  actions?: HTMLElement[];
  badge?: { text: string; title: string };
  printable?: () => Node;
  keydown?: (e: KeyboardEvent) => void;
  dispose?: () => void;
};
type FileApp = {
  id: AppId; logo: string; extensions: string[]; formats: string;
  view: (ctx: Context) => Promise<Editor>;
  section: HTMLElement; home: HTMLElement; editorHost: HTMLElement; input: HTMLInputElement; search: HTMLInputElement;
  renderRecent: () => void;
  editor?: { editor: Editor | null; ctx: Context; download: () => void };
};

const fileApps: FileApp[] = [];
const fileAppFor = (name: string) => fileApps.find((a) => a.extensions.includes(extension(name)));

async function openFile(file: OpenFile, app = fileAppFor(file.name)) {
  if (!app) {
    notify(t('files.unsupported', { extension: extension(file.name) }), 'warn');
    return;
  }
  if (location.hash !== `#${app.id}`) location.hash = app.id;
  route();
  remember(app.id, file);
  app.renderRecent();
  await openEditor(app, file);
}

async function openPicked(input: HTMLInputElement, app: FileApp) {
  const picked = input.files?.[0];
  if (!picked) return;
  const file = { name: picked.name, bytes: new Uint8Array(await picked.arrayBuffer()) };
  input.value = '';
  await openFile(file, fileAppFor(file.name) ?? app);
}

function buildFileApp(app: FileApp) {
  const info = appInfo(app.id);
  app.input = el('input', { type: 'file', id: `file-input-${app.id}`, className: 'visually-hidden', tabIndex: -1,
    accept: app.extensions.map((e) => `.${e}`).join(',') });
  app.input.addEventListener('change', () => openPicked(app.input, app));
  const pick = () => app.input.click();

  app.search = el('input', { type: 'search', placeholder: t('home.search'), aria: { label: t('home.search') } });
  const search = el('label', { className: 'search-field large' }, icon('search'), app.search);
  const openTile = el('button', { type: 'button', className: 'tile', aria: { label: t('home.openFromDevice') } },
    icon('open', 'icon tile-icon'), el('span', { className: 'tile-hint', textContent: t('home.orDrag') }));
  openTile.addEventListener('click', pick);
  const list = el('ul', { className: 'recent-list' });
  const empty = el('p', { className: 'recent-empty' });
  const head = el('div', { className: 'recent-head', aria: { hidden: 'true' } },
    el('span', { className: 'recent-title', textContent: t('home.recent') }),
    el('span', { className: 'col-when', textContent: t('home.opened') }),
    el('span', { className: 'col-size', textContent: t('home.size') }), el('span'));

  app.renderRecent = () => {
    const query = app.search.value.trim().toLocaleLowerCase();
    const mine = recentList().filter((r) => r.app === app.id);
    const shown = query ? mine.filter((r) => r.name.toLocaleLowerCase().includes(query)) : mine;
    list.replaceChildren(...shown.map((entry) => recentRow(app, entry)));
    empty.textContent = mine.length === 0 ? t('home.noRecent') : shown.length === 0 ? t('home.noMatch', { query: app.search.value.trim() }) : '';
    empty.hidden = shown.length > 0;
  };
  app.search.addEventListener('input', app.renderRecent);

  app.home = el('div', { className: 'home' },
    topBar(app.id, search),
    el('div', { className: 'home-scroll' },
      el('section', { className: 'open-strip' }, el('div', { className: 'wrap' },
        el('div', { className: 'strip-head' }, el('h2', { textContent: t(`${app.id}.empty`) }),
          el('span', { className: 'strip-formats', textContent: app.formats, dir: 'ltr' })),
        el('div', { className: 'tiles' }, el('div', { className: 'tile-box' }, openTile,
          el('span', { className: 'tile-label', textContent: t('home.openFromDevice') }))))),
      el('section', { className: 'recent wrap', aria: { label: t('home.recent') } }, head, list, empty,
        el('p', { className: 'privacy-line' }, icon('lock'), el('span', { textContent: t('home.privacy') })))));
  app.editorHost = el('div', { className: 'editor', hidden: true });
  app.section = el('section', { id: app.id, className: `screen file-screen app-${info.logo}`, hidden: true },
    app.home, app.editorHost, app.input);
  $('app').append(app.section);
  app.renderRecent();
}

function recentRow(app: FileApp, entry: Recent): HTMLElement {
  const inMemory = sessionFiles.has(recentKey(entry));
  const open = el('button', { type: 'button', className: 'recent-open', title: inMemory ? entry.name : t('home.reopen', { name: entry.name }) },
    el('span', { className: 'recent-icon' }, appLogo(appInfo(app.id).logo, 22)),
    el('span', { className: 'recent-name' }, el('span', { className: 'name', textContent: entry.name, dir: 'auto' }),
      el('span', { className: 'recent-meta', textContent: `${whenText(entry.at)} · ${bytesText(entry.size)}` })),
    el('span', { className: 'col-when', textContent: whenText(entry.at) }),
    el('span', { className: 'col-size', textContent: bytesText(entry.size) }));
  const reopen = () => {
    const bytes = sessionFiles.get(recentKey(entry));
    if (bytes) return openFile({ name: entry.name, bytes }, app);
    // The browser gives a page no access to a file it has not been handed again.
    notify(t('home.reopen', { name: entry.name }));
    app.input.click();
  };
  open.addEventListener('click', () => attempt(reopen));
  const more = el('button', { type: 'button', className: 'icon-button recent-more', title: t('recent.actions', { name: entry.name }),
    aria: { label: t('recent.actions', { name: entry.name }) } }, icon('more'));
  dropdown(more, () => [
    { label: t('menu.open'), icon: 'folder', run: reopen },
    { label: t('recent.remove'), icon: 'close', run: () => { forget(entry); app.renderRecent(); } },
  ], { alignEnd: true });
  return el('li', { className: 'recent-row' }, open, more);
}

function closeEditor(app: FileApp) {
  app.editor?.editor?.dispose?.();
  app.editor = undefined;
  closeMenus(0);
  app.editorHost.replaceChildren();
  app.editorHost.hidden = true;
  app.home.hidden = false;
  app.renderRecent();
}

async function openEditor(app: FileApp, file: OpenFile) {
  app.editor?.editor?.dispose?.();
  const info = appInfo(app.id);
  const kind = extension(file.name);
  const nameInput = el('input', { className: 'file-name', value: stem(file.name), spellcheck: false, aria: { label: t('editor.fileName') } });
  nameInput.size = Math.max(4, Math.min(40, nameInput.value.length + 1));
  nameInput.addEventListener('input', () => (nameInput.size = Math.max(4, Math.min(40, nameInput.value.length + 1))));
  nameInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' || e.key === 'Escape') {
      if (e.key === 'Escape' || !nameInput.value.trim()) nameInput.value = state.ctx.stem() || stem(file.name);
      nameInput.blur();
    }
  });
  const ctx: Context = {
    file, kind,
    stem: () => nameInput.value.trim() || stem(file.name),
    save: (data, ext) => download(data, `${ctx.stem()}.${ext}`, MIME[ext] ?? 'application/octet-stream'),
    print: () => printEditor(app),
  };
  const downloadOriginal = () => ctx.save(file.bytes, kind);
  const state: NonNullable<FileApp['editor']> = { editor: null, ctx, download: downloadOriginal };
  app.editor = state;
  const pick = () => app.input.click();
  const close = () => closeEditor(app);

  const home = el('button', { type: 'button', className: 'logo-button', title: t('bar.home', { app: info.name() }),
    aria: { label: t('bar.home', { app: info.name() }) } }, appLogo(info.logo, 30));
  home.addEventListener('click', close);
  const back = el('button', { type: 'button', className: 'icon-button back-button', title: t('bar.back', { app: info.name() }),
    aria: { label: t('bar.back', { app: info.name() }) } }, icon(rtl() ? 'next' : 'back'));
  back.addEventListener('click', close);
  const downloadPill = el('button', { type: 'button', className: 'download-pill', title: t('menu.download', { format: `.${kind}` }) },
    icon('download'), el('span', { textContent: t('editor.download') }));
  downloadPill.addEventListener('click', () => busy(downloadPill, downloadOriginal));
  const more = el('button', { type: 'button', className: 'icon-button overflow-button', title: t('bar.more'), aria: { label: t('bar.more') } }, icon('more'));
  const titleRow = el('div', { className: 'title-row' }, nameInput,
    el('span', { className: 'format-badge', textContent: `.${kind}`, dir: 'ltr' }));
  const privacy = el('span', { className: 'privacy-state' }, icon('lock'), el('span', { textContent: t('editor.onDevice') }));
  const actions = el('div', { className: 'editor-actions' });
  const bar = el('div', { className: 'menubar-slot' });
  const toolbar = el('div', { className: 'toolbar', role: 'toolbar', aria: { label: t('editor.toolbar') } });
  const body = el('div', { className: 'editor-body' }, el('div', { className: 'loading' }, el('span', { className: 'spinner' }),
    el('span', { textContent: t('files.opening', { name: file.name }) })));

  const fileMenu = (): MenuItem[] => {
    const editor = state.editor;
    const exports: MenuItem[] = (editor?.exports ?? []).map((x) => ({ label: x.label, hint: `.${x.ext}`, run: x.run }));
    const extra: MenuItem[] = (editor?.extraExports ?? []).map((x) => ({ label: x.label, run: x.run }));
    const downloadAs = [...exports, ...(exports.length && extra.length ? [SEP] : []), ...extra];
    return [
      { label: t('menu.open'), icon: 'folder', shortcut: shortcut('O'), run: pick },
      SEP,
      { label: t('menu.download', { format: `.${kind}` }), icon: 'download', shortcut: shortcut('S'), run: downloadOriginal, disabled: !editor },
      { label: t('menu.downloadAs'), submenu: () => downloadAs, disabled: !downloadAs.length },
      { label: t('menu.rename'), icon: 'rename', run: () => { nameInput.focus(); nameInput.select(); }, disabled: !editor },
      ...(editor?.fileItems?.length || editor?.printable ? [SEP] : []),
      ...(editor?.fileItems ?? []),
      ...(editor?.printable ? [{ label: t('menu.print'), icon: 'print', shortcut: shortcut('P'), run: ctx.print }] : []),
      SEP,
      { label: t('files.close'), icon: 'close', run: close },
    ];
  };
  const helpMenu = (): MenuItem[] => [
    { label: t('app.site'), icon: 'info', run: showAbout },
    { label: t('menu.website'), icon: 'external', run: () => window.open(`../${language}/`, '_blank', 'noopener') },
  ];
  const specs = (): MenuSpec[] => [
    { label: t('menu.file'), items: fileMenu },
    ...(state.editor?.menus ?? []),
    { label: t('menu.help'), items: helpMenu },
  ];
  // Phone: the same commands in one overflow menu, grouped under the menu names.
  dropdown(more, () => specs().flatMap((spec, i) => [...(i ? [SEP] : []), { heading: spec.label }, ...spec.items()]),
    { alignEnd: true, label: t('bar.more') });

  const render = () => {
    const editor = state.editor;
    bar.replaceChildren(menubar(specs()));
    actions.replaceChildren(...(editor?.actions ?? []), downloadPill, more);
    downloadPill.disabled = !editor;
    titleRow.querySelector('.read-only')?.remove();
    if (editor?.badge) titleRow.append(el('span', { className: 'read-only', textContent: editor.badge.text, title: editor.badge.title }));
    titleRow.append(privacy);
    toolbar.replaceChildren(...(editor?.toolbar ?? []).map((x) => (x === 'sep' ? separator() : x)));
    toolbar.hidden = !editor || editor.toolbar.length === 0;
  };

  app.editorHost.replaceChildren(
    el('header', { className: 'editor-bar' }, back, home,
      el('div', { className: 'editor-titles' }, titleRow, bar), actions),
    toolbar, body);
  app.home.hidden = true;
  app.editorHost.hidden = false;
  render();

  try {
    const editor = await app.view(ctx);
    if (app.editor !== state) return editor.dispose?.();
    state.editor = editor;
    body.replaceChildren(editor.body);
    render();
  } catch (error) {
    if (app.editor !== state) return;
    body.replaceChildren(el('div', { className: 'canvas failed' }, el('div', { className: 'failed-box' },
      icon('warn', 'icon big'), el('h2', { textContent: t('files.failed') }),
      el('p', { className: 'error-text', textContent: message(error) }),
      button(t('files.openFile'), pick, { icon: 'folder', className: 'pill-button' }))));
  }
}

// MARK: - Printing: the editor puts printable content in #print-root, print CSS hides everything else.

function printEditor(app: FileApp) {
  const content = app.editor?.editor?.printable?.();
  if (!content) return;
  $('print-root').className = `print-root app-${appInfo(app.id).logo}`;
  $('print-root').replaceChildren(content);
  document.documentElement.classList.add('printing');
  window.print();
}

window.addEventListener('afterprint', () => {
  document.documentElement.classList.remove('printing');
  $('print-root').replaceChildren();
});

// MARK: - Shared editor pieces

/** A zoom menu button for the toolbar plus View menu commands. `0` means "fit" where `fit` is offered. */
function zoomControl(levels: number[], initial: number, apply: (zoom: number) => void, fit = false) {
  let value = initial;
  const label = el('span', { className: 'zoom-label' });
  const button = el('button', { type: 'button', className: 'tool with-text zoom-button', title: t('menu.zoom'), aria: { label: t('menu.zoom') } },
    label, icon('down', 'icon small'));
  const set = (zoom: number) => {
    value = zoom;
    label.textContent = zoom === 0 ? t('menu.fit') : percent(zoom);
    apply(zoom);
  };
  const step = (direction: number) => {
    const now = value === 0 ? 100 : value;
    const next = direction > 0 ? levels.find((z) => z > now) : [...levels].reverse().find((z) => z < now);
    if (next) set(next);
  };
  dropdown(button, () => [
    ...(fit ? [{ label: t('menu.fit'), checked: value === 0, radio: true, run: () => set(0) }] : []),
    ...levels.map((z) => ({ label: percent(z), checked: value === z, radio: true, run: () => set(z) })),
  ], { label: t('menu.zoom') });
  set(initial);
  return {
    button,
    items: (): MenuItem[] => [
      { label: t('menu.zoomIn'), icon: 'zoomIn', run: () => step(1), disabled: value !== 0 && value >= levels.at(-1)! },
      { label: t('menu.zoomOut'), icon: 'zoomOut', run: () => step(-1), disabled: value !== 0 && value <= levels[0] },
      fit ? { label: t('menu.fit'), checked: value === 0, radio: true, run: () => set(0) }
        : { label: t('menu.actualSize'), checked: value === 100, radio: true, run: () => set(100) },
    ],
  };
}

function fullScreenItem(): MenuItem[] {
  if (!document.fullscreenEnabled) return [];
  return [{ label: t('menu.fullScreen'), icon: 'fullscreen', checked: !!document.fullscreenElement,
    run: () => (document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen()) }];
}

// MARK: - Documents

function documentKind(kind: string): string {
  return kind === 'markdown' ? 'md' : kind === 'htm' ? 'html' : kind;
}

// The page around the document's own HTML: A4 paper with margins. Links cannot navigate (the frame is sandboxed
// without popups or top navigation) and the frame runs no scripts.
const PAPER_STYLE = `<base target="_blank"><style>
html{background:#fff;color:#1f1f1f;color-scheme:light}
body{margin:0;padding:84px 96px 96px;font:16px/1.55 "Helvetica Neue",Arial,"Noto Sans",sans-serif;overflow:hidden;word-wrap:break-word}
img{max-width:100%;height:auto}table{border-collapse:collapse;max-width:100%}td,th{vertical-align:top}
a{color:#2457c5;pointer-events:none}
@media (max-width:600px){body{padding:28px 22px 40px}}
</style>`;

const DOC_FORMATS: [string, () => string][] = [
  ['docx', () => 'Microsoft Word'], ['odt', () => t('format.odt')], ['rtf', () => t('format.rtf')],
  ['md', () => 'Markdown'], ['html', () => t('format.html')], ['txt', () => t('format.txt')],
];

/** The document as printable content: its HTML in a shadow root (scripts and handlers removed). */
function printableDocument(html: string): Node {
  const parsed = new DOMParser().parseFromString(html, 'text/html');
  parsed.querySelectorAll('script, iframe, object, embed, base, link, meta').forEach((n) => n.remove());
  parsed.querySelectorAll('*').forEach((n) => [...n.attributes].forEach((a) => /^on/i.test(a.name) && n.removeAttribute(a.name)));
  const host = el('div', { className: 'print-document' });
  const shadow = host.attachShadow({ mode: 'open' });
  const styles = [...parsed.querySelectorAll('style')].map((s) => s.outerHTML.replace(/\b(html|body)\b/g, '.doc')).join('');
  shadow.innerHTML = `<style>.doc{font:12pt/1.5 "Helvetica Neue",Arial,"Noto Sans",sans-serif;color:#000}img{max-width:100%;height:auto}table{border-collapse:collapse}a{color:inherit}</style>${styles}<div class="doc">${parsed.body.innerHTML}</div>`;
  return host;
}

async function viewDocument(ctx: Context): Promise<Editor> {
  const kind = documentKind(ctx.kind);
  const { file } = ctx;
  const [html, info] = await Promise.all([
    call<string>('documentHtml', file.bytes, kind),
    json<{ words: number; characters: number; paragraphs: number }>('documentInfo', file.bytes, kind),
  ]);
  const frame = el('iframe', { className: 'paper', title: t('doc.preview') });
  // Same origin (so the page can size the frame to its content) but no scripts, forms, popups or navigation.
  frame.setAttribute('sandbox', 'allow-same-origin');
  frame.srcdoc = /<head[^>]*>/i.test(html) ? html.replace(/<head[^>]*>/i, (m) => m + PAPER_STYLE) : PAPER_STYLE + html;
  const observer = new ResizeObserver(() => fit());
  const fit = () => {
    const doc = frame.contentDocument;
    if (doc?.documentElement) frame.style.height = `${Math.max(doc.documentElement.scrollHeight, 1123)}px`;
  };
  frame.addEventListener('load', () => {
    fit();
    observer.observe(frame);
  });
  const page = el('div', { className: 'page-wrap' }, frame);
  const canvas = el('div', { className: 'canvas doc-canvas' }, page);
  const zoom = zoomControl([50, 75, 90, 100, 125, 150, 200], 100, (z) => (page.style.zoom = String(z / 100)));
  const convert = async (to: string) => {
    let output: Uint8Array;
    try {
      output = await call<Uint8Array>('documentConvert', file.bytes, kind, to, false);
    } catch (error) {
      const text = message(error);
      if (!/package parts/.test(text)) throw error;
      if (!(await ask(t('doc.convertAnyway', { error: text, format: to.toUpperCase() }), t('common.convertAnyway')))) return;
      output = await call<Uint8Array>('documentConvert', file.bytes, kind, to, true);
    }
    ctx.save(output, to);
  };
  const stats = () => showStats([
    [t('stats.words'), number(info.words)], [t('stats.characters'), number(info.characters)],
    [t('stats.paragraphs'), number(info.paragraphs)], [t('stats.format'), kind.toUpperCase()],
    [t('pdf.fileSize'), bytesText(file.bytes.length)],
  ]);
  return {
    body: canvas,
    toolbar: [tool('folder', t('menu.open'), () => $<HTMLInputElement>('file-input-documents').click()),
      tool('print', t('menu.print'), ctx.print), 'sep', zoom.button, 'sep',
      tool('stats', t('stats.title'), stats, t('doc.words', { words: number(info.words) }))],
    exports: DOC_FORMATS.map(([ext, label]) => ({ label: label(), ext, run: () => convert(ext) })),
    fileItems: [{ label: t('stats.title'), icon: 'stats', run: stats }],
    menus: [{ label: t('menu.view'), items: () => [...zoom.items(), SEP, ...fullScreenItem()] }],
    printable: () => printableDocument(html),
    dispose: () => observer.disconnect(),
  };
}

// MARK: - Spreadsheets

type Sheet = { name: string; rows: number; cols: number; truncated: boolean; cells: [number, number, string][] };
const MAX_ROWS = 1000;
const MAX_COLS = 100;
const NUMERIC = /^[-−+(]?[$€£¥₽₹]?\s?\d[\d\s.,'’ ]*(%|[$€£¥₽₹]|\s?[A-Z]{3})?\)?$/;
const PLAIN_NUMBER = /^[-−]?(\d{1,3}([,   ]\d{3})+|\d+)(\.\d+)?$/;

function columnName(index: number): string {
  let name = '';
  for (let n = index + 1; n > 0; n = Math.floor((n - 1) / 26)) name = String.fromCharCode(65 + ((n - 1) % 26)) + name;
  return name;
}

async function viewSpreadsheet(ctx: Context): Promise<Editor> {
  const { file, kind } = ctx;
  const book = await json<{ readOnly: boolean; sheets: Sheet[] }>('spreadsheetJson', file.bytes, kind, MAX_ROWS, MAX_COLS);
  let sheetIndex = 0;
  let table: HTMLTableElement;
  let size = { rows: 0, cols: 0 };
  let anchor = { r: 0, c: 0 };
  let focus = { r: 0, c: 0 };
  let marked: HTMLElement[] = [];
  const nameBox = el('span', { className: 'namebox', dir: 'ltr' });
  const valueBox = el('span', { className: 'valuebox', dir: 'auto' });
  const scroll = el('div', { className: 'grid-scroll', dir: 'ltr', tabIndex: 0, aria: { label: t('nav.spreadsheets') } });
  const tabs = el('div', { className: 'sheet-tabs', role: 'tablist' });
  const note = el('span', { className: 'sheet-note' });
  const chip = el('span', { className: 'status-chip', hidden: true });
  const zoomTarget = el('div', { className: 'grid-zoom' });
  scroll.append(zoomTarget);

  const cell = (r: number, c: number) => table.tBodies[0].rows[r]?.cells[c + 1];
  const select = (to: { r: number; c: number }, extend = false) => {
    focus = { r: Math.max(0, Math.min(size.rows - 1, to.r)), c: Math.max(0, Math.min(size.cols - 1, to.c)) };
    if (!extend) anchor = { ...focus };
    marked.forEach((n) => n.classList.remove('in-range', 'selected', 'hl'));
    marked = [];
    const [r0, r1] = [Math.min(anchor.r, focus.r), Math.max(anchor.r, focus.r)];
    const [c0, c1] = [Math.min(anchor.c, focus.c), Math.max(anchor.c, focus.c)];
    let sum = 0;
    let numbers = 0;
    let filled = 0;
    for (let r = r0; r <= r1; r++) {
      for (let c = c0; c <= c1; c++) {
        const td = cell(r, c);
        if (!td) continue;
        if (r1 > r0 || c1 > c0) {
          td.classList.add('in-range');
          marked.push(td);
        }
        const text = td.textContent!.trim();
        if (text) filled++;
        if (PLAIN_NUMBER.test(text)) {
          sum += Number(text.replace(/[,   ]/g, '').replace('−', '-'));
          numbers++;
        }
      }
    }
    const active = cell(focus.r, focus.c)!;
    active.classList.add('selected');
    marked.push(active);
    for (let c = c0; c <= c1; c++) {
      const th = table.tHead!.rows[0].cells[c + 1];
      th.classList.add('hl');
      marked.push(th);
    }
    for (let r = r0; r <= r1; r++) {
      const th = table.tBodies[0].rows[r].cells[0];
      th.classList.add('hl');
      marked.push(th);
    }
    const ref = `${columnName(focus.c)}${focus.r + 1}`;
    nameBox.textContent = r1 > r0 || c1 > c0 ? `${columnName(c0)}${r0 + 1}:${columnName(c1)}${r1 + 1}` : ref;
    valueBox.textContent = active.textContent;
    const parts = [];
    if (numbers) parts.push(t('sheet.sum', { sum: number(Math.round(sum * 1e10) / 1e10) }));
    if (filled > 1) parts.push(t('sheet.count', { count: number(filled) }));
    chip.textContent = parts.join(' · ');
    chip.hidden = parts.length === 0;
  };
  const reveal = () => cell(focus.r, focus.c)?.scrollIntoView({ block: 'nearest', inline: 'nearest' });

  const render = () => {
    const sheet = book.sheets[sheetIndex];
    const used = sheet.cells.reduce(([r, c], [row, col]) => [Math.max(r, row + 1), Math.max(c, col + 1)], [0, 0]);
    size = { rows: Math.min(Math.max(sheet.rows, used[0] + 20, 40), MAX_ROWS), cols: Math.min(Math.max(sheet.cols, used[1] + 4, 16), MAX_COLS) };
    const grid: string[][] = Array.from({ length: size.rows }, () => Array(size.cols).fill(''));
    for (const [r, c, text] of sheet.cells) if (r < size.rows && c < size.cols) grid[r][c] = text;
    const head = el('tr', {}, el('th', { className: 'corner' }),
      ...Array.from({ length: size.cols }, (_, c) => el('th', { textContent: columnName(c), scope: 'col' })));
    const body = grid.map((row, r) => el('tr', {}, el('th', { textContent: String(r + 1), scope: 'row' }),
      ...row.map((text, c) => el('td', { textContent: text, className: NUMERIC.test(text.trim()) ? 'num' : '',
        dataset: { ref: `${columnName(c)}${r + 1}` } }))));
    table = el('table', { className: 'grid' }, el('thead', {}, head), el('tbody', {}, ...body));
    const at = (target: EventTarget | null) => {
      const td = (target as HTMLElement | null)?.closest?.('td');
      return td ? { r: (td.parentElement as HTMLTableRowElement).sectionRowIndex, c: (td as HTMLTableCellElement).cellIndex - 1 } : null;
    };
    let dragging = false;
    table.addEventListener('pointerdown', (e) => {
      const pos = at(e.target);
      if (!pos || e.button !== 0) return;
      select(pos, e.shiftKey);
      dragging = true;
      scroll.focus({ preventScroll: true });
    });
    table.addEventListener('pointerover', (e) => {
      const pos = dragging && e.buttons === 1 ? at(e.target) : null;
      if (pos) select(pos, true);
    });
    window.addEventListener('pointerup', () => (dragging = false));
    zoomTarget.replaceChildren(table);
    scroll.scrollTo(0, 0);
    select({ r: 0, c: 0 });
    tabs.querySelectorAll('button').forEach((b, i) => {
      b.setAttribute('aria-selected', String(i === sheetIndex));
      b.tabIndex = i === sheetIndex ? 0 : -1;
    });
    note.textContent = sheet.truncated ? t('sheet.truncated', { rows: number(MAX_ROWS), columns: number(MAX_COLS) }) : '';
  };
  const showSheet = (i: number) => {
    sheetIndex = (i + book.sheets.length) % book.sheets.length;
    render();
  };
  book.sheets.forEach((sheet, i) => {
    const b = el('button', { type: 'button', role: 'tab', className: 'sheet-tab', textContent: sheet.name, dir: 'auto' });
    b.addEventListener('click', () => showSheet(i));
    tabs.append(b);
  });
  scroll.addEventListener('keydown', (e) => {
    const moves: Record<string, [number, number]> = { ArrowDown: [1, 0], ArrowUp: [-1, 0], ArrowRight: [0, 1], ArrowLeft: [0, -1], Enter: [1, 0] };
    const move = moves[e.key];
    if (!move || e.ctrlKey || e.metaKey || e.altKey) return;
    select({ r: focus.r + move[0], c: focus.c + move[1] }, e.shiftKey && e.key !== 'Enter');
    reveal();
    e.preventDefault();
  });
  render();

  const sheets = el('button', { type: 'button', className: 'tool', title: t('menu.allSheets'), aria: { label: t('menu.allSheets') } }, icon('list'));
  const sheetItems = (): MenuItem[] => book.sheets.map((s, i) => ({ label: s.name, checked: i === sheetIndex, radio: true, run: () => showSheet(i) }));
  dropdown(sheets, sheetItems, { label: t('menu.allSheets') });
  const zoom = zoomControl([50, 75, 90, 100, 125, 150, 200], 100, (z) => (zoomTarget.style.zoom = String(z / 100)));
  const valuesCsv = async () => download(await call<Uint8Array>('spreadsheetValuesCsv', file.bytes, kind, sheetIndex),
    `${ctx.stem()} - ${book.sheets[sheetIndex].name}.csv`, MIME.csv);
  const convert = async (to: string) => ctx.save(await call<Uint8Array>('spreadsheetConvert', file.bytes, kind, to), to);
  return {
    body: el('div', { className: 'sheet-view' },
      el('div', { className: 'formula-bar' }, nameBox, el('span', { className: 'fx', textContent: 'fx', aria: { hidden: 'true' } }), valueBox),
      scroll,
      el('footer', { className: 'sheet-bar' }, sheets, tabs, note, chip)),
    toolbar: [tool('folder', t('menu.open'), () => $<HTMLInputElement>('file-input-spreadsheets').click()),
      tool('print', t('menu.print'), ctx.print), 'sep', zoom.button],
    exports: book.readOnly ? [] : [['xlsx', 'Microsoft Excel'], ['ods', t('format.ods')], ['csv', 'CSV']]
      .map(([ext, label]) => ({ label, ext, run: () => convert(ext) })),
    extraExports: [{ label: t('sheet.valuesCsv'), run: valuesCsv }],
    badge: book.readOnly ? { text: t('sheet.readOnlyBadge'), title: t('sheet.readOnly') } : undefined,
    menus: [
      { label: t('menu.view'), items: () => [...zoom.items(), SEP, ...fullScreenItem()] },
      { label: t('menu.sheet'), items: () => [...sheetItems(), SEP,
        { label: t('menu.previousSheet'), icon: 'prev', disabled: book.sheets.length < 2, run: () => showSheet(sheetIndex - 1) },
        { label: t('menu.nextSheet'), icon: 'next', disabled: book.sheets.length < 2, run: () => showSheet(sheetIndex + 1) }] },
    ],
    printable: () => el('div', { className: 'print-sheet' }, table.cloneNode(true)),
    keydown: (e) => {
      if (e.key === 'PageDown' && e.altKey) showSheet(sheetIndex + 1);
      else if (e.key === 'PageUp' && e.altKey) showSheet(sheetIndex - 1);
      else return;
      e.preventDefault();
    },
  };
}

// MARK: - Presentations

type SlideObject = { kind: string; role: 'title' | 'body' | 'footer' | null; text: string; cells: string[][] };
type SlideOutline = { title: string; body: string; notes: string; hidden: boolean; objects: SlideObject[] };
const SLIDE_W = 960;
const SLIDE_H = 540;

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

/** A slide drawn at 960 × 540 (scaled by its container): title, accent rule, text, tables, picture placeholders. */
function slideCard(slide: SlideOutline): HTMLElement {
  const { title, rest } = slideParts(slide);
  const card = el('div', { className: 'slide-card' },
    el('h2', { className: title ? '' : 'untitled', textContent: title || t('slides.untitled'), dir: 'auto' }),
    el('span', { className: 'slide-rule' }));
  const body = el('div', { className: 'slide-body' });
  if (slide.body) body.append(bulletList(slide.body));
  for (const object of rest) {
    if (object.cells.length) {
      body.append(el('table', { className: 'slide-table' }, ...object.cells.map((row) =>
        el('tr', {}, ...row.map((text) => el('td', { textContent: text, dir: 'auto' }))))));
    } else if (object.kind === 'Picture') {
      body.append(el('div', { className: 'slide-picture' }, icon('picture'), el('span', { textContent: t('slides.pictureLabel') })));
    } else if (object.text) {
      if (object.role === 'footer') card.append(el('p', { className: 'slide-footer', textContent: object.text, dir: 'auto' }));
      else body.append(object.role === 'body' ? bulletList(object.text) : el('p', { textContent: object.text, dir: 'auto' }));
    }
  }
  card.append(body);
  return card;
}

async function viewPresentation(ctx: Context): Promise<Editor> {
  const { file, kind } = ctx;
  const { slides } = await json<{ slides: SlideOutline[] }>('presentationOutline', file.bytes, kind);
  let index = 0;
  let zoomValue = 0;
  let notesShown = true;
  const list = el('ol', { className: 'thumbs' });
  const scaler = el('div', { className: 'slide-scaler' });
  const stage = el('div', { className: 'slide-stage' }, scaler);
  const notes = el('div', { className: 'slide-notes', aria: { label: t('slides.notesTitle') } });
  const position = el('span', { className: 'slide-position' });
  const thumbs = slides.map((slide, i) => {
    const b = el('button', { type: 'button', className: `thumb${slide.hidden ? ' hidden-slide' : ''}`,
      aria: { label: t('slides.position', { index: i + 1, count: slides.length }) } },
      el('span', { className: 'thumb-number', textContent: String(i + 1) }),
      el('span', { className: 'thumb-frame' }, slideCard(slide)));
    if (slide.hidden) b.title = t('slides.hidden');
    b.addEventListener('click', () => show(i));
    list.append(el('li', {}, b));
    return b;
  });
  const layout = () => {
    const card = scaler.firstElementChild as HTMLElement | null;
    if (!card) return;
    const scale = zoomValue === 0
      ? Math.max(0.1, Math.min((stage.clientWidth - 48) / SLIDE_W, (stage.clientHeight - 32) / SLIDE_H))
      : zoomValue / 100;
    scaler.style.width = `${SLIDE_W * scale}px`;
    scaler.style.height = `${SLIDE_H * scale}px`;
    card.style.transform = `scale(${scale})`;
  };
  const observer = new ResizeObserver(layout);
  observer.observe(stage);
  const show = (i: number) => {
    if (!slides.length) return;
    index = Math.max(0, Math.min(slides.length - 1, i));
    thumbs.forEach((b, n) => b.setAttribute('aria-current', String(n === index)));
    thumbs[index].scrollIntoView({ block: 'nearest' });
    const slide = slides[index];
    scaler.replaceChildren(slideCard(slide));
    layout();
    notes.replaceChildren(el('p', { className: slide.notes ? 'notes-text' : 'notes-text muted', textContent: slide.notes || t('slides.noNotes'), dir: 'auto' }));
    position.textContent = t('slides.position', { index: index + 1, count: slides.length }) + (slide.hidden ? ` · ${t('slides.hidden')}` : '');
  };
  const zoom = zoomControl([50, 75, 100, 150, 200], 0, (z) => {
    zoomValue = z;
    stage.classList.toggle('zoomed', z !== 0);
    layout();
  }, true);
  const toggleNotes = () => {
    notesShown = !notesShown;
    notes.hidden = !notesShown;
    notesButton.setAttribute('aria-pressed', String(notesShown));
  };
  const notesButton = tool('notes', t('slides.notesTitle'), toggleNotes);
  notesButton.setAttribute('aria-pressed', 'true');
  const navigation: MenuItem[] = [
    { label: t('menu.previousSlide'), icon: 'prev', run: () => show(index - 1) },
    { label: t('menu.nextSlide'), icon: 'next', run: () => show(index + 1) },
    SEP,
    { label: t('menu.firstSlide'), icon: 'first', run: () => show(0) },
    { label: t('menu.lastSlide'), icon: 'last', run: () => show(slides.length - 1) },
  ];
  show(0);
  const convert = async (to: string) => ctx.save(await call<Uint8Array>('presentationConvert', file.bytes, kind, to), to);
  return {
    body: slides.length
      ? el('div', { className: 'slides-view' },
        el('aside', { className: 'slide-strip', aria: { label: t('nav.presentations') } }, list,
          el('p', { className: 'strip-note', textContent: t('slides.outlineNote') })),
        el('div', { className: 'slide-main' }, stage, notes))
      : el('div', { className: 'canvas failed' }, el('div', { className: 'failed-box' }, el('h2', { textContent: t('slides.none') }))),
    toolbar: [tool('folder', t('menu.open'), () => $<HTMLInputElement>('file-input-presentations').click()),
      tool('print', t('menu.print'), ctx.print), 'sep',
      tool(rtl() ? 'next' : 'prev', t('menu.previousSlide'), () => show(index - 1)), position,
      tool(rtl() ? 'prev' : 'next', t('menu.nextSlide'), () => show(index + 1)), 'sep', zoom.button, 'sep', notesButton],
    exports: [['pptx', 'Microsoft PowerPoint'], ['odp', t('format.odp')]].map(([ext, label]) => ({ label, ext, run: () => convert(ext) })),
    menus: [
      { label: t('menu.view'), items: () => [...zoom.items(), SEP,
        { label: t('slides.notesTitle'), icon: 'notes', checked: notesShown, run: toggleNotes }, ...fullScreenItem()] },
      { label: t('menu.slide'), items: () => navigation },
    ],
    printable: () => el('div', { className: 'print-slides' }, ...slides.map((s) => el('div', { className: 'print-slide' }, slideCard(s)))),
    keydown: (e) => {
      const forward = rtl() ? 'ArrowLeft' : 'ArrowRight';
      const back = rtl() ? 'ArrowRight' : 'ArrowLeft';
      if (['ArrowDown', forward, 'PageDown'].includes(e.key)) show(index + 1);
      else if (['ArrowUp', back, 'PageUp'].includes(e.key)) show(index - 1);
      else if (e.key === 'Home') show(0);
      else if (e.key === 'End') show(slides.length - 1);
      else return;
      e.preventDefault();
    },
    dispose: () => observer.disconnect(),
  };
}

// MARK: - PDF

type PdfInfo = {
  encrypted: boolean; pages: number; metadata: Record<string, string>; bookmarks: string[]; attachments: string[];
  formFields: { name: string; kind: string; value: string }[];
};

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

async function viewPdf(ctx: Context): Promise<Editor> {
  const { file } = ctx;
  const info = await json<PdfInfo>('pdfInfo', file.bytes);
  const note = el('p', { className: 'canvas-note' }, icon('info'), el('span', { textContent: t('pdf.noRendering') }));
  if (info.encrypted) {
    return {
      body: el('div', { className: 'canvas pdf-canvas' }, note, el('article', { className: 'summary-page' },
        icon('lock', 'icon big'), el('h1', { textContent: t('pdf.encrypted') }), el('p', { className: 'muted', textContent: t('pdf.encryptedText') }))),
      toolbar: [], exports: [],
    };
  }
  const meta = info.metadata;
  const metaKeys = ['Title', 'Author', 'Subject', 'Keywords', 'Creator', 'Producer'].filter((key) => meta[key]);
  const facts = (keys: string[]) => el('dl', { className: 'facts' },
    el('dt', { textContent: t('pdf.pages') }), el('dd', { textContent: number(info.pages) }),
    el('dt', { textContent: t('pdf.fileSize') }), el('dd', { textContent: bytesText(file.bytes.length) }),
    ...keys.flatMap((key) => [el('dt', { textContent: t(`pdf.meta.${key}`) }), el('dd', { textContent: meta[key], dir: 'auto' })]));
  const summary = el('article', { className: 'summary-page' },
    el('h1', { textContent: meta.Title || file.name, dir: 'auto' }),
    meta.Author ? el('p', { className: 'summary-author', textContent: meta.Author, dir: 'auto' }) : '',
    el('div', { className: 'summary-facts' }, facts(['Subject', 'Keywords', 'Creator', 'Producer'].filter((key) => meta[key]))),
    info.bookmarks.length ? el('section', {}, el('h2', { textContent: t('pdf.contents') }), bookmarkTree(info.bookmarks)) : '');

  // Side panel: document properties, bookmarks, attachments, form fields.
  const sections = [
    { id: 'document', label: t('pdf.document'), count: null as number | null, body: () => facts(metaKeys) },
    { id: 'bookmarks', label: t('pdf.bookmarks'), count: info.bookmarks.length,
      body: () => (info.bookmarks.length ? bookmarkTree(info.bookmarks) : el('p', { className: 'muted', textContent: t('pdf.noBookmarks') })) },
    { id: 'attachments', label: t('pdf.attachments'), count: info.attachments.length,
      body: () => (info.attachments.length ? el('ul', { className: 'plain-list' }, ...info.attachments.map((a) => el('li', { textContent: a, dir: 'auto' })))
        : el('p', { className: 'muted', textContent: '—' })) },
    { id: 'fields', label: t('pdf.formFields'), count: info.formFields.length,
      body: () => (info.formFields.length ? el('table', { className: 'fields' },
        el('thead', {}, el('tr', {}, ...['pdf.fieldName', 'pdf.fieldKind', 'pdf.fieldValue'].map((k) => el('th', { textContent: t(k) })))),
        el('tbody', {}, ...info.formFields.map((f) => el('tr', {},
          el('td', { textContent: f.name, dir: 'auto' }), el('td', { textContent: f.kind }), el('td', { textContent: f.value, dir: 'auto' })))))
        : el('p', { className: 'muted', textContent: '—' })) },
  ];
  let active = 'document';
  const tabButtons = sections.map((s) => {
    const b = el('button', { type: 'button', role: 'tab', className: 'panel-tab', dataset: { section: s.id } }, el('span', { textContent: s.label }),
      s.count === null ? '' : el('span', { className: 'count', textContent: number(s.count) }));
    b.addEventListener('click', () => showSection(s.id));
    return b;
  });
  const panelBody = el('div', { className: 'panel-body', role: 'tabpanel' });
  const closePanel = el('button', { type: 'button', className: 'icon-button', title: t('files.close'), aria: { label: t('files.close') } }, icon('close'));
  const panel = el('aside', { className: 'side-panel pdf-panel', aria: { label: t('pdf.panel') } },
    el('div', { className: 'panel-head' }, el('h2', { textContent: t('pdf.panel') }), closePanel),
    el('div', { className: 'panel-tabs', role: 'tablist' }, ...tabButtons), panelBody);
  const toggle = el('button', { type: 'button', className: 'icon-button panel-toggle', title: t('pdf.panel'), aria: { label: t('pdf.panel') } }, icon('panel'));
  const setPanel = (open: boolean) => {
    panel.hidden = !open;
    toggle.setAttribute('aria-pressed', String(open));
  };
  const showSection = (id: string) => {
    active = id;
    tabButtons.forEach((b) => b.setAttribute('aria-selected', String(b.dataset.section === id)));
    panelBody.replaceChildren(sections.find((s) => s.id === id)!.body());
    setPanel(true);
  };
  toggle.addEventListener('click', () => setPanel(panel.hidden));
  closePanel.addEventListener('click', () => setPanel(false));
  showSection('document');
  setPanel(window.innerWidth > 900);
  const sectionButtons = sections.map((s) => {
    const b = el('button', { type: 'button', className: 'tool with-text' }, el('span', { textContent: s.label }),
      s.count === null ? '' : el('span', { className: 'count', textContent: number(s.count) }));
    b.addEventListener('click', () => showSection(s.id));
    return b;
  });
  return {
    body: el('div', { className: 'pdf-view' }, el('div', { className: 'canvas pdf-canvas' }, note, summary), panel),
    toolbar: [tool('folder', t('menu.open'), () => $<HTMLInputElement>('file-input-pdf').click()), 'sep', ...sectionButtons],
    exports: [],
    actions: [toggle],
    fileItems: [{ label: t('pdf.document'), icon: 'info', run: () => showSection('document') }],
    menus: [{ label: t('menu.view'), items: () => [
      { label: t('pdf.panel'), icon: 'panel', checked: !panel.hidden, run: () => setPanel(panel.hidden) }, SEP,
      ...sections.map((s) => ({ label: s.label, checked: !panel.hidden && active === s.id, radio: true, run: () => showSection(s.id) })),
      SEP, ...fullScreenItem()] }],
  };
}

// MARK: - Tool screens

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

function setupTabs(root: HTMLElement) {
  const tabs = [...root.querySelectorAll<HTMLButtonElement>('.tabs [data-pane]')];
  const panes = [...root.querySelectorAll<HTMLElement>('.pane[data-pane]')];
  const show = (name: string) => {
    tabs.forEach((b) => {
      b.setAttribute('aria-selected', String(b.dataset.pane === name));
      b.tabIndex = b.dataset.pane === name ? 0 : -1;
    });
    panes.forEach((p) => (p.hidden = p.dataset.pane !== name));
  };
  tabs.forEach((b, i) => {
    b.addEventListener('click', () => show(b.dataset.pane!));
    b.addEventListener('keydown', (e) => {
      const step = e.key === (rtl() ? 'ArrowLeft' : 'ArrowRight') ? 1 : e.key === (rtl() ? 'ArrowRight' : 'ArrowLeft') ? -1 : 0;
      if (!step) return;
      const next = tabs[(i + step + tabs.length) % tabs.length];
      show(next.dataset.pane!);
      next.focus();
      e.preventDefault();
    });
  });
  show(tabs[0].dataset.pane!);
}

// MARK: - Labels & codes

type Stock = { id: string; name: string; width: number; height: number; columns: number; rows: number;
  marginLeft: number; marginTop: number; gapX: number; gapY: number };
type Mark = { id: string; name: string; category: string; purpose: string; warning?: string; unverified: boolean; needsNumber: boolean };

async function setupLabels() {
  const root = $('labels');
  setupTabs(root);

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
        if (withCode && label.code) slot.append(icon('qr', 'icon slot-code'));
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
  const categoryList = $('marks-category');
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
    tiles.forEach((n) => n.removeAttribute('aria-current'));
    tile.setAttribute('aria-current', 'true');
    detail.hidden = false;
    const widthInput = el('input', { type: 'number', min: '3', max: '250', value: '20' });
    const w = () => Number(widthInput.value) || 20;
    const art = el('div', { className: 'paper-stage mark-preview' });
    call<string>('productMarkSvg', mark.id, 40).then((svg) => art.replaceChildren(imageFrom(svg, MIME.svg, mark.name)), () => {});
    detail.replaceChildren(
      el('div', { className: 'panel-head' },
        el('div', {}, el('p', { className: 'eyebrow', textContent: mark.category }), el('h3', { textContent: mark.name, dir: 'auto' })),
        button('', () => { detail.hidden = true; tile.removeAttribute('aria-current'); }, { icon: 'close', title: t('files.close'), className: 'icon-button' })),
      art,
      el('p', { textContent: mark.purpose, dir: 'auto' }),
      mark.warning ? el('p', { className: 'note warning' }, icon('warn'), el('span', { textContent: mark.warning, dir: 'auto' })) : '',
      mark.unverified ? el('p', { className: 'note warning' }, icon('warn'), el('span', { textContent: t('marks.unverified') })) : '',
      mark.needsNumber ? el('p', { className: 'note', textContent: t('marks.needsNumber') }) : '',
      el('label', { className: 'field' }, el('span', { className: 'field-label', textContent: t('common.widthMm') }), widthInput),
      el('div', { className: 'actions' },
        button(t('common.downloadSvg'), async () => download(await call<string>('productMarkSvg', mark.id, w()), `${mark.id}.svg`, MIME.svg), { icon: 'download' }),
        button(t('common.downloadPng'), async () => download(await call<Uint8Array>('productMarkPng', mark.id, w()), `${mark.id}.png`, MIME.png), { icon: 'download', className: 'pill-button' })));
  };
  const filter = () => {
    const q = search.value.trim().toLowerCase();
    const category = categoryList.dataset.value ?? '';
    const shown = marks.filter((m) => (!category || m.category === category) &&
      // Short queries ("CE") match names and ids only; longer ones also the description.
      (!q || `${m.name} ${m.id} ${m.category}${q.length > 3 ? ` ${m.purpose}` : ''}`.toLowerCase().includes(q)));
    grid.replaceChildren(...shown.map((m) => tiles.get(m.id)!));
    if (!shown.length) grid.append(el('p', { className: 'muted empty-line', textContent: t('marks.noMatch') }));
    $('marks-count').textContent = t('marks.count', { count: number(shown.length) });
  };
  chips(categoryList, [{ value: '', label: t('marks.all') }, ...categories.map((c) => ({ value: c, label: c }))], '', filter);
  search.addEventListener('input', filter);
  filter();
}

// MARK: - Signatures & stamps

type Option = { value: number | string; title: string };
type Field = { key: string; label: string; example: string; multiline: boolean; date: boolean };
type Template = { id: string; title: string; category: string; round: boolean; ink: string; fields: Field[] };

async function setupSignatures() {
  setupTabs($('signatures'));
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
    results.replaceChildren(...variants.map((variant, i) => el('article', { className: 'variant' },
      variant.frame,
      el('div', { className: 'variant-foot' },
        el('span', { className: 'variant-number', textContent: t('sig.variant', { number: i + 1 }) }),
        button('', async () => { variant.impression = seed(); await draw(variant); }, { icon: 'refresh', title: t('common.newImpression'), className: 'icon-button' }),
        button('', () => variant.png && download(variant.png, `signature-${stem(name.value.trim().replace(/\s+/g, '-')) || 'off42'}-${i + 1}.png`, MIME.png),
          { icon: 'download', title: t('common.downloadPng'), className: 'icon-button' })))));
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
    const app = fileApps.find((a) => a.id === current);
    await openFile({ name: dropped.name, bytes: new Uint8Array(await dropped.arrayBuffer()) }, fileAppFor(dropped.name) ?? app);
  });
}

function setupKeys() {
  document.addEventListener('keydown', (e) => {
    if (e.defaultPrevented) return;
    const target = e.target as HTMLElement;
    if (target.closest('.menu, [role="menubar"], dialog') || document.querySelector('dialog[open]')) return;
    const app = fileApps.find((a) => a.id === current);
    const open = app?.editor;
    if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && app) {
      const key = e.key.toLowerCase();
      if (key === 'o') app.input.click();
      else if (key === 's' && open?.editor) attempt(open.download);
      else if (key === 'p' && open?.editor?.printable) open.ctx.print();
      else return;
      e.preventDefault();
      return;
    }
    if (e.ctrlKey || e.metaKey || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName) || target.closest('.grid-scroll')) return;
    open?.editor?.keydown?.(e);
  });
}

function splashProgress({ loaded, total }: Progress, translated: boolean) {
  const value = total ? Math.min(100, Math.round((loaded / total) * 100)) : 0;
  $('splash-fill').style.width = `${value}%`;
  $('splash').setAttribute('aria-valuenow', String(value));
  if (!translated) return;
  $('splash-text').textContent = t('app.loadingPercent', { percent: value });
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

  let version: string;
  try {
    version = await engine;
  } catch (error) {
    $('splash').classList.add('failed');
    $('splash-text').textContent = t('app.engineFailed', { error: message(error) });
    $('splash-note').replaceChildren(button(t('app.reload'), () => location.reload(), { icon: 'refresh', className: 'pill-button' }));
    return;
  }
  $('engine-version').textContent = t('app.engine', { version });
  $('about-version').textContent = t('app.engine', { version });

  const apps: Pick<FileApp, 'id' | 'logo' | 'extensions' | 'formats' | 'view'>[] = [
    { id: 'documents', logo: 'doc', extensions: ['docx', 'odt', 'rtf', 'md', 'markdown', 'html', 'htm', 'txt'],
      formats: 'DOCX, ODT, RTF, Markdown, HTML, TXT', view: viewDocument },
    { id: 'spreadsheets', logo: 'xls', extensions: ['xlsx', 'ods', 'xls', 'csv'], formats: 'XLSX, ODS, XLS, CSV', view: viewSpreadsheet },
    { id: 'presentations', logo: 'ppt', extensions: ['pptx', 'odp'], formats: 'PPTX, ODP', view: viewPresentation },
    { id: 'pdf', logo: 'pdf', extensions: ['pdf'], formats: 'PDF', view: viewPdf },
  ];
  for (const spec of apps) {
    const app = spec as FileApp;
    fileApps.push(app);
    buildFileApp(app);
  }
  for (const id of ['labels', 'signatures'] as const) {
    const section = $(id);
    section.prepend(topBar(id));
    $('app').append(section);
  }
  placeIcons();
  setupDrawer();
  ['about', 'stats', 'confirm'].forEach((id) => setupDialog($<HTMLDialogElement>(id)));
  setupDrop();
  setupKeys();
  window.addEventListener('hashchange', route);
  route();
  $('app').hidden = false;
  $('splash').classList.add('done');
  setTimeout(() => $('splash').remove(), 400);
  document.body.dataset.ready = 'true';
}

start();
