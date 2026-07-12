import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";

import {
  createAppFixture,
  readConvexFile,
  readConvexJson,
  readGeneratedFile,
  runCli,
} from "./helpers.mjs";
import {
  assertNodeRuntimeMetadata,
  assertRuntimeLanes,
} from "./runtime_metadata_assertions.mjs";

async function runActionFixtures() {
  await testHttpActionFixture();
  await testActionCompositionServerFixture();
  await testSchedulerServerFixture();
  await testNodeRuntimeConfigFixture();
  await testNodeRuntimeCurrentConfigFixture();
  await testNodeExternalPackagesMetadataFixture();
  await testDefaultRuntimeExternalPackageRequiresExternalizationFixture();
  await testDefaultRuntimeExternalPackageFixture();
  await testDefaultRuntimeExternalPackageStringBrowserFieldFixture();
  await testDefaultRuntimeExternalPackageNestedBrowserConditionFixture();
  await testNodeExternalPackageSymlinkContainmentFixture();
  await testNodeExternalPackageSymlinkCycleFixture();
  await testNodeExternalPackagesStarFixture();
  await testNodePackageImportRequiresExternalizationFixture();
  await testNodeExternalPackageRequiresLocalInstallFixture();
  await testNodeExternalPackagesStarMustStandAloneFixture();
  await testUseNodeActionFixture();
  await testUseNodeRejectsQueriesFixture();
  await testDefaultRuntimeRejectsNodeBuiltinsFixture();
  await testDefaultRuntimeRejectsUnusedUseNodeImportFixture();
  await testDefaultRuntimeRejectsUsedUseNodeImportFixture();
  await testDefaultRuntimeAllowsTypeOnlyUseNodeImportFixture();
  await testDefaultRuntimeAllowsGeneratedApiReferenceToUseNodeActionFixture();
  await testDebugNodeApisFixture();
  await testInvalidNodeVersionFixture();
}

async function testNodeExternalPackagesMetadataFixture() {
  const appDir = await createAppFixture(
    {
      "messages.ts": `
"use node";

import { action } from "./_generated/server";
import sharp from "sharp";
import { helper } from "@scope/pkg/subpath";

export const read = action({
  args: {},
  handler: async () => sharp.name + helper,
});
`,
    },
    {
      rootFiles: {
        "package.json": `{"name":"fixture","private":true}`,
        "node_modules/sharp/package.json": `{"name":"sharp","version":"1.2.3","main":"index.js"}`,
        "node_modules/sharp/index.js": `export default { name: "sharp" };`,
        "node_modules/@scope/pkg/package.json": `{"name":"@scope/pkg","version":"4.5.6","exports":{".":"./index.js","./subpath":"./subpath.js"}}`,
        "node_modules/@scope/pkg/index.js": `export const root = "scoped-root";`,
        "node_modules/@scope/pkg/subpath.js": `export const helper = "scoped";`,
        "convex.json": `{
  "node": {
    "nodeVersion": "20",
    "externalPackages": ["sharp", "@scope/pkg"]
  }
}
`,
      },
    },
  );

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.deepEqual(manifest.node, {
    externalPackages: ["sharp", "@scope/pkg"],
    nodeVersion: "20",
    runtimeTarget: "node20",
  });
  assertRuntimeLanes(manifest, "node20");
  assertNodeRuntimeMetadata(manifest.functions[0], {
    nodeVersion: "20",
    runtimeTarget: "node20",
  });
  assert.equal(manifest.functions[0].runtime_bindings.sharp.type, "node_external_package_default");
  assert.equal(manifest.functions[0].runtime_bindings.helper.type, "node_external_package_named");

  const packageReport = await readConvexJson(appDir, "node_external_packages.json");
  assert.equal(packageReport.mode, "explicit");
  assert.deepEqual(packageReport.configuredExternalPackages, ["sharp", "@scope/pkg"]);
  assert.equal(packageReport.limits.enforcedByNimbus, false);
  assert.equal(packageReport.limits.convexCloudReference.zippedBytes, 45 * 1024 * 1024);
  assert.deepEqual(
    packageReport.packages.map((entry) => entry.packageName),
    ["@scope/pkg", "sharp"],
  );
  assert.ok(packageReport.packages.every((entry) => entry.sizeBytes > 0));
  assert.match(
    await readConvexFile(appDir, "node_modules/sharp/package.json"),
    /"name":"sharp"/,
  );
  assert.match(
    await readConvexFile(appDir, "node_modules/@scope/pkg/subpath.js"),
    /helper = "scoped"/,
  );

  const runtimeBundle = await readConvexFile(appDir, "bundle.mjs");
  // This manifest has exactly one function and it's "use node", so the whole
  // bundle is single-runtime Node — never shared with a default-lane
  // function — and external package bindings are imported eagerly at the
  // top of the bundle for their load-time side effects, not deferred to
  // first invocation. See packages/codegen/src/emit/runtime_bundle.mjs.
  assert.match(runtimeBundle, /^import "sharp";/m);
  assert.match(runtimeBundle, /^import "@scope\/pkg\/subpath";/m);
  assert.match(runtimeBundle, /nodeExternalPackage\(specifier\)/);
  assert.match(runtimeBundle, /node_external_package_default/);
  assert.match(runtimeBundle, /node_external_package_named/);
}

