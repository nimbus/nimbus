---
title: Run an agent in a sandbox
description: Package an AI agent as an OCI image, launch it as an isolated sandbox, pass it configuration through the environment, reach it on the host, and tear it down — with a clear view of the sandbox boundary today.
sidebar:
  order: 8
---

This tutorial launches an AI agent as an isolated sandbox: you package the
agent as an OCI image, run it as a standalone sandbox, hand it its
configuration through the process environment, wait for it to come up, reach
it on the host, and tear it down. One server hosts the sandbox next to your
data.

New to the three resources? Start with the
[agent sandbox quickstart](/agents/sandbox-quickstart/) and
[Run sandboxes](/agents/sandboxes/). This page assumes you have seen a sandbox
come up once.

## What you'll build, and the boundary

You will run an agent that serves requests inside a sealed sandbox and answer
them from the host. One boundary shapes the whole design, so state it before
writing any code: **a sandbox created through this API runs sealed.** Its
outbound network is deny-all, it has no caller-supplied mounts, and it runs
under default resource limits — none of which the create call can change. The
sandbox cannot reach the network, and it cannot call back into Nimbus. So the
agent does its work self-contained, and the one surface it exposes is an
in-sandbox port that Nimbus binds on the host's loopback interface for you to
reach. What the sandbox *can* do today is run your code, in isolation, next to
your tenant.

## Prerequisites

You need a running server, and on macOS or WSL2 a running machine.

```bash
nimbus start --port 8080 --data-dir ./data
```

Sandboxes execute on a Linux host. On Linux, skip the next command. On macOS or
WSL2, start the managed Linux VM that hosts sandboxes first:

```bash
nimbus machine start
```

See the [CLI reference](/reference/cli/) for `nimbus machine` flags. The full
status table is in
[current capabilities](/reference/current-capabilities/).

Grab the admin token and create a tenant:

```bash
export NIMBUS_TOKEN=$(nimbus auth token)

curl -s -X POST http://localhost:8080/api/tenants \
  -H "Authorization: Bearer $NIMBUS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"id": "demo"}'
```

Install the SDK for the script below. It ships inside the Nimbus binary
rather than on the public npm registry, so `nimbus packages provision`
materializes it into the project and points `package.json` at the
provisioned copy:

```bash
mkdir agent-in-a-sandbox && cd agent-in-a-sandbox
npm init -y
nimbus packages provision nimbus
npm install
```

## 1. Package the agent as an OCI image

A public sandbox spec runs an **OCI image referenced by name**. The image
carries your agent code and its runtime. Because the sandbox has no network
egress, the agent must be self-contained — it works from its inputs and its
own model or logic, not by calling out. A minimal agent that serves one
request:

```javascript
// agent.js — baked into the image at /app/agent.js
import { createServer } from "node:http";

const port = Number(process.env.PORT ?? 8787);
const persona = process.env.AGENT_PERSONA ?? "assistant";

const server = createServer((req, res) => {
  let body = "";
  req.on("data", (chunk) => (body += chunk));
  req.on("end", () => {
    const { prompt } = JSON.parse(body || "{}");
    const answer = runYourAgent(prompt, persona); // your self-contained logic
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ persona, answer }));
  });
});

server.listen(port, "0.0.0.0", () => console.log(`agent listening on ${port}`));
```

Build it and publish it to a registry your deployment can pull from. Expose the
port the agent listens on so Nimbus can bind it on the host:

```dockerfile
FROM node:22-alpine
WORKDIR /app
COPY agent.js /app/agent.js
EXPOSE 8787
CMD ["node", "/app/agent.js"]
```

The sandbox spec accepts an image **reference** only. Local Dockerfile build
contexts and host rootfs paths are operator-only inputs and are rejected by the
create call — build and push the image yourself, then reference the published
tag.

## 2. Configure the client

```javascript
import { Nimbus } from "@nimbus/nimbus";

const nimbus = new Nimbus({
  endpoint: "http://localhost:8080",
  tenantId: "demo",
  token: process.env.NIMBUS_TOKEN,
});
```

Endpoint and credential discovery is covered in the
[SDK resources reference](/reference/sdk/resources/).

## 3. Create the sandbox

Create a standalone sandbox from the image. Pass the agent its configuration
through `process.env` — a list of `KEY=value` strings. This is the input
channel into a sealed sandbox:

```javascript
const sandbox = await nimbus.sandboxes.create({
  profile: "worker",
  spec: {
    owner: { kind: "standalone", displayName: "research-agent" },
    backend: "container",
    root: {
      kind: "oci_image",
      source: {
        kind: "reference",
        reference: "registry.example.com/acme/research-agent:1.0.0",
      },
    },
    process: {
      argv: ["node", "/app/agent.js"],
      env: ["PORT=8787", "AGENT_PERSONA=research"],
    },
  },
  labels: { purpose: "research" },
});
console.log("created", sandbox.metadata.id, sandbox.status.lifecycleState);
```

