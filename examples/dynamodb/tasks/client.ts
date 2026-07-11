import {
  CreateTableCommand,
  DeleteItemCommand,
  DynamoDBClient,
  ResourceInUseException,
  ScanCommand,
  type AttributeValue,
} from "@aws-sdk/client-dynamodb";
import { clientConfig } from "@nimbus/dynamodb";

declare const process: {
  env: Record<string, string | undefined>;
};

export const TABLE_NAME = "tasks";

export interface Task {
  id: string;
  text: string;
  completed: boolean;
  createdAt: number;
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function configuration() {
  const endpoint = process.env.NIMBUS_DYNAMODB_ENDPOINT;
  if (!endpoint) return clientConfig();

  const accessKeyId = process.env.NIMBUS_DYNAMODB_ACCESS_KEY_ID;
  const secretAccessKey = process.env.NIMBUS_DYNAMODB_SECRET_ACCESS_KEY;
  assert(
    accessKeyId && secretAccessKey,
    "NIMBUS_DYNAMODB_ENDPOINT requires the access-key id and secret written by nimbus dev.",
  );
  return clientConfig({ endpoint, accessKeyId, secretAccessKey });
}

export function createClient(): DynamoDBClient {
  return new DynamoDBClient(configuration());
}

export async function ensureTasksTable(client: DynamoDBClient): Promise<"created" | "existing"> {
  try {
    await client.send(
      new CreateTableCommand({
        TableName: TABLE_NAME,
        AttributeDefinitions: [{ AttributeName: "id", AttributeType: "S" }],
        KeySchema: [{ AttributeName: "id", KeyType: "HASH" }],
        BillingMode: "PAY_PER_REQUEST",
      }),
    );
    return "created";
  } catch (error) {
    if (error instanceof ResourceInUseException) return "existing";
    throw error;
  }
}

export function newTask(text: string, createdAt = Date.now()): Task {
  return {
    id: `${createdAt}-${Math.random().toString(36).slice(2)}`,
    text,
    completed: false,
    createdAt,
  };
}

export function taskItem(task: Task): Record<string, AttributeValue> {
  return {
    id: { S: task.id },
    text: { S: task.text },
    completed: { BOOL: task.completed },
    createdAt: { N: task.createdAt.toString() },
  };
}

function taskFromItem(item: Record<string, AttributeValue>): Task {
  const id = item.id?.S;
  const text = item.text?.S;
  const completed = item.completed?.BOOL;
  const createdAt = Number(item.createdAt?.N);
  assert(id, "task must have a stable string id");
  assert(typeof text === "string", "task text must be a string");
  assert(typeof completed === "boolean", "task completed must be a boolean");
  assert(Number.isFinite(createdAt), "task createdAt must be a finite number");
  return { id, text, completed, createdAt };
}

export async function listTasks(client: DynamoDBClient): Promise<Task[]> {
  const tasks: Task[] = [];
  let exclusiveStartKey: Record<string, AttributeValue> | undefined;
  do {
    const page = await client.send(
      new ScanCommand({ TableName: TABLE_NAME, ExclusiveStartKey: exclusiveStartKey }),
    );
    tasks.push(...(page.Items ?? []).map(taskFromItem));
    exclusiveStartKey = page.LastEvaluatedKey;
  } while (exclusiveStartKey);

  return tasks.sort(
    (left, right) => right.createdAt - left.createdAt || left.id.localeCompare(right.id),
  );
}

export async function clearTasks(client: DynamoDBClient): Promise<void> {
  for (const task of await listTasks(client)) {
    await client.send(
      new DeleteItemCommand({ TableName: TABLE_NAME, Key: { id: { S: task.id } } }),
    );
  }
}

export function sleep(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
