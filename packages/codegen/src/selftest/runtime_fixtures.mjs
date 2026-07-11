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
  await testMixedDefaultAndNodeRuntimeSharedBundleFixture();
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

  const bundleUrl =
    `${pathToFileURL(path.join(appDir, ".nimbus", "convex", "bundle.mjs")).href}?runtimeBindings=1`;
  const previousInvoke = globalThis.__nimbusInvoke;
  const previousCreateContext = globalThis.__nimbusCreateContext;

  let scheduledCall = null;
  globalThis.__nimbusCreateContext = () => ({
    db: {
      insert: async (_table, document) => document.body === "hello" ? "message-id" : "scheduled-id",
    },
    scheduler: {
      runAfter: async (delayMs, mutationRef, args) => {
        scheduledCall = { delayMs, mutationRef, args };
        return "job-id";
      },
    },
  });

  try {
    await import(bundleUrl);
    const response = await globalThis.__nimbusInvoke({
      kind: "mutation",
      function_name: "messages:sendAndSchedule",
      args: { body: "hello" },
    });
    assert.deepEqual(response, { status: "ok", value: "message-id" });
    assert.equal(scheduledCall?.delayMs, 1_000);
    assert.equal(scheduledCall?.mutationRef?.name, "messages:sendInternal");
    assert.equal(scheduledCall?.mutationRef?.visibility, "internal");
    assert.equal(scheduledCall?.mutationRef?.kind, "mutation");
    assert.deepEqual(scheduledCall?.args, { body: "hello later" });
  } finally {
    if (previousInvoke === undefined) {
      delete globalThis.__nimbusInvoke;
    } else {
      globalThis.__nimbusInvoke = previousInvoke;
    }
    if (previousCreateContext === undefined) {
      delete globalThis.__nimbusCreateContext;
    } else {
      globalThis.__nimbusCreateContext = previousCreateContext;
    }
  }
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

  const bundleUrl =
    `${pathToFileURL(path.join(appDir, ".nimbus", "convex", "bundle.mjs")).href}?mixedRuntime=1`;
  const previousInvoke = globalThis.__nimbusInvoke;
  const previousCreateContext = globalThis.__nimbusCreateContext;
  globalThis.__nimbusCreateContext = () => ({});

  try {
    await import(bundleUrl);

    const defaultResponse = await globalThis.__nimbusInvoke({
      kind: "action",
      function_name: "actions:runDefault",
      args: { text: "world" },
    });
    assert.deepEqual(defaultResponse, { status: "ok", value: "default:WORLD" });

    const expectedHash = createHash("sha256").update("hello", "utf8").digest("hex");
    const nodeResponse = await globalThis.__nimbusInvoke({
      kind: "action",
      function_name: "nodeActions:runNode",
      args: { text: "hello" },
    });
    assert.deepEqual(nodeResponse, { status: "ok", value: expectedHash });
  } finally {
    if (previousInvoke === undefined) {
      delete globalThis.__nimbusInvoke;
    } else {
      globalThis.__nimbusInvoke = previousInvoke;
    }
    if (previousCreateContext === undefined) {
      delete globalThis.__nimbusCreateContext;
    } else {
      globalThis.__nimbusCreateContext = previousCreateContext;
    }
  }
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
  assert.match(programBundle, /globalThis\.__nimbusInvoke/);
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
  assert.match(bunProgramBundle, /globalThis\.__nimbusInvoke/);
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
