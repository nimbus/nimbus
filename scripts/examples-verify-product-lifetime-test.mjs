#!/usr/bin/env node

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import {
  createCaseContext,
  createRunContext,
  finalizeRunContext,
  readCaseDiscovery,
  requestGracefulShutdown,
} from "./examples-verify-lifetime.mjs";
import {
  spawnManagedProcess,
  stopManagedProcess,
} from "./examples-verify-supervisor.mjs";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..");

function option(args, name, fallback) {
  const index = args.indexOf(name);
  if (index === -1) return fallback;
  assert(index + 1 < args.length, `${name} requires a value`);
  return args[index + 1];
}

function exactCaseEnvironment(context) {
  const environment = { ...process.env };
  for (const key of Object.keys(environment)) {
    if (key.startsWith("NIMBUS_")) delete environment[key];
  }
  return { ...environment, ...context.environment };
}

async function waitForProductDiscovery(context, pid, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const discovery = await readCaseDiscovery(context.discoveryPath, pid);
      const response = await fetch(`${discovery.url}/health`);
      if (response.ok && (await response.json()).ok === true) return discovery;
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`product server ${pid} did not become healthy: ${lastError?.message ?? "no discovery"}`);
}

async function assertPortReleased(address) {
  const endpoint = new URL(`http://${address}`);
  const server = net.createServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(Number(endpoint.port), endpoint.hostname, resolve);
  });
  await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
}

function readCaseAdminToken(binary, context) {
  const result = spawnSync(binary, ["auth", "token"], {
    cwd: REPO_ROOT,
    env: exactCaseEnvironment(context),
    encoding: "utf8",
  });
  assert.equal(result.status, 0, `case-local auth token read failed: ${result.stderr.trim()}`);
  assert.equal(result.stderr, "", "case-local auth token read must keep stderr clean");
  const token = result.stdout.trim();
  assert(token.length > 0, "case-local auth token must not be empty");
  return token;
}

async function main() {
  const binary = path.resolve(option(process.argv.slice(2), "--bin", path.join(REPO_ROOT, "target", "debug", "nimbus")));
  await fs.access(binary, fs.constants.X_OK);
  const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-avr7-product-parent-"));
  const artifactRoot = path.join(tempRoot, "artifacts");
  const run = await createRunContext({ repoRoot: REPO_ROOT, tempRoot, artifactRoot });
  const first = await createCaseContext(run, { name: "product-first", workspace: "product-first" });
  const second = await createCaseContext(run, { name: "product-second", workspace: "product-second" });
  const managed = [];
  let primaryError;
  try {
    for (const context of [first, second]) {
      const recordPath = path.join(context.processRoot, "server.json");
      const pid = await spawnManagedProcess({
        recordPath,
        logPath: path.join(context.logRoot, "server.log"),
        command: binary,
        args: [
          "start",
          "--port", "0",
          "--no-mongodb",
          "--no-dynamodb",
          "--no-s3",
          "--no-firestore",
          "--no-cloudflare",
          "--data-dir", context.dataRoot,
          "--control-data-dir", context.controlRoot,
          "--network-state-dir", context.networkStateRoot,
        ],
        environment: context.environment,
        clearPrefixes: ["NIMBUS_"],
      });
      managed.push({ context, pid, recordPath });
    }

    const [firstDiscovery, secondDiscovery] = await Promise.all(managed.map(({ context, pid }) =>
      waitForProductDiscovery(context, pid)));
    assert.notEqual(firstDiscovery.address, secondDiscovery.address, "concurrent product listeners must be distinct");
    assert.equal(first.networkStateRoot, second.networkStateRoot, "both cases must use one network authority");
    await assert.rejects(
      readCaseDiscovery(first.discoveryPath, managed[1].pid),
      /belongs to pid/u,
      "a second case must not satisfy the first case's discovery identity",
    );

    const firstToken = readCaseAdminToken(binary, first);
    const secondToken = readCaseAdminToken(binary, second);
    assert.notEqual(firstToken, secondToken, "case-local auth roots must produce distinct tokens");
    await Promise.all([
      requestGracefulShutdown(firstDiscovery.url, firstToken),
      requestGracefulShutdown(secondDiscovery.url, secondToken),
    ]);
    await Promise.all(managed.map(({ recordPath }) => stopManagedProcess(recordPath)));
    for (const { context } of managed) {
      const log = await fs.readFile(path.join(context.logRoot, "server.log"), "utf8");
      assert.doesNotMatch(
        log,
        /failed to record system shutdown event/u,
        "graceful cleanup must not emit a schema-invalid system-event warning",
      );
    }
    await Promise.all([
      assertPortReleased(firstDiscovery.address),
      assertPortReleased(secondDiscovery.address),
    ]);
    const settled = await finalizeRunContext(run, { runStatus: 0, cleanupStatus: 0 });
    assert.equal(settled.status, 0);
    await fs.rm(tempRoot, { recursive: true, force: true });
    console.log("PASS product_concurrent_cases_share_authority_and_isolate_operator_state");
    console.log("Summary: 1 passed, 0 failed");
  } catch (error) {
    primaryError = error;
    throw error;
  } finally {
    let cleanupStatus = 0;
    for (const item of managed) {
      try {
        await stopManagedProcess(item.recordPath);
      } catch (error) {
        cleanupStatus = 1;
        console.error(`cleanup failed for product server ${item.pid}: ${error.message}`);
      }
    }
    if (primaryError && await fs.stat(run.runRoot).then(() => true, () => false)) {
      const result = await finalizeRunContext(run, { runStatus: 1, cleanupStatus });
      if (result.retainedPath) console.error(`${result.reason}: ${result.retainedPath}`);
    }
  }
}

main().catch((error) => {
  console.error(`FAIL product lifetime: ${error.message}`);
  process.exitCode = 1;
});
