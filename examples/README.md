# Nimbus Examples

Runnable apps that show how to build on Nimbus through each of its API surfaces.
Every app imports this repo's workspace packages and runs against the current
build, so an example is both a learning starting point and a build check: a
change that breaks an example breaks it in the same pull request.

Pick the surface you already know — Convex, Firebase/Firestore, MongoDB,
DynamoDB, Cloud Functions, or the native Nimbus SDK — and start from its
directory.

## The run story is the same everywhere

Every example runs the same way. Start the local development server, which
watches your code and serves the app:

```bash
nimbus dev
```

Then deploy the app to a Nimbus resource:

```bash
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server (exactly like `nimbus dev`). The same command deploys to a laptop, a
single server, or a cluster — the resource kind is invisible to the command.

## Surfaces

| Surface | What it is | Examples here | Docs |
| --- | --- | --- | --- |
| Native | The first-party Nimbus SDK (`@nimbus/nimbus`): reactive documents over HTTP writes and live WebSocket subscriptions. | [`nimbus/`](nimbus/) | [Native API](../docs/developers/native/index.md) |
| Convex | Drop-in Convex surface — author functions with `convex/_generated/server`, `convex/values`, and a `convex/schema.ts`; run `convex/react` and `convex/browser` clients unchanged. | [`convex/`](convex/) | [Convex](../docs/developers/convex/index.md) |
| Firebase / Firestore | Stock `firebase/app` + `firebase/firestore` imports over Nimbus's REST, gRPC-Web, and WebSocket `Listen` surfaces. | [`firebase/`](firebase/) | [Firestore](../docs/developers/firebase/index.md) |
| MongoDB | Stock `mongodb` driver against the Nimbus wire-protocol listener via the `@nimbus/mongodb` URI helper. | [`mongodb/`](mongodb/) | [MongoDB](../docs/developers/mongodb/index.md) |
| DynamoDB | Stock AWS SDK client against the Nimbus DynamoDB surface via `@nimbus/dynamodb`. | [`dynamodb/`](dynamodb/) | [DynamoDB](../docs/developers/dynamodb/index.md) |
| Cloud Functions | `firebase-functions/v2` handlers — HTTP/callable endpoints and Firestore triggers with durable retry. | [`cloud-functions/`](cloud-functions/) | [Cloud Functions](../docs/developers/cloud-functions/index.md) |

Each adapter directory has its own `README.md` explaining the surface, listing
its apps, and stating exactly which of the surface's features it supports.

## Application verification

The repository manifest defines nine application cases and 37 smoke
assertions. All nine pass against a real Nimbus binary. The runner uses the
following update semantics:

| Case | Update mode | Meaning |
| --- | --- | --- |
| `nimbus/tasks` | `push` | A WebSocket subscription receives the change. |
| `nimbus/agent-chat` | `push` | A live subscription receives scheduled work. |
| `nimbus/agent-worker` | `push` | A live subscription receives worker progress. |
| `convex/tasks` | `push` | A reactive query receives the change. |
| `convex/runtimes` | `request-response` | The case compares direct function results. |
| `firebase/tasks` | `push` | Firestore `onSnapshot` receives the change. |
| `mongodb/tasks` | `polling` | Repeated reads observe the change. |
| `dynamodb/tasks` | `polling` | Repeated scans observe the change. |
| `cloud-functions/tasks` | `polling` | Repeated reads observe the derived trigger write. |

`push` proves server-delivered change notification. `polling` proves eventual
visibility through repeated reads and does not claim subscription support.
`request-response` has no asynchronous update contract.

Run the complete lane with a supported Node.js version (`>=22 <25`). Nimbus
tests Node.js 22 and 24:

```bash
make examples-verify
NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL=5 make examples-verify
NIMBUS_EXAMPLES_VERIFY_ONLY=convex/tasks make examples-verify
```

The default is one worker for serial diagnosis. Set
`NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL` from 1 through 9 for bounded parallel
work. Each case gets an isolated workspace and operator-state roots. Nimbus
providers assign its ports. The runner fails if source bytes change or cleanup
does not finish.

Each run writes `report.json` and `junit.xml` under
`target/examples-verify-results/<run-id>/`. On failure, stderr names one
retained diagnostic artifact with case logs and cleanup state. The runner
removes credentials before retention. Do not use Git reset, clean, or checkout
commands to recover from a failed test.

Every adapter directory above has a `tasks/` app for its supported subset of
the shared [`tasks` spec](specs/tasks.md). The per-surface demos remain beside
those apps. Each adapter README lists its complete set.

Run these examples in place, from a checkout of this repository. Copy-out
behavior varies by app until the `nimbus init --example` scaffolder ships:
apps importing unpublished workspace packages (`@nimbus/nimbus`,
`@nimbus/mongodb`) fail dependency resolution with a visible install error,
while the Firebase app's stock `firebase` dependency installs the real
upstream SDK from the registry but still expects a Nimbus server. The Convex
examples carry an extra wrinkle: because Nimbus's compatibility package takes
the official `convex` name, a copied-out Convex app's `"convex": "*"` resolves
to the real Convex Cloud package instead, which then breaks at codegen (the
app's `convex codegen --app .` uses a Nimbus-only flag) rather than failing on
an unresolved dependency. Read the warning in
[`convex/README.md`](convex/README.md) before copying one out.

## Shared behavior specs

Apps that implement the same canonical app (for example `tasks`) share one
behavior spec under [`specs/`](specs/): the schema, the flows, and the
observable assertions each adapter's app is checked against. The spec is the
contract. Each adapter directory says which parts of it that surface supports.

## Provisioning caveat

Some browser examples call `POST /api/tenants` from frontend code to create a
tenant on demand. That is convenient for local development but must not ship to
production — pre-provision tenants through the admin API or CLI instead. Each
app's README states how it provisions and what to change before deploying beyond
your machine.
