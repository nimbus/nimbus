import http from "node:http";
import { once } from "node:events";
import next from "next";

function requestText(url) {
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
          body,
        });
      });
    });
    request.on("error", reject);
  });
}

const appDir = process.argv[2];
if (!appDir) {
  throw new Error("next smoke script requires an app directory argument");
}

const app = next({
  dev: false,
  dir: appDir,
});
await app.prepare();
const handle = app.getRequestHandler();
const server = http.createServer((request, response) => {
  handle(request, response);
});
server.listen(0, "127.0.0.1");
await once(server, "listening");

try {
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : null;
  const ok = await requestText(`http://127.0.0.1:${port}/`);
  const missing = await requestText(`http://127.0.0.1:${port}/missing`);
  console.log(
    JSON.stringify({
      okStatus: ok.statusCode,
      okBodyIncludes: ok.body.includes("next-canary-ok"),
      missingStatus: missing.statusCode,
      missingBodyIncludes: missing.body.includes("next-canary-not-found"),
    }),
  );
} finally {
  await new Promise((resolve, reject) => {
    server.close((error) => {
      if (error) {
        reject(error);
      } else {
        resolve();
      }
    });
  });
}
