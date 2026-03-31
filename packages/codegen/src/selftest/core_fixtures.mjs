import assert from "node:assert/strict";
import { createHash } from "node:crypto";

import {
  createAppFixture,
  readConvexFile,
  readConvexJson,
  readGeneratedFile,
  runCli,
} from "./helpers.mjs";

async function runCoreFixtures() {
  await testSupportedDefineFixture();
  await testSupportedServerFixture();
  await testSchemaFixture();
  await testAuthConfigFixture();
  await testDuplicateAuthConfigFixture();
  await testUnsupportedFixture();
}

async function testSupportedDefineFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { defineQuery, defineMutation } from "convex/browser";

export const list = defineQuery("messages:list", () => ({
  table: "messages",
  filters: [],
  order: null,
  limit: 10,
}));

export const send = defineMutation("messages:send", ({ body }) => ({
  type: "insert",
  table: "messages",
  fields: { body },
}));
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const generatedApi = await readGeneratedFile(appDir, "api.ts");
  assert.match(
    generatedApi,
    /makeQueryReference<\{\}, unknown\[]>\("messages:list", "public"\)/,
  );
  assert.match(
    generatedApi,
    /makeMutationReference<\{\}, Id<"messages">>\("messages:send", "public"\)/,
  );

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.equal(manifest.functions.length, 2);
  assert.equal(manifest.functions[0].kind, "query");
  assert.equal(manifest.functions[1].kind, "mutation");
  assert.deepEqual(manifest.functions[0].plan, {
    table: "messages",
    filters: [],
    order: null,
    limit: 10,
  });
  assert.deepEqual(manifest.functions[1].plan.fields, {
    body: { $arg: "body" },
  });
  assert.equal(manifest.functions[0].visibility, "public");

  const runtimeBundle = await readConvexFile(appDir, "bundle.mjs");
  assert.match(runtimeBundle, /globalThis\.__neovexInvoke = async function/);
  assert.match(runtimeBundle, /globalThis\.__neovexInvokeNamedLocal = invokeNamedDefinitionLocally/);
  assert.doesNotMatch(runtimeBundle, /__neovexRawHostCall/);
  assert.match(runtimeBundle, /__neovexCreateContext/);
  assert.match(runtimeBundle, /"messages:list"/);

  const runtimeBundleHash = (await readConvexFile(appDir, "bundle.sha256")).trim();
  assert.equal(
    runtimeBundleHash,
    createHash("sha256").update(runtimeBundle).digest("hex"),
  );
}

async function testSupportedServerFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { query, internalMutation } from "./_generated/server";
import { v } from "convex/values";

export const list = query({
  args: {},
  handler: async (_ctx, _args) => ({
    table: "messages",
    filters: [],
    order: null,
    limit: 25,
  }),
});

