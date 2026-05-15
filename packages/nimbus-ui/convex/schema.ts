import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  machines: defineTable({
    name: v.string(),
    kind: v.string(),
    state: v.string(),
    provider: v.string(),
    resources: v.optional(v.any()),
    meta: v.optional(v.any()),
  })
    .index("by_name", ["name"])
    .index("by_state", ["state"])
    .index("by_provider", ["provider"]),

  services: defineTable({
    tenantId: v.string(),
    name: v.string(),
    machineId: v.optional(v.string()),
    bundleId: v.optional(v.string()),
    kind: v.string(),
    state: v.string(),
    endpoints: v.optional(v.array(v.any())),
    health: v.optional(v.any()),
  })
    .index("by_tenantId", ["tenantId"])
    .index("by_name", ["name"])
    .index("by_machineId", ["machineId"])
    .index("by_state", ["state"]),

  bundles: defineTable({
    sha256: v.string(),
    sizeBytes: v.optional(v.number()),
    sourceRef: v.optional(v.string()),
    status: v.string(),
  })
    .index("by_sha256", ["sha256"])
    .index("by_status", ["status"]),

  functions: defineTable({
    bundleId: v.string(),
    path: v.string(),
    kind: v.string(),
    argsSchema: v.optional(v.any()),
    returnsSchema: v.optional(v.any()),
  })
    .index("by_bundleId", ["bundleId"])
    .index("by_kind", ["kind"]),

  tables: defineTable({
    tenantId: v.string(),
    name: v.string(),
    schema: v.optional(v.any()),
    rowCount: v.optional(v.number()),
    lastWriteAt: v.optional(v.number()),
  })
    .index("by_tenantId", ["tenantId"])
    .index("by_name", ["name"])
    .index("by_tenantId_and_name", ["tenantId", "name"]),

  events: defineTable({
    source: v.string(),
    level: v.string(),
    category: v.string(),
    message: v.string(),
    data: v.optional(v.any()),
    correlationId: v.optional(v.string()),
    createdAt: v.number(),
  })
    .index("by_source", ["source"])
    .index("by_level", ["level"])
    .index("by_category", ["category"])
    .index("by_correlationId", ["correlationId"])
    .index("by_createdAt", ["createdAt"]),

  runs: defineTable({
    bundleId: v.optional(v.string()),
    functionPath: v.string(),
    kind: v.string(),
    durationMs: v.optional(v.number()),
    status: v.string(),
    error: v.optional(v.any()),
    startedAt: v.number(),
  })
    .index("by_bundleId", ["bundleId"])
    .index("by_functionPath", ["functionPath"])
    .index("by_status", ["status"])
    .index("by_startedAt", ["startedAt"]),

  scheduled_jobs: defineTable({
    tenantId: v.string(),
    functionPath: v.string(),
    scheduledTime: v.number(),
    status: v.string(),
    args: v.optional(v.any()),
    result: v.optional(v.any()),
  })
    .index("by_tenantId", ["tenantId"])
    .index("by_status", ["status"])
    .index("by_scheduledTime", ["scheduledTime"]),

  cron_jobs: defineTable({
    tenantId: v.string(),
    name: v.string(),
    schedule: v.string(),
    functionPath: v.string(),
    lastRunAt: v.optional(v.number()),
    nextRunAt: v.optional(v.number()),
    status: v.string(),
  })
    .index("by_tenantId", ["tenantId"])
    .index("by_status", ["status"])
    .index("by_nextRunAt", ["nextRunAt"]),

  routes: defineTable({
    method: v.string(),
    path: v.string(),
    adapter: v.string(),
    handler: v.optional(v.string()),
    authRequired: v.boolean(),
    lastRequestAt: v.optional(v.number()),
  })
    .index("by_adapter", ["adapter"])
    .index("by_path", ["path"]),

  listeners: defineTable({
    adapter: v.string(),
    protocol: v.string(),
    address: v.string(),
    state: v.string(),
    version: v.optional(v.string()),
    error: v.optional(v.string()),
  })
    .index("by_adapter", ["adapter"])
    .index("by_state", ["state"]),

  subscriptions: defineTable({
    tenantId: v.optional(v.string()),
    adapter: v.string(),
    queryKey: v.string(),
    clientCount: v.number(),
    lastDeliveryAt: v.optional(v.number()),
    error: v.optional(v.string()),
  })
    .index("by_tenantId", ["tenantId"])
    .index("by_adapter", ["adapter"]),

  ports: defineTable({
    machineId: v.optional(v.string()),
    serviceId: v.optional(v.string()),
    hostPort: v.number(),
    guestPort: v.optional(v.number()),
    protocol: v.string(),
    state: v.string(),
  })
    .index("by_machineId", ["machineId"])
    .index("by_serviceId", ["serviceId"])
    .index("by_state", ["state"]),

  adapter_capabilities: defineTable({
    adapter: v.string(),
    feature: v.string(),
    status: v.string(),
    caveat: v.optional(v.string()),
    evidence: v.optional(v.string()),
  })
    .index("by_adapter", ["adapter"])
    .index("by_status", ["status"]),

  system_status: defineTable({
    name: v.string(),
    version: v.string(),
    health: v.string(),
    startedAt: v.number(),
    updatedAt: v.number(),
    details: v.optional(v.any()),
  })
    .index("by_name", ["name"])
    .index("by_health", ["health"]),
});
