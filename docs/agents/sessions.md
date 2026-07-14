---
title: Open sessions
description: Lease scoped, expiring connections to running services and sandboxes — channels, TTLs, and target snapshots with the Nimbus SDK.
sidebar:
  order: 5
---

A session is a scoped, expiring connection to a running service or
sandbox. Simple service use needs no session — start the service and let
your app use the named dependency. Sessions model the *interactive* cases
— a stdio channel into a running workload, file exchange, or a
browser-control channel — as a declared, audited lease. Today the SDK and
HTTP surface cover the session lifecycle (open, get, list, close) and
validate the channels a target offers; client-facing byte transport over
those channels is not yet wired. A session is a lease, not an ambient
connection that outlives its purpose.

All examples assume a configured client:

```typescript
import { Nimbus } from "@nimbus/nimbus";

const nimbus = new Nimbus({
  endpoint: "http://localhost:8080",
  tenantId: "demo",
  token: process.env.NIMBUS_TOKEN,
});
```

## Open a session

```typescript
const session = await nimbus.sessions.open({
  target: { service: { name: "worker" } }, // or { sandbox: { id: "..." } }
  channels: ["stdio"],                     // "cdp" | "page" | "stdio" | "files"
  requestedTtlMs: 10 * 60 * 1000,
});

console.log(session.metadata.id, session.spec.expiresAt);
```

A session targets exactly one service (by name) or one sandbox (by id) —
never both, never neither. A sandbox target must be in the `ready` state.

## Declare the right channels

Channels are declared up front and validated against what the target can
actually offer:

| Target | Channels |
| --- | --- |
| Sandbox, or sandbox-backed service | `stdio`, `files` |
| Built-in `browser` service | `cdp`, `page` |
| Other built-in providers, external services | none |

## Set the TTL and close when done

The requested TTL is optional; the server applies a default of fifteen
minutes and caps every session at one hour. A session's state lives in
`session.status.lifecycleState`, which moves through `open`, `closed`
(explicitly closed, with a recorded reason), and `expired`. List with the
`state` filter to find sessions in a given state:

```typescript
const open = await nimbus.sessions.list({ state: "open" });

console.log(session.status.lifecycleState); // "open" | "closed" | "expired"

await nimbus.sessions.close({ id: session.metadata.id, reason: "done" });
```

## Check what the session attached to

The response's `spec.targetSnapshot` records exactly what the session
attached to — the service name or sandbox id, its generation, and its
backend at open time. Opening against a dynamic service whose definition
changed mid-open is rejected as a conflict rather than silently attaching
to something else.

Opening is also an authorization event: a service-targeted session needs
an exact grant for that service name, a sandbox-targeted session needs
reach to that specific sandbox id, and opens, lookups, and closes are
recorded as audit events.

## Error handling

SDK calls surface the server's structured errors — see the
[error reference](/reference/native/errors/) for the code catalog. The
underlying HTTP endpoints are listed in the
[HTTP API reference](/reference/native/http-api/) if you need to call
them without the SDK.

## Related pages

- [Run sandboxes](/agents/sandboxes/) — creating the targets sessions
  attach to.
- [Manage services](/agents/services/) — named targets and readiness.
- [Services, sandboxes, and sessions](/concepts/resource-model/) — why
  sessions are leases.
