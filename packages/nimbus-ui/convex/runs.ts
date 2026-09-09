import { v } from "convex/values";

import { query } from "./_generated/server";

export const recent = query({
  args: {
    bundleId: v.union(v.string(), v.null()),
    functionPath: v.union(v.string(), v.null()),
    status: v.union(v.string(), v.null()),
    // The tenant the reader is scoped to, or null for every tenant. Run rows
    // carry no tenant column yet (the writer only uses the tenant to skip
    // system-tenant runs), so a row without a tenant passes every scope.
    // When the server records tenantId on each run, this becomes an index
    // read on by_tenantId.
    tenantId: v.union(v.string(), v.null()),
    limit: v.union(v.number(), v.null()),
  },
  returns: v.array(v.any()),
  handler: async (ctx, { bundleId, functionPath, status, tenantId, limit }) => {
    const boundedLimit =
      limit === null || !Number.isFinite(limit)
        ? 100
        : Math.max(1, Math.min(200, Math.floor(limit)));
    const rows = await (async () => {
      if (bundleId) {
        return await ctx.db
          .query("runs")
          .withIndex("by_bundleId", (q) => q.eq("bundleId", bundleId))
          .take(boundedLimit);
      }
      if (functionPath) {
        return await ctx.db
          .query("runs")
          .withIndex("by_functionPath", (q) =>
            q.eq("functionPath", functionPath),
          )
          .take(boundedLimit);
      }
      if (status) {
        return await ctx.db
          .query("runs")
          .withIndex("by_status", (q) => q.eq("status", status))
          .take(boundedLimit);
      }
      return await ctx.db
        .query("runs")
        .withIndex("by_startedAt")
        .order("desc")
        .take(boundedLimit);
    })();
    // The bundle ships the handler body alone, so the scope check stays
    // inline. A row that names no tenant passes every scope.
    if (tenantId === null) return rows;
    return rows.filter(
      (row) =>
        typeof row !== "object" ||
        row === null ||
        !("tenantId" in row) ||
        row.tenantId === tenantId,
    );
  },
});

export const byId = query({
  args: {
    id: v.id("runs"),
  },
  returns: v.any(),
  handler: async (ctx, { id }) => await ctx.db.get(id),
});
