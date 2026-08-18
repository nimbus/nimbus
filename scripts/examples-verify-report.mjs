#!/usr/bin/env node

import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { createReadStream } from "node:fs";
import fs from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { loadValidatedManifest } from "./examples-verify-workspace.mjs";

export const REPORT_SCHEMA_VERSION = 1;

const REPORT_KIND = "nimbus.examples-verification";
const SHA256_PATTERN = /^[a-f0-9]{64}$/u;
const FORBIDDEN_KEY_PATTERN = /(?:authorization|cookie|credential|password|secret|token|api[_-]?key)/iu;
const CASE_STATUSES = new Set(["passed", "failed", "not-run"]);
const CLEANUP_STATUSES = new Set(["passed", "failed", "not-run"]);

function invariant(condition, message) {
  if (!condition) throw new Error(message);
}

function object(value, label) {
  invariant(value !== null && typeof value === "object" && !Array.isArray(value), `${label} must be an object`);
  return value;
}

function exactKeys(value, keys, label) {
  object(value, label);
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  invariant(
    JSON.stringify(actual) === JSON.stringify(expected),
    `${label} keys must be ${expected.join(", ")}; got ${actual.join(", ")}`,
  );
}

function nonEmptyString(value, label) {
  invariant(typeof value === "string" && value.length > 0, `${label} must be a non-empty string`);
  invariant(!value.includes("\0"), `${label} contains a null byte`);
  return value;
}

function nonNegativeInteger(value, label) {
  invariant(Number.isSafeInteger(value) && value >= 0, `${label} must be a non-negative integer`);
  return value;
}

function validTimestamp(value, label) {
  nonEmptyString(value, label);
  invariant(Number.isFinite(Date.parse(value)), `${label} must be an ISO timestamp`);
  invariant(new Date(value).toISOString() === value, `${label} must use canonical ISO format`);
  return value;
}

function uniqueStrings(values, label) {
  invariant(Array.isArray(values), `${label} must be an array`);
  const seen = new Set();
  for (const value of values) {
    nonEmptyString(value, `${label} entry`);
    invariant(!seen.has(value), `${label} contains a duplicate: ${value}`);
    seen.add(value);
  }
  return values;
}

function safeSegment(value, label) {
  nonEmptyString(value, label);
  const normalized = value
    .normalize("NFKD")
    .replaceAll(/[^A-Za-z0-9._-]+/gu, "-")
    .replaceAll(/^-+|-+$/gu, "")
    .toLowerCase();
  invariant(normalized.length > 0, `${label} does not contain a usable path segment`);
  return normalized;
}

function absolute(value, label) {
  nonEmptyString(value, label);
  invariant(path.isAbsolute(value), `${label} must be absolute: ${value}`);
  return path.resolve(value);
}

function redactString(value, sensitiveValues) {
  let result = value;
  for (const sensitive of sensitiveValues) {
    if (sensitive.length >= 4) result = result.replaceAll(sensitive, "[REDACTED]");
  }
  result = result.replaceAll(/Bearer\s+[^\s"']+/giu, "Bearer [REDACTED]");
  result = result.replaceAll(/([A-Za-z][A-Za-z0-9+.-]*:\/\/)[^\s/@:]+:[^\s/@]+@/gu, "$1[REDACTED]@");
  return result;
}

export function redact(value, { sensitiveValues = [] } = {}) {
  const normalizedSensitive = sensitiveValues
    .filter((candidate) => typeof candidate === "string" && candidate.length >= 4)
    .sort((left, right) => right.length - left.length);
  if (Array.isArray(value)) return value.map((item) => redact(item, { sensitiveValues: normalizedSensitive }));
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [
      key,
      FORBIDDEN_KEY_PATTERN.test(key)
        ? "[REDACTED]"
        : redact(item, { sensitiveValues: normalizedSensitive }),
    ]));
  }
  return typeof value === "string" ? redactString(value, normalizedSensitive) : value;
}

