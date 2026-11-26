// Offline check, no network: every oracle solution passes its grader through the real executor and schemas,
// and the untouched fixture fails (except where "no change" is the right answer and the reply is graded).
import { documentTasks } from './tasks/documents.mjs';
import { spreadsheetTasks } from './tasks/spreadsheets.mjs';
import { execute, TOOLS, initialMessages } from './lib/harness.mjs';

let failures = 0;
const tasks = [...documentTasks, ...spreadsheetTasks];
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
  const good = t.grade(state, t.oracleReply || 'Done.', initial);
  const untouched = t.grade(t.make(), 'Done.', initial);
  if (!good.ok) { console.log(`FAIL ${t.id}: oracle rejected: ${good.why}`); failures++; }
  if (untouched.ok) { console.log(`FAIL ${t.id}: untouched file accepted`); failures++; }
  initialMessages(state, t.prompt, false);
}
for (const [kind, defs] of Object.entries(TOOLS)) {
  for (const d of defs) {
    const p = d.function.parameters;
    if (p.additionalProperties !== false) { console.log(`FAIL ${kind}.${d.function.name}: additionalProperties`); failures++; }
  }
}
console.log(`${tasks.length} tasks (${documentTasks.length} document, ${spreadsheetTasks.length} spreadsheet), ${failures} failures`);
process.exit(failures ? 1 : 0);
