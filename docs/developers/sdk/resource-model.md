---
title: Manage services, sandboxes, and sessions
description: Define services, launch sandboxes, and open sessions with the Nimbus JavaScript SDK.
sidebar:
  order: 1
---

The `@nimbus/nimbus` SDK exposes three resource namespaces on one client:
`services` (named workloads the server runs and supervises), `sandboxes`
(isolated execution environments), and `sessions` (scoped connections to a
running service or sandbox). This guide walks through the lifecycle of each.

## 1. Set up the client

```typescript
import { Nimbus } from "@nimbus/nimbus";

const nimbus = new Nimbus({
  endpoint: "http://localhost:8080",
  tenantId: "demo",
  token: process.env.NIMBUS_TOKEN,
});
```

Every option is optional — the client discovers missing settings in this
order:

- **Endpoint** — `endpoint` option, then `NIMBUS_ENDPOINT`, then the
  `endpoint` field of the local credential file.
- **Credential** — `token`, `apiKey`, or `credential` option; then
  `NIMBUS_TOKEN` (or `NIMBUS_BEARER_TOKEN`), `NIMBUS_API_KEY`, or
  `NIMBUS_WORKLOAD_IDENTITY_TOKEN`; then the local credential file at
  `~/.config/nimbus/application_default_credentials.json` (override the
  path with `NIMBUS_APPLICATION_CREDENTIALS`); then a workload identity
  token file named by `NIMBUS_WORKLOAD_IDENTITY_TOKEN_FILE`.

Bearer and workload identity tokens are sent as `Authorization: Bearer`;
API keys are sent as `X-Nimbus-Api-Key`. If discovery finds no endpoint or
no credential, the first request throws with a message naming the options
to set.

`tenantId` set on the client becomes the default for every call; any call
can override it by passing its own `tenantId`. Calls that need a tenant
throw if neither is set.

## 2. Define a service

A service is a named definition with a backend that tells Nimbus how to
run or reach it. Create one backed by a sandbox image:

```typescript
const created = await nimbus.services.create({
  name: "worker",
  backend: {
    kind: "sandbox",
    sandbox: {
      owner: { kind: "service", serviceName: "worker" },
      backend: "container",
      root: {
        kind: "oci_image",
        source: { kind: "reference", reference: "docker.io/library/node:22-alpine" },
      },
      process: { argv: ["node", "server.js"] },
    },
  },
  labels: { team: "search" },
});

console.log(created.metadata.generation, created.status.lifecycleState);
```

Two other backend kinds are available:

```typescript
// A built-in provider implemented by the server.
backend: { kind: "builtIn", provider: "browser" }
// Providers: "loadBalancer" | "serviceDiscovery" | "browser" | "modelGateway"

// An external endpoint Nimbus health-checks but does not run.
backend: {
  kind: "external",
  endpoint: { url: "https://search.internal:9200" },
  auth: { kind: "none" },
  health: { kind: "http", path: "/healthz" },
}
```

Responses are resource objects with `metadata` (name, `generation`,
`resourceVersion`, timestamps, labels), `spec`, and `status` (lifecycle
state, readiness, health, and Kubernetes-style `conditions`).

## 3. Start it and wait for readiness

```typescript
const service = await nimbus.services.start({ name: "worker", waitUntil: "ready" });
```

`start` and `restart` accept `waitUntil: "ready"` or `"healthy"`; `stop`
accepts `waitUntil: "stopped"`. With `waitUntil` set, the call polls until
the service reaches that state. To control the polling yourself:

```typescript
await nimbus.services.wait({
  name: "worker",
  until: "healthy",
  timeoutMs: 60_000, // default 30_000
  intervalMs: 500,   // default 250
});
```

`wait` throws if the deadline passes, including the last observed status in
the error message.

## 4. Update and delete with generation checks

Updates and deletes require the generation you last read, so concurrent
edits fail instead of silently overwriting:

```typescript
const current = await nimbus.services.get({ name: "worker" });

await nimbus.services.update({
  name: "worker",
  backend: { kind: "builtIn", provider: "loadBalancer" },
  ifMatchGeneration: current.metadata.generation,
});

await nimbus.services.delete({
  name: "worker",
  ifMatchGeneration: current.metadata.generation + 1,
});
```

A stale `ifMatchGeneration` fails with the error code
`op.precondition_failed` — re-read the resource and retry. `delete` also
accepts `force: true` to tear down a service that is still running.

List with pagination:

```typescript
const page = await nimbus.services.list({ limit: 20 });
for (const service of page.items) {
  console.log(service.metadata.name, service.status.readiness);
}
// page.metadata.nextPageToken feeds the next call's pageToken.
```

## 5. Run a standalone sandbox

Sandboxes can also run on their own, without a service definition:

```typescript
const sandbox = await nimbus.sandboxes.create({
  profile: "worker", // or "desktop"
  spec: {
    owner: { kind: "standalone", displayName: "batch-job" },
    backend: "container", // or "krun"
    root: {
      kind: "oci_image",
      source: { kind: "reference", reference: "docker.io/library/python:3.12-slim" },
    },
    process: { argv: ["python", "job.py"] },
  },
  labels: { purpose: "batch" },
});

const running = await nimbus.sandboxes.get({ id: sandbox.metadata.id });
console.log(running.status.lifecycleState, running.status.endpoints);

await nimbus.sandboxes.stop({ id: sandbox.metadata.id });
```

Filter listings by status or label:

```typescript
const batch = await nimbus.sandboxes.list({
  labelKey: "purpose",
  labelValue: "batch",
});
```

Sandbox responses redact launch inputs: `process.argv` and
`process.environment` come back as `{ redacted: true, valueCount: n }`
rather than their values.

## 6. Open a session

A session is a scoped, expiring connection to a running service or
sandbox, opened for one or more channels:

```typescript
const session = await nimbus.sessions.open({
  target: { service: { name: "worker" } }, // or { sandbox: { id: "..." } }
  channels: ["stdio"],                     // "cdp" | "page" | "stdio" | "files"
  requestedTtlMs: 10 * 60 * 1000,
});

console.log(session.metadata.id, session.spec.expiresAt);
```

The response's `spec.targetSnapshot` pins what the session attached to —
the target's name or id, generation, and backend at open time. Sessions
move through `open`, `closed`, and `expired` lifecycle states:

```typescript
const open = await nimbus.sessions.list({ state: "open" });

await nimbus.sessions.close({ id: session.metadata.id, reason: "done" });
```

## Error handling

SDK calls surface the server's structured errors — see the
[error reference](/reference/native/errors/) for the code catalog. The
underlying HTTP endpoints are listed in the
[HTTP API reference](/reference/native/http-api/) if you need to call them
without the SDK.
