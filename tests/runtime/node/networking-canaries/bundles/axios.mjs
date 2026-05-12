import http from "node:http";
import axios from "axios";

function createServer() {
  return http.createServer((request, response) => {
    if (request.url === "/ok") {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({
        client: "axios",
        ok: true,
      }));
      return;
    }

    if (request.url === "/fail") {
      response.writeHead(418, { "content-type": "application/json" });
      response.end(JSON.stringify({
        client: "axios",
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

  const okResponse = await axios.get(`${baseUrl}/ok`);
  const errorResponse = await axios
    .get(`${baseUrl}/fail`)
    .catch((error) => error.response);

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
    okStatus: okResponse.status,
    okBody: okResponse.data,
    errorStatus: errorResponse?.status ?? null,
    errorBody: errorResponse?.data ?? null,
  };
};

export {};
