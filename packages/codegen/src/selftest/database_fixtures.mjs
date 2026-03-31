import assert from "node:assert/strict";

import {
  createAppFixture,
  readConvexFile,
  readConvexJson,
  readGeneratedFile,
  runCli,
} from "./helpers.mjs";

async function runDatabaseFixtures() {
  await testCtxDbServerFixture();
  await testPaginatedQueryBuilderServerFixture();
  await testCtxDbFilterServerFixture();
  await testCtxDbFirstServerFixture();
  await testCtxDbUniqueServerFixture();
  await testCtxDbIndexedFilterUniqueServerFixture();
  await testCtxDbPatchDeleteServerFixture();
  await testCtxDbGetServerFixture();
}

async function testCtxDbServerFixture() {
  const appDir = await createAppFixture({
    "schema.ts": `
import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  messages: defineTable({
    author: v.string(),
    body: v.string(),
    rank: v.number(),
  }).index("by_rank", ["rank"]),
});
`,
    "messages.ts": `
import { mutation, query } from "./_generated/server";
import { v } from "convex/values";

export const topRanked = query({
  args: { minimumRank: v.number() },
  handler: async (ctx, { minimumRank }) =>
    await ctx.db
      .query("messages")
      .withIndex("by_rank", (q) => q.gte("rank", minimumRank))
      .order("desc")
      .take(5),
});

export const send = mutation({
  args: {
    author: v.string(),
    body: v.string(),
    rank: v.number(),
  },
  returns: v.string(),
  handler: async (ctx, { author, body, rank }) =>
    await ctx.db.insert("messages", { author, body, rank }),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.deepEqual(manifest.functions[0].plan, {
    table: "messages",
    filters: [{ field: "rank", op: "gte", value: { $arg: "minimumRank" } }],
    order: { field: "rank", direction: "desc" },
    limit: 5,
  });
  assert.deepEqual(manifest.functions[1].plan, {
    type: "insert",
    table: "messages",
    fields: {
      author: { $arg: "author" },
      body: { $arg: "body" },
      rank: { $arg: "rank" },
    },
  });

  const runtimeBundle = await readConvexFile(appDir, "bundle.mjs");
  assert.match(runtimeBundle, /executeQueryDefinition/);
  assert.match(runtimeBundle, /__neovexCreateContext/);
  assert.match(runtimeBundle, /ctx\.db\.query/);
  assert.match(runtimeBundle, /executeMutationDefinition/);
  assert.match(runtimeBundle, /ctx\.db\.insert/);
}

async function testPaginatedQueryBuilderServerFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { paginatedQuery } from "./_generated/server";

export const listPage = paginatedQuery({
  args: {},
  handler: async (ctx, _args) => await ctx.db.query("messages"),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.deepEqual(manifest.functions[0].plan, {
    table: "messages",
    filters: [],
    order: null,
    limit: null,
  });

  const generatedApi = await readGeneratedFile(appDir, "api.ts");
  assert.match(
    generatedApi,
    /listPage: makePaginatedQueryReference<\{\}, unknown>\("messages:listPage", "public"\)/,
  );

  const runtimeBundle = await readConvexFile(appDir, "bundle.mjs");
  assert.match(runtimeBundle, /executePaginatedQueryDefinition/);
  assert.match(runtimeBundle, /op_neovex_ctx_paginated_query/);
}

async function testCtxDbFilterServerFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { query } from "./_generated/server";
import { v } from "convex/values";

export const matchingAuthors = query({
  args: {
    author: v.string(),
    minimumRank: v.number(),
  },
  handler: async (ctx, { author, minimumRank }) =>
    await ctx.db
      .query("messages")
      .filter((q) =>
        q.eq(q.field("author"), author).gte("rank", minimumRank),
      )
      .collect(),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.deepEqual(manifest.functions[0].plan, {
    table: "messages",
    filters: [
      { field: "author", op: "eq", value: { $arg: "author" } },
      { field: "rank", op: "gte", value: { $arg: "minimumRank" } },
    ],
    order: null,
    limit: null,
  });
}

async function testCtxDbFirstServerFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { query } from "./_generated/server";
import { v } from "convex/values";

export const latestByAuthor = query({
  args: {
    author: v.string(),
  },
  handler: async (ctx, { author }) =>
    await ctx.db
      .query("messages")
      .filter((q) => q.eq(q.field("author"), author))
      .order("desc")
      .first(),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.deepEqual(manifest.functions[0].plan, {
    type: "first",
    query: {
      table: "messages",
      filters: [{ field: "author", op: "eq", value: { $arg: "author" } }],
      order: { field: "author", direction: "desc" },
      limit: 1,
    },
  });
}

async function testCtxDbUniqueServerFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { query } from "./_generated/server";
import { v } from "convex/values";

export const uniqueByAuthor = query({
  args: {
    author: v.string(),
  },
  handler: async (ctx, { author }) =>
    await ctx.db
      .query("messages")
      .filter((q) => q.eq(q.field("author"), author))
      .unique(),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.deepEqual(manifest.functions[0].plan, {
    type: "unique",
    query: {
      table: "messages",
      filters: [{ field: "author", op: "eq", value: { $arg: "author" } }],
      order: null,
      limit: 2,
    },
  });
}

async function testCtxDbIndexedFilterUniqueServerFixture() {
  const appDir = await createAppFixture({
    "schema.ts": `
import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  messages: defineTable({
    author: v.string(),
    body: v.string(),
  }).index("by_author", ["author"]),
});
`,
    "messages.ts": `
import { query } from "./_generated/server";
import { v } from "convex/values";

export const exactByAuthorAndBody = query({
  args: {
    author: v.string(),
    body: v.string(),
  },
  handler: async (ctx, { author, body }) =>
    await ctx.db
      .query("messages")
      .withIndex("by_author", (q) => q.eq("author", author))
      .filter((q) => q.eq(q.field("body"), body))
      .unique(),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.deepEqual(manifest.functions[0].plan, {
    type: "unique",
    query: {
      table: "messages",
      filters: [
        { field: "author", op: "eq", value: { $arg: "author" } },
        { field: "body", op: "eq", value: { $arg: "body" } },
      ],
      order: null,
      limit: 2,
    },
  });
}

async function testCtxDbPatchDeleteServerFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { mutation } from "./_generated/server";
import { v } from "convex/values";

export const rename = mutation({
  args: {
    id: v.id("messages"),
    body: v.string(),
  },
  handler: async (ctx, { id, body }) => await ctx.db.patch(id, { body }),
});

export const remove = mutation({
  args: {
    id: v.id("messages"),
  },
  handler: async (ctx, { id }) => await ctx.db.delete(id),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.deepEqual(manifest.functions[0].plan, {
    type: "update",
    table: "messages",
    id: { $arg: "id" },
    patch: {
      body: { $arg: "body" },
    },
  });
  assert.deepEqual(manifest.functions[1].plan, {
    type: "delete",
    table: "messages",
    id: { $arg: "id" },
  });
}

async function testCtxDbGetServerFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { query } from "./_generated/server";
import { v } from "convex/values";

export const byId = query({
  args: {
    id: v.id("messages"),
  },
  handler: async (ctx, { id }) => await ctx.db.get(id),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.deepEqual(manifest.functions[0].plan, {
    type: "get",
    table: "messages",
    id: { $arg: "id" },
  });
}

export { runDatabaseFixtures };
