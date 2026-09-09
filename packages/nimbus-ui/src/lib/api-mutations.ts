import type { DeployHistory, RollbackResponse } from "./types/deploy";
import type {
  SandboxCollection,
  SandboxCreateRequest,
  SandboxResource,
  SessionOpenRequest,
  SessionResource,
} from "./types/sandbox";
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
export function apiErrorMessage(body: unknown, status: number): string {
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
        ...(init.body !== undefined
          ? { "content-type": "application/json" }
          : {}),
        ...init.headers,
      },
    });
  } catch (err) {
    return {
      ok: false,
      error: err instanceof Error ? err.message : String(err),
    };
  }

  const body = (await response.json().catch(() => null)) as unknown;

  if (!response.ok) {
    return {
      ok: false,
      status: response.status,
      error: apiErrorMessage(body, response.status),
    };
  }
  return { ok: true, data: body as T };
}

const enc = encodeURIComponent;

/** Comparison operators the engine accepts (`nimbus_core::query::FilterOp`). */
export const FILTER_OPS = ["eq", "neq", "gt", "gte", "lt", "lte"] as const;
export type FilterOp = (typeof FILTER_OPS)[number];

/** One predicate (`nimbus_core::query::Filter`). */
export type DocumentFilter = {
  field: string;
  op: FilterOp;
  value: unknown;
};

/** Sort order (`nimbus_core::query::OrderBy`). */
export type DocumentOrder = {
  field: string;
  direction: "asc" | "desc";
};

// The filter/order/limit half of a paginated query. These shapes mirror
// `crates/nimbus-core/src/query.rs` field for field: typed rather than
// `unknown` so the query bar cannot construct a predicate the engine will
// reject, which reaches the operator only as an opaque page error.
export type PaginatedQuery = {
  table: string;
  filters: DocumentFilter[];
  order: DocumentOrder | null;
  limit: number | null;
};

