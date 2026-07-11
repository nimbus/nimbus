import { defineSchema, defineTable } from "@nimbus/nimbus/server";
import { v } from "@nimbus/nimbus/values";

export default defineSchema({
  jobs: defineTable({
    label: v.string(),
    status: v.string(),
    createdAt: v.number(),
    completedAt: v.optional(v.number()),
  }).index("by_status", ["status"]),
});
