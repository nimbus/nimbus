import { ConvexClient, ConvexHttpClient } from "convex/browser";

import { api } from "./convex/_generated/api.ts";

declare const process: {
  env: Record<string, string | undefined>;
  exit(code: number): never;
  once(event: "SIGINT" | "SIGTERM", listener: () => void): void;
};

const nativeUrl = process.env.NIMBUS_NATIVE_URL ?? "http://localhost:8080";
const convexUrl = process.env.NIMBUS_CONVEX_URL ?? "http://localhost:8080/convex/demo";
const author = process.env.NIMBUS_NODE_DEMO_AUTHOR ?? "Node Demo";

async function ensureTenant() {
  const response = await fetch(`${nativeUrl}/api/tenants`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ id: "demo" }),
  });
  if (!response.ok && response.status !== 409) {
    throw new Error(`failed to ensure demo tenant: ${response.status}`);
  }
}

async function main() {
  await ensureTenant();

  const http = new ConvexHttpClient(convexUrl);
  const live = new ConvexClient(convexUrl, {
    webSocket: globalThis.WebSocket,
  });

  const unsubscribe = live.onUpdate(api.messages.list, {}, (messages) => {
    console.log("Live messages:", messages);
  });

  const shutdown = () => {
    unsubscribe();
    live.close();
    process.exit(0);
  };

  process.once("SIGINT", shutdown);
  process.once("SIGTERM", shutdown);

  const initialMessages = await http.query(api.messages.list, {});
  console.log("Initial messages:", initialMessages);

  const id = await live.mutation(api.messages.send, {
    author,
    body: `Hello from Node at ${new Date().toISOString()}`,
  });
  console.log("Inserted message:", id);
  console.log("Press Ctrl+C to exit.");

  await new Promise(() => {});
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
