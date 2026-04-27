function stripTrailingSlash(url) {
  return url.endsWith("/") ? url.slice(0, -1) : url;
}

function websocketUrlFromBase(baseUrl) {
    const url = new URL(baseUrl);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    url.pathname = "/ws";
    url.search = "";
  url.hash = "";
  return url.toString();
}

export class NeovexHttpClient {
  constructor(baseUrl = window.location.origin) {
    this.baseUrl = stripTrailingSlash(baseUrl);
  }

  async request(path, options = {}) {
    const response = await fetch(`${this.baseUrl}${path}`, {
      headers: {
        "Content-Type": "application/json",
        ...(options.headers ?? {}),
      },
      ...options,
    });

    if (response.status === 204) {
      return null;
    }

    const contentType = response.headers.get("content-type") ?? "";
    const body = contentType.includes("application/json")
      ? await response.json()
      : await response.text();

    if (!response.ok) {
      const message =
        typeof body === "string"
          ? body
          : body?.message ?? JSON.stringify(body, null, 2);
      throw new Error(message || `request failed with ${response.status}`);
    }

    return body;
  }

  health() {
    return this.request("/health", { method: "GET" });
  }

  createTenant(id) {
    return this.request("/api/tenants", {
      method: "POST",
      body: JSON.stringify({ id }),
    });
  }

  listTenants() {
    return this.request("/api/tenants", { method: "GET" });
  }

  setTableSchema(tenantId, table, schema) {
    return this.request(`/api/tenants/${tenantId}/schema/${table}`, {
      method: "PUT",
      body: JSON.stringify(schema),
    });
  }

  insertDocument(tenantId, table, fields) {
    return this.request(`/api/tenants/${tenantId}/documents`, {
      method: "POST",
      body: JSON.stringify({ table, fields }),
    });
  }

  scheduleMutation(tenantId, request) {
    return this.request(`/api/tenants/${tenantId}/schedule`, {
      method: "POST",
      body: JSON.stringify(request),
    });
  }

  listScheduledJobs(tenantId) {
    return this.request(`/api/tenants/${tenantId}/schedule`, { method: "GET" });
  }

  getScheduledJobResult(tenantId, jobId) {
    return this.request(`/api/tenants/${tenantId}/schedule/history/${jobId}`, {
      method: "GET",
    });
  }

  createCronJob(tenantId, request) {
    return this.request(`/api/tenants/${tenantId}/crons`, {
      method: "POST",
      body: JSON.stringify(request),
    });
  }

  listCronJobs(tenantId) {
    return this.request(`/api/tenants/${tenantId}/crons`, { method: "GET" });
  }

  deleteCronJob(tenantId, name) {
    return this.request(`/api/tenants/${tenantId}/crons/${encodeURIComponent(name)}`, {
      method: "DELETE",
    });
  }
}

export class NeovexSubscriptionClient {
  constructor(baseUrl, tenantId, { onLog } = {}) {
    this.baseUrl = stripTrailingSlash(baseUrl);
    this.tenantId = tenantId;
    this.onLog = onLog ?? (() => {});
    this.pending = new Map();
    this.subscriptions = new Map();
    this.requestCounter = 0;
    this.socket = null;
    }

    async connect() {
    if (this.socket && this.socket.readyState === WebSocket.OPEN) {
      return;
    }

    const wsUrl = websocketUrlFromBase(this.baseUrl);
    wsUrl.searchParams.set("tenant_id", this.tenantId);
    const socket = new WebSocket(wsUrl, ["neovex.v2"]);
    this.socket = socket;

    await new Promise((resolve, reject) => {
      const open = () => {
        cleanup();
        socket.send(JSON.stringify({
          type: "client_hello",
          protocol: "neovex.v2",
          client: {
            kind: "demo",
            version: "unknown",
          },
          capabilities: ["queries.v1", "subscriptions.v1"],
        }));
        this.onLog(`websocket connected to ${wsUrl}`);
        resolve();
      };
      const error = () => {
        cleanup();
        reject(new Error("websocket connection failed"));
      };
      const cleanup = () => {
        socket.removeEventListener("open", open);
        socket.removeEventListener("error", error);
      };

      socket.addEventListener("open", open);
      socket.addEventListener("error", error);
    });

    socket.addEventListener("message", (event) => {
      this.handleMessage(event.data);
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

  async subscribe(query, { onResult, onError } = {}) {
    this.ensureConnected();
    const requestId = `sub-${++this.requestCounter}`;

    const subscription = await new Promise((resolve, reject) => {
      this.pending.set(requestId, { resolve, reject, onResult, onError });
      this.socket.send(
        JSON.stringify({
          type: "subscribe",
          request_id: requestId,
          query,
        }),
      );
    });

    return subscription;
  }

  unsubscribe(subscriptionId) {
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

  close() {
    if (this.socket) {
      this.socket.close();
    }
  }

  ensureConnected() {
    if (!this.socket || this.socket.readyState !== WebSocket.OPEN) {
      throw new Error("websocket is not connected");
    }
  }

  handleMessage(raw) {
    const message = JSON.parse(raw);
    this.onLog(`ws <= ${JSON.stringify(message)}`);

    if (message.type === "hello") {
      return;
    }

    if (message.type === "fatal_error") {
      this.onLog(`ws fatal error: ${message.error?.message ?? "protocol failure"}`);
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

  handleSubscriptionResult(message) {
    if (message.request_id && this.pending.has(message.request_id)) {
      const pending = this.pending.get(message.request_id);
      this.pending.delete(message.request_id);
      const subscription = {
        subscriptionId: message.subscription_id,
        unsubscribe: () => this.unsubscribe(message.subscription_id),
      };
      this.subscriptions.set(message.subscription_id, {
        onResult: pending.onResult,
        onError: pending.onError,
      });
      pending.onResult?.(message.data, message);
      pending.resolve(subscription);
      return;
    }

    const active = this.subscriptions.get(message.subscription_id);
    active?.onResult?.(message.data, message);
  }

  handleError(message) {
    const requestId = typeof message.id === "string" ? message.id : message.request_id;
    const errorMessage = message.error?.message ?? "websocket request failed";
    if (requestId && this.pending.has(requestId)) {
      const pending = this.pending.get(requestId);
      this.pending.delete(requestId);
      const error = new Error(errorMessage);
      pending.onError?.(error, message);
      pending.reject(error);
      return;
    }

    this.onLog(`ws error: ${errorMessage}`);
  }
}

export function defaultDemoSchema(tableName) {
  return {
    table: tableName,
    fields: [
      { name: "title", field_type: "string", required: true },
      { name: "status", field_type: "string", required: true },
      { name: "priority", field_type: "number", required: false },
    ],
    indexes: [
      { name: "by_status", field: "status" },
      { name: "by_priority", field: "priority" },
    ],
  };
}

export function defaultDemoQuery(tableName) {
  return {
    table: tableName,
    filters: [],
    order: { field: "priority", direction: "asc" },
    limit: 25,
  };
}
