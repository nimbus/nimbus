#!/usr/bin/env node

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { junit, validateReport, writeJsonAtomically } from "./examples-verify-report.mjs";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..");
const RUNNER = path.join(SCRIPT_DIR, "examples-verify.sh");
const KIND = "nimbus.examples-verification-benchmark";
const RAW_KIND = "nimbus.examples-verification-benchmark-sample";

function invariant(condition, message) {
  if (!condition) throw new Error(message);
}

function positiveInteger(value, label) {
  const number = Number(value);
  invariant(Number.isSafeInteger(number) && number > 0, `${label} must be a positive integer`);
  return number;
}

function numberInRange(value, label, minimum, maximum) {
  const number = Number(value);
  invariant(Number.isFinite(number) && number >= minimum && number <= maximum, `${label} must be from ${minimum} through ${maximum}`);
  return number;
}

function option(args, name, fallback) {
  const index = args.indexOf(name);
  if (index === -1) return fallback;
  invariant(index + 1 < args.length, `${name} requires a value`);
  return args[index + 1];
}

function parseOptions(args) {
  const known = new Set(["--serial-samples", "--parallel-samples", "--max-seconds", "--parallelism"]);
  for (let index = 0; index < args.length; index += 2) {
    invariant(known.has(args[index]), `unknown benchmark option: ${args[index]}`);
    invariant(index + 1 < args.length, `${args[index]} requires a value`);
  }
  return {
    serialSamples: positiveInteger(option(args, "--serial-samples", "3"), "--serial-samples"),
    parallelSamples: positiveInteger(option(args, "--parallel-samples", "5"), "--parallel-samples"),
    maxSeconds: positiveInteger(option(args, "--max-seconds", "1200"), "--max-seconds"),
    parallelism: positiveInteger(option(args, "--parallelism", process.env.NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL ?? "5"), "--parallelism"),
    expectedCases: 9,
    expectedAnchors: 37,
  };
}

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])]));
  }
  return value;
}

function signature(value) {
  return createHash("sha256").update(JSON.stringify(canonical(value))).digest("hex");
}

export function hostFingerprint() {
  const cpus = os.cpus();
  const models = [...new Set(cpus.map((cpu) => cpu.model.trim()))].sort();
  return {
    hostname: os.hostname(),
    platform: process.platform,
    release: os.release(),
    architecture: process.arch,
    cpuCount: cpus.length,
    cpuModels: models,
    totalMemoryBytes: os.totalmem(),
    nodeVersion: process.version,
  };
}

function processCensus() {
  if (!new Set(["darwin", "linux"]).has(process.platform)) return [];
  const result = spawnSync("ps", ["-axo", "pid=,ppid=,command="], { encoding: "utf8", timeout: 5_000 });
  if (result.status !== 0) return [`process census unavailable: ${result.stderr.trim()}`];
  return result.stdout.split(/\r?\n/u)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .filter((line) => /(?:scripts\/examples-verify\.sh|target\/(?:debug|release)\/nimbus)(?:\s|$)/u.test(line));
}

export function hostActivity({ maxLoadPerCpu = 1.5, minFreeMemoryRatio = 0.05 } = {}) {
  const cpuCount = Math.max(1, os.cpus().length);
  const loadAverage = os.loadavg();
  const loadPerCpu = loadAverage[0] / cpuCount;
  const freeMemoryRatio = os.freemem() / Math.max(1, os.totalmem());
  const conflictingProcesses = processCensus();
  const reasons = [];
  if (loadPerCpu > maxLoadPerCpu) reasons.push(`one-minute load per CPU ${loadPerCpu.toFixed(3)} exceeds ${maxLoadPerCpu}`);
  if (freeMemoryRatio < minFreeMemoryRatio) reasons.push(`free-memory ratio ${freeMemoryRatio.toFixed(3)} is below ${minFreeMemoryRatio}`);
  if (conflictingProcesses.length > 0) reasons.push(`${conflictingProcesses.length} conflicting Nimbus verification processes are active`);
  return {
    observedAt: new Date().toISOString(),
    eligible: reasons.length === 0,
    reasons,
    loadAverage,
    loadPerCpu,
    freeMemoryBytes: os.freemem(),
    freeMemoryRatio,
    thresholds: { maxLoadPerCpu, minFreeMemoryRatio },
    conflictingProcesses,
  };
}

function median(values) {
  invariant(values.length > 0, "median requires at least one value");
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 1 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}

function coverageEvidence(report) {
  return report.cases.map((item) => ({
    name: item.name,
    desiredAnchors: item.desired.expectedAnchors,
    observedAnchors: item.observed.anchors,
    surfaces: item.desired.surfaces,
    updateSemantics: item.desired.updateSemantics,
  }));
}

