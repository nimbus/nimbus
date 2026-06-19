// Shared dependency-closed dist builder for Nimbus-owned JS packages (BPD1).
//
// Emits a provisioned `dist/` for a `packages/<name>` workspace:
//   - per-entry `.js` + `.d.ts` via `tsc` (rewriteRelativeImportExtensions
//     gives correct `.js` specifiers in the emitted JS; we post-process the
//     emitted `.d.ts` to match, since tsc leaves `.ts` specifiers there),
//   - a sanitized `dist/package.json` whose `exports` point at the built files
//     and whose dependency set is closed for the supported offline flow.
//
// Usage: node scripts/build-js-package.mjs <packageDirName>
// e.g.   node scripts/build-js-package.mjs mongodb
//
// The source workspace package is left untouched; only `dist/` is written.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  assertNimbusRootSdkArtifactText,
  assertNimbusRootSdkRouteArtifactText,
} from "./nimbus-root-sdk-artifact-policy.mjs";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const TSC = path.join(REPO_ROOT, "node_modules", "typescript", "bin", "tsc");

// Per-package sanitization rules for the *provisioned* manifest. Source package
// manifests are never modified by this script.
//   dropDependencies: deps removed from the provisioned manifest because they
//     are vestigial (unused by the shipped surface) or developer-supplied.
//   keepDependencies: deps the provisioned package genuinely needs at runtime
//     and that are co-provisioned (e.g. cross-package Nimbus deps).
// Anything not listed defaults to dropped, so the closure is explicit.
const SANITIZE = {
  // Helper-only surface: shipped `mongoUri()` imports nothing; the official mongodb
  // driver is developer-supplied (Offline contract boundaries).
  "@nimbus/mongodb": { dropDependencies: ["mongodb"], keepDependencies: [] },
  // Helper-only config surface; AWS SDK stays an optional peer (kept).
  "@nimbus/dynamodb": { dropDependencies: [], keepDependencies: [] },
  // Pure-TS client SDK; react/react-dom remain peers (kept).
  "@nimbus/nimbus": { dropDependencies: [], keepDependencies: [] },
  // Convex compat surface re-exports `@nimbus/nimbus` (co-provisioned, kept). Drops the
  // codegen-time deps: `@nimbus/codegen` (codegen runs in-binary) and `esbuild`
  // (only used by the dev/test scripts, never the shipped surface).
  convex: { dropDependencies: ["@nimbus/codegen", "esbuild"], keepDependencies: ["@nimbus/nimbus"] },
  // Firebase client — takes the stock npm name (like `convex`) so provisioned
  // apps keep stock `firebase/app` + `firebase/firestore` imports. Its three
  // runtime deps are zero-dep pure ESM used only by internal transport/
  // generated protos (the public surface does not expose their types), so they
  // are co-provisioned as additional binary-owned roots rather than bundled —
  // no fragile .d.ts-bundler is needed and the proven per-file tsc emit
  // applies. (Plan Decision permits "provisioned as additional binary-owned
  // package roots".)
  firebase: {
    dropDependencies: [],
    keepDependencies: ["@bufbuild/protobuf", "@connectrpc/connect", "@connectrpc/connect-web"],
  },
};

function fail(message) {
  console.error(`build-js-package: ${message}`);
  process.exit(1);
}

const pkgDirName = process.argv[2];
if (!pkgDirName) fail("missing <packageDirName> argument");

const pkgRoot = path.join(REPO_ROOT, "packages", pkgDirName);
const srcDir = path.join(pkgRoot, "src");
const distDir = path.join(pkgRoot, "dist");
const manifestPath = path.join(pkgRoot, "package.json");

if (!fs.existsSync(manifestPath)) fail(`no package.json at ${manifestPath}`);
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const rules = SANITIZE[manifest.name];
if (!rules) fail(`no sanitize rules for package "${manifest.name}" — add an entry to SANITIZE`);

