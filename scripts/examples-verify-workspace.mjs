#!/usr/bin/env node

import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { constants as fsConstants } from "node:fs";
import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SHELL_DELIMITER = "|";
const LIST_DELIMITER = ",";
const UPDATE_SEMANTICS = new Set(["polling", "push", "request-response"]);
const BOOT_MODES = new Set(["dev", "start"]);
const SMOKE_COMMANDS = new Set(["node", "npm"]);
const SOURCE_HASH_CONCURRENCY = 128;
const SURFACE_ENV_KEYS = new Map([
  ["mongodb-wire", ["NIMBUS_MONGODB_URL"]],
  ["dynamodb-wire", [
    "NIMBUS_DYNAMODB_ENDPOINT",
    "NIMBUS_DYNAMODB_ACCESS_KEY_ID",
    "NIMBUS_DYNAMODB_SECRET_ACCESS_KEY",
  ]],
  ["s3-wire", [
    "NIMBUS_S3_ENDPOINT",
    "NIMBUS_S3_REGION",
    "NIMBUS_S3_ACCESS_KEY_ID",
    "NIMBUS_S3_SECRET_ACCESS_KEY",
  ]],
]);

function invariant(condition, message) {
  if (!condition) throw new Error(message);
}

function safeRelativePath(value, label) {
  invariant(typeof value === "string" && value.length > 0, `${label} must be a non-empty string`);
  const portable = value.replaceAll("\\", "/");
  invariant(portable === value, `${label} must use forward slashes: ${value}`);
  invariant(!value.includes(SHELL_DELIMITER) && !value.includes("\n"), `${label} is not shell-safe: ${value}`);
  invariant(!path.posix.isAbsolute(portable), `${label} must be relative: ${value}`);
  const normalized = path.posix.normalize(portable);
  invariant(normalized !== ".." && !normalized.startsWith("../"), `${label} escapes its root: ${value}`);
  invariant(normalized === portable, `${label} must be normalized: ${value}`);
  return normalized;
}

function uniqueStrings(values, label, { allowEmpty = false } = {}) {
  invariant(Array.isArray(values), `${label} must be an array`);
  if (!allowEmpty) invariant(values.length > 0, `${label} must not be empty`);
  const seen = new Set();
  for (const value of values) {
    invariant(typeof value === "string" && value.length > 0, `${label} entries must be non-empty strings`);
    invariant(!seen.has(value), `${label} contains duplicate entry: ${value}`);
    invariant(!value.includes(SHELL_DELIMITER) && !value.includes("\n"), `${label} entry is not shell-safe: ${value}`);
    seen.add(value);
  }
  return values;
}

function shellListStrings(values, label, options = {}) {
  uniqueStrings(values, label, options);
  for (const value of values) {
    invariant(!value.includes(LIST_DELIMITER), `${label} entry contains the list delimiter: ${value}`);
  }
  return values;
}

function environmentEntries(values, label) {
  shellListStrings(values, label, { allowEmpty: true });
  for (const value of values) {
    invariant(/^[A-Z][A-Z0-9_]*=.+$/u.test(value), `${label} entry must be KEY=value: ${value}`);
  }
}

function resolveWithin(root, relative, label) {
  const resolved = path.resolve(root, relative);
  const relation = path.relative(path.resolve(root), resolved);
  invariant(relation !== ".." && !relation.startsWith(`..${path.sep}`) && !path.isAbsolute(relation), `${label} escapes ${root}`);
  return resolved;
}

async function readManifest(manifestPath) {
  let value;
  try {
    value = JSON.parse(await fs.readFile(manifestPath, "utf8"));
  } catch (error) {
    throw new Error(`cannot read case manifest ${manifestPath}: ${error.message}`, { cause: error });
  }
  return value;
}

function isIgnoredAppPath(relativePath, ignoredPaths) {
  return ignoredPaths.some((ignored) => relativePath === ignored || relativePath.startsWith(`${ignored}/`));
}

