---
title: Architecture
description: A system-by-system map of the Nimbus binary — thirteen pages, each owning one subsystem, each citing the crates and modules that implement it.
sidebar:
  order: 1
---

These pages explain how Nimbus is built, subsystem by subsystem. They are
the one place in the public docs where real crate and module paths appear
inline — every claim cites the source that implements it, so you can read
a page next to the code it describes. For the higher-altitude story, start
with [How Nimbus works](/concepts/how-nimbus-works/); for the contributor
entry point, the repository ships an `ARCHITECTURE.md` that links back
here.

## Request path

How a request enters the binary, takes on a protocol shape, and reaches
the engine.

- [Server and transport](/concepts/architecture/server-transport/) — how
  `nimbus-server` composes every protocol surface (HTTP, WebSockets,
  gRPC, sibling wire listeners) onto one engine, and where bind policy
  and the admin gate live.
- [Adapter crates](/concepts/architecture/adapters/) — the five protocol
  adapters as crates: what each owns, the thin server shims that mount
  them, and the bridge layer behind the runtime-executing pair.
- [Engine and the mutation path](/concepts/architecture/engine-mutation-path/)
  — the `Engine` coordinator, per-tenant runtimes, the single mutation
  path, the durable journal, execution units, the scheduler, and
  subscription delivery.
- [Storage](/concepts/architecture/storage/) — the five persistence
  providers, per-tenant physical isolation, the single-transaction
  atomicity invariant, index lifecycle, and encryption at rest.
- [Network control plane](/concepts/architecture/network-control-plane/):
  portable connectivity-resource identity, plans, leases, capability evidence,
  readiness, and recovery without transport or provider effects.

## Execution and isolation

Where user code runs and what keeps tenants apart.

- [Runtime and isolates](/concepts/architecture/runtime-isolates/) — the
  standalone V8 runtime crate, the `HostBridge` inversion, bundle
  integrity, and the resource limits around every invocation.
- [Sandboxes and machines](/concepts/architecture/sandbox-machines/) —
  the sandbox seam for tenant workloads (containers and libkrun
  microVMs) and the machine that provides a Linux host on non-Linux
  machines.
- [Auth and the trust boundary](/concepts/architecture/auth-trust/) —
  operator credentials, deploy credentials, end-user identity, and the
  line where adapters stop authenticating and the engine starts
  authorizing.
- [Tenancy](/concepts/architecture/tenancy/) — how Nimbus admits a
  tenant once, carries the decision everywhere, and keeps every layer
  fail-closed by default.

## Operating the binary

The surfaces an operator and a toolchain touch.

- [Node lifecycle](/concepts/architecture/node-lifecycle/) — how a node
  is installed and supervised, and the node-side machinery that drives
  systemd transient units over D-Bus.
- [CLI and codegen](/concepts/architecture/cli-codegen/) — the command
  tree, the boot sequence, the dev loop, and the embedded JavaScript
  codegen toolchain.
- [SDK and packages](/concepts/architecture/sdk-packages/) — the npm
  side of the monorepo: the canonical SDK, the Convex compatibility
  wrapper, codegen, the embedded admin UI, and binary-owned
  distribution.
- [Observability](/concepts/architecture/observability/) — the public
  health probe, admin-gated debug surfaces, per-tenant engine
  snapshots, latency budgets, structured logs, and the audit trail.