// Resolve entry source files from the source exports map.
const exportsMap = manifest.exports ?? {};
const entries = [];
for (const [key, value] of Object.entries(exportsMap)) {
  const target = typeof value === "string" ? value : value?.default;
  if (typeof target !== "string") fail(`export "${key}" has no string target`);
  if (!target.endsWith(".ts")) fail(`export "${key}" target ${target} is not a .ts source`);
  const rel = target.replace(/^\.\/src\//, "").replace(/\.ts$/, "");
  entries.push({ key, srcRel: target.replace(/^\.\//, ""), distRel: rel });
}
if (entries.length === 0) fail("no exports to build");

// Clean dist.
fs.rmSync(distDir, { recursive: true, force: true });
fs.mkdirSync(distDir, { recursive: true });

// Emit JS + declarations with tsc. tsc follows imports from the entry files, so
// internal modules (e.g. internal/shared.ts) are emitted too.
const entryFiles = entries.map((e) => path.join(pkgRoot, e.srcRel));
try {
  execFileSync(
    process.execPath,
    [
      TSC,
      ...entryFiles,
      "--declaration",
      "--rootDir", srcDir,
      "--outDir", distDir,
      "--module", "esnext",
      "--moduleResolution", "bundler",
      "--target", "es2022",
      "--lib", "es2022,dom,dom.iterable",
      "--jsx", "react-jsx",
      "--strict",
      "--skipLibCheck",
      "--allowImportingTsExtensions",
      "--rewriteRelativeImportExtensions",
    ],
    { stdio: "inherit", cwd: REPO_ROOT },
  );
} catch {
  fail(`tsc emit failed for ${manifest.name}`);
}

// Post-process emitted .d.ts: rewrite relative `.ts` specifiers to `.js` so a
// consumer resolves them to the sibling declaration files.
function rewriteDtsExtensions(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      rewriteDtsExtensions(full);
    } else if (entry.name.endsWith(".d.ts")) {
      const before = fs.readFileSync(full, "utf8");
      const after = before.replace(/(["'])(\.\.?\/[^"']*?)\.ts\1/g, "$1$2.js$1");
      if (after !== before) fs.writeFileSync(full, after);
    }
  }
}
rewriteDtsExtensions(distDir);

// Build the sanitized provisioned manifest.
const keep = new Set(rules.keepDependencies ?? []);
const drop = new Set(rules.dropDependencies ?? []);
const sanitizedDeps = {};
for (const [dep] of Object.entries(manifest.dependencies ?? {})) {
  if (drop.has(dep)) continue;
  // Kept inter-package deps point at the co-provisioned sibling root via a
  // relative `file:` spec (the provisioned layout places every root as a
  // sibling under `.nimbus/packages/`), so an offline install never reaches the
  // registry. The dep NAME equals its staging dir for every co-provisioned root.
  if (keep.has(dep)) sanitizedDeps[dep] = `file:../${dep}`;
  // unlisted deps are dropped (explicit closure); warn so it is never silent.
  else console.error(`build-js-package: dropping unlisted dependency "${dep}" from ${manifest.name} provisioned manifest`);
}

const distExports = {};
for (const e of entries) {
  distExports[e.key] = { types: `./${e.distRel}.d.ts`, default: `./${e.distRel}.js` };
}

const provisioned = {
  name: manifest.name,
  version: manifest.version,
  private: true,
  type: "module",
  exports: distExports,
};
if (Object.keys(sanitizedDeps).length > 0) provisioned.dependencies = sanitizedDeps;
if (manifest.peerDependencies) provisioned.peerDependencies = manifest.peerDependencies;
if (manifest.peerDependenciesMeta) provisioned.peerDependenciesMeta = manifest.peerDependenciesMeta;

fs.writeFileSync(path.join(distDir, "package.json"), `${JSON.stringify(provisioned, null, 2)}\n`);

if (manifest.name === "@nimbus/nimbus") {
  verifyNimbusRootSdkArtifact(path.join(distDir, "index.js"), true);
  verifyNimbusRootSdkArtifact(path.join(distDir, "index.d.ts"), false);
  verifyNimbusRootSdkRouteArtifact(path.join(distDir, "control_plane_routes.js"));
  verifyNimbusRootSdkRouteArtifact(path.join(distDir, "control_plane_routes.d.ts"));
}

const emitted = fs.readdirSync(distDir).sort().join(", ");
console.log(`${manifest.name}: wrote dist/ (${emitted})`);

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

function verifyNimbusRootSdkRouteArtifact(filePath) {
  const artifact = fs.readFileSync(filePath, "utf8");
  try {
    assertNimbusRootSdkRouteArtifactText(path.relative(REPO_ROOT, filePath), artifact);
  } catch (error) {
    if (error instanceof Error) {
      fail(error.message);
    }
    throw error;
  }
}
