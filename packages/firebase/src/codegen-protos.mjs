// Generates the firebase package's protobuf bindings via `buf generate`
// (pure npm — no cargo-registry protoc dependency). The proto sources stay
// vendored under crates/nimbus-firebase/proto; the Rust crate remains their
// owner. Output is gitignored (see DE16) and regenerated on every
// build/test/typecheck via the package.json `codegen:proto` pre-step.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

const require = createRequire(import.meta.url);
const packageRoot = fileURLToPath(new URL("../", import.meta.url));
const repoRoot = fileURLToPath(new URL("../../../", import.meta.url));
const protoRoot = path.join(repoRoot, "crates", "nimbus-firebase", "proto");
const defaultOutputRoot = path.join(packageRoot, "src", "gen");
const disableExperimentalWebStorage = "--no-experimental-webstorage";

export function nodeToolEnvironment(
  environment = process.env,
  allowedFlags = process.allowedNodeEnvironmentFlags,
) {
  const nodeOptions = environment.NODE_OPTIONS?.trim() ?? "";
  if (
    !allowedFlags.has(disableExperimentalWebStorage) ||
    nodeOptions.split(/\s+/u).includes(disableExperimentalWebStorage)
  ) {
    return { ...environment };
  }
  return {
    ...environment,
    NODE_OPTIONS: [nodeOptions, disableExperimentalWebStorage]
      .filter(Boolean)
      .join(" "),
  };
}

// Generates the buf/protoc-gen-es output into `outputRoot`, wiping it first.
// Exported (not just invoked as a CLI script) so the DE16 determinism check
// (selftest/codegen_determinism.mjs) can call this twice into disposable
// directories and diff the results without touching the real src/gen tree.
export async function generateProtos(outputRoot) {
  const bufBinary = resolveBinBinary("@bufbuild/buf", "buf");
  const protocGenEsBinary = resolveBinBinary(
    "@bufbuild/protoc-gen-es",
    "protoc-gen-es",
  );
  assert.ok(
    fs.existsSync(path.join(protoRoot, "google")),
    "No vendored Firestore proto files were found.",
  );

  await fsp.rm(outputRoot, { recursive: true, force: true });
  await fsp.mkdir(outputRoot, { recursive: true });

  const templateDir = await fsp.mkdtemp(path.join(os.tmpdir(), "nimbus-firebase-buf-"));
  const templatePath = path.join(templateDir, "buf.gen.yaml");
  try {
    await fsp.writeFile(
      templatePath,
      renderBufGenTemplate(protocGenEsBinary, outputRoot),
      "utf8",
    );
    // Run buf THROUGH node. `@bufbuild/buf` and `@bufbuild/protoc-gen-es`
    // resolve to JS launcher scripts, not native executables; exec'ing them
    // directly relies on a shebang, which Windows ignores (ENOENT on
    // `bin/buf`). Invoking `node <bin>` works on every platform. The plugin
    // is node-wrapped the same way inside the template (see below).
    execFileSync(
      process.execPath,
      [bufBinary, "generate", protoRoot, "--template", templatePath],
      {
        cwd: packageRoot,
        env: nodeToolEnvironment(),
        stdio: "inherit",
      },
    );
  } finally {
    await fsp.rm(templateDir, { recursive: true, force: true });
  }
}

const expectedOutputFiles = [
  "google/firestore/v1/document_pb.ts",
  "google/firestore/v1/firestore_pb.ts",
  "google/firestore/v1/query_pb.ts",
  "google/firestore/v1/write_pb.ts",
  "google/protobuf/timestamp_pb.ts",
];

async function missingOutputFiles(outputRoot) {
  const missing = [];
  for (const relativePath of expectedOutputFiles) {
    try {
      await fsp.access(path.join(outputRoot, relativePath));
    } catch {
      missing.push(relativePath);
    }
  }
  return missing;
}