export const storeInternal = internalMutation({
  args: { body: v.string() },
  handler: async (_ctx, { body }) => ({
    type: "insert",
    table: "messages",
    fields: { body },
  }),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const generatedApi = await readGeneratedFile(appDir, "api.ts");
  assert.match(generatedApi, /export const api = /);
  assert.match(
    generatedApi,
    /list: makeQueryReference<\{\}, unknown\[]>\("messages:list", "public"\)/,
  );
  assert.match(
    generatedApi,
    /storeInternal: makeMutationReference<\{\n  "body": string;\n\}, Id<"messages">>\("messages:storeInternal", "internal"\)/,
  );
  assert.match(generatedApi, /export const internal = /);

  const generatedServer = await readGeneratedFile(appDir, "server.ts");
  assert.match(generatedServer, /internalMutation/);
  assert.match(generatedServer, /query/);

  const generatedScheduled = await readGeneratedFile(appDir, "scheduled_functions.ts");
  assert.match(
    generatedScheduled,
    /internalScheduledFunctions = \{\n  messages: \{\n    storeInternal: makeMutationReference<\{\n  "body": string;\n\}, Id<"messages">>\("messages:storeInternal", "internal"\)/,
  );

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.equal(manifest.functions.length, 2);
  assert.equal(manifest.functions[0].name, "messages:list");
  assert.equal(manifest.functions[0].visibility, "public");
  assert.equal(manifest.functions[1].name, "messages:storeInternal");
  assert.equal(manifest.functions[1].visibility, "internal");
  assert.equal(manifest.functions[1].schedulable, true);
  assert.deepEqual(manifest.functions[1].plan.fields, {
    body: { $arg: "body" },
  });
}

async function testSchemaFixture() {
  const appDir = await createAppFixture({
    "schema.ts": `
import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  messages: defineTable({
    author: v.string(),
    body: v.string(),
    channelId: v.id("channels"),
    tags: v.array(v.string()),
    metadata: v.optional(v.object({ featured: v.boolean() })),
  }).index("by_author", ["author"]),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const generatedDataModel = await readGeneratedFile(appDir, "dataModel.d.ts");
  assert.match(generatedDataModel, /export type TableNames = "messages";/);
  assert.match(generatedDataModel, /_id: Id<"messages">;/);
  assert.match(generatedDataModel, /_creationTime: number;/);
  assert.match(generatedDataModel, /"channelId": GenericId<"channels">;/);
  assert.match(generatedDataModel, /"metadata": \{\n  "featured": boolean;\n\} \| undefined;/);
  assert.match(generatedDataModel, /"messages": "by_author";/);

  const schemaManifest = await readConvexJson(appDir, "schema.json");
  assert.deepEqual(schemaManifest.tables.messages.indexes, [
    { name: "by_author", fields: ["author"] },
  ]);
  assert.equal(schemaManifest.tables.messages.fields.channelId.kind, "id");
  assert.equal(schemaManifest.tables.messages.fields.channelId.tableName, "channels");
}

async function testAuthConfigFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { query } from "./_generated/server";

export const whoami = query({
  args: {},
  handler: async (ctx) => await ctx.auth.getUserIdentity(),
});
`,
    "auth.config.ts": `
import { AuthConfig } from "convex/server";

export default {
  providers: [
    {
      domain: "https://auth.example.com",
      applicationID: "neovex-dev",
    },
    {
      type: "customJwt",
      issuer: "https://issuer.example.com",
      jwks: "data:application/json;base64,eyJrZXlzIjpbXX0=",
      algorithm: "RS256",
    },
  ],
} satisfies AuthConfig;
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const manifest = await readConvexJson(appDir, "functions.json");
  assert.equal(manifest.functions.length, 1);
  assert.equal(manifest.functions[0].name, "messages:whoami");
  assert.equal(manifest.functions[0].runtime_handler !== null, true);

  const authConfig = await readConvexJson(appDir, "auth.config.json");
  assert.deepEqual(authConfig, {
    providers: [
      {
        domain: "https://auth.example.com",
        applicationID: "neovex-dev",
      },
      {
        type: "customJwt",
        issuer: "https://issuer.example.com",
        jwks: "data:application/json;base64,eyJrZXlzIjpbXX0=",
        algorithm: "RS256",
      },
    ],
  });
}

async function testDuplicateAuthConfigFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { query } from "convex/server";

export const whoami = query({
  args: {},
  handler: async (ctx) => await ctx.auth.getUserIdentity(),
});
`,
    "auth.config.ts": `
export default { providers: [] };
`,
    "auth.config.js": `
export default { providers: [] };
`,
  });

  const result = runCli(appDir);
  assert.notEqual(result.status, 0, result.stdout);
  assert.match(
    result.stderr || result.stdout,
    /Found both .*auth\.config\.(js|ts) and .*auth\.config\.(js|ts), choose one\./,
  );
}

async function testUnsupportedFixture() {
  const appDir = await createAppFixture({
    "messages.ts": `
export const list = () => "not supported";
`,
  });

  const result = runCli(appDir);
  assert.notEqual(result.status, 0, "unsupported fixture should fail");
  assert.match(
    result.stderr,
    /requires Phase 4C runtime execution support/,
  );
}

export { runCoreFixtures };
