import { v } from "convex/values";

import { query } from "./_generated/server";

export const list = query({
  args: {
    tenantId: v.union(v.string(), v.null()),
    limit: v.union(v.number(), v.null()),
  },
  returns: v.array(v.any()),
  handler: async (ctx, { tenantId, limit }) => {
    const boundedLimit =
      limit === null || !Number.isFinite(limit)
        ? 100
        : Math.max(1, Math.min(200, Math.floor(limit)));
    if (tenantId) {
      return await ctx.db
        .query("tables")
        .withIndex("by_tenantId", (q) => q.eq("tenantId", tenantId))
        .take(boundedLimit);
    }
    return await ctx.db.query("tables").take(boundedLimit);
  },
});

export const byName = query({
  args: {
    tenantId: v.string(),
    name: v.string(),
  },
  returns: v.any(),
  handler: async (ctx, { tenantId, name }) =>
    await ctx.db
      .query("tables")
      .withIndex("by_tenantId_and_name", (q) =>
        q.eq("tenantId", tenantId).eq("name", name),
      )
      .unique(),
});
