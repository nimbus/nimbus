---
title: Run sandboxes
description: Create standalone sandboxes with the Nimbus SDK — root images, owners, labels, listing, and what the API redacts.
sidebar:
  order: 3
---

A sandbox is a single isolated world: a root filesystem and a process to
run, with deny-by-default network egress, created for one purpose and
addressed by id for its whole life. This guide covers standalone sandboxes — the ones you create
directly, without a service definition. New to the model? Start with the
[agent sandbox quickstart](/agents/sandbox-quickstart/).

All examples assume a configured client:

```typescript
import { Nimbus } from "@nimbus/nimbus";

const nimbus = new Nimbus({
  endpoint: "http://localhost:8080",
  tenantId: "demo",
  token: process.env.NIMBUS_TOKEN,
});
```

Endpoint and credential discovery (environment variables, the local
credential file) is covered in the
[SDK resources reference](/reference/sdk/resources/).

## Create a standalone sandbox

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

The spec answers two separate questions:

- **What root material runs?** Either a prepared root filesystem or OCI
  image material — and image material is obtained either by *reference* (a
  registry image, as above) or by *build* (a Dockerfile and context). A
  build is a way to obtain an image, not a different kind of sandbox.
- **Who owns it?** `owner.kind: "standalone"` means you created it
  directly, with an optional display name. Sandboxes launched as a
  service's backend instead carry `{ kind: "service", serviceName }` owner
  metadata — see [Manage services](/agents/services/). Creating a
  standalone sandbox never implicitly registers a service name.

## Choose an isolation backend

`backend` selects the isolation mechanism, not the resource semantics —
the same spec, id, lifecycle, and session rules apply either way:

- `"container"` — OCI container isolation, driven through `crun`. This is
  the default backend for standalone sandboxes.
- `"krun"` — libkrun-based microVM isolation with per-sandbox egress
  enforcement. It executes workloads on Linux hosts — its launch stands up
  a deny-by-default network namespace and an egress proxy first — and is
  the default backend for services run through `nimbus compose`.

Both run on Linux hosts with deny-by-default outbound network access; on
macOS and WSL2, `nimbus machine` provides the hosting Linux VM. See
[current capabilities](/reference/current-capabilities/).

## List and filter

Labels are the filtering handle for throwaway resources:

```typescript
const batch = await nimbus.sandboxes.list({
  labelKey: "purpose",
  labelValue: "batch",
});
```

There is deliberately no resolve-sandbox-by-name API — a sandbox id is a
receipt, not a dependency contract. If other code needs to find the
workload by name, promote it to a service.

## What comes back redacted

Sandbox responses redact launch inputs: `process.argv` and
`process.environment` come back as `{ redacted: true, valueCount: n }`
rather than their values. Don't round-trip secrets through sandbox reads —
what you launched with is not readable back.

## Related pages

- [Open sessions](/agents/sessions/) — lease scoped, expiring sessions to a
  running sandbox.
- [Services, sandboxes, and sessions](/concepts/resource-model/) — the
  design rationale.
- [SDK resources reference](/reference/sdk/resources/) — full type and
  method signatures.
