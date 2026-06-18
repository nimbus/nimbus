import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  messages: defineTable({
    channel: v.string(),
    author: v.string(),
    body: v.string(),
    at: v.number(),
  }).index("by_channel", ["channel"]),

  users: defineTable({
    email: v.string(),
    name: v.string(),
  }).index("by_email", ["email"]),
});
