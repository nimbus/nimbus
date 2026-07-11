import { mongoUri } from "@nimbus/mongodb";
import { MongoClient, type Collection, type WithId } from "mongodb";

declare const process: {
  env: Record<string, string | undefined>;
  exitCode?: number;
};

interface TaskDocument {
  text: string;
  completed: boolean;
  createdAt: number;
}

const host = process.env.NIMBUS_MONGODB_HOST ?? "127.0.0.1";
const port = process.env.NIMBUS_MONGODB_PORT
  ? Number(process.env.NIMBUS_MONGODB_PORT)
  : 27017;
const username = process.env.NIMBUS_MONGODB_USERNAME;
const password = process.env.NIMBUS_MONGODB_PASSWORD;
let activeAnchor = "setup";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function sleep(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function pass(anchor: string, detail?: string): void {
  console.log(`PASS ${anchor}${detail ? ` — ${detail}` : ""}`);
}

function requireCredentials(): { username: string; password: string } {
  assert(
    username && password,
    "Set NIMBUS_MONGODB_USERNAME and NIMBUS_MONGODB_PASSWORD before running the MongoDB tasks smoke.",
  );
  return { username, password };
}

function asTask(task: WithId<TaskDocument>): WithId<TaskDocument> {
  assert(task._id.toString().length > 0, "task must have a stable _id");
  assert(typeof task.text === "string", "task text must be a string");
  assert(
    typeof task.completed === "boolean",
    "task completed must be a boolean",
  );
  assert(
    Number.isFinite(task.createdAt),
    "task createdAt must be a finite number",
  );
  return task;
}

async function listTasks(
  tasks: Collection<TaskDocument>,
): Promise<WithId<TaskDocument>[]> {
  const documents = (await tasks.find().toArray()).map(asTask);
  return documents.sort((left, right) => right.createdAt - left.createdAt);
}

async function main(): Promise<void> {
  const credentials = requireCredentials();
  const client = new MongoClient(
    mongoUri({ host, port, database: "demo", ...credentials }),
  );
  await client.connect();
  const tasks = client.db("demo").collection<TaskDocument>("tasks");

  try {
    await tasks.deleteMany({});

    // tasks.create
    activeAnchor = "tasks.create";
    const firstCreatedAt = Date.now();
    const firstInsert = await tasks.insertOne({
      text: "Write the first task",
      completed: false,
      createdAt: firstCreatedAt,
    });
    assert(
      firstInsert.insertedId.toString().length > 0,
      "tasks.create must return a stable id",
    );
    const afterCreate = await listTasks(tasks);
    assert(
      afterCreate.length === 1,
      `tasks.create expected exactly one task, got ${afterCreate.length}`,
    );
    const first = afterCreate[0]!;
    assert(
      first._id.equals(firstInsert.insertedId),
      "tasks.create id must remain stable when retrieved",
    );
    assert(
      first.createdAt === firstCreatedAt,
      "tasks.create must preserve createdAt",
    );
    assert(first.text === "Write the first task", "tasks.create must preserve text");
    assert(first.completed === false, "tasks.create must preserve completed === false");
    pass("tasks.create");

    // tasks.list (the example sorts client-side by createdAt descending).
    activeAnchor = "tasks.list";
    const secondCreatedAt = firstCreatedAt + 1;
    const secondInsert = await tasks.insertOne({
      text: "Verify newest-first ordering",
      completed: false,
      createdAt: secondCreatedAt,
    });
    const afterSecondCreate = await listTasks(tasks);
    assert(
      afterSecondCreate.length === 2,
      `tasks.list expected two tasks, got ${afterSecondCreate.length}`,
    );
    assert(
      !secondInsert.insertedId.equals(firstInsert.insertedId),
      "tasks.list tasks must have unique ids",
    );
    assert(
      afterSecondCreate[0]?._id.equals(secondInsert.insertedId),
      "tasks.list must order the newest task first",
    );
    assert(
      afterSecondCreate[1]?._id.equals(firstInsert.insertedId),
      "tasks.list must retain the first task second",
    );
    assert(
      afterSecondCreate[0]!.createdAt > afterSecondCreate[1]!.createdAt,
      "tasks.list must descend by createdAt",
    );
    pass("tasks.list", "client-side createdAt descending");

    // tasks.toggle
    activeAnchor = "tasks.toggle";
    const toggleResult = await tasks.updateOne(
      { _id: firstInsert.insertedId },
      { $set: { completed: true } },
    );
    assert(
      toggleResult.modifiedCount === 1,
      "tasks.toggle must modify exactly one task",
    );
    const afterToggle = await listTasks(tasks);
    assert(
      afterToggle.find((task) => task._id.equals(firstInsert.insertedId))?.completed === true,
      "tasks.toggle must persist completed === true",
    );
    pass("tasks.toggle");

    // tasks.delete
    activeAnchor = "tasks.delete";
    const deleteResult = await tasks.deleteOne({ _id: secondInsert.insertedId });
    assert(deleteResult.deletedCount === 1, "tasks.delete must delete exactly one task");
    const afterDelete = await listTasks(tasks);
    assert(
      !afterDelete.some((task) => task._id.equals(secondInsert.insertedId)),
      "tasks.delete must remove the selected task",
    );
    pass("tasks.delete");

    // tasks.live-update: change streams are unavailable, so poll tasks.list.
    activeAnchor = "tasks.live-update";
    const liveText = "Observed by polling tasks.list";
    const beforeLiveInsert = await listTasks(tasks);
    assert(
      !beforeLiveInsert.some((task) => task.text === liveText),
      "tasks.live-update baseline must not already contain the polled task",
    );
    let pollCount = 1;
    const liveInsert = await tasks.insertOne({
      text: liveText,
      completed: false,
      createdAt: secondCreatedAt + 1,
    });
    const deadline = Date.now() + 3_000;
    let observed: WithId<TaskDocument> | undefined;
    while (Date.now() < deadline) {
      await sleep(200);
      pollCount += 1;
      observed = (await listTasks(tasks)).find((task) =>
        task._id.equals(liveInsert.insertedId)
      );
      if (observed) break;
    }
    assert(
      observed?.text === liveText,
      "tasks.live-update polling timed out before the new task appeared",
    );
    assert(
      pollCount >= 2,
      "tasks.live-update must perform a repeated tasks.list poll",
    );
    pass(
      "tasks.live-update",
      `polling tasks.list observed the insert after ${pollCount} reads; no live subscription`,
    );
  } finally {
    await tasks.deleteMany({});
    await client.close();
  }
}

main().catch((error) => {
  console.error(`FAIL ${activeAnchor}: ${(error as Error).message}`);
  process.exitCode = 1;
});
