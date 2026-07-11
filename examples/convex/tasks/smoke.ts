import { ConvexClient, ConvexHttpClient } from "convex/browser";

import { api } from "./convex/_generated/api.ts";
import type { Doc, Id } from "./convex/_generated/dataModel.d.ts";

declare const process: {
  env: Record<string, string | undefined>;
  exitCode?: number;
};

type Task = Doc<"tasks">;

const nativeUrl = process.env.NIMBUS_NATIVE_URL ?? "http://localhost:8080";
const adminToken = process.env.NIMBUS_ADMIN_TOKEN;
const tenantId = process.env.NIMBUS_TENANT_ID ?? "demo";
const convexUrl = process.env.NIMBUS_CONVEX_URL ?? `${nativeUrl}/convex/${tenantId}`;
let activeAnchor = "setup";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function pass(anchor: string) {
  console.log(`PASS ${anchor}`);
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function withTimeout<T>(promise: Promise<T>, message: string): Promise<T> {
  let timeoutId: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timeoutId = setTimeout(() => reject(new Error(message)), 5_000);
  });
  return await Promise.race([promise, timeout]).finally(() => clearTimeout(timeoutId));
}

async function ensureTenant() {
  const response = await fetch(`${nativeUrl}/api/tenants`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(adminToken ? { Authorization: `Bearer ${adminToken}` } : {}),
    },
    body: JSON.stringify({ id: tenantId }),
  });
  if (!response.ok && response.status !== 409) {
    throw new Error(`failed to ensure smoke tenant: ${response.status}`);
  }
}

function assertTask(task: Task | undefined, message: string): asserts task is Task {
  assert(task !== undefined, message);
  assert(typeof task._id === "string" && task._id.length > 0, "task must have a stable id");
  assert(Number.isFinite(task.createdAt), "task must have a finite createdAt");
}

async function main() {
  await ensureTenant();

  const http = new ConvexHttpClient(convexUrl);
  const existingTasks = await http.query(api.tasks.list, {});
  for (const task of existingTasks) {
    await http.mutation(api.tasks.remove, { id: task._id });
  }

  // tasks.create
  activeAnchor = "tasks.create";
  const firstId = await http.mutation(api.tasks.create, {
    text: "Write the first task",
  });
  assert(typeof firstId === "string" && firstId.length > 0, "tasks.create must return a stable id");
  const afterCreate = await http.query(api.tasks.list, {});
  assert(afterCreate.length === 1, `tasks.create expected exactly one task, got ${afterCreate.length}`);
  const first = afterCreate[0];
  assertTask(first, "tasks.create must return the created task from tasks.list");
  assert(first._id === firstId, "tasks.create id must remain stable when retrieved");
  assert(first.text === "Write the first task", "tasks.create must preserve text");
  assert(first.completed === false, "tasks.create must default to completed === false");
  pass("tasks.create");

  // tasks.list
  activeAnchor = "tasks.list";
  while (Date.now() <= first.createdAt) {
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
  const secondId = await http.mutation(api.tasks.create, {
    text: "Verify newest-first ordering",
  });
  const afterSecondCreate = await http.query(api.tasks.list, {});
  assert(afterSecondCreate.length === 2, `tasks.list expected two tasks, got ${afterSecondCreate.length}`);
  assert(secondId !== firstId, "tasks.list tasks must have unique ids");
  assert(afterSecondCreate[0]?._id === secondId, "tasks.list must order the newest task first");
  assert(afterSecondCreate[1]?._id === firstId, "tasks.list must retain the first task second");
  assert(
    afterSecondCreate[0]!.createdAt > afterSecondCreate[1]!.createdAt,
    "tasks.list must descend by createdAt",
  );
  pass("tasks.list");

  // tasks.toggle
  activeAnchor = "tasks.toggle";
  await http.mutation(api.tasks.toggle, { id: firstId });
  const afterToggle = await http.query(api.tasks.list, {});
  assert(
    afterToggle.find((task) => task._id === firstId)?.completed === true,
    "tasks.toggle must persist completed === true",
  );
  pass("tasks.toggle");

  // tasks.delete
  activeAnchor = "tasks.delete";
  await http.mutation(api.tasks.remove, { id: secondId });
  const afterDelete = await http.query(api.tasks.list, {});
  assert(
    !afterDelete.some((task) => task._id === secondId),
    "tasks.delete must remove the selected task",
  );
  pass("tasks.delete");

  // tasks.live-update
  activeAnchor = "tasks.live-update";
  const live = new ConvexClient(convexUrl, {
    webSocket: globalThis.WebSocket,
  });
  const initial = deferred<Task[]>();
  const pushed = deferred<Task[]>();
  const liveText = "Arrived through the reactive query";
  let watchForLiveTask = false;
  const unsubscribe = live.onUpdate(
    api.tasks.list,
    {},
    (tasks) => {
      initial.resolve(tasks);
      if (watchForLiveTask && tasks.some((task) => task.text === liveText)) {
        pushed.resolve(tasks);
      }
    },
    (error) => {
      initial.reject(error);
      pushed.reject(error);
    },
  );

  try {
    await withTimeout(
      initial.promise,
      "tasks.live-update timed out waiting for the initial subscription result",
    );
    watchForLiveTask = true;
    const liveId: Id<"tasks"> = await http.mutation(api.tasks.create, { text: liveText });
    const liveTasks = await withTimeout(
      pushed.promise,
      "tasks.live-update timed out waiting for a subscription push",
    );
    assert(
      liveTasks.some((task) => task._id === liveId && task.text === liveText),
      "tasks.live-update push must contain the newly created task",
    );
    pass("tasks.live-update");
  } finally {
    unsubscribe();
    live.close();
  }
}

main().catch((error) => {
  console.error(`FAIL ${activeAnchor}: ${(error as Error).message}`);
  process.exitCode = 1;
});