// Tenant-scoped document writes plus the one typed read (the paginated query is
// a POST read, so it rides here beside the mutations rather than in a loader).
export const documents = {
  insert(
    tenant: string,
    table: string,
    fields: unknown,
  ): Promise<ApiResult<unknown>> {
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
  remove(
    tenant: string,
    table: string,
    id: string,
  ): Promise<ApiResult<unknown>> {
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

/** One document the schema apply route refused, and why. */
export type SchemaViolation = { id: string; message: string };

/**
 * The report `POST /api/tenants/{t}/schema/{table}/apply` returns. The route
 * answers 200 whether or not it applied, so the body is the outcome:
 * `applied` is false on a dry run and when any document violates the schema.
 * `violations` lists at most the first fifty; `violation_count` is the total.
 */
export type SchemaApplyReport = {
  applied: boolean;
  dry_run: boolean;
  scanned: number;
  violation_count: number;
  violations: SchemaViolation[];
};

// Tenant-scoped schema enforcement. `apply` scans the table before it stores
// the schema and refuses on any violation, so enforcement never lands on a
// table that already breaks it; `drop` removes enforcement while keeping the
// table's documents.
export const schema = {
  apply(
    tenant: string,
    table: string,
    value: unknown,
    options: { dryRun?: boolean } = {},
  ): Promise<ApiResult<SchemaApplyReport>> {
    const suffix = options.dryRun ? "?dry_run=true" : "";
    return apiFetch(
      `/api/tenants/${enc(tenant)}/schema/${enc(table)}/apply${suffix}`,
      { method: "POST", body: JSON.stringify(value) },
    );
  },
  get(tenant: string, table: string): Promise<ApiResult<unknown>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/schema/${enc(table)}`);
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

// Service lifecycle. Start and stop take no body; restart names the source
// generation the caller saw and a request id so the server can refuse a
// restart of a definition that changed under the operator. The routes answer
// with the service resource (start, stop) or a 202 restart receipt.
export type ServiceRestartRequest = {
  sourceGeneration: number;
  requestId: string;
};

export type ServiceLifecycleResponse = {
  tenant_id?: string;
  name?: string;
  state?: string;
  lifecycle_state?: string;
  readiness?: string;
  health?: string;
  endpoints?: unknown[];
};

export const services = {
  start(
    tenant: string,
    name: string,
  ): Promise<ApiResult<ServiceLifecycleResponse>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/services/${enc(name)}/start`, {
      method: "POST",
    });
  },
  stop(
    tenant: string,
    name: string,
  ): Promise<ApiResult<ServiceLifecycleResponse>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/services/${enc(name)}/stop`, {
      method: "POST",
    });
  },
  restart(
    tenant: string,
    name: string,
    request: ServiceRestartRequest,
  ): Promise<ApiResult<{ request_id?: string; disposition?: string }>> {
    return apiFetch(
      `/api/tenants/${enc(tenant)}/services/${enc(name)}/restart`,
      { method: "POST", body: JSON.stringify(request) },
    );
  },
};

/** One engine mutation as the scheduler stores it (`nimbus_core::Mutation`). */
export type ScheduleMutation =
  | { type: "insert"; table: string; id?: string; fields: unknown }
  | { type: "update"; table: string; id: string; patch: unknown }
  | { type: "delete"; table: string; id: string };

export type CronEntry = {
  name: string;
  schedule: { type: "interval"; seconds: number };
  mutation: ScheduleMutation;
  enabled: boolean;
  last_run?: number | null;
  next_run?: number;
  created_at?: number;
};

// Scheduler control. `runNow` enqueues a mutation for immediate execution,
// which is how Run now re-plays a job or a cron entry: the scheduler holds
// the mutation, the console only asks for another run of it.
export const schedules = {
  runNow(
    tenant: string,
    mutation: ScheduleMutation,
  ): Promise<ApiResult<{ job_id?: string }>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/schedule`, {
      method: "POST",
      body: JSON.stringify({ run_after_ms: 0, mutation }),
    });
  },
  cancel(tenant: string, jobId: string): Promise<ApiResult<unknown>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/schedule/${enc(jobId)}`, {
      method: "DELETE",
    });
  },
  listCrons(tenant: string): Promise<ApiResult<{ crons?: CronEntry[] }>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/crons`, { method: "GET" });
  },
  removeCron(tenant: string, name: string): Promise<ApiResult<unknown>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/crons/${enc(name)}`, {
      method: "DELETE",
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

// Deploy history and rollback on the local-admin routes. A rollback
// re-activates a retained bundle; the server refuses the active bundle,
// an unknown hash, and a bundle whose retained files fail the integrity
// check, each as a readable error.
export const deploys = {
  list(): Promise<ApiResult<DeployHistory>> {
    return apiFetch(`/api/admin/deploys`);
  },
  rollback(sha256: string): Promise<ApiResult<RollbackResponse>> {
    return apiFetch(`/api/admin/deploys/${enc(sha256)}/rollback`, {
      method: "POST",
    });
  },
};

// One bucket as the native object route reports it: the aggregate of the
// tenant's manifests under that name. A bucket exists only while it holds
// at least one object.
export type ObjectBucket = {
  bucket: string;
  objectCount: number;
  totalBytes: number;
};

// One object's manifest facts (`ObjectSummaryResponse` in
// crates/nimbus-server/src/http/objects.rs). `etag` is the MD5 hex of the
// bytes; `contentType` is what the writer declared, or null.
export type ObjectSummary = {
  bucket: string;
  key: string;
  size: number;
  contentType: string | null;
  etag: string;
  lastModifiedMillis: number;
};

export type ObjectListing = {
  bucket: string;
  prefix: string;
  objects: ObjectSummary[];
  // The bucket holds more keys under this prefix than the page carried.
  truncated: boolean;
};

export type UploadProgress = { loaded: number; total: number };

// A key keeps its slashes in the address (the route captures the rest of
// the path) while every other reserved byte is escaped per segment.
function objectPath(tenant: string, bucket: string, key: string): string {
  const segments = key.split("/").map(enc).join("/");
  return `/api/tenants/${enc(tenant)}/objects/${enc(bucket)}/${segments}`;
}

// Whole-object storage over the native, session-authenticated route family.
// The S3 listener is a separate front door with its own credentials, which
// the console never holds; these routes ride the session cookie like every
// other console write. Uploads go through XMLHttpRequest because `fetch`
// reports no upload progress. Downloads are plain navigations to `url()`.
export const objects = {
  buckets(tenant: string): Promise<ApiResult<{ buckets: ObjectBucket[] }>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/objects`, { method: "GET" });
  },
  list(
    tenant: string,
    bucket: string,
    prefix: string,
    limit?: number,
  ): Promise<ApiResult<ObjectListing>> {
    const params = new URLSearchParams();
    if (prefix) params.set("prefix", prefix);
    if (limit !== undefined) params.set("limit", String(limit));
    const query = params.toString();
    return apiFetch(
      `/api/tenants/${enc(tenant)}/objects/${enc(bucket)}${query ? `?${query}` : ""}`,
      { method: "GET" },
    );
  },
  url(
    tenant: string,
    bucket: string,
    key: string,
    options: { download?: boolean } = {},
  ): string {
    return `${objectPath(tenant, bucket, key)}${options.download ? "?download=1" : ""}`;
  },
  // The bytes of one object as text, for the preview. The caller bounds the
  // size before asking; the route serves the whole object.
  async readText(
    tenant: string,
    bucket: string,
    key: string,
  ): Promise<ApiResult<string>> {
    let response: Response;
    try {
      response = await fetch(objectPath(tenant, bucket, key), {
        credentials: "include",
      });
    } catch (err) {
      return {
        ok: false,
        error: err instanceof Error ? err.message : String(err),
      };
    }
    if (!response.ok) {
      return {
        ok: false,
        status: response.status,
        error: `Request failed: ${response.status}`,
      };
    }
    return { ok: true, data: await response.text() };
  },
  upload(
    tenant: string,
    bucket: string,
    key: string,
    body: Blob,
    options: {
      contentType?: string;
      onProgress?: (progress: UploadProgress) => void;
      signal?: AbortSignal;
    } = {},
  ): Promise<ApiResult<ObjectSummary>> {
    return new Promise((resolve) => {
      const xhr = new XMLHttpRequest();
      xhr.open("PUT", objectPath(tenant, bucket, key));
      xhr.withCredentials = true;
      if (options.contentType) {
        xhr.setRequestHeader("content-type", options.contentType);
      }
      xhr.upload.onprogress = (event) => {
        if (!options.onProgress) return;
        options.onProgress({
          loaded: event.loaded,
          total: event.lengthComputable ? event.total : body.size,
        });
      };
      xhr.onerror = () => resolve({ ok: false, error: "Network error" });
      xhr.onabort = () => resolve({ ok: false, error: "Upload cancelled" });
      xhr.onload = () => {
        let parsed: unknown = null;
        try {
          parsed = xhr.responseText ? JSON.parse(xhr.responseText) : null;
        } catch {
          parsed = null;
        }
        if (xhr.status >= 200 && xhr.status < 300) {
          resolve({ ok: true, data: parsed as ObjectSummary });
        } else {
          resolve({
            ok: false,
            status: xhr.status,
            error: apiErrorMessage(parsed, xhr.status),
          });
        }
      };
      options.signal?.addEventListener("abort", () => xhr.abort(), {
        once: true,
      });
      xhr.send(body);
    });
  },
  remove(
    tenant: string,
    bucket: string,
    key: string,
  ): Promise<ApiResult<unknown>> {
    return apiFetch(objectPath(tenant, bucket, key), { method: "DELETE" });
  },
};

