import type { PageResponse } from "./types/table";

// The console's one typed HTTP client for every hand-driven write (and the
// single paginated read that backs the storage table). Every call resolves to
// an `ApiResult<T>` — expected failures (validation, permission, a non-JSON or
// empty error body, a dropped connection) come back as `ok:false` with a
// readable message rather than throwing, so call sites branch on `ok` instead
// of wrapping each fetch in its own try/catch + `!response.ok` boilerplate.
export type ApiResult<T> =
  | { ok: true; data: T }
  // `status` is the HTTP status when the failure was a response (absent for a
  // network/parse fault). One-shot reads use it to promote a specific status
  // to a typed value — e.g. the Source tab treating 404 as "missing".
  | { ok: false; error: string; status?: number };

// Extract a human-readable message from a parsed error body. Handles both
// envelope shapes the server uses today — `{ error: { message } }` (storage,
// schema, tenant routes) and `{ error: "…" }` (system, machine routes) — and
// falls back to the status line for empty or unrecognized bodies.
function errorMessage(body: unknown, status: number): string {
  if (body && typeof body === "object" && "error" in body) {
    const error = (body as { error: unknown }).error;
    if (typeof error === "string" && error.length > 0) return error;
    if (error && typeof error === "object" && "message" in error) {
      const message = (error as { message: unknown }).message;
      if (typeof message === "string" && message.length > 0) return message;
    }
  }
  return `Request failed: ${status}`;
}

// The private core: a root-relative path, cookie credentials, and a JSON
// content-type by default (helpers still add `Authorization`/`accept`). The
// body is parsed once; a 204/empty success resolves to `data: null`.
export async function apiFetch<T>(
  path: string,
  init: RequestInit = {},
): Promise<ApiResult<T>> {
  let response: Response;
  try {
    response = await fetch(path, {
      credentials: "include",
      ...init,
      // Only advertise a JSON body when one is actually sent — no-body
      // writes (delete/drop/shutdown/rotate) sent no Content-Type before.
      headers: {
        ...(init.body !== undefined ? { "content-type": "application/json" } : {}),
        ...init.headers,
      },
    });
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }

  const body = (await response.json().catch(() => null)) as unknown;

  if (!response.ok) {
    return {
      ok: false,
      status: response.status,
      error: errorMessage(body, response.status),
    };
  }
  return { ok: true, data: body as T };
}

const enc = encodeURIComponent;

// The filter/order/limit half of a paginated query — kept loose because the
// storage table only ever sends the empty/unfiltered form today.
export type PaginatedQuery = {
  table: string;
  filters: unknown[];
  order: unknown;
  limit: number | null;
};

// Tenant-scoped document writes plus the one typed read (the paginated query is
// a POST read, so it rides here beside the mutations rather than in a loader).
export const documents = {
  insert(tenant: string, table: string, fields: unknown): Promise<ApiResult<unknown>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/documents`, {
      method: "POST",
      body: JSON.stringify({ table, fields }),
    });
  },
  update(
    tenant: string,
    table: string,
    id: string,
    patch: unknown,
  ): Promise<ApiResult<unknown>> {
    return apiFetch(
      `/api/tenants/${enc(tenant)}/documents/${enc(table)}/${enc(id)}`,
      { method: "PATCH", body: JSON.stringify({ patch }) },
    );
  },
  remove(tenant: string, table: string, id: string): Promise<ApiResult<unknown>> {
    return apiFetch(
      `/api/tenants/${enc(tenant)}/documents/${enc(table)}/${enc(id)}`,
      { method: "DELETE" },
    );
  },
  queryPaginated(
    tenant: string,
    query: PaginatedQuery,
    pageSize: number,
    after: string | null,
  ): Promise<ApiResult<PageResponse>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/query/paginated`, {
      method: "POST",
      body: JSON.stringify({ query, page_size: pageSize, after }),
    });
  },
};

// Tenant-scoped schema enforcement. `put` sends the raw schema object; `drop`
// removes enforcement while keeping the table's documents.
export const schema = {
  put(tenant: string, table: string, value: unknown): Promise<ApiResult<unknown>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/schema/${enc(table)}`, {
      method: "PUT",
      body: JSON.stringify(value),
    });
  },
  drop(tenant: string, table: string): Promise<ApiResult<unknown>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/schema/${enc(table)}`, {
      method: "DELETE",
    });
  },
};

// Tenant lifecycle from the operator console.
export const tenants = {
  create(id: string): Promise<ApiResult<{ id?: string }>> {
    return apiFetch(`/api/tenants`, {
      method: "POST",
      body: JSON.stringify({ id }),
    });
  },
  remove(id: string): Promise<ApiResult<unknown>> {
    return apiFetch(`/api/tenants/${enc(id)}`, { method: "DELETE" });
  },
};

// Machine lifecycle. The hand-rolled fetches used `credentials:"same-origin"`
// and the `accept: application/json` hint the endpoint keys off — both
// preserved exactly (same-origin is the stricter cookie mode; keep it).
export const machines = {
  action(name: string, action: string): Promise<ApiResult<unknown>> {
    return apiFetch(`/api/machines/${enc(name)}/${action}`, {
      method: "POST",
      credentials: "same-origin",
      headers: { accept: "application/json" },
      body: JSON.stringify({}),
    });
  },
  remove(name: string): Promise<ApiResult<unknown>> {
    return apiFetch(`/api/machines/${enc(name)}`, {
      method: "DELETE",
      credentials: "same-origin",
      headers: { accept: "application/json" },
    });
  },
};

// Session lifecycle. `rotateToken` authenticates with the current admin bearer
// (the only Authorization-header write); `shutdown` rides the session cookie.
export const system = {
  rotateToken(token: string): Promise<ApiResult<{ generation?: number }>> {
    return apiFetch(`/api/system/token/rotate`, {
      method: "POST",
      headers: { Authorization: `Bearer ${token}` },
    });
  },
  shutdown(): Promise<ApiResult<{ accepted?: boolean }>> {
    return apiFetch(`/api/system/shutdown`, { method: "POST" });
  },
};