async function walkFiles(root, ignoredPaths, relative = "") {
  const directory = resolveWithin(root, relative || ".", "application directory");
  const entries = await fs.readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries.sort((left, right) => left.name.localeCompare(right.name))) {
    const child = relative ? `${relative}/${entry.name}` : entry.name;
    if (isIgnoredAppPath(child, ignoredPaths)) continue;
    if (entry.isDirectory()) {
      files.push(...await walkFiles(root, ignoredPaths, child));
    } else {
      files.push(child);
    }
  }
  return files;
}

function validateCaseShape(item, index) {
  const label = `cases[${index}]`;
  invariant(item && typeof item === "object" && !Array.isArray(item), `${label} must be an object`);
  invariant(typeof item.name === "string" && item.name.length > 0, `${label}.name must be a non-empty string`);
  invariant(typeof item.workspace === "string" && item.workspace.length > 0, `${label}.workspace must be a non-empty string`);
  safeRelativePath(item.workspace, `${label}.workspace`);
  safeRelativePath(item.appDir, `${label}.appDir`);

  invariant(item.prepare && typeof item.prepare === "object", `${label}.prepare must be an object`);
  invariant(typeof item.prepare.codegen === "boolean", `${label}.prepare.codegen must be a boolean`);
  uniqueStrings(item.prepare.inputs, `${label}.prepare.inputs`);
  for (const input of item.prepare.inputs) safeRelativePath(input, `${label}.prepare.inputs`);

  invariant(item.boot && typeof item.boot === "object", `${label}.boot must be an object`);
  invariant(BOOT_MODES.has(item.boot.mode), `${label}.boot.mode must be dev or start`);
  invariant(typeof item.boot.needsAppDir === "boolean", `${label}.boot.needsAppDir must be a boolean`);
  environmentEntries(item.boot.environment, `${label}.boot.environment`);
  shellListStrings(item.boot.flags, `${label}.boot.flags`, { allowEmpty: true });

  invariant(item.smoke && typeof item.smoke === "object", `${label}.smoke must be an object`);
  invariant(SMOKE_COMMANDS.has(item.smoke.command), `${label}.smoke.command must be node or npm`);
  environmentEntries(item.smoke.environment, `${label}.smoke.environment`);
  invariant(typeof item.smoke.stdioContract === "boolean", `${label}.smoke.stdioContract must be a boolean`);

  shellListStrings(item.surfaces, `${label}.surfaces`);
  invariant(UPDATE_SEMANTICS.has(item.updateSemantics), `${label}.updateSemantics is invalid`);
  uniqueStrings(item.expectedAnchors, `${label}.expectedAnchors`);
  for (const anchor of item.expectedAnchors) {
    invariant(
      /^[A-Za-z0-9][A-Za-z0-9._-]*$/u.test(anchor),
      `${label}.expectedAnchors entry is invalid: ${anchor}`,
    );
  }
  invariant(!item.name.includes(SHELL_DELIMITER), `${label}.name is not shell-safe`);
  invariant(!item.workspace.includes(SHELL_DELIMITER), `${label}.workspace is not shell-safe`);
}

