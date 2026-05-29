import http from "node:http";
import { Redis } from "@upstash/redis";

async function readJson(request) {
  const chunks = [];
  for await (const chunk of request) {
    chunks.push(chunk);
  }
  const text = Buffer.concat(chunks).toString("utf8");
  return text ? JSON.parse(text) : null;
}

async function withServer(handler, callback) {
  const server = http.createServer(handler);
  await new Promise((resolve, reject) => {
    server.listen(0, "127.0.0.1", (error) => {
      if (error) reject(error);
      else resolve();
    });
  });
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : null;
  try {
    return await callback(`http://127.0.0.1:${port}`);
  } finally {
    await new Promise((resolve, reject) => {
      server.close((error) => {
        if (error) reject(error);
        else resolve();
      });
    });
  }
}

globalThis.__nimbusInvoke = async function () {
  const calls = [];
  const store = new Map();

  const value = await withServer(async (request, response) => {
    calls.push(request.url);
    const body = await readJson(request);
    const command = Array.isArray(body?.[0]) ? body[0] : Array.isArray(body) ? body : [];
    const verb = typeof command[0] === "string" ? command[0].toUpperCase() : null;
    if (verb === "SET") {
      store.set(command[1], command[2]);
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify([{ result: "OK" }]));
      return;
    }
    if (verb === "GET") {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify([{ result: store.get(command[1]) ?? null }]));
      return;
    }
    response.writeHead(400, { "content-type": "application/json" });
    response.end(JSON.stringify({ error: "unexpected command" }));
  }, async (baseUrl) => {
    const redis = new Redis({
      url: baseUrl,
      token: "upstash-nimbus",
      responseEncoding: "json",
    });
    await redis.set("nimbus:key", "redis-ok");
    return await redis.get("nimbus:key");
  });

  return {
    value,
    calls,
  };
};

export {};