// Sandboxes are live runtime resources on the service-control routes
// (`/api/tenants/{tenant}/sandboxes`). A server that runs no service
// manager answers 404 for every one of them; the pages read that status
// as "not available here", not as an empty list.
export const sandboxes = {
  list(tenant: string): Promise<ApiResult<SandboxCollection>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/sandboxes?limit=200`);
  },
  get(tenant: string, id: string): Promise<ApiResult<SandboxResource>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/sandboxes/${enc(id)}`);
  },
  create(
    tenant: string,
    request: SandboxCreateRequest,
  ): Promise<ApiResult<SandboxResource>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/sandboxes`, {
      method: "POST",
      body: JSON.stringify(request),
    });
  },
  stop(tenant: string, id: string): Promise<ApiResult<SandboxResource>> {
    return apiFetch(`/api/tenants/${enc(tenant)}/sandboxes/${enc(id)}/stop`, {
      method: "POST",
    });
  },
};

// Sessions attach channels to a sandbox or service. The console opens one
// per console panel with the `stdio` channel, streams it through
// `lib/session-channel.ts`, writes input to it here, and closes it when
// the panel goes away. Operator principals name the tenant in the query.
export const sessions = {
  open(request: SessionOpenRequest): Promise<ApiResult<SessionResource>> {
    return apiFetch("/api/sessions", {
      method: "POST",
      body: JSON.stringify(request),
    });
  },
  close(
    id: string,
    tenant: string,
    reason?: string,
  ): Promise<ApiResult<SessionResource>> {
    return apiFetch(`/api/sessions/${enc(id)}/close?tenantId=${enc(tenant)}`, {
      method: "POST",
      body: JSON.stringify(reason ? { reason } : {}),
    });
  },
  writeChannel(
    id: string,
    channel: string,
    tenant: string,
    data: string,
  ): Promise<ApiResult<null>> {
    return apiFetch(
      `/api/sessions/${enc(id)}/channels/${enc(channel)}/input?tenantId=${enc(tenant)}`,
      { method: "POST", body: JSON.stringify({ data }) },
    );
  },
};
