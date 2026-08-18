#!/usr/bin/env node

import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import {
  anchorsFromSmokeOutput,
  deterministicCases,
  junit,
  redact,
  validateReport,
  writeJsonAtomically,
} from "./examples-verify-report.mjs";

const SHA_A = "a".repeat(64);
const SHA_B = "b".repeat(64);
const SHA_C = "c".repeat(64);

function desired(name) {
  return {
    name,
    boot: { mode: "start" },
    surfaces: ["native-http"],
    updateSemantics: "push",
    expectedAnchors: [`${name}.create`, `${name}.list`],
  };
}

function caseRecord(name, { status = "passed", exitCode = 0, cleanup = "passed", anchors = null } = {}) {
  const item = desired(name);
  return {
    name,
    desired: {
      bootMode: item.boot.mode,
      surfaces: item.surfaces,
      updateSemantics: item.updateSemantics,
      expectedAnchors: item.expectedAnchors,
    },
    observed: {
      status,
      startedAt: "2026-08-18T00:00:01.000Z",
      completedAt: "2026-08-18T00:00:02.000Z",
      durationMs: 1_000,
      exitCode,
      endpoint: { protocol: "http", host: "127.0.0.1", port: 43123 },
      anchors: [...(anchors ?? item.expectedAnchors)],
    },
    cleanup: { status: cleanup },
  };
}

function notRunRecord(name) {
  const item = desired(name);
  return {
    name,
    desired: {
      bootMode: item.boot.mode,
      surfaces: item.surfaces,
      updateSemantics: item.updateSemantics,
      expectedAnchors: item.expectedAnchors,
    },
    observed: {
      status: "not-run",
      startedAt: null,
      completedAt: null,
      durationMs: 0,
      exitCode: 1,
      endpoint: null,
      anchors: [],
    },
    cleanup: { status: "not-run" },
  };
}

function reportFixture({ status = "passed", exitCode = 0, cases = null, cleanup = "passed" } = {}) {
  const selectedCases = ["first", "second"];
  return {
    schemaVersion: 1,
    kind: "nimbus.examples-verification",
    run: {
      id: "nimbus-examples-verify.fixture",
      status,
      startedAt: "2026-08-18T00:00:00.000Z",
      completedAt: "2026-08-18T00:00:03.000Z",
      durationMs: 3_000,
      exitCode,
      selectedCases,
    },
    provenance: {
      binary: { sha256: SHA_A, version: "nimbus 0.1.0" },
      manifest: { schemaVersion: 1, sha256: SHA_B },
      node: { version: "v24.0.0" },
      source: { beforeSha256: SHA_C, afterSha256: SHA_C, status: "matched" },
    },
    cases: cases ?? selectedCases.map((name) => caseRecord(name)),
    cleanup: {
      status: cleanup,
      exitCode: cleanup === "passed" ? 0 : 1,
      artifactRetained: cleanup === "failed",
      reason: cleanup === "passed" ? "run resources removed" : "cleanup failed",
    },
  };
}

async function schema_accepts_success_golden() {
  const report = reportFixture();
  assert.equal(validateReport(report), report);
}

async function schema_rejects_contradictions_and_credential_fields() {
  const missingAnchor = reportFixture();
  missingAnchor.cases[0].observed.anchors.pop();
  assert.throws(() => validateReport(missingAnchor), /must equal expectedAnchors/u);

  const credentialField = reportFixture();
  credentialField.run.adminToken = "must-not-survive";
  assert.throws(() => validateReport(credentialField), /forbidden credential field/u);
}

async function credential_redaction_is_recursive() {
  const secret = "fixture-sensitive-value";
  const value = redact({
    adminToken: secret,
    nested: [`Bearer ${secret}`, `https://user:${secret}@localhost/path`, `prefix-${secret}-suffix`],
  }, { sensitiveValues: [secret] });
  assert.equal(value.adminToken, "[REDACTED]");
  assert.deepEqual(value.nested, [
    "Bearer [REDACTED]",
    "https://[REDACTED]@localhost/path",
    "prefix-[REDACTED]-suffix",
  ]);
  assert.equal(JSON.stringify(value).includes(secret), false);
}

async function interrupted_atomic_write_preserves_canonical_file() {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-avr8-atomic-"));
  const output = path.join(root, "report.json");
  try {
    await writeJsonAtomically(output, { schemaVersion: 1, generation: 1 });
    await assert.rejects(
      writeJsonAtomically(output, { schemaVersion: 1, generation: 2 }, {
        beforeRename: async () => { throw new Error("injected interruption before rename"); },
      }),
      /injected interruption/u,
    );
    assert.deepEqual(JSON.parse(await fs.readFile(output, "utf8")), { schemaVersion: 1, generation: 1 });
    assert.deepEqual((await fs.readdir(root)).sort(), ["report.json"]);
    await writeJsonAtomically(output, { schemaVersion: 1, generation: 2 });
    assert.equal(JSON.parse(await fs.readFile(output, "utf8")).generation, 2);
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
}

async function report_order_follows_manifest_selection() {
  const manifestCases = [desired("first"), desired("second"), desired("third")];
  const records = [caseRecord("third"), caseRecord("first")];
  const ordered = deterministicCases(["first", "second", "third"], manifestCases, records);
  assert.deepEqual(ordered.map((item) => item.name), ["first", "second", "third"]);
  assert.equal(ordered[1].observed.status, "not-run");
}

async function success_junit_projection_is_deterministic() {
  const report = reportFixture();
  const first = junit(report);
  const second = junit(structuredClone(report));
  assert.equal(first, second);
  assert.match(first, /tests="5" failures="0" skipped="0"/u);
  assert.match(first, /name="run\.outcome"/u);
  assert.match(first, /name="run\.source"/u);
  assert.match(first, /name="run\.cleanup"/u);
  assert.doesNotMatch(first, /<failure/u);
}

async function failure_junit_projects_case_and_cleanup_truth() {
  const first = caseRecord("first", { status: "failed", exitCode: 7, anchors: ["first.create"] });
  const second = notRunRecord("second");
  const report = reportFixture({ status: "failed", exitCode: 7, cases: [first, second], cleanup: "failed" });
  report.provenance.source.afterSha256 = SHA_A;
  report.provenance.source.status = "mismatched";
  const xml = junit(report);
  assert.match(xml, /tests="5" failures="4" skipped="1"/u);
  assert.match(xml, /case exited 7/u);
  assert.match(xml, /verification runner exited 7/u);
  assert.match(xml, /source bytes changed during verification/u);
  assert.match(xml, /cleanup failed/u);
  assert.match(xml, /case did not run/u);
}

async function smoke_anchor_parser_keeps_only_contract_lines() {
  assert.deepEqual(
    anchorsFromSmokeOutput("noise\nPASS tasks.create\nPASS tasks.list — observed after 2 reads\n"),
    ["tasks.create", "tasks.list"],
  );
  assert.throws(
    () => anchorsFromSmokeOutput("PASS tasks.create\nPASS tasks.create\n"),
    /duplicate/u,
  );
}

const tests = [
  schema_accepts_success_golden,
  schema_rejects_contradictions_and_credential_fields,
  credential_redaction_is_recursive,
  interrupted_atomic_write_preserves_canonical_file,
  report_order_follows_manifest_selection,
  success_junit_projection_is_deterministic,
  failure_junit_projects_case_and_cleanup_truth,
  smoke_anchor_parser_keeps_only_contract_lines,
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
