#!/usr/bin/env node

import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  createCaseContext,
  createRunContext,
  finalizeRunContext,
  moveToArtifacts,
  readCaseDiscovery,
} from "./examples-verify-lifetime.mjs";
import {
  isManagedProcessLive,
  spawnManagedProcess,
  stopManagedProcess,
} from "./examples-verify-supervisor.mjs";

const SUPERVISOR_PATH = fileURLToPath(new URL("./examples-verify-supervisor.mjs", import.meta.url));

async function pathExists(candidate) {
  return await fs.lstat(candidate).then(() => true, () => false);
}

async function waitForFile(candidate, timeoutMs = 5_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await pathExists(candidate)) return;
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  throw new Error(`timed out waiting for ${candidate}`);
}

async function waitForProcessStop(pid, timeoutMs = 5_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (!await isManagedProcessLive(pid)) return;
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  assert.equal(await isManagedProcessLive(pid), false, `process ${pid} remained live`);
}

async function temporaryCampaign(test) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-avr7-test-"));
  const repoRoot = path.join(root, "repo");
  const tempRoot = path.join(root, "tmp");
  const artifactRoot = path.join(root, "artifacts");
  await fs.mkdir(repoRoot);
  await fs.mkdir(tempRoot);
  try {
    await test({ root, repoRoot, tempRoot, artifactRoot });
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
}

const serverFixture = String.raw`
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const net = require("node:net");
const discoveryPath = process.argv[1];
const childPath = process.argv[2] || "";
let child;
if (childPath) {
  child = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: "ignore" });
  fs.writeFileSync(childPath, String(child.pid));
}
const server = net.createServer((socket) => socket.end("ok"));
server.listen(0, "127.0.0.1", () => {
  const address = server.address();
  fs.mkdirSync(require("node:path").dirname(discoveryPath), { recursive: true });
  fs.writeFileSync(discoveryPath, JSON.stringify({ pid: process.pid, address: address.address + ":" + address.port }));
});
process.on("SIGTERM", () => server.close(() => process.exit(0)));
setInterval(() => {}, 1000);
`;

async function spawnFixture(caseContext, { withChild = false } = {}) {
  const childPath = withChild ? path.join(caseContext.caseRoot, "child.pid") : "";
  const processRecord = path.join(caseContext.caseRoot, "server-process.json");
  const serverLog = path.join(caseContext.logRoot, "server.log");
  const pid = await spawnManagedProcess({
    recordPath: processRecord,
    logPath: serverLog,
    command: process.execPath,
    args: ["-e", serverFixture, caseContext.discoveryPath, childPath],
    environment: caseContext.environment,
    clearPrefixes: ["NIMBUS_"],
  });
  await waitForFile(caseContext.discoveryPath);
  return { childPath, pid, processRecord };
}

async function case_context_isolates_operator_state_and_shares_network_root() {
  await temporaryCampaign(async ({ repoRoot, tempRoot, artifactRoot }) => {
    if (process.platform !== "win32") await fs.chmod(tempRoot, 0o755);
    const run = await createRunContext({ repoRoot, tempRoot, artifactRoot });
    const first = await createCaseContext(run, { name: "first/case", workspace: "first" });
    const second = await createCaseContext(run, { name: "second/case", workspace: "second" });
    await assert.rejects(
      createCaseContext(run, { name: "first-case", workspace: "third" }),
      /case root already exists/u,
      "normalized case identities must fail closed instead of sharing roots",
    );

    assert.notEqual(first.authRoot, second.authRoot);
    assert.notEqual(first.discoveryRoot, second.discoveryRoot);
    assert.notEqual(first.auditRoot, second.auditRoot);
    assert.notEqual(first.appRoot, second.appRoot);
    assert.notEqual(first.dataRoot, second.dataRoot);
    assert.notEqual(first.controlRoot, second.controlRoot);
    assert.notEqual(first.logRoot, second.logRoot);
    assert.notEqual(first.resultRoot, second.resultRoot);
    assert.equal(first.environment.NIMBUS_NETWORK_STATE_DIR, run.networkStateRoot);
    assert.equal(second.environment.NIMBUS_NETWORK_STATE_DIR, run.networkStateRoot);
    if (process.platform !== "win32") {
      assert.equal((await fs.stat(tempRoot)).mode & 0o777, 0o755, "the shared temp parent mode must not change");
    }
    for (const [firstRoot, secondRoot] of [
      [first.authRoot, second.authRoot],
      [first.discoveryRoot, second.discoveryRoot],
      [first.auditRoot, second.auditRoot],
      [first.dataRoot, second.dataRoot],
      [first.controlRoot, second.controlRoot],
      [first.logRoot, second.logRoot],
      [first.resultRoot, second.resultRoot],
    ]) {
      await fs.writeFile(path.join(firstRoot, "cross-case.sentinel"), "first-only\n");
      assert.equal(
        await pathExists(path.join(secondRoot, "cross-case.sentinel")),
        false,
        `case-local root ${secondRoot} must not observe the first case's sentinel`,
      );
    }

    await finalizeRunContext(run, { runStatus: 0, cleanupStatus: 0 });
    assert.equal(await pathExists(run.runRoot), false);
  });
}

