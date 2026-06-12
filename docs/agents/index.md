---
title: Agents
description: Nimbus as the backend for AI agents — isolated sandboxes, named services, and leased sessions, served by the same single binary as everything else.
sidebar:
  order: 1
---

Nimbus is a backend AI agents can **run**, **inspect**, and get **isolated
compute** from — without adding a second vendor next to the database.

- **Run.** The whole backend is one binary. An agent (or the human
  supervising it) starts it with `nimbus start` and has storage, functions,
  realtime, scheduling, and sandbox execution in a single process — there
  is no service mesh to assemble before the first task runs.
- **Inspect.** The docs ship as [llms.txt](/llms.txt) artifacts, every API
  returns structured errors from a published
  [error catalog](/reference/native/errors/), and
  [current capabilities](/reference/current-capabilities/) is a plain
  table of what works today.
- **Isolate.** Agent workloads get three purpose-built resources, managed
  through the `@nimbus/nimbus` SDK or plain HTTP: **sandboxes** (isolated
  execution environments addressed by id), **services** (named workloads
  other code can depend on), and **sessions** (scoped, expiring connections
  for stdio, files, and browser control).

The shape to remember: reach for a **sandbox** when an agent needs an
isolated world for one task — it has an id, a lifecycle, and session
access, and it disappears without leaving a name behind. Promote work to a
**service** only when other code should depend on it by name. Open a
**session** when something needs to interact with a running resource under
a lease that expires on its own. The design rationale behind the three
nouns is explained in
[Services, sandboxes, and sessions](/concepts/resource-model/).

## Start here

- [Agent sandbox quickstart](/agents/sandbox-quickstart/) — create a
  sandbox, watch it run, open a session, and tear it down, in about five
  minutes.
- [Run sandboxes](/agents/sandboxes/) — standalone sandboxes: specs,
  labels, listing, and what the API redacts.
- [Manage services](/agents/services/) — named workloads: backends,
  readiness waiting, and generation-checked updates.
- [Open sessions](/agents/sessions/) — channels, TTLs, and target
  snapshots.

## Where sandboxes actually run

Sandbox execution runs on Linux hosts: workloads launch as OCI containers
with deny-by-default network egress. (A libkrun microVM backend exists but
fails closed for process execution today — containers are what run
workloads.) On macOS and WSL2, `nimbus machine` provides the managed Linux
VM that hosts them — see the [CLI reference](/reference/cli/). The full
status table is in
[current capabilities](/reference/current-capabilities/).

Ordinary Nimbus functions are not sandboxes: they run in V8 isolates
inside the server process and never appear in sandbox listings. The
boundary is explained in
[Services, sandboxes, and sessions](/concepts/resource-model/).

## Going deeper

- [Sandboxes and machines](/concepts/architecture/sandbox-machines/) — how
  the isolation seam works under the hood.
- [SDK resources reference](/reference/sdk/resources/) — every type and
  method signature.
- [HTTP API reference](/reference/native/http-api/) — the wire surface
  beneath the SDK, for agents that speak plain HTTP.
