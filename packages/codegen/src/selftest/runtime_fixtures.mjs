import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import path from "node:path";
import { pathToFileURL } from "node:url";
import vm from "node:vm";

import { generateRuntimeProgramBundle } from "../emit/runtime_bundle.mjs";
import {
  createAppFixture,
  readConvexFile,
  readConvexJson,
  readGeneratedFile,
  runCli,
  runInWorkerRealm,
} from "./helpers.mjs";
import {
  assertBunJscRuntimeMetadata,
  assertDefaultRuntimeMetadata,
  assertRuntimeLanes,
} from "./runtime_metadata_assertions.mjs";

async function runRuntimeFixtures() {
  await testUnsupportedMultiOperationFixture();
  await testRuntimeOnlyQueryFixture();
  await testRuntimeOnlyPaginatedQueryFixture();
  await testRuntimeOnlyMutationImportedScheduledFunctionsFixture();
  await testRuntimeOnlyMutationImportedScheduledFunctionsWithJsExtensionFixture();
  await testHostDispatchedInternalMutationInvocationFixture();
  await testMixedDefaultAndNodeRuntimeSharedBundleFixture();
  await testSingleRuntimeNodeBundleImportsEagerlyAtLoadFixture();
  await testRuntimeProgramBundleCandidateFixture();
  await testBunRuntimeProgramBundleFixture();
  testRuntimeProgramBundleRejectsNodeRuntimeImports();
  await testImportedServerValidatorsFixture();
  await testUnsupportedPatchWithoutIdValidatorFixture();
  await testRuntimeHandlerWithTypeScriptSyntaxFailsLoudly();
}

async function testRuntimeHandlerWithTypeScriptSyntaxFailsLoudly() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { mutation } from "./_generated/server";
import { v } from "convex/values";

