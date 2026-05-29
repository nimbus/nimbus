import http from "node:http";
import Stripe from "stripe";

async function readBody(request) {
  const chunks = [];
  for await (const chunk of request) {
    chunks.push(chunk);
  }
  return Buffer.concat(chunks).toString("utf8");
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
    return await callback(port);
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
  let requestBody = null;
  let authHeader = null;

  const customer = await withServer(async (request, response) => {
    requestPath = request.url;
    authHeader = request.headers.authorization ?? null;
    requestBody = await readBody(request);
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({
      id: "cus_nimbus",
      object: "customer",
      email: "ada@example.com",
    }));
  }, async (port) => {
    const stripe = new Stripe("sk_test_nimbus", {
      host: "127.0.0.1",
      port,
      protocol: "http",
      telemetry: false,
    });
    return await stripe.customers.create({ email: "ada@example.com" });
  });

  return {
    id: customer.id,
    email: customer.email,
    requestPath,
    requestBody,
    authHeader,
  };
};

export {};
