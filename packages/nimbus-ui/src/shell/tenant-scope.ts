// Capability flag, not a preference: the operator observability surfaces read
// the `events` table, which carries no tenant column, so a tenant filter cannot
// be honored there yet. The top-nav selector and the observability route read
// this one constant so the control and the page can never disagree about
// whether filtering works. Flip it to `true` in the same change that adds the
// column, the `by_tenantId` index, and the query arg.
export const EVENTS_TABLE_HAS_TENANT_COLUMN = false;

export type TenantScope =
  | { kind: "all" }
  | { kind: "specific"; tenantId: string };

export function parseTenantScope(raw: string | undefined | null): TenantScope {
  if (typeof raw !== "string") return { kind: "all" };
  const trimmed = raw.trim();
  if (trimmed.length === 0) return { kind: "all" };
  return { kind: "specific", tenantId: trimmed };
}

export function serializeTenantScope(scope: TenantScope): string | undefined {
  return scope.kind === "specific" ? scope.tenantId : undefined;
}

export function describeTenantScope(scope: TenantScope): string {
  return scope.kind === "all" ? "all tenants" : scope.tenantId;
}