function provenanceEvidence(report) {
  return {
    binary: report.provenance.binary,
    manifest: report.provenance.manifest,
    node: report.provenance.node,
    sourceBeforeSha256: report.provenance.source.beforeSha256,
  };
}

export function reportSampleEvidence(report) {
  validateReport(report);
  return {
    reportId: report.run.id,
    reportDurationMs: report.run.durationMs,
    reportStatus: report.run.status,
    cleanupStatus: report.cleanup.status,
    sourceStatus: report.provenance.source.status,
    coverage: coverageEvidence(report),
    provenance: provenanceEvidence(report),
    ports: report.cases.map((item) => item.observed.endpoint?.port).filter(Number.isSafeInteger),
  };
}

export function validateSampleEvidence(sample, report, junitText, label = "benchmark sample") {
  const expected = reportSampleEvidence(report);
  invariant(junitText === junit(report), `${label} JUnit contradicts its canonical report`);
  for (const [key, value] of Object.entries(expected)) {
    invariant(Object.hasOwn(sample, key), `${label}.${key} is missing`);
    invariant(signature(sample[key]) === signature(value), `${label}.${key} contradicts its canonical report`);
  }
  invariant(Number.isFinite(Date.parse(sample.startedAt)), `${label}.startedAt is invalid`);
  invariant(new Date(sample.startedAt).toISOString() === sample.startedAt, `${label}.startedAt is not canonical`);
  invariant(Number.isFinite(Date.parse(sample.completedAt)), `${label}.completedAt is invalid`);
  invariant(new Date(sample.completedAt).toISOString() === sample.completedAt, `${label}.completedAt is not canonical`);
  const derivedWallDurationMs = Date.parse(sample.completedAt) - Date.parse(sample.startedAt);
  invariant(derivedWallDurationMs >= 0, `${label} timestamps are reversed`);
  invariant(sample.wallDurationMs === derivedWallDurationMs, `${label}.wallDurationMs contradicts its timestamps`);
  invariant(sample.wallDurationMs >= report.run.durationMs, `${label}.wallDurationMs is shorter than its canonical report`);
  return sample;
}

function check(name, passed, evidence) {
  return { name, status: passed ? "passed" : "failed", evidence };
}

export function evaluateSamples(samples, {
  serialSamples,
  parallelSamples,
  maxSeconds,
  relativeLimit = 0.6,
  expectedCases = 9,
  expectedAnchors = 37,
}) {
  const valid = samples.filter((sample) => sample.validity.status === "valid");
  const invalid = samples.filter((sample) => sample.validity.status === "invalid");
  const serial = valid.filter((sample) => sample.mode === "serial");
  const parallel = valid.filter((sample) => sample.mode === "parallel");
  if (serial.length !== serialSamples || parallel.length !== parallelSamples) {
    return {
      status: "invalid",
      reason: `need ${serialSamples} serial and ${parallelSamples} parallel valid samples; got ${serial.length} and ${parallel.length}`,
      validSampleIds: valid.map((sample) => sample.id),
      invalidSampleIds: invalid.map((sample) => sample.id),
      checks: [],
      metrics: null,
    };
  }

  const baseline = serial[0];
  const baselineCoverage = signature(baseline.coverage);
  const baselineProvenance = signature(baseline.provenance);
  const allPassed = valid.every((sample) => sample.reportStatus === "passed" && sample.cleanupStatus === "passed" && sample.sourceStatus === "matched");
  const caseCount = baseline.coverage.length;
  const anchorCount = baseline.coverage.reduce((total, item) => total + item.observedAnchors.length, 0);
  const exactCoverage = caseCount === expectedCases && anchorCount === expectedAnchors;
  const stableCoverage = valid.every((sample) => signature(sample.coverage) === baselineCoverage);
  const stableProvenance = valid.every((sample) => signature(sample.provenance) === baselineProvenance);
  const isolatedPorts = valid.every((sample) => sample.ports.length === sample.coverage.length && new Set(sample.ports).size === sample.ports.length);
  const serialMedianMs = median(serial.map((sample) => sample.wallDurationMs));
  const parallelMedianMs = median(parallel.map((sample) => sample.wallDurationMs));
  const relativeBudgetMs = serialMedianMs * relativeLimit;
  const absoluteBudgetMs = maxSeconds * 1_000;
  const checks = [
    check("requested-valid-samples", true, `${serial.length} serial and ${parallel.length} parallel`),
    check("all-runs-pass", allPassed, valid.map((sample) => `${sample.id}:${sample.reportStatus}/${sample.cleanupStatus}/${sample.sourceStatus}`)),
    check("exact-case-and-anchor-count", exactCoverage, { caseCount, anchorCount, expectedCases, expectedAnchors }),
    check("coverage-and-order-match", stableCoverage, baselineCoverage),
    check("binary-manifest-node-source-match", stableProvenance, baselineProvenance),
    check("case-ports-are-distinct", isolatedPorts, valid.map((sample) => ({ id: sample.id, ports: sample.ports }))),
    check("parallel-relative-budget", parallelMedianMs <= relativeBudgetMs, { parallelMedianMs, serialMedianMs, relativeLimit, relativeBudgetMs }),
    check("parallel-absolute-budget", parallelMedianMs <= absoluteBudgetMs, { parallelMedianMs, absoluteBudgetMs }),
  ];
  return {
    status: checks.every((item) => item.status === "passed") ? "passed" : "failed",
    reason: checks.every((item) => item.status === "passed") ? "all benchmark checks passed" : "one or more benchmark checks failed",
    validSampleIds: valid.map((sample) => sample.id),
    invalidSampleIds: invalid.map((sample) => sample.id),
    checks,
    metrics: {
      serialDurationsMs: serial.map((sample) => sample.wallDurationMs),
      parallelDurationsMs: parallel.map((sample) => sample.wallDurationMs),
      serialMedianMs,
      parallelMedianMs,
      relativeRatio: parallelMedianMs / serialMedianMs,
      relativeLimit,
      absoluteBudgetMs,
      caseCount,
      anchorCount,
    },
  };
}