function assertNoCredentialFields(value, label = "report") {
  if (Array.isArray(value)) {
    value.forEach((item, index) => assertNoCredentialFields(item, `${label}[${index}]`));
    return;
  }
  if (!value || typeof value !== "object") return;
  for (const [key, item] of Object.entries(value)) {
    invariant(!FORBIDDEN_KEY_PATTERN.test(key), `${label}.${key} is a forbidden credential field`);
    assertNoCredentialFields(item, `${label}.${key}`);
  }
}

function validateEndpoint(endpoint, label, { required }) {
  if (endpoint === null) {
    invariant(!required, `${label} is required for a passed case`);
    return;
  }
  exactKeys(endpoint, ["host", "port", "protocol"], label);
  invariant(["http", "https"].includes(endpoint.protocol), `${label}.protocol is invalid`);
  invariant(["127.0.0.1", "::1", "localhost"].includes(endpoint.host), `${label}.host must be loopback`);
  invariant(Number.isSafeInteger(endpoint.port) && endpoint.port > 0 && endpoint.port <= 65_535, `${label}.port is invalid`);
}

function validateCaseRecord(item, index) {
  const label = `cases[${index}]`;
  exactKeys(item, ["cleanup", "desired", "name", "observed"], label);
  nonEmptyString(item.name, `${label}.name`);

  exactKeys(item.desired, ["bootMode", "expectedAnchors", "surfaces", "updateSemantics"], `${label}.desired`);
  invariant(["dev", "start"].includes(item.desired.bootMode), `${label}.desired.bootMode is invalid`);
  uniqueStrings(item.desired.surfaces, `${label}.desired.surfaces`);
  invariant(["polling", "push", "request-response"].includes(item.desired.updateSemantics), `${label}.desired.updateSemantics is invalid`);
  uniqueStrings(item.desired.expectedAnchors, `${label}.desired.expectedAnchors`);

  exactKeys(
    item.observed,
    ["anchors", "completedAt", "durationMs", "endpoint", "exitCode", "startedAt", "status"],
    `${label}.observed`,
  );
  invariant(CASE_STATUSES.has(item.observed.status), `${label}.observed.status is invalid`);
  nonNegativeInteger(item.observed.exitCode, `${label}.observed.exitCode`);
  nonNegativeInteger(item.observed.durationMs, `${label}.observed.durationMs`);
  uniqueStrings(item.observed.anchors, `${label}.observed.anchors`);
  if (item.observed.status === "not-run") {
    invariant(item.observed.startedAt === null, `${label}.observed.startedAt must be null for not-run`);
    invariant(item.observed.completedAt === null, `${label}.observed.completedAt must be null for not-run`);
    invariant(item.observed.durationMs === 0, `${label}.observed.durationMs must be zero for not-run`);
  } else {
    validTimestamp(item.observed.startedAt, `${label}.observed.startedAt`);
    validTimestamp(item.observed.completedAt, `${label}.observed.completedAt`);
    invariant(
      Date.parse(item.observed.completedAt) >= Date.parse(item.observed.startedAt),
      `${label}.observed timestamps are reversed`,
    );
  }
  validateEndpoint(item.observed.endpoint, `${label}.observed.endpoint`, { required: item.observed.status === "passed" });
  const expectedPrefix = item.desired.expectedAnchors.slice(0, item.observed.anchors.length);
  invariant(
    JSON.stringify(item.observed.anchors) === JSON.stringify(expectedPrefix),
    `${label}.observed.anchors must follow the declared order`,
  );
  if (item.observed.status === "passed") {
    invariant(item.observed.exitCode === 0, `${label}.observed.exitCode must be zero for passed`);
    invariant(
      JSON.stringify(item.observed.anchors) === JSON.stringify(item.desired.expectedAnchors),
      `${label}.observed.anchors must equal expectedAnchors for passed`,
    );
  }

  exactKeys(item.cleanup, ["status"], `${label}.cleanup`);
  invariant(CLEANUP_STATUSES.has(item.cleanup.status), `${label}.cleanup.status is invalid`);
  if (item.observed.status === "passed") {
    invariant(item.cleanup.status === "passed", `${label}.cleanup.status must be passed for a passed case`);
  }
}

