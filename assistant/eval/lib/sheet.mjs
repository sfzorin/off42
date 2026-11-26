// In-memory single-sheet spreadsheet and executor for assistant/tools/spreadsheet.json.
// Formulas: + - * / ^, comparison, &, parentheses, cell refs ($ allowed), ranges,
// SUM AVERAGE MIN MAX COUNT COUNTA ROUND ABS IF PRODUCT. Formulas shift on insert/delete/fill/sort.
import { ToolError } from './doc.mjs';

export const colName = (c) => { let s = ''; c += 1; while (c) { const m = (c - 1) % 26; s = String.fromCharCode(65 + m) + s; c = Math.floor((c - 1) / 26); } return s; };
export const colIndex = (s) => { let n = 0; for (const ch of s.toUpperCase()) n = n * 26 + ch.charCodeAt(0) - 64; return n - 1; };
export const addr = (r, c) => colName(c) + (r + 1);

export function parseCell(a) {
  const m = /^\$?([A-Za-z]{1,3})\$?(\d+)$/.exec(String(a).trim());
  if (!m || +m[2] < 1) throw new ToolError(`invalid cell "${a}"`);
  return { r: +m[2] - 1, c: colIndex(m[1]) };
}
export function parseRange(s) {
  const parts = String(s).trim().replace(/^'?[^!]*'?!/, '').split(':');
  if (parts.length > 2) throw new ToolError(`invalid range "${s}"`);
  const a = parseCell(parts[0]);
  const b = parts[1] ? parseCell(parts[1]) : a;
  return { top: Math.min(a.r, b.r), left: Math.min(a.c, b.c), bottom: Math.max(a.r, b.r), right: Math.max(a.c, b.c) };
}
const rangeText = (g) => addr(g.top, g.left) + (g.top === g.bottom && g.left === g.right ? '' : ':' + addr(g.bottom, g.right));

export function makeSheet(rows, name = 'Sheet1') {
  const s = { kind: 'spreadsheet', name, cells: new Map() };
  rows.forEach((row, r) => row.forEach((v, c) => { if (v !== null && v !== undefined && v !== '') setInput(s, r, c, v); }));
  return s;
}
const key = (r, c) => r + ',' + c;
function setInput(s, r, c, v) {
  const k = key(r, c);
  const old = s.cells.get(k) || {};
  if (v === null || v === '') { if (old.format) s.cells.set(k, { format: old.format }); else s.cells.delete(k); return; }
  let input = v;
  if (typeof v === 'string' && !v.startsWith('=')) {
    const t = v.trim();
    if (/^-?\d+(\.\d+)?$/.test(t)) input = Number(t);
    else if (/^-?\d+(\.\d+)?%$/.test(t)) input = Number(t.slice(0, -1)) / 100;
  }
  s.cells.set(k, { ...old, input });
}
export function used(s) {
  let bottom = -1, right = -1;
  for (const [k, v] of s.cells) { if (v.input === undefined) continue; const [r, c] = k.split(',').map(Number); bottom = Math.max(bottom, r); right = Math.max(right, c); }
  return { top: 0, left: 0, bottom, right };
}

// ---------- formulas ----------
export function shiftFormula(f, dr, dc) {
  const one = (x) => { const r = x.absR ? x.r : x.r + dr, c = x.absC ? x.c : x.c + dc; return r < 0 || c < 0 ? null : { ...x, r, c }; };
  return transformRefs(f, one, (a, b) => { const A = one(a), B = one(b); return A && B ? [A, B] : null; });
}

