import { v } from "convex/values";

import { query } from "./_generated/server";

export const list = query({
  args: {
    adapter: v.union(v.string(), v.null()),
    status: v.union(v.string(), v.null()),
    limit: v.union(v.number(), v.null()),
  },
  returns: v.array(v.any()),
  handler: async (ctx, { adapter, status, limit }) => {
    const boundedLimit =
      limit === null || !Number.isFinite(limit)
        ? 100
        : Math.max(1, Math.min(200, Math.floor(limit)));
    if (adapter) {
      return await ctx.db
        .query("adapter_capabilities")
        .withIndex("by_adapter", (q) => q.eq("adapter", adapter))
        .take(boundedLimit);
    }
    if (status) {
      return await ctx.db
        .query("adapter_capabilities")
        .withIndex("by_status", (q) => q.eq("status", status))
        .take(boundedLimit);
    }
    return await ctx.db.query("adapter_capabilities").take(boundedLimit);
  },
});
