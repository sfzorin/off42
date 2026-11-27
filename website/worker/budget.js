// One SQLite-backed object per UTC day. Amounts are integer millionths of a US dollar.
import { DurableObject } from 'cloudflare:workers';

export class AssistantBudget extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    this.sql.exec('CREATE TABLE IF NOT EXISTS counters (id TEXT PRIMARY KEY, value INTEGER NOT NULL)');
    this.sql.exec('CREATE TABLE IF NOT EXISTS attempts (id TEXT PRIMARY KEY, user TEXT, ip TEXT, amount INTEGER)');
    this.sql.exec('CREATE TABLE IF NOT EXISTS consumed (id TEXT PRIMARY KEY)');
  }

  count(id) { return this.sql.exec('SELECT value FROM counters WHERE id=?', id).toArray()[0]?.value ?? 0; }
  add(id, value) {
    this.sql.exec('INSERT INTO counters VALUES (?,?) ON CONFLICT(id) DO UPDATE SET value=value+excluded.value', id, value);
  }
  async reserve({ user, ip, session, nonce, first, amount, day }) {
    if (day !== new Date().toISOString().slice(0,10)) return 'expired';
    if (await this.ctx.storage.getAlarm() === null) await this.ctx.storage.setAlarm(Date.parse(day+'T00:00:00Z')+2*86400000);
    return this.ctx.storage.transactionSync(() => {
      if (this.sql.exec('SELECT 1 FROM consumed WHERE id=?',nonce).toArray().length) return 'expired';
      if (first && this.count(session) >= 30) return 'session';
      const siteLimit = Math.floor(Number(this.env.ASSISTANT_DAILY_USD ?? 5)*1e6);
      if (!Number.isSafeInteger(siteLimit) || this.count(user)+amount > 100000 || this.count(ip)+amount > 500000 || this.count('site')+amount > siteLimit) return 'limit';
      this.sql.exec('INSERT INTO consumed VALUES (?)',nonce);
      this.sql.exec('INSERT INTO attempts VALUES (?,?,?,?)',nonce,user,ip,amount);
      if (first) this.add(session,1);
      for (const id of [user,ip,'site']) this.add(id,amount);
      return 'ok';
    });
  }

  settle(nonce, cost) {
    if (!Number.isSafeInteger(cost) || cost < 0) return;
    this.ctx.storage.transactionSync(() => {
      const entry = this.sql.exec('SELECT * FROM attempts WHERE id=?',nonce).toArray()[0];
      if (!entry) return;
      for (const id of [entry.user,entry.ip,'site']) this.add(id,cost-entry.amount);
      this.sql.exec('DELETE FROM attempts WHERE id=?',nonce);
    });
  }
  refused() { this.add('filtered',1); }
  async alarm() { await this.ctx.storage.deleteAll(); }
}