export function validateReport(report) {
  exactKeys(report, ["cases", "cleanup", "kind", "provenance", "run", "schemaVersion"], "report");
  invariant(report.schemaVersion === REPORT_SCHEMA_VERSION, `report schemaVersion must be ${REPORT_SCHEMA_VERSION}`);
  invariant(report.kind === REPORT_KIND, `report kind must be ${REPORT_KIND}`);
  assertNoCredentialFields(report);

  exactKeys(report.run, ["completedAt", "durationMs", "exitCode", "id", "selectedCases", "startedAt", "status"], "run");
  nonEmptyString(report.run.id, "run.id");
  invariant(["passed", "failed"].includes(report.run.status), "run.status is invalid");
  nonNegativeInteger(report.run.exitCode, "run.exitCode");
  nonNegativeInteger(report.run.durationMs, "run.durationMs");
  validTimestamp(report.run.startedAt, "run.startedAt");
  validTimestamp(report.run.completedAt, "run.completedAt");
  invariant(Date.parse(report.run.completedAt) >= Date.parse(report.run.startedAt), "run timestamps are reversed");
  uniqueStrings(report.run.selectedCases, "run.selectedCases");

  exactKeys(report.provenance, ["binary", "manifest", "node", "source"], "provenance");
  exactKeys(report.provenance.binary, ["sha256", "version"], "provenance.binary");
  invariant(SHA256_PATTERN.test(report.provenance.binary.sha256), "provenance.binary.sha256 is invalid");
  nonEmptyString(report.provenance.binary.version, "provenance.binary.version");
  exactKeys(report.provenance.manifest, ["schemaVersion", "sha256"], "provenance.manifest");
  invariant(report.provenance.manifest.schemaVersion === 1, "provenance.manifest.schemaVersion must be 1");
  invariant(SHA256_PATTERN.test(report.provenance.manifest.sha256), "provenance.manifest.sha256 is invalid");
  exactKeys(report.provenance.node, ["version"], "provenance.node");
  nonEmptyString(report.provenance.node.version, "provenance.node.version");
  exactKeys(report.provenance.source, ["afterSha256", "beforeSha256", "status"], "provenance.source");
  invariant(SHA256_PATTERN.test(report.provenance.source.beforeSha256), "provenance.source.beforeSha256 is invalid");
  invariant(SHA256_PATTERN.test(report.provenance.source.afterSha256), "provenance.source.afterSha256 is invalid");
  invariant(["matched", "mismatched"].includes(report.provenance.source.status), "provenance.source.status is invalid");
  invariant(
    report.provenance.source.status === (report.provenance.source.beforeSha256 === report.provenance.source.afterSha256 ? "matched" : "mismatched"),
    "provenance.source.status contradicts its digests",
  );

  invariant(Array.isArray(report.cases), "cases must be an array");
  invariant(report.cases.length === report.run.selectedCases.length, "cases must cover every selected case");
  report.cases.forEach(validateCaseRecord);
  invariant(
    JSON.stringify(report.cases.map((item) => item.name)) === JSON.stringify(report.run.selectedCases),
    "cases must use selectedCases order",
  );

  exactKeys(report.cleanup, ["artifactRetained", "exitCode", "reason", "status"], "cleanup");
  invariant(["passed", "failed"].includes(report.cleanup.status), "cleanup.status is invalid");
  nonNegativeInteger(report.cleanup.exitCode, "cleanup.exitCode");
  invariant(typeof report.cleanup.artifactRetained === "boolean", "cleanup.artifactRetained must be a boolean");
  nonEmptyString(report.cleanup.reason, "cleanup.reason");
  if (report.cleanup.status === "passed") invariant(report.cleanup.exitCode === 0, "cleanup.exitCode must be zero for passed");

  const shouldPass = report.run.exitCode === 0
    && report.cleanup.status === "passed"
    && report.provenance.source.status === "matched"
    && report.cases.every((item) => item.observed.status === "passed" && item.cleanup.status === "passed");
  invariant(report.run.status === (shouldPass ? "passed" : "failed"), "run.status contradicts report evidence");
  return report;
}

