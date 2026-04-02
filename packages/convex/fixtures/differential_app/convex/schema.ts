import { v } from "convex/values";

import { defineSchema, defineTable } from "convex/server";

export default defineSchema({
  messages: defineTable({
    author: v.string(),
    body: v.string(),
    rank: v.number(),
  }).index("by_rank", ["rank"]),
});
