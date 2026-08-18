#!/usr/bin/env node

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const DEFAULT_ROOT = path.resolve(SCRIPT_DIR, "..");

function option(args, name, fallback) {
  const index = args.indexOf(name);
  if (index === -1) return fallback;
  assert(index + 1 < args.length, `${name} requires a value`);
  return args[index + 1];
}

const repoRoot = path.resolve(option(process.argv.slice(2), "--repo-root", DEFAULT_ROOT));
const read = (relative) => fs.readFileSync(path.join(repoRoot, relative), "utf8");
const manifest = JSON.parse(read("scripts/examples-verify-cases.json"));
const examples = read("examples/README.md");
const operating = read("docs/private/operating/verification.md");
const runner = read("scripts/examples-verify.sh");

const tests = [];
function test(name, body) {
  tests.push({ name, body });
}

test("manifest-derived application documentation", () => {
  assert.equal(manifest.schemaVersion, 1);
  assert(Array.isArray(manifest.cases));
  const anchorCount = manifest.cases.reduce((total, item) => total + item.expectedAnchors.length, 0);
  assert.match(examples, new RegExp(`nine application cases and ${anchorCount} smoke`, "u"));
});

test("every case and update mode is documented", () => {
  for (const item of manifest.cases) {
    const row = `| \`${item.name}\` | \`${item.updateSemantics}\` |`;
    assert(examples.includes(row), `missing case/update row: ${row}`);
  }
  assert.match(examples, /`push` proves server-delivered change notification/u);
  assert.match(examples, /`polling` proves eventual\s+visibility through repeated reads/u);
});

test("node and runner commands match the contract", () => {
  assert.match(examples, /Node\.js version \(`>=22 <25`\)/u);
  assert.match(examples, /tests Node\.js 22 and 24/u);
  for (const token of [
    "make examples-verify",
    "NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL=5",
    "NIMBUS_EXAMPLES_VERIFY_ONLY=convex/tasks",
  ]) assert(examples.includes(token), `missing example command token: ${token}`);
  assert.match(operating, /NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL.*1 through 9/su);
});

test("reports and retained artifacts are actionable", () => {
  for (const text of [examples, operating]) {
    assert.match(text, /report\.json/u);
    assert.match(text, /junit\.xml/u);
    assert.match(text, /retained diagnostic artifact/u);
  }
  assert.match(examples, /target\/examples-verify-results\/<run-id>\//u);
  assert.match(operating, /case logs.*network lease state.*cleanup result/su);
});

test("stale status text is absent", () => {
  for (const stale of [
    "Five of the six verify fully green",
    "Convex app's smoke is partially verified",
    "Treat its console pass as application smoke evidence only",
    "current application verification lane contains known",
  ]) {
    assert(!`${examples}\n${operating}`.includes(stale), `stale text remains: ${stale}`);
  }
});

test("runner comment matches bounded scheduling", () => {
  assert.match(runner, /Every app in the validated manifest is independent/u);
  assert.match(runner, /bounded scheduler/u);
  assert.doesNotMatch(runner.slice(0, runner.indexOf("set -euo pipefail")), /sequentially|before moving to the next app/u);
});

let failed = 0;
for (const { name, body } of tests) {
  try {
    body();
    console.log(`PASS ${name}`);
  } catch (error) {
    failed += 1;
    console.error(`FAIL ${name}: ${error.message}`);
  }
}
console.log(`Summary: ${tests.length - failed} passed, ${failed} failed`);
if (failed > 0) process.exitCode = 1;