async function syncDirectory(directory) {
  let handle;
  try {
    handle = await fs.open(directory, "r");
    await handle.sync();
  } catch (error) {
    if (!["EINVAL", "ENOTSUP", "EISDIR", "EPERM"].includes(error?.code)) throw error;
  } finally {
    await handle?.close();
  }
}

export async function writeTextAtomically(filePath, contents, { beforeRename = async () => {} } = {}) {
  const resolved = absolute(filePath, "atomic output path");
  await fs.mkdir(path.dirname(resolved), { recursive: true, mode: 0o700 });
  const temporary = `${resolved}.${process.pid}.${Date.now()}.${Math.random().toString(16).slice(2)}.tmp`;
  let handle;
  try {
    handle = await fs.open(temporary, "wx", 0o600);
    await handle.writeFile(contents, "utf8");
    await handle.sync();
    await handle.close();
    handle = undefined;
    await beforeRename(temporary, resolved);
    await fs.rename(temporary, resolved);
    await syncDirectory(path.dirname(resolved));
  } catch (error) {
    await handle?.close().catch(() => undefined);
    await fs.rm(temporary, { force: true }).catch(() => undefined);
    throw error;
  }
}

export async function writeJsonAtomically(filePath, value, options = {}) {
  await writeTextAtomically(filePath, `${JSON.stringify(value, null, 2)}\n`, options);
}

async function writeJsonOnceAtomically(filePath, value) {
  const resolved = absolute(filePath, "single-write output path");
  await fs.mkdir(path.dirname(resolved), { recursive: true, mode: 0o700 });
  const temporary = `${resolved}.${process.pid}.${Date.now()}.${Math.random().toString(16).slice(2)}.tmp`;
  try {
    const handle = await fs.open(temporary, "wx", 0o600);
    try {
      await handle.writeFile(`${JSON.stringify(value, null, 2)}\n`, "utf8");
      await handle.sync();
    } finally {
      await handle.close();
    }
    await fs.link(temporary, resolved);
    await syncDirectory(path.dirname(resolved));
  } finally {
    await fs.rm(temporary, { force: true }).catch(() => undefined);
  }
}

async function readJson(filePath, label) {
  try {
    return JSON.parse(await fs.readFile(filePath, "utf8"));
  } catch (error) {
    throw new Error(`cannot read ${label} ${filePath}: ${error.message}`, { cause: error });
  }
}

async function pathExists(candidate) {
  return await fs.lstat(candidate).then(() => true, (error) => {
    if (error?.code === "ENOENT") return false;
    throw error;
  });
}

async function sha256File(filePath) {
  const digest = createHash("sha256");
  for await (const chunk of createReadStream(filePath)) digest.update(chunk);
  return digest.digest("hex");
}

function parseEndpoint(value) {
  if (!value) return null;
  const endpoint = new URL(value);
  invariant(endpoint.username === "" && endpoint.password === "", "endpoint must not contain credentials");
  invariant(endpoint.pathname === "/" && endpoint.search === "" && endpoint.hash === "", "endpoint must be an origin URL");
  const host = endpoint.hostname === "[::1]" ? "::1" : endpoint.hostname;
  const port = Number(endpoint.port || (endpoint.protocol === "https:" ? 443 : 80));
  const result = { protocol: endpoint.protocol.slice(0, -1), host, port };
  validateEndpoint(result, "endpoint", { required: true });
  return result;
}

export function anchorsFromSmokeOutput(output) {
  const anchors = [];
  for (const line of output.split(/\r?\n/u)) {
    const match = line.match(/^PASS ([A-Za-z0-9][A-Za-z0-9._-]*)(?: — .*)?$/u);
    if (match) anchors.push(match[1]);
  }
  return uniqueStrings(anchors, "observed anchors");
}

