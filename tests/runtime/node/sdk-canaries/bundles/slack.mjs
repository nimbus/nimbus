import http from "node:http";
import { WebClient } from "@slack/web-api";

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

  const result = await withServer((request, response) => {
    requestPath = request.url;
    authHeader = request.headers.authorization ?? null;
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({
      ok: true,
      url: "https://nimbus.slack.com/",
      team: "Nimbus",
      user: "ada",
      team_id: "T_NIMBUS",
      user_id: "U_NIMBUS",
    }));
  }, async (baseUrl) => {
    const client = new WebClient("xoxb-nimbus", {
      slackApiUrl: `${baseUrl}/api/`,
    });
    return await client.auth.test();
  });

  return {
    ok: result.ok,
    team: result.team,
    user: result.user,
    requestPath,
    authHeader,
  };
};

export {};
