---
title: SDK and packages
description: How the npm side of the Nimbus monorepo is organized — the canonical JavaScript SDK, the Convex compatibility wrapper, codegen, the embedded admin UI, and the binary-owned distribution model.
sidebar:
  label: SDK & packages
  order: 12
---

The Nimbus repository is two ecosystems in one tree: a Rust workspace under
`crates/` that builds the server binary, and an npm monorepo under
`packages/` that holds everything JavaScript — the SDK your app imports,
the compatibility wrappers, the code generator, and the admin UI. This page
explains how the JavaScript side is structured and why none of it ships
through the public npm registry.

## Two things named "nimbus"

The name appears on both sides of the repo, and they are different
artifacts:

- **`packages/nimbus`** is the JavaScript SDK — the npm package
  `@nimbus/nimbus` that application code imports.
- **`crates/nimbus`** is the Rust facade crate for embedders. Its
  `crates/nimbus/src/lib.rs` re-exports the stable high-level surface of
  the internal workspace crates (core types, the engine, the runtime
  contract) so a Rust program embedding Nimbus depends on one crate
  instead of several internal ones.

When this documentation says "the SDK" it always means the JavaScript
package. The Rust facade is an embedding surface, not a client library.

## The package map

The npm workspace (declared in the root `package.json`) contains seven
packages:

| Package | npm name | Role |
| --- | --- | --- |
| `packages/nimbus` | `@nimbus/nimbus` | Canonical JavaScript SDK |
| `packages/convex` | `convex` | Convex compatibility wrapper |
| `packages/codegen` | `@nimbus/codegen` | Code generator behind `nimbus codegen` |
| `packages/nimbus-ui` | `nimbus-ui` | Admin UI embedded into the server binary |
| `packages/firebase` | `firebase` | Drop-in Firebase app + Firestore client (stock npm name) |
| `packages/mongodb` | `@nimbus/mongodb` | Connection-string helper for the MongoDB surface |
| `packages/dynamodb` | `@nimbus/dynamodb` | Client-config helper for the DynamoDB surface |