function desiredFromManifestCase(item) {
  return {
    bootMode: item.boot.mode,
    surfaces: [...item.surfaces],
    updateSemantics: item.updateSemantics,
    expectedAnchors: [...item.expectedAnchors],
  };
}

function notRunCase(item) {
  return {
    name: item.name,
    desired: desiredFromManifestCase(item),
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

function incompleteCase(item, start, completedAt, runExitCode) {
  exactKeys(start, ["name", "schemaVersion", "startedAt"], `case start ${item.name}`);
  invariant(start.schemaVersion === REPORT_SCHEMA_VERSION, `case start ${item.name} schema is invalid`);
  invariant(start.name === item.name, `case start ${item.name} has crossed identity`);
  validTimestamp(start.startedAt, `case start ${item.name}.startedAt`);
  validTimestamp(completedAt, `case start ${item.name}.completedAt`);
  return buildCaseRecord({
    manifestCase: item,
    startedAt: start.startedAt,
    completedAt,
    status: "failed",
    exitCode: runExitCode === 0 ? 1 : runExitCode,
    endpoint: null,
    smokeOutput: "",
    cleanupStatus: "failed",
  });
}

export function deterministicCases(
  selectedCases,
  manifestCases,
  caseRecords,
  { caseStarts = [], completedAt = null, runExitCode = 1 } = {},
) {
  const manifestByName = new Map(manifestCases.map((item) => [item.name, item]));
  const recordsByName = new Map(caseRecords.map((item) => [item.name, item]));
  const startsByName = new Map(caseStarts.map((item) => [item.name, item]));
  invariant(recordsByName.size === caseRecords.length, "case records contain a duplicate name");
  invariant(startsByName.size === caseStarts.length, "case starts contain a duplicate name");
  return selectedCases.map((name) => {
    const manifestCase = manifestByName.get(name);
    invariant(manifestCase, `selected case is not in the manifest: ${name}`);
    const record = recordsByName.get(name);
    if (record) return record;
    const start = startsByName.get(name);
    return start
      ? incompleteCase(manifestCase, start, completedAt, runExitCode)
      : notRunCase(manifestCase);
  });
}

export function buildCaseRecord({ manifestCase, startedAt, completedAt, status, exitCode, endpoint, smokeOutput, cleanupStatus }) {
  const record = {
    name: manifestCase.name,
    desired: desiredFromManifestCase(manifestCase),
    observed: {
      status,
      startedAt,
      completedAt,
      durationMs: Math.max(0, Date.parse(completedAt) - Date.parse(startedAt)),
      exitCode,
      endpoint: parseEndpoint(endpoint),
      anchors: anchorsFromSmokeOutput(smokeOutput),
    },
    cleanup: { status: cleanupStatus },
  };
  validateCaseRecord(record, 0);
  return record;
}

function xmlEscape(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}

export function junit(report) {
  validateReport(report);
  const testCount = report.cases.length + 3;
  const failures = report.cases.filter((item) => item.observed.status === "failed" || item.cleanup.status === "failed").length
    + (report.run.exitCode === 0 ? 0 : 1)
    + (report.provenance.source.status === "matched" ? 0 : 1)
    + (report.cleanup.status === "failed" ? 1 : 0);
  const skipped = report.cases.filter((item) => item.observed.status === "not-run").length;
  const seconds = (milliseconds) => (milliseconds / 1_000).toFixed(3);
  const lines = [
    '<?xml version="1.0" encoding="UTF-8"?>',
    `<testsuite name="nimbus.examples-verification" tests="${testCount}" failures="${failures}" skipped="${skipped}" time="${seconds(report.run.durationMs)}">`,
    "  <properties>",
    `    <property name="binary.sha256" value="${report.provenance.binary.sha256}"/>`,
    `    <property name="manifest.sha256" value="${report.provenance.manifest.sha256}"/>`,
    `    <property name="source.status" value="${report.provenance.source.status}"/>`,
    "  </properties>",
  ];
  for (const item of report.cases) {
    lines.push(`  <testcase classname="examples" name="${xmlEscape(item.name)}" time="${seconds(item.observed.durationMs)}">`);
    if (item.observed.status === "not-run") {
      lines.push('    <skipped message="case did not run"/>');
    } else if (item.observed.status === "failed" || item.cleanup.status === "failed") {
      const reason = item.cleanup.status === "failed" ? "case cleanup failed" : `case exited ${item.observed.exitCode}`;
      lines.push(`    <failure message="${xmlEscape(reason)}"/>`);
    }
    lines.push("  </testcase>");
  }
  lines.push('  <testcase classname="examples" name="run.outcome" time="0.000">');
  if (report.run.exitCode !== 0) {
    lines.push(`    <failure message="verification runner exited ${report.run.exitCode}"/>`);
  }
  lines.push("  </testcase>");
  lines.push('  <testcase classname="examples" name="run.source" time="0.000">');
  if (report.provenance.source.status !== "matched") {
    lines.push('    <failure message="source bytes changed during verification"/>');
  }
  lines.push("  </testcase>");
  lines.push('  <testcase classname="examples" name="run.cleanup" time="0.000">');
  if (report.cleanup.status === "failed") {
    lines.push(`    <failure message="${xmlEscape(report.cleanup.reason)}"/>`);
  }
  lines.push("  </testcase>", "</testsuite>", "");
  return lines.join("\n");
}

function option(args, name, { required = true } = {}) {
  const index = args.indexOf(name);
  if (index === -1) {
    if (required) throw new Error(`${name} is required`);
    return null;
  }
  invariant(index + 1 < args.length, `${name} requires a value`);
  return args[index + 1];
}

function integerOption(args, name) {
  const value = Number(option(args, name));
  return nonNegativeInteger(value, name);
}

async function readStdin(maxBytes = 65_536) {
  const chunks = [];
  let size = 0;
  for await (const chunk of process.stdin) {
    size += chunk.length;
    invariant(size <= maxBytes, "stdin input is too large");
    chunks.push(chunk);
  }
  return Buffer.concat(chunks).toString("utf8");
}

async function initRun(args) {
  const manifestPath = absolute(option(args, "--manifest"), "manifest path");
  const repoRoot = absolute(option(args, "--repo-root"), "repository root");
  const runRoot = absolute(option(args, "--run-root"), "run root");
  const resultsRoot = absolute(option(args, "--results-root"), "results root");
  const binaryPath = absolute(option(args, "--binary"), "binary path");
  const only = option(args, "--only", { required: false });
  const manifest = await loadValidatedManifest(manifestPath, repoRoot, { verifyInputs: false });
  const selectedCases = only ? manifest.cases.filter((item) => item.name === only) : manifest.cases;
  invariant(selectedCases.length > 0, `NIMBUS_EXAMPLES_VERIFY_ONLY=${only} matched no app in the manifest`);
  const runPathHash = createHash("sha256").update(runRoot).digest("hex").slice(0, 12);
  const id = `${safeSegment(path.basename(runRoot), "run id")}-${runPathHash}`;
  const resultRoot = path.join(resultsRoot, id);
  await fs.mkdir(resultsRoot, { recursive: true, mode: 0o700 });
  try {
    await fs.mkdir(resultRoot, { mode: 0o700 });
  } catch (error) {
    if (error?.code === "EEXIST") {
      throw new Error(`report result root already exists: ${resultRoot}`, { cause: error });
    }
    throw error;
  }
  await writeJsonOnceAtomically(path.join(resultRoot, "run-state.json"), {
    schemaVersion: REPORT_SCHEMA_VERSION,
    id,
    startedAt: new Date().toISOString(),
    selectedCases: selectedCases.map((item) => item.name),
    repoRoot,
    manifestPath,
    binaryPath,
  });
  console.log(resultRoot);
}

async function validateSelection(args) {
  const manifestPath = absolute(option(args, "--manifest"), "manifest path");
  const repoRoot = absolute(option(args, "--repo-root"), "repository root");
  const only = option(args, "--only");
  const manifest = await loadValidatedManifest(manifestPath, repoRoot, { verifyInputs: false });
  invariant(
    manifest.cases.some((item) => item.name === only),
    `NIMBUS_EXAMPLES_VERIFY_ONLY=${only} matched no app in the manifest`,
  );
}

async function beginCase(args) {
  const resultRoot = absolute(option(args, "--result-root"), "result root");
  const caseName = option(args, "--case");
  const statePath = path.join(resultRoot, "case-start", `${safeSegment(caseName, "case name")}.json`);
  try {
    await writeJsonOnceAtomically(statePath, { schemaVersion: REPORT_SCHEMA_VERSION, name: caseName, startedAt: new Date().toISOString() });
  } catch (error) {
    if (error?.code === "EEXIST") throw new Error(`case already started: ${caseName}`, { cause: error });
    throw error;
  }
}

async function recordCase(args) {
  const resultRoot = absolute(option(args, "--result-root"), "result root");
  const caseName = option(args, "--case");
  const runState = await readJson(path.join(resultRoot, "run-state.json"), "run state");
  const manifest = await loadValidatedManifest(runState.manifestPath, runState.repoRoot, { verifyInputs: false });
  const manifestCase = manifest.cases.find((item) => item.name === caseName);
  invariant(manifestCase, `case is not in the manifest: ${caseName}`);
  invariant(runState.selectedCases.includes(caseName), `case is not selected for this run: ${caseName}`);
  const caseId = safeSegment(caseName, "case name");
  const outputPath = path.join(resultRoot, "cases", `${caseId}.json`);
  const start = await readJson(path.join(resultRoot, "case-start", `${caseId}.json`), "case start");
  const smokeLog = option(args, "--smoke-log", { required: false });
  const smokeOutput = smokeLog && await pathExists(smokeLog) ? await fs.readFile(smokeLog, "utf8") : "";
  const record = buildCaseRecord({
    manifestCase,
    startedAt: start.startedAt,
    completedAt: new Date().toISOString(),
    status: option(args, "--status"),
    exitCode: integerOption(args, "--exit-code"),
    endpoint: option(args, "--endpoint", { required: false }),
    smokeOutput,
    cleanupStatus: option(args, "--cleanup-status"),
  });
  try {
    await writeJsonOnceAtomically(outputPath, record);
  } catch (error) {
    if (error?.code === "EEXIST") throw new Error(`case already recorded: ${caseName}`, { cause: error });
    throw error;
  }
}

async function binaryVersion(binaryPath) {
  const environment = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("NIMBUS_")));
  const result = spawnSync(binaryPath, ["--version"], { encoding: "utf8", env: environment, timeout: 10_000 });
  if (result.error) throw result.error;
  invariant(result.status === 0, `binary --version exited ${result.status}: ${result.stderr.trim()}`);
  const version = nonEmptyString(result.stdout.trim(), "binary version");
  invariant(version.length <= 512 && !version.includes("\n") && !version.includes("\r"), "binary version is invalid");
  return version;
}

