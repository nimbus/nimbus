export interface RequestOptions extends Omit<RequestInit, "headers"> {
  headers?: Record<string, string>;
}

export interface TableSchema {
  table: string;
  fields: { name: string; field_type: string; required: boolean }[];
  indexes?: { name: string; field: string }[];
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
  schedule: string;
  mutation: {
    type: string;
    table: string;
    fields: Record<string, unknown>;
  };
}

export interface SubscribeQuery {
  table: string;
  filters?: unknown[];
  order?: { field: string; direction: "asc" | "desc" };
  limit?: number;
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

export class NeovexRestClient {
  readonly baseUrl: string;

  constructor(baseUrl: string) {
    this.baseUrl = stripTrailingSlash(baseUrl);
  }

  async request<T = unknown>(path: string, options: RequestOptions = {}): Promise<T> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      headers: {
        "Content-Type": "application/json",
        ...(options.headers ?? {}),
      },
      ...options,
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
    return this.request("/health", { method: "GET" });
  }

  createTenant(id: string): Promise<unknown> {
    return this.request("/api/tenants", {
      method: "POST",
      body: JSON.stringify({ id }),
    });
  }

  listTenants(): Promise<unknown> {
    return this.request("/api/tenants", { method: "GET" });
  }

  setTableSchema(tenantId: string, table: string, schema: TableSchema): Promise<unknown> {
    return this.request(`/api/tenants/${tenantId}/schema/${table}`, {
      method: "PUT",
      body: JSON.stringify(schema),
    });
  }

  insertDocument(
    tenantId: string,
    table: string,
    fields: Record<string, unknown>,
  ): Promise<unknown> {
    return this.request(`/api/tenants/${tenantId}/documents`, {
      method: "POST",
      body: JSON.stringify({ table, fields }),
    });
  }

  getDocument(tenantId: string, docId: string): Promise<unknown> {
    return this.request(`/api/tenants/${tenantId}/documents/${docId}`, { method: "GET" });
  }

  listDocuments(tenantId: string): Promise<unknown> {
    return this.request(`/api/tenants/${tenantId}/documents`, { method: "GET" });
  }

  updateDocument(
    tenantId: string,
    docId: string,
    fields: Record<string, unknown>,
  ): Promise<unknown> {
    return this.request(`/api/tenants/${tenantId}/documents/${docId}`, {
      method: "PATCH",
      body: JSON.stringify(fields),
    });
  }

  deleteDocument(tenantId: string, docId: string): Promise<unknown> {
    return this.request(`/api/tenants/${tenantId}/documents/${docId}`, { method: "DELETE" });
  }

  query(tenantId: string, query: SubscribeQuery): Promise<unknown> {
    return this.request(`/api/tenants/${tenantId}/query`, {
      method: "POST",
      body: JSON.stringify(query),
    });
  }

  scheduleMutation(
    tenantId: string,
    request: ScheduleMutationRequest,
  ): Promise<{ job_id: string }> {
    return this.request(`/api/tenants/${tenantId}/schedule`, {
      method: "POST",
      body: JSON.stringify(request),
    });
  }

  listScheduledJobs(tenantId: string): Promise<unknown> {
    return this.request(`/api/tenants/${tenantId}/schedule`, { method: "GET" });
  }

  getScheduledJobResult(tenantId: string, jobId: string): Promise<unknown> {
    return this.request(`/api/tenants/${tenantId}/schedule/history/${jobId}`, {
      method: "GET",
    });
  }

  createCronJob(tenantId: string, request: CronJobRequest): Promise<unknown> {
    return this.request(`/api/tenants/${tenantId}/crons`, {
      method: "POST",
      body: JSON.stringify(request),
    });
  }

  listCronJobs(tenantId: string): Promise<unknown> {
    return this.request(`/api/tenants/${tenantId}/crons`, { method: "GET" });
  }

  deleteCronJob(tenantId: string, name: string): Promise<unknown> {
    return this.request(`/api/tenants/${tenantId}/crons/${encodeURIComponent(name)}`, {
      method: "DELETE",
    });
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

export class NeovexSubscriptionClient {
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

    const socket = new WebSocket(wsUrlString, ["neovex.v2"]);
    this.socket = socket;

    await new Promise<void>((resolve, reject) => {
      const onOpen = () => {
        cleanup();
        socket.send(
          JSON.stringify({
            type: "client_hello",
            protocol: "neovex.v2",
            client: { kind: "neovex-rest", version: "0.1.0" },
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