// A default-runtime module (no "use node") importing a bare npm package
// specifier must be rejected the same way a Node-lane module importing an
// unexternalized package is -- Nimbus never implicitly bundles npm packages
// into either lane's runtime bundle.
async function testDefaultRuntimeExternalPackageRequiresExternalizationFixture() {
  const appDir = await createAppFixture(
    {
      "widgets.ts": `
import { greet } from "greetlib";
import { mutation } from "./_generated/server";

export const create = mutation({
  args: {},
  handler: async () => greet(),
});
`,
    },
    {
      rootFiles: {
        "package.json": `{"name":"fixture","private":true}`,
        "node_modules/greetlib/package.json": `{"name":"greetlib","version":"1.0.0","main":"index.js"}`,
        "node_modules/greetlib/index.js": `export const greet = () => "hi";`,
      },
    },
  );

  const result = runCli(appDir);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /imports package "greetlib", but that package is not externalized/);
  assert.match(result.stderr, /node\.externalPackages/);
}

// A real, browser-compatible npm package (no Node builtins) works from a
// default-runtime module once externalized in convex.json, exactly like the
// Node-lane case -- same staging root, same dynamic-import() resolution at
// runtime (see emit/runtime_bundle_preamble.mjs), just reached from a module
// without "use node" at the top.
async function testDefaultRuntimeExternalPackageFixture() {
  const appDir = await createAppFixture(
    {
      "widgets.ts": `
import { greet } from "greetlib";
import { mutation } from "./_generated/server";

export const create = mutation({
  args: {},
  handler: async () => greet(),
});
`,
    },
    {
      rootFiles: {
        "package.json": `{"name":"fixture","private":true}`,
        "node_modules/greetlib/package.json": `{"name":"greetlib","version":"1.0.0","main":"index.js"}`,
        "node_modules/greetlib/index.js": `export const greet = () => "hi";`,
        "convex.json": `{ "node": { "externalPackages": ["greetlib"] } }\n`,
      },
    },
  );

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.equal(manifest.functions[0].runtime_environment, "default");
  assert.equal(manifest.functions[0].runtime_bindings.greet.type, "node_external_package_named");

  const runtimeBundle = await readConvexFile(appDir, "bundle.mjs");
  assert.doesNotMatch(runtimeBundle, /^import .* from "greetlib"/m);
  assert.match(runtimeBundle, /node_external_package_named/);

  assert.match(
    await readConvexFile(appDir, "node_modules/greetlib/index.js"),
    /greet = \(\) => "hi"/,
  );
}

