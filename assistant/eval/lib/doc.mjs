// In-memory document model and executor for assistant/tools/document.json.
// Mirrors doc42-core's RichDocument shape: paragraphs with stable ids, a paragraph style and styled runs.

export const STYLES = ['normal', 'heading1', 'heading2', 'heading3', 'bullet_list', 'numbered_list'];
const MARKS = ['bold', 'italic', 'underline'];

export function makeDoc(paragraphs) {
  // paragraphs: [{style?, text} | {style?, runs:[{text,bold?,...}]}]
  let id = 0;
  return {
    kind: 'document',
    nextId: 0,
    paragraphs: paragraphs.map((p) => ({
      id: (id += 1),
      style: p.style || 'normal',
      runs: p.runs ? p.runs.map((r) => ({ ...r })) : [{ text: p.text }],
    })),
  };
}

export const textOf = (p) => p.runs.map((r) => r.text).join('');

function chars(p) {
  const out = [];
  for (const r of p.runs) for (const ch of r.text) out.push({ ch, bold: !!r.bold, italic: !!r.italic, underline: !!r.underline });
  return out;
}
function fromChars(cs) {
  const runs = [];
  for (const c of cs) {
    const last = runs[runs.length - 1];
    if (last && MARKS.every((m) => !!last[m] === c[m])) last.text += c.ch;
    else {
      const r = { text: c.ch };
      for (const m of MARKS) if (c[m]) r[m] = true;
      runs.push(r);
    }
  }
  return runs.length ? runs : [{ text: '' }];
}
// Offsets are in code points so they match what the model sees in JSON.
const cpLen = (s) => [...s].length;
function findAll(hay, needle, matchCase) {
  const h = [...(matchCase ? hay : hay.toLowerCase())];
  const n = [...(matchCase ? needle : needle.toLowerCase())];
  const out = [];
  for (let i = 0; i + n.length <= h.length; ) {
    let ok = true;
    for (let j = 0; j < n.length; j++) if (h[i + j] !== n[j]) { ok = false; break; }
    if (ok) { out.push(i); i += n.length; } else i++;
  }
  return out;
}

class ToolError extends Error {}
export { ToolError };

function maxId(doc) { return doc.paragraphs.reduce((m, p) => Math.max(m, p.id), 0); }
function para(doc, id) {
  const p = doc.paragraphs.find((x) => x.id === id);
  if (!p) throw new ToolError(`paragraph ${id} not found; call get_outline for current ids`);
  return p;
}

function formatting(p) {
  const spans = [];
  let pos = 0;
  for (const r of p.runs) {
    const n = cpLen(r.text);
    const marks = MARKS.filter((m) => r[m]);
    if (marks.length && n) {
      const span = { start: pos, end: pos + n };
      for (const m of marks) span[m] = true;
      spans.push(span);
    }
    pos += n;
  }
  return spans;
}

export function outline(doc) {
  return {
    paragraphs: doc.paragraphs.map((p) => {
      const t = [...textOf(p)];
      return { id: p.id, style: p.style, length: t.length, text: t.length > 100 ? t.slice(0, 100).join('') + '…' : t.join('') };
    }),
  };
}

export const docTools = {
  get_outline(doc) { return outline(doc); },

  read_range(doc, { ids }) {
    return {
      paragraphs: ids.map((id) => {
        const p = para(doc, id);
        const out = { id, style: p.style, text: textOf(p) };
        const f = formatting(p);
        if (f.length) out.formatting = f;
        return out;
      }),
    };
  },

  find(doc, { text, match_case = false }) {
    const matches = [];
    for (const p of doc.paragraphs) {
      const t = [...textOf(p)];
      for (const s of findAll(textOf(p), text, match_case)) {
        if (matches.length >= 50) break;
        const n = cpLen(text);
        matches.push({ id: p.id, start: s, context: t.slice(Math.max(0, s - 30), s + n + 30).join('') });
      }
    }
    return { count: matches.length, matches };
  },

  replace_text(doc, { id, find, replace }) {
    if (/[\r\n]/.test(replace) || (find && /[\r\n]/.test(find))) throw new ToolError('line breaks are not allowed; use insert_paragraphs for new paragraphs');
    if (id === undefined && find === undefined) throw new ToolError('give id, find or both');
    const targets = id === undefined ? doc.paragraphs : [para(doc, id)];
    if (find === undefined) {
      const p = targets[0];
      const cs = chars(p);
      const base = cs[0] || { bold: false, italic: false, underline: false };
      p.runs = fromChars([...replace].map((ch) => ({ ...base, ch })));
      return { replaced: 1 };
    }
    let count = 0;
    for (const p of targets) {
      const hits = findAll(textOf(p), find, true);
      if (!hits.length) continue;
      const cs = chars(p);
      const n = cpLen(find);
      const out = [];
      let i = 0;
      for (const h of hits) {
        out.push(...cs.slice(i, h));
        const style = cs[h];
        for (const ch of replace) out.push({ ...style, ch });
        i = h + n;
        count++;
      }
      out.push(...cs.slice(i));
      p.runs = fromChars(out);
    }
    if (!count) throw new ToolError(`"${find}" not found${id === undefined ? '' : ` in paragraph ${id}`} (search is exact and case-sensitive)`);
    return { replaced: count };
  },

  insert_paragraphs(doc, { after_id, paragraphs }) {
    let at = 0;
    if (after_id !== 0) at = doc.paragraphs.indexOf(para(doc, after_id)) + 1;
    let next = maxId(doc);
    const created = paragraphs.map((x) => {
      if (/[\r\n]/.test(x.text)) throw new ToolError('text must not contain line breaks; pass one item per paragraph');
      return { id: (next += 1), style: x.style, runs: [{ text: x.text }] };
    });
    doc.paragraphs.splice(at, 0, ...created);
    return { ids: created.map((p) => p.id) };
  },

  set_paragraph_style(doc, { ids, style }) {
    const ps = ids.map((id) => para(doc, id));
    for (const p of ps) p.style = style;
    return { updated: ps.length };
  },

  apply_text_style(doc, args) {
    const { ids, find, match_case = false } = args;
    const set = MARKS.filter((m) => args[m] !== undefined);
    if (!set.length) throw new ToolError('set at least one of bold, italic, underline');
    if (!ids && !find) throw new ToolError('give ids, find or both');
    const targets = ids ? ids.map((id) => para(doc, id)) : doc.paragraphs;
    let spans = 0;
    for (const p of targets) {
      const cs = chars(p);
      const ranges = find ? findAll(textOf(p), find, match_case).map((s) => [s, s + cpLen(find)]) : [[0, cs.length]];
      for (const [a, b] of ranges) {
        for (let i = a; i < b; i++) for (const m of set) cs[i][m] = !!args[m];
        spans++;
      }
      p.runs = fromChars(cs);
    }
    if (!spans && find) throw new ToolError(`"${find}" not found`);
    return { spans };
  },

  delete_paragraphs(doc, { ids }) {
    const ps = ids.map((id) => para(doc, id));
    if (ps.length >= doc.paragraphs.length) {
      // The document always keeps one paragraph, as in doc42-core.
      doc.paragraphs = [{ id: maxId(doc) + 1, style: 'normal', runs: [{ text: '' }] }];
      return { deleted: ps.length };
    }
    doc.paragraphs = doc.paragraphs.filter((p) => !ps.includes(p));
    return { deleted: ps.length };
  },
};

// Snapshot sent as the first user message: the outline only (D13: outline + selection, text via read tools).
export function docSnapshot(doc) {
  return 'Open file: text document.\nOutline (JSON):\n' + JSON.stringify(outline(doc));
}