async function process_spawn_record_failure_settles_unrecorded_child() {
  await temporaryCampaign(async ({ repoRoot, tempRoot, artifactRoot }) => {
    const run = await createRunContext({ repoRoot, tempRoot, artifactRoot });
    const context = await createCaseContext(run, { name: "record-failure", workspace: "record-failure" });
    const recordPath = path.join(context.processRoot, "server-process.json");
    let writeAttempted = false;
    let failure;
    try {
      await spawnManagedProcess({
        recordPath,
        logPath: path.join(context.logRoot, "unrecorded.log"),
        command: process.execPath,
        args: ["-e", "setInterval(() => {}, 1000)"],
        writeRecord: async () => {
          writeAttempted = true;
          throw new Error("injected record write failure");
        },
      });
    } catch (error) {
      failure = error;
    }
    assert(failure, "record failure must reject the spawn");
    assert.equal(writeAttempted, true, "the fault must occur after process creation");
    assert.match(failure.message, /failed after spawning unrecorded managed process/u);
    assert.equal(failure.processSettled, true);
    await waitForProcessStop(failure.processPid);
    await finalizeRunContext(run, { runStatus: 0, cleanupStatus: 0 });
  });
}

async function corrupt_process_record_fails_before_spawn() {
  await temporaryCampaign(async ({ repoRoot, tempRoot, artifactRoot }) => {
    const run = await createRunContext({ repoRoot, tempRoot, artifactRoot });
    const context = await createCaseContext(run, { name: "corrupt-record", workspace: "corrupt-record" });
    const recordPath = path.join(context.processRoot, "server-process.json");
    const spawnMarker = path.join(context.processRoot, "spawned.marker");
    await fs.writeFile(recordPath, "not-json\n");

    await assert.rejects(
      spawnManagedProcess({
        recordPath,
        logPath: path.join(context.logRoot, "server.log"),
        command: process.execPath,
        args: ["-e", "require('node:fs').writeFileSync(process.argv[1], 'spawned')", spawnMarker],
      }),
      /cannot read managed process record/u,
    );
    assert.equal(await pathExists(spawnMarker), false, "a corrupt owner record must fail before process creation");
    await fs.rm(recordPath);
    await finalizeRunContext(run, { runStatus: 0, cleanupStatus: 0 });
  });
}

