// SVG icons on a 24-unit grid, drawn with the current text colour (no glyph or emoji icons anywhere in the UI),
// and the six app logos: a folded sheet in the app colour with a white glyph.

// Stroked line icons. `F:` marks a shape that is filled instead of stroked.
const ICONS: Record<string, string> = {
  menu: 'M4 7h16M4 12h16M4 17h16',
  search: 'M11 4a7 7 0 1 0 0 14a7 7 0 1 0 0-14z M20 20l-3.5-3.5',
  globe: 'M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18z M3 12h18 M12 3c2.5 2.8 3.7 5.8 3.7 9s-1.2 6.2-3.7 9c-2.5-2.8-3.7-5.8-3.7-9S9.5 5.8 12 3z',
  help: 'M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18z M9.6 9.3a2.5 2.5 0 1 1 3.4 2.3c-.6.3-1 .8-1 1.5v.4 M12 17h.01',
  lock: 'M7 11h10a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2v-6a2 2 0 0 1 2-2z M8 11V8a4 4 0 0 1 8 0v3',
  download: 'M12 4v11 M7 10l5 5 5-5 M5 20h14',
  open: 'M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z M12 16v-6 M9.5 12.5 12 10l2.5 2.5',
  folder: 'M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z',
  more: 'F:M12 3.2a1.8 1.8 0 1 0 0 3.6a1.8 1.8 0 1 0 0-3.6z M12 10.2a1.8 1.8 0 1 0 0 3.6a1.8 1.8 0 1 0 0-3.6z M12 17.2a1.8 1.8 0 1 0 0 3.6a1.8 1.8 0 1 0 0-3.6z',
  back: 'M19 12H5 M11 6l-6 6 6 6',
  check: 'M5 12.5l4.5 4.5L19 7.5',
  next: 'M9 6l6 6-6 6',
  prev: 'M15 6l-6 6 6 6',
  down: 'M6 9l6 6 6-6',
  up: 'M6 15l6-6 6 6',
  print: 'M7 9V3h10v6 M5 9h14a2 2 0 0 1 2 2v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4a2 2 0 0 1 2-2z M7 14h10v7H7z',
  info: 'M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18z M12 11v6 M12 7.5h.01',
  close: 'M6 6l12 12 M18 6 6 18',
  plus: 'M12 5v14 M5 12h14',
  minus: 'M5 12h14',
  zoomIn: 'M11 4a7 7 0 1 0 0 14a7 7 0 1 0 0-14z M20 20l-3.5-3.5 M8 11h6 M11 8v6',
  zoomOut: 'M11 4a7 7 0 1 0 0 14a7 7 0 1 0 0-14z M20 20l-3.5-3.5 M8 11h6',
  stats: 'M5 20V10 M10 20V4 M15 20v-7 M20 20v-4 M3 20h18',
  panel: 'M4 4h16a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z M15 4v16',
  bookmark: 'M7 3h10v18l-5-4-5 4z',
  attach: 'M16 8l-6.5 6.5a2 2 0 0 0 2.8 2.8L19 10.6a4 4 0 0 0-5.7-5.7L6.6 11.6a6 6 0 0 0 8.5 8.5L20 15',
  field: 'M3 7h18v10H3z M7 10v4',
  warn: 'M12 3 22 20H2z M12 9v5 M12 17h.01',
  refresh: 'M20 8a8 8 0 1 0 .8 6 M20 3v5h-5',
  notes: 'M6 3h12v18H6z M9 8h6 M9 12h6 M9 16h4',
  picture: 'M5 4h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z M9 8a2 2 0 1 0 0 4a2 2 0 1 0 0-4z M21 16l-5-5-9 9',
  fullscreen: 'M4 9V4h5 M15 4h5v5 M20 15v5h-5 M9 20H4v-5',
  rename: 'M4 20h4L19 9l-4-4L4 16z M13.5 6.5l4 4',
  file: 'M6 3h8l4 4v14H6z M14 3v4h4',
  external: 'M14 4h6v6 M20 4l-9 9 M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5',
  list: 'M4 7h16 M4 12h16 M4 17h16',
  slides: 'M4 5h16v11H4z M9 20h6 M12 16v4',
  first: 'M7 6v12 M17 6l-6 6 6 6',
  last: 'M17 6v12 M7 6l6 6-6 6',
  qr: 'M4 4h6v6H4z M14 4h6v6h-6z M4 14h6v6H4z M14 14h2v2h-2z M18 18h2v2h-2z',
};

