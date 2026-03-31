import assert from "node:assert/strict";

import {
  createAppFixture,
  readConvexFile,
  readConvexJson,
  readGeneratedFile,
  runCli,
} from "./helpers.mjs";

async function runActionFixtures() {
  await testHttpActionFixture();
  await testActionCompositionServerFixture();
  await testSchedulerServerFixture();
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
  assert.match(runtimeBundle, /op_neovex_http_route/);
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
  assert.match(runtimeBundle, /op_neovex_ctx_action/);
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

export { runActionFixtures };
