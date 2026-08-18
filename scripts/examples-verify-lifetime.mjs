#!/usr/bin/env node

import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const SHELL_DELIMITER = "|";
const ARTIFACT_MARKER = ".nimbus-artifact.json";

function invariant(condition, message) {
  if (!condition) throw new Error(message);
}

function safeSegment(value, label) {
  invariant(typeof value === "string" && value.length > 0, `${label} must be a non-empty string`);
  invariant(!value.includes("\0") && !value.includes("\n"), `${label} contains an unsafe byte`);
  const normalized = value
    .normalize("NFKD")
    .replaceAll(/[^A-Za-z0-9._-]+/gu, "-")
    .replaceAll(/^-+|-+$/gu, "")
    .toLowerCase();
  invariant(normalized.length > 0, `${label} does not contain a usable path segment`);
  return normalized;
}

function absolute(candidate, label) {
  invariant(typeof candidate === "string" && candidate.length > 0, `${label} is required`);
  invariant(path.isAbsolute(candidate), `${label} must be absolute: ${candidate}`);
  return path.resolve(candidate);
}

async function makeOwnerOnly(directory) {
  await fs.mkdir(directory, { recursive: true, mode: 0o700 });
  if (process.platform !== "win32") await fs.chmod(directory, 0o700);
}