// EX10R.5: the common `"browser": "./browser.js"` string form of the legacy
// browser field must resolve, not just the object-shaped
// `{ "./main.js": "./browser.js" }` form legacyBrowserField already handled.
async function testDefaultRuntimeExternalPackageStringBrowserFieldFixture() {
  const appDir = await createAppFixture(
    {
      "widgets.ts": `
import { label } from "strbrowser";
import { mutation } from "./_generated/server";

export const create = mutation({
  args: {},
  handler: async () => label,
});
`,
    },
    {
      rootFiles: {
        "package.json": `{"name":"fixture","private":true}`,
        "node_modules/strbrowser/package.json":
          `{"name":"strbrowser","version":"1.0.0","main":"index.node.js","browser":"./index.browser.js"}`,
        "node_modules/strbrowser/index.node.js": `export const label = "node";`,
        "node_modules/strbrowser/index.browser.js": `export const label = "browser";`,
        "convex.json": `{ "node": { "externalPackages": ["strbrowser"] } }\n`,
      },
    },
  );

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const packageReport = await readConvexJson(appDir, "node_external_packages.json");
  const entry = packageReport.packages.find((candidate) => candidate.packageName === "strbrowser");
  assert.equal(entry.browserEntry, "./index.browser.js");

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.match(manifest.functions[0].runtime_bindings.label.specifier, /index\.browser\.js$/);
  assert.match(
    await readConvexFile(appDir, "node_modules/strbrowser/index.browser.js"),
    /label = "browser"/,
  );
}

// EX10R.5: a package that nests its browser export condition further by
// module format (`"browser": { "import": ..., "require": ... }`, common for
// dual ESM/CJS packages) must still resolve -- not just a bare
// `"browser": "./browser.js"` condition string.
async function testDefaultRuntimeExternalPackageNestedBrowserConditionFixture() {
  const appDir = await createAppFixture(
    {
      "widgets.ts": `
import { label } from "nestedbrowser";
import { mutation } from "./_generated/server";

export const create = mutation({
  args: {},
  handler: async () => label,
});
`,
    },
    {
      rootFiles: {
        "package.json": `{"name":"fixture","private":true}`,
        "node_modules/nestedbrowser/package.json": JSON.stringify({
          name: "nestedbrowser",
          version: "1.0.0",
          main: "index.js",
          exports: {
            ".": {
              browser: { import: "./browser.mjs", require: "./browser.cjs" },
              default: "./index.js",
            },
          },
        }),
        "node_modules/nestedbrowser/index.js": `export const label = "node";`,
        "node_modules/nestedbrowser/browser.mjs": `export const label = "browser-esm";`,
        "node_modules/nestedbrowser/browser.cjs": `module.exports = { label: "browser-cjs" };`,
        "convex.json": `{ "node": { "externalPackages": ["nestedbrowser"] } }\n`,
      },
    },
  );

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const packageReport = await readConvexJson(appDir, "node_external_packages.json");
  const entry = packageReport.packages.find((candidate) => candidate.packageName === "nestedbrowser");
  assert.equal(entry.browserEntry, "./browser.mjs");

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.match(manifest.functions[0].runtime_bindings.label.specifier, /browser\.mjs$/);
}

// EX10R.6: a dependency shipping a symlink whose real target resolves
// outside the package's own root (e.g. `secrets -> ../../.env`) must not
// have that target's contents copied into the staged deployment artifact.
async function testNodeExternalPackageSymlinkContainmentFixture() {
  const appDir = await createAppFixture(
    {
      "messages.ts": `
"use node";

import { action } from "./_generated/server";
import pkg from "leakypkg";

export const read = action({
  args: {},
  handler: async () => pkg.ok,
});
`,
    },
    {
      rootFiles: {
        "package.json": `{"name":"fixture","private":true}`,
        "secret.txt": "top-secret-value",
        "node_modules/leakypkg/package.json": `{"name":"leakypkg","version":"1.0.0","main":"index.js"}`,
        "node_modules/leakypkg/index.js": `export default { ok: true };`,
        "convex.json": `{ "node": { "externalPackages": ["leakypkg"] } }\n`,
      },
    },
  );

  await fs.symlink(
    path.join(appDir, "secret.txt"),
    path.join(appDir, "node_modules", "leakypkg", "leaked.txt"),
  );

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const stagedDir = path.join(appDir, ".nimbus", "convex", "node_modules", "leakypkg");
  await assert.rejects(
    fs.readFile(path.join(stagedDir, "leaked.txt")),
    (error) => error.code === "ENOENT",
    "a symlink escaping the package root must not be copied into the staged deployment artifact",
  );
  assert.match(
    await readConvexFile(appDir, "node_modules/leakypkg/index.js"),
    /ok: true/,
    "legitimate package files must still stage normally",
  );
}

