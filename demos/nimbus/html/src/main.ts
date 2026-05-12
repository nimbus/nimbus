import {
  NimbusRestClient,
  NimbusSubscriptionClient,
  type SubscribeQuery,
} from "nimbus/rest";
import "./app.css";

const $ = <T extends HTMLElement>(id: string) =>
  document.querySelector<T>(`#${id}`)!;

const elements = {
  activityLog: $<HTMLPreElement>("activity-log"),
  baseUrl: $<HTMLInputElement>("base-url"),
  connectSocket: $<HTMLButtonElement>("connect-socket"),
  createTenant: $<HTMLButtonElement>("create-tenant"),
  disconnectSocket: $<HTMLButtonElement>("disconnect-socket"),
  documents: $<HTMLDivElement>("documents"),
  healthButton: $<HTMLButtonElement>("health-button"),
  insertDocument: $<HTMLButtonElement>("insert-document"),
  insertJson: $<HTMLTextAreaElement>("insert-json"),
  installSchema: $<HTMLButtonElement>("install-schema"),
  jobId: $<HTMLElement>("job-id"),
  queryJson: $<HTMLTextAreaElement>("query-json"),
  refreshJobResult: $<HTMLButtonElement>("refresh-job-result"),
  scheduleDelay: $<HTMLInputElement>("schedule-delay"),
  scheduleInsert: $<HTMLButtonElement>("schedule-insert"),
  scheduleJson: $<HTMLTextAreaElement>("schedule-json"),
  serverOrigin: $<HTMLElement>("server-origin"),
  socketStatus: $<HTMLElement>("socket-status"),
  subscribeQuery: $<HTMLButtonElement>("subscribe-query"),
  subscriptionId: $<HTMLElement>("subscription-id"),
  tableName: $<HTMLInputElement>("table-name"),
  tenantId: $<HTMLInputElement>("tenant-id"),
};

const state: {
  http: NimbusRestClient | null;
  subscriptionClient: NimbusSubscriptionClient | null;
  currentSubscription: { subscriptionId: string; unsubscribe: () => void } | null;
  lastJobId: string | null;
} = {
  http: null,
  subscriptionClient: null,
  currentSubscription: null,
  lastJobId: null,
};

function log(message: string, data?: unknown) {
  const timestamp = new Date().toLocaleTimeString();
  const details = data === undefined ? "" : ` ${JSON.stringify(data, null, 2)}`;
  elements.activityLog.textContent = `[${timestamp}] ${message}${details}\n${elements.activityLog.textContent}`;
}

function parseJson(label: string, raw: string): Record<string, unknown> {
  try {
    return JSON.parse(raw) as Record<string, unknown>;
  } catch (error) {
    throw new Error(`${label} must be valid JSON: ${(error as Error).message}`);
  }
}

function currentBaseUrl(): string {
  return elements.baseUrl.value.trim() || window.location.origin;
}

function currentTenantId(): string {
  return elements.tenantId.value.trim();
}

function currentTableName(): string {
  return elements.tableName.value.trim();
}

function refreshClients() {
  const baseUrl = currentBaseUrl();
  state.http = new NimbusRestClient(baseUrl);
  state.subscriptionClient = new NimbusSubscriptionClient(baseUrl, currentTenantId(), {
    onLog: log,
  });
  elements.serverOrigin.textContent = baseUrl;
}

