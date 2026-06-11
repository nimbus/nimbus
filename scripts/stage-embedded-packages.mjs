// Stage the built, dependency-closed package payloads into a single directory
// that the `nimbus` binary embeds via rust-embed (BPD1, condition 5).
//
// Reads each `packages/<dir>/dist` (produced by `scripts/build-js-package.mjs`)
// and copies it under `crates/nimbus-assets/embedded/packages/<dir>/`, then writes
// a `manifest.json` recording each package's logical name, version, and a
// SHA-256 per file. The binary version-locks against this manifest and
// checksum-verifies provisioned bytes (conditions 5 + 21).
//
// Run after the package builds; the Makefile wires it as a binary build input.

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { assertNimbusRootSdkArtifactText } from "./nimbus-root-sdk-artifact-policy.mjs";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OUT_DIR = path.join(REPO_ROOT, "crates", "nimbus-assets", "embedded", "packages");

// Package source dirs whose `dist/` is provisioned into developer apps. Scoped
// npm package names use scoped staging dirs while keeping the source dir stable.
const PROVISIONED = [
  { sourceDir: "convex", stageDir: "convex" },
  { sourceDir: "nimbus", stageDir: "@nimbus/nimbus" },
  { sourceDir: "firebase", stageDir: "@nimbus/firebase" },
  { sourceDir: "mongodb", stageDir: "@nimbus/mongodb" },
  { sourceDir: "dynamodb", stageDir: "@nimbus/dynamodb" },
];

// Third-party runtime roots co-provisioned as embedded packages so the
// Nimbus-owned packages that depend on them install offline (closure). These
// are zero-`dependencies` pure ESM; their peer links resolve among siblings.
// NOTICE carries the repo-level attribution record for these embedded roots.
const THIRD_PARTY = ["@bufbuild/protobuf", "@connectrpc/connect", "@connectrpc/connect-web"];
const EXPECTED_ESBUILD_PLATFORM = (process.env.NIMBUS_EMBEDDED_ESBUILD_PLATFORM ?? "").trim();

function fail(message) {
  console.error(`stage-embedded-packages: ${message}`);
  process.exit(1);
}

function sha256(buf) {
  return crypto.createHash("sha256").update(buf).digest("hex");
}