async function two_cases_bind_concurrently_with_distinct_discovery() {
  await temporaryCampaign(async ({ repoRoot, tempRoot, artifactRoot }) => {
    const run = await createRunContext({ repoRoot, tempRoot, artifactRoot });
    const first = await createCaseContext(run, { name: "first", workspace: "first" });
    const second = await createCaseContext(run, { name: "second", workspace: "second" });
    const [firstProcess, secondProcess] = await Promise.all([
      spawnFixture(first),
      spawnFixture(second),
    ]);
    try {
      const [firstDiscovery, secondDiscovery] = await Promise.all([
        readCaseDiscovery(first.discoveryPath, firstProcess.pid),
        readCaseDiscovery(second.discoveryPath, secondProcess.pid),
      ]);
      assert.notEqual(firstDiscovery.address, secondDiscovery.address);
      assert.equal(first.environment.NIMBUS_NETWORK_STATE_DIR, second.environment.NIMBUS_NETWORK_STATE_DIR);
    } finally {
      await Promise.all([
        stopManagedProcess(firstProcess.processRecord),
        stopManagedProcess(secondProcess.processRecord),
      ]);
    }
    await finalizeRunContext(run, { runStatus: 0, cleanupStatus: 0 });
  });
}

async function external_binder_cannot_satisfy_case_discovery() {
  await temporaryCampaign(async ({ repoRoot, tempRoot, artifactRoot }) => {
    const run = await createRunContext({ repoRoot, tempRoot, artifactRoot });
    const context = await createCaseContext(run, { name: "expected", workspace: "expected" });
    const external = net.createServer();
    await new Promise((resolve, reject) => external.once("error", reject).listen(0, "127.0.0.1", resolve));
    const address = external.address();
    await fs.mkdir(path.dirname(context.discoveryPath), { recursive: true });
    await fs.writeFile(
      context.discoveryPath,
      `${JSON.stringify({ pid: process.pid, address: `${address.address}:${address.port}` })}\n`,
    );
    await assert.rejects(
      readCaseDiscovery(context.discoveryPath, process.pid + 1),
      /belongs to pid/u,
    );
    external.close();
    await finalizeRunContext(run, { runStatus: 0, cleanupStatus: 0 });
  });
}

async function process_tree_cleanup_stops_descendants() {
  await temporaryCampaign(async ({ repoRoot, tempRoot, artifactRoot }) => {
    const run = await createRunContext({ repoRoot, tempRoot, artifactRoot });
    const context = await createCaseContext(run, { name: "tree", workspace: "tree" });
    const managed = await spawnFixture(context, { withChild: true });
    await waitForFile(managed.childPath);
    const childPid = Number((await fs.readFile(managed.childPath, "utf8")).trim());
    assert.equal(await isManagedProcessLive(managed.pid), true);
    assert.equal(await isManagedProcessLive(childPid), true);
    await stopManagedProcess(managed.processRecord);
    assert.equal(await isManagedProcessLive(managed.pid), false);
    assert.equal(await isManagedProcessLive(childPid), false);
    await finalizeRunContext(run, { runStatus: 0, cleanupStatus: 0 });
  });
}

async function pre_signal_wait_allows_graceful_process_exit() {
  await temporaryCampaign(async ({ repoRoot, tempRoot, artifactRoot }) => {
    const run = await createRunContext({ repoRoot, tempRoot, artifactRoot });
    const context = await createCaseContext(run, { name: "graceful-exit", workspace: "graceful-exit" });
    const processRecord = path.join(context.processRoot, "server-process.json");
    const signalMarker = path.join(context.processRoot, "signal.marker");
    const fixture = [
      "const fs = require('node:fs');",
      "process.on('SIGTERM', () => fs.writeFileSync(process.argv[1], 'signaled'));",
      "setTimeout(() => process.exit(0), 250);",
    ].join("");
    const pid = await spawnManagedProcess({
      recordPath: processRecord,
      logPath: path.join(context.logRoot, "graceful-exit.log"),
      command: process.execPath,
      args: ["-e", fixture, signalMarker],
    });
    assert.equal(await isManagedProcessLive(pid), true);
    await stopManagedProcess(processRecord, { preSignalTimeoutMs: 2_000 });
    assert.equal(await isManagedProcessLive(pid), false);
    assert.equal(await pathExists(signalMarker), false, "the owner must not signal a process that exits inside the grace bound");
    assert.equal(await pathExists(processRecord), false);
    await finalizeRunContext(run, { runStatus: 0, cleanupStatus: 0 });
  });
}

