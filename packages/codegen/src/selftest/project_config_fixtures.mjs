import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import path from "node:path";

import {
  createAppFixture,
  readConvexFile,
  readGeneratedFile,
  runCli,
} from "./helpers.mjs";

// Fixture-driven coverage for convex.json's app-root settings that live
// outside the "node" block: "functions" (custom functions-dir path) and
// "generateCommonJSApi" (emit convex/_generated/api_cjs.cjs).
async function runProjectConfigFixtures() {
  await testFunctionsOverrideRelocatesSourceFixture();
  await testFunctionsOverrideTakesPriorityOverDefaultConvexDirFixture();
  await testFunctionsOverrideReportsMissingDirectoryFixture();
  await testFunctionsOverrideRejectsAbsolutePathFixture();
  await testGenerateCommonJSApiEmitsLoadableCjsFixture();
  await testGenerateCommonJSApiOmittedByDefaultFixture();
  await testGenerateCommonJSApiRejectsWrongTypeFixture();
}

async function testFunctionsOverrideRelocatesSourceFixture() {
  const appDir = await createAppFixture(
    {
      "messages.ts": `
import { query } from "./_generated/server";

export const list = query({
  args: {},
  handler: async () => [],
});
`,
    },
    {
      sourceDir: "src/backend",
      rootFiles: {
        "convex.json": `{"functions": "src/backend"}`,
      },
    },
  );

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const api = await readGeneratedFile(appDir, "api.ts", { sourceDir: "src/backend" });
  assert.match(api, /export const api = \{\n {2}messages: \{\n {4}list: makeQueryReference/);
}

async function testFunctionsOverrideTakesPriorityOverDefaultConvexDirFixture() {
  const appDir = await createAppFixture(
    {
      "messages.ts": `
import { query } from "./_generated/server";

export const fromDefaultConvexDir = query({
  args: {},
  handler: async () => [],
});
`,
    },
    {
      rootFiles: {
        "convex.json": `{"functions": "src/backend"}`,
        "src/backend/messages.ts": `
import { query } from "./_generated/server";

export const fromOverrideDir = query({
  args: {},
  handler: async () => [],
});
`,
      },
    },
  );

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const api = await readGeneratedFile(appDir, "api.ts", { sourceDir: "src/backend" });
  assert.match(api, /fromOverrideDir/);
  assert.doesNotMatch(api, /fromDefaultConvexDir/);
}

async function testFunctionsOverrideReportsMissingDirectoryFixture() {
  const appDir = await createAppFixture(
    {},
    {
      rootFiles: {
        "convex.json": `{"functions": "src/backend"}`,
      },
    },
  );

  const result = runCli(appDir);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /"functions": "src\/backend"/);
  assert.match(result.stderr, /is not a directory/);
}

async function testFunctionsOverrideRejectsAbsolutePathFixture() {
  const appDir = await createAppFixture(
    {},
    {
      rootFiles: {
        "convex.json": `{"functions": "/etc/backend"}`,
      },
    },
  );

  const result = runCli(appDir);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /"functions" must be a path relative to/);
}

async function testGenerateCommonJSApiEmitsLoadableCjsFixture() {
  const appDir = await createAppFixture(
    {
      "messages.ts": `
import { mutation, query } from "./_generated/server";

export const list = query({
  args: {},
  handler: async () => [],
});

export const send = mutation({
  args: {},
  handler: async () => null,
});
`,
    },
    {
      rootFiles: {
        "package.json": `{"name":"fixture","private":true}`,
        "convex.json": `{"generateCommonJSApi": true}`,
        "node_modules/convex/package.json": `{
  "name": "convex",
  "version": "0.0.0",
  "exports": { "./browser": "./browser.js" }
}
`,
        "node_modules/convex/browser.js": `
"use strict";
function makeQueryReference(name, visibility) {
  return { __kind: "query", name, visibility };
}
function makeMutationReference(name, visibility) {
  return { __kind: "mutation", name, visibility };
}
function makeActionReference(name, visibility) {
  return { __kind: "action", name, visibility };
}
function makePaginatedQueryReference(name, visibility) {
  return { __kind: "paginated_query", name, visibility };
}
module.exports = {
  makeQueryReference,
  makeMutationReference,
  makeActionReference,
  makePaginatedQueryReference,
};
`,
      },
    },
  );

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const cjsSource = await readGeneratedFile(appDir, "api_cjs.cjs");
  assert.match(cjsSource, /^"use strict";/m);
  assert.match(cjsSource, /require\("convex\/browser"\)/);
  assert.match(cjsSource, /module\.exports = \{ api, internal \};/);
  // The CJS variant is untyped JavaScript — it must not carry the ESM
  // file's TypeScript generic type arguments (makeQueryReference<...>).
  assert.doesNotMatch(cjsSource, /makeQueryReference</);

  // Prove the file actually loads under require(), not just that its text
  // matches a pattern — spawn a real node process against the generated
  // fixture package.
  const apiCjsPath = path.join(appDir, "convex", "_generated", "api_cjs.cjs");
  const proof = spawnSync(
    process.execPath,
    [
      "-e",
      `
const assert = require("node:assert/strict");
const { api } = require(${JSON.stringify(apiCjsPath)});
assert.equal(api.messages.list.__kind, "query");
assert.equal(api.messages.list.visibility, "public");
assert.equal(api.messages.send.__kind, "mutation");
assert.equal(api.messages.send.visibility, "public");
console.log("REQUIRE_OK");
`,
    ],
    { cwd: appDir, encoding: "utf8" },
  );
  assert.equal(proof.status, 0, proof.stderr || proof.stdout);
  assert.match(proof.stdout, /REQUIRE_OK/);
}

async function testGenerateCommonJSApiOmittedByDefaultFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { query } from "./_generated/server";

export const list = query({
  args: {},
  handler: async () => [],
});
`,
  });

  const firstRun = runCli(appDir);
  assert.equal(firstRun.status, 0, firstRun.stderr || firstRun.stdout);
  await assert.rejects(readGeneratedFile(appDir, "api_cjs.cjs"));

  // Turning the setting on and back off must remove the stale file, not
  // just skip writing a new one — mirrors the bun program bundle cleanup
  // in main.mjs.
  await readConvexFile(appDir, "functions.json"); // sanity: first run succeeded
  const convexJsonPath = path.join(appDir, "convex.json");
  await import("node:fs/promises").then((fs) =>
    fs.writeFile(convexJsonPath, `{"generateCommonJSApi": true}`, "utf8"),
  );
  const secondRun = runCli(appDir);
  assert.equal(secondRun.status, 0, secondRun.stderr || secondRun.stdout);
  await readGeneratedFile(appDir, "api_cjs.cjs"); // now present

  await import("node:fs/promises").then((fs) =>
    fs.writeFile(convexJsonPath, `{"generateCommonJSApi": false}`, "utf8"),
  );
  const thirdRun = runCli(appDir);
  assert.equal(thirdRun.status, 0, thirdRun.stderr || thirdRun.stdout);
  await assert.rejects(readGeneratedFile(appDir, "api_cjs.cjs"));
}

async function testGenerateCommonJSApiRejectsWrongTypeFixture() {
  const appDir = await createAppFixture(
    {},
    {
      rootFiles: {
        "convex.json": `{"generateCommonJSApi": "yes"}`,
      },
    },
  );

  const result = runCli(appDir);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /"generateCommonJSApi" must be a boolean/);
}

export { runProjectConfigFixtures };