async function stageSource(args) {
  const resultRoot = absolute(option(args, "--result-root"), "result root");
  const beforePath = absolute(option(args, "--source-before"), "source before path");
  const afterPath = absolute(option(args, "--source-after"), "source after path");
  const sourceStatus = option(args, "--source-status");
  invariant(["matched", "mismatched"].includes(sourceStatus), "source status must be matched or mismatched");
  const state = await readJson(path.join(resultRoot, "run-state.json"), "run state");
  const manifest = await readJson(state.manifestPath, "case manifest");
  const provenance = {
    binary: {
      sha256: await sha256File(state.binaryPath),
      version: await binaryVersion(state.binaryPath),
    },
    manifest: {
      schemaVersion: manifest.schemaVersion,
      sha256: await sha256File(state.manifestPath),
    },
    node: { version: process.version },
    source: {
      beforeSha256: await sha256File(beforePath),
      afterSha256: await sha256File(afterPath),
      status: sourceStatus,
    },
  };
  await writeJsonOnceAtomically(path.join(resultRoot, "provenance.json"), provenance);
}

async function finalizeReport(args) {
  const resultRoot = absolute(option(args, "--result-root"), "result root");
  const runExitCode = integerOption(args, "--run-exit-code");
  const lifetime = object(JSON.parse(await readStdin()), "lifetime result");
  nonNegativeInteger(lifetime.status, "lifetime.status");
  invariant(["passed", "failed"].includes(lifetime.cleanupStatus), "lifetime.cleanupStatus is invalid");
  invariant(typeof lifetime.reason === "string" && lifetime.reason.length > 0, "lifetime.reason is required");
  invariant(lifetime.retainedPath === null || typeof lifetime.retainedPath === "string", "lifetime.retainedPath is invalid");
  const state = await readJson(path.join(resultRoot, "run-state.json"), "run state");
  const manifest = await loadValidatedManifest(state.manifestPath, state.repoRoot, { verifyInputs: false });
  const provenance = await readJson(path.join(resultRoot, "provenance.json"), "provenance");
  const caseRecords = [];
  const caseStarts = [];
  const completedAt = new Date().toISOString();
  for (const name of state.selectedCases) {
    const caseId = safeSegment(name, "case name");
    const recordPath = path.join(resultRoot, "cases", `${caseId}.json`);
    if (await pathExists(recordPath)) caseRecords.push(await readJson(recordPath, "case result"));
    const startPath = path.join(resultRoot, "case-start", `${caseId}.json`);
    if (await pathExists(startPath)) {
      const start = await readJson(startPath, "case start");
      invariant(start.name === name, `case start ${name} has crossed identity`);
      caseStarts.push(start);
    }
  }
  const cases = deterministicCases(state.selectedCases, manifest.cases, caseRecords, {
    caseStarts,
    completedAt,
    runExitCode,
  });
  const cleanup = {
    status: lifetime.cleanupStatus,
    exitCode: lifetime.cleanupStatus === "passed" ? 0 : lifetime.status,
    artifactRetained: lifetime.retainedPath !== null,
    reason: lifetime.reason,
  };
  const shouldPass = runExitCode === 0
    && cleanup.status === "passed"
    && provenance.source.status === "matched"
    && cases.every((item) => item.observed.status === "passed" && item.cleanup.status === "passed");
  const report = redact({
    schemaVersion: REPORT_SCHEMA_VERSION,
    kind: REPORT_KIND,
    run: {
      id: state.id,
      status: shouldPass ? "passed" : "failed",
      startedAt: state.startedAt,
      completedAt,
      durationMs: Math.max(0, Date.parse(completedAt) - Date.parse(state.startedAt)),
      exitCode: runExitCode,
      selectedCases: [...state.selectedCases],
    },
    provenance,
    cases,
    cleanup,
  });
  validateReport(report);
  const reportPath = path.join(resultRoot, "report.json");
  const junitPath = path.join(resultRoot, "junit.xml");
  const junitText = junit(report);
  await writeJsonAtomically(reportPath, report);
  await writeTextAtomically(junitPath, junitText);
  console.log(`${reportPath}|${junitPath}`);
}

async function main(args) {
  const command = args[0];
  if (command === "validate-selection") return validateSelection(args.slice(1));
  if (command === "init") return initRun(args.slice(1));
  if (command === "begin-case") return beginCase(args.slice(1));
  if (command === "record-case") return recordCase(args.slice(1));
  if (command === "stage-source") return stageSource(args.slice(1));
  if (command === "finalize") return finalizeReport(args.slice(1));
  throw new Error("usage: examples-verify-report.mjs validate-selection|init|begin-case|record-case|stage-source|finalize ...");
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(`examples verification report error: ${error.message}`);
    process.exitCode = 1;
  });
}
