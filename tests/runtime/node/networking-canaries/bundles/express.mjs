import http from "node:http";
import { once } from "node:events";
import express from "express";

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
          traceHeader: response.headers["x-neovex-trace"] ?? null,
        });
      });
    });
    request.on("error", reject);
  });
}

globalThis.__neovexInvoke = async function () {
  const app = express();
  app.use((_request, response, next) => {
    response.setHeader("x-neovex-trace", "middleware-hit");
    next();
  });
  app.get("/ok", (_request, response) => {
    response.status(200).json({
      framework: "express",
      ok: true,
    });
  });
  app.get("/boom", (_request, _response, next) => {
    const error = new Error("express-canary-boom");
    error.statusCode = 418;
    next(error);
  });
  app.use((error, _request, response, _next) => {
    response.status(error?.statusCode ?? 500).json({
      framework: "express",
      ok: false,
      message: error?.message ?? "unknown",
    });
  });

  const server = app.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : null;
  const baseUrl = `http://127.0.0.1:${port}`;
  const ok = await requestJson(`${baseUrl}/ok`);
  const error = await requestJson(`${baseUrl}/boom`);
  await new Promise((resolve, reject) => {
    server.close((closeError) => {
      if (closeError) {
        reject(closeError);
      } else {
        resolve();
      }
    });
  });

  return {
    okStatus: ok.statusCode,
    okBody: ok.body,
    traceHeader: ok.traceHeader,
    errorStatus: error.statusCode,
    errorBody: error.body,
  };
};

export {};
