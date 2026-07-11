import { deleteApp, initializeApp } from "firebase/app";
import {
  addDoc,
  collection,
  connectFirestoreEmulator,
  deleteDoc,
  getDocs,
  initializeFirestore,
  onSnapshot,
  orderBy,
  query,
  terminate,
  updateDoc,
  type DocumentData,
  type QuerySnapshot,
} from "firebase/firestore";

declare const process: {
  env: Record<string, string | undefined>;
  exitCode?: number;
};

interface Task {
  id: string;
  text: string;
  completed: boolean;
  createdAt: number;
}

const baseUrl = new URL(process.env.NIMBUS_FIRESTORE_URL ?? "http://localhost:8080");
const projectId = process.env.NIMBUS_FIRESTORE_PROJECT_ID ?? "demo";
let activeAnchor = "setup";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function pass(anchor: string): void {
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

function asTask(id: string, data: DocumentData): Task {
  assert(id.length > 0, "task must have a stable document id");
  assert(typeof data.text === "string", "task text must be a string");
  assert(typeof data.completed === "boolean", "task completed must be a boolean");
  assert(Number.isFinite(data.createdAt), "task createdAt must be a finite number");
  return {
    id,
    text: data.text,
    completed: data.completed,
    createdAt: data.createdAt as number,
  };
}

function tasksFrom(snapshot: QuerySnapshot): Task[] {
  return snapshot.docs.map((document) => asTask(document.id, document.data()));
}

async function main(): Promise<void> {
  const app = initializeApp(
    { apiKey: "nimbus-tasks-smoke", projectId },
    { name: projectId },
  );
  const firestore = initializeFirestore(app, { experimentalUnaryTransport: "rest" });
  // `mockUserToken` is required: Firestore requests must carry a verified
  // Firebase project claim (the #24 admission gate,
  // crates/nimbus-firebase/src/project_tenant_registry.rs) — an anonymous
  // (no-token) request has no verified project and is always refused. Only
  // `nimbus dev`'s Firestore-client auto-tenant bypass verifies this mock
  // token locally; there is no equivalent on `nimbus start`.
  connectFirestoreEmulator(
    firestore,
    baseUrl.hostname,
    baseUrl.port ? Number(baseUrl.port) : baseUrl.protocol === "https:" ? 443 : 80,
    {
      mockUserToken: {
        sub: "firebase-tasks-smoke",
        iss: `https://securetoken.google.com/${projectId}`,
      },
    },
  );
  const tasksCollection = collection(firestore, "tasks");
  const tasksQuery = query(tasksCollection, orderBy("createdAt", "desc"));
  const listTasks = async () => tasksFrom(await getDocs(tasksQuery));

  try {
    for (const existing of (await getDocs(tasksQuery)).docs) {
      await deleteDoc(existing.ref);
    }

    // tasks.create
    activeAnchor = "tasks.create";
    const firstCreatedAt = Date.now();
    const firstReference = await addDoc(tasksCollection, {
      text: "Write the first task",
      completed: false,
      createdAt: firstCreatedAt,
    });
    assert(firstReference.id.length > 0, "tasks.create must return a stable document id");
    const afterCreate = await listTasks();
    assert(afterCreate.length === 1, `tasks.create expected exactly one task, got ${afterCreate.length}`);
    const first = afterCreate[0]!;
    assert(first.id === firstReference.id, "tasks.create id must remain stable when retrieved");
    assert(first.createdAt === firstCreatedAt, "tasks.create must preserve createdAt");
    assert(first.text === "Write the first task", "tasks.create must preserve text");
    assert(first.completed === false, "tasks.create must preserve completed === false");
    pass("tasks.create");

    // tasks.list
    activeAnchor = "tasks.list";
    const secondCreatedAt = firstCreatedAt + 1;
    const secondReference = await addDoc(tasksCollection, {
      text: "Verify newest-first ordering",
      completed: false,
      createdAt: secondCreatedAt,
    });
    const afterSecondCreate = await listTasks();
    assert(afterSecondCreate.length === 2, `tasks.list expected two tasks, got ${afterSecondCreate.length}`);
    assert(secondReference.id !== firstReference.id, "tasks.list tasks must have unique ids");
    assert(afterSecondCreate[0]?.id === secondReference.id, "tasks.list must order the newest task first");
    assert(afterSecondCreate[1]?.id === firstReference.id, "tasks.list must retain the first task second");
    assert(
      afterSecondCreate[0]!.createdAt > afterSecondCreate[1]!.createdAt,
      "tasks.list must descend by createdAt",
    );
    pass("tasks.list");

    // tasks.toggle
    activeAnchor = "tasks.toggle";
    await updateDoc(firstReference, { completed: true });
    const afterToggle = await listTasks();
    assert(
      afterToggle.find((task) => task.id === firstReference.id)?.completed === true,
      "tasks.toggle must persist completed === true",
    );
    pass("tasks.toggle");

    // tasks.delete
    activeAnchor = "tasks.delete";
    await deleteDoc(secondReference);
    const afterDelete = await listTasks();
    assert(
      !afterDelete.some((task) => task.id === secondReference.id),
      "tasks.delete must remove the selected task",
    );
    pass("tasks.delete");

    // tasks.live-update
    activeAnchor = "tasks.live-update";
    const initial = deferred<Task[]>();
    const pushed = deferred<Task[]>();
    const liveText = "Arrived through onSnapshot";
    let watchForLiveTask = false;
    const unsubscribe = onSnapshot(
      tasksQuery,
      (snapshot) => {
        try {
          const liveTasks = tasksFrom(snapshot);
          initial.resolve(liveTasks);
          if (watchForLiveTask && liveTasks.some((task) => task.text === liveText)) {
            pushed.resolve(liveTasks);
          }
        } catch (error) {
          initial.reject(error as Error);
          pushed.reject(error as Error);
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
        "tasks.live-update timed out waiting for the initial onSnapshot result",
      );
      watchForLiveTask = true;
      const liveReference = await addDoc(tasksCollection, {
        text: liveText,
        completed: false,
        createdAt: secondCreatedAt + 1,
      });
      const liveTasks = await withTimeout(
        pushed.promise,
        "tasks.live-update timed out waiting for an onSnapshot push",
      );
      assert(
        liveTasks.some((task) => task.id === liveReference.id && task.text === liveText),
        "tasks.live-update push must contain the newly created task",
      );
      pass("tasks.live-update");
    } finally {
      unsubscribe();
    }
  } finally {
    await terminate(firestore);
    await deleteApp(app);
  }
}

main().catch((error) => {
  console.error(`FAIL ${activeAnchor}: ${(error as Error).message}`);
  process.exitCode = 1;
});
