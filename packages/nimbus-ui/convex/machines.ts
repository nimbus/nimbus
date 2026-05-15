import { v } from "convex/values";

import { query } from "./_generated/server";

export const list = query({
  args: {
    state: v.union(v.string(), v.null()),
    provider: v.union(v.string(), v.null()),
    limit: v.union(v.number(), v.null()),
  },
  returns: v.array(v.any()),
  handler: async (ctx, { state, provider, limit }) => {
    const boundedLimit =
      limit === null || !Number.isFinite(limit)
        ? 100
        : Math.max(1, Math.min(200, Math.floor(limit)));
    if (state) {
      return await ctx.db
        .query("machines")
        .withIndex("by_state", (q) => q.eq("state", state))
        .take(boundedLimit);
    }
    if (provider) {
      return await ctx.db
        .query("machines")
        .withIndex("by_provider", (q) => q.eq("provider", provider))
        .take(boundedLimit);
    }
    return await ctx.db.query("machines").take(boundedLimit);
  },
});

export const byId = query({
  args: {
    id: v.id("machines"),
  },
  returns: v.any(),
  handler: async (ctx, { id }) => await ctx.db.get(id),
});
