import { Worker } from "node:worker_threads";

globalThis.__nimbusInvoke = function () {
  let worker = null;
  try {
    worker = new Worker("require('node:worker_threads').parentPort.postMessage('unexpected')", {
      eval: true,
    });
    worker.terminate();
    return {
      surface: "worker_threads",
      supportStatus: "service_microvm_required",
      diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
      denied: null,
    };
  } catch (error) {
    return {
      surface: "worker_threads",
      supportStatus: "service_microvm_required",
      diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
      denied: error?.message ?? String(error),
    };
  }
};

export {};
