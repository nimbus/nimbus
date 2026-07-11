import {
  NimbusRestClient,
  NimbusSubscriptionClient,
  type SubscribeQuery,
  type TableSchema,
} from "@nimbus/nimbus/transports/rest";
import "./app.css";

interface Task {
  _id: string;
  text: string;
  completed: boolean;
  createdAt: number;
}

interface TenantList {
  tenants: string[];
}

const tenantId = "demo";
const table = "tasks";
const baseUrl = new URLSearchParams(window.location.search).get("server") ?? "http://localhost:8080";
const http = new NimbusRestClient(baseUrl);
const live = new NimbusSubscriptionClient(baseUrl, tenantId);
const query: SubscribeQuery = {
  table,
  filters: [],
  order: { field: "createdAt", direction: "desc" },
};
const schema: TableSchema = {
  table,
  fields: [
    { name: "text", field_type: "string", required: true },
    { name: "completed", field_type: "boolean", required: true },
    { name: "createdAt", field_type: "number", required: true },
  ],
  indexes: [{ name: "by_created_at", fields: ["createdAt"] }],
};

const form = document.querySelector<HTMLFormElement>("#task-form")!;
const input = document.querySelector<HTMLInputElement>("#task-text")!;
const list = document.querySelector<HTMLUListElement>("#task-list")!;
const count = document.querySelector<HTMLElement>("#task-count")!;
const empty = document.querySelector<HTMLElement>("#empty-state")!;
const errorMessage = document.querySelector<HTMLElement>("#error-message")!;
const connectionDot = document.querySelector<HTMLElement>("#connection-dot")!;
const connectionStatus = document.querySelector<HTMLElement>("#connection-status")!;

let tasks: Task[] = [];
let unsubscribe: (() => void) | undefined;

function isTask(value: unknown): value is Task {
  if (!value || typeof value !== "object") return false;
  const task = value as Partial<Task>;
  return typeof task._id === "string"
    && typeof task.text === "string"
    && typeof task.completed === "boolean"
    && typeof task.createdAt === "number";
}

function setError(error?: unknown) {
  errorMessage.textContent = error ? (error as Error).message : "";
}

function setConnected(connected: boolean) {
  connectionDot.classList.toggle("connected", connected);
  connectionStatus.textContent = connected ? `Live on ${tenantId}` : "Connecting…";
}

function render() {
  list.replaceChildren(...tasks.map((task) => {
    const item = document.createElement("li");
    const label = document.createElement("label");
    const toggle = document.createElement("input");
    const text = document.createElement("span");
    const remove = document.createElement("button");

    toggle.type = "checkbox";
    toggle.checked = task.completed;
    toggle.setAttribute("aria-label", `Mark ${task.text} ${task.completed ? "incomplete" : "complete"}`);
    toggle.addEventListener("change", () => void updateTask(task, toggle.checked));
    text.textContent = task.text;
    label.append(toggle, text);
    remove.type = "button";
    remove.className = "delete";
    remove.textContent = "Delete";
    remove.setAttribute("aria-label", `Delete ${task.text}`);
    remove.addEventListener("click", () => void deleteTask(task));
    item.classList.toggle("completed", task.completed);
    item.append(label, remove);
    return item;
  }));
  count.textContent = `${tasks.length} ${tasks.length === 1 ? "task" : "tasks"}`;
  empty.hidden = tasks.length > 0;
}

async function updateTask(task: Task, completed: boolean) {
  try {
    setError();
    await http.updateDocument(tenantId, table, task._id, { completed });
  } catch (error) {
    setError(error);
    render();
  }
}

async function deleteTask(task: Task) {
  try {
    setError();
    await http.deleteDocument(tenantId, table, task._id);
  } catch (error) {
    setError(error);
  }
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  const text = input.value.trim();
  if (!text) return;
  try {
    setError();
    await http.insertDocument(tenantId, table, { text, completed: false, createdAt: Date.now() });
    form.reset();
    input.focus();
  } catch (error) {
    setError(error);
  }
});

async function start() {
  const tenants = await http.listTenants() as TenantList;
  if (!tenants.tenants.includes(tenantId)) await http.createTenant(tenantId);
  await http.setTableSchema(tenantId, table, schema);
  await live.connect();
  const subscription = await live.subscribe(query, {
    onResult(data) {
      tasks = data.filter(isTask);
      setConnected(true);
      render();
    },
    onError(error) {
      setError(error);
      setConnected(false);
    },
  });
  unsubscribe = subscription.unsubscribe;
}

window.addEventListener("beforeunload", () => {
  unsubscribe?.();
  live.close();
});

render();
start().catch((error) => {
  setError(error);
  setConnected(false);
});