export const toggle = mutation({
  args: { id: v.id("messages") },
  handler: async (ctx, { id }) => {
    const message = (await ctx.db.get(id)) as { pinned: boolean } | null;
    if (message === null) {
      throw new Error("message not found");
    }
    await ctx.db.patch(id, { pinned: !message.pinned });
    return null;
  },
});
`,
  });

  const result = runCli(appDir);
  assert.notEqual(
    result.status,
    0,
    "TypeScript-only syntax in a runtime handler must fail codegen loudly",
  );
  assert.match(result.stderr, /messages:toggle/);
  assert.match(result.stderr, /not valid JavaScript/);
  assert.match(result.stderr, /messages:\d+/);
  assert.match(result.stderr, /TypeScript-only syntax/);
}

async function testUnsupportedMultiOperationFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { mutation } from "./_generated/server";
import { v } from "convex/values";

export const sendAndSchedule = mutation({
  args: {
    body: v.string(),
  },
  handler: async (ctx, { body }) => {
    await ctx.db.insert("messages", { body });
    return await ctx.scheduler.runAfter(
      1000,
      { kind: "mutation", name: "messages:sendAndSchedule", visibility: "public" },
      { body },
    );
  },
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.equal(manifest.functions[0].plan, null);
  assertDefaultRuntimeMetadata(manifest.functions[0]);
  assertRuntimeLanes(manifest, "node24");
  assert.match(manifest.functions[0].runtime_handler, /ctx\.db\.insert/);
  assert.match(manifest.functions[0].runtime_handler, /ctx\.scheduler\.runAfter/);

  const runtimeBundle = await readConvexFile(appDir, "bundle.mjs");
  assert.match(runtimeBundle, /runtimeHandlersByName/);
  assert.match(runtimeBundle, /compileRuntimeHandler/);
}

async function testRuntimeOnlyQueryFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { query } from "./_generated/server";
import { v } from "convex/values";

export const maybeByAuthor = query({
  args: {
    author: v.union(v.string(), v.null()),
  },
  handler: async (ctx, { author }) => {
    const messages = author
      ? await ctx.db
        .query("messages")
        .withIndex("by_author", (q) => q.eq("author", author))
        .take(20)
      : await ctx.db.query("messages").take(20);
    return messages.slice(0, 20);
  },
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.equal(manifest.functions[0].plan, null);
  assert.match(manifest.functions[0].runtime_handler, /ctx\.db/);
  assert.match(manifest.functions[0].runtime_handler, /slice/);

  const runtimeBundle = await readConvexFile(appDir, "bundle.mjs");
  assert.match(runtimeBundle, /invokeNamedDefinitionLocally/);
  assert.match(runtimeBundle, /runtimeHandlersByName/);
}

async function testRuntimeOnlyPaginatedQueryFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { paginatedQuery } from "./_generated/server";
import { v } from "convex/values";

export const listPage = paginatedQuery({
  args: {
    author: v.union(v.string(), v.null()),
  },
  handler: async (ctx, { author }) => {
    const normalizedAuthor = author?.trim();
    if (normalizedAuthor) {
      return ctx.db
        .query("messages")
        .withIndex("by_author", (q) => q.eq("author", normalizedAuthor));
    }
    return ctx.db.query("messages");
  },
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.equal(manifest.functions[0].plan, null);
  assert.match(manifest.functions[0].runtime_handler, /trim/);

  const runtimeBundle = await readConvexFile(appDir, "bundle.mjs");
  assert.match(runtimeBundle, /op_nimbus_ctx_query_paginate/);
  assert.match(runtimeBundle, /__builderId/);
}

async function testRuntimeOnlyMutationImportedScheduledFunctionsFixture() {
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

export const sendAndSchedule = mutation({
  args: {
    body: v.string(),
  },
  handler: async (ctx, { body }) => {
    const id = await ctx.db.insert("messages", { body });
    await ctx.scheduler.runAfter(
      1_000,
      internalScheduledFunctions.messages.sendInternal,
      { body: \`\${body} later\` },
    );
    return id;
  },
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.equal(manifest.functions[1].plan, null);
  assert.match(manifest.functions[1].runtime_handler, /internalScheduledFunctions/);
  assert.deepEqual(manifest.functions[1].runtime_bindings, {
    internalScheduledFunctions: {
      type: "generated_reference_tree",
      visibility: "internal",
      reference_kind: "mutation",
    },
  });

  const runtimeBundle = await readConvexFile(appDir, "bundle.mjs");
  assert.match(runtimeBundle, /materializeRuntimeBindings/);
  assert.match(runtimeBundle, /generated_reference_tree/);

  const bundleUrl = pathToFileURL(
    path.join(appDir, ".nimbus", "convex", "bundle.mjs"),
  ).href;

  // HG0 (Band B-FIX, CAPTURE-ORDERING): __nimbusInvoke is now installed via
  // Object.defineProperty(configurable:false, writable:false), so it can
  // never be deleted/reinstalled on the selftest process's own globalThis.
  // Drive the import + invoke in a fresh worker realm instead (see
  // runInWorkerRealm in helpers.mjs) — the mock context and captured
  // scheduler call travel back over postMessage, and assertions stay here.
  const source = `
