// Offline check, no network: every oracle solution passes its grader through the real executor and schemas,
// and the untouched fixture fails (except where "no change" is the right answer and the reply is graded).
import { documentTasks } from './tasks/documents.mjs';
import { spreadsheetTasks } from './tasks/spreadsheets.mjs';
import { scopeTasks } from './tasks/scope.mjs';
import { REFUSAL } from './lib/scope.mjs';
import { execute, TOOLS, initialMessages } from './lib/harness.mjs';

let failures = 0;
// A harmless change to a fixture, to prove the graders notice edits.
const changed = (f) => { if (f.kind === 'document') f.paragraphs[0].style = 'heading3'; else f.cells.set('Z99', { input: 'x' }); return f; };
const tasks = [...documentTasks, ...spreadsheetTasks, ...scopeTasks];
const ids = new Set();
for (const t of tasks) {
  if (ids.has(t.id)) { console.log(`duplicate id ${t.id}`); failures++; }
  ids.add(t.id);
  const initial = t.make();
  const state = t.make();
  for (const [name, args] of t.oracle) {
    const r = execute(state, { function: { name, arguments: JSON.stringify(args) } });
    if (!r.valid || r.toolError || r.harnessError) { console.log(`FAIL ${t.id}: oracle call ${name} -> ${r.error || r.toolError || r.harnessError}`); failures++; }
  }
  const good = t.grade(state, t.oracleReply || 'Done.', initial, { calls: t.oracle.length });
  const untouched = t.grade(t.make(), 'Done.', initial, { calls: 0 });
  if (!good.ok) { console.log(`FAIL ${t.id}: oracle rejected: ${good.why}`); failures++; }
  if (untouched.ok) { console.log(`FAIL ${t.id}: untouched file accepted`); failures++; }
  // Scope tasks: wrong behaviour must be rejected as well.
  const reject = (label, f, reply, m) => { if (t.grade(f, reply, initial, m).ok) { console.log(`FAIL ${t.id}: accepted ${label}`); failures++; } };
  if (t.wrongReply) reject('the wrong reply', t.make(), t.wrongReply, { calls: 0 });
  if (t.scope === 'out') {
    reject('a refusal after a tool call', t.make(), t.oracleReply, { calls: 1 });
    reject('a refusal in another language', t.make(), t.lang === 'en' ? REFUSAL.ru : REFUSAL.en, { calls: 0 });
    reject('a refusal that names the model', t.make(), t.oracleReply + ' (DeepSeek)', { calls: 0 });
    reject('a refusal followed by an answer', t.make(), t.oracleReply + ' ' + 'x'.repeat(200), { calls: 0 });
    reject('a refusal with a changed file', changed(t.make()), t.oracleReply, { calls: 0 });
  }
  if (t.scope === 'in') {
    reject('a correct answer with a changed file', changed(t.make()), t.oracleReply, { calls: 0 });
    reject('an answer that names the model', t.make(), t.oracleReply + ' I am Claude.', { calls: 0 });
  }
  initialMessages(state, t.prompt, false);
}
for (const [kind, defs] of Object.entries(TOOLS)) {
  for (const d of defs) {
    const p = d.function.parameters;
    if (p.additionalProperties !== false) { console.log(`FAIL ${kind}.${d.function.name}: additionalProperties`); failures++; }
  }
}
console.log(`${tasks.length} tasks (${documentTasks.length} document, ${spreadsheetTasks.length} spreadsheet, ${scopeTasks.length} scope), ${failures} failures`);
process.exit(failures ? 1 : 0);
