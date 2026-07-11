import {
  DeleteItemCommand,
  GetItemCommand,
  PutItemCommand,
  UpdateItemCommand,
} from "@aws-sdk/client-dynamodb";
import {
  TABLE_NAME,
  clearTasks,
  createClient,
  ensureTasksTable,
  listTasks,
  newTask,
  sleep,
  taskItem,
} from "./client.ts";

declare const process: {
  exitCode?: number;
};

let activeAnchor = "setup";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function pass(anchor: string, detail?: string): void {
  console.log(`PASS ${anchor}${detail ? ` — ${detail}` : ""}`);
}

async function main(): Promise<void> {
  const client = createClient();
  try {
    const tableState = await ensureTasksTable(client);
    console.log(`SETUP tasks table ${tableState}; ACTIVE synchronously (no waiter)`);
    await clearTasks(client);

    // tasks.create
    activeAnchor = "tasks.create";
    const first = newTask("Write the first task");
    await client.send(new PutItemCommand({ TableName: TABLE_NAME, Item: taskItem(first) }));
    const roundTrip = await client.send(
      new GetItemCommand({ TableName: TABLE_NAME, Key: { id: { S: first.id } } }),
    );
    assert(roundTrip.Item?.id?.S === first.id, "tasks.create must preserve its stable id");
    assert(
      Number(roundTrip.Item.createdAt?.N) === first.createdAt,
      "tasks.create must preserve createdAt",
    );
    const afterCreate = await listTasks(client);
    assert(
      afterCreate.length === 1,
      `tasks.create expected exactly one task, got ${afterCreate.length}`,
    );
    assert(afterCreate[0]?.text === first.text, "tasks.create must preserve text");
    assert(
      afterCreate[0]?.completed === false,
      "tasks.create must preserve completed === false",
    );
    pass("tasks.create");

    // tasks.list: Scan has no ordering guarantee, so listTasks sorts client-side.
    activeAnchor = "tasks.list";
    const second = newTask("Verify newest-first ordering", first.createdAt + 1);
    await client.send(new PutItemCommand({ TableName: TABLE_NAME, Item: taskItem(second) }));
    const afterSecondCreate = await listTasks(client);
    assert(
      afterSecondCreate.length === 2,
      `tasks.list expected two tasks, got ${afterSecondCreate.length}`,
    );
    assert(second.id !== first.id, "tasks.list tasks must have unique ids");
    assert(afterSecondCreate[0]?.id === second.id, "tasks.list must order newest first");
    assert(afterSecondCreate[1]?.id === first.id, "tasks.list must retain the older task");
    assert(
      afterSecondCreate[0]!.createdAt > afterSecondCreate[1]!.createdAt,
      "tasks.list must descend by createdAt",
    );
    pass("tasks.list", "Scan sorted client-side by createdAt descending");

    // tasks.toggle
    activeAnchor = "tasks.toggle";
    await client.send(
      new UpdateItemCommand({
        TableName: TABLE_NAME,
        Key: { id: { S: first.id } },
        UpdateExpression: "SET completed = :completed",
        ExpressionAttributeValues: { ":completed": { BOOL: true } },
      }),
    );
    const afterToggle = await listTasks(client);
    assert(
      afterToggle.find(({ id }) => id === first.id)?.completed === true,
      "tasks.toggle must persist completed === true",
    );
    pass("tasks.toggle");

    // tasks.delete
    activeAnchor = "tasks.delete";
    await client.send(
      new DeleteItemCommand({ TableName: TABLE_NAME, Key: { id: { S: second.id } } }),
    );
    const afterDelete = await listTasks(client);
    assert(
      !afterDelete.some(({ id }) => id === second.id),
      "tasks.delete must remove the selected task",
    );
    pass("tasks.delete");

    // tasks.live-update: DynamoDB has no live-query view here, so poll tasks.list.
    activeAnchor = "tasks.live-update";
    const liveTask = newTask("Observed by polling tasks.list", second.createdAt + 1);
    const beforeLiveInsert = await listTasks(client);
    assert(
      !beforeLiveInsert.some(({ id }) => id === liveTask.id),
      "tasks.live-update baseline must not contain the polled task",
    );
    await client.send(
      new PutItemCommand({ TableName: TABLE_NAME, Item: taskItem(liveTask) }),
    );
    const deadline = Date.now() + 3_000;
    let reads = 1;
    let observed = false;
    while (Date.now() < deadline) {
      await sleep(200);
      reads += 1;
      observed = (await listTasks(client)).some(({ id }) => id === liveTask.id);
      if (observed) break;
    }
    assert(observed, "tasks.live-update polling timed out before the task appeared");
    assert(reads >= 2, "tasks.live-update must repeat tasks.list after insertion");
    pass(
      "tasks.live-update",
      `polling tasks.list observed the insert after ${reads} reads; no live subscription`,
    );
  } finally {
    await clearTasks(client).catch(() => undefined);
    client.destroy();
  }
}

main().catch((error) => {
  console.error(`FAIL ${activeAnchor}: ${(error as Error).message}`);
  process.exitCode = 1;
});
