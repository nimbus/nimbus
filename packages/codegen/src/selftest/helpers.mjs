import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { Worker } from "node:worker_threads";

const cliPath = fileURLToPath(new URL("../cli.mjs", import.meta.url));
const cloudFunctionsBundleWorkerPath = fileURLToPath(
  new URL("./cloud_functions_bundle_worker.mjs", import.meta.url),
);

async function createAppFixture(files, { sourceDir = "convex", rootFiles = {} } = {}) {
  const appDir = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus_codegen_"));
  await fs.mkdir(path.join(appDir, sourceDir), { recursive: true });
  for (const [fileName, source] of Object.entries(rootFiles)) {
    const filePath = path.join(appDir, fileName);
    await fs.mkdir(path.dirname(filePath), { recursive: true });
    await fs.writeFile(filePath, source, "utf8");
  }
  for (const [fileName, source] of Object.entries(files)) {
    const filePath = path.join(appDir, sourceDir, fileName);
    await fs.mkdir(path.dirname(filePath), { recursive: true });
    await fs.writeFile(filePath, source, "utf8");
  }
  return appDir;
}

function runCli(appDir, extraArgs = []) {
  return spawnSync(process.execPath, [cliPath, "--app", appDir, ...extraArgs], {
    encoding: "utf8",
  });
}

async function readGeneratedFile(appDir, fileName, { sourceDir = "convex" } = {}) {
  return fs.readFile(path.join(appDir, sourceDir, "_generated", fileName), "utf8");
}

async function readConvexFile(appDir, fileName) {
  return fs.readFile(path.join(appDir, ".nimbus", "convex", fileName), "utf8");
}

async function readConvexJson(appDir, fileName) {
  return JSON.parse(await readConvexFile(appDir, fileName));
}

async function readCloudFunctionsFile(appDir, fileName) {
  return fs.readFile(path.join(appDir, ".nimbus", "firebase", fileName), "utf8");
}

async function readCloudFunctionsJson(appDir, fileName) {
  return JSON.parse(await readCloudFunctionsFile(appDir, fileName));
}

// HG0 (Band B-FIX, CAPTURE-ORDERING): __nimbusInvoke is installed via
// Object.defineProperty(configurable:false, writable:false), so a generated
// cloud-functions bundle can only ever install it once per realm — the same
// guarantee production guests are held to. Import the bundle into a fresh
// worker_thread rather than the shared selftest process globalThis, so each
// fixture gets its own realm instead of deleting/reinstalling a hardened
// property. `requests` runs sequentially against the same imported module
// instance (one worker, one import) so fixtures that issue several
// __nimbusInvoke calls against one deploy still share module-level state the
// way a real warm instance would.
async function invokeCloudFunctionsBundle(appDir, requests, { hostResponses } = {}) {
  const bundleUrl = pathToFileURL(
    path.join(appDir, ".nimbus", "firebase", "bundle.mjs"),
  ).href;
  const worker = new Worker(cloudFunctionsBundleWorkerPath, {
    workerData: { bundleUrl, requests, hostResponses: hostResponses ?? null },
  });
  try {
    return await new Promise((resolve, reject) => {
      worker.once("message", (message) => {
        if (message.ok) {
          resolve({ results: message.results, hostCalls: message.hostCalls });
        } else {
          const error = new Error(message.error.message);
          error.stack = message.error.stack ?? error.stack;
          reject(error);
        }
      });
      worker.once("error", reject);
    });
  } finally {
    await worker.terminate();
  }
}

// HG0 (Band B-FIX, CAPTURE-ORDERING): same rationale as invokeCloudFunctionsBundle
// above, but generalized for fixtures whose per-test __nimbusCreateContext mock
// and assertion-worthy side effects vary too much for a data-driven
// hostResponses map (call-count-dependent stubs, captured call-argument
// arrays, multi-checkpoint global reads). `source` is a self-contained async
// IIFE (no static import/export syntax — it runs via Worker's eval mode,
// which only guarantees dynamic import()) that builds its own mocks, imports
// the bundle from workerData.bundleUrl, drives __nimbusInvoke, and posts back
// `{ ok: true, value }` or `{ ok: false, error }`. Each call gets a fresh
// worker realm, so the hardened __nimbusInvoke property is always installed
// exactly once.
async function runInWorkerRealm(source, workerData) {
  const worker = new Worker(source, { eval: true, workerData });
  try {
    return await new Promise((resolve, reject) => {
      worker.once("message", (message) => {
        if (message.ok) {
          resolve(message.value);
        } else {
          const error = new Error(message.error.message);
          error.stack = message.error.stack ?? error.stack;
          reject(error);
        }
      });
      worker.once("error", reject);
    });
  } finally {
    await worker.terminate();
  }
}

export {
  createAppFixture,
  invokeCloudFunctionsBundle,
  readCloudFunctionsFile,
  readCloudFunctionsJson,
  readConvexFile,
  readConvexJson,
  readGeneratedFile,
  runCli,
  runInWorkerRealm,
};
