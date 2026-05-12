import http from "node:http";
import { once } from "node:events";
import { Server } from "socket.io";
import { io as ioClient } from "socket.io-client";

function awaitEvent(socket, eventName) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error(`timeout waiting for ${eventName}`));
    }, 5000);

    socket.once(eventName, (payload) => {
      clearTimeout(timeout);
      resolve(payload);
    });
    socket.once("connect_error", (error) => {
      clearTimeout(timeout);
      reject(error);
    });
  });
}

globalThis.__nimbusInvoke = async function () {
  const server = http.createServer();
  const io = new Server(server, {
    cors: {
      origin: "*",
    },
  });

  io.on("connection", (socket) => {
    socket.emit("welcome", {
      transport: socket.conn.transport.name,
    });
    socket.on("ping-event", (payload) => {
      socket.emit("pong-event", {
        echoed: payload,
        clientCount: io.engine.clientsCount,
      });
    });
  });

  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : null;
  const client = ioClient(`http://127.0.0.1:${port}`, {
    transports: ["websocket"],
    reconnection: false,
    timeout: 5000,
  });

  const welcome = await awaitEvent(client, "welcome");
  const pongEvent = awaitEvent(client, "pong-event");
  client.emit("ping-event", {
    message: "hello",
  });
  const pongPayload = await pongEvent;

  client.close();
  await new Promise((resolve) => {
    io.close(() => {
      server.close(() => resolve());
    });
  });

  return {
    welcomeTransport: welcome?.transport ?? null,
    pongPayload,
  };
};

export {};
