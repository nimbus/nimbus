# Adapter Contributor Routing

Nimbus exposes several compatible protocol surfaces over one engine, storage
layer, runtime, and trust model. Adapter crates own protocol semantics. Server
modules own transport mounting. They do not create alternate mutation,
persistence, or tenant-authority paths.

## Routing

| Surface | Protocol owner | Transport or integration owner | Contributor guidance |
| --- | --- | --- | --- |
| Convex | `crates/nimbus-convex/`, `packages/convex/` | `crates/nimbus-server/src/adapters/`, `crates/nimbus-bridge/` | [`convex/ai-guidelines.md`](convex/ai-guidelines.md) |
| Firebase and Firestore | `crates/nimbus-firebase/`, `packages/firebase/` | `crates/nimbus-server/src/adapters/` | [`../../developers/firebase/`](../../developers/firebase/) |
| Cloud Functions | `crates/nimbus-cloud-functions/` | `crates/nimbus-server/src/adapters/`, `crates/nimbus-bridge/` | [`../operating/cloudflare-adapters.md`](../operating/cloudflare-adapters.md) for Cloudflare deployment work |
| MongoDB | `crates/nimbus-mongodb/`, `packages/mongodb/` | Dedicated listener composition outside the protocol crate | [`../../developers/mongodb/`](../../developers/mongodb/) |
| DynamoDB | `crates/nimbus-dynamodb/`, `packages/dynamodb/` | Dedicated listener composition outside the protocol crate | [`../../developers/dynamodb/`](../../developers/dynamodb/) |
| Nimbus-native APIs | `packages/nimbus/` and engine-facing Rust seams | `crates/nimbus-server/` | [`../../reference/sdk/`](../../reference/sdk/) |

The public compatibility truth is in
[`../../reference/adapter-capabilities.md`](../../reference/adapter-capabilities.md)
and each surface's reference pages. Update
[`../../source-map.md`](../../source-map.md) when a public behavior claim
changes.

## Shared invariants

- Every client document mutation uses the engine-owned queued journal, direct,
  or execution-unit path. Do not add a fourth route.
- Document, index, and commit-log effects remain one storage transaction.
- Tenant and application identity come from trusted server context. A request
  field, URL segment, issuer, or subject is not authority by itself.
- `nimbus-runtime` keeps zero workspace dependencies. Nimbus-specific host-call
  integration stays in `nimbus-bridge` and its composition roots.
- Protocol crates do not bind listeners. Transport effects stay in the server
  or dedicated listener owner.
- Compatibility claims must have source and behavioral-test evidence. Do not
  infer parity from matching names or types.

For a cross-adapter change, identify the protocol owner, transport adapter,
engine path, authentication boundary, and public compatibility statement
before editing.
