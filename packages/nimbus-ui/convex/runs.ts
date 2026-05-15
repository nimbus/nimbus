import { v } from "convex/values";

import { query } from "./_generated/server";

export const recent = query({
  args: {
    bundleId: v.union(v.string(), v.null()),
    functionPath: v.union(v.string(), v.null()),
    status: v.union(v.string(), v.null()),
    limit: v.union(v.number(), v.null()),
  },
  returns: v.array(v.any()),
  handler: async (ctx, { bundleId, functionPath, status, limit }) => {
    const boundedLimit =
      limit === null || !Number.isFinite(limit)
        ? 100
        : Math.max(1, Math.min(200, Math.floor(limit)));
    if (bundleId) {
      return await ctx.db
        .query("runs")
        .withIndex("by_bundleId", (q) => q.eq("bundleId", bundleId))
        .take(boundedLimit);
    }
    if (functionPath) {
      return await ctx.db
        .query("runs")
        .withIndex("by_functionPath", (q) => q.eq("functionPath", functionPath))
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
