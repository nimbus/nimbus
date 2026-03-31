import {
  NeovexHttpClient,
  NeovexSubscriptionClient,
  defaultDemoQuery,
  defaultDemoSchema,
} from "../shared/neovex-client.js";

const elements = {
  activityLog: document.querySelector("#activity-log"),
  baseUrl: document.querySelector("#base-url"),
  connectSocket: document.querySelector("#connect-socket"),
  createTenant: document.querySelector("#create-tenant"),
  disconnectSocket: document.querySelector("#disconnect-socket"),
  documents: document.querySelector("#documents"),
  healthButton: document.querySelector("#health-button"),
  insertDocument: document.querySelector("#insert-document"),
  insertJson: document.querySelector("#insert-json"),
  installSchema: document.querySelector("#install-schema"),
  jobId: document.querySelector("#job-id"),
  queryJson: document.querySelector("#query-json"),
  refreshJobResult: document.querySelector("#refresh-job-result"),
  scheduleDelay: document.querySelector("#schedule-delay"),
  scheduleInsert: document.querySelector("#schedule-insert"),
  scheduleJson: document.querySelector("#schedule-json"),
  serverOrigin: document.querySelector("#server-origin"),
  socketStatus: document.querySelector("#socket-status"),
  subscribeQuery: document.querySelector("#subscribe-query"),
  subscriptionId: document.querySelector("#subscription-id"),
  tableName: document.querySelector("#table-name"),
  tenantId: document.querySelector("#tenant-id"),
};

const state = {
  http: null,
  subscriptionClient: null,
  currentSubscription: null,
  lastJobId: null,
};

function log(message, data) {
  const timestamp = new Date().toLocaleTimeString();
  const details = data === undefined ? "" : ` ${JSON.stringify(data, null, 2)}`;
  elements.activityLog.textContent = `[${timestamp}] ${message}${details}\n${elements.activityLog.textContent}`;
}

function parseJson(label, raw) {
  try {
    return JSON.parse(raw);
  } catch (error) {
    throw new Error(`${label} must be valid JSON: ${error.message}`);
  }
}

function currentBaseUrl() {
  return elements.baseUrl.value.trim() || window.location.origin;
}

function currentTenantId() {
  return elements.tenantId.value.trim();
}

function currentTableName() {
  return elements.tableName.value.trim();
}

function refreshClients() {
  const baseUrl = currentBaseUrl();
  state.http = new NeovexHttpClient(baseUrl);
  state.subscriptionClient = new NeovexSubscriptionClient(baseUrl, currentTenantId(), {
    onLog: log,
  });
  elements.serverOrigin.textContent = baseUrl;
}

function resetDefaults() {
  const table = currentTableName();
  elements.queryJson.value = JSON.stringify(defaultDemoQuery(table), null, 2);
  elements.insertJson.value = JSON.stringify(
    {
      title: "Ship convex-style Neovex demo",
      status: "open",
      priority: 1,
    },
    null,
    2,
  );
  elements.scheduleJson.value = JSON.stringify(
    {
      title: "Scheduled follow-up task",
      status: "queued",
      priority: 2,
    },
    null,
    2,
  );
}

function setSocketStatus(status, tone = "normal") {
  elements.socketStatus.textContent = status;
  elements.socketStatus.style.color =
    tone === "ok" ? "var(--ok)" : tone === "warn" ? "var(--warn)" : "inherit";
}

function renderDocuments(documents) {
  if (!documents.length) {
    elements.documents.innerHTML = `<div class="empty-state">No documents yet. Insert or schedule one to watch the subscription update.</div>`;
    return;
  }

  elements.documents.innerHTML = documents
    .map((document) => {
      const title =
        typeof document.title === "string"
          ? document.title
          : document._id ?? "document";
      return `
        <article class="document-card">
          <h3>${escapeHtml(title)}</h3>
          <pre>${escapeHtml(JSON.stringify(document, null, 2))}</pre>
        </article>
      `;
    })
    .join("");
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

async function withUiTask(label, work) {
  try {
    const result = await work();
    log(`${label} succeeded`, result);
    return result;
  } catch (error) {
    log(`${label} failed`, { message: error.message });
    throw error;
  }
}

async function ensureConnected() {
  refreshClients();
  await state.subscriptionClient.connect();
  setSocketStatus("connected", "ok");
}

elements.healthButton.addEventListener("click", async () => {
  refreshClients();
  await withUiTask("health check", () => state.http.health());
});

elements.createTenant.addEventListener("click", async () => {
  refreshClients();
  const tenantId = currentTenantId();
  await withUiTask("create tenant", () => state.http.createTenant(tenantId));
});

elements.installSchema.addEventListener("click", async () => {
  refreshClients();
  const table = currentTableName();
  const tenantId = currentTenantId();
  const schema = defaultDemoSchema(table);
  await withUiTask("install schema", () => state.http.setTableSchema(tenantId, table, schema));
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
  const query = parseJson("Query JSON", elements.queryJson.value);

  if (state.currentSubscription) {
    state.currentSubscription.unsubscribe();
    state.currentSubscription = null;
    elements.subscriptionId.textContent = "none";
  }

  const subscription = await withUiTask("subscribe query", () =>
    state.subscriptionClient.subscribe(query, {
      onResult: (documents) => {
        renderDocuments(documents);
        log("subscription result received", {
          count: documents.length,
          ids: documents.map((document) => document._id),
        });
      },
      onError: (error) => {
        log("subscription error", { message: error.message });
      },
    }),
  );

  state.currentSubscription = subscription;
  elements.subscriptionId.textContent = String(subscription.subscriptionId);
});

elements.insertDocument.addEventListener("click", async () => {
  refreshClients();
  const tenantId = currentTenantId();
  const table = currentTableName();
  const fields = parseJson("Insert fields JSON", elements.insertJson.value);
  await withUiTask("insert document", () => state.http.insertDocument(tenantId, table, fields));
});

elements.scheduleInsert.addEventListener("click", async () => {
  refreshClients();
  const tenantId = currentTenantId();
  const table = currentTableName();
  const fields = parseJson("Scheduled fields JSON", elements.scheduleJson.value);
  const runAfterMs = Number(elements.scheduleDelay.value);
  const response = await withUiTask("schedule insert", () =>
    state.http.scheduleMutation(tenantId, {
      run_after_ms: runAfterMs,
      mutation: {
        type: "insert",
        table,
        fields,
      },
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

  const tenantId = currentTenantId();
  await withUiTask("load scheduled job result", () =>
    state.http.getScheduledJobResult(tenantId, state.lastJobId),
  );
});

elements.tableName.addEventListener("change", resetDefaults);

elements.baseUrl.value = window.location.origin;
elements.serverOrigin.textContent = window.location.origin;
setSocketStatus("disconnected");
resetDefaults();
renderDocuments([]);
log("demo ready", {
  note: "Neovex is Convex-style and Convex-style at the product-model level, but this is a Neovex-native demo client, not the official Convex client SDK.",
});
