---
title: JavaScript SDK
description: Reference for the @nimbus/nimbus JavaScript SDK — entry points, exports, and conventions.
sidebar:
  label: Overview
  order: 1
---

`@nimbus/nimbus` is the Nimbus-native JavaScript SDK. It covers three roles:

- **Server functions** — queries, mutations, actions, HTTP actions, and schema
  definitions that run inside a Nimbus deployment.
- **Data-plane clients** — browser and Node clients that call functions,
  subscribe to live query results, and schedule work against a deployment.
- **Control-plane client** — the `Nimbus` root client that manages services,
  sandboxes, and sessions.

The package is ESM-only (`"type": "module"`) and ships TypeScript sources
directly. `react` and `react-dom` (`^18` or `^19`) are optional peer
dependencies, needed only for the `@nimbus/nimbus/react` entry point.

## Installation

The package is private and is distributed with the Nimbus binary rather than
through the public npm registry. Projects scaffolded by the Nimbus CLI
reference the SDK with `file:` specifiers that point into the project's
`.nimbus/packages/` directory; the binary provisions and refreshes those
packages. See [Your first app](/developers/first-app/) for the project setup
flow.

The `convex` compatibility package wraps this SDK and re-exports its server
APIs under Convex-style names. If you are migrating a Convex project, see
[Convex compatibility](/developers/convex/).

## Entry points

| Entry point | Import specifier | Contents | Reference |
| --- | --- | --- | --- |
| `.` | `@nimbus/nimbus` | `Nimbus` control-plane client: services, sandboxes, sessions | [Resources](/reference/sdk/resources/) |
| `./server` | `@nimbus/nimbus/server` | Function builders, context types, schema builders, pagination, auth types | [Server functions](/reference/sdk/server/) |
| `./values` | `@nimbus/nimbus/values` | `v` validator namespace, `GenericId`, `Infer`, `Validator` | [Server functions](/reference/sdk/server/) |
| `./browser` | `@nimbus/nimbus/browser` | `NimbusClient`, `NimbusHttpClient`, function references | [Clients](/reference/sdk/client/) |
| `./transports/rest` | `@nimbus/nimbus/transports/rest` | `NimbusRestClient`, `NimbusSubscriptionClient` for the native HTTP/WebSocket API | [Clients](/reference/sdk/client/) |
| `./react` | `@nimbus/nimbus/react` | React providers and hooks | [React](/reference/sdk/react/) |

## Choosing an entry point

- Writing functions that run in a deployment (queries, mutations, actions,
  HTTP actions, schema)? Use `@nimbus/nimbus/server` together with
  `@nimbus/nimbus/values`.
- Calling functions or subscribing to live results from an app? Use
  `@nimbus/nimbus/browser` (or `@nimbus/nimbus/react` in React apps).
- Managing services, sandboxes, or sessions programmatically? Use the root
  entry `@nimbus/nimbus`. See the
  [Agents guides](/agents/) for worked examples.
- Talking to the native admin surface (tenants, documents, schedules, crons)
  directly? Use `@nimbus/nimbus/transports/rest`, or call the
  [native HTTP API](/reference/native/http-api/) yourself.

## Address conventions

Data-plane clients (`NimbusClient`, `NimbusHttpClient`, `NimbusReactClient`)
take a deployment URL of the form `{origin}/convex/{tenant}`, for example
`http://127.0.0.1:8080/convex/demo`. That single prefix owns the function-call
routes (`/query`, `/mutation`, `/action`, …) and the WebSocket endpoint
(`/ws`) used for subscriptions.

The control-plane `Nimbus` client and `NimbusRestClient` take the server
origin itself (for example `http://127.0.0.1:8080`) and address resources
under `/api/...` paths. See the
[native HTTP API reference](/reference/native/http-api/) for the full route
list and the
[WebSocket protocol reference](/reference/native/websocket-protocol/) for the
wire protocol.

## Pages in this reference

- [Server functions](/reference/sdk/server/) — `@nimbus/nimbus/server` and
  `@nimbus/nimbus/values`: function builders, `ctx` surfaces, database reads
  and writes, scheduler, auth, HTTP router, schema, pagination, validators.
- [Clients](/reference/sdk/client/) — `@nimbus/nimbus/browser` and
  `@nimbus/nimbus/transports/rest`: deployment clients, subscriptions,
  connection state, the native REST client.
- [Resources](/reference/sdk/resources/) — the root `@nimbus/nimbus` entry:
  the `Nimbus` client, credential discovery, and the services, sandboxes, and
  sessions APIs.
- [React](/reference/sdk/react/) — `@nimbus/nimbus/react`: providers and
  hooks.

For a guided introduction, start with the
[quickstart](/get-started/quickstart/) or
[your first app](/developers/first-app/).
