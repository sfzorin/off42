// The OFF42 engine (crates/off42-wasm) runs in this module worker, so parsing and converting large files never
// blocks the page. The wasm module is fetched as a stream to report download progress, then compiled while it
// arrives. The page first sends the module's URL (`{wasm}`); then requests are `{id, name, args}` and replies
// `{id, result}` or `{id, error}`.
import init, * as off42 from './wasm/off42_wasm.js';

declare const __WASM_SIZE__: number;

type Request = { id: number; name: string; args: unknown[] };
const post = (message: unknown, transfer: Transferable[] = []) =>
  (self as unknown as Worker).postMessage(message, transfer);

async function load(wasmUrl: string) {
  const response = await fetch(wasmUrl);
  if (!response.ok || !response.body) throw new Error(`HTTP ${response.status}`);
  // Bytes are counted after decompression, so the total is the size of the uncompressed module, known at build
  // time; an uncompressed response's Content-Length is the same number.
  const total = Number(response.headers.get('content-encoding') ? 0 : response.headers.get('content-length'))
    || __WASM_SIZE__;
  let loaded = 0;
  let reported = 0;
  const counted = response.body.pipeThrough(new TransformStream<Uint8Array, Uint8Array>({
    transform(chunk, controller) {
      loaded += chunk.byteLength;
      const now = performance.now();
      if (now - reported > 50) {
        reported = now;
        post({ progress: { loaded, total } });
      }
      controller.enqueue(chunk);
    },
  }));
  await init({ module_or_path: new Response(counted, { headers: { 'content-type': 'application/wasm' } }) });
  post({ progress: { loaded: total, total } });
}

let ready: Promise<void> | undefined;

const exports = off42 as unknown as Record<string, (...args: unknown[]) => unknown>;

self.addEventListener('message', async (event: MessageEvent<Request | { wasm: string }>) => {
  if ('wasm' in event.data) {
    ready ??= load(event.data.wasm).then(
      () => post({ ready: off42.version() }),
      (error) => post({ failed: error instanceof Error ? error.message : String(error) }),
    );
    return;
  }
  const { id, name, args } = event.data;
  await ready;
  try {
    const fn = exports[name];
    if (typeof fn !== 'function' || name === 'default' || name === 'initSync') throw new Error(`Unknown operation ${name}`);
    const result = fn(...args);
    post({ id, result }, result instanceof Uint8Array ? [result.buffer] : []);
  } catch (error) {
    post({ id, error: error instanceof Error ? error.message : String(error) });
  }
});
