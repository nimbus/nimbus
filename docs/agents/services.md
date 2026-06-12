---
title: Manage services
description: Define named services with the Nimbus SDK — sandbox-backed workloads, readiness waiting, and generation-checked updates and deletes.
sidebar:
  order: 4
---

A service is a tenant-scoped named definition: a name other code can
depend on, a backend that tells Nimbus how the capability is provided, and
a generation counter that guards concurrent edits. Use one when work
should outlive a single task and be addressable by name — for everything
else, a [standalone sandbox](/agents/sandboxes/) is enough.

All examples assume a configured client:

```typescript
import { Nimbus } from "@nimbus/nimbus";

const nimbus = new Nimbus({
  endpoint: "http://localhost:8080",
  tenantId: "demo",
  token: process.env.NIMBUS_TOKEN,
});
```

## Define a sandbox-backed service

The workhorse backend embeds a sandbox spec in the definition; Nimbus
launches and supervises the sandbox on the service's behalf:

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

Responses are resource objects with `metadata` (name, `generation`,
`resourceVersion`, timestamps, labels), `spec`, and `status` (lifecycle
state, readiness, health, and Kubernetes-style `conditions`).

The owner metadata must name the owning service — launch is rejected if
they disagree. The service is not its sandbox: applications address the
capability by the service name, never by the id of whichever sandbox
happens to be running behind it.

## Start it and wait for readiness

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

`wait` throws if the deadline passes, including the last observed status
in the error message.

## Update and delete with generation checks

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

## Built-in and external backends

Two other backend kinds exist alongside sandbox-backed services:

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

Today only sandbox-backed services can actually be launched by the
service manager; built-in and external services exist as validated
definitions and report a `declared` lifecycle state rather than a running
one.

## Related pages

- [Open sessions](/agents/sessions/) — interactive access to a running
  service.
- [Services, sandboxes, and sessions](/concepts/resource-model/) — naming
  rules and the design rationale.
- [SDK resources reference](/reference/sdk/resources/) — full type and
  method signatures.