(async () => {
  const { parentPort, workerData } = await import("node:worker_threads");
  try {
    let scheduledCall = null;
    globalThis.__nimbusCreateContext = () => ({
      db: {
        insert: async (_table, document) =>
          document.body === "hello" ? "message-id" : "scheduled-id",
      },
      scheduler: {
        runAfter: async (delayMs, mutationRef, args) => {
          // mutationRef carries internal dispatch machinery that isn't
          // structured-cloneable across postMessage; the test only asserts
          // on these three fields, so pull just those out.
          scheduledCall = {
            delayMs,
            mutationRef: {
              name: mutationRef?.name,
              visibility: mutationRef?.visibility,
              kind: mutationRef?.kind,
            },
            args,
          };
          return "job-id";
        },
      },
    });
    await import(workerData.bundleUrl);
    const response = await globalThis.__nimbusInvoke({
      kind: "mutation",
      function_name: "messages:sendAndSchedule",
      args: { body: "hello" },
    });
    parentPort.postMessage({ ok: true, value: { response, scheduledCall } });
  } catch (error) {
    parentPort.postMessage({
      ok: false,
      error: { message: error?.message ?? String(error), stack: error?.stack ?? null },
    });
  }
})();
`;

  const { response, scheduledCall } = await runInWorkerRealm(source, { bundleUrl });
  assert.deepEqual(response, { status: "ok", value: "message-id" });
  assert.equal(scheduledCall?.delayMs, 1_000);
  assert.equal(scheduledCall?.mutationRef?.name, "messages:sendInternal");
  assert.equal(scheduledCall?.mutationRef?.visibility, "internal");
  assert.equal(scheduledCall?.mutationRef?.kind, "mutation");
  assert.deepEqual(scheduledCall?.args, { body: "hello later" });
}

// Regression fixture for the cross-lane-internal-call bug found via the
// convex/runtimes example: a host-constructed invocation of the bundle (a
// cross-lane nested ctx.run* re-entering through host dispatch, or top-level
// scheduler/client traffic) carries no request.visibility — the host has
// already resolved and enforced visibility against its registry. The
// generated invokeNamedDefinitionLocally gate used to default a missing
// visibility to "public" and reject every internal function on those paths
// ("nimbus function x is internal, not public"), while an explicit
// reference-tree visibility (supplied only by same-isolate nested ctx.run*
// dispatch) must keep being enforced.
async function testHostDispatchedInternalMutationInvocationFixture() {
  const appDir = await createAppFixture({
    "digests.ts": `
import { internalMutation } from "./_generated/server";
import { v } from "convex/values";