async function six_fault_cuts_release_active_resources() {
  const cuts = [
    "after-run-root",
    "after-case-root",
    "after-server-spawn",
    "after-server-ready",
    "during-smoke",
    "before-server-stop",
  ];
  for (const cut of cuts) {
    await temporaryCampaign(async ({ repoRoot, tempRoot, artifactRoot }) => {
      const run = await createRunContext({ repoRoot, tempRoot, artifactRoot });
      let context;
      let managed;
      try {
        if (cut === "after-run-root") throw new Error(cut);
        context = await createCaseContext(run, { name: cut, workspace: cut });
        if (cut === "after-case-root") throw new Error(cut);
        managed = await spawnFixture(context);
        if (cut === "after-server-spawn") throw new Error(cut);
        await readCaseDiscovery(context.discoveryPath, managed.pid);
        if (["after-server-ready", "during-smoke", "before-server-stop"].includes(cut)) {
          throw new Error(cut);
        }
      } catch (error) {
        assert.equal(error.message, cut);
      } finally {
        if (managed) await stopManagedProcess(managed.processRecord);
      }
      if (managed) assert.equal(await isManagedProcessLive(managed.pid), false);
      const result = await finalizeRunContext(run, { runStatus: 17, cleanupStatus: 0 });
      assert.equal(result.status, 17);
      assert.equal(result.cleanupStatus, "passed");
      assert.equal(await pathExists(run.runRoot), false);
      assert.equal(await pathExists(result.retainedPath), true);
    });
  }
}

async function cleanup_retry_converges_after_retained_failure() {
  await temporaryCampaign(async ({ repoRoot, tempRoot, artifactRoot }) => {
    const run = await createRunContext({ repoRoot, tempRoot, artifactRoot });
    await fs.writeFile(path.join(run.runRoot, "cleanup.marker"), "present\n");
    const failed = await finalizeRunContext(run, { runStatus: 0, cleanupStatus: 1 });
    assert.equal(failed.status, 1);
    assert.equal(failed.cleanupStatus, "failed");
    assert.equal(failed.retainedPath, run.runRoot);
    assert.equal(await pathExists(run.runRoot), true);
    const retried = await finalizeRunContext(run, { runStatus: 0, cleanupStatus: 0 });
    assert.equal(retried.status, 0);
    assert.equal(retried.cleanupStatus, "passed");
    assert.equal(await pathExists(run.runRoot), false);
  });
}

async function cross_filesystem_retention_retry_converges_after_source_removal_failure() {
  await temporaryCampaign(async ({ repoRoot, tempRoot, artifactRoot }) => {
    const run = await createRunContext({ repoRoot, tempRoot, artifactRoot });
    await fs.writeFile(path.join(run.runRoot, "failure.log"), "retained evidence\n");
    let removeAttempts = 0;
    const crossFilesystemRename = async (source, destination) => {
      if (source === run.runRoot) {
        const error = new Error("injected cross-filesystem rename");
        error.code = "EXDEV";
        throw error;
      }
      await fs.rename(source, destination);
    };
    const failFirstSourceRemoval = async (candidate, options) => {
      if (candidate === run.runRoot && removeAttempts++ === 0) {
        const error = new Error("injected source removal failure");
        error.code = "EPERM";
        throw error;
      }
      await fs.rm(candidate, options);
    };

    await assert.rejects(
      moveToArtifacts(run, {
        rename: crossFilesystemRename,
        remove: failFirstSourceRemoval,
      }),
      /injected source removal failure/u,
    );
    const destination = path.join(artifactRoot, path.basename(run.runRoot));
    assert.equal(await pathExists(run.runRoot), true);
    assert.equal(await pathExists(destination), true);

    assert.equal(await moveToArtifacts(run), destination);
    assert.equal(await pathExists(run.runRoot), false);
    assert.equal(await fs.readFile(path.join(destination, "failure.log"), "utf8"), "retained evidence\n");
  });
}

