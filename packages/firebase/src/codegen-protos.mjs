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
    execFileSync(
      bufBinary,
      ["generate", protoRoot, "--template", templatePath],
      { cwd: packageRoot, stdio: "inherit" },
    );
  } finally {
    await fsp.rm(templateDir, { recursive: true, force: true });
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
  return [
    "version: v2",
    "plugins:",
    `  - local: ${protocGenEsBinary}`,
    `    out: ${outDir}`,
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
