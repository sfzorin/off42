// An opt-in, billed evaluation of the web assistant. Only generated files leave this process.
// Run from website: node test/large-assistant.mjs [--live]. Without --live exercise the oracle and Undo offline.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, mkdtempSync, rmSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as engine from '../../web/src/wasm/off42_wasm.js';
import { contextCommand, toolCommand, toolResult, readTools } from '../../web/src/assistant-context.ts';

const root = fileURLToPath(new URL('../', import.meta.url));
const live = process.argv.includes('--live');
engine.initSync({ module: readFileSync(join(root, '../web/src/wasm/off42_wasm_bg.wasm')) });
const apply = (id, command) => engine.editApply(id, JSON.stringify(command));
function fixture(text, from, to) {
  const id = engine.editOpen(new TextEncoder().encode(text), from);
  try {
    return engine.editSave(id, to, true);
  } finally {
    engine.editClose(id);
  }
}
const orderCode = (row) => 'Заказ-' + String((331 * row + 17) % 1201).padStart(4, '0');
const doc = fixture(
  Array.from({ length: 1200 }, (_, i) => {
    const code = 'Договор-' + String(i + 1).padStart(4, '0');
    if (i === 850)
      return (
        '## Раздел 18\n\n' +
        code +
        '. ' +
        'Проверка комплектации. '.repeat(160) +
        'Маркер окна: Янтарь. Поставка по графику.'
      );
    return (
      (i % 50 === 0 ? '## Раздел ' + (i / 50 + 1) + '\n\n' : '') +
      code +
      '. Поставка по графику. Статус: ожидает. Ответственный проверяет объём и дату отгрузки.'
    );
  }).join('\n\n'),
  'md',
  'docx',
);
const sheet = fixture(
  Array.from({ length: 1200 }, (_, i) => {
    const row = Array.from({ length: 240 }, (_, c) =>
      i === 0 ? 'Статья ' + (c + 1) : c === 0 ? orderCode(i + 1) : c < 10 || c === 239 ? String(i + c) : '',
    );
    if (i === 804) row[30] = 'Приложение. '.repeat(280) + 'Маркер окна: Кобальт.';
    return row.join(',');
  }).join('\n'),
  'csv',
  'xlsx',
);
const hash = (data) => createHash('sha256').update(data).digest('hex');
function fingerprint(session, kind) {
  const snap = JSON.parse(engine.editSnapshot(session, '{"maxRows":1048576,"maxCols":16384}'));
  if (kind === 'document') return hash(JSON.stringify(snap.blocks));
  assert.equal(snap.sheet.truncated, false);
  return hash(JSON.stringify(snap.sheet));
}
const cases = [];
for (const n of [7, 603, 1198]) {
  const label = 'Договор-' + String(n).padStart(4, '0');
  cases.push({
    kind: 'document',
    label,
    prompt: 'Только в абзаце с меткой «' + label + '» замени слово «ожидает» на «принято». Остальное сохрани.',
    oracle(id) {
      const matches = JSON.parse(apply(id, { command: 'find', text: label })).matches;
      assert.equal(matches.length, 1);
      return { command: 'replace_text', id: matches[0].id, find: 'ожидает', replace: 'принято' };
    },
  });
}
for (const n of [4, 607, 1198]) {
  const label = orderCode(n);
  cases.push({
    kind: 'spreadsheet',
    label,
    prompt:
      'Найди в столбце A строку «' +
      label +
      '». Только в столбце IF этой строки запиши формулу суммы ячеек B и C этой же строки с помощью SUM по диапазону.',
    oracle() {
      return { command: 'set_cells', sheet: 0, range: 'IF' + n, values: [['=SUM(B' + n + ':C' + n + ')']] };
    },
  });
}
// A caret deep inside a long paragraph and a cell far from A1. Their previews omit the marker.
cases.push({
  kind: 'document',
  label: 'long-selection',
  selection(id) {
    const match = JSON.parse(apply(id, { command: 'find', text: 'Договор-0851' })).matches[0];
    const point = { id: match.id, offset: 3050 };
    return { start: point, end: point };
  },
  prompt:
    'В выбранном абзаце найди метку «Маркер окна» и замени следующее за ней название цвета на «Серебро». Остальной текст сохрани.',
  oracle(id) {
    const p = JSON.parse(apply(id, { command: 'find', text: 'Маркер окна' })).matches[0];
    return { command: 'replace_text', id: p.id, find: 'Янтарь', replace: 'Серебро' };
  },
});
cases.push({
  kind: 'spreadsheet',
  label: 'long-cell',
  selection: () => 'AE805',
  prompt:
    'В конце выбранной ячейки после слов «Маркер окна» записано одно слово. Скопируй только это слово без точки в следующую ячейку справа. Сохрани исходную ячейку.',
  oracle: () => ({ command: 'set_cells', sheet: 0, range: 'AF805', values: [['Кобальт']] }),
});
cases.push({
  kind: 'spreadsheet',
  label: 'sum',
  prompt: 'Какова сумма значений B2:B1200? Ответь одним числом. Не меняй таблицу.',
  answer: String((1200 * 1201) / 2 - 1),
});
let mf, dir, metric;
const body = { id: crypto.randomUUID(), session: crypto.randomUUID(), challenge: 'fixture' };
async function startWorker() {
  const { Miniflare, convertV4MiniflareOptions } = await import('miniflare');
  const key =
    process.env.OPENROUTER_API_KEY ||
    readFileSync(join(homedir(), '.config/off42/openrouter.key'), 'utf8').trim();
  dir = mkdtempSync(join(tmpdir(), 'off42-context-'));
  execFileSync(
    process.execPath,
    [join(root, 'node_modules/wrangler/bin/wrangler.js'), 'deploy', '--dry-run', '--outdir', dir],
    { cwd: root, stdio: 'pipe' },
  );
  mf = new Miniflare(
    convertV4MiniflareOptions({
      workers: [
        {
          name: 'off42',
          modulesRoot: dir,
          modules: [
            { type: 'ESModule', path: join(dir, 'index.js') },
            {
              type: 'Text',
              path: join(
                dir,
                readdirSync(dir).find((f) => f.endsWith('-system.md')),
              ),
            },
          ],
          compatibilityDate: '2026-09-30',
          durableObjects: { ASSISTANT_BUDGET: { className: 'AssistantBudget', useSQLite: true } },
          bindings: { OPENROUTER_API_KEY: key, TURNSTILE_SECRET: 'fixture', ASSISTANT_SITE_KEY: 'fixture' },
          outboundService: async (request) => {
            if (request.url === 'https://challenges.cloudflare.com/turnstile/v0/siteverify')
              return Response.json({ success: true, hostname: 'off42.test', action: 'off42_assistant' });
            assert.equal(request.url, 'https://openrouter.ai/api/v1/chat/completions');
            const res = await fetch(request.url, {
              method: 'POST',
              headers: {
                Authorization: request.headers.get('Authorization'),
                'Content-Type': 'application/json',
              },
              body: await request.text(),
            });
            if (res.ok) {
              const { usage, model } = await res.clone().json();
              assert.ok(Number.isFinite(usage?.cost));
              metric.charges.push({
                model,
                usd: usage.cost,
                input: usage.prompt_tokens,
                output: usage.completion_tokens,
              });
            } else metric.httpErrors.push(res.status);
            return res;
          },
        },
      ],
    }),
  );
}
async function completion(id, kind, prompt, context) {
  let request = { ...body, kind, prompt, context };
  const writes = [];
  for (let turn = 0; turn < 8; turn++) {
    const res = await mf.dispatchFetch('https://off42.test/api/assistant', {
      method: 'POST',
      headers: {
        Origin: 'https://off42.test',
        'CF-Connecting-IP': '192.0.2.1',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(request),
    });
    const reply = await res.json();
    assert.equal(res.status, 200, reply.error);
    if (!reply.commands) {
      assert.ok(!reply.refused && reply.text);
      return { writes, answer: reply.text };
    }
    const results = [];
    for (const { id: callId, name, args } of reply.commands) {
      const cmd = toolCommand(name, args, kind === 'spreadsheet' ? 0 : undefined);
      metric.tools.push(name);
      let content;
      let applied = false;
      try {
        content = apply(id, cmd);
        if (!readTools.has(name)) {
          writes.push(cmd);
          applied = true;
        }
      } catch (error) {
        content = JSON.stringify({ error: String(error) });
      }
      results.push({ id: callId, content: toolResult(content, reply.commands.length, applied) });
    }
    request = { continuation: reply.continuation, results };
  }
  throw Error('No final reply in eight turns');
}
let failures = 0;
try {
  if (live) await startWorker();
  for (const task of cases) {
    const { kind, label } = task;
    const ext = kind === 'document' ? 'docx' : 'xlsx';
    const file = kind === 'document' ? doc : sheet;
    const id = engine.editOpen(file, ext);
    const oracle = engine.editFork(id);
    const draft = engine.editFork(id);
    metric = { kind, label, charges: [], httpErrors: [], tools: [] };
    try {
      const before = fingerprint(id, kind);
      const pos = {
        id:
          kind === 'document'
            ? JSON.parse(apply(id, { command: 'get_outline', limit: 1 })).paragraphs[0].id
            : 0,
        offset: 0,
      };
      const selection = task.selection?.(id) ?? (kind === 'document' ? { start: pos, end: pos } : 'A1');
      const context = apply(draft, contextCommand(selection, kind === 'spreadsheet' ? 0 : undefined));
      metric.contextBytes = Buffer.byteLength(context);
      assert.ok(metric.contextBytes < 8192);
      assert.ok(!context.includes(label) || label === 'sum', 'target should require another read');
      if (task.selection) assert.ok(!/Янтарь|Кобальт/.test(context));
      const info = JSON.parse(context);
      if (kind === 'document') assert.equal(info.block_count, 1224);
      else assert.equal(info.used_range, 'A1:IF1200');
      const expected = task.oracle?.(oracle);
      if (expected) apply(oracle, expected);
      const outcome = live
        ? await completion(draft, kind, task.prompt, context)
        : { writes: expected ? [expected] : [], answer: task.answer };
      if (task.answer) {
        assert.equal(outcome.writes.length, 0);
        assert.equal(outcome.answer.trim().replace(/[ \u00a0\u202f]/g, ''), task.answer);
      } else {
        assert.ok(outcome.writes.length);
        apply(id, outcome.writes);
      }
      assert.equal(fingerprint(id, kind), fingerprint(oracle, kind), 'unexpected changes in the open file');
      const actualFile = engine.editSave(id, ext, false);
      const expectedFile = engine.editSave(oracle, ext, false);
      const reopened = engine.editOpen(actualFile, ext),
        reference = engine.editOpen(expectedFile, ext);
      try {
        assert.equal(
          fingerprint(reopened, kind),
          fingerprint(reference, kind),
          'unexpected changes in the reopened file',
        );
      } finally {
        engine.editClose(reopened);
        engine.editClose(reference);
      }
      if (expected) {
        assert.ok(JSON.parse(engine.editUndo(id)).changed);
        assert.equal(fingerprint(id, kind), before, 'Undo did not restore the whole file');
      }
      metric.passed = true;
    } catch (error) {
      metric.passed = false;
      metric.error = String(error);
      failures++;
    } finally {
      for (const handle of [id, draft, oracle]) engine.editClose(handle);
    }
    metric.usd = metric.charges.reduce((n, c) => n + c.usd, 0);
    console.log(JSON.stringify(metric));
  }
} finally {
  await mf?.dispose();
  if (dir) rmSync(dir, { recursive: true, force: true });
}
process.exitCode = failures ? 1 : 0;
