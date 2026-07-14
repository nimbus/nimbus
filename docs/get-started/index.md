---
title: What is Nimbus
description: The single-binary backend for apps and AI agents. Drop-in compatible with Convex, Firestore, MongoDB, and DynamoDB.
sidebar:
  order: 1
---

Nimbus is a backend in a single Rust binary: storage, compute, networking,
realtime subscriptions, and scheduling in one self-hostable process. It speaks
the wire protocols your clients already use — Convex, Firestore, Cloud
Functions, MongoDB, and DynamoDB — plus its own native HTTP/WebSocket API and
SDK.

Nimbus is **source-available** and built to be the thing you actually deploy:
on your own hardware, air-gapped if needed, with no telemetry and no metered
pricing.

## Choose your path

- **[Developer quickstart](/get-started/quickstart/)** — build an app on
  Nimbus: scaffold a project, write TypeScript functions, get reactive
  queries in your frontend. About 5 minutes.
- **[Build an AI agent](/agents/)** — give an agent isolated compute next
  to its data through sandboxes, services, and sessions. Start with the
  [sandbox quickstart](/agents/sandbox-quickstart/).
- **[Self-host quickstart](/get-started/self-host/)** — run Nimbus as a
  server and talk to it over HTTP. About 2 minutes, `curl` is enough.
- **[Coming from Convex](/get-started/from-convex/)** — what works today,
  what differs, and how to point an existing Convex project at Nimbus.
- **[Deploy to production](/get-started/deploy/)** — package your app and
  activate it against a running Nimbus server.

## Where to go next

- **[Developers](/developers/)** — tutorials and how-to guides for building
  apps: functions, schema, scheduling, adapters, the SDK.
- **[Agents](/agents/)** — build AI agents on Nimbus: sandboxes, services,
  and sessions through the SDK.
- **[Operators](/operators/)** — self-host Nimbus for your team: deploy,
  tenants, storage backends, encryption, networking, observability.
- **[Concepts](/concepts/)** — how Nimbus works: the engine, the data model,
  tenancy, and the architecture.
- **[Reference](/reference/)** — the CLI, configuration, APIs, and
  compatibility matrices.
