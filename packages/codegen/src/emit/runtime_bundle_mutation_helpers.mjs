function runtimeBundleMutationHelpers() {
  return `async function executeResolvedMutationPlan(ctx, plan) {
  if (isPlainObject(plan) && plan.type === "insert") {
    return ctx.db.insert(plan.table, plan.fields);
  }
  if (isPlainObject(plan) && plan.type === "update") {
    return ctx.db.patch(plan.table, plan.id, plan.patch);
  }
  if (isPlainObject(plan) && plan.type === "delete") {
    return ctx.db.delete(plan.table, plan.id);
  }
  if (isPlainObject(plan) && plan.type === "schedule_run_after") {
    return ctx.scheduler.runAfter(
      plan.delay_ms,
      {
        kind: "mutation",
        name: plan.name,
        visibility: plan.visibility ?? "public",
      },
      plan.args ?? {},
    );
  }
  if (isPlainObject(plan) && plan.type === "schedule_run_at") {
    return ctx.scheduler.runAt(
      plan.timestamp_ms,
      {
        kind: "mutation",
        name: plan.name,
        visibility: plan.visibility ?? "public",
      },
      plan.args ?? {},
    );
  }
  if (isPlainObject(plan) && plan.type === "schedule_cancel") {
    return ctx.scheduler.cancel(plan.job_id);
  }
  if (isPlainObject(plan) && (plan.type === "get" || plan.type === "first" || plan.type === "unique")) {
    return await executeResolvedQueryPlan(ctx, plan);
  }
  if (isQueryShape(plan)) {
    return await executeResolvedQueryPlan(ctx, plan);
  }
  return await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_mutation", {
    mutation: plan,
    session_id: "convex-runtime-mutation-plan",
  });
}`;
}

export { runtimeBundleMutationHelpers };