export async function validateManifestValue(value, { manifestPath = "<memory>", repoRoot, verifyInputs = true } = {}) {
  invariant(value && typeof value === "object" && !Array.isArray(value), "case manifest must be an object");
  invariant(value.schemaVersion === 1, "case manifest schemaVersion must be 1");
  invariant(Array.isArray(value.cases) && value.cases.length === 9, "case manifest must contain exactly nine cases");
  uniqueStrings(value.sourceGuardPaths, "sourceGuardPaths");
  uniqueStrings(value.ignoredAppPaths, "ignoredAppPaths");
  for (const guarded of value.sourceGuardPaths) safeRelativePath(guarded, "sourceGuardPaths");
  for (const ignored of value.ignoredAppPaths) safeRelativePath(ignored, "ignoredAppPaths");

  const names = new Set();
  const workspaces = new Set();
  const appDirs = new Set();
  for (const [index, item] of value.cases.entries()) {
    validateCaseShape(item, index);
    invariant(!names.has(item.name), `case name is duplicated: ${item.name}`);
    invariant(!workspaces.has(item.workspace), `case workspace is duplicated: ${item.workspace}`);
    invariant(!appDirs.has(item.appDir), `case appDir is duplicated: ${item.appDir}`);
    names.add(item.name);
    workspaces.add(item.workspace);
    appDirs.add(item.appDir);
  }

  if (verifyInputs) {
    invariant(repoRoot, `repoRoot is required to verify ${manifestPath}`);
    for (const item of value.cases) {
      const appRoot = resolveWithin(repoRoot, item.appDir, `${item.name}.appDir`);
      const comparePaths = (left, right) => left.localeCompare(right);
      const discovered = (await walkFiles(appRoot, value.ignoredAppPaths)).sort(comparePaths);
      const declared = [...item.prepare.inputs].sort(comparePaths);
      invariant(
        JSON.stringify(discovered) === JSON.stringify(declared),
        `${item.name} input declaration mismatch; declared=${declared.join(",")} discovered=${discovered.join(",")}`,
      );
    }
  }
  return value;
}

export async function loadValidatedManifest(manifestPath, repoRoot, options = {}) {
  const value = await readManifest(manifestPath);
  return validateManifestValue(value, { manifestPath, repoRoot, ...options });
}

function listField(values) {
  return values.length === 0 ? "-" : values.join(LIST_DELIMITER);
}

export function caseShellRows(manifest) {
  return manifest.cases.map((item) => [
    item.name,
    item.workspace,
    item.appDir,
    item.prepare.codegen ? "1" : "0",
    item.boot.needsAppDir ? "1" : "0",
    listField(item.boot.environment),
    listField(item.boot.flags),
    listField(item.smoke.environment),
    item.boot.mode,
    item.smoke.command,
    item.smoke.stdioContract ? "1" : "0",
    item.updateSemantics,
    listField(item.surfaces),
  ].join(SHELL_DELIMITER));
}

function parseNimbusOwnedEnv(content) {
  const values = new Map();
  for (const line of content.split(/\r?\n/u)) {
    if (!line.startsWith("NIMBUS_")) continue;
    const separator = line.indexOf("=");
    invariant(separator > 0, "Nimbus-owned .env.local entry must be KEY=value");
    const key = line.slice(0, separator);
    invariant(/^[A-Z][A-Z0-9_]*$/u.test(key), `invalid Nimbus-owned environment key: ${key}`);
    invariant(!values.has(key), `duplicate Nimbus-owned environment key: ${key}`);
    const value = line.slice(separator + 1);
    invariant(value.length > 0 && !value.includes("\0"), `${key} must have a non-empty value`);
    values.set(key, value);
  }
  return values;
}

function requireLoopbackEndpoint(value, key, protocols) {
  let endpoint;
  try {
    endpoint = new URL(value);
  } catch (error) {
    throw new Error(`${key} must be an absolute URL`, { cause: error });
  }
  invariant(protocols.includes(endpoint.protocol), `${key} has unsupported protocol ${endpoint.protocol}`);
  invariant(
    ["127.0.0.1", "::1", "[::1]", "localhost"].includes(endpoint.hostname),
    `${key} must name a loopback host`,
  );
  invariant(Number(endpoint.port) > 0 && Number(endpoint.port) <= 65_535, `${key} must name a non-zero port`);
  return endpoint;
}

