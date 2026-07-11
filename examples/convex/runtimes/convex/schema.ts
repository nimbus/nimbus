import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  digests: defineTable({
    runtime: v.union(v.literal("default"), v.literal("node")),
    algorithm: v.string(),
    input: v.string(),
    output: v.string(),
    createdAt: v.number(),
  }).index("by_created_at", ["createdAt"]),

  shareIds: defineTable({
    id: v.string(),
    createdAt: v.number(),
  }).index("by_created_at", ["createdAt"]),
});
