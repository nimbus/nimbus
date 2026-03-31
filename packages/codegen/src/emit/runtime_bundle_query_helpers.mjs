function runtimeBundleQueryHelpers() {
  return `function isQueryShape(plan) {
  return isPlainObject(plan)
    && typeof plan.table === "string"
    && Array.isArray(plan.filters)
    && Object.prototype.hasOwnProperty.call(plan, "order")
    && Object.prototype.hasOwnProperty.call(plan, "limit");
}

function createConstraintBuilderFromPlan(builder, filters) {
  for (const filter of filters ?? []) {
    const field = builder.field(filter.field);
    switch (filter.op) {
      case "eq":
        builder.eq(field, filter.value);
        break;
      case "neq":
        builder.neq(field, filter.value);
        break;
      case "gt":
        builder.gt(field, filter.value);
        break;
      case "gte":
        builder.gte(field, filter.value);
        break;
      case "lt":
        builder.lt(field, filter.value);
        break;
      case "lte":
        builder.lte(field, filter.value);
        break;
      default:
        throw new Error(\`unsupported convex filter op: \${filter.op}\`);
    }
  }
  return builder;
}

function buildQueryFromPlan(ctx, query) {
  let builder = ctx.db.query(query.table);
  if (Array.isArray(query.filters) && query.filters.length > 0) {
    builder = builder.filter((q) => createConstraintBuilderFromPlan(q, query.filters));
  }
  if (query.order && typeof query.order.direction === "string") {
    builder = builder.order(query.order.direction);
  }
  return builder;
}

async function executeResolvedQueryPlan(ctx, plan) {
  if (isPlainObject(plan) && plan.type === "get") {
    return await ctx.db.get(plan.table, plan.id);
  }
  if (isPlainObject(plan) && plan.type === "first") {
    return await buildQueryFromPlan(ctx, plan.query).first();
  }
  if (isPlainObject(plan) && plan.type === "unique") {
    return await buildQueryFromPlan(ctx, plan.query).unique();
  }
  if (isQueryShape(plan)) {
    const builder = buildQueryFromPlan(ctx, plan);
    return typeof plan.limit === "number"
      ? await builder.take(plan.limit)
      : await builder.collect();
  }
  return await globalThis.__neovexAsyncHostValue("op_neovex_ctx_query", {
    query: plan,
    session_id: "convex-runtime-query-plan",
  });
}`;
}

export { runtimeBundleQueryHelpers };
