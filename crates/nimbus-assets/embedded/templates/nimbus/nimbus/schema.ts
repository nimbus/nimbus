import { defineSchema, defineTable } from "@nimbus/nimbus/server";
import { v } from "@nimbus/nimbus/values";

export default defineSchema({
  messages: defineTable({
    author: v.string(),
    body: v.string(),
  }).index("by_author", ["author"]),
});
