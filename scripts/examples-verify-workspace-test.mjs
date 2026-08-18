#!/usr/bin/env node

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { constants as fsConstants } from "node:fs";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  caseShellRows,
  captureSourceByteManifest,
  generatedCaseEnvironment,
  loadValidatedManifest,
  prepareCaseWorkspace,
  refreshProvisionedDependencies,
  validateManifestValue,
  verifySourceByteManifest,
} from "./examples-verify-workspace.mjs";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..");
const CASE_MANIFEST = path.join(SCRIPT_DIR, "examples-verify-cases.json");

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", ...options });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed: ${result.stderr.trim()}`);
  }
  return result;
}

async function manifest_rejects_duplicate_or_incomplete_case() {
  const source = JSON.parse(await fs.readFile(CASE_MANIFEST, "utf8"));
  const duplicate = structuredClone(source);
  duplicate.cases[1].name = duplicate.cases[0].name;
  await assert.rejects(
    validateManifestValue(duplicate, { manifestPath: CASE_MANIFEST, repoRoot: REPO_ROOT }),
    /case name is duplicated/u,
  );

  const incomplete = structuredClone(source);
  delete incomplete.cases[0].smoke;
  await assert.rejects(
    validateManifestValue(incomplete, { manifestPath: CASE_MANIFEST, repoRoot: REPO_ROOT }),
    /smoke must be an object/u,
  );

  const escapingWorkspace = structuredClone(source);
  escapingWorkspace.cases[0].workspace = "../outside";
  await assert.rejects(
    validateManifestValue(escapingWorkspace, { manifestPath: CASE_MANIFEST, repoRoot: REPO_ROOT }),
    /workspace escapes its root/u,
  );

  const unsafeAppDir = structuredClone(source);
  unsafeAppDir.cases[0].appDir = "examples|outside";
  await assert.rejects(
    validateManifestValue(unsafeAppDir, { manifestPath: CASE_MANIFEST, repoRoot: REPO_ROOT }),
    /appDir is not shell-safe/u,
  );

  const ambiguousFlag = structuredClone(source);
  ambiguousFlag.cases[0].boot.flags = ["--first,--second"];
  await assert.rejects(
    validateManifestValue(ambiguousFlag, { manifestPath: CASE_MANIFEST, repoRoot: REPO_ROOT }),
    /boot\.flags entry contains the list delimiter/u,
  );
}

async function shell_rows_include_declared_surfaces() {
  const manifest = await loadValidatedManifest(CASE_MANIFEST, REPO_ROOT);
  const rows = caseShellRows(manifest);
  assert.equal(rows.length, 9);
  const mongo = rows.find((row) => row.startsWith("mongodb/tasks|"));
  assert.equal(mongo.split("|").at(-1), "mongodb-wire");
}

