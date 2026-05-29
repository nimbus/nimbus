import http from "node:http";
import Anthropic from "@anthropic-ai/sdk";

async function readJson(request) {
  const chunks = [];
  for await (const chunk of request) {
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
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
  let requestPath = null;
  let requestModel = null;
  let apiKeyHeader = null;

  const text = await withServer(async (request, response) => {
    requestPath = request.url;
    apiKeyHeader = request.headers["x-api-key"] ?? null;
    const body = await readJson(request);
    requestModel = body.model;
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({
      id: "msg_nimbus",
      type: "message",
      role: "assistant",
      model: body.model,
      content: [{ type: "text", text: "anthropic-ok" }],
      stop_reason: "end_turn",
      stop_sequence: null,
      usage: { input_tokens: 1, output_tokens: 1 },
    }));
  }, async (baseUrl) => {
    const client = new Anthropic({
      apiKey: "sk-ant-nimbus",
      baseURL: baseUrl,
    });
    const message = await client.messages.create({
      model: "claude-nimbus",
      max_tokens: 16,
      messages: [{ role: "user", content: "ping" }],
    });
    return message.content[0].text;
  });

  return {
    text,
    requestPath,
    requestModel,
    apiKeyHeader,
  };
};

export {};
