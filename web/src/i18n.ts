// Interface strings in the 20 languages of off42.cc. Catalogs are flat JSON files in ./locales (English is the
// reference and the fallback for any missing key). The language comes from `?lang=xx`, otherwise from the
// browser's preferred languages, otherwise English.
import english from './locales/en.json';

type Catalog = Record<string, string>;

/** Language codes and native names, in the order of the site's language picker. */
export const LANGUAGES: [string, string][] = [
  ['en', 'English'], ['zh-Hans', '简体中文'], ['zh-Hant', '繁體中文（台灣）'], ['es', 'Español'], ['ar', 'العربية'],
  ['fr', 'Français'], ['pt', 'Português'], ['id', 'Bahasa Indonesia'], ['ru', 'Русский'], ['de', 'Deutsch'],
  ['ja', '日本語'], ['vi', 'Tiếng Việt'], ['tr', 'Türkçe'], ['fil', 'Filipino'], ['ko', '한국어'], ['th', 'ไทย'],
  ['it', 'Italiano'], ['pl', 'Polski'], ['ms', 'Bahasa Melayu'], ['hu', 'Magyar'],
];
const CODES = LANGUAGES.map(([code]) => code);
const RTL = new Set(['ar']);

const catalogs = import.meta.glob<Catalog>(['./locales/*.json', '!./locales/en.json'], { import: 'default' });

let strings: Catalog = english;
export let language = 'en';

/** Maps a BCP 47 tag (`de-AT`, `zh-TW`, `tl`) to one of the 20 interface languages, if any. */
export function matchLanguage(tag: string): string | undefined {
  const lower = tag.trim().toLowerCase();
  const exact = CODES.find((code) => code.toLowerCase() === lower);
  if (exact) return exact;
  const primary = lower.split(/[-_]/)[0];
  if (primary === 'zh') return /hant|-tw|-hk|-mo/.test(lower) ? 'zh-Hant' : 'zh-Hans';
  if (primary === 'tl') return 'fil';
  return CODES.find((code) => code === primary);
}

export function chooseLanguage(search = location.search, preferred = navigator.languages): string {
  const requested = new URLSearchParams(search).get('lang');
  if (requested) {
    const match = matchLanguage(requested);
    if (match) return match;
  }
  for (const tag of preferred ?? []) {
    const match = matchLanguage(tag);
    if (match) return match;
  }
  return 'en';
}

export async function loadLanguage(code: string) {
  language = CODES.includes(code) ? code : 'en';
  const load = catalogs[`./locales/${language}.json`];
  strings = load ? { ...english, ...(await load()) } : english;
  document.documentElement.lang = language;
  document.documentElement.dir = RTL.has(language) ? 'rtl' : 'ltr';
}

/** The string for `key` with `{name}` placeholders filled in. */
export function t(key: string, values: Record<string, string | number> = {}): string {
  const text = strings[key] ?? (english as Catalog)[key] ?? key;
  return text.replace(/\{(\w+)\}/g, (match, name) => (name in values ? String(values[name]) : match));
}

/** Translates the static page: `data-i18n` (text), `data-i18n-placeholder`, `data-i18n-label` (aria-label). */
export function translatePage(root: ParentNode = document) {
  root.querySelectorAll<HTMLElement>('[data-i18n]').forEach((node) => {
    node.textContent = t(node.dataset.i18n!, node.dataset.i18nFormats ? { formats: node.dataset.i18nFormats } : {});
  });
  root.querySelectorAll<HTMLInputElement>('[data-i18n-placeholder]').forEach((node) => {
    node.placeholder = t(node.dataset.i18nPlaceholder!);
  });
  root.querySelectorAll<HTMLElement>('[data-i18n-label]').forEach((node) => {
    node.setAttribute('aria-label', t(node.dataset.i18nLabel!));
  });
  document.title = t('app.title');
}

/** Reloads the page in another language (`?lang=`); the app and its hash survive, open files do not. */
export function switchLanguage(code: string) {
  const url = new URL(location.href);
  url.searchParams.set('lang', code);
  location.assign(url.toString());
}
