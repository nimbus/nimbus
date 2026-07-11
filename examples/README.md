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
| DynamoDB | Stock AWS SDK client against the Nimbus DynamoDB surface via `@nimbus/dynamodb`. | _coming soon_ | [DynamoDB](../docs/developers/dynamodb/index.md) |
| Cloud Functions | `firebase-functions/v2` handlers — HTTP/callable endpoints and Firestore triggers with durable retry. | _coming soon_ | [Cloud Functions](../docs/developers/cloud-functions/index.md) |

Each adapter directory has its own `README.md` explaining the surface, listing
its apps, and stating exactly which of the surface's features it supports.

**What exists today vs. what is still being built.** The linked directories above
hold per-surface demos that run against the current build today — the Convex
apps operate on a `messages` collection, the native and Firestore apps on their
own shapes, and so on. The shared `tasks` example that the [specs](specs/tasks.md)
describe — one `tasks` app per surface plus a smoke script that asserts the
spec's flow anchors — is not built yet. Until it lands, each adapter README's
`tasks` support table describes the target subset that app will meet, not
current coverage, and the DynamoDB and Cloud Functions directories (marked
_coming soon_) do not exist yet.

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
contract; each adapter directory says which parts of it that surface supports.

## Provisioning caveat

Some browser examples call `POST /api/tenants` from frontend code to create a
tenant on demand. That is convenient for local development but must not ship to
production — pre-provision tenants through the admin API or CLI instead. Each
app's README states how it provisions and what to change before deploying beyond
your machine.
