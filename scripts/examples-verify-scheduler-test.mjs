#!/usr/bin/env node

import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..");
const RUNNER = path.join(SCRIPT_DIR, "examples-verify.sh");
const MANIFEST = JSON.parse(await fs.readFile(path.join(SCRIPT_DIR, "examples-verify-cases.json"), "utf8"));
const MANIFEST_ORDER = MANIFEST.cases.map((item) => item.name);
const PRIORITY_ORDER = [
  ...MANIFEST.cases.filter((item) => item.prepare.codegen || item.surfaces.includes("cloud-functions-http")),
  ...MANIFEST.cases.filter((item) => !item.prepare.codegen && !item.surfaces.includes("cloud-functions-http") && item.boot.mode === "dev"),
  ...MANIFEST.cases.filter((item) => !item.prepare.codegen && !item.surfaces.includes("cloud-functions-http") && item.boot.mode !== "dev"),
].map((item) => item.name);

function option(args, name, fallback) {
  const index = args.indexOf(name);
  if (index === -1) return fallback;
  assert(index + 1 < args.length, `${name} requires a value`);
  return args[index + 1];
}

async function pathExists(candidate) {
  return await fs.lstat(candidate).then(() => true, (error) => {
    if (error?.code === "ENOENT") return false;
    throw error;
  });
}

async function waitFor(predicate, label, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`timed out waiting for ${label}`);
}

async function walk(root, matches = []) {
  if (!await pathExists(root)) return matches;
  for (const entry of await fs.readdir(root, { withFileTypes: true })) {
    const candidate = path.join(root, entry.name);
    if (entry.isDirectory()) await walk(candidate, matches);
    else matches.push(candidate);
  }
  return matches;
}

function reportPaths(stdout) {
  const match = stdout.match(/^(.+\/report\.json)\|(.+\/junit\.xml)$/mu);
  assert(match, "runner must print its canonical JSON and JUnit paths");
  return { reportPath: match[1], junitPath: match[2] };
}

function retainedPath(stderr) {
  const retained = stderr.match(/run failure retained diagnostic artifacts: (.+)$/mu)?.[1];
  assert(retained && path.isAbsolute(retained), "failed run must retain one absolute artifact path");
  return retained;
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

async function verifyFailedRun({ stdout, stderr, expectedExit, requireDrainedCases }) {
  const { reportPath, junitPath } = reportPaths(stdout);
  const retained = retainedPath(stderr);
  const report = JSON.parse(await fs.readFile(reportPath, "utf8"));
  const junit = await fs.readFile(junitPath, "utf8");
  assert.equal(report.run.status, "failed");
  assert.equal(report.run.exitCode, expectedExit);
  assert.equal(report.provenance.source.status, "matched");
  assert.equal(report.cleanup.status, "passed");
  assert.equal(report.cleanup.artifactRetained, true);
  assert.deepEqual(report.run.selectedCases, MANIFEST_ORDER);
  assert.deepEqual(report.cases.map((item) => item.name), MANIFEST_ORDER);
  assert.match(junit, /<failure/u);
  if (requireDrainedCases) {
    const casesByName = new Map(report.cases.map((item) => [item.name, item]));
    assert(
      PRIORITY_ORDER.slice(0, 2).every((name) => casesByName.get(name).observed.status === "passed"),
      `signal must let active cases drain; observed ${report.cases.map((item) => item.observed.status).join(", ")}`,
    );
    assert(
      report.cases
        .filter((item) => !PRIORITY_ORDER.slice(0, 2).includes(item.name))
        .every((item) => item.observed.status === "not-run"),
      "signal must prevent later cases from starting",
    );
  }

  const processRecords = (await walk(retained)).filter((candidate) => /\/processes\/server\.json$/u.test(candidate));
  assert.deepEqual(processRecords, [], "retained evidence must contain no live-process owner records");
  const statePath = path.join(retained, "network-authority", "networks", "control-plane", "state.json");
  if (await pathExists(statePath)) {
    const phases = collectPhases(JSON.parse(await fs.readFile(statePath, "utf8")));
    assert(phases.every((phase) => ["released", "failed"].includes(phase)), `unsettled lease phases: ${phases.join(", ")}`);
  }
  const serverLogs = (await walk(retained)).filter((candidate) => /\/logs\/server\.log$/u.test(candidate));
  for (const serverLog of serverLogs) await assertReleasedAddress(serverLog);
  return { report, reportRoot: path.dirname(reportPath), retained };
}

async function cleanupEvidence(...paths) {
  for (const candidate of paths) await fs.rm(candidate, { recursive: true, force: true });
}

async function failure_drains_without_starting_later_cases(binary) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-avr9-failure-"));
  const results = path.join(root, "results");
  let retained;
  try {
    const result = spawnSync("bash", [RUNNER], {
      cwd: REPO_ROOT,
      env: {
        ...process.env,
        TMPDIR: root,
        NIMBUS_EXAMPLES_VERIFY_BIN: binary,
        NIMBUS_EXAMPLES_VERIFY_RESULTS_DIR: results,
        NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL: "2",
        NIMBUS_EXAMPLES_VERIFY_FAULT_CUT: "after-case-root",
        NIMBUS_EXAMPLES_VERIFY_FAULT_CASE: PRIORITY_ORDER[0],
      },
      encoding: "utf8",
      timeout: 120_000,
      maxBuffer: 32 * 1024 * 1024,
    });
    if (result.error) throw result.error;
    assert.equal(result.status, 97, `targeted fault returned ${result.status}: ${result.stderr}`);
    assert.match(`${result.stdout}\n${result.stderr}`, /injected examples verification fault at after-case-root/u);
    retained = retainedPath(result.stderr);
    const evidence = await verifyFailedRun({
      stdout: result.stdout,
      stderr: result.stderr,
      expectedExit: 97,
      requireDrainedCases: false,
    });
    const casesByName = new Map(evidence.report.cases.map((item) => [item.name, item]));
    assert.equal(casesByName.get(PRIORITY_ORDER[0]).observed.status, "failed");
    assert(
      evidence.report.cases
        .filter((item) => !PRIORITY_ORDER.slice(0, 2).includes(item.name))
        .every((item) => item.observed.status === "not-run"),
      "workers must not begin a later manifest case after the first failure",
    );
    await cleanupEvidence(evidence.retained, evidence.reportRoot);
    retained = undefined;
  } finally {
    if (retained) await cleanupEvidence(retained);
    await cleanupEvidence(root);
  }
}

