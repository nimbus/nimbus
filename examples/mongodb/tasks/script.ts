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

function requireCredentials(): { username: string; password: string } {
  if (!username || !password) {
    throw new Error(
      "Set NIMBUS_MONGODB_USERNAME and NIMBUS_MONGODB_PASSWORD before running the MongoDB tasks demo.",
    );
  }
  return { username, password };
}

function sleep(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function listTasks(
  tasks: Collection<TaskDocument>,
): Promise<WithId<TaskDocument>[]> {
  const documents = await tasks.find().toArray();
  return documents.sort((left, right) => right.createdAt - left.createdAt);
}

function taskSignature(tasks: WithId<TaskDocument>[]): string {
  return JSON.stringify(
    tasks.map(({ _id, text, completed, createdAt }) => ({
      id: _id.toString(),
      text,
      completed,
      createdAt,
    })),
  );
}

async function main(): Promise<void> {
  const credentials = requireCredentials();
  const client = new MongoClient(
    mongoUri({ host, port, database: "demo", ...credentials }),
  );
  await client.connect();

  try {
    const tasks = client.db("demo").collection<TaskDocument>("tasks");

    const createdAt = Date.now();
    const created = await tasks.insertOne({
      text: "Try the MongoDB tasks example",
      completed: false,
      createdAt,
    });
    console.log("tasks.create", created.insertedId.toString());

    console.log("tasks.list", await listTasks(tasks));

    await tasks.updateOne(
      { _id: created.insertedId },
      { $set: { completed: true } },
    );
    console.log("tasks.toggle", await listTasks(tasks));

    await tasks.deleteOne({ _id: created.insertedId });
    console.log("tasks.delete", await listTasks(tasks));

    console.log(
      "tasks.live-update uses polling because change streams are not supported.",
    );
    let previous = taskSignature(await listTasks(tasks));
    const polledTask = await tasks.insertOne({
      text: "Observed by polling tasks.list",
      completed: false,
      createdAt: Date.now(),
    });
    const deadline = Date.now() + 3_000;
    let observedChange = false;
    while (Date.now() < deadline) {
      await sleep(200);
      const currentTasks = await listTasks(tasks);
      const current = taskSignature(currentTasks);
      if (current !== previous) {
        console.log("tasks.live-update polled change", currentTasks);
        observedChange = true;
        break;
      }
      previous = current;
    }
    if (!observedChange) {
      throw new Error("tasks.live-update polling timed out before observing the new task");
    }
    await tasks.deleteOne({ _id: polledTask.insertedId });
  } finally {
    await client.close();
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
