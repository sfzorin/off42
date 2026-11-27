import { assistant } from './assistant.js';
export { AssistantBudget } from './budget.js';
// Serves byte ranges for the demo videos. Workers Static Assets answer every request with the whole file
// (HTTP 200), and Safari only plays video that answers Range requests with 206 Partial Content.
// The assistant has its own handler. Static assets serve the remaining site routes.
export default {
  async fetch(request, env) {
    if (new URL(request.url).pathname === '/api/assistant') return assistant(request, env);
    const range = request.headers.get('Range');
    const headers = new Headers(request.headers); headers.delete('Range');
    const response = await env.ASSETS.fetch(new Request(request.url, { method: request.method === 'HEAD' ? 'HEAD' : 'GET', headers }));
    const out = new Headers(response.headers); out.set('Accept-Ranges', 'bytes');
    const match = range && /^bytes=(\d*)-(\d*)$/.exec(range.trim());
    if (!response.ok || request.method === 'HEAD' || !match || (!match[1] && !match[2])) {
      return new Response(response.body, { status: response.status, headers: out });
    }
    const body = await response.arrayBuffer(), size = body.byteLength;
    let start = match[1] ? Number(match[1]) : Math.max(0, size - Number(match[2]));
    let end = match[1] && match[2] ? Math.min(Number(match[2]), size - 1) : size - 1;
    if (start > end || start >= size) {
      out.set('Content-Range', `bytes */${size}`); out.delete('Content-Length');
      return new Response(null, { status: 416, headers: out });
    }
    out.set('Content-Range', `bytes ${start}-${end}/${size}`); out.set('Content-Length', String(end - start + 1));
    return new Response(body.slice(start, end + 1), { status: 206, headers: out });
  },
};
