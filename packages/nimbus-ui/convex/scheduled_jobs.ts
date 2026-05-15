import { v } from "convex/values";

import { query } from "./_generated/server";

export const list = query({
  args: {
    tenantId: v.union(v.string(), v.null()),
    status: v.union(v.string(), v.null()),
    limit: v.union(v.number(), v.null()),
  },
  returns: v.array(v.any()),
  handler: async (ctx, { tenantId, status, limit }) => {
    const boundedLimit =
      limit === null || !Number.isFinite(limit)
        ? 100
        : Math.max(1, Math.min(200, Math.floor(limit)));
    if (tenantId) {
      return await ctx.db
        .query("scheduled_jobs")
        .withIndex("by_tenantId", (q) => q.eq("tenantId", tenantId))
        .take(boundedLimit);
    }
    if (status) {
      return await ctx.db
        .query("scheduled_jobs")
        .withIndex("by_status", (q) => q.eq("status", status))
        .take(boundedLimit);
    }
    return await ctx.db
      .query("scheduled_jobs")
      .withIndex("by_scheduledTime")
      .order("desc")
      .take(boundedLimit);
  },
});