async function signal_drains_active_workers(binary) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-avr9-signal-"));
  const results = path.join(root, "results");
  const child = spawn("bash", [RUNNER], {
    cwd: REPO_ROOT,
    env: {
      ...process.env,
      TMPDIR: root,
      NIMBUS_EXAMPLES_VERIFY_BIN: binary,
      NIMBUS_EXAMPLES_VERIFY_RESULTS_DIR: results,
      NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL: "2",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  let retained;
  child.stdout.setEncoding("utf8").on("data", (chunk) => { stdout += chunk; });
  child.stderr.setEncoding("utf8").on("data", (chunk) => { stderr += chunk; });
  let timeout;
  try {
    await waitFor(() => stdout.includes("scheduling 9 applications with max_parallel=2"), "scheduler start");
    await waitFor(async () => {
      const records = (await walk(root)).filter((candidate) => /\/processes\/server\.json$/u.test(candidate));
      return records.length === 2;
    }, "two active server records");
    child.kill("SIGTERM");
    const result = await new Promise((resolve, reject) => {
      timeout = setTimeout(() => reject(new Error("signal-drain runner did not exit within 60 seconds")), 60_000);
      child.once("error", reject);
      child.once("close", (status, signal) => resolve({ status, signal }));
    });
    assert.equal(result.signal, null);
    assert.equal(result.status, 143, `signaled runner returned ${result.status}: ${stderr}`);
    retained = retainedPath(stderr);
    const evidence = await verifyFailedRun({ stdout, stderr, expectedExit: 143, requireDrainedCases: true });
    await cleanupEvidence(evidence.retained, evidence.reportRoot);
    retained = undefined;
  } finally {
    clearTimeout(timeout);
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
    if (retained) await cleanupEvidence(retained);
    await cleanupEvidence(root);
  }
}

async function main() {
  const binary = path.resolve(option(process.argv.slice(2), "--bin", path.join(REPO_ROOT, "target", "debug", "nimbus")));
  await fs.access(binary);
  await failure_drains_without_starting_later_cases(binary);
  console.log("PASS failure_drains_without_starting_later_cases");
  await signal_drains_active_workers(binary);
  console.log("PASS signal_drains_active_workers");
  console.log("Summary: 2 passed, 0 failed");
}

main().catch((error) => {
  console.error(`FAIL scheduler behavior: ${error.stack ?? error.message}`);
  process.exitCode = 1;
});
