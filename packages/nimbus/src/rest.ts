export interface RequestOptions extends Omit<RequestInit, "headers"> {
  headers?: Record<string, string>;
}

export type FetchLike = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

export interface NimbusRestClientOptions {
  fetch?: FetchLike;
  headers?: Record<string, string>;
  token?: string;
}

export interface TableSchema {
  table: string;
  fields: {
    name: string;
    field_type: "string" | "number" | "boolean" | "array" | "object" | "any";
    required: boolean;
  }[];
  indexes: { name: string; fields: string[] }[];
}

export interface ScheduleMutationRequest {
  run_after_ms: number;
  mutation: {
    type: string;
    table: string;
    fields: Record<string, unknown>;
  };
}

export interface CronJobRequest {
  name: string;
  schedule: { type: "interval"; seconds: number };
  mutation: {
    type: string;
    table: string;
    fields: Record<string, unknown>;
  };
}

export interface SubscribeQuery {
  table: string;
  /** Required by the server — pass `[]` for no filters. */
  filters: unknown[];
  order?: { field: string; direction: "asc" | "desc" };
  limit?: number;
}

export interface PaginatedQueryRequest {
  query: SubscribeQuery;
  page_size: number;
  after?: string | null;
}

export interface Subscription {
  subscriptionId: string;
  unsubscribe: () => void;
}

function stripTrailingSlash(url: string): string {
  return url.endsWith("/") ? url.slice(0, -1) : url;
}

function websocketUrlFromBase(baseUrl: string): URL {
  const url = new URL(baseUrl);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.pathname = "/ws";
  url.search = "";
  url.hash = "";
  return url;
}

/// The native REST surface this client speaks, one entry per method.
/// Path templates use `{placeholder}` segments matching the server's
/// router; `native_rest_routes.json` carries the same table and the
/// selftest parity check (`rest_client_route_parity.mjs`) fails if the
/// two drift or if any method stops honoring its entry.
export const NIMBUS_REST_ROUTES = {
  health: { verb: "GET", path: "/health" },
  createTenant: { verb: "POST", path: "/api/tenants" },
  listTenants: { verb: "GET", path: "/api/tenants" },
  deleteTenant: { verb: "DELETE", path: "/api/tenants/{tenant_id}" },
  getSchema: { verb: "GET", path: "/api/tenants/{tenant_id}/schema" },
  getTableSchema: { verb: "GET", path: "/api/tenants/{tenant_id}/schema/{table}" },
  setTableSchema: { verb: "PUT", path: "/api/tenants/{tenant_id}/schema/{table}" },
  deleteTableSchema: { verb: "DELETE", path: "/api/tenants/{tenant_id}/schema/{table}" },
  insertDocument: { verb: "POST", path: "/api/tenants/{tenant_id}/documents" },
  listDocuments: { verb: "GET", path: "/api/tenants/{tenant_id}/documents/{table}" },
  getDocument: {
    verb: "GET",
    path: "/api/tenants/{tenant_id}/documents/{table}/{document_id}",
  },
  updateDocument: {
    verb: "PATCH",
    path: "/api/tenants/{tenant_id}/documents/{table}/{document_id}",
  },
  deleteDocument: {
    verb: "DELETE",
    path: "/api/tenants/{tenant_id}/documents/{table}/{document_id}",
  },
  query: { verb: "POST", path: "/api/tenants/{tenant_id}/query" },
  queryPaginated: { verb: "POST", path: "/api/tenants/{tenant_id}/query/paginated" },
  readJournal: { verb: "GET", path: "/api/tenants/{tenant_id}/journal" },
  bootstrapJournal: { verb: "GET", path: "/api/tenants/{tenant_id}/journal/bootstrap" },
  scheduleMutation: { verb: "POST", path: "/api/tenants/{tenant_id}/schedule" },
  listScheduledJobs: { verb: "GET", path: "/api/tenants/{tenant_id}/schedule" },
  cancelScheduledJob: {
    verb: "DELETE",
    path: "/api/tenants/{tenant_id}/schedule/{job_id}",
  },
  getScheduledJobResult: {
    verb: "GET",
    path: "/api/tenants/{tenant_id}/schedule/history/{job_id}",
  },
  createCronJob: { verb: "POST", path: "/api/tenants/{tenant_id}/crons" },
  listCronJobs: { verb: "GET", path: "/api/tenants/{tenant_id}/crons" },
  deleteCronJob: { verb: "DELETE", path: "/api/tenants/{tenant_id}/crons/{name}" },
} as const;