// Regenerates `outputRoot` only if it is missing expected output, serialized
// via an advisory lock directory. `npm run test`/`build`/`typecheck` already
// regenerate the real src/gen tree via the package.json `codegen:proto`
// pre-step, but some callers invoke selftest.mjs directly (the Rust
// node-dependent tests that exercise the firebase package: nimbus-cli's
// dev-adoption round-trip and nimbus-server's firebase REST CRUD smoke test),
// bypassing that pre-step. Those two tests run in separate cargo-nextest test
// binaries that CI executes concurrently, so both can independently spawn
// `node ./src/selftest.mjs` against a clean checkout where src/gen does not
// exist yet — without locking, one process's wipe-and-regenerate would tear
// the directory out from under the other mid-build (observed as esbuild
// "Could not resolve" errors during local reproduction of this fix).
export async function ensureProtosGenerated(outputRoot = defaultOutputRoot) {
  if ((await missingOutputFiles(outputRoot)).length === 0) {
    return;
  }
  await withGenerationLock(outputRoot, async () => {
    // Re-check inside the lock: another process may have finished
    // generating while this one was waiting to acquire it.
    if ((await missingOutputFiles(outputRoot)).length > 0) {
      await generateProtos(outputRoot);
    }
  });
  const stillMissing = await missingOutputFiles(outputRoot);
  assert.deepEqual(
    stillMissing,
    [],
    `Missing generated Firestore protobuf output under ${outputRoot} after running codegen: ${stillMissing.join(", ")}. Check that crates/nimbus-firebase/proto has vendored Firestore proto sources.`,
  );
}

async function withGenerationLock(outputRoot, fn) {
  // `fs.mkdir` without `recursive` fails with EEXIST if the directory is
  // already there, which makes directory creation an atomic mutex primitive
  // usable across separate OS processes (no extra dependency required).
  const lockDir = `${outputRoot}.lock`;
  const deadlineMs = Date.now() + 60_000;
  for (;;) {
    try {
      await fsp.mkdir(lockDir);
      break;
    } catch (err) {
      if (err.code !== "EEXIST") {
        throw err;
      }
      if (Date.now() > deadlineMs) {
        throw new Error(`Timed out waiting for the protobuf codegen lock at ${lockDir}`);
      }
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }
  try {
    await fn();
  } finally {
    await fsp.rm(lockDir, { recursive: true, force: true });
  }
}

// Only regenerate the package's real src/gen when this file runs as the CLI
// entry point (`npm run codegen:proto`) — not when imported as a module.
if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  await generateProtos(defaultOutputRoot);
}

function renderBufGenTemplate(protocGenEsBinary, outDir) {
  // Same plugin + options as the prior raw-protoc invocation:
  // import_extension=ts so tsc's rewriteRelativeImportExtensions gives correct
  // `.js` specifiers in the emitted JS; we post-process the emitted `.d.ts` to
  // match, since tsc leaves `.ts` specifiers there.
  //
  // The `local` plugin is given as an argv list `[node, <protoc-gen-es>]` so
  // buf invokes the JS plugin launcher through node — the extensionless
  // `bin/protoc-gen-es` script is not directly executable on Windows.
  // Paths are single-quoted because Windows paths contain backslashes, which
  // are literal in YAML single-quoted scalars.
  const yq = (value) => `'${String(value).replace(/'/g, "''")}'`;
  return [
    "version: v2",
    "plugins:",
    "  - local:",
    `      - ${yq(process.execPath)}`,
    `      - ${yq(protocGenEsBinary)}`,
    `    out: ${yq(outDir)}`,
    "    opt:",
    "      - target=ts",
    "      - json_types=true",
    "      - import_extension=ts",
    "",
  ].join("\n");
}

function resolveBinBinary(packageName, binName) {
  const packageJsonPath = require.resolve(`${packageName}/package.json`);
  const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, "utf8"));
  const binRelativePath =
    typeof packageJson.bin === "string" ? packageJson.bin : packageJson.bin?.[binName];
  assert.ok(binRelativePath, `Unable to resolve ${binName} binary from ${packageName}.`);
  return path.join(path.dirname(packageJsonPath), binRelativePath);
}
