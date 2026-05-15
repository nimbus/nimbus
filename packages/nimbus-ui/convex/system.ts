import { v } from "convex/values";

import { query } from "./_generated/server";

export const status = query({
  args: {},
  returns: v.union(v.any(), v.null()),
  handler: async (ctx) =>
    await ctx.db
      .query("system_status")
      .withIndex("by_name", (q) => q.eq("name", "server"))
      .unique(),
});
