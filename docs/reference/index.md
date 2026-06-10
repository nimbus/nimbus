---
title: Reference
description: The facts — CLI, configuration, APIs, and compatibility matrices.
sidebar:
  order: 1
---

Information-oriented reference that mirrors the product surface. One
canonical page per fact; if a claim here disagrees with the code, the code
wins and the page gets fixed.

## Native API

- [HTTP API](/reference/native/http-api/) — every endpoint, method, and
  response shape.
- [WebSocket protocol](/reference/native/websocket-protocol/) — the
  subscription handshake and frame catalog.
- [Errors](/reference/native/errors/) — the error envelope and full code
  catalog.

## Adapter compatibility

- [Convex](/reference/convex/compatibility/) — plus the
  [project layout](/reference/convex/project-layout/) and
  [usage rules](/reference/convex/usage-rules/).
- [Firestore](/reference/firebase/compatibility/) — plus
  [auth](/reference/firebase/auth/) and the
  [Listen protocol](/reference/firebase/websocket-listen/).
- [Cloud Functions](/reference/cloud-functions/compatibility/).
- [MongoDB](/reference/mongodb/operations/) — plus
  [drivers](/reference/mongodb/drivers/) and
  [tenant isolation](/reference/mongodb/tenant-isolation/).
- [DynamoDB](/reference/dynamodb/feature-coverage/) — plus
  [divergences](/reference/dynamodb/divergences/),
  [SDK compatibility](/reference/dynamodb/sdk-compatibility/), and
  [readiness](/reference/dynamodb/readiness/).

## Node.js runtime

- [Node compatibility](/reference/runtimes/node-compat/) — supported Node
  lines and what "supported" means.
- [Node APIs](/reference/runtimes/node-apis/) — built-in API coverage.
- [Packages](/reference/runtimes/packages/) — npm package support matrix.

## Still arriving

- **CLI** — every `nimbus` command, subcommand, and flag.
- **Configuration** — the flag ↔ environment variable ↔ config-file
  cross-reference.
- **Deploy & admin API** — staging, diff, and activation.
- **Current capabilities** — the honest snapshot of what works today.
