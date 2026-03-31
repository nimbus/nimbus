import { internalScheduledFunctions } from "./_generated/scheduled_functions";
import { action, httpAction, internalMutation, mutation, query } from "./_generated/server";
import { internal } from "./_generated/api";
import { v } from "convex/values";

export const list = query({
  args: {},
  handler: async (ctx) => await ctx.db.query("messages").take(20),
});

export const byAuthor = query({
  args: { author: v.union(v.string(), v.null()) },
  handler: async (ctx, { author }) =>
    await ctx.db
      .query("messages")
      .withIndex("by_author", (q) => q.eq("author", author))
      .take(20),
});

export const maybeByAuthor = query({
  args: { author: v.union(v.string(), v.null()) },
  returns: v.array(v.object({
    _id: v.id("messages"),
    _creationTime: v.number(),
    author: v.string(),
    body: v.string(),
  })),
  handler: async (ctx, { author }) => {
    const messages = author
      ? await ctx.db
        .query("messages")
        .withIndex("by_author", (q) => q.eq("author", author))
        .take(20)
      : await ctx.db.query("messages").take(20);
    return messages.slice(0, 20);
  },
});

export const byId = query({
  args: {
    id: v.id("messages"),
  },
  handler: async (ctx, { id }) => await ctx.db.get(id),
});

export const uniqueByAuthor = query({
  args: {
    author: v.string(),
  },
  handler: async (ctx, { author }) =>
    await ctx.db
      .query("messages")
      .withIndex("by_author", (q) => q.eq("author", author))
      .unique(),
});

export const exactByAuthorAndBody = query({
  args: {
    author: v.string(),
    body: v.string(),
  },
  handler: async (ctx, { author, body }) =>
    await ctx.db
      .query("messages")
      .withIndex("by_author", (q) => q.eq("author", author))
      .filter((q) => q.eq(q.field("body"), body))
      .unique(),
});

export const send = mutation({
  args: {
    author: v.string(),
    body: v.string(),
  },
  handler: async (ctx, { author, body }) =>
    await ctx.db.insert("messages", { author, body }),
});

export const sendInternal = internalMutation({
  args: {
    author: v.string(),
    body: v.string(),
  },
  handler: async (ctx, { author, body }) =>
    await ctx.db.insert("messages", { author, body }),
});

export const sendViaAction = action({
  args: {
    author: v.string(),
    body: v.string(),
  },
  handler: async (ctx, { author, body }) =>
    await ctx.runMutation(internal.messages.sendInternal, { author, body }),
});

export const sendViaHttp = httpAction(async (ctx, request) => {
  const { author, body } = await request.json();
  const id = await ctx.runMutation(internal.messages.sendInternal, { author, body });
  return Response.json({ id }, { status: 201 });
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

export const sendAndSchedule = mutation({
  args: {
    author: v.string(),
    body: v.string(),
  },
  returns: v.id("messages"),
  handler: async (ctx, { author, body }) => {
    const id = await ctx.db.insert("messages", { author, body });
    await ctx.scheduler.runAfter(
      1_000,
      internalScheduledFunctions.messages.sendInternal,
      { author, body: `${body} (scheduled)` },
    );
    return id;
  },
});
