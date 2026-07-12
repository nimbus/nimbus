import {
  NimbusRestClient,
  NimbusSubscriptionClient,
  type SubscribeQuery,
  type TableSchema,
} from "@nimbus/nimbus/transports/rest";
import WebSocket from "ws";

interface Task {
  _id: string;
  text: string;
  completed: boolean;
  createdAt: number;
}

interface IdResponse { id: string }
interface DataResponse { data: unknown[] }

const baseUrl = process.env.NIMBUS_NATIVE_URL ?? "http://localhost:8080";
const adminToken = process.env.NIMBUS_ADMIN_TOKEN;
const tenantId = `tasks-smoke-${Date.now()}`;
const table = "tasks";
const http = new NimbusRestClient(baseUrl, adminToken ? { token: adminToken } : {});
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
let activeAnchor = "setup";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function asTask(value: unknown): Task {
  assert(value !== null && typeof value === "object", "task must be an object");
  const task = value as Partial<Task>;
  assert(typeof task._id === "string" && task._id.length > 0, "task must have a stable _id");
  assert(typeof task.text === "string", "task text must be a string");
  assert(typeof task.completed === "boolean", "task completed must be a boolean");
  assert(typeof task.createdAt === "number", "task createdAt must be a number");
  return task as Task;
}

async function listTasks(): Promise<Task[]> {
  const response = await http.query(tenantId, query) as DataResponse;
  assert(Array.isArray(response.data), "tasks.list must return a data array");
  return response.data.map(asTask);
}

function pass(anchor: string) {
  console.log(`PASS ${anchor}`);
}

async function main() {
  await http.createTenant(tenantId);
  await http.setTableSchema(tenantId, table, schema);

  // tasks.create
  activeAnchor = "tasks.create";
  const firstCreatedAt = Date.now();
  const firstInsert = await http.insertDocument(tenantId, table, {
    text: "Write the first task",
    completed: false,
    createdAt: firstCreatedAt,
  }) as IdResponse;
  assert(typeof firstInsert.id === "string" && firstInsert.id.length > 0, "tasks.create must return an id");
  const afterCreate = await listTasks();
  assert(afterCreate.length === 1, `tasks.create expected exactly one task, got ${afterCreate.length}`);
  const first = afterCreate[0]!;
  assert(first._id === firstInsert.id, "tasks.create id must remain stable when retrieved");
  assert(first.createdAt === firstCreatedAt, "tasks.create must preserve createdAt");
  assert(first.text === "Write the first task", "tasks.create must preserve text");
  assert(first.completed === false, "tasks.create must default to incomplete");
  pass("tasks.create");

  // tasks.list
  activeAnchor = "tasks.list";
  const secondCreatedAt = firstCreatedAt + 1;
  const secondInsert = await http.insertDocument(tenantId, table, {
    text: "Verify newest-first ordering",
    completed: false,
    createdAt: secondCreatedAt,
  }) as IdResponse;
  const afterSecondCreate = await listTasks();
  assert(afterSecondCreate.length === 2, `tasks.list expected two tasks, got ${afterSecondCreate.length}`);
  assert(secondInsert.id !== firstInsert.id, "tasks.list tasks must have unique ids");
  assert(afterSecondCreate[0]!._id === secondInsert.id, "tasks.list must order newest first");
  assert(afterSecondCreate[1]!._id === firstInsert.id, "tasks.list must retain the first task second");
  assert(afterSecondCreate[0]!.createdAt > afterSecondCreate[1]!.createdAt, "tasks.list must descend by createdAt");
  pass("tasks.list");

  // tasks.toggle
  activeAnchor = "tasks.toggle";
  await http.updateDocument(tenantId, table, firstInsert.id, { completed: true });
  const afterToggle = await listTasks();
  assert(afterToggle.find((task) => task._id === firstInsert.id)?.completed === true, "tasks.toggle must persist completed === true");
  pass("tasks.toggle");

  // tasks.delete
  activeAnchor = "tasks.delete";
  await http.deleteDocument(tenantId, table, secondInsert.id);
  const afterDelete = await listTasks();
  assert(!afterDelete.some((task) => task._id === secondInsert.id), "tasks.delete must remove the selected task");
  pass("tasks.delete");

  // tasks.live-update
  activeAnchor = "tasks.live-update";
  const live = new NimbusSubscriptionClient(baseUrl, tenantId, adminToken ? {
    webSocketFactory(url, protocols) {
      return new WebSocket(url, protocols, {
        headers: { Authorization: `Bearer ${adminToken}` },
      }) as unknown as globalThis.WebSocket;
    },
  } : {});
  await live.connect();
  let subscription: Awaited<ReturnType<typeof live.subscribe>> | undefined;
  try {
    let observedInitialResult = false;
    let resolvePush!: (tasks: Task[]) => void;
    let rejectPush!: (error: Error) => void;
    const pushed = new Promise<Task[]>((resolve, reject) => {
      resolvePush = resolve;
      rejectPush = reject;
    });
    const liveText = "Arrived through the live subscription";
    subscription = await live.subscribe(query, {
      onResult(data) {
        try {
          const liveTasks = data.map(asTask);
          if (!observedInitialResult) {
            observedInitialResult = true;
            return;
          }
          if (liveTasks.some((task) => task.text === liveText)) resolvePush(liveTasks);
        } catch (error) {
          rejectPush(error as Error);
        }
      },
      onError(error) {
        rejectPush(error);
      },
    });
    assert(observedInitialResult, "tasks.live-update subscription must deliver its initial result before create");
    const liveInsert = await http.insertDocument(tenantId, table, {
      text: liveText,
      completed: false,
      createdAt: secondCreatedAt + 1,
    }) as IdResponse;
    let timeoutId: ReturnType<typeof setTimeout> | undefined;
    const timeout = new Promise<never>((_, reject) => {
      timeoutId = setTimeout(() => reject(new Error("tasks.live-update timed out waiting for a WebSocket push")), 5_000);
    });
    const liveTasks = await Promise.race([pushed, timeout]).finally(() => clearTimeout(timeoutId));
    assert(liveTasks.some((task) => task._id === liveInsert.id), "tasks.live-update push must contain the newly created task");
    pass("tasks.live-update");
  } finally {
    subscription?.unsubscribe();
    live.close();
  }
}

main().catch((error) => {
  console.error(`FAIL ${activeAnchor}: ${(error as Error).message}`);
  process.exitCode = 1;
});
