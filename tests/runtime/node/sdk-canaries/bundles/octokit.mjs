import http from "node:http";
import { Octokit } from "octokit";

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
  let requestPath = null;
  let authHeader = null;

  const data = await withServer((request, response) => {
    requestPath = request.url;
    authHeader = request.headers.authorization ?? null;
    response.writeHead(200, {
      "content-type": "application/json",
      "x-github-api-version-selected": "2022-11-28",
    });
    response.end(JSON.stringify({
      login: "nimbus-bot",
      id: 42,
      type: "Bot",
    }));
  }, async (baseUrl) => {
    const octokit = new Octokit({
      auth: "ghp_nimbus",
      baseUrl,
    });
    const response = await octokit.request("GET /user");
    return response.data;
  });

  return {
    login: data.login,
    id: data.id,
    requestPath,
    authHeader,
  };
};

export {};
