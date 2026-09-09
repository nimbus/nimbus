import { v } from "convex/values";

import { query } from "./_generated/server";

export const recent = query({
  args: {
    source: v.union(v.string(), v.null()),
    level: v.union(v.string(), v.null()),
    category: v.union(v.string(), v.null()),
    correlationId: v.union(v.string(), v.null()),
    // The tenant the reader is scoped to, or null for every tenant. A
    // tenant scope is an index read on by_tenantId_and_createdAt; a line
    // the server wrote outside any tenant carries no tenantId and shows
    // only under the every-tenant scope.
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
    // A correlated read is one run. The run id is unique across tenants,
    // so the tenant scope is a check on the rows rather than the index.
    if (correlationId) {
      const rows = await ctx.db
        .query("events")
        .withIndex("by_correlationId", (q) =>
          q.eq("correlationId", correlationId),
        )
        .take(boundedLimit);
      if (tenantId === null) return rows;
      return rows.filter(
        (row) =>
          typeof row === "object" &&
          row !== null &&
          "tenantId" in row &&
          row.tenantId === tenantId,
      );
    }
    if (tenantId !== null) {
      return await ctx.db
        .query("events")
        .withIndex("by_tenantId_and_createdAt", (q) =>
          q.eq("tenantId", tenantId),
        )
        .filter((q) => {
          let narrowed = q;
          if (source) narrowed = narrowed.eq(q.field("source"), source);
          if (level) narrowed = narrowed.eq(q.field("level"), level);
          if (category) narrowed = narrowed.eq(q.field("category"), category);
          return narrowed;
        })
        .order("desc")
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
  },
});
