import { v } from "convex/values";

import { internalScheduledFunctions } from "./_generated/scheduled_functions";
import { internalMutation, mutation, paginatedQuery, query } from "./_generated/server";

export const list = query({
  args: {},
  handler: async (ctx, _args) => await ctx.db.query("messages").take(20),
});

export const byAuthor = query({
  args: {
    author: v.string(),
  },
  handler: async (ctx, { author }) =>
    await ctx.db
      .query("messages")
      .filter((q) => q.eq(q.field("author"), author))
      .take(20),
});

export const maybeByAuthor = query({
  args: {
    author: v.union(v.string(), v.null()),
  },
  returns: v.array(v.object({
    _id: v.id("messages"),
    _creationTime: v.number(),
    _updateTime: v.number(),
    author: v.string(),
    body: v.string(),
  })),
  handler: async (ctx, { author }) => {
    const messages = author
      ? await ctx.db
        .query("messages")
        .filter((q) => q.eq(q.field("author"), author))
        .take(20)
      : await ctx.db.query("messages").take(20);
    return messages.slice(0, 20);
  },
});

export const latestByAuthor = query({
  args: {
    author: v.string(),
  },
  handler: async (ctx, { author }) =>
    await ctx.db
      .query("messages")
      .filter((q) => q.eq(q.field("author"), author))
      .order("desc")
      .first(),
});

export const uniqueByAuthor = query({
  args: {
    author: v.string(),
  },
  handler: async (ctx, { author }) =>
    await ctx.db
      .query("messages")
      .filter((q) => q.eq(q.field("author"), author))
      .unique(),
});

export const byId = query({
  args: {
    id: v.id("messages"),
  },
  handler: async (ctx, { id }) => await ctx.db.get(id),
});

export const listPage = paginatedQuery({
  args: {
    author: v.union(v.string(), v.null()),
  },
  returns: v.object({
    _id: v.id("messages"),
    _creationTime: v.number(),
    _updateTime: v.number(),
    author: v.string(),
    body: v.string(),
  }),
  handler: async (ctx, { author }) => {
    const normalizedAuthor = author?.trim();
    if (normalizedAuthor) {
      return ctx.db
        .query("messages")
        .filter((q) => q.eq(q.field("author"), normalizedAuthor));
    }
    return ctx.db.query("messages");
  },
});

export const sendInternal = internalMutation({
  args: {
    author: v.string(),
    body: v.string(),
  },
  handler: async (ctx, { author, body }) =>
    await ctx.db.insert("messages", { author, body }),
});

export const send = mutation({
  args: {
    author: v.string(),
    body: v.string(),
  },
  returns: v.string(),
  handler: async (ctx, { author, body }) =>
    await ctx.db.insert("messages", { author, body }),
});

export const scheduleSend = mutation({
  args: {
    author: v.string(),
    body: v.string(),
    delayMs: v.number(),
  },
  handler: async (ctx, { author, body, delayMs }) =>
    await ctx.scheduler.runAfter(
      delayMs,
      internalScheduledFunctions.messages.sendInternal,
      { author, body },
    ),
});

export const rename = mutation({
  args: {
    id: v.id("messages"),
    body: v.string(),
  },
  returns: v.string(),
  handler: async (ctx, { id, body }) => await ctx.db.patch(id, { body }),
});

export const remove = mutation({
  args: {
    id: v.id("messages"),
  },
  handler: async (ctx, { id }) => await ctx.db.delete(id),
});