function reportPaths(stdout) {
  const match = stdout.match(/^(.+\/report\.json)\|(.+\/junit\.xml)$/mu);
  invariant(match, "examples verifier did not print canonical report paths");
  return { reportPath: match[1], junitPath: match[2] };
}

async function runSample({ id, mode, maxParallel, binary, benchmarkRoot, baselineFingerprint, maxSeconds, activity }) {
  const resultsRoot = path.join(benchmarkRoot, "runs", id);
  const startedMilliseconds = Date.now();
  const startedAt = new Date(startedMilliseconds).toISOString();
  const result = spawnSync("bash", [RUNNER], {
    cwd: REPO_ROOT,
    env: {
      ...process.env,
      NIMBUS_EXAMPLES_VERIFY_BIN: binary,
      NIMBUS_EXAMPLES_VERIFY_RESULTS_DIR: resultsRoot,
      NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL: String(maxParallel),
    },
    encoding: "utf8",
    timeout: (maxSeconds * 1_000) + 120_000,
    maxBuffer: 64 * 1024 * 1024,
  });
  const completedMilliseconds = Date.now();
  const completedAt = new Date(completedMilliseconds).toISOString();
  const wallDurationMs = completedMilliseconds - startedMilliseconds;
  if (result.error) throw result.error;
  invariant(result.status === 0, `${id} product verification failed with exit ${result.status}:\n${result.stdout}\n${result.stderr}`);
  const { reportPath, junitPath } = reportPaths(result.stdout);
  const relativeReportPath = path.relative(benchmarkRoot, reportPath);
  const relativeJunitPath = path.relative(benchmarkRoot, junitPath);
  invariant(!relativeReportPath.startsWith("..") && !path.isAbsolute(relativeReportPath), `${id} report escaped the benchmark root`);
  invariant(!relativeJunitPath.startsWith("..") && !path.isAbsolute(relativeJunitPath), `${id} JUnit escaped the benchmark root`);
  const report = validateReport(JSON.parse(await fs.readFile(reportPath, "utf8")));
  const junitText = await fs.readFile(junitPath, "utf8");
  invariant(report.run.status === "passed", `${id} report status is ${report.run.status}`);
  invariant(report.cleanup.status === "passed", `${id} cleanup status is ${report.cleanup.status}`);
  invariant(report.provenance.source.status === "matched", `${id} source status is ${report.provenance.source.status}`);
  const afterFingerprint = hostFingerprint();
  const fingerprintMatches = signature(afterFingerprint) === signature(baselineFingerprint);
  const sample = {
    schemaVersion: 1,
    kind: RAW_KIND,
    id,
    mode,
    maxParallel,
    startedAt,
    completedAt,
    wallDurationMs,
    validity: {
      status: activity.eligible && fingerprintMatches ? "valid" : "invalid",
      reasons: [...activity.reasons, ...(fingerprintMatches ? [] : ["host fingerprint changed during the sample"])],
      eligibilityMoment: "before-run",
    },
    host: { before: baselineFingerprint, after: afterFingerprint, activityBefore: activity, activityAfter: hostActivity(activity.thresholds) },
    reportPath: relativeReportPath,
    junitPath: relativeJunitPath,
    reportId: report.run.id,
    reportDurationMs: report.run.durationMs,
    reportStatus: report.run.status,
    cleanupStatus: report.cleanup.status,
    sourceStatus: report.provenance.source.status,
    coverage: coverageEvidence(report),
    provenance: provenanceEvidence(report),
    ports: report.cases.map((item) => item.observed.endpoint?.port).filter(Number.isSafeInteger),
  };
  validateSampleEvidence(sample, report, junitText, id);
  await writeJsonAtomically(path.join(benchmarkRoot, "raw", `${id}.json`), sample);
  return sample;
}