async function generated_environment_reads_only_validated_nimbus_keys() {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-avr7-env-"));
  try {
    const mongo = path.join(root, "mongo");
    const mongoUrl = new URL("mongodb://127.0.0.1:43123/");
    mongoUrl.username = "nimbus";
    mongoUrl.password = "fixture-password";
    await fs.mkdir(mongo);
    await fs.writeFile(
      path.join(mongo, ".env.local"),
      [
        "USER_OWNED=must-not-escape",
        "NIMBUS_DEPLOYMENT=local:fixture",
        `NIMBUS_MONGODB_URL=${mongoUrl}`,
        "",
      ].join("\n"),
    );
    assert.deepEqual(
      await generatedCaseEnvironment({
        manifestPath: CASE_MANIFEST,
        repoRoot: REPO_ROOT,
        caseName: "mongodb/tasks",
        destination: mongo,
      }),
      [
        `NIMBUS_MONGODB_URL=${mongoUrl}`,
        "NIMBUS_MONGODB_HOST=127.0.0.1",
        "NIMBUS_MONGODB_PORT=43123",
        "NIMBUS_MONGODB_USERNAME=nimbus",
        "NIMBUS_MONGODB_PASSWORD=fixture-password",
      ],
    );

    const dynamo = path.join(root, "dynamo");
    await fs.mkdir(dynamo);
    await fs.writeFile(
      path.join(dynamo, ".env.local"),
      [
        "NIMBUS_DYNAMODB_ENDPOINT=http://127.0.0.1:43234",
        "NIMBUS_DYNAMODB_ACCESS_KEY_ID=access",
        "NIMBUS_DYNAMODB_SECRET_ACCESS_KEY=secret",
        "",
      ].join("\n"),
    );
    assert.deepEqual(
      await generatedCaseEnvironment({
        manifestPath: CASE_MANIFEST,
        repoRoot: REPO_ROOT,
        caseName: "dynamodb/tasks",
        destination: dynamo,
      }),
      [
        "NIMBUS_DYNAMODB_ENDPOINT=http://127.0.0.1:43234",
        "NIMBUS_DYNAMODB_ACCESS_KEY_ID=access",
        "NIMBUS_DYNAMODB_SECRET_ACCESS_KEY=secret",
      ],
    );

    await fs.writeFile(
      path.join(dynamo, ".env.local"),
      [
        "NIMBUS_DYNAMODB_ENDPOINT=http://example.com:43234",
        "NIMBUS_DYNAMODB_ACCESS_KEY_ID=access",
        "NIMBUS_DYNAMODB_SECRET_ACCESS_KEY=secret",
        "",
      ].join("\n"),
    );
    await assert.rejects(
      generatedCaseEnvironment({
        manifestPath: CASE_MANIFEST,
        repoRoot: REPO_ROOT,
        caseName: "dynamodb/tasks",
        destination: dynamo,
      }),
      /must name a loopback host/u,
    );
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
}

async function fileBytes(filePath) {
  try {
    return Buffer.from(`symlink\0${await fs.readlink(filePath)}`);
  } catch (error) {
    if (!["EINVAL", "UNKNOWN"].includes(error?.code)) throw error;
  }
  const flags = fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW ?? 0);
  const handle = await fs.open(filePath, flags);
  try {
    assert.equal((await handle.stat()).isFile(), true, `source path is not a file: ${filePath}`);
    return await handle.readFile();
  } finally {
    await handle.close();
  }
}