// EX10R.6: a symlink cycle inside a staged package must terminate the copy
// instead of recursing unboundedly.
async function testNodeExternalPackageSymlinkCycleFixture() {
  const appDir = await createAppFixture(
    {
      "messages.ts": `
"use node";

import { action } from "./_generated/server";
import pkg from "cyclicpkg";

export const read = action({
  args: {},
  handler: async () => pkg.ok,
});
`,
    },
    {
      rootFiles: {
        "package.json": `{"name":"fixture","private":true}`,
        "node_modules/cyclicpkg/package.json": `{"name":"cyclicpkg","version":"1.0.0","main":"index.js"}`,
        "node_modules/cyclicpkg/index.js": `export default { ok: true };`,
        "convex.json": `{ "node": { "externalPackages": ["cyclicpkg"] } }\n`,
      },
    },
  );

  // node_modules/cyclicpkg/loop -> node_modules/cyclicpkg (the package's own
  // root): a naive recursive copy would walk loop/loop/loop/... forever.
  await fs.symlink(
    path.join(appDir, "node_modules", "cyclicpkg"),
    path.join(appDir, "node_modules", "cyclicpkg", "loop"),
  );

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.match(
    await readConvexFile(appDir, "node_modules/cyclicpkg/index.js"),
    /ok: true/,
  );

  const stagedLoopExists = await fs
    .stat(path.join(appDir, ".nimbus", "convex", "node_modules", "cyclicpkg", "loop"))
    .then(() => true)
    .catch(() => false);
  assert.equal(stagedLoopExists, false, "a symlink cycle must be skipped, not recursively expanded");
}

async function testNodeExternalPackagesStarFixture() {
  const appDir = await createAppFixture(
    {
      "messages.ts": `
"use node";

import { action } from "./_generated/server";
import * as pkg from "pkg";

export const read = action({
  args: {},
  handler: async () => pkg.answer,
});
`,
    },
    {
      rootFiles: {
        "package.json": `{"name":"fixture","private":true}`,
        "node_modules/pkg/package.json": `{"name":"pkg","version":"1.0.0","main":"index.js"}`,
        "node_modules/pkg/index.js": `export const answer = 42;`,
        "convex.json": `{
  "node": {
    "externalPackages": ["*"]
  }
}
`,
      },
    },
  );

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const packageReport = await readConvexJson(appDir, "node_external_packages.json");
  assert.equal(packageReport.mode, "all");
  assert.deepEqual(packageReport.configuredExternalPackages, ["*"]);
  assert.equal(packageReport.packages[0].packageName, "pkg");
  assert.equal(packageReport.packages[0].importers[0].specifier, "pkg");
  assert.match(await readConvexFile(appDir, "node_modules/pkg/index.js"), /answer = 42/);
}

async function testNodePackageImportRequiresExternalizationFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
"use node";

import { action } from "./_generated/server";
import pkg from "pkg";

export const read = action({
  args: {},
  handler: async () => pkg.answer,
});
`,
  });

  const result = runCli(appDir);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /imports package "pkg", but that package is not externalized/);
  assert.match(result.stderr, /does not yet bundle npm packages/);
  assert.match(result.stderr, /node\.externalPackages/);
}

async function testNodeExternalPackageRequiresLocalInstallFixture() {
  const appDir = await createAppFixture(
    {
      "messages.ts": `
"use node";

import { action } from "./_generated/server";
import pkg from "pkg";