export const store = internalMutation({
  args: {
    body: v.string(),
  },
  handler: async (ctx, { body }) =>
    await ctx.db.insert("digests", { body, createdAt: Date.now() }),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.equal(manifest.functions[0].name, "digests:store");
  assert.equal(manifest.functions[0].visibility, "internal");
  assert.equal(manifest.functions[0].plan, null);
  assert.match(manifest.functions[0].runtime_handler, /Date\.now/);

  const bundleUrl = pathToFileURL(
    path.join(appDir, ".nimbus", "convex", "bundle.mjs"),
  ).href;

  // HG0 (Band B-FIX, CAPTURE-ORDERING): see the comment in
  // testRuntimeOnlyMutationImportedScheduledFunctionsFixture above — drive
  // this in a fresh worker realm instead of delete-then-reinstall against the
  // selftest process's own (now hardened) globalThis.__nimbusInvoke. The
  // mismatched-visibility call's rejection is caught inside the worker (a
  // rejected promise can't cross postMessage) and reported back as a plain
  // message for the outer assertion to match against.
  const source = `
(async () => {
  const { parentPort, workerData } = await import("node:worker_threads");
  try {
    const insertedDocuments = [];
    globalThis.__nimbusCreateContext = () => ({
      db: {
        insert: async (table, document) => {
          insertedDocuments.push({ table, document });
          return "id-" + insertedDocuments.length;
        },
      },
    });
    await import(workerData.bundleUrl);

    // Host-constructed request (no visibility): must invoke the internal
    // mutation — the host already enforced visibility before dispatching.
    const hostDispatched = await globalThis.__nimbusInvoke({
      kind: "mutation",
      function_name: "digests:store",
      args: { body: "cross-lane" },
    });

    // Same-isolate nested dispatch with the matching internal reference tree
    // keeps working. invokeNamedDefinitionLocally is module-private (HG2) —
    // there is no globalThis bridge to call directly anymore, so this drives
    // it the same way a real ctx.run* call does: through
    // globalThis.__nimbusInvoke, which forwards the request (including an
    // explicit visibility) straight through to invokeNamedDefinitionLocally.
    const localInternal = await globalThis.__nimbusInvoke({
      kind: "mutation",
      function_name: "digests:store",
      visibility: "internal",
      args: { body: "local" },
    });

    // An explicit public reference aimed at an internal function is still a
    // reference-selection error. The gate throws a plain Error (no
    // nimbusHostError), so __nimbusInvoke's catch rethrows it unchanged.
    let mismatchedError = null;
    try {
      await globalThis.__nimbusInvoke({
        kind: "mutation",
        function_name: "digests:store",
        visibility: "public",
        args: { body: "mismatched" },
      });
    } catch (error) {
      mismatchedError = { message: error?.message ?? String(error) };
    }

    parentPort.postMessage({
      ok: true,
      value: { hostDispatched, localInternal, mismatchedError, insertedDocuments },
    });
  } catch (error) {
    parentPort.postMessage({
      ok: false,
      error: { message: error?.message ?? String(error), stack: error?.stack ?? null },
    });
  }
})();
`;

  const { hostDispatched, localInternal, mismatchedError, insertedDocuments } =
    await runInWorkerRealm(source, { bundleUrl });

  assert.deepEqual(hostDispatched, { status: "ok", value: "id-1" });
  assert.equal(insertedDocuments[0]?.table, "digests");
  assert.equal(insertedDocuments[0]?.document?.body, "cross-lane");

  assert.deepEqual(localInternal, { status: "ok", value: "id-2" });

  assert.ok(mismatchedError, "expected the mismatched-visibility call to reject");
  assert.match(mismatchedError.message, /digests:store is internal, not public/);
  assert.equal(insertedDocuments.length, 2);
}

// Regression test for a NodeNext-moduleResolution app whose relative
// imports carry an explicit ".js" extension (required by NodeNext, optional
// under Bundler resolution). createKnownImportBindingRecord in
// parser/compile_bindings.mjs must recognize "./_generated/scheduled_functions.js"
// the same way it recognizes the extensionless form, or the scheduled-target
// reference silently drops out of runtime_bindings and the handler throws a
// ReferenceError the first time the scheduling branch actually executes.
async function testRuntimeOnlyMutationImportedScheduledFunctionsWithJsExtensionFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { internalMutation, mutation } from "./_generated/server.js";
import { internalScheduledFunctions } from "./_generated/scheduled_functions.js";
import { v } from "convex/values";

export const sendInternal = internalMutation({
  args: {
    body: v.string(),
  },
  handler: async (ctx, { body }) => await ctx.db.insert("messages", { body }),
});

export const sendAndSchedule = mutation({
  args: {
    body: v.string(),
  },
  handler: async (ctx, { body }) => {
    const id = await ctx.db.insert("messages", { body });
    await ctx.scheduler.runAfter(
      1_000,
      internalScheduledFunctions.messages.sendInternal,
      { body: \`\${body} later\` },
    );
    return id;
  },
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.equal(manifest.functions[1].plan, null);
  assert.match(manifest.functions[1].runtime_handler, /internalScheduledFunctions/);
  assert.deepEqual(manifest.functions[1].runtime_bindings, {
    internalScheduledFunctions: {
      type: "generated_reference_tree",
      visibility: "internal",
      reference_kind: "mutation",
    },
  });
}

// Regression fixture for a real bug found while building the convex/runtimes
// example: crates/nimbus-convex loads exactly one bundle.mjs per app and
// shares it across every V8-based runtime lane (the default web-standard
// isolate and every node* lane alike). The codegen bundle template used to
// emit a static top-level `import ... from "node:x"` for every Node builtin
// used ANYWHERE in the app, unconditionally — so an app mixing one
// default-runtime function with one "use node" function importing a Node
// builtin failed module linking for the *default* function too, even though
// it never touches that builtin. Node imports must resolve lazily (only when
// the function that actually uses them is invoked) so a default-runtime
// function sharing the bundle is never asked to resolve them at all.
async function testMixedDefaultAndNodeRuntimeSharedBundleFixture() {
  const appDir = await createAppFixture({
    "actions.ts": `
import { action } from "./_generated/server";
import { v } from "convex/values";

export const runDefault = action({
  args: { text: v.string() },
  returns: v.string(),
  handler: async (_ctx, { text }) => \`default:\${text.toUpperCase()}\`,
});
`,
    "nodeActions.ts": `
"use node";

import crypto from "node:crypto";
import { action } from "./_generated/server";
import { v } from "convex/values";

export const runNode = action({
  args: { text: v.string() },
  returns: v.string(),
  handler: async (_ctx, { text }) =>
    crypto.createHash("sha256").update(text, "utf8").digest("hex"),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const runtimeBundle = await readConvexFile(appDir, "bundle.mjs");
  assert.doesNotMatch(runtimeBundle, /^import .* from "node:/m);
  assert.match(runtimeBundle, /"actions:runDefault"/);
  assert.match(runtimeBundle, /"nodeActions:runNode"/);

  const bundleUrl = pathToFileURL(
    path.join(appDir, ".nimbus", "convex", "bundle.mjs"),
  ).href;

  // HG0 (Band B-FIX, CAPTURE-ORDERING): see the comment in
  // testRuntimeOnlyMutationImportedScheduledFunctionsFixture above.
  const source = `
(async () => {
  const { parentPort, workerData } = await import("node:worker_threads");
  try {
    globalThis.__nimbusCreateContext = () => ({});
    await import(workerData.bundleUrl);

    const defaultResponse = await globalThis.__nimbusInvoke({
      kind: "action",
      function_name: "actions:runDefault",
      args: { text: "world" },
    });

    const nodeResponse = await globalThis.__nimbusInvoke({
      kind: "action",
      function_name: "nodeActions:runNode",
      args: { text: "hello" },
    });

    parentPort.postMessage({ ok: true, value: { defaultResponse, nodeResponse } });
  } catch (error) {
    parentPort.postMessage({
      ok: false,
      error: { message: error?.message ?? String(error), stack: error?.stack ?? null },
    });
  }
})();
`;

  const { defaultResponse, nodeResponse } = await runInWorkerRealm(source, { bundleUrl });
  assert.deepEqual(defaultResponse, { status: "ok", value: "default:WORLD" });

  const expectedHash = createHash("sha256").update("hello", "utf8").digest("hex");
  assert.deepEqual(nodeResponse, { status: "ok", value: expectedHash });
}

// EX10R.4: a bundle whose functions are entirely "use node" must import its
// Node/external-package bindings eagerly, at bundle load, not lazily at
// first invocation -- so a dependency that throws or captures state at
// module-init time fails at deploy, not mid-request. This drives the
// dependency's own top-level side effect and asserts it has already fired
// once the bundle module is loaded, before any function is ever invoked.
async function testSingleRuntimeNodeBundleImportsEagerlyAtLoadFixture() {
  const appDir = await createAppFixture(
    {
      "messages.ts": `
"use node";

import sideEffectPackage from "eager-side-effect-package";
import { action } from "./_generated/server";
import { v } from "convex/values";

export const run = action({
  args: { text: v.string() },
  returns: v.string(),
  handler: async (_ctx, { text }) => \`\${sideEffectPackage.marker}:\${text}\`,
});
`,
    },
    {
      rootFiles: {
        "package.json": `{"name":"fixture","private":true}`,
        "node_modules/eager-side-effect-package/package.json":
          `{"name":"eager-side-effect-package","version":"1.0.0","main":"index.js"}`,
        "node_modules/eager-side-effect-package/index.js":
          "globalThis.__nimbusCodegenEagerImportFired = "
          + "(globalThis.__nimbusCodegenEagerImportFired ?? 0) + 1;\n"
          + 'export default { marker: "loaded" };\n',
        "convex.json": `{ "node": { "externalPackages": ["eager-side-effect-package"] } }\n`,
      },
    },
  );

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const runtimeBundle = await readConvexFile(appDir, "bundle.mjs");
  assert.match(runtimeBundle, /^import "eager-side-effect-package";/m);

  const bundleUrl = pathToFileURL(
    path.join(appDir, ".nimbus", "convex", "bundle.mjs"),
  ).href;

  // HG0 (Band B-FIX, CAPTURE-ORDERING): see the comment in
  // testRuntimeOnlyMutationImportedScheduledFunctionsFixture above. A fresh
  // worker realm also gives __nimbusCodegenEagerImportFired a clean start
  // for free, so both checkpoints are read straight off the worker's own
  // globalThis rather than saved/restored on the shared process global.
  const source = `
(async () => {
  const { parentPort, workerData } = await import("node:worker_threads");
  try {
    globalThis.__nimbusCreateContext = () => ({});
    await import(workerData.bundleUrl);
    const firedAfterLoad = globalThis.__nimbusCodegenEagerImportFired;

    const response = await globalThis.__nimbusInvoke({
      kind: "action",
      function_name: "messages:run",
      args: { text: "hello" },
    });
    const firedAfterInvoke = globalThis.__nimbusCodegenEagerImportFired;

    parentPort.postMessage({
      ok: true,
      value: { firedAfterLoad, response, firedAfterInvoke },
    });
  } catch (error) {
    parentPort.postMessage({
      ok: false,
      error: { message: error?.message ?? String(error), stack: error?.stack ?? null },
    });
  }
})();
`;

  const { firedAfterLoad, response, firedAfterInvoke } = await runInWorkerRealm(source, {
    bundleUrl,
  });
  assert.equal(
    firedAfterLoad,
    1,
    "single-runtime Node bundle must import its dependency's side effects at load, before any invocation",
  );
  assert.deepEqual(response, { status: "ok", value: "loaded:hello" });
  assert.equal(
    firedAfterInvoke,
    1,
    "the lazy per-function dynamic import() at first invocation must resolve from the module cache, not re-run init side effects",
  );
}

async function testRuntimeProgramBundleCandidateFixture() {
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

export const sendAndSchedule = mutation({
  args: {
    body: v.string(),
  },
  handler: async (ctx, { body }) => {
    const id = await ctx.db.insert("messages", { body });
    await ctx.scheduler.runAfter(
      1_000,
      internalScheduledFunctions.messages.sendInternal,
      { body: \`\${body} later\` },
    );
    return id;
  },
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  const moduleBundle = await readConvexFile(appDir, "bundle.mjs");
  assert.match(moduleBundle, /^export \{\};$/m);

  const programBundle = generateRuntimeProgramBundle({
    functions: manifest.functions,
    routes: [],
  });
  assert.doesNotMatch(programBundle, /^import\s/m);
  assert.doesNotMatch(programBundle, /^export\s/m);
  // HG0 (Band B-FIX, CAPTURE-ORDERING): __nimbusInvoke is installed via
  // Object.defineProperty (configurable:false, writable:false), not a plain
  // assignment — see runtimeBundleDispatchGlobalInvoke in
  // emit/runtime_bundle_dispatch_global_invoke.mjs.
  assert.match(programBundle, /Object\.defineProperty\(globalThis, "__nimbusInvoke"/);
  assert.match(programBundle, /materializeRuntimeBindings/);

  let scheduledCall = null;
  const sandbox = {
    __nimbusCreateContext: () => ({
      db: {
        insert: async (_table, document) =>
          document.body === "hello" ? "message-id" : "scheduled-id",
      },
      scheduler: {
        runAfter: async (delayMs, mutationRef, args) => {
          scheduledCall = { delayMs, mutationRef, args };
          return "job-id";
        },
      },
    }),
  };
  vm.runInNewContext(programBundle, sandbox, {
    filename: "nimbus-runtime-program-bundle.js",
  });

  assert.equal(typeof sandbox.__nimbusInvoke, "function");
  const response = await sandbox.__nimbusInvoke({
    kind: "mutation",
    function_name: "messages:sendAndSchedule",
    args: { body: "hello" },
  });
  assert.equal(response.status, "ok");
  assert.equal(response.value, "message-id");
  assert.equal(scheduledCall?.delayMs, 1_000);
  assert.equal(scheduledCall?.mutationRef?.name, "messages:sendInternal");
  assert.equal(scheduledCall?.mutationRef?.visibility, "internal");
  assert.equal(scheduledCall?.mutationRef?.kind, "mutation");
  assert.deepEqual(JSON.parse(JSON.stringify(scheduledCall?.args)), {
    body: "hello later",
  });
}

async function testBunRuntimeProgramBundleFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
"use bun";

import { mutation } from "./_generated/server";
import { v } from "convex/values";

export const send = mutation({
  args: {
    body: v.string(),
  },
  handler: async (ctx, { body }) => {
    return await ctx.db.insert("messages", { body: body.trim() });
  },
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.equal(manifest.functions[0].name, "messages:send");
  assert.equal(manifest.functions[0].plan, null);
  assertBunJscRuntimeMetadata(manifest.functions[0]);
  assertRuntimeLanes(manifest, "node24");

  const bunProgramBundle = await readConvexFile(appDir, "bun_program_bundle.js");
  const bunProgramBundleHash = await readConvexFile(appDir, "bun_program_bundle.sha256");
  // HG0 (Band B-FIX, CAPTURE-ORDERING): see the note above in
  // testRuntimeProgramBundleCandidateFixture — same shared emitter.
  assert.match(bunProgramBundle, /Object\.defineProperty\(globalThis, "__nimbusInvoke"/);
  assert.match(bunProgramBundle, /runtimeHandlersByName/);
  assert.doesNotMatch(bunProgramBundle, /^import\s/m);
  assert.doesNotMatch(bunProgramBundle, /^export\s/m);
  assert.match(bunProgramBundle, /"runtime_environment": "bun"/);
  assert.equal(
    bunProgramBundleHash.trim().length,
    64,
    "Bun/JSC program bundle should have a sha256 sidecar",
  );

  const defaultBundle = await readConvexFile(appDir, "bundle.mjs");
  assert.match(defaultBundle, /^export \{\};$/m);
  assert.doesNotMatch(defaultBundle, /"runtime_environment": "bun"/);
  assert.doesNotMatch(defaultBundle, /"messages:send"/);
}

function testRuntimeProgramBundleRejectsNodeRuntimeImports() {
  assert.throws(
    () => generateRuntimeProgramBundle({
      functions: [
        {
          name: "actions:read",
          runtime_bindings: {
            fs: {
              type: "node_builtin_namespace",
              specifier: "node:fs",
            },
          },
        },
      ],
      routes: [],
    }),
    /runtime program bundle cannot materialize Node runtime imports: node:fs/,
  );
}

async function testImportedServerValidatorsFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { query } from "./_generated/server";
import { paginationOptsValidator, paginationResultValidator } from "convex/server";
import { v } from "convex/values";

export const listPage = query({
  args: {
    author: v.string(),
    paginationOpts: paginationOptsValidator,
  },
  returns: paginationResultValidator(
    v.object({
      author: v.string(),
      body: v.string(),
    }),
  ),
  handler: async (_ctx, { author }) => ({
    page: [{ author, body: "hello" }],
    continueCursor: "",
    isDone: true,
    splitCursor: null,
    pageStatus: null,
  }),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const apiFile = await readGeneratedFile(appDir, "api.ts");
  assert.match(apiFile, /paginationOpts/);
  assert.match(apiFile, /"continueCursor": string/);
  assert.match(apiFile, /"page": \(\{/);
}

async function testUnsupportedPatchWithoutIdValidatorFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { mutation } from "./_generated/server";
import { v } from "convex/values";

export const rename = mutation({
  args: {
    id: v.string(),
    body: v.string(),
  },
  handler: async (ctx, { id, body }) => await ctx.db.patch(id, { body }),
});
`,
  });

  const result = runCli(appDir);
  assert.notEqual(result.status, 0, "patch without v.id validator should fail");
  assert.match(
    result.stderr,
    /ctx\.db\.patch requires an id argument declared with v\.id\("table"\) in 4B/,
  );
}

export { runRuntimeFixtures };