export async function generatedCaseEnvironment({ manifestPath, repoRoot, caseName, destination }) {
  const manifest = await loadValidatedManifest(manifestPath, repoRoot);
  const item = manifest.cases.find((candidate) => candidate.name === caseName);
  invariant(item, `unknown case: ${caseName}`);
  const expected = item.surfaces.flatMap((surface) => SURFACE_ENV_KEYS.get(surface) ?? []);
  if (expected.length === 0) return [];

  const destinationRoot = path.resolve(destination);
  const envPath = resolveWithin(destinationRoot, ".env.local", `${caseName} generated environment`);
  const values = parseNimbusOwnedEnv(await fs.readFile(envPath, "utf8"));
  for (const key of expected) invariant(values.has(key), `${caseName} .env.local is missing ${key}`);

  const environment = new Map(expected.map((key) => [key, values.get(key)]));
  if (values.has("NIMBUS_MONGODB_URL")) {
    const endpoint = requireLoopbackEndpoint(
      values.get("NIMBUS_MONGODB_URL"),
      "NIMBUS_MONGODB_URL",
      ["mongodb:"],
    );
    invariant(endpoint.username.length > 0 && endpoint.password.length > 0, "NIMBUS_MONGODB_URL must include credentials");
    environment.set("NIMBUS_MONGODB_HOST", endpoint.hostname);
    environment.set("NIMBUS_MONGODB_PORT", endpoint.port);
    environment.set("NIMBUS_MONGODB_USERNAME", decodeURIComponent(endpoint.username));
    environment.set("NIMBUS_MONGODB_PASSWORD", decodeURIComponent(endpoint.password));
  }
  for (const key of ["NIMBUS_DYNAMODB_ENDPOINT", "NIMBUS_S3_ENDPOINT"]) {
    if (environment.has(key)) requireLoopbackEndpoint(environment.get(key), key, ["http:", "https:"]);
  }
  return [...environment.entries()].map(([key, value]) => `${key}=${value}`);
}

async function verifyDeclaredDependencies(sourceRoot, inputs, dependencyRoot, caseName) {
  for (const input of inputs.filter((candidate) => path.basename(candidate) === "package.json")) {
    const manifestPath = resolveWithin(sourceRoot, input, `${caseName} package manifest`);
    const packageManifest = JSON.parse(await fs.readFile(manifestPath, "utf8"));
    const dependencies = new Set([
      ...Object.keys(packageManifest.dependencies ?? {}),
      ...Object.keys(packageManifest.devDependencies ?? {}),
      ...Object.keys(packageManifest.peerDependencies ?? {}),
    ]);
    for (const dependency of [...dependencies].sort()) {
      const installed = resolveWithin(dependencyRoot, dependency, `${caseName} dependency`);
      const installedStat = await fs.stat(installed).catch(() => null);
      invariant(
        installedStat?.isDirectory(),
        `${caseName} dependency ${dependency} is missing at ${installed}; run npm ci`,
      );
    }
  }
}

async function linkDirectoryEntries(sourceDirectory, destinationDirectory) {
  await fs.mkdir(destinationDirectory, { recursive: true });
  const entries = await fs.readdir(sourceDirectory, { withFileTypes: true });
  for (const entry of entries) {
    if (entry.name === ".package-lock.json") continue;
    const source = path.join(sourceDirectory, entry.name);
    const destination = path.join(destinationDirectory, entry.name);
    await fs.symlink(source, destination, entry.isDirectory() ? "dir" : undefined);
  }
}

async function materializeDependencyLinks(dependencyRoot, destinationRoot) {
  const localNodeModules = path.join(destinationRoot, "node_modules");
  await fs.mkdir(localNodeModules);
  const entries = await fs.readdir(dependencyRoot, { withFileTypes: true });
  for (const entry of entries) {
    if (entry.name === ".package-lock.json") continue;
    const source = path.join(dependencyRoot, entry.name);
    const destination = path.join(localNodeModules, entry.name);
    if (entry.name === ".bin" || (entry.name.startsWith("@") && entry.isDirectory())) {
      await linkDirectoryEntries(source, destination);
    } else {
      await fs.symlink(source, destination, entry.isDirectory() ? "dir" : undefined);
    }
  }
  return localNodeModules;
}