// Remove fields that would make an offline `npm install` reach the registry or
// execute package-managed code when a third-party root is linked via a `file:`
// specifier. npm installs the devDependencies of a `file:`-linked package and
// runs lifecycle scripts, so e.g. @bufbuild/protobuf's devDependency
// "upstream-protobuf" would be fetched. Optional/bundled dependency metadata is
// also stripped because a future upstream release could otherwise introduce a
// registry probe while the closure gate still passed. The provisioned runtime
// needs only the built dist + runtime deps; strip the rest and refresh the
// staged manifest's checksum entry.
function stripNonRuntimeManifestFields(stagedPkgPath, files) {
  const manifest = JSON.parse(fs.readFileSync(stagedPkgPath, "utf8"));
  let changed = false;
  for (const field of [
    "devDependencies",
    "optionalDependencies",
    "bundleDependencies",
    "bundledDependencies",
    "scripts",
  ]) {
    if (manifest[field] !== undefined) {
      delete manifest[field];
      changed = true;
    }
  }
  if (!changed) return;
  const buf = Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`);
  fs.writeFileSync(stagedPkgPath, buf);
  const entry = files.find((f) => f.path === "package.json");
  if (entry) entry.sha256 = sha256(buf);
}

// Recursively copy a directory, collecting {relPath, sha256} for every file.
function copyTree(srcDir, destDir, relBase, files) {
  fs.mkdirSync(destDir, { recursive: true });
  for (const entry of fs.readdirSync(srcDir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    const src = path.join(srcDir, entry.name);
    const dest = path.join(destDir, entry.name);
    const rel = relBase ? `${relBase}/${entry.name}` : entry.name;
    if (entry.isDirectory()) {
      if (entry.name === "node_modules") continue; // never nest installer trees
      copyTree(src, dest, rel, files);
    } else {
      const buf = fs.readFileSync(src);
      fs.writeFileSync(dest, buf);
      files.push({ path: rel, sha256: sha256(buf) });
    }
  }
}

fs.rmSync(OUT_DIR, { recursive: true, force: true });
fs.mkdirSync(OUT_DIR, { recursive: true });

const packages = [];
for (const pkg of PROVISIONED) {
  const distDir = path.join(REPO_ROOT, "packages", pkg.sourceDir, "dist");
  const distManifestPath = path.join(distDir, "package.json");
  if (!fs.existsSync(distManifestPath)) {
    fail(`missing ${path.relative(REPO_ROOT, distManifestPath)} — run the package builds first (npm run build)`);
  }
  const distManifest = JSON.parse(fs.readFileSync(distManifestPath, "utf8"));
  const files = [];
  const stagedDir = path.join(OUT_DIR, pkg.stageDir);
  copyTree(distDir, stagedDir, "", files);
  if (distManifest.name === "@nimbus/nimbus") {
    verifyNimbusRootSdkArtifact(path.join(stagedDir, "index.js"), true);
    verifyNimbusRootSdkArtifact(path.join(stagedDir, "index.d.ts"), false);
  }
  files.sort((a, b) => a.path.localeCompare(b.path));
  packages.push({
    dir: pkg.stageDir,
    sourceDir: pkg.sourceDir,
    name: distManifest.name,
    version: distManifest.version,
    thirdParty: false,
    files,
  });
}

for (const name of THIRD_PARTY) {
  const srcDir = path.join(REPO_ROOT, "node_modules", name);
  const pkgJsonPath = path.join(srcDir, "package.json");
  if (!fs.existsSync(pkgJsonPath)) {
    fail(`missing node_modules/${name} — run npm install before staging`);
  }
  const pkgJson = JSON.parse(fs.readFileSync(pkgJsonPath, "utf8"));
  const files = [];
  copyTree(srcDir, path.join(OUT_DIR, name), "", files);
  stripNonRuntimeManifestFields(path.join(OUT_DIR, name, "package.json"), files);
  files.sort((a, b) => a.path.localeCompare(b.path));
  packages.push({ dir: name, name: pkgJson.name, version: pkgJson.version, thirdParty: true, files });
}

// Tooling closure for the embedded V8 codegen runner (BPD4): the codegen
// prebundle (typescript inlined) + esbuild (JS wrapper) + the host-platform
// @esbuild native binary. Staged under `.tooling/` — build-time tooling run
// targets discovered by the V8 tooling runtime ($discovered_tooling), NOT
// app-provisioned packages. esbuild here runs as a staged tooling binary, not
// external Node.
const tooling = [];

function stageToolingTree(srcDir, toolingName) {
  if (!fs.existsSync(srcDir)) fail(`missing tooling source ${path.relative(REPO_ROOT, srcDir)}`);
  const files = [];
  copyTree(srcDir, path.join(OUT_DIR, ".tooling", toolingName), "", files);
  files.sort((a, b) => a.path.localeCompare(b.path));
  tooling.push({ name: toolingName, kind: "tree", files });
}

// 1. codegen prebundle (single pure-JS file; typescript inlined, esbuild lazy).
{
  const bundle = path.join(REPO_ROOT, "packages", "codegen", "dist", "codegen.bundle.mjs");
  if (!fs.existsSync(bundle)) {
    fail("missing codegen prebundle — run `npm run build -w @nimbus/codegen` before staging");
  }
  const destDir = path.join(OUT_DIR, ".tooling", "codegen");
  fs.mkdirSync(destDir, { recursive: true });
  const buf = fs.readFileSync(bundle);
  fs.writeFileSync(path.join(destDir, "codegen.bundle.mjs"), buf);
  tooling.push({
    name: "codegen",
    kind: "bundle",
    files: [{ path: "codegen.bundle.mjs", sha256: sha256(buf) }],
  });
}

// 2. esbuild JS wrapper.
stageToolingTree(path.join(REPO_ROOT, "node_modules", "esbuild"), "esbuild");

// 3. host-platform @esbuild/<platform> native binary (matches this binary build).
const esbuildPlatformRoot = path.join(REPO_ROOT, "node_modules", "@esbuild");
const platforms = fs.existsSync(esbuildPlatformRoot)
  ? fs.readdirSync(esbuildPlatformRoot).filter((d) => !d.startsWith("."))
  : [];
if (platforms.length === 0) fail("no node_modules/@esbuild/<platform> found — run npm install");
const selectedPlatforms = EXPECTED_ESBUILD_PLATFORM
  ? platforms.filter((platform) => platform === EXPECTED_ESBUILD_PLATFORM)
  : platforms;
if (EXPECTED_ESBUILD_PLATFORM && selectedPlatforms.length !== 1) {
  fail(
    `expected node_modules/@esbuild/${EXPECTED_ESBUILD_PLATFORM} for this binary target, ` +
      `but installed platforms are: ${platforms.join(", ") || "<none>"}`,
  );
}
for (const platform of selectedPlatforms) {
  stageToolingTree(path.join(esbuildPlatformRoot, platform), `@esbuild/${platform}`);
}

const manifest = { schema: 1, packages, tooling };
fs.writeFileSync(path.join(OUT_DIR, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);

const total = packages.reduce((n, p) => n + p.files.length, 0);
console.log(`staged ${packages.length} packages (${total} files) → ${path.relative(REPO_ROOT, OUT_DIR)}`);
for (const p of packages) console.log(`  ${p.name}@${p.version} (${p.files.length} files)`);

function verifyNimbusRootSdkArtifact(filePath, runtime) {
  const artifact = fs.readFileSync(filePath, "utf8");
  try {
    assertNimbusRootSdkArtifactText(path.relative(REPO_ROOT, filePath), artifact, {
      runtime,
    });
  } catch (error) {
    if (error instanceof Error) {
      fail(error.message);
    }
    throw error;
  }
}
