// A static server that mimics how off42.cc serves the web app: the build is mounted at /app/ and every response
// carries the headers that website/public/_headers assigns to its path (Cloudflare semantics: all matching rules
// apply in file order, a repeated header is joined with ", ", "! Name" removes what earlier rules added). The
// site's Content-Security-Policy header is therefore enforced together with the policy in the page's own <meta>.
import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { join, normalize, extname } from 'node:path';

const TYPES = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm',
  '.json': 'application/json', '.txt': 'text/plain; charset=utf-8', '.svg': 'image/svg+xml', '.png': 'image/png',
};

export function parseHeaders(text) {
  const rules = [];
  for (const line of text.split('\n')) {
    if (!line.trim() || line.trim().startsWith('#')) continue;
    if (!/^\s/.test(line)) {
      const pattern = new RegExp('^' + line.trim().replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*') + '$');
      rules.push({ pattern, lines: [] });
    } else if (line.trim().startsWith('!')) {
      rules.at(-1).lines.push({ detach: line.trim().slice(1).trim().toLowerCase() });
    } else {
      const at = line.indexOf(':');
      rules.at(-1).lines.push({ name: line.slice(0, at).trim().toLowerCase(), value: line.slice(at + 1).trim() });
    }
  }
  // Rules apply in file order; "! Name" removes only what earlier rules (or lines) have added.
  return (path) => {
    const headers = new Map();
    for (const rule of rules.filter((r) => r.pattern.test(path))) {
      for (const line of rule.lines) {
        if (line.detach) headers.delete(line.detach);
        else headers.set(line.name, headers.has(line.name) ? `${headers.get(line.name)}, ${line.value}` : line.value);
      }
    }
    return headers;
  };
}

/** Serves `dir` at /app/ with the headers from `headersFile`; resolves to the server's origin. */
export async function serveApp(dir, headersFile) {
  const headersFor = parseHeaders(await readFile(headersFile, 'utf8'));
  const server = createServer(async (request, response) => {
    const path = decodeURIComponent(new URL(request.url, 'http://x').pathname);
    if (!path.startsWith('/app/')) {
      response.writeHead(path === '/app' ? 307 : 404, path === '/app' ? { Location: '/app/' } : {});
      return response.end();
    }
    let file = normalize(join(dir, path.slice('/app/'.length)));
    if (!file.startsWith(normalize(dir))) return response.writeHead(403).end();
    try {
      if ((await stat(file)).isDirectory()) file = join(file, 'index.html');
      const body = await readFile(file);
      const headers = Object.fromEntries(headersFor(path));
      headers['content-type'] = TYPES[extname(file)] ?? 'application/octet-stream';
      response.writeHead(200, headers);
      response.end(body);
    } catch {
      response.writeHead(404).end();
    }
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return { origin: `http://127.0.0.1:${server.address().port}`, close: () => new Promise((r) => server.close(r)) };
}
