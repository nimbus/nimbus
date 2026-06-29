import { WebSocketServer, WebSocket } from "ws";
import { once } from "node:events";

// Loopback WebSocket round-trip using the raw `ws` package (the engine under
// socket.io). Proves an isolate under the local-development profile can both
// listen on loopback (net_listen 127.0.0.1) and connect to it (net_connect
// 127.0.0.1) within a single invocation. Production denies this; see the
// host-heavy ws-server-listen.mjs negative canary.
globalThis.__nimbusInvoke = async function () {
  const server = new WebSocketServer({ host: "127.0.0.1", port: 0 });
  await once(server, "listening");

  server.on("connection", (socket) => {
    socket.on("message", (data) => {
      socket.send(`echo:${data.toString()}`);
    });
  });

  const { port } = server.address();
  const client = new WebSocket(`ws://127.0.0.1:${port}`);
  await once(client, "open");

  const received = new Promise((resolve, reject) => {
    const timeout = setTimeout(
      () => reject(new Error("timeout waiting for echo")),
      5000,
    );
    client.once("message", (data) => {
      clearTimeout(timeout);
      resolve(data.toString());
    });
  });
  client.send("hello-ws");
  const echoed = await received;

  client.close();
  await new Promise((resolve) => server.close(() => resolve()));

  return {
    protocol: "ws",
    sent: "hello-ws",
    echoed,
  };
};

export {};
