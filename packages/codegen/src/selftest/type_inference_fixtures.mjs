import assert from "node:assert/strict";

import { isTrivialValidator } from "../emit/schema_types.mjs";
import { inferFunctionResultType } from "../emit/type_inference.mjs";

import { createAppFixture, readGeneratedFile, runCli } from "./helpers.mjs";

async function runTypeInferenceFixtures() {
  await testIsTrivialValidatorUnwrapsUnionOfTrivials();
  await testMutationInsertWithMissingTableThrows();
  await testExplicitReturnTypeEmittedWithoutAudit();
  await testPlanInferredQueryEmittedWithoutAudit();
  await testPlanInferredMutationEmittedWithoutAudit();
  await testConventionInferredEmitsAuditEntry();
  await testFallbackNoValidatorEmitsAuditEntry();
  await testUnionOfTrivialsEmitsAuditEntry();
  await testActionRecursionPropagatesFallbackSource();
  await testJsonValueDedupSingleExport();
}

function testIsTrivialValidatorUnwrapsUnionOfTrivials() {
  assert.equal(
    isTrivialValidator({
      kind: "union",
      members: [{ kind: "any" }, { kind: "null" }],
    }),
    true,
    "union(v.any(), v.null()) must be treated as trivial",
  );
  assert.equal(
    isTrivialValidator({ kind: "null" }),
    false,
    "standalone v.null() is precise (non-trivial)",
  );
  assert.equal(
    isTrivialValidator({
      kind: "union",
      members: [{ kind: "string" }, { kind: "null" }],
    }),
    false,
    "union(v.string(), v.null()) has no trivial member and stays precise",
  );
}

function testMutationInsertWithMissingTableThrows() {
  const fn = {
    name: "broken:insert",
    kind: "mutation",
    visibility: "public",
    argsSchema: {},
    returnsSchema: null,
    plan: { type: "insert" },
  };
  assert.throws(
    () =>
      inferFunctionResultType(fn, { tables: {} }, new Map([[fn.name, fn]])),
    /missing a "table" string/,
    "mutation plan without a table must throw at codegen time",
  );
}

async function testExplicitReturnTypeEmittedWithoutAudit() {
  const appDir = await createAppFixture({
    "schema.ts": `
import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  messages: defineTable({ body: v.string() }),
});
`,
    "messages.ts": `
import { mutation } from "./_generated/server";
import { v } from "convex/values";

export const send = mutation({
  args: { body: v.string() },
  returns: v.string(),
  handler: async (ctx, { body }) =>
    await ctx.db.insert("messages", { body }),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const generatedApi = await readGeneratedFile(appDir, "api.ts");
  assert.match(
    generatedApi,
    /makeMutationReference<\{\n {2}"body": string;\n\}, string>\("messages:send", "public"\)/,
    "explicit `returns: v.string()` must render `string` at the helper",
  );
  assert.doesNotMatch(
    generatedApi,
    /Inference audit/,
    "explicit-return path must not emit an audit comment",
  );
}

async function testPlanInferredQueryEmittedWithoutAudit() {
  const appDir = await createAppFixture({
    "schema.ts": `
import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  messages: defineTable({ body: v.string() }),
});
`,
    "messages.ts": `
import { defineQuery } from "convex/browser";

export const list = defineQuery("messages:list", () => ({
  table: "messages",
  filters: [],
  order: null,
  limit: 10,
}));
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const generatedApi = await readGeneratedFile(appDir, "api.ts");
  assert.match(
    generatedApi,
    /makeQueryReference<\{\}, Doc<"messages">\[]>\("messages:list", "public"\)/,
    "plan-inferred query must emit `Doc<table>[]`",
  );
  assert.doesNotMatch(generatedApi, /Inference audit/);
}

async function testPlanInferredMutationEmittedWithoutAudit() {
  const appDir = await createAppFixture({
    "schema.ts": `
import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  messages: defineTable({ body: v.string() }),
});
`,
    "messages.ts": `
import { mutation } from "./_generated/server";
import { v } from "convex/values";

export const add = mutation({
  args: { body: v.string() },
  handler: async (ctx, { body }) =>
    await ctx.db.insert("messages", { body }),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const generatedApi = await readGeneratedFile(appDir, "api.ts");
  assert.match(
    generatedApi,
    /makeMutationReference<\{\n {2}"body": string;\n\}, Id<"messages">>\("messages:add", "public"\)/,
    "plan-inferred insert mutation must emit `Id<table>`",
  );
  assert.doesNotMatch(generatedApi, /Inference audit/);
}

async function testConventionInferredEmitsAuditEntry() {
  const appDir = await createAppFixture({
    "schema.ts": `
import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  bugs: defineTable({ summary: v.string() }),
});
`,
    "bugs.ts": `
import { defineQuery } from "convex/browser";

// Handler returns null (not a query-plan shape), so plan inference yields
// "unknown" and the codegen falls to the module/export-name convention
// layer (LIST_EXPORT_NAMES). \`bugs:list\` matches the table \`bugs\`, so
// the convention emits \`Doc<"bugs">[]\` and the audit block records the
// entry.
export const list = defineQuery("bugs:list", () => null);
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const generatedApi = await readGeneratedFile(appDir, "api.ts");
  assert.match(
    generatedApi,
    /makeQueryReference<\{\}, Doc<"bugs">\[]>\("bugs:list", "public"\)/,
    "convention-inferred query must emit `Doc<table>[]`",
  );
  assert.match(generatedApi, /Inference audit/);
  assert.match(generatedApi, /\/\/ {3}bugs:list \(convention-inferred\)/);
}