The same request over HTTP:

```bash
curl -s -X POST http://localhost:8080/api/tenants/demo/sandboxes \
  -H "Authorization: Bearer $NIMBUS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "profile": "worker",
    "spec": {
      "owner": { "kind": "standalone", "displayName": "research-agent" },
      "backend": "container",
      "root": {
        "kind": "oci_image",
        "source": { "kind": "reference", "reference": "registry.example.com/acme/research-agent:1.0.0" }
      },
      "process": {
        "argv": ["node", "/app/agent.js"],
        "env": ["PORT=8787", "AGENT_PERSONA=research"]
      }
    },
    "labels": { "purpose": "research" }
  }'
```

`backend: "container"` selects OCI container isolation — the default for
standalone sandboxes. A libkrun microVM backend (`"krun"`) also runs on Linux
and is the default for services run through `nimbus compose`; see
[Run sandboxes](/agents/sandboxes/).

Launch inputs are write-only. When you read the sandbox back, `process.argv`
and `process.environment` return as `{ redacted: true, valueCount: n }`, not
their values — do not expect to read configuration back out of a sandbox.

## 4. Wait until it is ready

Poll the lifecycle state until the sandbox reports `ready`:

```javascript
let current = sandbox;
while (current.status.lifecycleState !== "ready") {
  await new Promise((resolve) => setTimeout(resolve, 250));
  current = await nimbus.sandboxes.get({ id: sandbox.metadata.id });
}
console.log("ready", current.metadata.id);
```

## 5. Reach the agent on the host

A port the image exposes is bound on the Nimbus host's loopback interface and
reported on the sandbox's status. Read the endpoints and send the agent a
request from the same machine the server runs on:

```javascript
// Each endpoint is { name, protocol, host, port } — for example
// { name: "http", protocol: "tcp", host: "127.0.0.1", port: 49152 }.
const [endpoint] = current.status.endpoints;
if (!endpoint) throw new Error("the sandbox exposed no endpoints");

const res = await fetch(`http://${endpoint.host}:${endpoint.port}`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ prompt: "Summarize the Q3 sales notes" }),
});
console.log(await res.json());
```

These endpoints are reachable from the host that runs the server, not from a
remote client — no public Nimbus route proxies to them. For a local
development server, that is the machine you are working on.

## 6. Tear down

Stop the sandbox when you are done:

```javascript
await nimbus.sandboxes.stop({ id: sandbox.metadata.id });
console.log("stopped");
```

A sandbox persists and counts against your tenant's sandbox quota — active
count, vCPUs, memory, disk, and log budget — until you stop it. Stop it to
release the charge.

## Current limitations

The agent sandbox is an early surface. A publicly created sandbox is **sealed
today**: it runs your code in isolation, but it has no data plane in or out
beyond the two channels this tutorial uses — configuration in through `env`,
and one request/response path out through a host-bound port. These are current
constraints, tracked in
[current capabilities](/reference/current-capabilities/), not the intended
long-term design. The create call accepts a deliberately small spec: `owner`,
`backend`, `root` (an OCI image reference), `process`, and an optional
`tenantId` that must match the route. Everything else about the sandbox's world
is fixed by the server, so design your agent for these boundaries:

- **Deny-all egress.** The sandbox has no outbound network — not to the
  internet, and not back to Nimbus. A create call cannot grant an egress
  allowance. Give the agent everything it needs at launch, through its image
  and `env`.
- **No host or database access.** In-sandbox code cannot call `ctx.db` or any
  Nimbus host operation; there is no bridge from a sandbox into the engine.
- **No caller mounts or volumes.** A created sandbox starts with no
  caller-supplied mounts.
- **Default resource limits.** vCPU, memory, disk, and log budgets come from
  tenant defaults; the create call does not set them.
- **Sessions carry no bytes.** The session API opens, lists, gets, and closes
  a lease and validates channel names, but moves no stdio or file bytes to a
  client. Reach an agent through a host-bound port, as above.

## Related pages

- [Agent sandbox quickstart](/agents/sandbox-quickstart/) — the shorter path
  through create, ready, session, and teardown.
- [Run sandboxes](/agents/sandboxes/) — specs, backends, labels, and what the
  API redacts.
- [Open sessions](/agents/sessions/) — channels, TTLs, and target rules.
- [Services, sandboxes, and sessions](/concepts/resource-model/) — why the
  model is shaped this way.
- [HTTP API reference](/reference/native/http-api/) — the wire surface beneath
  the SDK.
