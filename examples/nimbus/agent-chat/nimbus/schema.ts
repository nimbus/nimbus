import { defineSchema, defineTable } from "@nimbus/nimbus/server";
import { v } from "@nimbus/nimbus/values";

export default defineSchema({
  messages: defineTable({
    conversationId: v.string(),
    role: v.string(),
    text: v.string(),
    tool: v.optional(v.string()),
    createdAt: v.number(),
  }).index("by_conversation", ["conversationId"]),
  agentMemory: defineTable({
    conversationId: v.string(),
    text: v.string(),
    createdAt: v.number(),
  }).index("by_conversation", ["conversationId"]),
});