function defaultDemoSchema(tableName: string) {
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

function defaultDemoQuery(tableName: string): SubscribeQuery {
  return {
    table: tableName,
    filters: [],
    order: { field: "priority", direction: "asc" },
    limit: 25,
  };
}

function resetDefaults() {
  const table = currentTableName();
  elements.queryJson.value = JSON.stringify(defaultDemoQuery(table), null, 2);
  elements.insertJson.value = JSON.stringify(
    { title: "Ship convex-style Nimbus demo", status: "open", priority: 1 },
    null,
    2,
  );
  elements.scheduleJson.value = JSON.stringify(
    { title: "Scheduled follow-up task", status: "queued", priority: 2 },
    null,
    2,
  );
}

function setSocketStatus(status: string, tone: "normal" | "ok" | "warn" = "normal") {
  elements.socketStatus.textContent = status;
  elements.socketStatus.style.color =
    tone === "ok" ? "var(--ok)" : tone === "warn" ? "var(--warn)" : "inherit";
}

function escapeHtml(value: unknown): string {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function renderDocuments(documents: Record<string, unknown>[]) {
  if (!documents.length) {
    elements.documents.innerHTML =
      '<div class="empty-state">No documents yet. Insert or schedule one to watch the subscription update.</div>';
    return;
  }

  elements.documents.innerHTML = documents
    .map((doc) => {
      const title =
        typeof doc.title === "string" ? doc.title : (doc._id as string) ?? "document";
      return `
        <article class="document-card">
          <h3>${escapeHtml(title)}</h3>
          <pre>${escapeHtml(JSON.stringify(doc, null, 2))}</pre>
        </article>
      `;
    })
    .join("");
}

async function withUiTask<T>(label: string, work: () => Promise<T>): Promise<T> {
  try {
    const result = await work();
    log(`${label} succeeded`, result);
    return result;
  } catch (error) {
    log(`${label} failed`, { message: (error as Error).message });
    throw error;
  }
}

async function ensureConnected() {
  refreshClients();
  await state.subscriptionClient!.connect();
  setSocketStatus("connected", "ok");
}

elements.healthButton.addEventListener("click", async () => {
  refreshClients();
  await withUiTask("health check", () => state.http!.health());
});

elements.createTenant.addEventListener("click", async () => {
  refreshClients();
  await withUiTask("create tenant", () => state.http!.createTenant(currentTenantId()));
});

elements.installSchema.addEventListener("click", async () => {
  refreshClients();
  const table = currentTableName();
  const schema = defaultDemoSchema(table);
  await withUiTask("install schema", () =>
    state.http!.setTableSchema(currentTenantId(), table, schema),
  );
});

elements.connectSocket.addEventListener("click", async () => {
  await withUiTask("connect websocket", ensureConnected);
});

elements.disconnectSocket.addEventListener("click", async () => {
  if (state.currentSubscription) {
    state.currentSubscription.unsubscribe();
    state.currentSubscription = null;
    elements.subscriptionId.textContent = "none";
  }
  state.subscriptionClient?.close();
  setSocketStatus("disconnected");
  log("websocket disconnected by user");
});

elements.subscribeQuery.addEventListener("click", async () => {
  await ensureConnected();
  const query = parseJson("Query JSON", elements.queryJson.value) as unknown as SubscribeQuery;

  if (state.currentSubscription) {
    state.currentSubscription.unsubscribe();
    state.currentSubscription = null;
    elements.subscriptionId.textContent = "none";
  }

  const subscription = await withUiTask("subscribe query", () =>
    state.subscriptionClient!.subscribe(query, {
      onResult: (documents) => {
        renderDocuments(documents as Record<string, unknown>[]);
        log("subscription result received", {
          count: documents.length,
          ids: (documents as Record<string, unknown>[]).map((d) => d._id),
        });
      },
      onError: (error) => {
        log("subscription error", { message: error.message });
      },
    }),
  );

  state.currentSubscription = subscription;
  elements.subscriptionId.textContent = subscription.subscriptionId;
});

elements.insertDocument.addEventListener("click", async () => {
  refreshClients();
  const fields = parseJson("Insert fields JSON", elements.insertJson.value);
  await withUiTask("insert document", () =>
    state.http!.insertDocument(currentTenantId(), currentTableName(), fields),
  );
});

elements.scheduleInsert.addEventListener("click", async () => {
  refreshClients();
  const fields = parseJson("Scheduled fields JSON", elements.scheduleJson.value);
  const runAfterMs = Number(elements.scheduleDelay.value);
  const response = await withUiTask("schedule insert", () =>
    state.http!.scheduleMutation(currentTenantId(), {
      run_after_ms: runAfterMs,
      mutation: { type: "insert", table: currentTableName(), fields },
    }),
  );
  state.lastJobId = response.job_id;
  elements.jobId.textContent = response.job_id;
});

elements.refreshJobResult.addEventListener("click", async () => {
  refreshClients();
  if (!state.lastJobId) {
    log("refresh job result skipped", { message: "no scheduled job yet" });
    return;
  }
  await withUiTask("load scheduled job result", () =>
    state.http!.getScheduledJobResult(currentTenantId(), state.lastJobId!),
  );
});

elements.tableName.addEventListener("change", resetDefaults);

elements.baseUrl.value = window.location.origin;
elements.serverOrigin.textContent = window.location.origin;
setSocketStatus("disconnected");
resetDefaults();
renderDocuments([]);
log("demo ready");