export const read = action({
  args: {},
  handler: async () => pkg.answer,
});
`,
    },
    {
      rootFiles: {
        "package.json": `{"name":"fixture","private":true}`,
        "convex.json": `{
  "node": {
    "externalPackages": ["pkg"]
  }
}
`,
      },
    },
  );

  const result = runCli(appDir);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /externalizes package "pkg"/);
  assert.match(result.stderr, /not resolvable from local node_modules/);
}

async function testNodeExternalPackagesStarMustStandAloneFixture() {
  const appDir = await createAppFixture(
    {
      "messages.ts": `
import { action } from "./_generated/server";

export const read = action({
  args: {},
  handler: async () => "ok",
});
`,
    },
    {
      rootFiles: {
        "convex.json": `{
  "node": {
    "externalPackages": ["*", "pkg"]
  }
}
`,
      },
    },
  );

  const result = runCli(appDir);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /must use "\*" by itself/);
}

async function testHttpActionFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { httpAction, internalMutation, query } from "./_generated/server";
import { internal } from "./_generated/api";
import { v } from "convex/values";

export const byAuthor = query({
  args: { author: v.string() },
  handler: async (ctx, { author }) =>
    await ctx.db
      .query("messages")
      .filter((q) => q.eq(q.field("author"), author))
      .collect(),
});

export const sendInternal = internalMutation({
  args: {
    author: v.string(),
    body: v.string(),
  },
  handler: async (ctx, { author, body }) =>
    await ctx.db.insert("messages", { author, body }),
});

export const postMessage = httpAction(async (ctx, request) => {
  const { author, body } = await request.json();
  const id = await ctx.runMutation(internal.messages.sendInternal, { author, body });
  return Response.json({ id }, { status: 201 });
});
`,
    "http.ts": `
import { httpRouter } from "convex/server";
import { httpAction } from "./_generated/server";
import { api } from "./_generated/api";
import { postMessage } from "./messages";

const http = httpRouter();

http.route({
  path: "/messages",
  method: "POST",
  handler: postMessage,
});

http.route({
  pathPrefix: "/messages/by-author",
  method: "GET",
  handler: httpAction(async (ctx, request) => {
    const author = new URL(request.url).searchParams.get("author");
    return Response.json(await ctx.runQuery(api.messages.byAuthor, { author }));
  }),
});

export default http;
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const generatedServer = await readGeneratedFile(appDir, "server.ts");
  assert.match(generatedServer, /httpRouter/);

  const routes = await readConvexJson(appDir, "http_routes.json");
  assert.equal(routes.routes.length, 2);
  assert.equal(routes.routes[0].method, "POST");
  assert.equal(routes.routes[0].path, "/messages");
  assert.equal(routes.routes[0].name, "messages:postMessage");
  assert.deepEqual(routes.routes[0].plan.response, {
    kind: "json",
    body: {
      id: {
        $result: {
          index: 0,
          path: "",
        },
      },
    },
    status: 201,
  });
  assert.equal(routes.routes[1].method, "GET");
  assert.equal(routes.routes[1].path_prefix, "/messages/by-author");
  assert.equal(routes.routes[1].name, "http:inline:1");
  assert.deepEqual(routes.routes[1].plan.operation, {
    type: "call_query",
    name: "messages:byAuthor",
    visibility: "public",
    args: {
      author: {
        $request: {
          source: "query",
          name: "author",
        },
      },
    },
  });

  const runtimeBundle = await readConvexFile(appDir, "bundle.mjs");
  assert.match(runtimeBundle, /routesByName/);
  assert.match(runtimeBundle, /"messages:postMessage"/);
  assert.match(runtimeBundle, /op_nimbus_http_route/);
}

async function testActionCompositionServerFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { action, internalAction, internalMutation, query } from "./_generated/server";
import { api, internal } from "./_generated/api";
import { v } from "convex/values";

export const list = query({
  args: { author: v.string() },
  handler: async (_ctx, { author }) => ({
    table: "messages",
    filters: [{ field: "author", op: "eq", value: author }],
    order: null,
    limit: null,
  }),
});

export const storeInternal = internalMutation({
  args: {
    author: v.string(),
    body: v.string(),
  },
  handler: async (ctx, { author, body }) =>
    await ctx.db.insert("messages", { author, body }),
});

export const listInternal = internalAction({
  args: { author: v.string() },
  handler: async (ctx, { author }) =>
    await ctx.runQuery(api.messages.list, { author }),
});

export const sendViaAction = action({
  args: {
    author: v.string(),
    body: v.string(),
  },
  handler: async (ctx, { author, body }) =>
    await ctx.runMutation(internal.messages.storeInternal, { author, body }),
});

export const listViaAction = action({
  args: { author: v.string() },
  handler: async (ctx, { author }) =>
    await ctx.runAction(internal.messages.listInternal, { author }),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.deepEqual(manifest.functions[2].plan, {
    type: "call_query",
    name: "messages:list",
    visibility: "public",
    args: {
      author: { $arg: "author" },
    },
  });
  assert.deepEqual(manifest.functions[3].plan, {
    type: "call_mutation",
    name: "messages:storeInternal",
    visibility: "internal",
    args: {
      author: { $arg: "author" },
      body: { $arg: "body" },
    },
  });
  assert.deepEqual(manifest.functions[4].plan, {
    type: "call_action",
    name: "messages:listInternal",
    visibility: "internal",
    args: {
      author: { $arg: "author" },
    },
  });

  const generatedApi = await readGeneratedFile(appDir, "api.ts");
  assert.match(
    generatedApi,
    /sendViaAction: makeActionReference<\{\n  "author": string;\n  "body": string;\n\}, Id<"messages">>\("messages:sendViaAction", "public"\)/,
  );
  assert.match(
    generatedApi,
    /listViaAction: makeActionReference<\{\n  "author": string;\n\}, unknown\[]>\("messages:listViaAction", "public"\)/,
  );

  const runtimeBundle = await readConvexFile(appDir, "bundle.mjs");
  assert.match(runtimeBundle, /executeActionDefinition/);
  assert.match(runtimeBundle, /op_nimbus_ctx_action/);
  assert.match(runtimeBundle, /runQuery/);
}

async function testSchedulerServerFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { internalMutation, mutation } from "./_generated/server";
import { internalScheduledFunctions } from "./_generated/scheduled_functions";
import { v } from "convex/values";

export const sendInternal = internalMutation({
  args: {
    body: v.string(),
  },
  handler: async (ctx, { body }) => await ctx.db.insert("messages", { body }),
});

export const scheduleInternal = mutation({
  args: {
    body: v.string(),
    delayMs: v.number(),
  },
  handler: async (ctx, { body, delayMs }) =>
    await ctx.scheduler.runAfter(delayMs, internalScheduledFunctions.messages.sendInternal, {
      body,
    }),
});

export const cancelScheduled = mutation({
  args: {
    jobId: v.string(),
  },
  handler: async (ctx, { jobId }) => await ctx.scheduler.cancel(jobId),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.deepEqual(manifest.functions[1].plan, {
    type: "schedule_run_after",
    delay_ms: { $arg: "delayMs" },
    name: "messages:sendInternal",
    visibility: "internal",
    args: {
      body: { $arg: "body" },
    },
  });
  assert.deepEqual(manifest.functions[2].plan, {
    type: "schedule_cancel",
    job_id: { $arg: "jobId" },
  });

  const runtimeBundle = await readConvexFile(appDir, "bundle.mjs");
  assert.match(runtimeBundle, /ctx\.scheduler\.runAfter/);
  assert.match(runtimeBundle, /ctx\.scheduler\.cancel/);
}

async function testNodeRuntimeConfigFixture() {
  const appDir = await createAppFixture(
    {
      "messages.ts": `
