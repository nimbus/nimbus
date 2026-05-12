import http from "node:http";
import { request } from "undici";

function createServer() {
  return http.createServer((requestMessage, response) => {
    if (requestMessage.url === "/ok") {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({
        client: "undici",
        ok: true,
      }));
      return;
    }

    if (requestMessage.url === "/fail") {
      response.writeHead(418, { "content-type": "application/json" });
      response.end(JSON.stringify({
        client: "undici",
        ok: false,
      }));
      return;
    }

    response.writeHead(404, { "content-type": "application/json" });
    response.end(JSON.stringify({ ok: false }));
  });
}

globalThis.__nimbusInvoke = async function () {
  const server = createServer();
  await new Promise((resolve, reject) => {
    server.listen(0, "127.0.0.1", (error) => {
      if (error) {
        reject(error);
      } else {
        resolve();
      }
    });
  });
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : null;
  const baseUrl = `http://127.0.0.1:${port}`;

  const okResponse = await request(`${baseUrl}/ok`);
  const errorResponse = await request(`${baseUrl}/fail`);
  const okBody = await okResponse.body.json();
  const errorBody = await errorResponse.body.json();

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
    okStatus: okResponse.statusCode,
    okBody,
    errorStatus: errorResponse.statusCode,
    errorBody,
  };
};

export {};
