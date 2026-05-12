import assert from "node:assert/strict";
import path from "node:path";
import { pathToFileURL } from "node:url";

import {
  createAppFixture,
  readConvexFile,
  readConvexJson,
  readGeneratedFile,
  runCli,
} from "./helpers.mjs";

async function runRuntimeFixtures() {
  await testUnsupportedMultiOperationFixture();
  await testRuntimeOnlyQueryFixture();
  await testRuntimeOnlyPaginatedQueryFixture();
  await testRuntimeOnlyMutationImportedScheduledFunctionsFixture();
  await testImportedServerValidatorsFixture();
  await testUnsupportedPatchWithoutIdValidatorFixture();
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
