#!/usr/bin/env node

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import net from "node:net";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..");
const RUNNER = path.join(SCRIPT_DIR, "examples-verify.sh");
const CUTS = [
  "after-run-root",
  "after-case-root",
  "after-server-spawn",
  "after-server-ready",
  "during-smoke",
  "before-server-stop",
];

function option(args, name, fallback) {
  const index = args.indexOf(name);
  if (index === -1) return fallback;
  assert(index + 1 < args.length, `${name} requires a value`);
  return args[index + 1];
}

function collectPhases(value, phases = []) {
  if (Array.isArray(value)) {
    for (const item of value) collectPhases(item, phases);
  } else if (value && typeof value === "object") {
    if (typeof value.phase === "string") phases.push(value.phase);
    for (const item of Object.values(value)) collectPhases(item, phases);
  }
  return phases;
}

async function assertReleasedAddress(serverLog) {
  const log = await fs.readFile(serverLog, "utf8");
  const match = log.match(/Nimbus server listening at http:\/\/(127\.0\.0\.1):(\d+)\//u);
  if (!match) return;
  const server = net.createServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(Number(match[2]), match[1], resolve);
  });
  await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
}

async function verifyRetainedFailure(cut, result) {
  assert.notEqual(result.status, 0, `${cut} must make the runner red`);
  assert.match(result.stderr, new RegExp(`injected examples verification fault at ${cut}`, "u"));
  if (cut !== "after-run-root") assert.match(result.stdout, /source byte manifest matches/u);
  const retained = result.stderr.match(/run failure retained diagnostic artifacts: (.+)$/mu)?.[1];
  assert(retained && path.isAbsolute(retained), `${cut} must report one retained artifact root`);
  assert.equal((await fs.stat(retained)).isDirectory(), true);

  const statePath = path.join(retained, "network-authority", "networks", "control-plane", "state.json");
  const state = await fs.readFile(statePath, "utf8").then(JSON.parse, () => null);
  if (state) {
    const phases = collectPhases(state);
    assert(
      phases.every((phase) => ["released", "failed"].includes(phase)),
      `${cut} left an unsettled provider lease phase: ${phases.join(", ")}`,
    );
  }

  const caseRoot = path.join(retained, "cases", "nimbus-tasks");
  if (await fs.stat(caseRoot).then(() => true, () => false)) {
    const lifetime = JSON.parse(await fs.readFile(path.join(retained, "lifetime.json"), "utf8"));
    const context = JSON.parse(await fs.readFile(path.join(caseRoot, "context.json"), "utf8"));
    const relocated = (candidate) => path.join(retained, path.relative(lifetime.runRoot, candidate));
    assert.equal(await fs.stat(relocated(context.discoveryPath)).then(() => true, () => false), false);
    const records = await fs.readdir(relocated(context.processRoot));
    assert.deepEqual(records, [], `${cut} must leave no managed process record`);
    assert.equal(
      await fs.stat(path.join(relocated(context.authRoot), "smoke.env")).then(() => true, () => false),
      false,
      `${cut} must scrub its smoke credential file before artifact retention`,
    );
    const serverLog = path.join(relocated(context.logRoot), "server.log");
    if (await fs.stat(serverLog).then(() => true, () => false)) await assertReleasedAddress(serverLog);
  }
  await fs.rm(retained, { recursive: true });
}

async function verifyCredentialCleanupRetry(binary) {
  const result = spawnSync("bash", [RUNNER], {
    cwd: REPO_ROOT,
    env: {
      ...process.env,
      NIMBUS_EXAMPLES_VERIFY_BIN: binary,
      NIMBUS_EXAMPLES_VERIFY_ONLY: "nimbus/tasks",
      NIMBUS_EXAMPLES_VERIFY_CREDENTIAL_DELETE_FAILURES: "1",
    },
    encoding: "utf8",
    timeout: 60_000,
    maxBuffer: 16 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  assert.notEqual(result.status, 0, "an injected credential deletion failure must make the runner red");
  assert.match(result.stderr, /injected smoke credential deletion failure/u);
  const retained = result.stderr.match(/run failure retained diagnostic artifacts: (.+)$/mu)?.[1];
  assert(retained && path.isAbsolute(retained), "credential cleanup retry must retain one artifact root");
  const context = JSON.parse(await fs.readFile(path.join(retained, "cases", "nimbus-tasks", "context.json"), "utf8"));
  const lifetime = JSON.parse(await fs.readFile(path.join(retained, "lifetime.json"), "utf8"));
  const relocatedAuthRoot = path.join(retained, path.relative(lifetime.runRoot, context.authRoot));
  assert.equal(await fs.stat(path.join(relocatedAuthRoot, "smoke.env")).then(() => true, () => false), false);
  await fs.rm(retained, { recursive: true });
}

async function main() {
  const binary = path.resolve(option(process.argv.slice(2), "--bin", path.join(REPO_ROOT, "target", "debug", "nimbus")));
  await fs.access(binary);
  for (const cut of CUTS) {
    const result = spawnSync("bash", [RUNNER], {
      cwd: REPO_ROOT,
      env: {
        ...process.env,
        NIMBUS_EXAMPLES_VERIFY_BIN: binary,
        NIMBUS_EXAMPLES_VERIFY_ONLY: "nimbus/tasks",
        NIMBUS_EXAMPLES_VERIFY_FAULT_CUT: cut,
      },
      encoding: "utf8",
      timeout: 60_000,
      maxBuffer: 16 * 1024 * 1024,
    });
    if (result.error) throw result.error;
    await verifyRetainedFailure(cut, result);
    console.log(`PASS runner_fault_cut_${cut.replaceAll("-", "_")}`);
  }
  await verifyCredentialCleanupRetry(binary);
  console.log("PASS runner_credential_cleanup_retry");
  console.log(`Summary: ${CUTS.length + 1} passed, 0 failed`);
}

main().catch((error) => {
  console.error(`FAIL runner fault cuts: ${error.message}`);
  process.exitCode = 1;
});
