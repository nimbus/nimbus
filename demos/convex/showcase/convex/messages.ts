import { v } from "convex/values";

import { mutation, query } from "./_generated/server";

// A locally-typed constant — hovering `MAX_PAGE` in the console Source tab shows
// `const MAX_PAGE: number` (FSV8 type-hover).
const MAX_PAGE: number = 50;

export const list = query({
  args: { channel: v.string() },
  handler: async (ctx, { channel }) => {
    const rows = await ctx.db
      .query("messages")
      .withIndex("by_channel", (q) => q.eq("channel", channel))
      .order("desc")
      .take(MAX_PAGE);
    return rows.reverse();
  },
});

export const send = mutation({
  args: { channel: v.string(), author: v.string(), body: v.string() },
  handler: async (ctx, { channel, author, body }) => {
    if (body.length === 0) throw new Error("message body must not be empty");
    await ctx.db.insert("messages", { channel, author, body, at: Date.now() });
  },
});

export const summary = query({
  args: { channel: v.string() },
  handler: async (ctx, { channel }) => {
    const rows = await ctx.db
      .query("messages")
      .withIndex("by_channel", (q) => q.eq("channel", channel))
      .collect();
    return `${rows.length} message${rows.length === 1 ? "" : "s"}`;
  },
});