"use node";

import { action } from "./_generated/server";

export const readFile = action({
  args: {},
  handler: async () => "ok",
});
`,
    },
    {
      rootFiles: {
        "convex.json": `{
  "$schema": "./node_modules/convex/schemas/convex.schema.json",
  "node": {
    "nodeVersion": "24"
  }
}
`,
      },
    },
  );

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.deepEqual(manifest.node, {
    externalPackages: [],
    nodeVersion: "24",
    runtimeTarget: "node24",
  });
  assertRuntimeLanes(manifest, "node24");
  assertNodeRuntimeMetadata(manifest.functions[0], {
    nodeVersion: "24",
    runtimeTarget: "node24",
  });
}

async function testNodeRuntimeCurrentConfigFixture() {
  const appDir = await createAppFixture(
    {
      "messages.ts": `
"use node";

import { action } from "./_generated/server";

export const current = action({
  args: {},
  handler: async () => "ok",
});
`,
    },
    {
      rootFiles: {
        "convex.json": `{
  "node": {
    "nodeVersion": "26"
  }
}
`,
      },
    },
  );

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.deepEqual(manifest.node, {
    externalPackages: [],
    nodeVersion: "26",
    runtimeTarget: "node26",
  });
  assertRuntimeLanes(manifest, "node26");
  assertNodeRuntimeMetadata(manifest.functions[0], {
    nodeVersion: "26",
    runtimeTarget: "node26",
  });
}

async function testUseNodeActionFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
/* leading comments are allowed before the directive */
"use node";

import { internalAction } from "./_generated/server";
import fs from "node:fs";
import { readFileSync } from "fs";

export const runInternal = internalAction({
  args: {},
  handler: async () => readFileSync(fs.realpathSync("."), "utf8"),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.deepEqual(manifest.node, {
    externalPackages: [],
    nodeVersion: "24",
    runtimeTarget: "node24",
  });
  assertRuntimeLanes(manifest, "node24");
  assertNodeRuntimeMetadata(manifest.functions[0], {
    nodeVersion: "24",
    runtimeTarget: "node24",
  });

  const runtimeBundle = await readConvexFile(appDir, "bundle.mjs");
  // This manifest has exactly one function and it's "use node", so the whole
  // bundle is single-runtime Node — never shared with a default-lane
  // function, so there's no risk a static top-level "node:fs" import breaks
  // module linking for one — and it is imported eagerly at the top of the
  // bundle for its load-time side effects. See
  // packages/codegen/src/emit/runtime_bundle.mjs.
  assert.match(runtimeBundle, /^import "node:fs";/m);
  assert.match(runtimeBundle, /nodeBuiltinModule\(specifier\)/);
  assert.match(runtimeBundle, /return import\(specifier\)/);
  assert.match(runtimeBundle, /"type": "node_builtin_default"/);
  assert.match(runtimeBundle, /"type": "node_builtin_named"/);
}

async function testUseNodeRejectsQueriesFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
"use node";

import { query } from "./_generated/server";

export const list = query({
  args: {},
  handler: async () => [],
});
`,
  });

  const result = runCli(appDir);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /uses "use node"/);
  assert.match(result.stderr, /only supported for action functions/);
}

