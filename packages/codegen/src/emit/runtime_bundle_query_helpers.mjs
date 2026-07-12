function runtimeBundleQueryHelpers() {
  return `function isQueryShape(plan) {
  return isPlainObject(plan)
    && typeof plan.table === "string"
    && Array.isArray(plan.filters)
    && Object.prototype.hasOwnProperty.call(plan, "order")
    && Object.prototype.hasOwnProperty.call(plan, "limit");
}

// Compiled query plans are the same JSON the server's native dispatch
// resolves (ConvexExecutableQuery), so they execute wholesale through the
// direct query op. Replaying a plan through the runtime query-builder ops
// would be lossy — the builder API cannot express index-backed ordering,
// which a compiled plan's \`order.field\` carries.
async function executeResolvedQueryPlan(ctx, plan) {
  return await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_query", {
    query: plan,
  });
}`;
}

export { runtimeBundleQueryHelpers };
