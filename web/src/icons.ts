// Line icons on a 36-unit grid, drawn with the current text colour. The app icons are the ones off42.cc uses.
const PATHS: Record<string, string> = {
  doc: 'M8 3h16v28H8z M12 10h8 M12 15h8 M12 20h8 M12 25h5',
  xls: 'M5 6h23v23H5z M5 13h23 M5 21h23 M13 6v23 M21 6v23',
  ppt: 'M4 6h25v18H4z M16 24v7 M10 31h12 M10 17l4-5 5 4 5-7',
  pdf: 'M9 3h9l6 6v22H9z M18 3v7h6 M13 17h7 M13 22h7',
  labels: 'M5 5h10v10H5z M21 5h10v10H21z M5 21h10v10H5z M9 9h2v2H9z M25 9h2v2h-2z M9 25h2v2H9z M21 21h4v4h-4z M29 21h2 M21 29h2 M27 27h4v4h-4z',
  sign: 'M4 30c3 0 5-6 7.5-6S11 29 14 29s3.5-4 6-4 2 3 4 3 3-2 5-2 M18 18 29 7a2.8 2.8 0 0 0-4-4L14 14l-1 5z',
  lock: 'M10 15v-5a7 7 0 0 1 14 0v5 M7 15h20v17H7z M17 22v4',
  globe: 'M18 4a14 14 0 1 0 0 28a14 14 0 1 0 0-28z M4 18h28 M18 4c-4.5 4-6 9-6 14s1.5 10 6 14 M18 4c4.5 4 6 9 6 14s-1.5 10-6 14',
  download: 'M18 5v18 M11 16l7 7 7-7 M6 26v5h24v-5',
  open: 'M4 9h10l3 3h15v17H4z M18 26V16 M13.5 20.5 18 16l4.5 4.5',
  search: 'M15 5a10 10 0 1 0 0 20a10 10 0 1 0 0-20z M22.5 22.5 31 31',
  refresh: 'M29 12a12 12 0 1 0 1 9 M29 4v8h-8',
  close: 'M9 9l18 18 M27 9 9 27',
  clock: 'M18 5a13 13 0 1 0 0 26a13 13 0 1 0 0-26z M18 11v8l5 3',
  page: 'M9 4h12l6 6v22H9z M21 4v6h6',
  picture: 'M5 7h26v22H5z M5 25l8-8 6 6 4-4 8 8 M23 13h.1',
  attach: 'M24 12 14 22a3 3 0 0 0 4 4l11-11a6 6 0 0 0-8.5-8.5L9 18a9 9 0 0 0 12.7 12.7L30 22',
  bookmark: 'M10 4h16v27l-8-6-8 6z',
  field: 'M4 11h28v14H4z M9 15v6',
  info: 'M18 4a14 14 0 1 0 0 28a14 14 0 1 0 0-28z M18 16v9 M18 11v.1',
  warn: 'M18 4 33 30H3z M18 13v8 M18 25v.1',
  check: 'M7 18l7 7L29 10',
  slides: 'M4 6h25v18H4z M10 12h13 M10 17h9',
  notes: 'M8 4h20v28H8z M12 11h12 M12 16h12 M12 21h8',
  chevron: 'M14 9l9 9-9 9',
};

export function icon(name: string, className = 'icon'): SVGSVGElement {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 36 36');
  svg.setAttribute('class', className);
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '2');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  path.setAttribute('d', PATHS[name] ?? PATHS.page);
  svg.append(path);
  return svg;
}

/** Fills every `[data-icon]` placeholder below `root` with its icon. */
export function placeIcons(root: ParentNode = document) {
  root.querySelectorAll<HTMLElement>('[data-icon]').forEach((node) => {
    if (!node.firstChild) node.append(icon(node.dataset.icon!));
  });
}