export async function prepareCaseWorkspace({ manifestPath, repoRoot, caseName, destination }) {
  const manifest = await loadValidatedManifest(manifestPath, repoRoot);
  const item = manifest.cases.find((candidate) => candidate.name === caseName);
  invariant(item, `unknown case: ${caseName}`);

  const destinationRoot = path.resolve(destination);
  await fs.mkdir(path.dirname(destinationRoot), { recursive: true });
  await fs.mkdir(destinationRoot);
  const sourceRoot = resolveWithin(repoRoot, item.appDir, `${item.name}.appDir`);
  for (const input of item.prepare.inputs) {
    const source = resolveWithin(sourceRoot, input, `${item.name} input`);
    const target = resolveWithin(destinationRoot, input, `${item.name} destination`);
    const sourceStat = await fs.lstat(source);
    invariant(sourceStat.isFile() || sourceStat.isSymbolicLink(), `${item.name} input is not a file: ${input}`);
    await fs.mkdir(path.dirname(target), { recursive: true });
    if (sourceStat.isSymbolicLink()) {
      await fs.symlink(await fs.readlink(source), target);
    } else {
      await fs.copyFile(source, target);
      await fs.chmod(target, sourceStat.mode & 0o777);
    }
  }

  const dependencyRoot = path.join(repoRoot, "node_modules");
  const dependencyStat = await fs.stat(dependencyRoot).catch(() => null);
  invariant(dependencyStat?.isDirectory(), `node_modules is missing at ${dependencyRoot}; run npm ci`);
  const localNodeModules = await materializeDependencyLinks(dependencyRoot, destinationRoot);
  await verifyDeclaredDependencies(sourceRoot, item.prepare.inputs, localNodeModules, item.name);
  return destinationRoot;
}

async function provisionedPackageDirectories(packageRoot) {
  const packages = [];
  const entries = await fs.readdir(packageRoot, { withFileTypes: true });
  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const entryRoot = path.join(packageRoot, entry.name);
    if (entry.name.startsWith("@")) {
      const scoped = await fs.readdir(entryRoot, { withFileTypes: true });
      for (const child of scoped) {
        if (child.isDirectory()) packages.push({ expectedName: `${entry.name}/${child.name}`, root: path.join(entryRoot, child.name) });
      }
    } else {
      packages.push({ expectedName: entry.name, root: entryRoot });
    }
  }
  return packages;
}

export async function refreshProvisionedDependencies({ destination }) {
  const destinationRoot = path.resolve(destination);
  const localNodeModules = path.join(destinationRoot, "node_modules");
  const localStat = await fs.lstat(localNodeModules).catch(() => null);
  invariant(localStat?.isDirectory() && !localStat.isSymbolicLink(), `case node_modules must be an owned directory: ${localNodeModules}`);
  const packageRoot = path.join(destinationRoot, ".nimbus", "packages");
  const packageStat = await fs.stat(packageRoot).catch(() => null);
  if (!packageStat?.isDirectory()) return 0;

  let linked = 0;
  for (const provisioned of await provisionedPackageDirectories(packageRoot)) {
    const packageJson = path.join(provisioned.root, "package.json");
    const packageManifest = JSON.parse(await fs.readFile(packageJson, "utf8"));
    invariant(packageManifest.name === provisioned.expectedName, `provisioned package name mismatch at ${packageJson}`);
    const relativeName = safeRelativePath(packageManifest.name, "provisioned package name");
    const localPackage = resolveWithin(localNodeModules, relativeName, "local provisioned dependency");
    const localParent = path.dirname(localPackage);
    await fs.mkdir(localParent, { recursive: true });
    const parentStat = await fs.lstat(localParent);
    invariant(parentStat.isDirectory() && !parentStat.isSymbolicLink(), `dependency scope must be an owned directory: ${localParent}`);
    await fs.rm(localPackage, { recursive: true, force: true });
    await fs.symlink(provisioned.root, localPackage, "dir");
    linked += 1;
  }
  return linked;
}

