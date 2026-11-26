// The page side of the engine worker (engine.worker.ts): every export of off42-wasm becomes an async call.
// Byte arrays are copied to the worker (the page keeps the open file for later conversions); byte results are
// transferred back.

import wasmUrl from './wasm/off42_wasm_bg.wasm?url';

type Pending = { resolve: (value: never) => void; reject: (error: Error) => void };
export type Progress = { loaded: number; total: number };

let worker: Worker;
let nextId = 1;
const pending = new Map<number, Pending>();
let started: Promise<string>;

function spawn(onProgress: (progress: Progress) => void): Promise<string> {
  worker = new Worker(new URL('./engine.worker.ts', import.meta.url), { type: 'module' });
  return new Promise((resolve, reject) => {
    worker.addEventListener('message', (event) => {
      const data = event.data;
      if (data.progress) onProgress(data.progress);
      else if (data.ready) resolve(data.ready);
      else if (data.failed) reject(new Error(data.failed));
      else {
        const call = pending.get(data.id);
        if (!call) return;
        pending.delete(data.id);
        if ('error' in data) call.reject(new Error(data.error));
        else call.resolve(data.result as never);
      }
    });
    worker.addEventListener('error', (event) => reject(new Error(event.message || 'worker error')));
    worker.postMessage({ wasm: new URL(wasmUrl, location.href).href });
  });
}

/** Starts the worker and resolves to the engine version once the wasm module is compiled. */
export function startEngine(onProgress: (progress: Progress) => void): Promise<string> {
  started = spawn(onProgress);
  return started;
}

/** Calls an export of off42-wasm in the worker. */
export async function call<T>(name: string, ...args: unknown[]): Promise<T> {
  await started;
  const id = nextId++;
  const result = new Promise<T>((resolve, reject) => {
    pending.set(id, { resolve: resolve as (value: never) => void, reject });
  });
  worker.postMessage({ id, name, args });
  try {
    return await result;
  } catch (error) {
    // The engine is built with panic=abort: after a trap its memory may be inconsistent, so start a fresh one.
    if (/unreachable|RuntimeError/i.test((error as Error).message)) {
      worker.terminate();
      for (const call of pending.values()) call.reject(new Error('engine restarted'));
      pending.clear();
      started = spawn(() => {});
    }
    throw error;
  }
}

export const json = async <T>(name: string, ...args: unknown[]): Promise<T> => JSON.parse(await call<string>(name, ...args));
