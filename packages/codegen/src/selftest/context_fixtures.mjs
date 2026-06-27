import assert from "node:assert/strict";

import { createContextProxy } from "../planner/context_api.mjs";

async function runContextFixtures() {
  await testConvexNestedCallMatrix();
}

async function testConvexNestedCallMatrix() {
  assert.deepEqual(await nestedCallOutcome("query"), {
    operations: ["call_query"],
    errors: {
      runMutation: "ctx.runMutation requires the Phase 4C runtime",
      runAction: "ctx.runAction requires the Phase 4C runtime",
    },
  });
  assert.deepEqual(await nestedCallOutcome("mutation"), {
    operations: ["call_query", "call_mutation"],
    errors: {
      runAction: "ctx.runAction requires the Phase 4C runtime",
    },
  });
  assert.deepEqual(await nestedCallOutcome("action"), {
    operations: ["call_query", "call_mutation", "call_action"],
    errors: {},
  });
  assert.deepEqual(await nestedCallOutcome("http_action"), {
    operations: ["call_query", "call_mutation", "call_action"],
    errors: {},
  });
}

async function nestedCallOutcome(kind) {
  const operationLog = [];
  const ctx = createContextProxy(
    "convex/messages.ts",
    {},
    kind,
    operationLog,
    {},
  );
  const errors = {};
  for (const [label, invoke] of [
    ["runQuery", () => ctx.runQuery(functionRef("messages:list"), {})],
    ["runMutation", () => ctx.runMutation(functionRef("messages:send"), {})],
    ["runAction", () => ctx.runAction(functionRef("messages:fanout"), {})],
  ]) {
    try {
      await invoke();
    } catch (error) {
      errors[label] = error.message;
    }
  }
  return {
    operations: operationLog.map((operation) => operation.type),
    errors,
  };
}

function functionRef(name) {
  return { name, visibility: "public" };
}

export { runContextFixtures };
