import { v } from "convex/values";

import { query } from "./_generated/server";

export const recent = query({
  args: {
    source: v.union(v.string(), v.null()),
    level: v.union(v.string(), v.null()),
    category: v.union(v.string(), v.null()),
    correlationId: v.union(v.string(), v.null()),
    limit: v.union(v.number(), v.null()),
  },
  returns: v.array(v.any()),
  handler: async (ctx, { source, level, category, correlationId, limit }) => {
    const boundedLimit =
      limit === null || !Number.isFinite(limit)
        ? 100
        : Math.max(1, Math.min(200, Math.floor(limit)));
    if (correlationId) {
      return await ctx.db
        .query("events")
        .withIndex("by_correlationId", (q) => q.eq("correlationId", correlationId))
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
