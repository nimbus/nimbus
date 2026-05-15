import { v } from "convex/values";

import { query } from "./_generated/server";

export const list = query({
  args: {
    tenantId: v.union(v.string(), v.null()),
    adapter: v.union(v.string(), v.null()),
    limit: v.union(v.number(), v.null()),
  },
  returns: v.array(v.any()),
  handler: async (ctx, { tenantId, adapter, limit }) => {
    const boundedLimit =
      limit === null || !Number.isFinite(limit)
        ? 100
        : Math.max(1, Math.min(200, Math.floor(limit)));
    if (tenantId) {
      return await ctx.db
        .query("subscriptions")
        .withIndex("by_tenantId", (q) => q.eq("tenantId", tenantId))
        .take(boundedLimit);
    }
    if (adapter) {
      return await ctx.db
        .query("subscriptions")
        .withIndex("by_adapter", (q) => q.eq("adapter", adapter))
        .take(boundedLimit);
    }
    return await ctx.db.query("subscriptions").take(boundedLimit);
  },
});
