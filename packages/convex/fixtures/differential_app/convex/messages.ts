import { v } from "convex/values";
import { paginationOptsValidator } from "convex/server";

import { mutation, query } from "./_generated/server";

export const byAuthor = query({
  args: {
    author: v.string(),
  },
  handler: async (ctx, { author }) =>
    await ctx.db
      .query("messages")
      .withIndex("by_rank", (q) => q.gte("rank", 0))
      .filter((q) => q.eq(q.field("author"), author))
      .collect(),
});

export const listPage = query({
  args: {
    author: v.string(),
    paginationOpts: paginationOptsValidator,
  },
  handler: async (ctx, { author, paginationOpts }) =>
    await ctx.db
      .query("messages")
      .withIndex("by_rank", (q) => q.gte("rank", 0))
      .filter((q) => q.eq(q.field("author"), author))
      .paginate(paginationOpts),
});

export const send = mutation({
  args: {
    author: v.string(),
    body: v.string(),
    rank: v.number(),
  },
  handler: async (ctx, { author, body, rank }) =>
    await ctx.db.insert("messages", { author, body, rank }),
});
