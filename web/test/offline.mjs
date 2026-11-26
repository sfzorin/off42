// Checks the production build for the "nothing leaves the browser" promise: the page carries a Content
// Security Policy that limits fetches to its own origin, and the bundle references no remote URLs.
// Run after `npm run build`.
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import assert from 'node:assert/strict';

const dist = join(dirname(fileURLToPath(import.meta.url)), '..', 'dist');
const html = readFileSync(join(dist, 'index.html'), 'utf8');

const csp = html.match(/http-equiv="Content-Security-Policy" content="([^"]+)"/)?.[1];
assert.ok(csp, 'index.html has a Content-Security-Policy');
const directives = Object.fromEntries(csp.split(';').map((d) => d.trim().split(/\s+/)).map(([k, ...v]) => [k, v]));
assert.deepEqual(directives['connect-src'], ["'self'"]);
assert.deepEqual(directives['default-src'], ["'self'"]);
assert.ok(!directives['script-src'].some((s) => /^https?:|\*/.test(s)), 'scripts only from the app');

// The only URL allowed in the bundle is the example QR content shown in a form field.
const allowed = new Set(['https://off42.cc']);
const files = [join(dist, 'index.html'), ...readdirSync(join(dist, 'assets'))
  .filter((f) => /\.(js|css)$/.test(f)).map((f) => join(dist, 'assets', f))];
for (const file of files) {
  for (const url of readFileSync(file, 'utf8').match(/https?:\/\/[^"'`\s)<>]+/g) ?? []) {
    assert.ok(allowed.has(url) || url.startsWith('http://www.w3.org/'), `unexpected remote URL ${url} in ${file}`);
  }
}
assert.ok(readdirSync(join(dist, 'assets')).some((f) => f.endsWith('.wasm')), 'the engine is bundled as a local file');
console.log('ok   production build is self-contained (CSP connect-src \'self\', no remote URLs)');
