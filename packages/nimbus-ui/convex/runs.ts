import { v } from "convex/values";

import { query } from "./_generated/server";

export const recent = query({
  args: {
    bundleId: v.union(v.string(), v.null()),
    functionPath: v.union(v.string(), v.null()),
    status: v.union(v.string(), v.null()),
    // The tenant the reader is scoped to, or null for every tenant. Every
    // run row names its tenant, so a tenant scope is an index read on
    // by_tenantId_and_startedAt.
    tenantId: v.union(v.string(), v.null()),
    limit: v.union(v.number(), v.null()),
  },
  returns: v.array(v.any()),
  handler: async (ctx, { bundleId, functionPath, status, tenantId, limit }) => {
    const boundedLimit =
      limit === null || !Number.isFinite(limit)
        ? 100
        : Math.max(1, Math.min(200, Math.floor(limit)));
    if (tenantId !== null) {
      return await ctx.db
        .query("runs")
        .withIndex("by_tenantId_and_startedAt", (q) =>
          q.eq("tenantId", tenantId),
        )
        .filter((q) => {
          let narrowed = q;
          if (bundleId) narrowed = narrowed.eq(q.field("bundleId"), bundleId);
          if (functionPath) {
            narrowed = narrowed.eq(q.field("functionPath"), functionPath);
          }
          if (status) narrowed = narrowed.eq(q.field("status"), status);
          return narrowed;
        })
        .order("desc")
        .take(boundedLimit);
    }
    if (bundleId) {
      return await ctx.db
        .query("runs")
        .withIndex("by_bundleId", (q) => q.eq("bundleId", bundleId))
        .take(boundedLimit);
    }
    if (functionPath) {
      return await ctx.db
        .query("runs")
        .withIndex("by_functionPath", (q) =>
          q.eq("functionPath", functionPath),
        )
        .take(boundedLimit);
    }
    if (status) {
      return await ctx.db
        .query("runs")
        .withIndex("by_status", (q) => q.eq("status", status))
        .take(boundedLimit);
    }
    return await ctx.db
      .query("runs")
      .withIndex("by_startedAt")
      .order("desc")
      .take(boundedLimit);
  },
});

export const byId = query({
  args: {
    id: v.id("runs"),
  },
  returns: v.any(),
  handler: async (ctx, { id }) => await ctx.db.get(id),
});