const NS = 'http://www.w3.org/2000/svg';

export function icon(name: string, className = 'icon'): SVGSVGElement {
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('class', className);
  svg.setAttribute('aria-hidden', 'true');
  const spec = ICONS[name] ?? ICONS.file;
  const path = document.createElementNS(NS, 'path');
  if (spec.startsWith('F:')) {
    svg.setAttribute('fill', 'currentColor');
    path.setAttribute('d', spec.slice(2));
  } else {
    svg.setAttribute('fill', 'none');
    svg.setAttribute('stroke', 'currentColor');
    svg.setAttribute('stroke-width', '1.8');
    svg.setAttribute('stroke-linecap', 'round');
    svg.setAttribute('stroke-linejoin', 'round');
    path.setAttribute('d', spec);
  }
  svg.append(path);
  return svg;
}

// App logos on a 28 × 32 grid: the sheet, its folded corner (a lighter tint) and a white glyph.
const LOGOS: Record<string, { color: string; fold: string; glyph: string }> = {
  doc: { color: '#2f6fde', fold: '#a9c2f2', glyph: '<path d="M8 15h12M8 19h12M8 23h8" stroke="#fff" stroke-width="2"/>' },
  xls: { color: '#1e8e4f', fold: '#9fd6b5', glyph: '<path d="M7.5 14h13v10h-13zM7.5 19h13M14 14v10" stroke="#fff" stroke-width="1.8" fill="none"/>' },
  ppt: { color: '#e0891b', fold: '#f6cf98', glyph: '<rect x="7.5" y="14" width="13" height="9" rx="1" stroke="#fff" stroke-width="1.8" fill="none"/>' },
  pdf: { color: '#d23f31', fold: '#f2aaa2', glyph: '<path d="M8 14h12M8 18h8" stroke="#fff" stroke-width="2"/><rect x="7" y="21" width="14" height="4.5" rx="1" fill="#fff"/>' },
  labels: { color: '#0b8a8a', fold: '#95d3d0', glyph: '<path d="M8 14h4v4H8zM16 14h4v4h-4zM8 22h4v4H8z" fill="#fff"/><path d="M16 22h1.6v1.6H16zM18.4 24.4H20V26h-1.6z" fill="#fff"/>' },
  signatures: { color: '#7b4fc4', fold: '#c9b4ea', glyph: '<path d="M6.5 24c2 0 3-4.5 4.6-4.5S10.4 23.5 12.5 23.5s2.3-3 4-3 1.2 2.2 2.6 2.2 1.8-1.4 3-1.4" stroke="#fff" stroke-width="1.8" fill="none" stroke-linecap="round"/><path d="M14 18.5l5.5-5.5" stroke="#fff" stroke-width="2" stroke-linecap="round"/>' },
};

export const APP_COLORS = Object.fromEntries(Object.entries(LOGOS).map(([id, logo]) => [id, logo.color]));

export function appLogo(app: string, size = 28): SVGSVGElement {
  const logo = LOGOS[app] ?? LOGOS.doc;
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('viewBox', '0 0 28 32');
  svg.setAttribute('width', String(Math.round((size * 28) / 32)));
  svg.setAttribute('height', String(size));
  svg.setAttribute('class', 'app-logo');
  svg.setAttribute('aria-hidden', 'true');
  // Static markup from this file only.
  svg.innerHTML = `<path d="M3 2h15l7 7v21H3z" fill="${logo.color}"/><path d="M18 2v7h7" fill="${logo.fold}"/>${logo.glyph}`;
  return svg;
}

/** Fills every `[data-icon]` placeholder below `root` with its icon (`[data-logo]` with an app logo). */
export function placeIcons(root: ParentNode = document) {
  root.querySelectorAll<HTMLElement>('[data-icon]').forEach((node) => {
    if (!node.firstChild) node.append(icon(node.dataset.icon!));
  });
  root.querySelectorAll<HTMLElement>('[data-logo]').forEach((node) => {
    if (!node.firstChild) node.append(appLogo(node.dataset.logo!, Number(node.dataset.size) || 28));
  });
}
