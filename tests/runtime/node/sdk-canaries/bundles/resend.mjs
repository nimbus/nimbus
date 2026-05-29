import http from "node:http";
import { Resend } from "resend";

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
  let requestSubject = null;
  let authHeader = null;

  const data = await withServer(async (request, response) => {
    requestPath = request.url;
    authHeader = request.headers.authorization ?? null;
    const body = await readJson(request);
    requestSubject = body.subject;
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({ id: "email_nimbus" }));
  }, async (baseUrl) => {
    const resend = new Resend("re_nimbus", { baseUrl });
    const result = await resend.emails.send({
      from: "Nimbus <noreply@example.com>",
      to: "ada@example.com",
      subject: "Nimbus canary",
      html: "<p>ok</p>",
    });
    return result.data;
  });

  return {
    id: data.id,
    requestPath,
    requestSubject,
    authHeader,
  };
};

export {};