async function testDefaultRuntimeRejectsNodeBuiltinsFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import fs from "fs";
import { action } from "./_generated/server";

export const read = action({
  args: {},
  handler: async () => fs.realpathSync("."),
});
`,
  });

  const result = runCli(appDir);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /imports Node\.js builtin module/);
  assert.match(result.stderr, /--debug-node-apis/);
}

// A default-runtime module cannot import a "use node" module directly, even
// if the import is never referenced anywhere — the two modules execute on
// separate runtime lanes (see emit/runtime_bundle_preamble.mjs) and codegen
// must reject the cross-runtime import outright rather than let it through
// silently. See parser.mjs's validateCrossModuleRuntimeImports.
async function testDefaultRuntimeRejectsUnusedUseNodeImportFixture() {
  const appDir = await createAppFixture({
    "nodeHelpers.ts": `
"use node";

import { createHash } from "node:crypto";
import { action } from "./_generated/server";

export const hashIt = action({
  args: {},
  handler: async () => createHash("sha256").update("x").digest("hex"),
});
`,
    "widgets.ts": `
import { hashIt } from "./nodeHelpers";
import { query } from "./_generated/server";

export const list = query({
  args: {},
  handler: async () => [],
});
`,
  });

  const result = runCli(appDir);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /widgets\.ts imports "\.\/nodeHelpers"/);
  assert.match(result.stderr, /begins with "use node"/);
  assert.match(result.stderr, /cannot import a "use node" module directly/);
  assert.match(result.stderr, /ctx\.runAction\(internal\.nodeHelpers\.<exportName>, args\)/);
}

// The same clear cross-runtime-import error must fire even when the import
// is actually referenced in a handler body — previously this case surfaced a
// generic, implementation-detail "Phase 4C ... unsupported export shape"
// error out of the compile-time plan resolver instead.
async function testDefaultRuntimeRejectsUsedUseNodeImportFixture() {
  const appDir = await createAppFixture({
    "nodeHelpers.ts": `
