import { parentPort, workerData } from "node:worker_threads";

// HG0 (Band B-FIX, CAPTURE-ORDERING): __nimbusInvoke is installed via
// Object.defineProperty(configurable:false, writable:false), so once a
// generated bundle installs it on a realm's globalThis, that realm can never
// have it deleted or reinstalled — by a guest or by test code. This worker
// thread gives each import()+invoke batch its own fresh globalThis (a real
// worker realm, not the parent process's), mirroring a cold start, instead of
// relying on delete-then-reimport against one shared process global.
const { bundleUrl, requests, hostResponses } = workerData;
const hostCalls = [];

if (hostResponses) {
  globalThis.__nimbusAsyncHostValue = async (opName, payload) => {
    hostCalls.push({ opName, payload: JSON.parse(JSON.stringify(payload)) });
    const operation = payload && typeof payload === "object" ? payload.operation : undefined;
    if (
      opName === "op_nimbus_runtime_extension_call"
      && operation
      && Object.hasOwn(hostResponses, operation)
    ) {
      return hostResponses[operation];
    }
    throw new Error(`unexpected host op ${opName}`);
  };
}

const results = [];
try {
  await import(bundleUrl);
  for (const request of requests) {
    try {
      const value = await globalThis.__nimbusInvoke(request);
      results.push({ ok: true, value });
    } catch (error) {
      results.push({
        ok: false,
        error: { message: error?.message ?? String(error), stack: error?.stack ?? null },
      });
    }
  }
  parentPort.postMessage({ ok: true, results, hostCalls });
} catch (error) {
  parentPort.postMessage({
    ok: false,
    error: { message: error?.message ?? String(error), stack: error?.stack ?? null },
    results,
    hostCalls,
  });
}
