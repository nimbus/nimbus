#!/usr/bin/env node

import assert from "node:assert/strict";

import { evaluateSamples } from "./examples-verify-benchmark.mjs";

const request = {
  serialSamples: 3,
  parallelSamples: 5,
  maxSeconds: 1,
  relativeLimit: 0.6,
  expectedCases: 2,
  expectedAnchors: 2,
};

function sample(id, mode, wallDurationMs, overrides = {}) {
  return {
    id,
    mode,
    wallDurationMs,
    validity: { status: "valid", reasons: [] },
    reportStatus: "passed",
    cleanupStatus: "passed",
    sourceStatus: "matched",
    coverage: [
      { name: "first", desiredAnchors: ["first.pass"], observedAnchors: ["first.pass"], surfaces: ["native-http"], updateSemantics: "push" },
      { name: "second", desiredAnchors: ["second.pass"], observedAnchors: ["second.pass"], surfaces: ["convex-http"], updateSemantics: "polling" },
    ],
    provenance: {
      binary: { sha256: "a".repeat(64), version: "nimbus 1" },
      manifest: { schemaVersion: 1, sha256: "b".repeat(64) },
      node: { version: "v24.0.0" },
      sourceBeforeSha256: "c".repeat(64),
    },
    ports: mode === "serial" ? [10_001, 10_002] : [20_001, 20_002],
    ...overrides,
  };
}

function passingSamples() {
  return [
    sample("serial-01", "serial", 100),
    sample("serial-02", "serial", 110),
    sample("serial-03", "serial", 105),
    sample("parallel-01", "parallel", 50),
    sample("parallel-02", "parallel", 55),
    sample("parallel-03", "parallel", 60),
    sample("parallel-04", "parallel", 45),
    sample("parallel-05", "parallel", 50),
  ];
}

function passing_campaign_uses_medians_and_all_contracts() {
  const result = evaluateSamples(passingSamples(), request);
  assert.equal(result.status, "passed");
  assert.equal(result.metrics.serialMedianMs, 105);
  assert.equal(result.metrics.parallelMedianMs, 50);
  assert(result.checks.every((item) => item.status === "passed"));
}

function busy_or_different_host_sample_is_invalid_not_failed() {
  const samples = passingSamples();
  samples.push(sample("busy-attempt", "parallel", 20, {
    validity: { status: "invalid", reasons: ["host was busy"] },
    reportStatus: "failed",
  }));
  const result = evaluateSamples(samples, request);
  assert.equal(result.status, "passed");
  assert.deepEqual(result.invalidSampleIds, ["busy-attempt"]);

  const insufficient = evaluateSamples(samples.filter((item) => item.id !== "parallel-05"), request);
  assert.equal(insufficient.status, "invalid");
  assert.match(insufficient.reason, /need 3 serial and 5 parallel valid samples/u);
}

function coverage_or_order_drift_fails() {
  const samples = passingSamples();
  samples.at(-1).coverage = [...samples.at(-1).coverage].reverse();
  const result = evaluateSamples(samples, request);
  assert.equal(result.status, "failed");
  assert.equal(result.checks.find((item) => item.name === "coverage-and-order-match").status, "failed");
}

function relative_and_absolute_budgets_fail_independently() {
  const relative = passingSamples();
  for (const item of relative.filter((candidate) => candidate.mode === "parallel")) item.wallDurationMs = 70;
  const relativeResult = evaluateSamples(relative, request);
  assert.equal(relativeResult.checks.find((item) => item.name === "parallel-relative-budget").status, "failed");
  assert.equal(relativeResult.checks.find((item) => item.name === "parallel-absolute-budget").status, "passed");

  const absoluteRequest = { ...request, maxSeconds: 0.04 };
  const absoluteResult = evaluateSamples(passingSamples(), absoluteRequest);
  assert.equal(absoluteResult.checks.find((item) => item.name === "parallel-relative-budget").status, "passed");
  assert.equal(absoluteResult.checks.find((item) => item.name === "parallel-absolute-budget").status, "failed");
}

function duplicate_ports_fail_isolation() {
  const samples = passingSamples();
  samples[4].ports = [20_001, 20_001];
  const result = evaluateSamples(samples, request);
  assert.equal(result.status, "failed");
  assert.equal(result.checks.find((item) => item.name === "case-ports-are-distinct").status, "failed");
}

const tests = [
  passing_campaign_uses_medians_and_all_contracts,
  busy_or_different_host_sample_is_invalid_not_failed,
  coverage_or_order_drift_fails,
  relative_and_absolute_budgets_fail_independently,
  duplicate_ports_fail_isolation,
];

let failed = 0;
for (const test of tests) {
  try {
    test();
    console.log(`PASS ${test.name}`);
  } catch (error) {
    failed += 1;
    console.error(`FAIL ${test.name}: ${error.stack ?? error.message}`);
  }
}
console.log(`Summary: ${tests.length - failed} passed, ${failed} failed`);
if (failed > 0) process.exitCode = 1;
