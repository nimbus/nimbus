import { v } from "convex/values";

import type { Doc } from "./_generated/dataModel";
import { mutation, query } from "./_generated/server";

export const list = query({
  args: {},
  handler: async (ctx, _args) =>
    await ctx.db
      .query("tasks")
      .withIndex("by_created_at")
      .order("desc")
      .collect(),
});

export const create = mutation({
  args: {
    text: v.string(),
  },
  returns: v.id("tasks"),
  handler: async (ctx, { text }) => {
    const normalizedText = text.trim();
    if (!normalizedText) {
      throw new Error("Task text must not be empty");
    }

    return await ctx.db.insert("tasks", {
      text: normalizedText,
      completed: false,
      createdAt: Date.now(),
    });
  },
});

export const toggle = mutation({
  args: {
    id: v.id("tasks"),
  },
  returns: v.null(),
  handler: async (ctx, { id }) => {
    const task = await ctx.db.get(id) as Doc<"tasks"> | null;
    if (task === null) {
      throw new Error("Task not found");
    }

    await ctx.db.patch(id, { completed: !task.completed });
    return null;
  },
});

export const remove = mutation({
  args: {
    id: v.id("tasks"),
  },
  returns: v.null(),
  handler: async (ctx, { id }) => {
    await ctx.db.delete(id);
    return null;
  },
});
