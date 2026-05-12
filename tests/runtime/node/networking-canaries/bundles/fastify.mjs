import http from "node:http";
import Fastify from "fastify";

function requestJson(url) {
  return new Promise((resolve, reject) => {
    const request = http.get(url, (response) => {
      let body = "";
      response.setEncoding("utf8");
      response.on("data", (chunk) => {
        body += chunk;
      });
      response.on("end", () => {
        resolve({
          statusCode: response.statusCode ?? null,
          body: body.length === 0 ? null : JSON.parse(body),
          traceHeader: response.headers["x-nimbus-trace"] ?? null,
        });
      });
    });
    request.on("error", reject);
  });
}

globalThis.__nimbusInvoke = async function () {
  const app = Fastify();
  app.addHook("onSend", async (_request, reply, payload) => {
    reply.header("x-nimbus-trace", "fastify-hook");
    return payload;
  });
  app.get("/ok", async () => ({
    framework: "fastify",
    ok: true,
  }));
  app.get("/boom", async () => {
    const error = new Error("fastify-canary-boom");
    error.statusCode = 418;
    throw error;
  });
  app.setErrorHandler((error, _request, reply) => {
    reply.status(error?.statusCode ?? 500).send({
      framework: "fastify",
      ok: false,
      message: error?.message ?? "unknown",
    });
  });

  await app.listen({ port: 0, host: "127.0.0.1" });
  const address = app.server.address();
  const port = typeof address === "object" && address ? address.port : null;
  const baseUrl = `http://127.0.0.1:${port}`;
  const ok = await requestJson(`${baseUrl}/ok`);
  const error = await requestJson(`${baseUrl}/boom`);
  await app.close();

  return {
    okStatus: ok.statusCode,
    okBody: ok.body,
    traceHeader: ok.traceHeader,
    errorStatus: error.statusCode,
    errorBody: error.body,
  };
};

export {};
