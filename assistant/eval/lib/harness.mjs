// Shared pieces: prompt/tool loading, request layout, tool execution.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { validate } from './schema.mjs';
import { docTools, docSnapshot, ToolError } from './doc.mjs';
import { sheetTools, sheetSnapshot } from './sheet.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
export const SYSTEM = readFileSync(join(root, 'prompt', 'system.md'), 'utf8').trim();
export const TOOLS = {
  document: JSON.parse(readFileSync(join(root, 'tools', 'document.json'), 'utf8')),
  spreadsheet: JSON.parse(readFileSync(join(root, 'tools', 'spreadsheet.json'), 'utf8')),
};
const IMPL = { document: docTools, spreadsheet: sheetTools };
export const snapshot = (state) => (state.kind === 'document' ? docSnapshot(state) : sheetSnapshot(state));

// Deterministic JSON (sorted keys) for tool results, so identical states give identical bytes.
export function stable(v) {
  if (Array.isArray(v)) return '[' + v.map(stable).join(',') + ']';
  if (v && typeof v === 'object') return '{' + Object.keys(v).sort().map((k) => JSON.stringify(k) + ':' + stable(v[k])).join(',') + '}';
  return JSON.stringify(v === undefined ? null : v);
}

// Request layout: [system + tools: identical for every request and user] -> [file snapshot: fixed for the
// session] -> [conversation: append-only]. cacheControl adds explicit breakpoints after the system prompt
// and after the snapshot, for providers that only cache on request (Alibaba's Qwen endpoints).
export function initialMessages(state, userText, cacheControl) {
  const part = (text) => (cacheControl ? [{ type: 'text', text, cache_control: { type: 'ephemeral' } }] : text);
  return [
    { role: 'system', content: part(SYSTEM) },
    { role: 'user', content: part(snapshot(state)) },
    { role: 'user', content: userText },
  ];
}

// Execute one tool call against the in-memory file. Returns {content, valid, error}.
export function execute(state, call) {
  const tools = TOOLS[state.kind];
  const name = call.function?.name;
  const def = tools.find((t) => t.function.name === name);
  if (!def) return { valid: false, error: `unknown tool ${name}`, content: stable({ error: `unknown tool "${name}"` }) };
  let args;
  try {
    const raw = call.function.arguments;
    args = raw === '' || raw === undefined || raw === null ? {} : typeof raw === 'string' ? JSON.parse(raw) : raw;
  } catch {
    return { valid: false, error: 'arguments are not JSON', content: stable({ error: 'arguments are not valid JSON' }) };
  }
  const errs = validate(def.function.parameters, args);
  if (errs.length) return { valid: false, error: errs.join('; '), content: stable({ error: 'invalid arguments: ' + errs.join('; ') }) };
  try {
    return { valid: true, content: stable(IMPL[state.kind][name](state, args)) };
  } catch (e) {
    if (e instanceof ToolError) return { valid: true, toolError: e.message, content: stable({ error: e.message }) };
    return { valid: true, harnessError: String(e.stack || e), content: stable({ error: 'internal error' }) };
  }
}