async function supervisor_environment_file_keeps_secret_values_out_of_argv() {
  await temporaryCampaign(async ({ root }) => {
    const environmentPath = path.join(root, "smoke.env");
    const outputPath = path.join(root, "child-environment.txt");
    const secretValue = `private-value-${process.pid}`;
    await fs.writeFile(environmentPath, `NIMBUS_AVR7_SECRET=${secretValue}\n`, { mode: 0o600 });
    const fixture = [
      "const fs = require('node:fs');",
      "fs.writeFileSync(process.argv[1], process.env.NIMBUS_AVR7_SECRET || 'missing');",
      "setTimeout(() => {}, 250);",
    ].join("");
    const supervisor = spawn(process.execPath, [
      SUPERVISOR_PATH,
      "exec",
      "--cwd",
      root,
      "--clear-prefix",
      "NIMBUS_",
      "--env-file",
      environmentPath,
      "--",
      process.execPath,
      "-e",
      fixture,
      outputPath,
    ], { stdio: "ignore" });
    const exit = new Promise((resolve, reject) => {
      supervisor.once("error", reject);
      supervisor.once("exit", resolve);
    });
    assert.equal(
      supervisor.spawnargs.some((argument) => argument.includes(secretValue)),
      false,
      "the supervisor argv must contain only the environment-file path, never its values",
    );
    await waitForFile(outputPath);
    assert.equal(await fs.readFile(outputPath, "utf8"), secretValue);
    assert.equal(await exit, 0);

    if (process.platform !== "win32") {
      const linkedEnvironmentPath = path.join(root, "linked-smoke.env");
      const linkedOutputPath = path.join(root, "linked-child-environment.txt");
      await fs.symlink(environmentPath, linkedEnvironmentPath);
      const rejected = spawnSync(process.execPath, [
        SUPERVISOR_PATH,
        "exec",
        "--cwd",
        root,
        "--env-file",
        linkedEnvironmentPath,
        "--",
        process.execPath,
        "-e",
        "require('node:fs').writeFileSync(process.argv[1], 'spawned')",
        linkedOutputPath,
      ], { encoding: "utf8" });
      assert.notEqual(rejected.status, 0, "a linked credential file must fail before child spawn");
      assert.equal(await pathExists(linkedOutputPath), false);
    }
  });
}

async function failed_run_retains_artifact_without_live_resources() {
  await temporaryCampaign(async ({ repoRoot, tempRoot, artifactRoot }) => {
    const run = await createRunContext({ repoRoot, tempRoot, artifactRoot });
    const context = await createCaseContext(run, { name: "failure", workspace: "failure" });
    const managed = await spawnFixture(context);
    await fs.writeFile(path.join(context.logRoot, "smoke.stderr"), "failure evidence\n");
    await stopManagedProcess(managed.processRecord);
    const result = await finalizeRunContext(run, { runStatus: 23, cleanupStatus: 0 });
    assert.equal(result.status, 23);
    assert.equal(result.cleanupStatus, "passed");
    assert.equal(await isManagedProcessLive(managed.pid), false);
    assert.equal(await pathExists(run.runRoot), false);
    assert.equal(await fs.readFile(path.join(result.retainedPath, "cases", "failure", "logs", "smoke.stderr"), "utf8"), "failure evidence\n");
  });
}

const tests = [
  case_context_isolates_operator_state_and_shares_network_root,
  two_cases_bind_concurrently_with_distinct_discovery,
  external_binder_cannot_satisfy_case_discovery,
  process_tree_cleanup_stops_descendants,
  pre_signal_wait_allows_graceful_process_exit,
  six_fault_cuts_release_active_resources,
  cleanup_retry_converges_after_retained_failure,
  cross_filesystem_retention_retry_converges_after_source_removal_failure,
  failed_run_retains_artifact_without_live_resources,
  supervisor_environment_file_keeps_secret_values_out_of_argv,
  process_spawn_record_failure_settles_unrecorded_child,
  corrupt_process_record_fails_before_spawn,
];

for (const test of tests) {
  await test();
  console.log(`PASS ${test.name}`);
}
console.log(`Summary: ${tests.length} passed, 0 failed`);