async function all_nine_preparation_fixtures() {
  const manifest = await loadValidatedManifest(CASE_MANIFEST, REPO_ROOT);
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-avr4-prepare-"));
  try {
    const dependencyRoot = path.join(REPO_ROOT, "node_modules");
    for (const item of manifest.cases) {
      const destination = path.join(root, item.workspace);
      await prepareCaseWorkspace({
        manifestPath: CASE_MANIFEST,
        repoRoot: REPO_ROOT,
        caseName: item.name,
        destination,
      });
      for (const input of item.prepare.inputs) {
        const source = path.join(REPO_ROOT, item.appDir, input);
        const copied = path.join(destination, input);
        assert.deepEqual(await fileBytes(copied), await fileBytes(source), `${item.name} copied ${input}`);
      }
      const localNodeModules = path.join(destination, "node_modules");
      assert.equal((await fs.lstat(localNodeModules)).isDirectory(), true);
      assert.equal((await fs.lstat(path.join(localNodeModules, ".bin"))).isDirectory(), true);
      await assert.rejects(fs.lstat(path.join(destination, ".nimbus")), { code: "ENOENT" });
      const removablePackage = item.name.startsWith("nimbus/") ? "@nimbus/nimbus" : null;
      if (removablePackage) {
        const localPackage = path.join(localNodeModules, removablePackage);
        const sourcePackage = path.join(dependencyRoot, removablePackage);
        assert.equal(await fs.realpath(localPackage), await fs.realpath(sourcePackage));
        await fs.rm(localPackage, { recursive: true, force: true });
        assert.equal((await fs.stat(sourcePackage)).isDirectory(), true, "local removal must not cross into source node_modules");
        const provisionedPackage = path.join(destination, ".nimbus", "packages", removablePackage);
        await fs.mkdir(provisionedPackage, { recursive: true });
        await fs.writeFile(
          path.join(provisionedPackage, "package.json"),
          `${JSON.stringify({ name: removablePackage, version: "0.0.0-test" })}\n`,
        );
        assert.equal(await refreshProvisionedDependencies({ destination }), 1);
        assert.equal(await fs.realpath(localPackage), await fs.realpath(provisionedPackage));
        assert.equal((await fs.stat(sourcePackage)).isDirectory(), true, "refresh must not change source node_modules");
      }
      console.log(`PASS prepare_case_workspace ${item.name}`);
    }
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
}

async function external_package_vendor_replaces_same_version_symlink() {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-avr7-external-package-"));
  try {
    const appRoot = path.join(root, "app");
    const packageRoot = path.join(root, "shared", "nanoid");
    await fs.mkdir(path.join(appRoot, "scripts"), { recursive: true });
    await fs.mkdir(path.join(appRoot, "node_modules"), { recursive: true });
    await fs.mkdir(packageRoot, { recursive: true });
    await fs.writeFile(
      path.join(packageRoot, "package.json"),
      `${JSON.stringify({ name: "nanoid", version: "3.3.12", main: "index.js" })}\n`,
    );
    await fs.writeFile(path.join(packageRoot, "index.js"), "module.exports = () => 'fixture';\n");
    await fs.writeFile(
      path.join(appRoot, "convex.json"),
      `${JSON.stringify({ node: { externalPackages: ["nanoid"] } })}\n`,
    );
    const vendorScript = path.join(appRoot, "scripts", "vendor-external-packages.mjs");
    await fs.copyFile(
      path.join(REPO_ROOT, "examples", "convex", "runtimes", "scripts", "vendor-external-packages.mjs"),
      vendorScript,
    );
    const localPackage = path.join(appRoot, "node_modules", "nanoid");
    await fs.symlink(packageRoot, localPackage, "dir");

    run(process.execPath, [vendorScript], { cwd: appRoot });

    const localStat = await fs.lstat(localPackage);
    assert.equal(localStat.isDirectory(), true);
    assert.equal(localStat.isSymbolicLink(), false, "same-version source link must become case-local bytes");
    assert.notEqual(await fs.realpath(localPackage), await fs.realpath(packageRoot));
    assert.equal(await fs.readFile(path.join(localPackage, "index.js"), "utf8"), "module.exports = () => 'fixture';\n");
    assert.equal((await fs.stat(packageRoot)).isDirectory(), true, "vendoring must not change the source package");
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
}

function fixtureCase(index) {
  return {
    name: `case-${index}`,
    workspace: `workspace-${index}`,
    appDir: `apps/case-${index}`,
    prepare: { codegen: false, inputs: ["input.txt"] },
    boot: { mode: "start", needsAppDir: false, environment: [], flags: [] },
    smoke: { command: "npm", environment: [], stdioContract: false },
    surfaces: ["fixture"],
    updateSemantics: "request-response",
  };
}

async function createSourceFixture() {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-avr4-source-"));
  const repoRoot = path.join(root, "repo");
  await fs.mkdir(repoRoot);
  const manifest = {
    schemaVersion: 1,
    sourceGuardPaths: ["package.json", "package-lock.json", "compose.yaml"],
    ignoredAppPaths: [".nimbus", "node_modules", "dist", "functions/lib"],
    cases: Array.from({ length: 9 }, (_, index) => fixtureCase(index)),
  };
  const manifestPath = path.join(repoRoot, "scripts", "examples-verify-cases.json");
  await fs.mkdir(path.dirname(manifestPath), { recursive: true });
  await fs.writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  await fs.writeFile(path.join(repoRoot, "package.json"), "{}\n");
  await fs.writeFile(path.join(repoRoot, "package-lock.json"), "{}\n");
  await fs.writeFile(path.join(repoRoot, "compose.yaml"), "services: {}\n");
  for (let index = 0; index < 9; index += 1) {
    const appRoot = path.join(repoRoot, `apps/case-${index}`);
    await fs.mkdir(appRoot, { recursive: true });
    await fs.writeFile(path.join(appRoot, "input.txt"), `source-${index}\n`);
  }
  run("git", ["init", "-q", repoRoot]);
  run("git", ["-C", repoRoot, "add", "."]);
  run("git", [
    "-C", repoRoot,
    "-c", "user.name=Nimbus Test",
    "-c", "user.email=nimbus-test@example.invalid",
    "commit", "-q", "-m", "fixture",
  ]);
  return { root, repoRoot, manifestPath };
}

async function withSourceFixture(test) {
  const fixture = await createSourceFixture();
  try {
    await test(fixture);
  } finally {
    await fs.rm(fixture.root, { recursive: true, force: true });
  }
}

async function dirty_source_bytes_survive_success() {
  await withSourceFixture(async ({ root, repoRoot, manifestPath }) => {
    await fs.writeFile(path.join(repoRoot, "apps/case-0/input.txt"), "user-dirty-bytes\n");
    const snapshotPath = path.join(root, "before.json");
    await captureSourceByteManifest({ manifestPath, repoRoot, outputPath: snapshotPath });
    await verifySourceByteManifest({ manifestPath, repoRoot, snapshotPath });
    assert.equal(await fs.readFile(path.join(repoRoot, "apps/case-0/input.txt"), "utf8"), "user-dirty-bytes\n");
  });
}

async function dirty_source_bytes_survive_failure() {
  await withSourceFixture(async ({ root, repoRoot, manifestPath }) => {
    await fs.writeFile(path.join(repoRoot, "apps/case-1/input.txt"), "dirty-before-failure\n");
    const snapshotPath = path.join(root, "before.json");
    await captureSourceByteManifest({ manifestPath, repoRoot, outputPath: snapshotPath });
    const failed = spawnSync(process.execPath, ["-e", "process.exit(7)"]);
    assert.equal(failed.status, 7);
    await verifySourceByteManifest({ manifestPath, repoRoot, snapshotPath });
    assert.equal(await fs.readFile(path.join(repoRoot, "apps/case-1/input.txt"), "utf8"), "dirty-before-failure\n");
  });
}

async function staged_source_bytes_survive_failure() {
  await withSourceFixture(async ({ root, repoRoot, manifestPath }) => {
    const stagedPath = path.join(repoRoot, "apps/case-2/input.txt");
    await fs.writeFile(stagedPath, "staged-before-failure\n");
    run("git", ["-C", repoRoot, "add", "apps/case-2/input.txt"]);
    const beforeIndex = run("git", ["-C", repoRoot, "write-tree"]).stdout.trim();
    const snapshotPath = path.join(root, "before.json");
    await captureSourceByteManifest({ manifestPath, repoRoot, outputPath: snapshotPath });
    const failed = spawnSync(process.execPath, ["-e", "process.exit(9)"]);
    assert.equal(failed.status, 9);
    await verifySourceByteManifest({ manifestPath, repoRoot, snapshotPath });
    assert.equal(run("git", ["-C", repoRoot, "write-tree"]).stdout.trim(), beforeIndex);
    assert.equal(await fs.readFile(stagedPath, "utf8"), "staged-before-failure\n");
  });
}

async function source_byte_manifest_detects_mutation_without_restore() {
  await withSourceFixture(async ({ root, repoRoot, manifestPath }) => {
    const snapshotPath = path.join(root, "before.json");
    await captureSourceByteManifest({ manifestPath, repoRoot, outputPath: snapshotPath });
    await fs.writeFile(path.join(repoRoot, "apps/case-3/input.txt"), "unexpected-mutation\n");
    await assert.rejects(
      verifySourceByteManifest({ manifestPath, repoRoot, snapshotPath }),
      /source bytes changed: apps\/case-3\/input\.txt/u,
    );
    assert.equal(await fs.readFile(path.join(repoRoot, "apps/case-3/input.txt"), "utf8"), "unexpected-mutation\n");
  });
}

const tests = [
  manifest_rejects_duplicate_or_incomplete_case,
  shell_rows_include_declared_surfaces,
  generated_environment_reads_only_validated_nimbus_keys,
  all_nine_preparation_fixtures,
  external_package_vendor_replaces_same_version_symlink,
  dirty_source_bytes_survive_success,
  dirty_source_bytes_survive_failure,
  staged_source_bytes_survive_failure,
  source_byte_manifest_detects_mutation_without_restore,
];

for (const test of tests) {
  await test();
  console.log(`PASS ${test.name}`);
}
console.log(`Summary: ${tests.length} passed, 0 failed`);
