#!/usr/bin/env node
// OFF42 assistant evaluation against OpenRouter's chat-completions API (OpenAI-compatible tool calling).
//   node run.mjs [--models id,id] [--tasks prefix,id] [--label name] [--budget 2] [--reasoning default|off|low]
//                [--probe] [--data-collection deny|allow]
// The API key is read from OPENROUTER_API_KEY or ~/.config/off42/openrouter.key and is never printed or written.
import { readFileSync, mkdirSync, appendFileSync, writeFileSync, existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { documentTasks } from './tasks/documents.mjs';
import { spreadsheetTasks } from './tasks/spreadsheets.mjs';
import { TOOLS, initialMessages, execute } from './lib/harness.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const opt = (name, dflt) => { const i = argv.indexOf('--' + name); return i < 0 ? dflt : argv[i + 1] === undefined || argv[i + 1].startsWith('--') ? true : argv[i + 1]; };

function loadKey() {
  if (process.env.OPENROUTER_API_KEY) return process.env.OPENROUTER_API_KEY.trim();
  const file = join(homedir(), '.config', 'off42', 'openrouter.key');
  if (existsSync(file)) return readFileSync(file, 'utf8').trim();
  return null;
}
const KEY = loadKey();
if (!KEY) { console.error('No key: set OPENROUTER_API_KEY or create ~/.config/off42/openrouter.key (mode 600).'); process.exit(2); }
const redact = (s) => String(s).split(KEY).join('[REDACTED]');

const ENDPOINT = 'https://openrouter.ai/api/v1/chat/completions';
const MAX_TURNS = 8;
const TEMPERATURE = Number(opt('temperature', 0.2));
const MAX_TOKENS = Number(opt('max-tokens', 2048));
const BUDGET = Number(opt('budget', 2));
const REASONING = opt('reasoning', 'default');
const DATA = opt('data-collection', 'deny');
const label = opt('label', new Date().toISOString().slice(0, 16).replace(/[:T]/g, '-'));
const outDir = join(here, 'runs', label);
mkdirSync(outDir, { recursive: true });

const config = JSON.parse(readFileSync(join(here, 'models.json'), 'utf8')).models;
const wantModels = opt('models', null);
let models = wantModels ? config.filter((m) => wantModels.split(',').includes(m.id)) : config;
// --provider-order a,b pins every selected model to those providers (to exercise fallback providers).
if (opt('provider-order', null)) models = models.map((m) => ({ ...m, provider: { order: opt('provider-order').split(','), allow_fallbacks: false } }));
const allTasks = [...documentTasks, ...spreadsheetTasks];
const wantTasks = opt('tasks', null);
const picked = wantTasks ? allTasks.filter((t) => wantTasks.split(',').some((w) => t.id.startsWith(w))) : allTasks;
// --repeat-each N runs every task N times back to back (cache measurements).
const tasks = picked.flatMap((t) => Array(Number(opt('repeat-each', 1))).fill(t));
// --cache-control off drops explicit breakpoints even for models configured with them.
const CACHE_CONTROL = opt('cache-control', 'config');

let spent = 0;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function body(model, messages, kind, dataCollection) {
  const b = {
    model: model.id,
    messages,
    tools: TOOLS[kind],
    tool_choice: 'auto',
    temperature: TEMPERATURE,
    max_tokens: MAX_TOKENS,
    provider: { ...model.provider, data_collection: dataCollection, require_parameters: true },
    usage: { include: true },
  };
  if (REASONING === 'off') b.reasoning = { enabled: false };
  else if (REASONING === 'low') b.reasoning = { effort: 'low' };
  return b;
}

async function post(payload) {
  for (let attempt = 0; ; attempt++) {
    const t0 = performance.now();
    let res, text;
    try {
      res = await fetch(ENDPOINT, {
        method: 'POST',
        headers: { Authorization: `Bearer ${KEY}`, 'Content-Type': 'application/json', 'X-Title': 'OFF42 assistant eval' },
        body: JSON.stringify(payload),
        signal: AbortSignal.timeout(120000),
      });
      text = await res.text();
    } catch (e) {
      if (attempt < 2) { await sleep(3000 * (attempt + 1)); continue; }
      return { error: redact(e.message), ms: performance.now() - t0 };
    }
    const ms = performance.now() - t0;
    let json = null;
    try { json = JSON.parse(text); } catch {}
    const err = !res.ok ? `HTTP ${res.status}: ${redact((json && json.error && json.error.message) || text.slice(0, 300))}` : json && json.error ? redact(json.error.message || JSON.stringify(json.error)) : null;
    if (err && (res.status === 429 || res.status >= 500 || /rate|overload|timeout|temporar/i.test(err)) && attempt < 3) { await sleep(8000 * 2 ** attempt); continue; }
    if (err) return { error: err, status: res.status, ms };
    return { json, ms };
  }
}

function addUsage(acc, u) {
  if (!u) return;
  acc.prompt += u.prompt_tokens || 0;
  acc.completion += u.completion_tokens || 0;
  acc.cached += u.prompt_tokens_details?.cached_tokens || 0;
  acc.cacheWrite += u.prompt_tokens_details?.cache_write_tokens || 0;
  acc.reasoning += u.completion_tokens_details?.reasoning_tokens || 0;
  acc.cost += u.cost || 0;
}

async function runTask(model, task, dataCollection) {
  const initial = task.make();
  const state = task.make();
  const messages = initialMessages(state, task.prompt, CACHE_CONTROL === 'off' ? false : !!model.cacheControl);
  const rec = { model: model.id, task: task.id, lang: task.lang, adversarial: !!task.adversarial, turns: 0, calls: 0, validCalls: 0, toolErrors: 0, firstTryValid: true, requests: [], usage: { prompt: 0, completion: 0, cached: 0, cacheWrite: 0, reasoning: 0, cost: 0 }, errors: [], latencyMs: 0 };
  let reply = '';
  for (let turn = 0; turn < MAX_TURNS; turn++) {
    if (spent > BUDGET) { rec.errors.push('budget exhausted'); break; }
    const r = await post(body(model, messages, state.kind, dataCollection));
    rec.turns++;
    rec.latencyMs += r.ms;
    if (r.error) { rec.errors.push(r.error); break; }
    const u = r.json.usage;
    addUsage(rec.usage, u);
    spent += u?.cost || 0;
    const choice = r.json.choices?.[0];
    const msg = choice?.message || {};
    rec.requests.push({ ms: Math.round(r.ms), provider: r.json.provider, model: r.json.model, finish: choice?.finish_reason, prompt: u?.prompt_tokens, cached: u?.prompt_tokens_details?.cached_tokens || 0, cacheWrite: u?.prompt_tokens_details?.cache_write_tokens || 0, completion: u?.completion_tokens, cost: u?.cost });
    if (!choice) { rec.errors.push('no choices in response'); break; }
    const calls = msg.tool_calls || [];
    if (!calls.length) { reply = msg.content || ''; break; }
    const assistant = { role: 'assistant', content: msg.content || '', tool_calls: calls.map((c) => ({ id: c.id, type: 'function', function: { name: c.function?.name, arguments: c.function?.arguments ?? '' } })) };
    if (msg.reasoning_details) assistant.reasoning_details = msg.reasoning_details; // thinking models need it back
    messages.push(assistant);
    for (const c of calls) {
      const x = execute(state, c);
      rec.calls++;
      if (x.valid) rec.validCalls++; else { rec.firstTryValid = false; rec.errors.push(`invalid call ${c.function?.name}: ${x.error}`); }
      if (x.toolError) rec.toolErrors++;
      if (x.harnessError) rec.errors.push('harness: ' + x.harnessError.split('\n')[0]);
      messages.push({ role: 'tool', tool_call_id: c.id, content: x.content });
    }
    if (turn === MAX_TURNS - 1) rec.errors.push('turn limit reached');
  }
  const g = task.grade(state, reply, initial);
  rec.ok = !!g.ok && !rec.errors.some((e) => /^HTTP|budget|no choices|turn limit/.test(e));
  rec.why = g.ok ? undefined : g.why;
  rec.reply = reply.slice(0, 400);
  rec.transcript = messages.slice(2);
  return rec;
}

async function probe(model) {
  // One tiny request per data policy, to see whether the model is reachable when providers may not retain data.
  const res = {};
  for (const dc of ['deny', 'allow']) {
    const r = await post({ model: model.id, messages: [{ role: 'user', content: 'Reply with OK.' }], max_tokens: 16, provider: { ...model.provider, data_collection: dc }, usage: { include: true } });
    res[dc] = r.error ? `unavailable: ${r.error.slice(0, 200)}` : `ok (${r.json.provider})`;
    if (!r.error) spent += r.json.usage?.cost || 0;
  }
  return res;
}

async function runModel(model) {
  const file = join(outDir, model.id.replace(/[/:]/g, '_') + '.jsonl');
  const probeRes = await probe(model);
  appendFileSync(file, JSON.stringify({ probe: probeRes, model: model.id }) + '\n');
  console.log(`${model.id}: data_collection deny -> ${probeRes.deny}; allow -> ${probeRes.allow}`);
  if (opt('probe', false)) return;
  // Free endpoints that retain prompts are evaluated with "allow" on synthetic fixtures only.
  const dc = probeRes.deny.startsWith('ok') ? DATA : model.free ? 'allow' : DATA;
  if (dc === 'deny' && !probeRes.deny.startsWith('ok')) { console.log(`${model.id}: skipped (unavailable under deny)`); return; }
  let done = 0, okCount = 0, hardErrors = 0;
  for (const task of tasks) {
    const rec = await runTask(model, task, dc);
    rec.dataCollection = dc;
    appendFileSync(file, JSON.stringify(rec) + '\n');
    done++; if (rec.ok) okCount++;
    if (rec.errors.some((e) => /^HTTP|no choices/.test(e))) hardErrors++;
    console.log(`${model.id} ${task.id}: ${rec.ok ? 'ok' : 'FAIL ' + (rec.why || rec.errors[0] || '')} turns=${rec.turns} $${rec.usage.cost.toFixed(5)} total $${spent.toFixed(4)}`);
    if (done >= 10 && okCount / done < 0.3) { console.log(`${model.id}: stopping early (${okCount}/${done})`); break; }
    if (hardErrors >= 5) { console.log(`${model.id}: stopping, repeated API errors`); break; }
    if (spent > BUDGET) { console.log('budget exhausted'); break; }
  }
}

await Promise.all(models.map((m) => runModel(m).catch((e) => console.log(`${m.id}: crashed ${redact(e.message)}`))));
writeFileSync(join(outDir, 'meta.json'), JSON.stringify({ label, temperature: TEMPERATURE, max_tokens: MAX_TOKENS, reasoning: REASONING, data_collection: DATA, cache_control: CACHE_CONTROL, tasks: tasks.map((t) => t.id), spent }, null, 1));
console.log(`spent $${spent.toFixed(4)}`);
