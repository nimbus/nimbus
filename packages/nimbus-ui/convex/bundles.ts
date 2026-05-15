import { v } from "convex/values";

import { query } from "./_generated/server";

export const list = query({
  args: {
    status: v.union(v.string(), v.null()),
    limit: v.union(v.number(), v.null()),
  },
  returns: v.array(v.any()),
  handler: async (ctx, { status, limit }) => {
    const boundedLimit =
      limit === null || !Number.isFinite(limit)
        ? 100
        : Math.max(1, Math.min(200, Math.floor(limit)));
    if (status) {
      return await ctx.db
        .query("bundles")
        .withIndex("by_status", (q) => q.eq("status", status))
        .take(boundedLimit);
    }
    return await ctx.db.query("bundles").take(boundedLimit);
  },
});

export const bySha256 = query({
  args: {
    sha256: v.string(),
  },
  returns: v.any(),
  handler: async (ctx, { sha256 }) =>
    await ctx.db
      .query("bundles")
      .withIndex("by_sha256", (q) => q.eq("sha256", sha256))
      .unique(),
});
