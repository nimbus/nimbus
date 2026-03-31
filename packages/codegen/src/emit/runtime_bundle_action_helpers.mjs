function runtimeBundleActionHelpers() {
  return `async function executeResolvedActionPlan(ctx, plan, request) {
  if (!isPlainObject(plan) || typeof plan.type !== "string") {
    return await globalThis.__neovexAsyncHostValue("op_neovex_ctx_action", {
      action: plan,
      session_id: request.kind + ":" + request.function_name,
    });
  }

  switch (plan.type) {
    case "query":
      return await executeResolvedQueryPlan(ctx, plan.query);
    case "paginated_query":
      return await globalThis.__neovexAsyncHostValue("op_neovex_ctx_paginated_query", {
        query: plan.query.query,
        page_size: plan.query.page_size,
        cursor: plan.query.after ?? null,
        session_id: request.kind + ":" + request.function_name,
      });
    case "mutation":
      return await executeResolvedMutationPlan(ctx, plan.mutation);
    case "call_query":
      return ctx.runQuery(
        { kind: "query", name: plan.name, visibility: plan.visibility ?? "public" },
        plan.args ?? {},
      );
    case "call_mutation":
      return ctx.runMutation(
        { kind: "mutation", name: plan.name, visibility: plan.visibility ?? "public" },
        plan.args ?? {},
      );
    case "call_action":
      return ctx.runAction(
        { kind: "action", name: plan.name, visibility: plan.visibility ?? "public" },
        plan.args ?? {},
      );
    case "schedule_run_after":
    case "schedule_run_at":
    case "schedule_cancel":
      return await executeResolvedMutationPlan(ctx, plan);
    default:
      return await globalThis.__neovexAsyncHostValue("op_neovex_ctx_action", {
        action: plan,
        session_id: request.kind + ":" + request.function_name,
      });
  }
}`;
}

export { runtimeBundleActionHelpers };
