import { v } from "convex/values";

import { query } from "./_generated/server";

export const list = query({
  args: {
    bundleId: v.union(v.string(), v.null()),
    kind: v.union(v.string(), v.null()),
    limit: v.union(v.number(), v.null()),
  },
  returns: v.array(v.any()),
  handler: async (ctx, { bundleId, kind, limit }) => {
    const boundedLimit =
      limit === null || !Number.isFinite(limit)
        ? 100
        : Math.max(1, Math.min(200, Math.floor(limit)));
    if (bundleId) {
      return await ctx.db
        .query("functions")
        .withIndex("by_bundleId", (q) => q.eq("bundleId", bundleId))
        .take(boundedLimit);
    }
    if (kind) {
      return await ctx.db
        .query("functions")
        .withIndex("by_kind", (q) => q.eq("kind", kind))
        .take(boundedLimit);
    }
    return await ctx.db.query("functions").take(boundedLimit);
  },
});