function digest(value) {
  return createHash("sha256").update(value).digest("hex");
}

async function byteEntry(repoRoot, relativePath) {
  const absolute = resolveWithin(repoRoot, relativePath, "source byte path");
  const flags = fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW ?? 0);
  for (let attempt = 0; attempt < 3; attempt += 1) {
    try {
      const target = await fs.readlink(absolute);
      return { path: relativePath, kind: "symlink", sha256: digest(`symlink\0${target}`) };
    } catch (error) {
      if (error?.code === "ENOENT") {
        return { path: relativePath, kind: "missing", sha256: digest("missing\0") };
      }
      if (!["EINVAL", "UNKNOWN"].includes(error?.code)) throw error;
    }
    let handle;
    try {
      handle = await fs.open(absolute, flags);
    } catch (error) {
      if (error?.code === "ENOENT") {
        return { path: relativePath, kind: "missing", sha256: digest("missing\0") };
      }
      if (error?.code === "ELOOP") continue;
      throw error;
    }
    try {
      invariant((await handle.stat()).isFile(), `source byte path is not a file: ${relativePath}`);
      return { path: relativePath, kind: "file", sha256: digest(await handle.readFile()) };
    } finally {
      await handle.close();
    }
  }
  throw new Error(`source byte path changed type while reading: ${relativePath}`);
}

function git(repoRoot, args, { allowFailure = false } = {}) {
  const result = spawnSync("git", ["-C", repoRoot, ...args], {
    encoding: null,
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error) {
    throw new Error(`git ${args.join(" ")} could not start: ${result.error.message}`, { cause: result.error });
  }
  if (!allowFailure && result.status !== 0) {
    throw new Error(`git ${args.join(" ")} failed: ${result.stderr.toString("utf8").trim()}`);
  }
  return result;
}

function nulFields(buffer) {
  return buffer.toString("utf8").split("\0").filter(Boolean);
}

async function buildSourceSnapshot({ manifestPath, repoRoot }) {
  const manifest = await loadValidatedManifest(manifestPath, repoRoot);
  const probe = git(repoRoot, ["rev-parse", "--is-inside-work-tree"], { allowFailure: true });
  let mode;
  let sourcePaths;
  let indexSha256 = null;
  if (probe.status === 0 && probe.stdout.toString("utf8").trim() === "true") {
    mode = "git";
    indexSha256 = digest(git(repoRoot, ["ls-files", "--stage", "-z"]).stdout);
    sourcePaths = nulFields(git(repoRoot, [
      "diff",
      "--name-only",
      "--no-ext-diff",
      "--no-renames",
      "-z",
      "--",
    ]).stdout).sort();
  } else {
    mode = "export";
    const manifestRelative = path.relative(repoRoot, manifestPath).replaceAll(path.sep, "/");
    const protectedPaths = new Set([...manifest.sourceGuardPaths, manifestRelative]);
    for (const item of manifest.cases) {
      for (const input of item.prepare.inputs) protectedPaths.add(`${item.appDir}/${input}`);
    }
    sourcePaths = [...protectedPaths].sort();
  }
  const entries = new Array(sourcePaths.length);
  let cursor = 0;
  await Promise.all(Array.from(
    { length: Math.min(SOURCE_HASH_CONCURRENCY, sourcePaths.length) },
    async () => {
      while (cursor < sourcePaths.length) {
        const index = cursor;
        cursor += 1;
        entries[index] = await byteEntry(repoRoot, sourcePaths[index]);
      }
    },
  ));
  return { schemaVersion: 1, mode, indexSha256, entries };
}

export async function captureSourceByteManifest({ manifestPath, repoRoot, outputPath }) {
  const snapshot = await buildSourceSnapshot({ manifestPath, repoRoot });
  await fs.writeFile(outputPath, `${JSON.stringify(snapshot, null, 2)}\n`, { flag: "wx" });
  return snapshot;
}

function snapshotDifferences(expected, observed) {
  const differences = [];
  if (expected.schemaVersion !== 1 || observed.schemaVersion !== 1) differences.push("schema version changed");
  if (expected.mode !== observed.mode) differences.push(`repository mode changed: ${expected.mode} -> ${observed.mode}`);
  if (expected.indexSha256 !== observed.indexSha256) differences.push("Git index changed");
  const before = new Map(expected.entries.map((entry) => [entry.path, entry]));
  const after = new Map(observed.entries.map((entry) => [entry.path, entry]));
  for (const relativePath of [...new Set([...before.keys(), ...after.keys()])].sort()) {
    const left = before.get(relativePath);
    const right = after.get(relativePath);
    if (!left || !right || left.kind !== right.kind || left.sha256 !== right.sha256) {
      differences.push(`source bytes changed: ${relativePath}`);
    }
  }
  return differences;
}

export async function verifySourceByteManifest({ manifestPath, repoRoot, snapshotPath, observedOutputPath = null }) {
  const expected = JSON.parse(await fs.readFile(snapshotPath, "utf8"));
  const observed = await buildSourceSnapshot({ manifestPath, repoRoot });
  if (observedOutputPath) {
    await fs.writeFile(observedOutputPath, `${JSON.stringify(observed, null, 2)}\n`, { flag: "wx" });
  }
  const differences = snapshotDifferences(expected, observed);
  if (differences.length > 0) {
    throw new Error(`source byte manifest mismatch:\n  ${differences.join("\n  ")}`);
  }
  return observed;
}

function parseOptions(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    invariant(flag?.startsWith("--") && value !== undefined, `invalid option sequence at ${flag ?? "<end>"}`);
    options[flag.slice(2)] = value;
  }
  return options;
}

