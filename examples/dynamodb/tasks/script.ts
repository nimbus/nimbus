import {
  DeleteItemCommand,
  PutItemCommand,
  UpdateItemCommand,
} from "@aws-sdk/client-dynamodb";
import {
  TABLE_NAME,
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

async function main(): Promise<void> {
  const client = createClient();
  try {
    const tableState = await ensureTasksTable(client);
    console.log(`tasks table ${tableState}; tables are ACTIVE synchronously`);

    const task = newTask("Try the DynamoDB tasks example");
    await client.send(new PutItemCommand({ TableName: TABLE_NAME, Item: taskItem(task) }));
    console.log("tasks.create", task.id);

    console.log("tasks.list", await listTasks(client));

    await client.send(
      new UpdateItemCommand({
        TableName: TABLE_NAME,
        Key: { id: { S: task.id } },
        UpdateExpression: "SET completed = :completed",
        ExpressionAttributeValues: { ":completed": { BOOL: true } },
      }),
    );
    console.log("tasks.toggle", await listTasks(client));

    await client.send(
      new DeleteItemCommand({ TableName: TABLE_NAME, Key: { id: { S: task.id } } }),
    );
    console.log("tasks.delete", await listTasks(client));

    console.log(
      "tasks.live-update uses polling because DynamoDB has no live-query view on this surface.",
    );
    const polledTask = newTask("Observed by polling tasks.list");
    await client.send(
      new PutItemCommand({ TableName: TABLE_NAME, Item: taskItem(polledTask) }),
    );
    const deadline = Date.now() + 3_000;
    while (Date.now() < deadline) {
      await sleep(200);
      if ((await listTasks(client)).some(({ id }) => id === polledTask.id)) {
        console.log("tasks.live-update polled change", polledTask);
        await client.send(
          new DeleteItemCommand({
            TableName: TABLE_NAME,
            Key: { id: { S: polledTask.id } },
          }),
        );
        return;
      }
    }
    throw new Error("tasks.live-update polling timed out before observing the new task");
  } finally {
    client.destroy();
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