"use node";

import { createHash } from "node:crypto";
import { action } from "./_generated/server";

export const hashIt = action({
  args: {},
  handler: async () => createHash("sha256").update("x").digest("hex"),
});
`,
    "widgets.ts": `
import { hashIt } from "./nodeHelpers";
import { query } from "./_generated/server";

export const list = query({
  args: {},
  handler: async () => {
    return typeof hashIt;
  },
});
`,
  });

  const result = runCli(appDir);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /widgets\.ts imports "\.\/nodeHelpers"/);
  assert.match(result.stderr, /begins with "use node"/);
  assert.doesNotMatch(result.stderr, /Phase 4C/);
}

// A whole-statement `import type ... from "./useNodeModule"` is erased by
// the TypeScript compiler and never reaches the runtime bundle, so it must
// not trip the cross-runtime import rule.
async function testDefaultRuntimeAllowsTypeOnlyUseNodeImportFixture() {
  const appDir = await createAppFixture({
    "nodeHelpers.ts": `
"use node";

import { createHash } from "node:crypto";
import { action } from "./_generated/server";

export const hashIt = action({
  args: {},
  handler: async () => createHash("sha256").update("x").digest("hex"),
});
`,
    "widgets.ts": `
import type { hashIt } from "./nodeHelpers";
import { query } from "./_generated/server";

export const list = query({
  args: {},
  handler: async () => [],
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);
}

// The supported way to call a "use node" action from a default-runtime
// function is through the generated API reference (internal.<module>.<name>)
// passed to ctx.runAction — this must continue to work unaffected by the
// cross-runtime import rule, since it never imports the "use node" module's
// file directly.
async function testDefaultRuntimeAllowsGeneratedApiReferenceToUseNodeActionFixture() {
  const appDir = await createAppFixture({
    "nodeHelpers.ts": `
"use node";

import { createHash } from "node:crypto";
import { action } from "./_generated/server";

export const hashIt = action({
  args: {},
  handler: async () => createHash("sha256").update("x").digest("hex"),
});
`,
    "widgets.ts": `
import { internal } from "./_generated/api";
import { action } from "./_generated/server";

export const runIt = action({
  args: {},
  handler: async (ctx) => {
    return await ctx.runAction(internal.nodeHelpers.hashIt, {});
  },
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);
}

async function testDebugNodeApisFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import fs from "fs";
import { readFile } from "node:fs/promises";
import { action } from "./_generated/server";

export const read = action({
  args: {},
  handler: async () => readFile(fs.realpathSync("."), "utf8"),
});
`,
  });

  const result = runCli(appDir, ["--debug-node-apis"]);
  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.match(result.stderr, /Node\.js builtin API usage was found/);
  assert.match(result.stderr, /import: fs \(canonical: fs\)/);
  assert.match(result.stderr, /import: node:fs\/promises \(canonical: fs\/promises\)/);
}

async function testInvalidNodeVersionFixture() {
  const appDir = await createAppFixture(
    {
      "messages.ts": `
import { action } from "./_generated/server";

export const read = action({
  args: {},
  handler: async () => "ok",
});
`,
    },
    {
      rootFiles: {
        "convex.json": `{
  "node": {
    "nodeVersion": "18"
  }
}
`,
      },
    },
  );

  const result = runCli(appDir);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /node\.nodeVersion/);
  assert.match(result.stderr, /"20", "22", "24", "26"/);
}

export { runActionFixtures };