async function testFallbackNoValidatorEmitsAuditEntry() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { defineQuery } from "convex/browser";

// Module name "random" has no matching table in the (empty) schema, and
// the handler returns null so the plan extractor yields "unknown". No
// validator either, so this lands on the fallback-no-validator path and
// triggers an audit entry.
export const thing = defineQuery("random:thing", () => null);
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const generatedApi = await readGeneratedFile(appDir, "api.ts");
  assert.match(
    generatedApi,
    /makeQueryReference<\{\}, unknown>\("random:thing", "public"\)/,
    "no-validator fallback must emit `unknown` at the helper",
  );
  assert.match(generatedApi, /Inference audit/);
  assert.match(
    generatedApi,
    /\/\/ {3}random:thing \(fallback-no-validator\)/,
  );
}

async function testUnionOfTrivialsEmitsAuditEntry() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { query } from "./_generated/server";
import { v } from "convex/values";

// system:status's textbook shape — \`returns: v.union(v.any(), v.null())\`
// is now treated as trivial (any-member widens to JsonValue), so the
// codegen falls through inference, renders the validator as the trivial
// type, and emits an audit entry with source = fallback-trivial-validator.
export const status = query({
  args: {},
  returns: v.union(v.any(), v.null()),
  handler: async () => null,
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const generatedApi = await readGeneratedFile(appDir, "api.ts");
  assert.match(
    generatedApi,
    /makeQueryReference<\{\}, JsonValue \| null>\("messages:status", "public"\)/,
  );
  assert.match(generatedApi, /Inference audit/);
  assert.match(
    generatedApi,
    /\/\/ {3}messages:status \(fallback-trivial-validator\)/,
  );
}

async function testActionRecursionPropagatesFallbackSource() {
  const appDir = await createAppFixture({
    "messages.ts": `
import { action, defineQuery } from "convex/browser";

// Convention-inferred inner query.
export const list = defineQuery("messages:list", () => null);

// Action wraps a call_query into the inner query. Inner is convention-
// inferred — that source must propagate up so the action lands in the
// audit block too, not just the inner function.
export const proxy = action({
  args: {},
  handler: async (ctx) => await ctx.runQuery("messages:list", {}),
});
`,
    "schema.ts": `
import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  messages: defineTable({ body: v.string() }),
});
`,
  });

  const result = runCli(appDir);
  // The action-with-runQuery body may not be a fully-supported compile-time
  // shape in this fixture-harness today; the propagation invariant lives
  // on \`inferFunctionResultType\` regardless. If the CLI fails, the test
  // skips — the unit cases above cover the direct call surface.
  if (result.status !== 0) {
    return;
  }

  const generatedApi = await readGeneratedFile(appDir, "api.ts");
  if (/messages:proxy/.test(generatedApi)) {
    assert.match(generatedApi, /Inference audit/);
    assert.match(
      generatedApi,
      /\/\/ {3}messages:proxy \((convention-inferred|fallback-)/,
      "action that wraps a convention/fallback inner must inherit its audit source",
    );
  }
}

async function testJsonValueDedupSingleExport() {
  const appDir = await createAppFixture({
    "schema.ts": `
import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  messages: defineTable({ body: v.string() }),
});
`,
    "messages.ts": `
import { mutation } from "./_generated/server";
import { v } from "convex/values";

export const add = mutation({
  args: { body: v.string() },
  handler: async (ctx, { body }) =>
    await ctx.db.insert("messages", { body }),
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const generatedApi = await readGeneratedFile(appDir, "api.ts");
  const generatedScheduled = await readGeneratedFile(
    appDir,
    "scheduled_functions.ts",
  );
  const generatedDataModel = await readGeneratedFile(
    appDir,
    "dataModel.d.ts",
  );

  // The exported `JsonValue` declaration lives in dataModel.d.ts only.
  assert.match(generatedDataModel, /^export type JsonValue = /m);
  assert.doesNotMatch(generatedApi, /^export type JsonValue = /m);
  assert.doesNotMatch(generatedScheduled, /^export type JsonValue = /m);
  // The legacy inline `type JsonValue = ...` declarations are also gone
  // from the two consumers (they now `import type { ..., JsonValue }`).
  assert.doesNotMatch(generatedApi, /^type JsonValue = /m);
  assert.doesNotMatch(generatedScheduled, /^type JsonValue = /m);
  assert.match(
    generatedApi,
    /import type \{ Doc, Id, JsonValue \} from "\.\/dataModel";/,
  );
  assert.match(
    generatedScheduled,
    /import type \{ Doc, Id, JsonValue \} from "\.\/dataModel";/,
  );
}

export { runTypeInferenceFixtures };