async function pathExists(candidate) {
  try {
    await fs.lstat(candidate);
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}

function runContextFromPaths({ runRoot, artifactRoot, repoRoot }) {
  const resolvedRunRoot = absolute(runRoot, "run root");
  return {
    schemaVersion: 1,
    runRoot: resolvedRunRoot,
    networkStateRoot: path.join(resolvedRunRoot, "network-authority"),
    casesRoot: path.join(resolvedRunRoot, "cases"),
    workspaceRoot: path.join(resolvedRunRoot, "workspaces"),
    artifactRoot: absolute(artifactRoot, "artifact root"),
    repoRoot: absolute(repoRoot, "repository root"),
  };
}

export async function createRunContext({ repoRoot, tempRoot = os.tmpdir(), artifactRoot } = {}) {
  const resolvedRepoRoot = absolute(repoRoot, "repository root");
  const resolvedTempRoot = absolute(tempRoot, "temporary root");
  const resolvedArtifactRoot = artifactRoot
    ? absolute(artifactRoot, "artifact root")
    : path.join(resolvedRepoRoot, "target", "examples-verify-artifacts");
  await fs.mkdir(resolvedTempRoot, { recursive: true });
  const runRoot = await fs.mkdtemp(path.join(resolvedTempRoot, "nimbus-examples-verify."));
  await makeOwnerOnly(runRoot);
  const context = runContextFromPaths({
    runRoot,
    artifactRoot: resolvedArtifactRoot,
    repoRoot: resolvedRepoRoot,
  });
  await Promise.all([
    makeOwnerOnly(context.networkStateRoot),
    makeOwnerOnly(context.casesRoot),
    makeOwnerOnly(context.workspaceRoot),
  ]);
  await fs.writeFile(
    path.join(runRoot, "lifetime.json"),
    `${JSON.stringify(context, null, 2)}\n`,
    { mode: 0o600 },
  );
  return context;
}

export async function createCaseContext(run, { name, workspace }) {
  invariant(run?.schemaVersion === 1, "run context schemaVersion must be 1");
  const caseId = safeSegment(name, "case name");
  const workspaceId = safeSegment(workspace, "workspace");
  const caseRoot = path.join(run.casesRoot, caseId);
  const operatorRoot = path.join(caseRoot, "operator");
  const homeRoot = path.join(operatorRoot, "home");
  const authRoot = path.join(operatorRoot, "auth");
  const discoveryRoot = path.join(operatorRoot, "discovery");
  const auditRoot = path.join(operatorRoot, "audit");
  const configRoot = path.join(operatorRoot, "config");
  const windowsRoot = path.join(operatorRoot, "windows-local-app-data");
  const appRoot = path.join(run.workspaceRoot, workspaceId);
  const dataRoot = path.join(caseRoot, "data");
  const controlRoot = path.join(caseRoot, "control");
  const logRoot = path.join(caseRoot, "logs");
  const resultRoot = path.join(caseRoot, "results");
  const processRoot = path.join(caseRoot, "processes");
  try {
    await fs.mkdir(caseRoot, { mode: 0o700 });
  } catch (error) {
    if (error?.code === "EEXIST") {
      throw new Error(`case root already exists for ${name}: ${caseRoot}`, { cause: error });
    }
    throw error;
  }
  if (process.platform !== "win32") await fs.chmod(caseRoot, 0o700);
  await Promise.all([
    homeRoot,
    authRoot,
    discoveryRoot,
    auditRoot,
    configRoot,
    windowsRoot,
    dataRoot,
    controlRoot,
    logRoot,
    resultRoot,
    processRoot,
  ].map(makeOwnerOnly));

  const context = {
    schemaVersion: 1,
    name,
    caseId,
    caseRoot,
    homeRoot,
    authRoot,
    discoveryRoot,
    discoveryPath: path.join(discoveryRoot, "nimbus", "server.json"),
    auditRoot,
    configRoot,
    windowsRoot,
    appRoot,
    dataRoot,
    controlRoot,
    logRoot,
    resultRoot,
    processRoot,
    networkStateRoot: run.networkStateRoot,
  };
  context.environment = {
    HOME: homeRoot,
    TMPDIR: discoveryRoot,
    XDG_CONFIG_HOME: configRoot,
    XDG_DATA_HOME: authRoot,
    XDG_STATE_HOME: auditRoot,
    XDG_RUNTIME_DIR: discoveryRoot,
    LOCALAPPDATA: windowsRoot,
    USERPROFILE: homeRoot,
    NIMBUS_NETWORK_STATE_DIR: run.networkStateRoot,
    NIMBUS_DATA_DIR: dataRoot,
    NIMBUS_CONTROL_DATA_DIR: controlRoot,
  };
  await fs.writeFile(
    path.join(caseRoot, "context.json"),
    `${JSON.stringify(context, null, 2)}\n`,
    { mode: 0o600 },
  );
  return context;
}

export async function readCaseDiscovery(discoveryPath, expectedPid) {
  const resolved = absolute(discoveryPath, "discovery path");
  invariant(Number.isSafeInteger(expectedPid) && expectedPid > 0, "expected pid must be positive");
  let record;
  try {
    record = JSON.parse(await fs.readFile(resolved, "utf8"));
  } catch (error) {
    throw new Error(`cannot read case discovery ${resolved}: ${error.message}`, { cause: error });
  }
  invariant(record && typeof record === "object", "case discovery must be an object");
  invariant(record.pid === expectedPid, `case discovery belongs to pid ${record.pid}, expected ${expectedPid}`);
  invariant(typeof record.address === "string", "case discovery address must be a string");
  const address = new URL(`http://${record.address}`);
  invariant(
    ["127.0.0.1", "::1", "[::1]", "localhost"].includes(address.hostname),
    `case discovery address must be loopback: ${record.address}`,
  );
  invariant(Number(address.port) > 0 && Number(address.port) <= 65_535, `case discovery port is invalid: ${record.address}`);
  return { ...record, address: record.address, url: address.origin };
}

export async function requestGracefulShutdown(serverUrl, adminToken) {
  invariant(typeof adminToken === "string" && adminToken.length > 0, "admin token is required");
  invariant(adminToken.length <= 16_384 && !adminToken.includes("\0"), "admin token is invalid");
  const endpoint = new URL("/api/system/shutdown", serverUrl);
  invariant(endpoint.protocol === "http:", "examples verification shutdown requires local HTTP");
  invariant(
    ["127.0.0.1", "::1", "[::1]", "localhost"].includes(endpoint.hostname),
    "examples verification shutdown requires a loopback target",
  );
  const response = await fetch(endpoint, {
    method: "POST",
    headers: { "X-Nimbus-Admin-Token": adminToken },
    signal: AbortSignal.timeout(5_000),
  });
  invariant(response.ok, `graceful shutdown returned HTTP ${response.status}`);
}

async function readStdinSecret() {
  const chunks = [];
  let size = 0;
  for await (const chunk of process.stdin) {
    size += chunk.length;
    invariant(size <= 16_384, "stdin secret is too large");
    chunks.push(chunk);
  }
  return Buffer.concat(chunks).toString("utf8");
}

async function writeArtifactMarker(run) {
  await fs.writeFile(
    path.join(run.runRoot, ARTIFACT_MARKER),
    `${JSON.stringify({ schemaVersion: 1, sourceRunRoot: run.runRoot }, null, 2)}\n`,
    { mode: 0o600 },
  );
}

async function assertMatchingArtifact(destination, run) {
  let marker;
  try {
    marker = JSON.parse(await fs.readFile(path.join(destination, ARTIFACT_MARKER), "utf8"));
  } catch (error) {
    throw new Error(`cannot verify existing artifact destination ${destination}: ${error.message}`, { cause: error });
  }
  invariant(marker.schemaVersion === 1, `artifact destination has unsupported schemaVersion: ${destination}`);
  invariant(marker.sourceRunRoot === run.runRoot, `artifact destination belongs to another run: ${destination}`);
}

export async function moveToArtifacts(
  run,
  { rename = fs.rename, remove = fs.rm } = {},
) {
  await makeOwnerOnly(run.artifactRoot);
  const destination = path.join(run.artifactRoot, path.basename(run.runRoot));
  if (await pathExists(destination)) {
    await assertMatchingArtifact(destination, run);
    if (await pathExists(run.runRoot)) await remove(run.runRoot, { recursive: true });
    return destination;
  }
  await writeArtifactMarker(run);
  try {
    await rename(run.runRoot, destination);
  } catch (error) {
    if (error?.code !== "EXDEV") throw error;
    const staging = `${destination}.${process.pid}.${Date.now()}.stage`;
    try {
      await fs.cp(run.runRoot, staging, {
        recursive: true,
        verbatimSymlinks: true,
        force: false,
        errorOnExist: true,
      });
      await rename(staging, destination);
      await remove(run.runRoot, { recursive: true });
    } catch (copyError) {
      await fs.rm(staging, { recursive: true, force: true });
      throw copyError;
    }
  }
  return destination;
}

/// Settle one run root after all active process and listener owners are gone.
/// A primary failure becomes a retained diagnostic artifact. A cleanup_failure
/// must retain its original root and return red so evidence is not erased.
export async function finalizeRunContext(run, { runStatus, cleanupStatus }) {
  invariant(Number.isInteger(runStatus) && runStatus >= 0, "run status must be a non-negative integer");
  invariant(Number.isInteger(cleanupStatus) && cleanupStatus >= 0, "cleanup status must be a non-negative integer");
  if (cleanupStatus !== 0) {
    return {
      status: cleanupStatus,
      retainedPath: run.runRoot,
      reason: "cleanup failure retained the original run root",
    };
  }
  try {
    if (runStatus !== 0) {
      return {
        status: runStatus,
        retainedPath: await moveToArtifacts(run),
        reason: "run failure retained diagnostic artifacts",
      };
    }
    await fs.rm(run.runRoot, { recursive: true });
    return { status: 0, retainedPath: null, reason: "run resources removed" };
  } catch (error) {
    return {
      status: 1,
      retainedPath: run.runRoot,
      reason: `cleanup failure retained the original run root: ${error.message}`,
    };
  }
}

function shellRow(values) {
  for (const value of values) {
    invariant(typeof value === "string", "shell row values must be strings");
    invariant(!value.includes(SHELL_DELIMITER) && !value.includes("\n"), `path is not shell-safe: ${value}`);
  }
  return values.join(SHELL_DELIMITER);
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

async function main(args) {
  const command = args[0];
  if (command === "create-run") {
    const context = await createRunContext({
      repoRoot: option(args, "--repo-root"),
      tempRoot: option(args, "--temp-root", { required: false }) ?? os.tmpdir(),
      artifactRoot: option(args, "--artifact-root", { required: false }) ?? undefined,
    });
    console.log(shellRow([context.runRoot, context.networkStateRoot, context.artifactRoot]));
    return;
  }
  if (command === "create-case") {
    const run = runContextFromPaths({
      runRoot: option(args, "--run-root"),
      artifactRoot: option(args, "--artifact-root"),
      repoRoot: option(args, "--repo-root"),
    });
    const context = await createCaseContext(run, {
      name: option(args, "--name"),
      workspace: option(args, "--workspace"),
    });
    console.log(shellRow([
      context.caseRoot,
      context.homeRoot,
      context.authRoot,
      context.discoveryRoot,
      context.discoveryPath,
      context.auditRoot,
      context.configRoot,
      context.windowsRoot,
      context.appRoot,
      context.dataRoot,
      context.controlRoot,
      context.logRoot,
      context.resultRoot,
      context.processRoot,
    ]));
    return;
  }
  if (command === "read-discovery") {
    const record = await readCaseDiscovery(
      option(args, "--path"),
      Number(option(args, "--pid")),
    );
    console.log(record.url);
    return;
  }
  if (command === "finalize") {
    const run = runContextFromPaths({
      runRoot: option(args, "--run-root"),
      artifactRoot: option(args, "--artifact-root"),
      repoRoot: option(args, "--repo-root"),
    });
    const result = await finalizeRunContext(run, {
      runStatus: Number(option(args, "--run-status")),
      cleanupStatus: Number(option(args, "--cleanup-status")),
    });
    if (result.retainedPath) console.error(`${result.reason}: ${result.retainedPath}`);
    process.exitCode = result.status;
    return;
  }
  if (command === "shutdown") {
    await requestGracefulShutdown(option(args, "--url"), await readStdinSecret());
    return;
  }
  throw new Error("usage: examples-verify-lifetime.mjs create-run|create-case|read-discovery|shutdown|finalize ...");
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(`examples verification lifetime error: ${error.message}`);
    process.exitCode = 1;
  });
}
