import { useCallback, useEffect, useRef, useState } from "react";

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

// `reload` rides along with the state rather than arriving in a wrapper object
// so that narrowing on `.kind` keeps working at every existing call site. An
// error state that cannot retry is a dead end: the only recovery left is
// reloading the whole console, which throws away every other panel's data to
// re-run one failed request.
export type TenantListResult = TenantListState & { reload: () => void };

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

export function useTenantList(): TenantListResult {
  const [state, setState] = useState<TenantListState>({ kind: "loading" });
  // The in-flight request is held across renders so every start aborts the
  // previous one. Without it, a reader who clicks Retry twice can have the
  // first response land second and overwrite the newer answer.
  const active = useRef<AbortController | null>(null);

  // `reload` runs the request rather than bumping a nonce the effect watches:
  // a nonce the effect body never reads is an unused dependency, and the fix
  // the linter offers for that — dropping it from the dep array — silently
  // removes the retry. The identity is stable, so the effect below still runs
  // exactly once per mount.
  const reload = useCallback(() => {
    active.current?.abort();
    const controller = new AbortController();
    active.current = controller;
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
  }, []);

  useEffect(() => {
    reload();
    return () => active.current?.abort();
  }, [reload]);

  return { ...state, reload };
}
