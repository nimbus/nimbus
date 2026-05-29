import { spawn } from "node:child_process";

function captureSpawn(command, args) {
  return new Promise((resolve) => {
    let child = null;
    const timeout = setTimeout(() => {
      try {
        child?.kill?.();
      } catch (_error) {
      }
      resolve("node --test timed out before returning a process result");
    }, 1000);
    try {
      child = spawn(command, args, { stdio: "pipe" });
      child.once("error", (error) => {
        clearTimeout(timeout);
        resolve(error?.message ?? String(error));
      });
      child.once("exit", (status, signal) => {
        clearTimeout(timeout);
        resolve(`node --test unexpectedly exited status=${status} signal=${signal}`);
      });
    } catch (error) {
      clearTimeout(timeout);
      resolve(error?.message ?? String(error));
    }
  });
}

globalThis.__nimbusInvoke = async function () {
  const denied = await captureSpawn(process.execPath, ["--test"]);
  return {
    surface: "node_test_runner",
    supportStatus: "service_microvm_required",
    diagnostic: "NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED",
    denied,
  };
};

export {};
