import { v } from "convex/values";

import { query } from "./_generated/server";

export const list = query({
  args: {
    machineId: v.union(v.string(), v.null()),
    serviceId: v.union(v.string(), v.null()),
    observedPhase: v.union(v.string(), v.null()),
    limit: v.union(v.number(), v.null()),
  },
  returns: v.array(v.any()),
  handler: async (ctx, { machineId, serviceId, observedPhase, limit }) => {
    const boundedLimit =
      limit === null || !Number.isFinite(limit)
        ? 100
        : Math.max(1, Math.min(200, Math.floor(limit)));
    if (machineId) {
      return await ctx.db
        .query("ports")
        .withIndex("by_machineId", (q) => q.eq("machineId", machineId))
        .take(boundedLimit);
    }
    if (serviceId) {
      return await ctx.db
        .query("ports")
        .withIndex("by_serviceId", (q) => q.eq("serviceId", serviceId))
        .take(boundedLimit);
    }
    if (observedPhase) {
      return await ctx.db
        .query("ports")
        .withIndex("by_observedPhase", (q) =>
          q.eq("observedPhase", observedPhase),
        )
        .take(boundedLimit);
    }
    return await ctx.db.query("ports").take(boundedLimit);
  },
});
