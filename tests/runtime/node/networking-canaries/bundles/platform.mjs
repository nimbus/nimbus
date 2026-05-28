import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import { createRequire } from "node:module";
import path from "node:path";
import { Readable } from "node:stream";
import { setTimeout as delay } from "node:timers/promises";

function listen(server) {
  return new Promise((resolve, reject) => {
    server.listen(0, "127.0.0.1", (error) => {
      if (error) {
        reject(error);
      } else {
        resolve();
      }
    });
  });
}

function close(server) {
  return new Promise((resolve, reject) => {
    server.close((error) => {
      if (error) {
        reject(error);
      } else {
        resolve();
      }
    });
  });
}

async function readableText(stream) {
  let text = "";
  for await (const chunk of stream) {
    text += chunk;
  }
  return text;
}

globalThis.__nimbusInvoke = async function () {
  const localDir = path.dirname(new URL(import.meta.url).pathname);
  const canaryFile = path.join(localDir, "platform-canary.txt");
  fs.writeFileSync(canaryFile, "platform-canary", "utf8");

  const cjsPath = path.join(localDir, "platform-cjs.cjs");
  fs.writeFileSync(cjsPath, "module.exports = { cjsValue: 'cjs-ok' };\n", "utf8");
  const require = createRequire(import.meta.url);
  const { cjsValue } = require("./platform-cjs.cjs");

  const server = http.createServer((_request, response) => {
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({ ok: true, source: "platform" }));
  });
  await listen(server);
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : null;
  const response = await fetch(`http://127.0.0.1:${port}/platform`);
  const fetchBody = await response.json();
  await close(server);

  const streamText = await readableText(Readable.from(["stream", "-", "ok"]));
  const timerValue = await delay(1, "timer-ok");
  const nodeMajor = Number.parseInt(process.versions.node.split(".")[0], 10);

  return {
    nodeMajor,
    releaseLts: process.release.lts ?? null,
    esmValue: "esm-ok",
    cjsValue,
    fileRoundtrip: fs.readFileSync(canaryFile, "utf8"),
    pathBasename: path.basename(canaryFile),
    cryptoHash: crypto
      .createHash("sha256")
      .update("nimbus-node-platform-canary")
      .digest("hex")
      .slice(0, 12),
    streamText,
    timerValue,
    fetchStatus: response.status,
    fetchBody,
  };
};

export {};
