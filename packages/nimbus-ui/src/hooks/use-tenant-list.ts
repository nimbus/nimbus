import { useEffect, useState } from "react";

export type TenantListEntry = {
  id: string;
  backend?: string;
};

type TenantListResponse = {
  tenants?: Array<
    string | { id?: string; tenantId?: string; name?: string; backend?: string }
  >;
};

export type TenantListState =
  | { kind: "loading" }
  | { kind: "loaded"; tenants: TenantListEntry[] }
  | { kind: "error"; message: string };

// Normalize the `/api/tenants` payload — entries arrive as bare id strings or
// as objects under any of `tenantId`/`id`/`name` — into sorted, non-empty
// entries. Shared by both readers below so the parse lives in one place.
function normalizeEntries(body: TenantListResponse): TenantListEntry[] {
  return (body.tenants ?? [])
    .map<TenantListEntry | null>((entry) => {
      if (typeof entry === "string") return { id: entry };
      const id = entry.tenantId ?? entry.id ?? entry.name;
      if (!id) return null;
      return { id, backend: entry.backend };
    })
    .filter((entry): entry is TenantListEntry => entry !== null)
    .sort((a, b) => a.id.localeCompare(b.id));
}

// Throwing reader for the reactive hook and any caller that wants full entries
// (id + backend). Rejects on a non-OK response with the error-envelope message.
export async function loadTenantList(
  signal: AbortSignal,
): Promise<TenantListEntry[]> {
  const response = await fetch("/api/tenants", {
    credentials: "include",
    signal,
  });
  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as {
      error?: { message?: string };
    } | null;
    throw new Error(
      body?.error?.message ?? `Request failed: ${response.status}`,
    );
  }
  return normalizeEntries((await response.json()) as TenantListResponse);
}

// Non-throwing id-only reader for route loaders and bootstrap effects that only
// need the ids and treat a non-OK response as "no list" (null) rather than an
// error. Network faults still reject, matching the contract these call sites
// were built around before the tenant-list fetchers were consolidated here.
export async function fetchTenants(
  signal: AbortSignal,
): Promise<string[] | null> {
  const response = await fetch("/api/tenants", {
    credentials: "include",
    signal,
  });
  if (!response.ok) return null;
  return normalizeEntries((await response.json()) as TenantListResponse).map(
    (entry) => entry.id,
  );
}

export function useTenantList(): TenantListState {
  const [state, setState] = useState<TenantListState>({ kind: "loading" });

  useEffect(() => {
    const controller = new AbortController();
    setState({ kind: "loading" });
    loadTenantList(controller.signal)
      .then((tenants) => {
        if (controller.signal.aborted) return;
        setState({ kind: "loaded", tenants });
      })
      .catch((err) => {
        if (controller.signal.aborted) return;
        setState({
          kind: "error",
          message: err instanceof Error ? err.message : String(err),
        });
      });
    return () => controller.abort();
  }, []);

  return state;
}
