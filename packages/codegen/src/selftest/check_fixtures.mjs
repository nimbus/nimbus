import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const srcDir = fileURLToPath(new URL("../", import.meta.url));
const newFunctionPattern = new RegExp("\\bnew\\s+Function\\b", "g");
const evalCallPattern = new RegExp("\\beval\\s*\\(", "g");

const allowedNewFunctionCounts = new Map([
  ["emit/runtime_bundle_preamble.mjs", 1],
]);

async function runCodegenChecks() {
  const files = await listMjsFiles(srcDir);
  for (const filePath of files) {
    assertNodeSyntaxCheck(filePath);
    await assertCodeGenerationGuardrails(filePath);
  }
}

async function listMjsFiles(directory) {
  const files = [];
  for (const entry of await fs.readdir(directory, { withFileTypes: true })) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...await listMjsFiles(entryPath));
    } else if (entry.isFile() && entry.name.endsWith(".mjs")) {
      files.push(entryPath);
    }
  }
  return files.sort();
}

function assertNodeSyntaxCheck(filePath) {
  const result = spawnSync(process.execPath, ["--check", filePath], {
    encoding: "utf8",
  });
  assert.equal(
    result.status,
    0,
    result.stderr || result.stdout || `${relativeCodegenPath(filePath)} should parse`,
  );
}

async function assertCodeGenerationGuardrails(filePath) {
  const source = await fs.readFile(filePath, "utf8");
  const relativePath = relativeCodegenPath(filePath);
  const allowedNewFunctionCount = allowedNewFunctionCounts.get(relativePath) ?? 0;
  assert.equal(
    matchCount(source, newFunctionPattern),
    allowedNewFunctionCount,
    `${relativePath} has an unexpected Function-constructor compile path`,
  );
  assert.equal(
    matchCount(source, evalCallPattern),
    0,
    `${relativePath} must not use a direct eval call`,
  );
}

function matchCount(source, pattern) {
  return [...source.matchAll(pattern)].length;
}

function relativeCodegenPath(filePath) {
  return path.relative(srcDir, filePath).split(path.sep).join("/");
}

export { runCodegenChecks };
