import http from "node:http";
import OpenAI from "openai";

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
  let authHeader = null;

  const result = await withServer(async (request, response) => {
    requestPath = request.url;
    authHeader = request.headers.authorization ?? null;
    const body = await readJson(request);
    requestModel = body.model;
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({
      id: "chatcmpl_nimbus",
      object: "chat.completion",
      created: 0,
      model: body.model,
      choices: [
        {
          index: 0,
          message: { role: "assistant", content: "openai-ok" },
          finish_reason: "stop",
        },
      ],
      usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
    }));
  }, async (baseUrl) => {
    const client = new OpenAI({
      apiKey: "sk-nimbus",
      baseURL: `${baseUrl}/v1`,
    });
    const completion = await client.chat.completions.create({
      model: "nimbus-test-model",
      messages: [{ role: "user", content: "ping" }],
    });
    return completion.choices[0].message.content;
  });

  return {
    content: result,
    requestPath,
    requestModel,
    authHeader,
  };
};

export {};
