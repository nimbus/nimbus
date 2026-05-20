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
  const body = (await response.json()) as TenantListResponse;
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
