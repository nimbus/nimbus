#!/usr/bin/env node

import { createHash } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
import fsSync from "node:fs";
import fs from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

function invariant(condition, message) {
  if (!condition) throw new Error(message);
}

function parseEnvironmentEntry(entry) {
  const separator = entry.indexOf("=");
  invariant(separator > 0, `environment entry must be KEY=value: ${entry}`);
  const key = entry.slice(0, separator);
  invariant(/^[A-Za-z_][A-Za-z0-9_]*$/u.test(key), `environment key is invalid: ${key}`);
  const value = entry.slice(separator + 1);
  invariant(!value.includes("\0") && !value.includes("\n") && !value.includes("\r"), `environment value is invalid: ${key}`);
  return [key, value];
}

function childEnvironment(environment, clearPrefixes) {
  const result = { ...process.env };
  for (const key of Object.keys(result)) {
    if (clearPrefixes.some((prefix) => key.startsWith(prefix))) delete result[key];
  }
  for (const [key, value] of Object.entries(environment)) result[key] = String(value);
  return result;
}

async function writeJsonAtomically(filePath, value) {
  await fs.mkdir(path.dirname(filePath), { recursive: true, mode: 0o700 });
  const temporary = `${filePath}.${process.pid}.${Date.now()}.tmp`;
  await fs.writeFile(temporary, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  await fs.rename(temporary, filePath);
}

export async function isManagedProcessLive(pid) {
  if (!Number.isSafeInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    if (error?.code === "ESRCH") return false;
    if (error?.code === "EPERM") return true;
    throw error;
  }
}

async function isProcessGroupLive(pid) {
  if (process.platform === "win32") return isManagedProcessLive(pid);
  try {
    process.kill(-pid, 0);
    return true;
  } catch (error) {
    if (error?.code === "ESRCH") return false;
    if (error?.code === "EPERM") return true;
    throw error;
  }
}

async function readProcessRecord(recordPath, { missing = "error" } = {}) {
  let record;
  try {
    record = JSON.parse(await fs.readFile(recordPath, "utf8"));
  } catch (error) {
    if (error?.code === "ENOENT" && missing === "null") return null;
    throw new Error(`cannot read managed process record ${recordPath}: ${error.message}`, { cause: error });
  }
  invariant(record.schemaVersion === 1, `managed process record has unsupported schemaVersion at ${recordPath}`);
  invariant(Number.isSafeInteger(record.pid) && record.pid > 0, `managed process record has invalid pid at ${recordPath}`);
  return record;
}

export async function isManagedRecordLive(recordPath) {
  const record = await readProcessRecord(recordPath, { missing: "null" });
  return record ? isProcessGroupLive(record.pid) : false;
}

async function waitForStop(pid, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (!await isProcessGroupLive(pid)) return true;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  return !await isProcessGroupLive(pid);
}

async function signalGroup(pid, signal) {
  if (process.platform === "win32") {
    const args = ["/pid", String(pid), "/t"];
    if (signal === "SIGKILL") args.push("/f");
    const result = spawnSync("taskkill", args, { encoding: "utf8" });
    if (result.status !== 0 && await isManagedProcessLive(pid)) {
      throw new Error(`taskkill failed for pid ${pid}: ${result.stderr.trim()}`);
    }
    return;
  }
  process.kill(-pid, signal);
}

async function failAfterUnrecordedSpawn(pid, primary) {
  let cleanupError;
  try {
    await signalGroup(pid, "SIGTERM");
    if (!await waitForStop(pid, 2_000)) {
      await signalGroup(pid, "SIGKILL");
      if (!await waitForStop(pid, 2_000)) {
        throw new Error(`unrecorded managed process group ${pid} remained live`);
      }
    }
  } catch (error) {
    cleanupError = error;
  }
  if (cleanupError) {
    const failure = new AggregateError(
      [primary, cleanupError],
      `failed after spawning unrecorded managed process ${pid} and failed to settle it`,
    );
    failure.processPid = pid;
    failure.processSettled = false;
    throw failure;
  }
  const failure = new Error(
    `failed after spawning unrecorded managed process ${pid}; the process was settled: ${primary.message}`,
    { cause: primary },
  );
  failure.processPid = pid;
  failure.processSettled = true;
  throw failure;
}

export async function spawnManagedProcess({
  recordPath,
  logPath,
  command,
  args = [],
  environment = {},
  clearPrefixes = [],
  writeRecord = writeJsonAtomically,
}) {
  invariant(path.isAbsolute(recordPath), "process record path must be absolute");
  invariant(path.isAbsolute(logPath), "process log path must be absolute");
  invariant(typeof command === "string" && command.length > 0, "process command is required");
  const existing = await readProcessRecord(recordPath, { missing: "null" });
  if (existing && await isProcessGroupLive(existing.pid)) {
    throw new Error(`managed process record is already live at ${recordPath}`);
  }
  await fs.mkdir(path.dirname(logPath), { recursive: true, mode: 0o700 });
  const log = fsSync.openSync(logPath, "a", 0o600);
  let child;
  let spawnError;
  try {
    child = spawn(command, args, {
      detached: true,
      env: childEnvironment(environment, clearPrefixes),
      stdio: ["ignore", log, log],
      windowsHide: true,
    });
    await new Promise((resolve, reject) => {
      child.once("spawn", resolve);
      child.once("error", reject);
    });
  } catch (error) {
    spawnError = error;
  } finally {
    try {
      fsSync.closeSync(log);
    } catch (error) {
      spawnError = spawnError
        ? new AggregateError([spawnError, error], "managed process spawn and log close both failed")
        : error;
    }
  }
  if (spawnError) {
    if (Number.isSafeInteger(child?.pid) && child.pid > 0) {
      await failAfterUnrecordedSpawn(child.pid, spawnError);
    }
    throw spawnError;
  }
  invariant(Number.isSafeInteger(child.pid) && child.pid > 0, "managed process did not report a pid");
  const record = {
    schemaVersion: 1,
    pid: child.pid,
    processGroup: process.platform === "win32" ? null : child.pid,
    startedAt: new Date().toISOString(),
    commandSha256: createHash("sha256").update(JSON.stringify([command, ...args])).digest("hex"),
  };
  try {
    await writeRecord(recordPath, record);
  } catch (primary) {
    await failAfterUnrecordedSpawn(child.pid, primary);
  }
  child.unref();
  return child.pid;
}

export async function stopManagedProcess(recordPath, { gracefulTimeoutMs = 5_000, killTimeoutMs = 2_000 } = {}) {
  const record = await readProcessRecord(recordPath, { missing: "null" });
  if (!record) return;
  if (await isProcessGroupLive(record.pid)) {
    try {
      await signalGroup(record.pid, "SIGTERM");
    } catch (error) {
      if (error?.code !== "ESRCH") throw error;
    }
  }
  if (!await waitForStop(record.pid, gracefulTimeoutMs)) {
    try {
      await signalGroup(record.pid, "SIGKILL");
    } catch (error) {
      if (error?.code !== "ESRCH") throw error;
    }
    if (!await waitForStop(record.pid, killTimeoutMs)) {
      throw new Error(`managed process group ${record.pid} remained live after SIGKILL`);
    }
  }
  await fs.rm(recordPath, { force: true });
}

export function execManagedProcess({ command, args = [], environment = {}, clearPrefixes = [], cwd }) {
  const result = spawnSync(command, args, {
    cwd,
    env: childEnvironment(environment, clearPrefixes),
    stdio: "inherit",
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.signal) throw new Error(`${command} terminated by ${result.signal}`);
  return result.status ?? 1;
}

function repeatedOptions(args, name) {
  const values = [];
  for (let index = 0; index < args.length; index += 1) {
    if (args[index] === name) {
      invariant(index + 1 < args.length, `${name} requires a value`);
      values.push(args[index + 1]);
      index += 1;
    }
  }
  return values;
}

function option(args, name) {
  const index = args.indexOf(name);
  invariant(index !== -1 && index + 1 < args.length, `${name} is required`);
  return args[index + 1];
}

function commandAfterSeparator(args) {
  const separator = args.indexOf("--");
  invariant(separator !== -1 && separator + 1 < args.length, "managed command must follow --");
  return [args[separator + 1], args.slice(separator + 2)];
}

async function environmentOptions(args) {
  const entries = repeatedOptions(args, "--env").map(parseEnvironmentEntry);
  for (const environmentPath of repeatedOptions(args, "--env-file")) {
    invariant(path.isAbsolute(environmentPath), `environment file path must be absolute: ${environmentPath}`);
    const stat = await fs.stat(environmentPath);
    invariant(stat.isFile(), `environment file must be a regular file: ${environmentPath}`);
    if (process.platform !== "win32") {
      invariant((stat.mode & 0o077) === 0, `environment file must be owner-only: ${environmentPath}`);
    }
    const contents = await fs.readFile(environmentPath, "utf8");
    invariant(Buffer.byteLength(contents) <= 65_536, `environment file is too large: ${environmentPath}`);
    for (const line of contents.split(/\r?\n/u)) {
      if (line.length > 0) entries.push(parseEnvironmentEntry(line));
    }
  }
  const environment = {};
  for (const [key, value] of entries) {
    invariant(!Object.hasOwn(environment, key), `environment key is duplicated: ${key}`);
    environment[key] = value;
  }
  return environment;
}

async function main(args) {
  const action = args[0];
  if (action === "spawn") {
    const [command, commandArgs] = commandAfterSeparator(args);
    const pid = await spawnManagedProcess({
      recordPath: option(args, "--record"),
      logPath: option(args, "--log"),
      command,
      args: commandArgs,
      environment: await environmentOptions(args),
      clearPrefixes: repeatedOptions(args, "--clear-prefix"),
    });
    console.log(pid);
    return;
  }
  if (action === "stop") {
    await stopManagedProcess(option(args, "--record"));
    return;
  }
  if (action === "status") {
    process.exitCode = await isManagedRecordLive(option(args, "--record")) ? 0 : 1;
    return;
  }
  if (action === "exec") {
    const [command, commandArgs] = commandAfterSeparator(args);
    process.exitCode = execManagedProcess({
      command,
      args: commandArgs,
      environment: await environmentOptions(args),
      clearPrefixes: repeatedOptions(args, "--clear-prefix"),
      cwd: option(args, "--cwd"),
    });
    return;
  }
  throw new Error("usage: examples-verify-supervisor.mjs spawn|stop|status|exec [--env-file PATH] ...");
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(`examples verification supervisor error: ${error.message}`);
    process.exitCode = 1;
  });
}
