import { NimbusClient, NimbusHttpClient } from "@nimbus/nimbus/browser";

import { api } from "./nimbus/_generated/api.ts";
import type { Doc } from "./nimbus/_generated/dataModel.d.ts";

type Message = Doc<"messages">;

const nativeUrl = process.env.NIMBUS_NATIVE_URL ?? "http://localhost:8080";
const adminToken = process.env.NIMBUS_ADMIN_TOKEN;
const tenantId = process.env.NIMBUS_TENANT_ID ?? "demo";
const nimbusUrl = process.env.NIMBUS_URL ?? `${nativeUrl}/convex/${tenantId}`;
const conversationId = `smoke-${Date.now()}`;
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

function assertMessage(message: Message | undefined, message2: string): asserts message is Message {
  assert(message !== undefined, message2);
  assert(typeof message._id === "string" && message._id.length > 0, "message must have a stable id");
  assert(Number.isFinite(message.createdAt), "message must have a finite createdAt");
}

async function main() {
  await ensureTenant();

  const http = new NimbusHttpClient(nimbusUrl);

  // agent-chat.converse
  activeAnchor = "agent-chat.converse";
  const converseResult = await http.mutation(api.agent.send, {
    conversationId,
    text: "Hello there",
  });
  assert(converseResult.tool === null, "agent-chat.converse must not trigger a tool");
  const afterConverse = await http.query(api.agent.list, { conversationId });
  assert(afterConverse.length === 2, `agent-chat.converse expected two turns, got ${afterConverse.length}`);
  const userTurn = afterConverse.find((message) => message.role === "user");
  assertMessage(userTurn, "agent-chat.converse must persist the user's turn");
  assert(userTurn.text === "Hello there", "agent-chat.converse must preserve the user's text");
  const assistantTurn = afterConverse.find((message) => message.role === "assistant");
  assertMessage(assistantTurn, "agent-chat.converse must persist an assistant reply");
  assert(
    assistantTurn.text === 'Got your message: "Hello there".',
    "agent-chat.converse must echo the message in the reply",
  );
  assert(assistantTurn.tool === undefined, "agent-chat.converse reply must not carry a tool tag");
  pass("agent-chat.converse");

  // agent-chat.remember
  activeAnchor = "agent-chat.remember";
  const rememberResult = await http.mutation(api.agent.send, {
    conversationId,
    text: "remember: my favorite color is teal",
  });
  assert(rememberResult.tool === "remember", "agent-chat.remember must report the remember tool");
  const memoryAfterRemember = await http.query(api.agent.listMemory, { conversationId });
  assert(memoryAfterRemember.length === 1, `agent-chat.remember expected one fact, got ${memoryAfterRemember.length}`);
  assert(
    memoryAfterRemember[0]?.text === "my favorite color is teal",
    "agent-chat.remember must persist the exact fact text",
  );
  pass("agent-chat.remember");

  // agent-chat.recall
  activeAnchor = "agent-chat.recall";
  const recallResult = await http.mutation(api.agent.send, {
    conversationId,
    text: "what do you remember",
  });
  assert(recallResult.tool === "recall", "agent-chat.recall must report the recall tool");
  const afterRecall = await http.query(api.agent.list, { conversationId });
  const recallReply = [...afterRecall].reverse().find((message) => message.tool === "recall");
  assertMessage(recallReply, "agent-chat.recall must persist a reply carrying the recall tool");
  assert(
    recallReply.text.includes("1 thing"),
    `agent-chat.recall reply must report a live fact count, got: ${recallReply.text}`,
  );
  pass("agent-chat.recall");

  // agent-chat.schedule
  activeAnchor = "agent-chat.schedule";
  const live = new NimbusClient(nimbusUrl, { webSocket: globalThis.WebSocket });
  const initial = deferred<Message[]>();
  const delivered = deferred<Message[]>();
  const reminderText = "check the oven";
  const deliveredText = `Reminder: ${reminderText}`;
  let watchForReminder = false;
  const unsubscribe = live.onUpdate(
    api.agent.list,
    { conversationId },
    (messages) => {
      initial.resolve(messages);
      if (watchForReminder && messages.some((message) => message.text === deliveredText)) {
        delivered.resolve(messages);
      }
    },
    (error) => {
      initial.reject(error);
      delivered.reject(error);
    },
  );

  try {
    await withTimeout(
      initial.promise,
      "agent-chat.schedule timed out waiting for the initial subscription result",
    );
    watchForReminder = true;
    const scheduleResult = await http.mutation(api.agent.send, {
      conversationId,
      text: `remind me in 300ms: ${reminderText}`,
    });
    assert(scheduleResult.tool === "remind", "agent-chat.schedule must report the remind tool immediately");
    const deliveredMessages = await withTimeout(
      delivered.promise,
      "agent-chat.schedule timed out waiting for the scheduled follow-up to land",
    );
    assert(
      deliveredMessages.some((message) => message.text === deliveredText && message.tool === "remind-delivery"),
      "agent-chat.schedule follow-up must land tagged as remind-delivery",
    );
    pass("agent-chat.schedule");
  } finally {
    unsubscribe();
    live.close();
  }
}

main().catch((error) => {
  console.error(`FAIL ${activeAnchor}: ${(error as Error).message}`);
  process.exitCode = 1;
});
