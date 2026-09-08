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