function tokenize(src) {
  const t = []; let i = 0;
  while (i < src.length) {
    const ch = src[i];
    if (/\s/.test(ch)) { i++; continue; }
    let m;
    const rest = src.slice(i);
    if ((m = /^\d+(\.\d+)?(e[+-]?\d+)?%?/i.exec(rest))) { let v = parseFloat(m[0]); if (m[0].endsWith('%')) v /= 100; t.push({ k: 'num', v }); i += m[0].length; continue; }
    if ((m = /^"([^"]*)"/.exec(rest))) { t.push({ k: 'str', v: m[1] }); i += m[0].length; continue; }
    if ((m = /^#REF!/.exec(rest))) { t.push({ k: 'err', v: '#REF!' }); i += 5; continue; }
    if ((m = /^\$?[A-Za-z]{1,3}\$?\d+(:\$?[A-Za-z]{1,3}\$?\d+)?(?![(A-Za-z_])/.exec(rest))) { t.push({ k: 'ref', v: m[0] }); i += m[0].length; continue; }
    if ((m = /^[A-Za-z_][A-Za-z0-9_.]*/.exec(rest))) { t.push({ k: 'name', v: m[0].toUpperCase() }); i += m[0].length; continue; }
    if ((m = /^(<=|>=|<>|[-+*/^(),;:&=<>])/.exec(rest))) { t.push({ k: 'op', v: m[0] === ';' ? ',' : m[0] }); i += m[0].length; continue; }
    throw new Error('#NAME?');
  }
  return t;
}
class Err { constructor(v) { this.v = v; } toString() { return this.v; } }
function evalFormula(s, src, stack) {
  const toks = tokenize(src); let p = 0;
  const peek = () => toks[p]; const eat = (v) => { if (toks[p] && toks[p].v === v) { p++; return true; } return false; };
  const num = (x) => { if (x instanceof Err) throw x; if (Array.isArray(x)) x = x[0]; if (x === null || x === '') return 0; if (typeof x === 'boolean') return +x; if (typeof x === 'number') return x; const n = Number(x); if (Number.isNaN(n)) throw new Err('#VALUE!'); return n; };
  function primary() {
    const t = toks[p++];
    if (!t) throw new Err('#ERROR!');
    if (t.k === 'num' || t.k === 'str') return t.v;
    if (t.k === 'err') throw new Err(t.v);
    if (t.k === 'ref') {
      const g = parseRange(t.v);
      if (!t.v.includes(':')) return valueAt(s, g.top, g.left, stack);
      const vals = []; for (let r = g.top; r <= g.bottom; r++) for (let c = g.left; c <= g.right; c++) vals.push(valueAt(s, r, c, stack));
      return vals;
    }
    if (t.k === 'name') {
      if (t.v === 'TRUE') return true; if (t.v === 'FALSE') return false;
      if (!eat('(')) throw new Err('#NAME?');
      const args = [];
      if (!eat(')')) { do args.push(expr()); while (eat(',')); if (!eat(')')) throw new Err('#ERROR!'); }
      const flat = args.flat();
      const nums = flat.filter((x) => typeof x === 'number');
      for (const x of flat) if (x instanceof Err) throw x;
      switch (t.v) {
        case 'SUM': return nums.reduce((a, b) => a + b, 0);
        case 'PRODUCT': return nums.reduce((a, b) => a * b, 1);
        case 'AVERAGE': if (!nums.length) throw new Err('#DIV/0!'); return nums.reduce((a, b) => a + b, 0) / nums.length;
        case 'MIN': return nums.length ? Math.min(...nums) : 0;
        case 'MAX': return nums.length ? Math.max(...nums) : 0;
        case 'COUNT': return nums.length;
        case 'COUNTA': return flat.filter((x) => x !== null && x !== '').length;
        case 'ROUND': { const k = 10 ** num(args[1] ?? 0); return Math.round(num(args[0]) * k) / k; }
        case 'ABS': return Math.abs(num(args[0]));
        case 'IF': return (Array.isArray(args[0]) ? args[0][0] : args[0]) ? args[1] : (args[2] ?? false);
        default: throw new Err('#NAME?');
      }
    }
    if (t.v === '(') { const v = expr(); if (!eat(')')) throw new Err('#ERROR!'); return v; }
    if (t.v === '-') return -num(unary());
    if (t.v === '+') return num(unary());
    throw new Err('#ERROR!');
  }
  function unary() { return primary(); }
  function pow() { let v = unary(); while (eat('^')) v = num(v) ** num(unary()); return v; }
  function mul() { let v = pow(); for (;;) { if (eat('*')) v = num(v) * num(pow()); else if (eat('/')) { const d = num(pow()); if (d === 0) throw new Err('#DIV/0!'); v = num(v) / d; } else return v; } }
  function add() { let v = mul(); for (;;) { if (eat('+')) v = num(v) + num(mul()); else if (eat('-')) v = num(v) - num(mul()); else return v; } }
  function cat() { let v = add(); while (eat('&')) v = String(Array.isArray(v) ? v[0] ?? '' : v ?? '') + String(add() ?? ''); return v; }
  function expr() {
    let v = cat();
    for (const op of ['<=', '>=', '<>', '=', '<', '>']) if (peek() && peek().v === op) { p++; const w = cat(); const a = typeof v === 'number' ? v : v; return op === '=' ? a == w : op === '<>' ? a != w : op === '<' ? a < w : op === '>' ? a > w : op === '<=' ? a <= w : a >= w; }
    return v;
  }
  const v = expr();
  if (p !== toks.length) throw new Err('#ERROR!');
  return Array.isArray(v) ? v[0] ?? null : v;
}
export function valueAt(s, r, c, stack = new Set()) {
  const cell = s.cells.get(key(r, c));
  if (!cell || cell.input === undefined) return null;
  if (typeof cell.input !== 'string' || !cell.input.startsWith('=')) return cell.input;
  const k = key(r, c);
  if (stack.has(k)) return new Err('#CIRC!');
  stack.add(k);
  try { return evalFormula(s, cell.input.slice(1), stack); }
  catch (e) { return e instanceof Err ? e : new Err(e.message.startsWith('#') ? e.message : '#ERROR!'); }
  finally { stack.delete(k); }
}
export const valueJSON = (v) => (v instanceof Err ? v.v : v);
export const formulaAt = (s, r, c) => { const x = s.cells.get(key(r, c)); return x && typeof x.input === 'string' && x.input.startsWith('=') ? x.input : null; };
export const formatAt = (s, r, c) => (s.cells.get(key(r, c)) || {}).format || null;

// ---------- structural edits ----------
// Transform every reference in a formula. fn(ref) for single refs returns a ref or null (#REF!);
// ranges are passed to rangeFn(a, b) which returns [a, b] or null.
const ANYREF = /("[^"]*")|((?:^|[^A-Za-z_$\d]))(\$?[A-Z]{1,3}\$?\d+)(?::(\$?[A-Z]{1,3}\$?\d+))?(?![\d(A-Za-z_])/g;
const parseRef = (t) => { const m = /^(\$?)([A-Z]{1,3})(\$?)(\d+)$/.exec(t); return { absC: !!m[1], c: colIndex(m[2]), absR: !!m[3], r: +m[4] - 1 }; };
const refText = (x) => (x.absC ? '$' : '') + colName(x.c) + (x.absR ? '$' : '') + (x.r + 1);
function transformRefs(formula, fn, rangeFn) {
  return formula.replace(ANYREF, (m, str, pre, a, b) => {
    if (str) return str;
    if (b) { const res = rangeFn(parseRef(a), parseRef(b)); return pre + (res ? refText(res[0]) + ':' + refText(res[1]) : '#REF!'); }
    const res = fn(parseRef(a));
    return pre + (res ? refText(res) : '#REF!');
  });
}
function remap(s, cellFn, refFn, rangeFn) {
  const next = new Map();
  for (const [k, v] of s.cells) {
    const [r, c] = k.split(',').map(Number);
    const to = cellFn(r, c);
    if (!to) continue;
    const nv = { ...v };
    if (typeof nv.input === 'string' && nv.input.startsWith('=')) nv.input = transformRefs(nv.input, refFn, rangeFn);
    next.set(key(to.r, to.c), nv);
  }
  s.cells = next;
}
const F = (axis) => (axis === 'r' ? 'r' : 'c');
function insertAxis(s, axis, at, n) {
  const pos = (x) => (x >= at ? x + n : x);
  const ref = (x) => ({ ...x, [F(axis)]: pos(x[F(axis)]) });
  remap(s, (r, c) => (axis === 'r' ? { r: pos(r), c } : { r, c: pos(c) }), ref, (a, b) => [ref(a), ref(b)]);
}
function deleteAxis(s, axis, at, n) {
  const f = F(axis);
  const pos = (x) => (x < at ? x : x >= at + n ? x - n : null);
  const ref = (x) => { const v = pos(x[f]); return v === null ? null : { ...x, [f]: v }; };
  const range = (a, b) => {
    // Endpoints inside the deleted block clamp to the remaining cells, as spreadsheets do.
    let lo = a[f], hi = b[f];
    lo = lo >= at && lo < at + n ? at + n : lo;
    hi = hi >= at && hi < at + n ? at - 1 : hi;
    if (hi < lo) return null;
    return [{ ...a, [f]: pos(lo) }, { ...b, [f]: pos(hi) }];
  };
  remap(s, (r, c) => { const v = pos(axis === 'r' ? r : c); return v === null ? null : axis === 'r' ? { r: v, c } : { r, c: v }; }, ref, range);
}

const MAX_CELLS = 2000;
function info(s) {
  const u = used(s);
  const preview = [];
  for (let r = 0; r <= Math.min(u.bottom, 4); r++) { const row = []; for (let c = 0; c <= u.right; c++) row.push(valueJSON(valueAt(s, r, c))); preview.push(row); }
  const out = { sheet: s.name, used_range: u.bottom < 0 ? null : rangeText(u), rows: u.bottom + 1, columns: u.right + 1, first_rows: preview };
  // Say plainly when the preview is partial; models otherwise treat it as the whole table.
  if (u.bottom > 4) out.rows_not_shown = `${u.bottom - 4} more rows (6 to ${u.bottom + 1}); use read_cells`;
  return out;
}

export const sheetTools = {
  get_sheet_info: (s) => info(s),

  read_cells(s, { range }) {
    const g = parseRange(range);
    if ((g.bottom - g.top + 1) * (g.right - g.left + 1) > MAX_CELLS) throw new ToolError(`range too large (max ${MAX_CELLS} cells)`);
    const values = [], formulas = {}, formats = {};
    for (let r = g.top; r <= g.bottom; r++) {
      const row = [];
      for (let c = g.left; c <= g.right; c++) {
        row.push(valueJSON(valueAt(s, r, c)));
        const f = formulaAt(s, r, c); if (f) formulas[addr(r, c)] = f;
        const n = formatAt(s, r, c); if (n) formats[addr(r, c)] = n;
      }
      values.push(row);
    }
    const out = { range: rangeText(g), values };
    if (Object.keys(formulas).length) out.formulas = formulas;
    if (Object.keys(formats).length) out.formats = formats;
    return out;
  },

  set_cells(s, { range, values }) {
    const g = parseRange(range);
    const width = Math.max(...values.map((r) => r.length));
    if (values.length * width > MAX_CELLS * 5) throw new ToolError('too many cells');
    const conv = (v) => (typeof v === 'boolean' ? String(v).toUpperCase() : v);
    if (values.length === 1 && width === 1 && (g.bottom > g.top || g.right > g.left)) {
      const v = conv(values[0][0]);
      for (let r = g.top; r <= g.bottom; r++) for (let c = g.left; c <= g.right; c++)
        setInput(s, r, c, typeof v === 'string' && v.startsWith('=') ? shiftFormula(v, r - g.top, c - g.left) : v);
      return { written: rangeText(g) };
    }
    if (g.bottom > g.top || g.right > g.left) {
      if (g.bottom - g.top + 1 !== values.length || g.right - g.left + 1 !== width)
        throw new ToolError(`range ${rangeText(g)} is ${g.bottom - g.top + 1}x${g.right - g.left + 1} but values are ${values.length}x${width}; give only the top-left cell or a matching range`);
    }
    values.forEach((row, i) => row.forEach((v, j) => setInput(s, g.top + i, g.left + j, conv(v))));
    return { written: rangeText({ top: g.top, left: g.left, bottom: g.top + values.length - 1, right: g.left + width - 1 }) };
  },

  insert_rows(s, { row, count }) { insertAxis(s, 'r', row - 1, count); return { inserted: count, used_range: info(s).used_range }; },
  delete_rows(s, { row, count }) { deleteAxis(s, 'r', row - 1, count); return { deleted: count, used_range: info(s).used_range }; },
  insert_columns(s, { column, count }) { if (!/^[A-Za-z]{1,3}$/.test(column)) throw new ToolError('column must be a letter like C'); insertAxis(s, 'c', colIndex(column), count); return { inserted: count, used_range: info(s).used_range }; },
  delete_columns(s, { column, count }) { if (!/^[A-Za-z]{1,3}$/.test(column)) throw new ToolError('column must be a letter like C'); deleteAxis(s, 'c', colIndex(column), count); return { deleted: count, used_range: info(s).used_range }; },

  set_number_format(s, { range, format }) {
    const g = parseRange(range);
    if ((g.bottom - g.top + 1) * (g.right - g.left + 1) > 100000) throw new ToolError('range too large');
    for (let r = g.top; r <= g.bottom; r++) for (let c = g.left; c <= g.right; c++) {
      const k = key(r, c); const cell = s.cells.get(k) || {};
      if (format === 'General') delete cell.format; else cell.format = format;
      if (Object.keys(cell).length) s.cells.set(k, cell); else s.cells.delete(k);
    }
    return { formatted: rangeText(g) };
  },

  sort_range(s, { range, by_column, descending, has_header }) {
    const g = parseRange(range);
    const kc = colIndex(by_column);
    if (kc < g.left || kc > g.right) throw new ToolError(`column ${by_column} is outside ${rangeText(g)}`);
    const start = g.top + (has_header ? 1 : 0);
    const rows = [];
    for (let r = start; r <= g.bottom; r++) {
      const cells = []; for (let c = g.left; c <= g.right; c++) cells.push(s.cells.get(key(r, c)));
      rows.push({ r, cells, v: valueJSON(valueAt(s, r, kc)) });
    }
    const rank = (v) => (v === null ? 3 : typeof v === 'number' ? 0 : typeof v === 'string' ? 1 : 2);
    const cmp = (a, b) => {
      if (a.v === null || b.v === null) return (a.v === null) - (b.v === null); // blanks last either way
      const d = rank(a.v) - rank(b.v) || (typeof a.v === 'number' ? a.v - b.v : String(a.v).localeCompare(String(b.v)));
      return descending ? -d : d;
    };
    const sorted = [...rows].sort(cmp);
    sorted.forEach((row, i) => {
      const to = start + i;
      row.cells.forEach((cell, j) => {
        const k = key(to, g.left + j);
        if (!cell) { s.cells.delete(k); return; }
        const nc = { ...cell };
        if (typeof nc.input === 'string' && nc.input.startsWith('=')) nc.input = shiftFormula(nc.input, to - row.r, 0);
        s.cells.set(k, nc);
      });
    });
    return { sorted_rows: rows.length };
  },
};

export function sheetSnapshot(s) {
  return 'Open file: spreadsheet.\nSheet info (JSON):\n' + JSON.stringify(info(s));
}
