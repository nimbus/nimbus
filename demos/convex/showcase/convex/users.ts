import { v } from "convex/values";

import { internalMutation, query } from "./_generated/server";

export const getByEmail = query({
  args: { email: v.string() },
  handler: async (ctx, { email }) =>
    await ctx.db
      .query("users")
      .withIndex("by_email", (q) => q.eq("email", email))
      .unique(),
});

export const touch = internalMutation({
  args: { email: v.string(), name: v.string() },
  handler: async (ctx, { email, name }) =>
    await ctx.db.insert("users", { email, name }),
});
