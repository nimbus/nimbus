import { v } from "convex/values";

import { query } from "./_generated/server";

export const recent = query({
  args: {
    source: v.union(v.string(), v.null()),
    level: v.union(v.string(), v.null()),
    category: v.union(v.string(), v.null()),
    correlationId: v.union(v.string(), v.null()),
    // The tenant the reader is scoped to, or null for every tenant. Event
    // rows carry no tenant column yet, so the scope is applied to the rows
    // that name one and a row without a tenant passes every scope. When the
    // server records tenantId on each event, this becomes an index read on
    // by_tenantId and the tenant facet in the console narrows for real.
    tenantId: v.union(v.string(), v.null()),
    limit: v.union(v.number(), v.null()),
  },
  returns: v.array(v.any()),
  handler: async (
    ctx,
    { source, level, category, correlationId, tenantId, limit },
  ) => {
    const boundedLimit =
      limit === null || !Number.isFinite(limit)
        ? 100
        : Math.max(1, Math.min(200, Math.floor(limit)));
    const rows = await (async () => {
      if (correlationId) {
        return await ctx.db
          .query("events")
          .withIndex("by_correlationId", (q) =>
            q.eq("correlationId", correlationId),
          )
          .take(boundedLimit);
      }
      if (source) {
        return await ctx.db
          .query("events")
          .withIndex("by_source", (q) => q.eq("source", source))
          .take(boundedLimit);
      }
      if (level) {
        return await ctx.db
          .query("events")
          .withIndex("by_level", (q) => q.eq("level", level))
          .take(boundedLimit);
      }
      if (category) {
        return await ctx.db
          .query("events")
          .withIndex("by_category", (q) => q.eq("category", category))
          .take(boundedLimit);
      }
      return await ctx.db
        .query("events")
        .withIndex("by_createdAt")
        .order("desc")
        .take(boundedLimit);
    })();
    // The bundle ships the handler body alone, so the scope check stays
    // inline. A row that names no tenant passes every scope.
    if (tenantId === null) return rows;
    return rows.filter(
      (row) =>
        typeof row !== "object" ||
        row === null ||
        !("tenantId" in row) ||
        row.tenantId === tenantId,
    );
  },
});