async function delay(milliseconds) {
  await new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function collectSamples(options, benchmarkRoot, binary) {
  const maxLoadPerCpu = numberInRange(process.env.NIMBUS_EXAMPLES_VERIFY_BENCHMARK_MAX_LOAD_PER_CPU ?? "1.5", "NIMBUS_EXAMPLES_VERIFY_BENCHMARK_MAX_LOAD_PER_CPU", 0.1, 10);
  const minFreeMemoryRatio = numberInRange(process.env.NIMBUS_EXAMPLES_VERIFY_BENCHMARK_MIN_FREE_MEMORY_RATIO ?? "0.05", "NIMBUS_EXAMPLES_VERIFY_BENCHMARK_MIN_FREE_MEMORY_RATIO", 0, 1);
  const maximumInvalidPreflights = positiveInteger(process.env.NIMBUS_EXAMPLES_VERIFY_BENCHMARK_MAX_INVALID_PREFLIGHTS ?? "12", "NIMBUS_EXAMPLES_VERIFY_BENCHMARK_MAX_INVALID_PREFLIGHTS");
  const waitMilliseconds = positiveInteger(process.env.NIMBUS_EXAMPLES_VERIFY_BENCHMARK_PREFLIGHT_WAIT_MS ?? "5000", "NIMBUS_EXAMPLES_VERIFY_BENCHMARK_PREFLIGHT_WAIT_MS");
  const baselineFingerprint = hostFingerprint();
  const invalidPreflights = [];
  const samples = [];
  const modes = [
    ...Array.from({ length: options.serialSamples }, (_, index) => ({ id: `serial-${String(index + 1).padStart(2, "0")}`, mode: "serial", maxParallel: 1 })),
    ...Array.from({ length: options.parallelSamples }, (_, index) => ({ id: `parallel-${String(index + 1).padStart(2, "0")}`, mode: "parallel", maxParallel: options.parallelism })),
  ];
  for (const requested of modes) {
    let activity;
    while (true) {
      const currentFingerprint = hostFingerprint();
      activity = hostActivity({ maxLoadPerCpu, minFreeMemoryRatio });
      const fingerprintMatches = signature(currentFingerprint) === signature(baselineFingerprint);
      if (activity.eligible && fingerprintMatches) break;
      invalidPreflights.push({
        id: `${requested.id}-preflight-${invalidPreflights.length + 1}`,
        requestedMode: requested.mode,
        observedAt: new Date().toISOString(),
        status: "invalid",
        reasons: [...activity.reasons, ...(fingerprintMatches ? [] : ["host fingerprint differs from the campaign baseline"])],
        fingerprint: currentFingerprint,
        activity,
      });
      if (invalidPreflights.length >= maximumInvalidPreflights) return { samples, invalidPreflights, baselineFingerprint, incomplete: true };
      await delay(waitMilliseconds);
    }
    console.log(`==> benchmark ${requested.id} max_parallel=${requested.maxParallel}`);
    samples.push(await runSample({
      ...requested,
      binary,
      benchmarkRoot,
      baselineFingerprint,
      maxSeconds: options.maxSeconds,
      activity,
    }));
  }
  return { samples, invalidPreflights, baselineFingerprint, incomplete: false };
}

async function validateEvidence(verdictPath) {
  const resolvedVerdict = path.resolve(verdictPath);
  const benchmarkRoot = path.dirname(resolvedVerdict);
  const verdict = JSON.parse(await fs.readFile(resolvedVerdict, "utf8"));
  invariant(verdict.schemaVersion === 1 && verdict.kind === KIND, "benchmark verdict kind or schema is invalid");
  const rawRoot = path.join(benchmarkRoot, "raw");
  const rawFiles = (await fs.readdir(rawRoot)).filter((name) => name.endsWith(".json")).sort((left, right) => {
    const leftMode = left.startsWith("serial-") ? 0 : 1;
    const rightMode = right.startsWith("serial-") ? 0 : 1;
    return leftMode - rightMode || left.localeCompare(right);
  });
  const expectedSamples = verdict.request.serialSamples + verdict.request.parallelSamples;
  invariant(rawFiles.length === expectedSamples, `expected ${expectedSamples} raw samples; got ${rawFiles.length}`);
  const samples = [];
  for (const name of rawFiles) {
    const sample = JSON.parse(await fs.readFile(path.join(rawRoot, name), "utf8"));
    invariant(sample.schemaVersion === 1 && sample.kind === RAW_KIND, `${name} kind or schema is invalid`);
    invariant(["serial", "parallel"].includes(sample.mode), `${name} mode is invalid`);
    invariant(["valid", "invalid"].includes(sample.validity?.status), `${name} validity is invalid`);
    const reportPath = path.resolve(benchmarkRoot, sample.reportPath);
    const junitPath = path.resolve(benchmarkRoot, sample.junitPath);
    invariant(reportPath.startsWith(`${benchmarkRoot}${path.sep}`), `${name} report escapes the evidence root`);
    invariant(junitPath.startsWith(`${benchmarkRoot}${path.sep}`), `${name} JUnit escapes the evidence root`);
    const report = validateReport(JSON.parse(await fs.readFile(reportPath, "utf8")));
    const junitText = await fs.readFile(junitPath, "utf8");
    validateSampleEvidence(sample, report, junitText, name);
    samples.push(sample);
  }
  const evaluation = evaluateSamples(samples, verdict.request);
  invariant(JSON.stringify(evaluation) === JSON.stringify(verdict.evaluation), "stored verdict does not match the raw samples");
  invariant(verdict.status === evaluation.status, "verdict status contradicts its evaluation");
  console.log(`validated ${samples.length} benchmark samples with verdict ${evaluation.status}`);
  return verdict;
}

async function main(args) {
  if (args[0] === "validate") {
    const verdictPath = option(args.slice(1), "--verdict", null);
    invariant(verdictPath, "validate requires --verdict");
    await validateEvidence(verdictPath);
    return;
  }
  const options = parseOptions(args);
  invariant(options.parallelism >= 2 && options.parallelism <= 9, "--parallelism must be from 2 through 9");
  const binary = path.resolve(process.env.NIMBUS_EXAMPLES_VERIFY_BIN ?? path.join(REPO_ROOT, "target", "debug", "nimbus"));
  await fs.access(binary);
  const rootParent = path.resolve(process.env.NIMBUS_EXAMPLES_VERIFY_BENCHMARK_DIR ?? path.join(REPO_ROOT, "target", "examples-verify-benchmarks"));
  await fs.mkdir(rootParent, { recursive: true, mode: 0o700 });
  const benchmarkRoot = await fs.mkdtemp(path.join(rootParent, "nimbus-examples-benchmark."));
  await fs.mkdir(path.join(benchmarkRoot, "raw"), { mode: 0o700 });
  const startedAt = new Date().toISOString();
  let collection;
  let evaluation;
  try {
    collection = await collectSamples(options, benchmarkRoot, binary);
    evaluation = evaluateSamples(collection.samples, options);
  } catch (error) {
    evaluation = { status: "failed", reason: error.message, validSampleIds: [], invalidSampleIds: [], checks: [], metrics: null };
  }
  const verdict = {
    schemaVersion: 1,
    kind: KIND,
    status: evaluation.status,
    startedAt,
    completedAt: new Date().toISOString(),
    benchmarkRoot,
    request: { ...options, relativeLimit: 0.6 },
    hostFingerprint: collection?.baselineFingerprint ?? hostFingerprint(),
    invalidPreflights: collection?.invalidPreflights ?? [],
    evaluation,
  };
  const verdictPath = path.join(benchmarkRoot, "verdict.json");
  await writeJsonAtomically(verdictPath, verdict);
  console.log(`==> benchmark verdict ${evaluation.status}: ${verdictPath}`);
  if (evaluation.metrics) {
    console.log(`    serial median ${evaluation.metrics.serialMedianMs} ms`);
    console.log(`    parallel median ${evaluation.metrics.parallelMedianMs} ms (${evaluation.metrics.relativeRatio.toFixed(3)}x)`);
  }
  if (evaluation.status === "invalid") process.exitCode = 2;
  else if (evaluation.status !== "passed") process.exitCode = 1;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(`examples verification benchmark error: ${error.message}`);
    process.exitCode = 1;
  });
}