Every one of them is marked `"private": true`. None is published to the
public npm registry — see [the distribution model](#packages-ship-inside-the-binary)
below for how they reach applications instead.

## The canonical SDK: `packages/nimbus`

`@nimbus/nimbus` is the single canonical JavaScript implementation. It is
ESM-only, ships TypeScript sources directly, and exposes six entry points
(the `exports` map in `packages/nimbus/package.json`):

| Entry point | What it owns |
| --- | --- |
| `@nimbus/nimbus` | The `Nimbus` root client — the control-plane resources client for services, sandboxes, and sessions |
| `@nimbus/nimbus/server` | Function builders (`query`, `mutation`, `action`, `httpAction`, …), `defineSchema`/`defineTable`, HTTP router |
| `@nimbus/nimbus/values` | Value and validator types shared between server and clients |
| `@nimbus/nimbus/browser` | Data-plane clients: `NimbusClient` (WebSocket), `NimbusHttpClient`, `NimbusReactClient` |
| `@nimbus/nimbus/transports/rest` | The REST transport (`NimbusRestClient`) used by the root client |
| `@nimbus/nimbus/react` | React bindings: `NimbusProvider`, `useQuery`, `useMutation`, `usePaginatedQuery`, … |

`react` and `react-dom` are optional peer dependencies, needed only for
the `/react` entry point. The full API surface is documented in the
[SDK reference](/reference/sdk/); this page only places the package in the
architecture.

## The Convex wrapper: `packages/convex`

The repo's rule is that `packages/nimbus` is the canonical implementation
and `packages/convex` stays a thin compatibility layer. Reading its
sources shows what "thin" means in practice:

- `packages/convex/src/server.ts` imports the canonical builders from
  `@nimbus/nimbus/server` (`query`, `mutation`, `defineSchema`, …) and
  re-exports them under the Convex API names and types.
- `packages/convex/src/react.ts` builds Convex's React surface
  (`ConvexProvider`, `useQuery`, …) directly on `NimbusProvider` and the
  `@nimbus/nimbus/react` hooks.
- `packages/convex/src/browser.ts` wraps the `NimbusClient`,
  `NimbusHttpClient`, and `NimbusReactClient` classes from
  `@nimbus/nimbus/browser` behind Convex client names such as
  `ConvexReactClient`.
- `packages/convex/src/values.ts` is the one place with local logic: it
  implements Convex's `v.*` validator vocabulary as lightweight validator
  descriptors rather than re-exporting, because the shapes are
  Convex-specific.

The package also carries a small `convex` CLI shim
(`packages/convex/src/cli.mjs`) whose `codegen` subcommand delegates to
`@nimbus/codegen` — so `npx convex codegen` works inside a Nimbus app.

## Code generation: `packages/codegen`

`@nimbus/codegen` is the generator invoked by `nimbus codegen` (and the
`convex` shim above). For an app it produces two layers of artifacts
(`packages/codegen/src/main.mjs`):

- **Developer-facing, in the function source root's `_generated/`
  directory:** `api.ts` (typed function references), `server.ts`
  (re-export shims for the builders), `scheduled_functions.ts`, and
  `dataModel.d.ts` (types derived from the schema definition).
- **Server-facing, under `.nimbus/convex/`:** the function manifest
  (`functions.json`), `schema.json`, `http_routes.json`,
  `auth.config.json`, and the runtime bundle `bundle.mjs` with its
  `bundle.sha256` — the integrity hash the server checks before every
  invocation.

How those artifacts flow into a deployment is covered in
[CLI and codegen](/concepts/architecture/cli-codegen/).

## The admin UI: `packages/nimbus-ui`

`nimbus-ui` is a Vite-built React single-page app. It is not deployed
anywhere — it is compiled into the server binary:

- `crates/nimbus-assets/src/ui.rs` embeds the entire
  `packages/nimbus-ui/dist/` directory into the binary with `rust-embed`,
  alongside a small static auth page kept under
  `crates/nimbus-assets/embedded/ui-auth/`.
- `crates/nimbus-assets/build.rs` fails the build with an actionable
  message if the UI feature is enabled but `dist/index.html` has not been
  built, so a binary can never silently ship without its UI.
- `crates/nimbus-server` depends on `nimbus-assets` with the `ui` feature
  and serves the embedded assets at `/ui` from
  `crates/nimbus-server/src/http/ui.rs`, falling back to the embedded
  `index.html` for SPA routes.

## Adapter packages

Three packages support the protocol adapters on the client side:

- `firebase` is a drop-in implementation of the modular `firebase/app` +
  `firebase/firestore` API — an `initializeApp` surface
  (`packages/firebase/src/app.ts`) and a Firestore client
  (`packages/firebase/src/firestore.ts`) speaking the Firestore wire
  protocol via Connect-Web generated protobuf code. Like `convex`, it
  takes the stock npm name, so existing imports work unchanged once the
  app's `firebase` dependency points at the provisioned copy —
  `nimbus packages provision firebase` rewires the `package.json`
  dependency to `file:./.nimbus/packages/firebase` itself.
- `@nimbus/mongodb` exports a single `mongoUri` helper that builds a
  connection string pointed at a Nimbus tenant, for use with the stock
  `mongodb` driver.
- `@nimbus/dynamodb` exports `clientConfig` and `endpoint` helpers that
  configure the stock AWS SDK client against a Nimbus endpoint.

For MongoDB and DynamoDB the official driver or SDK does the real work and
the helper only points it at Nimbus; the `firebase` package is a full
client because Nimbus's Firestore surface replaces the upstream transport
stack.

## Packages ship inside the binary

Nimbus packages are not on the public npm registry. They travel inside the
server binary and are materialized into each app:

```text
packages/*/dist
      │  staged with per-file SHA-256 manifest
      ▼
crates/nimbus-assets/embedded/packages/   (rust-embed, version-locked)
      │  nimbus init / dev / codegen / deploy
      ▼
<app>/.nimbus/packages/<name>/  +  .version stamp
      ▲
"convex": "file:./.nimbus/packages/convex"   (scaffolded or auto-wired package.json)
```

- A staging script copies each built package — plus three co-provisioned
  third-party ESM roots (`@bufbuild/protobuf`, `@connectrpc/connect`,
  `@connectrpc/connect-web`) — under `crates/nimbus-assets/embedded/packages/`
  with a checksummed `manifest.json`, which
  `crates/nimbus-assets/src/js_packages.rs` embeds into the binary.
- `crates/nimbus-cli/src/provision.rs` writes the embedded payload into
  `<app>/.nimbus/packages/`, resolving the dependency closure for the
  selected adapter, and stamps `.nimbus/packages/.version` with the
  manifest digest only after every file lands — so an interrupted run is
  rewritten on the next call.
- Scaffolds reference the payload with `file:` specifiers (for example
  `"convex": "file:./.nimbus/packages/convex"` in the embedded init
  template), so `npm install` resolves entirely offline. For an existing
  app, adapter provisioning writes the same specifier into the app's
  `package.json` itself — replacing a registry spec — so migrating needs no
  manual dependency edit.
- On a binary upgrade the stamp drifts, provisioning rewrites the payload,
  and the installed copies are invalidated so Node reinstalls from the
  fresh bytes. `nimbus packages verify` re-checks every provisioned file
  against the embedded checksums.

The codegen tooling itself (a prebundled `@nimbus/codegen` plus esbuild)
is embedded the same way but is *not* provisioned into apps — it is
materialized to a temporary directory when the binary runs codegen.

The net effect: the SDK version is a property of the binary, not of a
registry lookup. A given `nimbus` binary always provisions exactly the
package bytes it was built with, verified by hash.

## Where to go next

- [SDK reference](/reference/sdk/) — entry points, classes, and hooks in detail.
- [CLI and codegen](/concepts/architecture/cli-codegen/) — how generated artifacts become a deployment.
- [Adapters](/concepts/architecture/adapters/) — the server-side protocol surfaces these packages talk to.
- [Developers guide](/developers/) — task-oriented guides per surface.