async function main() {
  const [command, ...argv] = process.argv.slice(2);
  const options = parseOptions(argv);
  const manifestPath = path.resolve(options.manifest ?? "scripts/examples-verify-cases.json");
  const repoRoot = path.resolve(options["repo-root"] ?? ".");
  if (command === "validate") {
    await loadValidatedManifest(manifestPath, repoRoot);
    console.log("validated 9 application verification cases");
    return;
  }
  if (command === "emit-shell") {
    const manifest = await loadValidatedManifest(manifestPath, repoRoot);
    for (const row of caseShellRows(manifest)) console.log(row);
    return;
  }
  if (command === "prepare") {
    invariant(options.case && options.destination, "prepare requires --case and --destination");
    const destination = await prepareCaseWorkspace({
      manifestPath,
      repoRoot,
      caseName: options.case,
      destination: options.destination,
    });
    console.log(destination);
    return;
  }
  if (command === "refresh-dependencies") {
    invariant(options.destination, "refresh-dependencies requires --destination");
    const linked = await refreshProvisionedDependencies({ destination: options.destination });
    console.log(`linked ${linked} provisioned dependencies`);
    return;
  }
  if (command === "emit-generated-env") {
    invariant(options.case && options.destination, "emit-generated-env requires --case and --destination");
    for (const entry of await generatedCaseEnvironment({
      manifestPath,
      repoRoot,
      caseName: options.case,
      destination: options.destination,
    })) console.log(entry);
    return;
  }
  if (command === "capture-source") {
    invariant(options.output, "capture-source requires --output");
    await captureSourceByteManifest({ manifestPath, repoRoot, outputPath: path.resolve(options.output) });
    return;
  }
  if (command === "verify-source") {
    invariant(options.snapshot, "verify-source requires --snapshot");
    await verifySourceByteManifest({
      manifestPath,
      repoRoot,
      snapshotPath: path.resolve(options.snapshot),
      observedOutputPath: options["observed-output"] ? path.resolve(options["observed-output"]) : null,
    });
    console.log("source byte manifest matches");
    return;
  }
  throw new Error(`unknown command: ${command ?? "<missing>"}`);
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