export type NimbusRestRouteName = keyof typeof NIMBUS_REST_ROUTES;

function expandPath(template: string, params: Record<string, string>): string {
  return template.replace(/\{([a-z_]+)\}/g, (_, name: string) => {
    const value = params[name];
    if (value === undefined) {
      throw new Error(`missing path parameter \`${name}\` for ${template}`);
    }
    return encodeURIComponent(value);
  });
}

export class NimbusRestClient {
  readonly baseUrl: string;
  private readonly fetchImpl: FetchLike;
  private readonly defaultHeaders: Record<string, string>;

  constructor(baseUrl: string, options: NimbusRestClientOptions = {}) {
    this.baseUrl = stripTrailingSlash(baseUrl);
    this.fetchImpl = options.fetch ?? fetch.bind(globalThis);
    this.defaultHeaders = {
      ...(options.token ? { Authorization: `Bearer ${options.token}` } : {}),
      ...(options.headers ?? {}),
    };
  }

  private route<T = unknown>(
    name: NimbusRestRouteName,
    params: Record<string, string>,
    body?: unknown,
  ): Promise<T> {
    const { verb, path } = NIMBUS_REST_ROUTES[name];
    return this.request<T>(expandPath(path, params), {
      method: verb,
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
  }

  async request<T = unknown>(path: string, options: RequestOptions = {}): Promise<T> {
    const { headers, ...requestOptions } = options;
    const response = await this.fetchImpl(`${this.baseUrl}${path}`, {
      ...requestOptions,
      headers: {
        "Content-Type": "application/json",
        ...this.defaultHeaders,
        ...(headers ?? {}),
      },
    });

    if (response.status === 204) {
      return null as T;
    }

    const contentType = response.headers.get("content-type") ?? "";
    const body = contentType.includes("application/json")
      ? await response.json()
      : await response.text();

    if (!response.ok) {
      const message =
        typeof body === "string"
          ? body
          : (body as { message?: string })?.message ?? JSON.stringify(body, null, 2);
      throw new Error(message || `request failed with ${response.status}`);
    }

    return body as T;
  }

  health(): Promise<unknown> {
    return this.route("health", {});
  }

  createTenant(id: string): Promise<unknown> {
    return this.route("createTenant", {}, { id });
  }

  listTenants(): Promise<unknown> {
    return this.route("listTenants", {});
  }

  deleteTenant(tenantId: string): Promise<unknown> {
    return this.route("deleteTenant", { tenant_id: tenantId });
  }

  getSchema(tenantId: string): Promise<unknown> {
    return this.route("getSchema", { tenant_id: tenantId });
  }

  getTableSchema(tenantId: string, table: string): Promise<unknown> {
    return this.route("getTableSchema", { tenant_id: tenantId, table });
  }

  setTableSchema(tenantId: string, table: string, schema: TableSchema): Promise<unknown> {
    return this.route("setTableSchema", { tenant_id: tenantId, table }, schema);
  }

  deleteTableSchema(tenantId: string, table: string): Promise<unknown> {
    return this.route("deleteTableSchema", { tenant_id: tenantId, table });
  }

  insertDocument(
    tenantId: string,
    table: string,
    fields: Record<string, unknown>,
  ): Promise<unknown> {
    return this.route("insertDocument", { tenant_id: tenantId }, { table, fields });
  }

  listDocuments(tenantId: string, table: string): Promise<unknown> {
    return this.route("listDocuments", { tenant_id: tenantId, table });
  }

  getDocument(tenantId: string, table: string, docId: string): Promise<unknown> {
    return this.route("getDocument", {
      tenant_id: tenantId,
      table,
      document_id: docId,
    });
  }

  updateDocument(
    tenantId: string,
    table: string,
    docId: string,
    patch: Record<string, unknown>,
  ): Promise<unknown> {
    return this.route(
      "updateDocument",
      { tenant_id: tenantId, table, document_id: docId },
      { patch },
    );
  }

  deleteDocument(tenantId: string, table: string, docId: string): Promise<unknown> {
    return this.route("deleteDocument", {
      tenant_id: tenantId,
      table,
      document_id: docId,
    });
  }

  query(tenantId: string, query: SubscribeQuery): Promise<unknown> {
    return this.route("query", { tenant_id: tenantId }, query);
  }

  queryPaginated(tenantId: string, query: PaginatedQueryRequest): Promise<unknown> {
    return this.route("queryPaginated", { tenant_id: tenantId }, query);
  }

  readJournal(tenantId: string): Promise<unknown> {
    return this.route("readJournal", { tenant_id: tenantId });
  }

  bootstrapJournal(tenantId: string): Promise<unknown> {
    return this.route("bootstrapJournal", { tenant_id: tenantId });
  }

  scheduleMutation(
    tenantId: string,
    request: ScheduleMutationRequest,
  ): Promise<{ job_id: string }> {
    return this.route("scheduleMutation", { tenant_id: tenantId }, request);
  }

  listScheduledJobs(tenantId: string): Promise<unknown> {
    return this.route("listScheduledJobs", { tenant_id: tenantId });
  }

  cancelScheduledJob(tenantId: string, jobId: string): Promise<unknown> {
    return this.route("cancelScheduledJob", { tenant_id: tenantId, job_id: jobId });
  }

  getScheduledJobResult(tenantId: string, jobId: string): Promise<unknown> {
    return this.route("getScheduledJobResult", { tenant_id: tenantId, job_id: jobId });
  }

  createCronJob(tenantId: string, request: CronJobRequest): Promise<unknown> {
    return this.route("createCronJob", { tenant_id: tenantId }, request);
  }

  listCronJobs(tenantId: string): Promise<unknown> {
    return this.route("listCronJobs", { tenant_id: tenantId });
  }

  deleteCronJob(tenantId: string, name: string): Promise<unknown> {
    return this.route("deleteCronJob", { tenant_id: tenantId, name });
  }
}

interface PendingRequest {
  resolve: (value: Subscription) => void;
  reject: (error: Error) => void;
  onResult?: (data: unknown[], message: unknown) => void;
  onError?: (error: Error, message: unknown) => void;
}

interface ActiveSubscription {
  onResult?: (data: unknown[], message: unknown) => void;
  onError?: (error: Error, message: unknown) => void;
}

export interface SubscriptionClientOptions {
  onLog?: (message: string) => void;
}

export class NimbusSubscriptionClient {
  readonly baseUrl: string;
  readonly tenantId: string;
  private readonly onLog: (message: string) => void;
  private pending = new Map<string, PendingRequest>();
  private subscriptions = new Map<string, ActiveSubscription>();
  private requestCounter = 0;
  private socket: WebSocket | null = null;

  constructor(baseUrl: string, tenantId: string, options: SubscriptionClientOptions = {}) {
    this.baseUrl = stripTrailingSlash(baseUrl);
    this.tenantId = tenantId;
    this.onLog = options.onLog ?? (() => {});
  }

  async connect(): Promise<void> {
    if (this.socket && this.socket.readyState === WebSocket.OPEN) {
      return;
    }

    const wsUrl = websocketUrlFromBase(this.baseUrl);
    wsUrl.searchParams.set("tenant_id", this.tenantId);
    const wsUrlString = wsUrl.toString();

    const socket = new WebSocket(wsUrlString, ["nimbus.v2"]);
    this.socket = socket;

    await new Promise<void>((resolve, reject) => {
      const onOpen = () => {
        cleanup();
        socket.send(
          JSON.stringify({
            type: "client_hello",
            protocol: "nimbus.v2",
            client: { kind: "nimbus-rest", version: "0.1.0" },
            capabilities: ["queries.v1", "subscriptions.v1"],
          }),
        );
        this.onLog(`websocket connected to ${wsUrlString}`);
        resolve();
      };
      const onError = () => {
        cleanup();
        reject(new Error("websocket connection failed"));
      };
      const cleanup = () => {
        socket.removeEventListener("open", onOpen);
        socket.removeEventListener("error", onError);
      };
      socket.addEventListener("open", onOpen);
      socket.addEventListener("error", onError);
    });

    socket.addEventListener("message", (event) => {
      this.handleMessage(event.data as string);
    });
    socket.addEventListener("close", () => {
      this.onLog("websocket disconnected");
      this.socket = null;
      for (const pending of this.pending.values()) {
        pending.reject(new Error("websocket disconnected"));
      }
      this.pending.clear();
      this.subscriptions.clear();
    });
  }

  async subscribe(
    query: SubscribeQuery,
    callbacks: { onResult?: (data: unknown[], message: unknown) => void; onError?: (error: Error, message: unknown) => void } = {},
  ): Promise<Subscription> {
    this.ensureConnected();
    const requestId = `sub-${++this.requestCounter}`;

    return new Promise<Subscription>((resolve, reject) => {
      this.pending.set(requestId, {
        resolve,
        reject,
        onResult: callbacks.onResult,
        onError: callbacks.onError,
      });
      this.socket!.send(
        JSON.stringify({
          type: "subscribe",
          request_id: requestId,
          query,
        }),
      );
    });
  }

  unsubscribe(subscriptionId: string): void {
    if (!this.socket || this.socket.readyState !== WebSocket.OPEN) {
      return;
    }
    this.subscriptions.delete(subscriptionId);
    this.socket.send(
      JSON.stringify({
        type: "unsubscribe",
        subscription_id: subscriptionId,
      }),
    );
    this.onLog(`unsubscribed ${subscriptionId}`);
  }

  close(): void {
    if (this.socket) {
      this.socket.close();
    }
  }

  private ensureConnected(): void {
    if (!this.socket || this.socket.readyState !== WebSocket.OPEN) {
      throw new Error("websocket is not connected");
    }
  }

  private handleMessage(raw: string): void {
    const message = JSON.parse(raw) as Record<string, unknown>;
    this.onLog(`ws <= ${JSON.stringify(message)}`);

    if (message.type === "hello") return;

    if (message.type === "fatal_error") {
      const error = message.error as { message?: string } | undefined;
      this.onLog(`ws fatal error: ${error?.message ?? "protocol failure"}`);
      return;
    }

    if (message.type === "subscription_result") {
      this.handleSubscriptionResult(message);
      return;
    }

    if (message.type === "error" || message.type === "op.error") {
      this.handleError(message);
    }
  }

  private handleSubscriptionResult(message: Record<string, unknown>): void {
    const requestId = message.request_id as string | undefined;
    if (requestId && this.pending.has(requestId)) {
      const pending = this.pending.get(requestId)!;
      this.pending.delete(requestId);
      const subscriptionId = message.subscription_id as string;
      const subscription: Subscription = {
        subscriptionId,
        unsubscribe: () => this.unsubscribe(subscriptionId),
      };
      this.subscriptions.set(subscriptionId, {
        onResult: pending.onResult,
        onError: pending.onError,
      });
      pending.onResult?.(message.data as unknown[], message);
      pending.resolve(subscription);
      return;
    }

    const subscriptionId = message.subscription_id as string;
    const active = this.subscriptions.get(subscriptionId);
    active?.onResult?.(message.data as unknown[], message);
  }

  private handleError(message: Record<string, unknown>): void {
    const requestId =
      typeof message.id === "string" ? message.id : (message.request_id as string | undefined);
    const error = message.error as { message?: string } | undefined;
    const errorMessage = error?.message ?? "websocket request failed";

    if (requestId && this.pending.has(requestId)) {
      const pending = this.pending.get(requestId)!;
      this.pending.delete(requestId);
      const err = new Error(errorMessage);
      pending.onError?.(err, message);
      pending.reject(err);
      return;
    }

    this.onLog(`ws error: ${errorMessage}`);
  }
}
