export type TenantListEntry =
  | string
  | { id?: string; tenantId?: string; name?: string };

export type TenantListResponse = {
  tenants?: TenantListEntry[];
};

export async function fetchTenants(
  signal: AbortSignal,
): Promise<string[] | null> {
  const response = await fetch("/api/tenants", {
    credentials: "include",
    signal,
  });
  if (!response.ok) return null;
  const body = (await response.json()) as TenantListResponse;
  const ids = (body.tenants ?? [])
    .map((entry) =>
      typeof entry === "string"
        ? entry
        : (entry.tenantId ?? entry.id ?? entry.name ?? null),
    )
    .filter((id): id is string => typeof id === "string" && id.length > 0)
    .sort((a, b) => a.localeCompare(b));
  return ids;
}
