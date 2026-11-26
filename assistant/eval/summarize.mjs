#!/usr/bin/env node
// node summarize.mjs <label> [<label> ...]
// Prints markdown tables and writes results/<label>.json: per-task records without transcripts or replies.
import { readFileSync, readdirSync, writeFileSync, mkdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const labels = process.argv.slice(2);
const median = (a) => { if (!a.length) return NaN; const s = [...a].sort((x, y) => x - y); const m = s.length >> 1; return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2; };
const pct = (a, b) => (b ? Math.round((100 * a) / b) + '%' : '–');

mkdirSync(join(here, 'results'), { recursive: true });
for (const label of labels) {
  const dir = join(here, 'runs', label);
  const byModel = {};
  const probes = {};
  for (const f of readdirSync(dir).filter((x) => x.endsWith('.jsonl'))) {
    for (const line of readFileSync(join(dir, f), 'utf8').split('\n').filter(Boolean)) {
      const r = JSON.parse(line);
      if (r.probe) { probes[r.model] = r.probe; continue; }
      delete r.transcript; delete r.reply;
      (byModel[r.model] ||= []).push(r);
    }
  }
  writeFileSync(join(here, 'results', label + '.json'), JSON.stringify({ label, probes, records: byModel }));
  console.log(`\n### Run ${label}\n`);
  console.log('| Model | Tasks | Success | Docs | Sheets | Adversarial | Valid tool calls | Tasks all-valid | Median turns | Median latency/task | Mean cost/task | Cache hit | Reasoning tok/task |');
  console.log('|---|---|---|---|---|---|---|---|---|---|---|---|---|');
  const rows = Object.entries(byModel).map(([m, rs]) => {
    const n = rs.length, okN = rs.filter((r) => r.ok).length;
    const cat = (p) => { const x = rs.filter(p); return `${x.filter((r) => r.ok).length}/${x.length}`; };
    const calls = rs.reduce((a, r) => a + r.calls, 0), valid = rs.reduce((a, r) => a + r.validCalls, 0);
    const prompt = rs.reduce((a, r) => a + r.usage.prompt, 0), cached = rs.reduce((a, r) => a + r.usage.cached, 0);
    const cost = rs.reduce((a, r) => a + r.usage.cost, 0);
    return { m, n, okN, line: `| ${m} | ${n} | **${pct(okN, n)}** | ${cat((r) => !r.adversarial && !r.task.startsWith('xls'))} | ${cat((r) => !r.adversarial && r.task.startsWith('xls'))} | ${cat((r) => r.adversarial)} | ${pct(valid, calls)} (${valid}/${calls}) | ${pct(rs.filter((r) => r.firstTryValid).length, n)} | ${median(rs.map((r) => r.turns))} | ${(median(rs.map((r) => r.latencyMs)) / 1000).toFixed(1)} s | $${(cost / n).toFixed(5)} | ${pct(cached, prompt)} | ${Math.round(rs.reduce((a, r) => a + r.usage.reasoning, 0) / n)} |` };
  });
  rows.sort((a, b) => b.okN / b.n - a.okN / a.n);
  for (const r of rows) console.log(r.line);
  console.log('\nFailures:\n');
  for (const [m, rs] of Object.entries(byModel)) {
    const bad = rs.filter((r) => !r.ok);
    if (bad.length) console.log(`- **${m}**: ` + bad.map((r) => `${r.task} (${(r.why || r.errors[0] || '').slice(0, 120)})`).join('; '));
  }
  console.log('\nCaching (cached / prompt tokens): first request of each task vs follow-up requests within a task; providers seen:\n');
  console.log('| Model | First request | Follow-ups | Cache writes | Providers |');
  console.log('|---|---|---|---|---|');
  for (const [m, rs] of Object.entries(byModel)) {
    const reqs = rs.flatMap((r) => r.requests.map((q, i) => ({ ...q, i })));
    const first = reqs.filter((q) => q.i === 0 && q.prompt), later = reqs.filter((q) => q.i > 0 && q.prompt);
    const sum = (a, k) => a.reduce((x, q) => x + (q[k] || 0), 0);
    const prov = {}; for (const q of reqs) prov[q.provider] = (prov[q.provider] || 0) + 1;
    console.log(`| ${m} | ${pct(sum(first, 'cached'), sum(first, 'prompt'))} | ${pct(sum(later, 'cached'), sum(later, 'prompt'))} | ${sum(reqs, 'cacheWrite')} | ${Object.entries(prov).map(([p, c]) => `${p} ${c}`).join(', ')} |`);
  }
  // With --repeat-each 2: the same task again right away. Its first request repeats system+tools+snapshot+prompt.
  const repeated = Object.values(byModel).some((rs) => new Set(rs.map((r) => r.task)).size < rs.length);
  if (repeated) {
    console.log('\nSame task twice (first request of each run: cached / prompt tokens; whole task cost):\n');
    console.log('| Model | 1st run, 1st request | 2nd run, 1st request | All requests | Cost 1st run | Cost 2nd run |');
    console.log('|---|---|---|---|---|---|');
    for (const [m, rs] of Object.entries(byModel)) {
      const seen = {}; const a = [], b = [];
      for (const r of rs) { (seen[r.task] ? b : a).push(r); seen[r.task] = true; }
      const q = (xs) => { const p = xs.reduce((s, r) => s + (r.requests[0]?.prompt || 0), 0), c = xs.reduce((s, r) => s + (r.requests[0]?.cached || 0), 0); return pct(c, p); };
      const all = rs.flatMap((r) => r.requests);
      const cost = (xs) => '$' + xs.reduce((s, r) => s + r.usage.cost, 0).toFixed(5);
      console.log(`| ${m} | ${q(a)} | ${q(b)} | ${pct(all.reduce((s, x) => s + (x.cached || 0), 0), all.reduce((s, x) => s + (x.prompt || 0), 0))} | ${cost(a)} | ${cost(b)} |`);
    }
  }
  if (Object.keys(probes).length) {
    console.log('\nData policy probe:\n');
    for (const [m, p] of Object.entries(probes)) console.log(`- ${m}: deny → ${p.deny}; allow → ${p.allow}`);
  }
}
