---
title: Reference
description: The facts — CLI, configuration, APIs, and compatibility matrices.
sidebar:
  order: 1
---

Information-oriented reference that mirrors the product surface. One
canonical page per fact; if a claim here disagrees with the code, the code
wins and the page gets fixed.

## Server

- [CLI](/reference/cli/) — every `nimbus` command, subcommand, and flag.
- [Configuration](/reference/configuration/) — every `nimbus start` flag,
  environment variable, and config key.
- [Deploy & admin API](/reference/deploy-admin-api/) — staging, diff, and
  activation.
- [Current capabilities](/reference/current-capabilities/) — the honest
  snapshot of what works today.

## JavaScript SDK

- [Overview](/reference/sdk/) — the `@nimbus/nimbus` package and its entry
  points.
- [Server functions](/reference/sdk/server/) — builders, validators, and
  context types.
- [Clients](/reference/sdk/client/) — WebSocket, HTTP, and REST clients.
- [Resources](/reference/sdk/resources/) — services, sandboxes, and
  sessions from the SDK.
- [React](/reference/sdk/react/) — providers and hooks.

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

