#!/usr/bin/env node

import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import process from "node:process";

import { execManagedProcess } from "./examples-verify-supervisor.mjs";

async function stdout_log_captures_exact_child_output() {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-supervisor-output-"));
  const output = path.join(root, "smoke.stdout.log");
  try {
    const status = execManagedProcess({
      command: process.execPath,
      args: ["-e", 'process.stdout.write("PASS fixture.anchor\\n")'],
      cwd: root,
      stdoutLog: output,
    });
    assert.equal(status, 0);
    assert.equal(await fs.readFile(output, "utf8"), "PASS fixture.anchor\n");
    if (process.platform !== "win32") {
      assert.equal((await fs.stat(output)).mode & 0o077, 0, "stdout evidence must be owner-only");
    }
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
}

async function stdout_log_refuses_existing_output_before_spawn() {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-supervisor-output-"));
  const output = path.join(root, "smoke.stdout.log");
  const marker = path.join(root, "spawned.marker");
  try {
    await fs.writeFile(output, "existing\n");
    assert.throws(
      () => execManagedProcess({
        command: process.execPath,
        args: ["-e", "require('node:fs').writeFileSync(process.argv[1], 'spawned')", marker],
        cwd: root,
        stdoutLog: output,
      }),
      /EEXIST/u,
    );
    assert.equal(await fs.stat(marker).then(() => true, () => false), false);
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
}

const tests = [
  stdout_log_captures_exact_child_output,
  stdout_log_refuses_existing_output_before_spawn,
];

let failed = 0;
for (const test of tests) {
  try {
    await test();
    console.log(`PASS ${test.name}`);
  } catch (error) {
    failed += 1;
    console.error(`FAIL ${test.name}: ${error.stack ?? error.message}`);
  }
}
console.log(`Summary: ${tests.length - failed} passed, ${failed} failed`);
if (failed > 0) process.exitCode = 1;
