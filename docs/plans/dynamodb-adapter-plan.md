# DynamoDB Adapter Plan

Canonical execution plan for an Amazon DynamoDB wire-protocol compatibility
adapter: HTTP/JSON listener on a Nimbus-owned port, AttributeValue codec,
DynamoDB operation dispatch (data plane, control plane, streams), an
AWS-SDK-shaped JavaScript helper package, and parity-test coverage against
ExtendDB plus DynamoDB Local.

This plan follows the architecture established by the extracted adapter crates:
each concrete provider adapter lives in a `nimbus-<provider>` crate, adapters
own protocol translation, `nimbus-server` owns transport composition, and
Nimbus core owns data primitives. DynamoDB therefore targets
`crates/nimbus-dynamodb`, not a `*-adapter` suffixed crate and not another
server-local module. Where DynamoDB needs shared behavior that overlaps with
Convex, Firebase, or MongoDB, promote that behavior into a protocol-neutral
Nimbus primitive before adding adapter-local copies.

## Context

Nimbus already ships four compatibility adapters:

- **Convex (`nimbus-convex`)** — deep function runtime, WebSocket
  subscriptions, V8 host bridge.
- **Firebase (`nimbus-firebase`)** — Firestore data API, gRPC/REST/WebSocket,
  reactive Listen streams.
- **Cloud Functions (`nimbus-cloud-functions`)** — document triggers,
  HTTP/callable handlers, Firebase v2 and standalone Functions Framework
  authoring surfaces.
- **MongoDB (`nimbus-mongodb`)** — binary OP_MSG wire protocol, BSON codec,
  CRUD/cursor/index/collection/aggregation/transaction/change-stream commands,
  SCRAM-SHA-256 auth, `@nimbus/mongodb` driver package, and
  unified-test-format spec runner.

DynamoDB is closest in shape to the MongoDB adapter: a separate listener
exposing a wire protocol on its own port, an item-oriented data model, and a
typed-value bridge that must roundtrip through Nimbus's JSON document storage.
The differences are surface-level — HTTP/JSON rather than binary OP_MSG, AWS
SigV4 rather than SCRAM-SHA-256, expression-language rather than operator
documents — but the Nimbus-side boundary (engine + storage + shared
primitives) is unchanged.

### Why DynamoDB

DynamoDB is one of the most widely deployed managed NoSQL services on the
public cloud. AWS itself shipped `ExtendDB` v0.1 on 2026-05-20 as an
Apache-2.0 reference adapter that implements the DynamoDB API against
pluggable storage backends; the existence of an AWS-blessed open adapter
confirms portable DynamoDB compatibility is a recognized need. Nimbus's value
proposition over ExtendDB is the same one the MongoDB adapter proves: the
same data is reachable from every adapter — a record written via a Convex
mutation, a Firestore set, or a Mongo `insertOne` is queryable via DynamoDB
`Query` against the same Nimbus storage instance. One backend, one ops story,
one DR story, one auth model, N protocols.

### Open Source Resources

| Resource | URL | Local Path | Purpose |
|----------|-----|-----------|---------|
| ExtendDB | `https://github.com/ExtendDB/extenddb` (Apache-2.0) | `/Users/jack/src/github.com/ExtendDB/extenddb` (cloned 2026-05-26) | AWS-maintained DynamoDB-compatible adapter. Reference implementation, parity-test target, divergence catalogue (`docs/differences-from-dynamodb.md`). **Source of `extenddb-core` direct dependency** (AttributeValue, expression language, type model, validation, error taxonomy) and **source of vendored SigV4 module** (4 files from `crates/auth/src/sigv4/`). See "Upstream Crate Reuse" below for the per-crate decision matrix. |
| AWS DynamoDB Local | `amazon/dynamodb-local` Docker image (Java JAR) | — | Behavioral ground truth for DynamoDB API semantics |
| `aws-sdk-rust` (DynamoDB client) | `https://github.com/awslabs/aws-sdk-rust` | clone on demand under `~/src/github.com/awslabs/aws-sdk-rust` | Canonical Rust client; useful for SigV4, request shapes, response envelopes |
| `aws-sdk-js-v3` (DynamoDB client) | `https://github.com/aws/aws-sdk-js-v3` | clone on demand under `~/src/github.com/aws/aws-sdk-js-v3` | Canonical JS client; primary SDK target for `@nimbus/dynamodb` |
| `boto3` / `botocore` | `https://github.com/boto/boto3` | clone on demand | Python SDK; secondary parity target |
| moto (DynamoDB) | `https://github.com/getmoto/moto` (Apache-2.0) | clone on demand under `~/src/github.com/getmoto/moto` | Python in-memory DynamoDB mock; behavioral reference for edge cases |
| `aws-sigv4` (Rust crate) | crates.io | — | Reference only. Not a planned dependency because the focused ExtendDB server-side SigV4 module is vendored instead. |

ExtendDB is already cloned at `/Users/jack/src/github.com/ExtendDB/extenddb`
(top-level wrapper directory `/Users/jack/src/github.com/ExtendDB/` contains
the repo as `extenddb/`). Read the local checkout for canonical behavior
rather than fetching documentation snippets from the URL. Especially useful
local paths inside the clone:

- `docs/differences-from-dynamodb.md` — divergence catalogue against real
  DynamoDB; every divergence entry maps to a classification decision in
  Nimbus's parity-test runner.
- `docs/dynamodb-limits.md` — sizes, counts, and rate limits.
- `docs/adr/` and `docs/rfcs/` — architecture decisions and design notes.
- `crates/server/` — HTTP listener, X-Amz-Target dispatch, request envelope
  handling — closest structural twin to what `crates/nimbus-dynamodb/src/`
  must implement.
- `crates/auth/` — SigV4 verification scaffold; reusable canonical-request
  construction shape.
- `crates/core/` — AttributeValue type definitions and expression handling.
- `tests/` — Python and Rust parity-test corpus (`test_item_operations.py`,
  `test_query_scan.py`, `test_batch_operations.py`,
  `test_conditional_writes.py`, `test_transaction_operations.py`,
  `test_streams.py`, `test_ttl.py`, `test_multipart_gsi.py`,
  `test_auth_integration.py`, etc.). Scenarios here are excellent
  parity-test seeds for `crates/nimbus-dynamodb/tests/dynamodb_spec/`.

#### License posture

ExtendDB is Apache-2.0. Apache-2.0 is permissive and compatible with the
Nimbus Community License 1.0 — incorporating ExtendDB source into Nimbus is
allowed. Compliance is mechanical, not gated on a review:

- Preserve the Apache-2.0 license header at the top of any file copied (or
  derived heavily) from ExtendDB.
- Add ExtendDB to the repo `NOTICE` file (create one at repo root if absent)
  with the upstream copyright line and a link back to the source.
- Mark material modifications inside copied files per Apache-2.0 §4(b)
  (a brief "Modified from ExtendDB by Nimbus contributors, YYYY-MM-DD"
  banner is sufficient).
- The Apache-2.0 patent grant flows through; no additional Nimbus-side
  patent paperwork is required.
- The combined binary ships under the Nimbus Community License 1.0; the
  Apache-2.0 files retain their original license. This is the standard
  multi-licensed-project pattern (it is how almost every Rust workspace
  carries vendored MIT/Apache dependencies).

Copy-versus-reimplement is therefore an engineering decision, not a legal
one. Prefer copy when the upstream code is small, focused, and would
otherwise be reimplemented identically (e.g., AttributeValue type encoding,
SigV4 canonical-request construction, expression-language lexer). Prefer
reimplement when the upstream module is entangled with PostgreSQL specifics
that do not apply to Nimbus's storage layer (e.g., ExtendDB's `storage-postgres`
crate), or when the Nimbus seam shape is substantially different from
ExtendDB's.

Clone the AWS SDK and moto repos locally on demand during the foundation and
hardening phases when their source is needed in-flight.

## Status

- **Plan status:** `pending`
- **Control item:** `none`
- **Status values:** `pending`, `in_progress`, `done`, `blocked`
- **Primary source of truth:** this file plus the current git worktree.
- **Checkpoint rule:** every work session that changes implementation state
  must update the roadmap item status, the phase status ledger, and the
  execution log before stopping.

Promote this plan from `pending` to `in_progress` only after the current
server crate extraction completion gates remain green and the release-readiness
plan does not require freezing the adapter surface.

## Plan Ownership And Canonical Inputs

This plan owns the implementation of DynamoDB wire-protocol compatibility and
any Nimbus primitive promotion the work requires.

Implementation work must keep these source inputs open:

- Top-level repo references: `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
  and `docs/plans/README.md`.
- Latest completed adapter baselines:
  - `docs/plans/archive/mongodb-adapter-plan.md` and
    `docs/plans/archive/mongodb-adapter-hardening-plan.md` — closest
    structural precedent; reuse the listener/dispatch/bridge shape.
  - `docs/plans/archive/runtime-capability-adapter-boundary-plan.md` —
    adapter/runtime ownership baseline; DynamoDB work must not duplicate
    provider-specific leakage patterns corrected there.
  - `docs/plans/archive/multi-adapter-boundary-hardening-plan.md` — earlier
    completed cross-adapter hardening wave.
- DynamoDB protocol sources: AWS API reference, DynamoDB JSON wire format,
  AttributeValue encoding, expression-language grammar, SigV4 spec, DynamoDB
  Streams record format, ExtendDB `docs/differences-from-dynamodb.md`.
- Nimbus seam sources: core types/mutations/query, engine execution units and
  subscriptions, server router/state/security, the extracted adapter crates
  (`crates/nimbus-{convex,firebase,cloud-functions,mongodb}/`), and the thin
  server composition shims under `crates/nimbus-server/src/adapters/`.
- Test evidence: AWS SDK acceptance tests, DynamoDB Local Java binary,
  ExtendDB integration tests, moto DynamoDB tests.

## Current Assessed State

- Nimbus's document model (schemaless JSON documents in named tables with
  optional schema, indexed fields, and reactive subscriptions) maps naturally
  to DynamoDB's item model (typed attribute-value documents in named tables).
- The shared atomic write batch primitive used by the Firebase and MongoDB
  adapters supports set/patch/delete with conditional verification, which
  covers DynamoDB `PutItem`/`UpdateItem`/`DeleteItem`/`TransactWriteItems`
  semantics.
- The structured query AST used by the MongoDB adapter supports filters,
  ordering, cursors, offsets, limits, and projections — covering most of
  DynamoDB's `Query` and `Scan` surface.
- The subscription snapshot/diff infrastructure that backs MongoDB change
  streams can back DynamoDB Streams with protocol-level shard wrapping.
- The transaction session manager provides cross-RPC transaction tokens
  compatible with DynamoDB `TransactWriteItems`/`TransactGetItems`.
- The MongoDB adapter already proved the typed-scalar metadata pattern for
  preserving non-JSON value types (Binary, Decimal128, etc.); reuse the same
  infrastructure for DynamoDB Binary (`B`), Number-with-precision (`N` as
  arbitrary-precision string), and Set types (`SS`/`NS`/`BS`).
- The MongoDB adapter already proved the pattern of a sibling TCP listener
  alongside the axum HTTP server sharing the same `Arc<Service>` instance.
  DynamoDB needs an axum HTTP listener on a separate port — even simpler than
  the MongoDB raw-TCP listener.
- There is no AWS SigV4 verification in the codebase. SigV4 is a standalone
  canonical-request-plus-derived-key signing scheme; this plan vendors the
  focused ExtendDB SigV4 verification module into `nimbus-dynamodb` instead of
  adding a separate `aws-sigv4` dependency.
- DynamoDB's composite primary key (partition key + optional sort key) is
  richer than Nimbus's single `DocumentId` string. A canonical reversible
  encoding (e.g., `pk\x00sk` with size validation) is required.

## Autonomous Execution Contract

This plan is designed for agent-driven execution with minimal human
intervention. Each roadmap item must be completable in a single context
window using only the plan, the git worktree, and the cloned reference repos.

### Startup Prompt

To be authored at promotion time as `docs/prompts/dynamodb-adapter-start.md`,
modeled on the MongoDB startup prompt.

### Upstream Crate Reuse

ExtendDB is a clean-room Rust workspace under Apache-2.0. Significant pieces
of it are directly reusable — especially the protocol-neutral core types and
expression language, which would otherwise cost weeks to reimplement and
where divergence from AWS in obscure corners (operator precedence, function
handling, reserved-word collisions, error-string trailing whitespace) is
exactly what trips parity tests.

The reuse decisions, by crate:

| ExtendDB crate | LOC | Decision | Rationale |
|----------------|-----|----------|-----------|
| `extenddb-core` | 12,587 | **Depend directly (git rev pin)** | Zero workspace deps, pure sync, no I/O. Ships `AttributeValue` with serde wire-format impls, the full expression language (~4,500 LOC across tokenizer, parser, evaluator, update parser/evaluator, key condition, projection, resolver, 615-LOC reserved-word catalogue), complete typed I/O envelopes (`PutItemInput/Output`, `Query`, `Scan`, `BatchWriteItem`, `TransactWriteItems`, `StreamRecord`), `DynamoDbError` taxonomy with AWS-fidelity error strings, 1,095-LOC `validation/mod.rs`, limits and throttle types. All dependencies (serde, serde_json, thiserror, base64, bigdecimal, time, uuid) compatible with Nimbus's pinned versions. |
| `extenddb-auth/sigv4/` | ~760 | **Vendor the 4 source files** | `canonical.rs` (172), `parse.rs` (147), `signing_key.rs` (99), `verify.rs` (342). Self-contained except for `axum::http::HeaderMap` (already a Nimbus dep) and `extenddb_core::error::DynamoDbError`. Preserve Apache-2.0 header per file; add ExtendDB to repo `NOTICE`. Vendoring avoids dragging in the policy module's 2,700 LOC of IAM that Nimbus replaces with its own auth model. |
| `extenddb-auth/policy/` | 2,693 | **Skip** | IAM policy engine (statements, principals, condition operators). Nimbus has its own tenant/principal model. |
| `extenddb-storage` | 2,271 | **Reference only** | 6 RPITIT traits (`TableEngine`, `DataEngine`, `MetadataEngine`, `StreamEngine`, `WorkerStore`, `BackupEngine`), all `account_id`-scoped. Useful as a shape reference for what an item-store interface looks like; does not map onto Nimbus's `Service`. |
| `extenddb-engine` | 6,730 | **Reimplement against `Service`** | Every handler takes `OperationContext { storage: Arc<dyn extenddb_storage::StorageEngine> }`. Reusing the handlers would require Nimbus storage to implement the full 6-trait ExtendDB surface — more work than reimplementing the handlers, which are mostly validation + dispatch + serialize. The heavy lifting (expression evaluation, validation, AttributeValue codec) is in `extenddb-core` and is reused via that crate. |
| `extenddb-storage-postgres` | ~6,000 | **Skip** | PostgreSQL-specific. |
| `extenddb-server` | 8,332 | **Skip** | Includes axum listener, management API (`server/src/management/*` IAM CRUD), 2,500-LOC web console. The X-Amz-Target dispatch in `handler.rs` (340 LOC) is the only reusable shape and is small enough to write fresh against Nimbus's `Service`. |
| `extenddb` (bin) | — | **Skip** | ExtendDB CLI. |

#### Dependency wiring (D0.0)

Add to the Nimbus workspace `Cargo.toml`:

```toml
[workspace]
members = [
    "crates/nimbus-dynamodb",
    # ...
]

[workspace.dependencies]
extenddb-core = { git = "https://github.com/ExtendDB/extenddb", rev = "<pin-at-D0.0>" }
bigdecimal = { version = "0.4", features = ["serde"] }   # new transitive dep
```

Cargo resolves workspace members of git dependencies by package name; pointing
at the repo URL with a `rev` pin fetches the entire ExtendDB workspace and
selects `extenddb-core` from it. Set the pin to the ExtendDB HEAD commit at
the moment D0.0 runs and record the sha in the execution log.

In `crates/nimbus-dynamodb/Cargo.toml`:

```toml
[package]
name = "nimbus-dynamodb"

[dependencies]
extenddb-core = { workspace = true }
bigdecimal = { workspace = true }
nimbus-core = { path = "../nimbus-core" }
nimbus-engine = { path = "../nimbus-engine" }
nimbus-tenant = { path = "../nimbus-tenant" }
```

In `crates/nimbus-server/Cargo.toml`, depend only on the concrete adapter
crate:

```toml
[dependencies]
nimbus-dynamodb = { path = "../nimbus-dynamodb" }
```

Do not add `extenddb-core` or DynamoDB protocol dependencies to
`nimbus-server`; those remain owned by `nimbus-dynamodb`.

Pin upgrade cadence: bump on a quarterly schedule or when a needed fix lands
upstream. Carry local patches as a vendored fork (Option C below) only if a
needed change is rejected upstream.

#### Vendoring SigV4 (D0.8)

Copy `crates/auth/src/sigv4/{canonical.rs,parse.rs,signing_key.rs,verify.rs}`
into `crates/nimbus-dynamodb/src/auth/sigv4/` verbatim. Keep
the Apache-2.0 header at the top of every copied file. Add a one-line
"Modified from ExtendDB by Nimbus contributors, YYYY-MM-DD" banner if any
file is modified. Update repo-root `NOTICE` with one entry covering all
vendored files.

#### Fallback (vendor-everything path)

If upstream stability or pin churn becomes a problem, switch to a local
vendored copy of `extenddb-core` inside `crates/nimbus-dynamodb/src/extenddb_core/`.
Same compliance steps: Apache-2.0 headers preserved, modification banner added,
NOTICE updated. Keep the vendored copy inside the concrete adapter crate unless
a later multi-adapter need justifies a separate crate. Do this only if Option A
demonstrates real maintenance pain — the git rev pin path is cleaner.

### Module Structure

The DynamoDB adapter lives in the concrete `crates/nimbus-dynamodb` crate with
the following initial file layout (created during D0.1). Modules labeled
"[uses extenddb-core]" delegate parsing/validation/evaluation to the upstream
crate and own only the Nimbus-side bridging (storage I/O, tenant resolution,
error mapping back to the adapter envelope). The crate must not depend on
`nimbus-server`, must not accept `AppState`, and must expose narrow router or
operation entrypoints over explicit capabilities such as `Arc<Service>`.

```
crates/nimbus-dynamodb/
├── Cargo.toml            # package name: nimbus-dynamodb
├── src/
│   ├── lib.rs            # DynamoDbConfig, public API, router factory exports
│   ├── router.rs         # axum Router factory; no socket bind or spawn lifecycle
│   ├── wire.rs           # JSON request/response envelope, X-Amz-Target parsing, error envelope [uses extenddb-core error taxonomy]
│   ├── attribute_value.rs # extenddb_core::types::AttributeValue ↔ Nimbus value bridge (S/N/B/M/L/SS/NS/BS/BOOL/NULL roundtrip via typed-scalar metadata)
│   ├── key.rs            # partition+sort composite-key encoding, DocumentId mapping
│   ├── error.rs          # DynamoDbError → Nimbus error taxonomy mapping (both directions)
│   ├── auth/
│   │   ├── mod.rs        # AuthProvider entry, access-key → tenant resolution
│   │   └── sigv4/        # vendored from extenddb-auth/sigv4/ — see Upstream Crate Reuse
│   │       ├── canonical.rs
│   │       ├── parse.rs
│   │       ├── signing_key.rs
│   │       └── verify.rs
│   ├── expression.rs     # thin shim: extenddb_core::expression::{parser,evaluator,update_evaluator,key_condition,projection,resolver} wired to Nimbus document values
│   ├── commands/
│   │   ├── mod.rs        # X-Amz-Target → handler dispatch table
│   │   ├── control_plane.rs # CreateTable, DescribeTable, ListTables, UpdateTable, DeleteTable, DescribeEndpoints, DescribeLimits
│   │   ├── item.rs       # PutItem, GetItem, UpdateItem, DeleteItem [uses extenddb-core input/output types + validation]
│   │   ├── query_scan.rs # Query, Scan (with pagination, parallel-scan segments) [uses extenddb-core]
│   │   ├── batch.rs      # BatchGetItem, BatchWriteItem [uses extenddb-core]
│   │   ├── transact.rs   # TransactGetItems, TransactWriteItems [uses extenddb-core]
│   │   ├── index.rs      # GSI/LSI definition + index-targeted dispatch helpers
│   │   ├── streams.rs    # DescribeStream, GetShardIterator, GetRecords, ListStreams [uses extenddb-core stream types]
│   │   ├── ttl.rs        # UpdateTimeToLive, DescribeTimeToLive
│   │   └── tagging.rs    # TagResource, UntagResource, ListTagsOfResource
│   ├── streams/
│   │   ├── mod.rs        # stream registry, shard model
│   │   ├── shard.rs      # shard iterator format, sequence numbers, record mapping
│   │   └── record.rs     # StreamRecord (KEYS_ONLY/NEW_IMAGE/OLD_IMAGE/NEW_AND_OLD_IMAGES)
│   └── tenant.rs         # access-key → tenant resolution, table-name → Nimbus-table mapping
└── tests/
    └── dynamodb_spec/    # parity runner and SDK-shaped scenarios
```

Files may be split further per the modularity thresholds in `AGENTS.md`
(1500-line soft limit, 2000-line hard limit). New sub-modules should follow
concept-owned naming. The `expression.rs` shim is expected to stay small
(under 500 LOC) because all the heavy work is in `extenddb-core::expression`.

`crates/nimbus-server/src/adapters/dynamodb/` may exist only as a thin
composition shim if useful. It can re-export `DynamoDbConfig`, call
`nimbus_dynamodb::build_router(...)`, and own bind/spawn/shutdown plumbing. It
must not contain DynamoDB protocol parsing, AttributeValue conversion,
expression evaluation, SigV4 verification, operation dispatch, or parity-test
logic.

### Boot Sequence Integration

The DynamoDB HTTP listener integrates into the server startup in
`crates/nimbus-server/src/lib.rs`:

1. Add `dynamodb_config: Option<nimbus_dynamodb::DynamoDbConfig>` to
   `ServeOptions` and a `.with_dynamodb(config)` fluent builder method.
2. In `serve_with_options`, if `dynamodb_config` is `Some`, bind a separate
   `tokio::net::TcpListener` on the configured port (default 8000, matching
   DynamoDB Local) and spawn an axum HTTP server with the
   `nimbus-dynamodb` router as a sibling `tokio::spawn` task sharing the same
   `Arc<Service>` instance.
3. In `crates/nimbus-bin/src/start/boot.rs`, add a `--dynamodb-port` CLI flag
   (default: disabled) that creates a `DynamoDbConfig` and passes it to
   `ServeOptions::with_dynamodb`.
4. Add a `dynamodb` optional feature to `crates/nimbus-adapters` only after
   the concrete `nimbus-dynamodb` crate compiles and has focused tests. The
   facade remains default-empty, and `nimbus-server` must continue to depend
   directly on `nimbus-dynamodb`, not on `nimbus-adapters`.

### Dependency Management

The following crates are new or version-bumped workspace dependencies:

- `extenddb-core` (git rev pin against `https://github.com/ExtendDB/extenddb`)
  — DynamoDB types, expression language, validation, error taxonomy.
  Apache-2.0. See "Upstream Crate Reuse" for the per-crate decision matrix.
- `bigdecimal` 0.4 — required by `extenddb-core` for arbitrary-precision
  numbers. MIT/Apache-2.0.
- `uuid` 1 — already a `extenddb-core` transitive dep; check whether Nimbus
  pulls it elsewhere and align features.
- `hmac` 0.12, `sha2` 0.10, `hex` 0.4 — SigV4 signing primitives required by
  the vendored sigv4 module. MIT/Apache-2.0.
- `axum` extractors for raw JSON body + headers (already present).
- `base64` 0.22 (already present) — Binary attribute encoding.
- `serde_json` (already present) — JSON envelope.

The plan does **not** depend on `aws-sigv4`. The vendored ExtendDB sigv4
module covers verification (the server side); `aws-sigv4` is primarily a
client-side signing crate and would be redundant given the vendored module.

Run `make deny` after adding dependencies to confirm no license or advisory
violations. All new deps are Apache-2.0 / MIT, compatible with the Nimbus
Community License 1.0.

### Spec And Parity Test Vendoring

DynamoDB does not publish a public canonical conformance suite. The plan uses
five evidence sources, with explicit authority levels:

1. **AWS DynamoDB Local** (Java JAR). Distributed by AWS; run as a Docker
   image (`amazon/dynamodb-local`) or a downloaded JAR. Behavior is treated
   as authoritative for ambiguous semantics.
2. **Official AWS client SDKs and AWS CLI.** Run real clients against the
   Nimbus endpoint, not only hand-built JSON requests:
   `@aws-sdk/client-dynamodb` + `@aws-sdk/lib-dynamodb` where applicable,
   `aws-sdk-dynamodb` for Rust, `boto3`/`botocore` for Python, and the AWS
   CLI. These prove endpoint override, request serialization, paginator
   behavior, retry/error classification, and SigV4 signing compatibility from
   the same client stacks enterprises use in AWS.
3. **botocore DynamoDB service model.** Use the botocore model, waiters, and
   paginator definitions as a generated-shape oracle for operation names,
   request/response fields, errors, idempotency tokens, pagination tokens, and
   modeled exceptions. This is not a behavior oracle by itself; it prevents
   Nimbus from drifting from the SDK-visible contract.
4. **ExtendDB** (Apache-2.0). Run as a Rust binary built from
   `/Users/jack/src/github.com/ExtendDB/extenddb` (`cargo build --release` →
   `./target/release/extenddb init` then `./target/release/extenddb serve`).
   Behavior is the AWS-blessed open reference; documented divergences from
   real DynamoDB are in
   `/Users/jack/src/github.com/ExtendDB/extenddb/docs/differences-from-dynamodb.md`
   — Nimbus picks `match`, `accept-extenddb-divergence`, or `diverge` for
   each, recorded explicitly.
5. **moto DynamoDB tests.** Use the moto DynamoDB corpus as an edge-case
   scenario source for common application expectations. Moto is not treated as
   authoritative when it conflicts with DynamoDB Local or official SDK models.

The parity test runner lives at
`crates/nimbus-dynamodb/tests/dynamodb_spec/` with a `mod.rs` that constructs a
shared scenario list, executes each scenario against Nimbus, then against
DynamoDB Local (and optionally ExtendDB) using the same wire client, and
diffs the responses. Use the same scenario model the MongoDB adapter's
`mongodb_spec/` runner established. Seed the scenario list from the ExtendDB
Python parity corpus at
`/Users/jack/src/github.com/ExtendDB/extenddb/tests/` (each `test_*.py` is a
ready-made behavior set covering one operation family — item ops, query/scan,
batch, transact, conditional writes, streams, TTL, GSI, etc.). Translate the
scenarios into the Nimbus scenario model; do not copy ExtendDB Python code
into the Nimbus tree.

In addition to translated scenarios, add a canary runner that can execute
external suites by path, modeled after ExtendDB's `external-suites.toml`
approach. The runner records the suite name, upstream path, commit SHA,
command, environment, client SDK, target endpoint, pass/fail count, skipped
tests, and artifacts. External canaries are allowed to be slower than PR lanes;
they should run in nightly/manual lanes, with a small critical subset promoted
to PR once stable.

Every scenario records its provenance: DynamoDB Local probe, AWS SDK/CLI source
test, botocore model shape, ExtendDB test path, moto test path, or Nimbus-only
regression. Enterprise compatibility evidence is not accepted if it only proves
the Rust handler accepts a manually assembled JSON body. At least one official
SDK client must exercise each supported operation family before the tier can be
called complete.

### External Test Reuse And Canary Policy

Nimbus needs its own tests and it should also reuse external compatibility
knowledge. Large known-good suites are valuable and should be treated like the
Node LTS compatibility suites: pinned, repeatable external canary corpora that
run against Nimbus by endpoint override. The default is not "copy only small
fixtures." The default is:

1. Run large external suites by reference from their own checkout or package
   installation.
2. Translate important scenarios into Nimbus-native tests when they become
   product invariants or need deterministic PR coverage.
3. Vendor fixtures or whole suites only when copying is legally clean,
   maintainable, and better than a path-referenced canary lane.

Use four explicit categories:

- `source-reference`: upstream source, SDK tests, or docs were read and cited,
  but no code or fixture was copied.
- `external-canary-suite`: a large upstream or customer-like suite is run from
  its own checkout/package path against Nimbus using endpoint override. The
  suite is not copied into the Nimbus tree, but its commit/version, command,
  environment, pass/fail count, and exclusions are recorded.
- `translated-scenario`: an upstream behavior case was rewritten into the
  Nimbus scenario model, with provenance recorded as the upstream repo path,
  test name, commit SHA, and any semantic changes.
- `vendored-fixture`: a small upstream fixture, golden request/response, SigV4
  vector, or focused test helper was copied into the Nimbus tree.

External canary suites are required for enterprise DynamoDB compatibility.
Initial required canaries:

- ExtendDB Python/boto3 suites from `/Users/jack/src/github.com/ExtendDB/extenddb/tests/`
  and `/Users/jack/src/github.com/ExtendDB/extenddb/tests/python/`, pinned by
  ExtendDB commit SHA.
- ExtendDB Rust SDK suite from
  `/Users/jack/src/github.com/ExtendDB/extenddb/tests/rust/`, pinned by
  ExtendDB commit SHA and the AWS SDK Rust checkout/revision it uses.
- Official AWS CLI command suite for supported T0-T7 operation families.
- Official AWS SDK suites or Nimbus-authored canary projects using
  JavaScript v3, Rust, Python/boto3, and, when practical, Java v2.

ExtendDB does not need a separate client SDK for Nimbus to reuse. Its README
states that unmodified AWS SDKs, CLI, and tools should work by changing the
endpoint, and its tests are built around that model. Nimbus should therefore
use ExtendDB as both a behavior corpus and an example of endpoint-overridden
official SDK verification.

Before a suite is accepted as a canary, perform a quality audit and record:

- License and whether code is copied, referenced, or translated.
- Upstream commit SHA or package version.
- Whether the suite can run against real DynamoDB, DynamoDB Local, ExtendDB,
  and Nimbus by endpoint/config only.
- Which official client it uses, such as AWS CLI, boto3/botocore,
  `@aws-sdk/client-dynamodb`, `aws-sdk-dynamodb`, or AWS SDK for Java v2.
- Coverage by operation family, modeled errors, pagination, SigV4, retry
  behavior, streams, TTL, tags, transactions, and secondary indexes.
- Skip/xfail policy. Required canaries must not hide supported-operation
  failures behind broad skips or expected failures.
- Cleanup/isolation behavior, credential handling, determinism, runtime cost,
  and whether tests can run in PR, nightly, or manual lanes.

Vendoring is allowed when all of these are true:

- The upstream license is compatible with the Nimbus repository license
  posture, such as Apache-2.0, MIT, or BSD-style terms.
- The copied artifact is stable enough to audit and maintain. A large suite may
  be vendored only with an explicit owner, update cadence, license/NOTICE
  proof, and reason the external-canary path is insufficient.
- Original license headers are preserved, modifications are marked, and
  repo-root `NOTICE` coverage is updated when required.
- The test does not require upstream-private services, credentials, or brittle
  timing assumptions.
- The execution log records the upstream commit SHA and why translation was
  insufficient.

Never copy GPL/AGPL or unknown-license tests into Nimbus. For those, use
`source-reference` or `external-canary-suite` only if execution is legally and
operationally acceptable, or use `translated-scenario` without copying code.

Nimbus-owned tests are still required for Nimbus-specific guarantees:
tenant isolation, access-key binding, engine/storage atomicity, cancellation
boundaries, crate dependency boundaries, `_nimbus` ownership, performance
baselines, and soak/failure-injection behavior. External suites can prove
compatibility; they cannot prove Nimbus production safety by themselves.

## Control Plan Rules

1. Read `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
   `docs/plans/README.md`, and this plan before starting a roadmap item.
2. Run `git status --short` before choosing work. If the worktree is dirty,
   inspect the changed files and reconcile them with the current
   `in_progress` item or execution log before editing.
3. If any roadmap item is `in_progress`, resume that item. If none is
   `in_progress`, pick the first `pending` item in roadmap order whose hard
   dependencies are `done`.
4. Mark exactly one item `in_progress` before implementation. Do not advance
   another item until the active item is `done` or `blocked`.
5. Prefer one roadmap item per context window. If an item cannot fit with its
   relevant source, implementation, tests, and checkpoint loaded at once,
   split the item in this plan before starting it.
6. DynamoDB work that discovers shared database behavior in any existing
   adapter must either promote that behavior into a Nimbus primitive or add a
   design note here explaining why it remains adapter-specific.
7. A roadmap item is not `done` until its completion gate and verification
   commands are recorded in the execution log.
8. If blocked, mark the item `blocked`, record the blocker and next concrete
   action in the execution log, and do not silently skip to dependent work.

## Verification Contract

Every completed item must leave durable evidence:

- The roadmap item status is updated.
- The phase status ledger is updated when a phase moves state.
- The execution log records the date, item, files or modules touched, and
  verification commands/results.
- Focused tests cover the changed behavior. For Rust implementation items,
  run the narrowest meaningful `cargo test` or `cargo check` lane first, then
  `cargo fmt --all --check`.
- Run `make clippy` before any PR or after shared primitive work that
  touches `nimbus-core`, `nimbus-engine`, `nimbus-storage`, or `nimbus-server`
  behavior broadly.
- For JavaScript package work, run the relevant package build/typecheck/test
  command plus root `npm run typecheck` when exported API surfaces change.
- Parity-test divergences from DynamoDB Local or ExtendDB must be either
  fixed or explicitly classified in the divergence log.
- Crate-boundary evidence must be recorded whenever D0 changes wiring:
  `cargo tree -p nimbus-dynamodb --edges normal`, `cargo tree -p nimbus-server --edges normal`,
  and a stale-reference audit showing no old `*-adapter` package,
  path, or import names.

## Production Readiness Success Criteria

The DynamoDB adapter is not production-ready because it compiles or because a
single AWS CLI workflow passes. A tier can be marked `done` only when the
relevant criteria below have durable proof in the execution log and committed
evidence artifacts.

### Feature Parity Gate

For every supported operation in tiers T0-T7:

- The operation appears in a generated coverage table with status
  `implemented`, `classified-divergence`, or `unsupported-deferred`.
- `implemented` operations have request-shape validation, success response
  tests, modeled error tests, limit tests, and malformed-input tests.
- Every modeled exception the adapter claims to support has a test that
  asserts the HTTP status, DynamoDB `__type`, message shape, and SDK-visible
  error classification.
- Pagination operations prove token roundtrip, exhausted pagination, invalid
  token rejection, and SDK paginator compatibility.
- Batch and transaction operations prove partial failure envelopes,
  cancellation reasons, idempotency/client-token behavior where applicable,
  and atomicity boundaries.
- Every intentional divergence from DynamoDB Local or the botocore model is
  recorded in `docs/adapters/dynamodb/divergences.md` with rationale and a
  regression test asserting the chosen behavior.

Success metric: 100% of T0-T7 supported operations have a coverage-table row,
100% of `implemented` rows have focused tests, and 0 unclassified differences
remain in the parity report.

### Official SDK Compatibility Gate

For every supported operation family, at least one scenario must pass through
each official client family that enterprises commonly use:

- AWS CLI.
- JavaScript v3 (`@aws-sdk/client-dynamodb`, plus `@aws-sdk/lib-dynamodb`
  where document-client behavior matters).
- Rust (`aws-sdk-dynamodb`).
- Python (`boto3`/`botocore`).

Success metric: the SDK matrix records exact package versions, auth mode,
endpoint URL, operation families, pass/fail counts, and modeled divergences.
No tier may be marked `done` if an official SDK client fails a supported
operation due to Nimbus request parsing, response shape, SigV4 signing,
pagination, retry classification, or modeled exception drift.

### Reliability Gate

Each implemented tier must include failure-mode and concurrency coverage:

- Malformed JSON, missing headers, unknown operations, unsupported parameters,
  oversized payloads, empty sets, invalid keys, and invalid pagination tokens
  fail closed with DynamoDB-shaped errors.
- Dropped client connections, request cancellation before commit, cancellation
  after durable commit, engine errors, storage errors, and timeout paths do not
  panic, leak tasks, or return partial success envelopes.
- Concurrent writes, conditional writes, batch writes, transactions, stream
  reads, and TTL sweeps have race tests or deterministic stress tests proving
  stable behavior.
- Tenant isolation is proven with at least two tenants/access keys: cross-tenant
  reads, writes, streams, tags, TTL settings, and table listings are denied or
  invisible as appropriate.
- A soak test runs mixed SDK traffic for a fixed duration and records request
  count, error count by class, task count before/after, memory high-water mark,
  and panic count.

Success metric: focused failure/concurrency tests pass, the soak test records
0 panics, 0 task leaks, 0 unclassified 5xx responses, and 0 tenant-isolation
violations.

### Performance Gate

Performance proof is required before declaring enterprise readiness:

- Add a deterministic benchmark profile for embedded local storage covering
  PutItem, GetItem, UpdateItem, Query, Scan, BatchGetItem, BatchWriteItem,
  TransactWriteItems, and Streams GetRecords.
- Record throughput plus p50/p95/p99 latency for each operation family, item
  size class, and concurrency level.
- Commit an initial benchmark baseline once the operation family is complete.
  Future changes must stay within the configured non-regression threshold or
  update the baseline with a written justification.
- Include at least one mixed workload benchmark that combines reads, writes,
  conditional writes, queries, streams, and TTL/tag metadata access.
- Include memory-allocation or resident-set tracking for large scans, paginated
  reads, batch operations, and stream reads.

Success metric: every T0-T7 operation family has a benchmark baseline and a
non-regression gate. The plan may set the first numeric SLO after the initial
baseline is measured, but it must record p50/p95/p99 latency, throughput,
dataset size, item size, concurrency, storage backend, host hardware, and
commit SHA.

## Compatibility Tiers

| Tier | Goal | Required features |
|------|------|-------------------|
| T0 | Wire envelope + control plane | HTTP/JSON listener on port 8000, X-Amz-Target dispatch, AttributeValue codec, error envelope (`__type`), CreateTable, DescribeTable, ListTables, UpdateTable, DeleteTable, DescribeEndpoints, DescribeLimits |
| T1 | Single-item ops + expressions | PutItem, GetItem, DeleteItem, UpdateItem; ConditionExpression, UpdateExpression (SET/REMOVE/ADD/DELETE), ProjectionExpression, ExpressionAttributeNames, ExpressionAttributeValues, ReturnValues (NONE/ALL_OLD/ALL_NEW/UPDATED_OLD/UPDATED_NEW) |
| T2 | Query and Scan | Query (KeyConditionExpression + FilterExpression + ProjectionExpression + pagination via LastEvaluatedKey/ExclusiveStartKey + Limit + ScanIndexForward), Scan (FilterExpression + ProjectionExpression + Segment/TotalSegments parallel scan) |
| T3 | Batch and transactional ops | BatchGetItem (≤100 keys), BatchWriteItem (≤25 ops), TransactGetItems (≤100 items), TransactWriteItems (≤100 ops including Put/Update/Delete/ConditionCheck) |
| T4 | Secondary indexes | GSI (Create/Update/Delete via UpdateTable), LSI (declared at CreateTable), projection types (KEYS_ONLY, INCLUDE, ALL), index-targeted Query and Scan via `IndexName` |
| T5 | DynamoDB Streams | StreamSpecification (StreamEnabled, StreamViewType: KEYS_ONLY/NEW_IMAGE/OLD_IMAGE/NEW_AND_OLD_IMAGES), DescribeStream, GetShardIterator, GetRecords, ListStreams |
| T6 | TTL and tagging | UpdateTimeToLive, DescribeTimeToLive, TagResource, UntagResource, ListTagsOfResource |
| T7 | SigV4 strict mode | SigV4 canonical request verification, derived-key chain, request expiration window, principal-to-tenant resolution from access key |
| T8 | JavaScript SDK package | `@nimbus/dynamodb` connecting `@aws-sdk/client-dynamodb` to Nimbus |
| Deferred | Advanced features | DAX (Accelerator), Global Tables, PartiQL (ExecuteStatement, ExecuteTransaction, BatchExecuteStatement), CreateBackup/RestoreTable, PITR (DescribeContinuousBackups, UpdateContinuousBackups), ImportTable from S3, ExportTableToPointInTime to S3, Kinesis Data Streams, Contributor Insights |

## Architecture Boundary Contract

### Crate Boundary

- `nimbus-dynamodb` owns DynamoDB wire semantics, AttributeValue conversion,
  expression bridging, operation dispatch, SigV4 verification, stream shaping,
  parity-test scaffolding, and the axum router factory.
- `nimbus-server` owns listener bind/spawn/shutdown, CLI/ServeOptions
  composition, global task supervision, and any route mounting needed to expose
  the `nimbus-dynamodb` router.
- `nimbus-adapters` is an optional default-empty facade. It may add a
  `dynamodb` feature that re-exports `nimbus-dynamodb` after the concrete
  crate compiles and has focused tests. `nimbus-server` must not depend on the
  facade.
- `nimbus-dynamodb` must not depend on `nimbus-server`, must not import
  server-private modules, must not accept `AppState`, and must not write
  `_nimbus` or other system-owned state directly.
- Tenant authority flows through explicit tenant bindings and narrow Nimbus
  engine/service capabilities. DynamoDB table names, access keys, request
  headers, or SigV4 credentials must not directly select lower-layer storage
  without adapter admission.

### Nimbus Core Owns

- Document identity validation and key-generation primitives. DynamoDB's
  partition/sort-key codec stays in `nimbus-dynamodb` unless a later
  cross-adapter need justifies promoting a protocol-neutral composite-key
  primitive.
- Atomic write batch semantics: insert, update, delete, conditional update.
- Query representation and execution: filters, ordering, cursors,
  projections, limits, offsets.
- Transaction/session lifecycle: token creation, read tracking,
  commit/rollback.
- Subscription/change-feed snapshot and diff surfaces.
- Index definition and maintenance (compound, unique, partial, TTL metadata).
- Protocol-neutral error taxonomy.

### Adapter Owns

- DynamoDB router construction and request handling for the DynamoDB port.
  Socket bind, task lifecycle, and shutdown stay in `nimbus-server`.
- DynamoDB JSON wire protocol envelope: `X-Amz-Target` dispatch, request body
  parsing, response body shaping, error envelope (`__type` + message).
- AttributeValue serialization (S/N/B/M/L/SS/NS/BS/BOOL/NULL) including
  typed-scalar metadata for number-with-precision and binary roundtrip.
- Composite-key encoding (partition + sort) into Nimbus `DocumentId` with a
  reversible canonical form.
- Expression language parsing and evaluation: ConditionExpression,
  UpdateExpression, FilterExpression, ProjectionExpression,
  KeyConditionExpression. ExpressionAttributeNames and ExpressionAttributeValues
  substitution.
- Pagination state: opaque `LastEvaluatedKey` ↔ Nimbus cursor mapping.
- ReturnValues handling on writes.
- DynamoDB-specific error codes (ConditionalCheckFailedException,
  ResourceNotFoundException, ResourceInUseException, ValidationException,
  TransactionCanceledException, TransactionConflictException,
  ProvisionedThroughputExceededException, ItemCollectionSizeLimitExceededException,
  ThrottlingException).
- Secondary-index addressing (`IndexName` parameter on Query/Scan, GSI
  projection materialization).
- Stream protocol: StreamArn, shard model, ShardIterator format, sequence
  numbers, StreamRecord shaping.
- AWS SigV4 verification: canonical request, derived-key chain, signature
  comparison, request-expiration enforcement.
- Endpoint discovery (`DescribeEndpoints`).
- TTL sweeper trigger (the sweep itself lives in core; the adapter declares
  the TTL attribute name).
- Tag storage and retrieval (separate from item content).

### Shared Primitive Promotion Rule

Before landing DynamoDB work that resembles existing Firebase, Convex, or
MongoDB adapter logic, compare the paths:

- If the logic is about Nimbus data semantics (document writes, query
  planning, subscriptions, transactions, composite-key canonicalization),
  move it to a shared seam and thin the adapter.
- If the logic is about DynamoDB wire-protocol shape, AttributeValue
  encoding, expression-language semantics, SigV4 canonical-request
  construction, or stream shard format, keep it in the DynamoDB adapter.

## Required Foundation Work

### D0.1: HTTP Listener For DynamoDB Port

Nimbus already runs an axum HTTP listener for the Convex/Firebase HTTP
surfaces. The DynamoDB adapter needs a separate axum app bound to its own
port so the X-Amz-Target dispatch does not collide with existing routes:

- Add a `DynamoDbConfig` to the server configuration model with optional
  port (default 8000), bind address, and enabled/disabled flag.
- The DynamoDB axum app runs as a sibling `tokio::spawn` task sharing the
  same `Arc<Service>` instance.
- All requests are POST `/`; `X-Amz-Target: DynamoDB_20120810.<Operation>`
  selects the handler.
- The adapter must reject non-POST and missing/invalid `X-Amz-Target` with
  the DynamoDB error envelope (HTTP 400, `__type: "UnknownOperationException"`
  or `"SerializationException"`).

### D0.2: AttributeValue Bridge

DynamoDB AttributeValue is a tagged union over ten variants. The bridge
must:

- Convert AttributeValue documents to Nimbus `serde_json::Value` documents
  for storage, using the typed-scalar metadata infrastructure for `N`
  (string-encoded arbitrary-precision number), `B` (base64 binary), `SS`
  (string set, order-insensitive equality), `NS` (number set), `BS` (binary
  set).
- Convert Nimbus documents back to AttributeValue for response shaping,
  preserving the original type via the typed-scalar metadata.
- Reject empty sets and empty top-level documents with `ValidationException`
  per DynamoDB semantics.
- Preserve DynamoDB's strict type comparison rules for sorting (N comparison
  is numeric, S is lexicographic UTF-8, B is byte-wise).

### D0.3: Composite Primary Key Encoding

DynamoDB tables have a partition key (HASH) and an optional sort key
(RANGE). Nimbus `DocumentId` is a single validated UTF-8 string with max
1500 bytes, no `/`, no NUL (`crates/nimbus-core/src/types.rs`
`validate_document_key`).

The encoding must:

- Map `(pk, sk)` to a single deterministic, reversible string within the
  validation rules. Initial encoding: `<pk-base64url>.<sk-base64url>` when
  `sk` is present; `<pk-base64url>` when `sk` is None. Base64url avoids `/`
  and NUL; the `.` separator is unambiguous because base64url omits `=`
  padding inside the encoded segments.
- Reject combined encodings exceeding 1500 bytes with `ValidationException`
  (DynamoDB itself limits item keys to 2KB for HASH + 1KB for RANGE — Nimbus
  documents the tighter limit).
- Preserve original `pk` and `sk` AttributeValues by storing them as
  separate fields in the document body (`_pk`, `_sk`) so query/scan and
  GSI/LSI evaluation can read them without re-decoding the key.

### D0.4: DynamoDB Error Code Mapping

Map Nimbus's shared error taxonomy to DynamoDB error codes and response
format:

- DynamoDB errors are returned with HTTP 400 (4xx) or 500 (5xx) and a JSON
  body `{ "__type": "com.amazonaws.dynamodb.v20120810#<Code>", "message": "..." }`.
- Map `NotFound` → `ResourceNotFoundException`, `AlreadyExists` →
  `ResourceInUseException`, `InvalidInput` → `ValidationException`,
  `Unauthorized` → `AccessDeniedException`, `Conflict` →
  `TransactionConflictException`, `RateLimited` →
  `ProvisionedThroughputExceededException`.
- Conditional check failures must return `ConditionalCheckFailedException`
  with the optional `Item` field (when `ReturnValuesOnConditionCheckFailure`
  is set).

### D0.5: DynamoDB Table-To-Tenant Mapping

DynamoDB has a flat account-level namespace for tables. Nimbus uses tenant +
table:

- Map each access key (configured at adapter setup) to a tenant.
- Within that tenant, DynamoDB table name maps directly to Nimbus table
  name. Validate names against DynamoDB's rule (3–255 chars, ASCII alnum +
  `_` + `-` + `.`).
- `ListTables` enumerates Nimbus tables in the resolved tenant.

## Protocol Specification

### Wire Format

| Property | Value |
|----------|-------|
| Transport | HTTP/1.1 (HTTP/2 acceptable) |
| Method | POST |
| Path | `/` (DynamoDB ignores path) |
| Target header | `X-Amz-Target: DynamoDB_20120810.<Operation>` |
| Content type | `application/x-amz-json-1.0` |
| Body | JSON request document |
| Auth | `Authorization: AWS4-HMAC-SHA256 Credential=... SignedHeaders=... Signature=...` plus `X-Amz-Date` and optional `X-Amz-Security-Token` |
| Response | HTTP 200 (success) or 4xx/5xx (error) with JSON body; success bodies have no envelope, error bodies have `__type` and `message` |

### Operation Scope By Tier

| Operation | Tier | Notes |
|-----------|------|-------|
| `CreateTable` | T0 | Includes KeySchema, AttributeDefinitions, BillingMode, GSI/LSI (LSI handled at T4) |
| `DescribeTable` | T0 | Returns TableDescription with KeySchema, AttributeDefinitions, ItemCount, IndexStatus |
| `ListTables` | T0 | Paginated via `ExclusiveStartTableName` + `LastEvaluatedTableName` |
| `UpdateTable` | T0 (basic), T4 (GSI) | Includes GSI CRUD via `GlobalSecondaryIndexUpdates`, StreamSpecification updates |
| `DeleteTable` | T0 | Soft-deletes then physically removes |
| `DescribeEndpoints` | T0 | Returns the configured Nimbus DynamoDB endpoint |
| `DescribeLimits` | T0 | Returns stubbed limits (Nimbus has no provisioned-throughput model) |
| `PutItem` | T1 | With ConditionExpression, ReturnValues, ReturnConsumedCapacity, ReturnItemCollectionMetrics |
| `GetItem` | T1 | With ProjectionExpression, ConsistentRead |
| `DeleteItem` | T1 | With ConditionExpression, ReturnValues |
| `UpdateItem` | T1 | With UpdateExpression (SET, REMOVE, ADD, DELETE), ConditionExpression, ReturnValues |
| `Query` | T2 | With KeyConditionExpression, FilterExpression, ProjectionExpression, IndexName, ScanIndexForward, Limit, ExclusiveStartKey, ConsistentRead, Select |
| `Scan` | T2 | With FilterExpression, ProjectionExpression, IndexName, Limit, Segment/TotalSegments, ExclusiveStartKey, Select |
| `BatchGetItem` | T3 | Up to 100 keys across tables, returns Responses + UnprocessedKeys |
| `BatchWriteItem` | T3 | Up to 25 PutRequest/DeleteRequest across tables, returns UnprocessedItems |
| `TransactGetItems` | T3 | Up to 100 reads with optional ProjectionExpression per item |
| `TransactWriteItems` | T3 | Up to 100 ops (Put/Update/Delete/ConditionCheck) atomic; CancellationReasons on failure |
| `DescribeStream` | T5 | StreamDescription with Shards, StreamStatus, StreamViewType |
| `GetShardIterator` | T5 | Returns opaque ShardIterator from TRIM_HORIZON/LATEST/AT_SEQUENCE_NUMBER/AFTER_SEQUENCE_NUMBER |
| `GetRecords` | T5 | Returns Records + NextShardIterator |
| `ListStreams` | T5 | Optionally filtered by TableName |
| `UpdateTimeToLive` | T6 | Enable/disable TTL on a named attribute |
| `DescribeTimeToLive` | T6 | Returns current TTL configuration |
| `TagResource` | T6 | Per-table tag store |
| `UntagResource` | T6 | Tag removal |
| `ListTagsOfResource` | T6 | Tag enumeration |

### Expression Language

The expression language is shared across ConditionExpression,
UpdateExpression, FilterExpression, ProjectionExpression, and
KeyConditionExpression. The grammar (simplified):

```
expression       := condition | update | projection | key_condition
condition        := operand op operand | function | NOT condition | condition AND condition | condition OR condition | ( condition )
operand          := path | literal_ref
path             := name ('.' name | '[' int ']')*
name             := identifier | '#' identifier (resolved via ExpressionAttributeNames)
literal_ref      := ':' identifier (resolved via ExpressionAttributeValues)
op               := '=' | '<>' | '<' | '<=' | '>' | '>=' | BETWEEN operand AND operand | IN ( operand, ... )
function         := attribute_exists(path) | attribute_not_exists(path) | attribute_type(path, ":t") | begins_with(path, operand) | contains(path, operand) | size(path)
update           := action_clause ( ',' action_clause | action_clause )*
action_clause    := 'SET' set_action (, set_action)* | 'REMOVE' path (, path)* | 'ADD' add_action (, add_action)* | 'DELETE' delete_action (, delete_action)*
set_action       := path '=' update_value
update_value     := operand | operand '+' operand | operand '-' operand | function_call
function_call    := if_not_exists(path, operand) | list_append(operand, operand)
projection       := path (',' path)*
key_condition    := key_op (AND key_op)?  -- partition key '=' plus optional sort-key comparator
```

Reserved words (573 of them) are handled transparently: clients always use
`#name` placeholders when they collide. The parser does not need to reject
reserved word usage outright.

### Reserved Names In Nimbus Documents

Nimbus uses leading-underscore field names internally. The DynamoDB adapter
must:

- Reject attribute names beginning with `_nimbus_` to avoid collision with
  internal markers.
- Reserve `_pk` and `_sk` as internal fields for partition/sort key
  preservation; reject these attribute names in incoming items.

## Context Window Budget

| Phase | Scope | Context windows |
|-------|-------|-----------------|
| D0 | Wire envelope, AttributeValue bridge, composite-key encoding, control plane | 6-8 |
| D1 | Single-item operations + expression language | 8-12 |
| D2 | Query and Scan with pagination | 5-7 |
| D3 | Batch and transactional operations | 4-6 |
| D4 | Secondary indexes (GSI, LSI) | 5-7 |
| D5 | DynamoDB Streams | 4-6 |
| D6 | TTL, tagging, control-plane completion | 3-5 |
| D7 | SigV4 strict mode | 3-5 |
| D8 | `@nimbus/dynamodb` SDK package + parity test integration | 6-9 |
| D9 | Enterprise readiness, reliability, and performance closeout | 4-7 |
| Buffer | Upstream SDK alignment, edge-case divergences | 4-6 |
| **Total** | | **52-78** |

## Implementation Phases

### D0: Wire Envelope, AttributeValue Bridge, Control Plane

Location: `crates/nimbus-dynamodb/src/`, with only a thin server composition
shim under `crates/nimbus-server/src/adapters/dynamodb/` if needed.

Context window budget: 6-8 focused windows.

- Add the `nimbus-dynamodb` workspace crate and package, following the
  concrete provider naming pattern used by `nimbus-mongodb`,
  `nimbus-firebase`, `nimbus-cloud-functions`, and `nimbus-convex`.
- Add `DynamoDbConfig` in `nimbus-dynamodb` and optional HTTP listener
  composition to server configuration.
- Implement an axum router factory in `nimbus-dynamodb` that POSTs all
  requests through the X-Amz-Target dispatch. The server binds the socket and
  supervises the task.
- Implement the AttributeValue ↔ Nimbus value bridge using the typed-scalar
  metadata infrastructure for type-preserving roundtrips (N, B, SS, NS, BS).
- Implement composite-key encoding with reversible canonical form.
- Implement error envelope shaping with `__type` codes.
- Implement table-to-tenant resolution from access-key prefix or header
  binding.
- Implement control-plane operations: `CreateTable`, `DescribeTable`,
  `ListTables`, `UpdateTable` (basic stream + table-class fields, GSI
  deferred to D4), `DeleteTable`, `DescribeEndpoints`, `DescribeLimits`.
- Parse SigV4 authorization headers in lookup-only mode (extract access key
  → tenant principal; strict signature verification deferred to D7).

Exit gate: `aws dynamodb create-table` / `describe-table` / `list-tables` /
`delete-table` via the AWS CLI with `--endpoint-url http://localhost:8000`
succeeds. The `aws-sdk-rust` DynamoDB client can issue the same operations.

### D1: Single-Item Operations And Expression Language

Location: `crates/nimbus-dynamodb/src/`.

Context window budget: 8-12 focused windows.

- Wire the `extenddb-core` expression parser/evaluator into the
  `nimbus-dynamodb` bridge. Do not reimplement the expression grammar unless
  the direct upstream dependency proves unmaintainable and the fallback path is
  explicitly recorded.
- Implement ConditionExpression evaluation against an item document.
- Implement UpdateExpression with SET (with `if_not_exists`, `list_append`,
  arithmetic), REMOVE, ADD (numeric add, set add), DELETE (set subtract).
- Implement ProjectionExpression as a path-based field selector.
- Implement `PutItem`, `GetItem`, `DeleteItem`, `UpdateItem` with full
  expression support and `ReturnValues`
  (NONE / ALL_OLD / ALL_NEW / UPDATED_OLD / UPDATED_NEW).
- Map UpdateExpression `ADD numeric` to the shared `FieldTransformOperation`
  (Increment). Map `ADD set` and `DELETE set` to the existing set transforms.
  Operations without a direct transform (REMOVE on nested path, SET with
  if_not_exists or list_append) execute as read-modify-write within the
  shared atomic write batch using ConditionExpression for optimistic
  concurrency.
- Handle ConditionalCheckFailedException with the optional `Item` field when
  `ReturnValuesOnConditionCheckFailure` is set.

Exit gate: full single-item AWS CLI workflow succeeds:
`put-item`, `get-item`, `update-item` with all four UpdateExpression action
kinds, `delete-item`, all with ConditionExpression. Parity diff against
DynamoDB Local for the AWS-SDK-generated request shapes is clean for
covered operations.

### D2: Query And Scan

Location: `crates/nimbus-dynamodb/src/`.

Context window budget: 5-7 focused windows.

- Implement `Query` translating KeyConditionExpression into a primary-key
  prefix scan, then applying FilterExpression in-memory (or as a query AST
  filter when possible), with ProjectionExpression projection,
  ScanIndexForward sort order, Limit, and ExclusiveStartKey/LastEvaluatedKey
  pagination.
- Implement `Scan` translating FilterExpression and ProjectionExpression
  over a full-table iteration, with Limit, ExclusiveStartKey, and
  Segment/TotalSegments parallel scan partitioning.
- Implement Select modes: `ALL_ATTRIBUTES`, `ALL_PROJECTED_ATTRIBUTES`
  (T4-dependent), `SPECIFIC_ATTRIBUTES`, `COUNT`.
- Implement `LastEvaluatedKey` as an opaque base64-encoded cursor that round-
  trips Nimbus cursor state.
- ConsistentRead currently maps to Nimbus's standard read (Nimbus is
  strongly consistent by default); accept and ignore the flag.

Exit gate: AWS CLI `query --key-condition-expression "pk = :pk"` and `scan`
work end-to-end with pagination. Parity diff against DynamoDB Local for the
AWS-SDK-generated request shapes is clean.

### D3: Batch And Transactional Operations

Location: `crates/nimbus-dynamodb/src/`.

Context window budget: 4-6 focused windows.

- Implement `BatchGetItem` fan-out across up to 100 keys, returning
  `Responses` keyed by table name and `UnprocessedKeys` for any failures.
- Implement `BatchWriteItem` fan-out across up to 25 PutRequest/DeleteRequest
  operations using the shared atomic write batch primitive, returning
  `UnprocessedItems`. Each write within the batch is independent; failures
  do not roll back successes.
- Implement `TransactGetItems` returning up to 100 reads atomically (snapshot
  consistency through the engine's transaction session manager).
- Implement `TransactWriteItems` with up to 100 ops (Put/Update/Delete/
  ConditionCheck) atomic via the engine transaction session manager.
  Failure returns `TransactionCanceledException` with the
  `CancellationReasons` array describing each op's outcome.
- Enforce DynamoDB-side limits: 100-item batch reads, 25-op write batch,
  100-op write transaction, 4MB combined item size for transactions.

Exit gate: AWS SDK `batchGet`, `batchWrite`, `transactGet`, `transactWrite`
all execute end-to-end. Parity diff is clean for covered shapes.

### D4: Secondary Indexes (GSI, LSI)

Location: `crates/nimbus-dynamodb/src/`.

Context window budget: 5-7 focused windows.

- Implement Global Secondary Index definitions in `CreateTable` and
  `UpdateTable` (`GlobalSecondaryIndexUpdates`: Create/Update/Delete).
- Implement Local Secondary Index definitions declared at `CreateTable`
  only (LSI cannot be added or removed after table creation in real
  DynamoDB).
- Implement projection types: KEYS_ONLY (return only base + index keys),
  INCLUDE (return base + index keys + specified attributes), ALL (return
  full item).
- Map GSI/LSI to Nimbus secondary indexes. Apply projection at response
  time.
- Implement index-targeted Query and Scan via the `IndexName` parameter.
- GSI consistency: real DynamoDB GSIs are eventually consistent. Nimbus
  indexes are strongly consistent. Document this as a divergence (clients
  setting `ConsistentRead=true` on a GSI Query receive a ValidationException
  in real DynamoDB; Nimbus accepts and serves consistently — document
  whether to match the rejection behavior or accept it).

Exit gate: AWS SDK CreateTable with GSI/LSI, UpdateTable adding/removing
GSI, and Query/Scan targeting indexes all execute end-to-end. Parity diff is
clean for covered shapes with explicit divergence classification for the
ConsistentRead-on-GSI question.

### D5: DynamoDB Streams

Location: `crates/nimbus-dynamodb/src/`.

Context window budget: 4-6 focused windows.

- Implement StreamSpecification at table creation/update: StreamEnabled,
  StreamViewType (KEYS_ONLY, NEW_IMAGE, OLD_IMAGE, NEW_AND_OLD_IMAGES).
- Implement `DescribeStream` returning a single-shard stream description for
  each enabled table (single-shard model; document the divergence from real
  DynamoDB's hierarchical shard tree).
- Implement `GetShardIterator` for TRIM_HORIZON, LATEST,
  AT_SEQUENCE_NUMBER, AFTER_SEQUENCE_NUMBER iterator types.
- Implement `GetRecords` returning up to 1000 records and a
  NextShardIterator. Map Nimbus subscription snapshot diffs to
  StreamRecord using the configured StreamViewType.
- Implement `ListStreams` enumerating active streams.
- Record retention: real DynamoDB Streams retain 24 hours of records. Nimbus
  initial retention: configurable (default 24 hours) with eviction when
  records exceed retention.

Exit gate: AWS SDK can describe a stream, request an iterator, and pull
records that reflect actual writes. Parity diff is clean for the
single-shard model; multi-shard behavior is documented as an accepted
divergence.

### D6: TTL, Tagging, Control-Plane Completion

Location: `crates/nimbus-dynamodb/src/`.

Context window budget: 3-5 focused windows.

- Implement `UpdateTimeToLive` (enable/disable on a named attribute).
- Implement `DescribeTimeToLive`.
- Implement the TTL sweeper as a periodic engine-owned task that scans
  TTL-enabled tables and deletes items whose TTL attribute is past current
  epoch seconds. Real DynamoDB sweeps lazily (up to 48 hours after
  expiration); Nimbus aims for the same shape with a configurable sweep
  interval.
- Implement per-table tag storage (`TagResource`, `UntagResource`,
  `ListTagsOfResource`). Tags live in adapter-local metadata, not item
  documents.
- Close out any remaining control-plane operations not in D0.

Exit gate: AWS CLI TTL workflow (enable, describe, observe expiry) and
tagging workflow execute end-to-end.

### D7: SigV4 Strict Mode

Location: `crates/nimbus-dynamodb/src/`.

Context window budget: 3-5 focused windows.

- Implement SigV4 canonical-request construction matching the client.
- Implement the derived-key chain (kSecret → kDate → kRegion → kService →
  kSigning) using `hmac-sha256`.
- Verify the request signature against the computed signature; reject
  mismatches with `InvalidSignatureException`.
- Enforce request expiration (5-minute window typical).
- Add a `DynamoDbConfig::auth_mode: SigV4Strict | SigV4Lookup | Disabled`
  toggle so D0-D6 verification harnesses can run in the more permissive
  lookup-only mode.
- Add a Nimbus-native access-key management surface (configure access key,
  secret, region, tenant binding) since Nimbus has no IAM.

Exit gate: AWS SDK requests authenticated with SigV4 succeed; requests
without a valid signature are rejected in SigV4Strict mode. The `aws-sdk-rust`
default credential chain works against Nimbus.

### D8: JavaScript SDK Package And Parity Test Integration

Location: `packages/dynamodb/` and `crates/nimbus-dynamodb/tests/dynamodb_spec/`.

Context window budget: 6-9 focused windows.

- Package name: `@nimbus/dynamodb`. Thin wrapper around
  `@aws-sdk/client-dynamodb` and `@aws-sdk/util-dynamodb` that defaults the
  endpoint to the local Nimbus listener and provides Nimbus-specific
  helpers: connection-string builder, tenant selection, access-key
  configuration.
- Add an integration selftest against a local Nimbus server covering
  table lifecycle, item CRUD, query, scan, batch, transact, and stream
  iteration.
- Build the parity-test runner at
  `crates/nimbus-dynamodb/tests/dynamodb_spec/` analogous to
  `mongodb_spec/`. Each scenario executes the same operation sequence
  against:
  1. A local Nimbus DynamoDB listener.
  2. DynamoDB Local (Docker container).
  3. ExtendDB compiled from
     `/Users/jack/src/github.com/ExtendDB/extenddb` and launched on a
     scratch port (preferred over Docker — direct access to ExtendDB logs
     for divergence triage, no image-pull overhead, build artifact reused
     across runs).
- Diff the responses and classify divergences per the table in `Spec
  Tests` below.
- Add DynamoDB cases to the verification harness with deterministic
  seed-based scenarios:
  - `dynamodb-wire-handshake-and-control-plane`
  - `dynamodb-item-crud-roundtrip`
  - `dynamodb-query-scan-with-pagination`
  - `dynamodb-transact-write-commit-abort`
  - `dynamodb-streams-event-delivery`

Exit gate: `@nimbus/dynamodb` selftest passes; parity-test runner emits a
classification report covering at least 80% of AWS-SDK-generated request
shapes for tiers T0-T6; official JS, Rust, Python, and AWS CLI client smoke
suites pass against Nimbus for every supported operation family; verification
harness includes the five DynamoDB cases in PR and nightly lanes.

### D9: Enterprise Readiness, Reliability, And Performance Closeout

Location: `crates/nimbus-dynamodb/`, `docs/adapters/dynamodb/`, and the
verification/benchmark harnesses.

Context window budget: 4-7 focused windows.

- Generate and commit a feature-parity coverage table for T0-T7 operation
  families using the DynamoDB API reference, botocore service model, and the
  implemented Nimbus operation registry.
- Generate and commit an official SDK compatibility matrix for AWS CLI,
  JavaScript v3, Rust, and Python clients with exact versions and pass/fail
  counts.
- Add failure-injection, concurrency, tenant-isolation, and cancellation tests
  for the supported tiers.
- Add deterministic benchmark coverage for the operation families listed in
  the Performance Gate and commit the first baseline.
- Add a mixed-workload soak test and record request count, error classes, task
  count before/after, memory high-water mark, and panic count.
- Produce `docs/adapters/dynamodb/enterprise-readiness.md` summarizing feature
  coverage, known divergences, SDK versions tested, reliability proof,
  benchmark baselines, deferred features, and operational limits.

Exit gate: the enterprise-readiness doc exists, every supported operation has
coverage and SDK evidence, soak and failure-injection tests pass with no
unclassified failures, benchmark baselines are committed, and all divergences
are documented with tests.

## Testing Strategy

### Layer 1: Wire Envelope Tests

- POST `/` with valid `X-Amz-Target` dispatches correctly.
- Missing `X-Amz-Target` returns `UnknownOperationException`.
- Unknown `X-Amz-Target` value returns `UnknownOperationException`.
- Malformed JSON body returns `SerializationException`.
- Empty body for parameterless operations returns success.
- Error envelope shape (`__type` + `message`) for every error variant.

### Layer 2: AttributeValue Roundtrip Tests

- Every AttributeValue type roundtrips through Nimbus storage and back to
  AttributeValue with type fidelity.
- Number precision preserved through string-encoded N.
- Binary roundtrips via base64-encoded B without corruption.
- Empty set rejection (SS, NS, BS).
- Empty top-level document rejection.
- Empty string acceptance (DynamoDB allows empty strings as of 2020).
- Deeply nested M/L preservation.

### Layer 3: Expression Language Tests

- Parser accepts every documented operator and function.
- Parser rejects malformed expressions with appropriate error.
- ConditionExpression evaluates correctly against absent/present attributes.
- UpdateExpression SET / REMOVE / ADD / DELETE actions produce correct
  document mutations.
- UpdateExpression `if_not_exists` and `list_append` semantics.
- FilterExpression and KeyConditionExpression evaluate consistently.
- ProjectionExpression supports nested paths and array indexing.
- ExpressionAttributeNames and ExpressionAttributeValues substitution.

### Layer 4: Operation Contract Tests

- Each supported operation has focused Rust tests proving:
  - Correct response envelope.
  - Correct error envelope (`__type` per error class).
  - ReturnValues / ReturnConsumedCapacity / ReturnItemCollectionMetrics
    behavior.
  - Pagination state roundtrip (LastEvaluatedKey ↔ ExclusiveStartKey).
  - Atomic batch/transaction semantics.

### Layer 5: Composite Key Encoding Tests

- (pk, sk) encoding is reversible across the full UTF-8 plane.
- Encoded key length validation rejects oversized inputs.
- AttributeValue type fidelity preserved through key + body separation.

### Layer 6: SigV4 Verification Tests

- Valid SigV4 signatures pass verification.
- Invalid signatures return `InvalidSignatureException`.
- Expired signatures return `SignatureDoesNotMatch` /
  `RequestExpired`.
- Lookup-only mode passes without strict signature verification.

### Layer 7: Parity Tests Against DynamoDB Local And ExtendDB

Each scenario classification:

| Classification | Meaning |
|---------------|---------|
| `pass` | Identical or behaviorally-equivalent responses |
| `accept-extenddb-divergence` | Nimbus matches ExtendDB but diverges from DynamoDB Local — accepted because ExtendDB is the AWS-blessed open reference |
| `nimbus-divergence` | Nimbus diverges from both DynamoDB Local and ExtendDB — must be documented in `docs/adapters/dynamodb/divergences.md` with rationale |
| `skip-unsupported` | Feature intentionally outside supported tiers (DAX, Global Tables, PartiQL, Backup/Restore) |
| `skip-topology` | Test requires multi-region or accelerator endpoint |
| `fail-known` | Known behavioral gap with tracking note |

### Layer 8: Official SDK Compatibility Matrix

For every supported tier, run the same scenario intent through real client
libraries, not only Nimbus's internal test request builder:

- AWS CLI commands for table lifecycle, item CRUD, query/scan, batch,
  transaction, streams, TTL, and tagging where the CLI exposes the operation.
- JavaScript v3: `@aws-sdk/client-dynamodb`, `@aws-sdk/lib-dynamodb`, and the
  `@nimbus/dynamodb` helper package.
- Rust: `aws-sdk-dynamodb` with endpoint override and SigV4 signing.
- Python: `boto3`/`botocore` client calls, paginators, waiters, modeled
  exceptions, and retry classification.

The matrix records exact SDK package versions, request families covered,
auth mode, endpoint URL, pass/fail count, and any classified divergence. A
scenario is not enterprise-complete when it only passes a Nimbus-local unit
test but fails through an official SDK client.

### Layer 9: Verification Harness

DynamoDB cases run in the existing verification harness with deterministic
seed-based scenarios. Both PR and nightly lanes include the five
above-listed cases.

## Key Architectural Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Crate name | `nimbus-dynamodb` | Matches the extracted concrete adapter naming pattern (`nimbus-mongodb`, `nimbus-firebase`, `nimbus-convex`, `nimbus-cloud-functions`) and avoids the redundant `*-adapter` suffix |
| Adapter facade | Optional `nimbus-adapters` feature | Keeps an ergonomic aggregate re-export without making server depend on a facade or hiding concrete provider ownership |
| Listener | `nimbus-dynamodb` router plus `nimbus-server` listener lifecycle | DynamoDB uses HTTP/JSON; isolating the router avoids X-Amz-Target route collisions while keeping socket bind/spawn/shutdown in the server composition layer |
| Default port | 8000 | Matches DynamoDB Local convention; AWS SDK `--endpoint-url http://localhost:8000` works out of the box |
| AttributeValue codec | Typed-scalar metadata (reused from F3.4b1 and MongoDB adapter) | N/B/SS/NS/BS need roundtrip fidelity through JSON storage; MongoDB adapter already proved this for Decimal128 and Binary |
| Composite key encoding | `<pk-base64url>.<sk-base64url>` | Reversible, fits Nimbus DocumentId rules (no `/`, no NUL, ≤1500B), unambiguous separator |
| Expression parser | `extenddb-core` expression parser/evaluator through a Nimbus shim | Reuses the AWS-maintained open reference for the highest-risk protocol grammar and keeps Nimbus code focused on capability checks and storage bridging |
| Table-to-tenant mapping | Per-access-key tenant binding | DynamoDB has flat account namespace; Nimbus needs tenant scoping; binding is configured at adapter setup |
| Stream shard model | Single-shard per stream | Nimbus subscription model is non-sharded; document divergence from DynamoDB's hierarchical shard tree; sufficient for non-throughput-bound workloads |
| SigV4 auth | Two-mode: SigV4Strict and SigV4Lookup | Allows D0-D6 verification under lookup-only mode while D7 brings strict verification online; mirrors ExtendDB's pragmatic approach |
| ConsistentRead semantics | Always strongly consistent | Nimbus is strongly consistent by default; accept ConsistentRead flag as a no-op for base table reads; explicit divergence call for GSI ConsistentRead behavior |
| GSI consistency | Strongly consistent | Real DynamoDB GSIs are eventually consistent; Nimbus indexes are strongly consistent; document this as an accepted upgrade |
| Parity reference | DynamoDB Local primary, ExtendDB secondary | DynamoDB Local is closest to real DynamoDB; ExtendDB is AWS-blessed open reference; both inform divergence classification |
| Error codes | DynamoDB-canonical `__type` strings | Preserves SDK error-handling expectations |

## Deferred Or Out Of Scope

- **DAX (DynamoDB Accelerator).** Caching layer with its own protocol; out
  of scope for adapter compatibility.
- **Global Tables.** Multi-region replication; Nimbus has no equivalent
  concept.
- **PartiQL (`ExecuteStatement`, `ExecuteTransaction`, `BatchExecuteStatement`).**
  SQL-shaped surface over DynamoDB; substantial parser/optimizer scope.
- **Backup/Restore (`CreateBackup`, `RestoreTable`).** Nimbus has its own
  backup story; deferred until customer demand.
- **PITR (`DescribeContinuousBackups`, `UpdateContinuousBackups`).**
  Point-in-time recovery; depends on Nimbus's storage versioning roadmap.
- **Import/Export (`ImportTable`, `ExportTableToPointInTime`).** S3-backed
  bulk transfer; mocked-S3 path is possible but deferred.
- **Kinesis Data Streams integration.** Kinesis adapter is separate work.
- **Contributor Insights.** Top-N analytics; deferred.
- **Provisioned throughput billing and throttling.** Nimbus has no
  provisioned-throughput concept; accept and ignore RCU/WCU fields, never
  emit `ProvisionedThroughputExceededException` unless explicit Nimbus rate
  limits trigger.
- **Reserved capacity.** AWS-specific billing concept; not applicable.

## Risks

**R1: AttributeValue fidelity through JSON storage (Critical, D0).**
DynamoDB types like N (string-encoded arbitrary precision), B (binary), and
SS/NS/BS (sets) cannot roundtrip through plain JSON. Mitigation: reuse the
typed-scalar metadata infrastructure proven for MongoDB Decimal128 and
Binary; add explicit unit tests for every AttributeValue variant.

**R2: Composite-key encoding correctness (Critical, D0).** Reversibility
across the full UTF-8 plane plus the 1500-byte Nimbus key limit is
non-trivial. Mitigation: use base64url for both pk and sk segments with a
`.` separator; add property tests across the full Unicode plane; reject
oversized keys with `ValidationException`.

**R3: Expression language complexity (High, D1).** The expression grammar
covers comparison, logical, function, and update-action surfaces, plus
ExpressionAttributeNames/Values substitution. Mitigation: reuse
`extenddb-core`'s expression parser/evaluator through a thin Nimbus shim; add
focused adapter tests around Nimbus document bridging; use parity tests against
DynamoDB Local and ExtendDB as the correctness oracle.

**R4: SigV4 verification correctness (High, D7).** SigV4 requires precise
canonical-request construction, ordered header signing, and key-derivation
chain matching the SDK exactly. Any drift causes signature mismatch.
Mitigation: vendor ExtendDB's focused SigV4 verification module into
`nimbus-dynamodb`, keep it isolated from server composition, test against
multiple SDKs (Rust, JS v3, Python boto3), and ship lookup-only mode for D0-D6
so verification can land progressively.

**R5: Pagination state semantics (High, D2).** `LastEvaluatedKey` must be
stable across requests and survive parallel reads. Mitigation: encode the
Nimbus cursor token as opaque base64; never expose internal cursor
structure; cover with parity tests against DynamoDB Local for
Scan/Query/BatchGet workflows.

**R6: GSI eventual-consistency divergence (Medium, D4).** Real DynamoDB
GSIs are eventually consistent; Nimbus indexes are strongly consistent.
Some workloads depend on the GSI staleness window; others don't. Mitigation:
document the divergence; accept the upgrade; consider a configurable
artificial-delay knob (ExtendDB ships one) only if a customer needs to test
GSI lag behavior.

**R7: Stream shard model divergence (Medium, D5).** Real DynamoDB shards
split based on throughput; Nimbus emits a single shard per stream.
Mitigation: document the divergence; emit a stable shard ID; non-
throughput-bound workloads should be unaffected. Multi-shard model can be
added later if needed.

**R8: TransactWriteItems atomicity under contention (Medium, D3).** Up to
100 ops across tables must be atomic. Mitigation: route through the
engine's existing transaction session manager (proven for MongoDB
multi-document transactions); `TransactionCanceledException` returns
per-op `CancellationReasons`.

**R9: Reserved-name collision with Nimbus internals (Low, D0).** Nimbus
uses `_pk` and `_sk` internally to preserve partition/sort key values.
Customer items with those attribute names would collide. Mitigation:
reject incoming items with `_pk`/`_sk`/`_nimbus_*` attribute names at the
adapter boundary with a clear ValidationException.

**R10: TTL sweep timing semantics (Low, D6).** Real DynamoDB sweeps lazily
(up to 48 hours after expiration). Nimbus picks a configurable interval.
Mitigation: document the timing model; align default to ~hourly; expose
the interval as a config knob.

**R11: ExtendDB attribution hygiene (Low, ongoing).** ExtendDB is
Apache-2.0, fully compatible with the Nimbus Community License 1.0 — direct
code reuse is allowed without legal review. The only ongoing risk is
attribution drift: Apache-2.0 §4 requires preserving the license header on
copied files, recording material modifications, and acknowledging upstream
in a `NOTICE` file. Mitigation: when copying ExtendDB code, retain its
Apache-2.0 header verbatim, add or update a top-of-file
"Modified from ExtendDB by Nimbus contributors, YYYY-MM-DD" banner, and
maintain a single repo-root `NOTICE` file with one ExtendDB entry covering
all copied/derived files (no per-file `NOTICE` fan-out needed). Add an
audit step to the hardening-plan follow-on that walks every Apache-2.0
header in the tree and confirms `NOTICE` coverage.

## Phase Status Ledger

| Phase | Status | Context budget | Start condition | Done when |
|-------|--------|----------------|-----------------|-----------|
| D0: Wire envelope, AttributeValue bridge, control plane | `pending` | 6-8 context windows | Plan promoted to `in_progress` and server extraction completion gates green | `nimbus-dynamodb` compiles as a concrete crate; server listener composition works; AWS CLI control plane (create/describe/list/update/delete) succeeds end-to-end |
| D1: Single-item ops + expression language | `pending` | 8-12 context windows | D0 is `done` | PutItem/GetItem/UpdateItem/DeleteItem with all expression kinds succeed; parity diff clean for covered shapes |
| D2: Query and Scan | `pending` | 5-7 context windows | D1 is `done` | Query/Scan with pagination succeed end-to-end; parity diff clean |
| D3: Batch and transactional ops | `pending` | 4-6 context windows | D1 is `done` | BatchGet/BatchWrite/TransactGet/TransactWrite succeed; failure paths return correct exceptions |
| D4: Secondary indexes (GSI, LSI) | `pending` | 5-7 context windows | D2 is `done` | CreateTable with GSI/LSI; UpdateTable adding/removing GSI; Query/Scan via IndexName |
| D5: Streams | `pending` | 4-6 context windows | D1 is `done` | DescribeStream/GetShardIterator/GetRecords work; insert/update/delete events appear with correct StreamViewType |
| D6: TTL, tagging, control-plane completion | `pending` | 3-5 context windows | D1 is `done` | TTL enable/describe/expire and tag CRUD work end-to-end |
| D7: SigV4 strict mode | `pending` | 3-5 context windows | D0 is `done` | SigV4Strict mode verifies SDK requests and rejects malformed signatures |
| D8: SDK + parity tests + verification harness | `pending` | 6-9 context windows | D1-D6 are `done` | `@nimbus/dynamodb` selftest passes; parity classification report covers ≥80% of SDK shapes for T0-T6; official SDK smoke suites pass for supported operation families; harness includes the five DynamoDB cases |
| D9: Enterprise readiness, reliability, and performance closeout | `pending` | 4-7 context windows | D0-D8 are `done` | Feature coverage table, SDK matrix, reliability proof, tenant-isolation proof, soak report, benchmark baseline, and enterprise-readiness doc are committed |

## Roadmap Items

Each item is intended to fit in one focused context window. If an item
cannot fit with the relevant source context, implementation, tests, and
checkpoint update loaded at once, split it before starting.

### D0 Work Queue: Wire Envelope, AttributeValue Bridge, Control Plane

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D0.0 Scaffold `nimbus-dynamodb`, wire `extenddb-core`, and vendor SigV4 | `pending` | none | `crates/nimbus-dynamodb` added as a workspace member with package name `nimbus-dynamodb`. `extenddb-core` added as git rev pin in workspace `Cargo.toml` and pulled into `crates/nimbus-dynamodb/Cargo.toml`. `crates/nimbus-server/Cargo.toml` depends only on `nimbus-dynamodb`, not on `extenddb-core`. SigV4 files from `crates/auth/src/sigv4/` vendored verbatim into `crates/nimbus-dynamodb/src/auth/sigv4/` with Apache-2.0 headers preserved. Repo-root `NOTICE` file created (or updated) with one ExtendDB entry. The pinned ExtendDB sha recorded in execution log. | `cargo check -p nimbus-dynamodb` clean; `cargo check -p nimbus-server` clean; `cargo tree -p nimbus-dynamodb --edges normal` includes pinned `extenddb-core`; `cargo tree -p nimbus-server --edges normal` shows `nimbus-dynamodb` and not `extenddb-core`; `make deny` clean. |
| D0.1 HTTP router scaffold, server listener composition, and X-Amz-Target dispatch | `pending` | D0.0 | `nimbus-dynamodb` exposes an axum router factory that accepts explicit capabilities and POST `/` dispatch by `X-Amz-Target`. `nimbus-server` owns port 8000 bind/spawn/shutdown through `ServeOptions`. Unknown/missing target returns DynamoDB error envelope. | Focused `nimbus-dynamodb` tests for dispatch matrix, missing target, malformed body; focused `nimbus-server` test for optional listener wiring. |
| D0.2 AttributeValue bridge | `pending` | D0.1 | `extenddb_core::types::AttributeValue` ↔ Nimbus value roundtrip for S/N/B/M/L/SS/NS/BS/BOOL/NULL using typed-scalar metadata. Empty sets and empty top-level docs rejected (delegated to `extenddb_core::validation`). | Roundtrip tests for every variant; number precision tests; binary base64 tests. |
| D0.3 Composite primary-key encoding | `pending` | D0.2 | Reversible `(pk, sk)` ↔ DocumentId encoding via base64url segments. Oversize keys rejected with ValidationException. `_pk`/`_sk` body fields preserved. | Property tests across the Unicode plane; size-limit rejection tests. |
| D0.4 Error envelope and code mapping | `pending` | D0.1 | Errors return HTTP 4xx/5xx with `{ "__type": "...", "message": "..." }` envelope. Shared error taxonomy maps to DynamoDB codes. | Tests for every mapped error class. |
| D0.5 Tenant resolution from access key | `pending` | D0.1 | Access-key prefix or configured binding resolves to a Nimbus tenant. Unknown key rejected with AccessDeniedException. | Tests for known/unknown key, multiple tenants. |
| D0.6 Control plane: Create/Describe/Delete/List/UpdateTable | `pending` | D0.3, D0.5 | CreateTable accepts KeySchema and AttributeDefinitions; DescribeTable returns the TableDescription; DeleteTable removes; ListTables paginates; UpdateTable handles StreamSpecification and table-class fields (GSI deferred to D4). | AWS CLI control-plane workflow succeeds end-to-end. |
| D0.7 DescribeEndpoints and DescribeLimits | `pending` | D0.6 | DescribeEndpoints returns the Nimbus listener URL; DescribeLimits returns stubbed account/table limits. | Focused tests for both shapes. |
| D0.8 SigV4 lookup-only auth | `pending` | D0.5 | Authorization header parsed via vendored `auth/sigv4/parse.rs`; access key extracted; signature is not verified yet (deferred to D7). Tenant principal threaded through dispatch. | Tests for valid/missing auth header, access-key extraction. |
| D0.9 Optional `nimbus-adapters` facade export | `pending` | D0.1 | `crates/nimbus-adapters` adds a default-off `dynamodb` feature that depends on and re-exports `nimbus-dynamodb`. `nimbus-server` continues to depend directly on `nimbus-dynamodb`. | `cargo check -p nimbus-adapters --features dynamodb`; `cargo tree -p nimbus-adapters --edges normal` has no default adapter deps; `cargo tree -p nimbus-server --edges normal` has no `nimbus-adapters`. |

### D1 Work Queue: Single-Item Operations And Expression Language

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D1.1 Expression shim wiring | `pending` | D0 done | `expression.rs` shim adapts `extenddb_core::expression::{tokenizer, parser, evaluator, update_evaluator, key_condition, projection, resolver}` to operate on Nimbus document values via the AttributeValue bridge. ExpressionAttributeNames / ExpressionAttributeValues resolution flows through the upstream resolver. | Smoke test that a representative ConditionExpression, UpdateExpression, ProjectionExpression, and KeyConditionExpression each parse and evaluate end-to-end against an in-memory item. |
| D1.2 ConditionExpression integration | `pending` | D1.1 | All comparison, logical, and function operators (`attribute_exists`, `attribute_not_exists`, `attribute_type`, `begins_with`, `contains`, `size`) evaluate correctly against Nimbus-stored items. Errors map to `ConditionalCheckFailedException`. | Coverage tests for every operator and function; parity classification clean against DynamoDB Local. |
| D1.3 UpdateExpression integration | `pending` | D1.1 | All actions (SET with `if_not_exists`/`list_append`/arithmetic, REMOVE, ADD on numeric and set, DELETE on set) apply correctly to Nimbus documents and respect the AttributeValue bridge. | Tests for each action kind and combination. |
| D1.4 ProjectionExpression integration | `pending` | D1.1 | Path-based field selector with dot/bracket notation works against Nimbus documents. | Tests for nested paths and array indexing. |
| D1.5 PutItem | `pending` | D1.2, D1.4 | PutItem with ConditionExpression, ReturnValues (NONE/ALL_OLD). ConditionalCheckFailedException with optional Item field. | AWS SDK PutItem succeeds; parity diff clean. |
| D1.6 GetItem | `pending` | D1.4 | GetItem with ProjectionExpression and ConsistentRead flag (accept-and-ignore). | AWS SDK GetItem succeeds; parity diff clean. |
| D1.7 DeleteItem | `pending` | D1.2 | DeleteItem with ConditionExpression and ReturnValues (NONE/ALL_OLD). | AWS SDK DeleteItem succeeds; parity diff clean. |
| D1.8 UpdateItem | `pending` | D1.3 | UpdateItem with full UpdateExpression action support, ConditionExpression, all four ReturnValues modes. ADD numeric maps to shared FieldTransformOperation; complex actions execute as RMW within the atomic write batch. | AWS SDK UpdateItem succeeds for every action kind; parity diff clean. |

### D2 Work Queue: Query And Scan

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D2.1 KeyConditionExpression and Query | `pending` | D1.4 | KeyConditionExpression compiles to a primary-key partition-equals-plus-optional-sort-range query. ScanIndexForward, Limit, ExclusiveStartKey/LastEvaluatedKey pagination work. | AWS SDK Query succeeds with pagination; parity diff clean. |
| D2.2 FilterExpression for Query | `pending` | D2.1 | FilterExpression applies after key selection. Select modes (ALL_ATTRIBUTES, SPECIFIC_ATTRIBUTES, COUNT) work. | Tests for filter + projection composition. |
| D2.3 Scan with FilterExpression and pagination | `pending` | D1.4 | Scan iterates a full table or segment with FilterExpression, ProjectionExpression, Limit, ExclusiveStartKey. | AWS SDK Scan succeeds; parity diff clean. |
| D2.4 Parallel Scan segments | `pending` | D2.3 | Segment/TotalSegments parameters partition the scan deterministically. | Tests for parallel scan correctness across all segments. |

### D3 Work Queue: Batch And Transactional Operations

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D3.1 BatchGetItem | `pending` | D1.6 | Fan-out across ≤100 keys; Responses + UnprocessedKeys envelope. | AWS SDK batchGet succeeds; parity diff clean. |
| D3.2 BatchWriteItem | `pending` | D1.5, D1.7 | Fan-out across ≤25 PutRequest/DeleteRequest; UnprocessedItems envelope. Per-op failures do not roll back successes. | AWS SDK batchWrite succeeds; parity diff clean. |
| D3.3 TransactGetItems | `pending` | D1.6 | ≤100 reads via engine transaction session manager for snapshot consistency. | AWS SDK transactGet succeeds; parity diff clean. |
| D3.4 TransactWriteItems | `pending` | D1.5, D1.7, D1.8 | ≤100 ops (Put/Update/Delete/ConditionCheck) atomic via engine transaction session manager. Failure returns TransactionCanceledException with CancellationReasons. | AWS SDK transactWrite succeeds; conflict path returns correct exception; parity diff clean. |

### D4 Work Queue: Secondary Indexes

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D4.1 LSI definitions at CreateTable | `pending` | D0.6 | LSI declared at CreateTable; mapped to Nimbus secondary indexes. LSI immutable after creation (match real DynamoDB rejection of LSI updates). | Tests for LSI creation and Query targeting. |
| D4.2 GSI definitions and UpdateTable Create/Update/Delete | `pending` | D0.6 | GSI CRUD via `GlobalSecondaryIndexUpdates`. Index status reported in DescribeTable. | AWS SDK GSI CRUD workflow succeeds; parity diff clean. |
| D4.3 Projection types and index-targeted Query/Scan | `pending` | D4.1, D4.2, D2.1, D2.3 | KEYS_ONLY/INCLUDE/ALL projection applied at response shaping. `IndexName` parameter routes Query/Scan to the index. | Tests for each projection type and index-targeted read. |
| D4.4 GSI ConsistentRead divergence decision | `pending` | D4.3 | Decision recorded in `docs/adapters/dynamodb/divergences.md`: either match DynamoDB's ValidationException rejection of `ConsistentRead=true` on a GSI Query, or accept-and-serve consistently as a Nimbus upgrade. | Test for the chosen behavior; divergence doc entry. |

### D5 Work Queue: Streams

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D5.1 StreamSpecification at CreateTable/UpdateTable | `pending` | D0.6 | StreamEnabled and StreamViewType (KEYS_ONLY/NEW_IMAGE/OLD_IMAGE/NEW_AND_OLD_IMAGES) recorded per table. | Tests for stream-enabled table description. |
| D5.2 DescribeStream and shard model | `pending` | D5.1 | Single-shard stream description per enabled table; shard ID stable. | Test for shape; divergence doc entry for single-shard model. |
| D5.3 GetShardIterator | `pending` | D5.2 | TRIM_HORIZON/LATEST/AT_SEQUENCE_NUMBER/AFTER_SEQUENCE_NUMBER iterator types. Iterator format opaque to clients. | Tests for each iterator type. |
| D5.4 GetRecords with StreamViewType shaping | `pending` | D5.3, D1.5, D1.7, D1.8 | GetRecords returns Records (≤1000) and NextShardIterator. Records shape matches the configured StreamViewType. | Tests for each StreamViewType against insert/update/delete event types. |
| D5.5 ListStreams and retention | `pending` | D5.2 | ListStreams enumerates active streams (optionally filtered by TableName); records evicted past retention window. | Tests for enumeration and eviction. |

### D6 Work Queue: TTL, Tagging, Control-Plane Completion

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D6.1 UpdateTimeToLive and DescribeTimeToLive | `pending` | D0.6 | TTL attribute name enable/disable; descriptive response. | Tests for enable/disable/describe roundtrip. |
| D6.2 TTL sweeper integration | `pending` | D6.1, D1.7 | Periodic engine-owned task deletes items whose TTL attribute is past current epoch seconds. Sweep interval configurable. | Tests for expired-item sweep; eventual delete event in the stream. |
| D6.3 Tagging surface | `pending` | D0.6 | TagResource/UntagResource/ListTagsOfResource over adapter-local tag store. | Tests for tag CRUD roundtrip. |

### D7 Work Queue: SigV4 Strict Mode

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D7.1 Canonical request and derived-key chain | `pending` | D0.8 | SigV4 canonical request matches SDK; derived-key chain matches; signature comparison clean. | Tests against signed requests from aws-sdk-rust/aws-sdk-js-v3/boto3. |
| D7.2 Strict-mode toggle and signature rejection | `pending` | D7.1 | SigV4Strict mode rejects malformed signatures with InvalidSignatureException. Request expiration enforced. | Tests for invalid signature, expired request, missing header. |
| D7.3 Nimbus-native access-key management | `pending` | D7.2 | Surface for configuring access key, secret, region, tenant binding. Persisted in Nimbus storage. | Tests for key configuration and rotation. |

### D8 Work Queue: SDK Package, Parity Tests, Verification Harness

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D8.1 @nimbus/dynamodb package scaffold | `pending` | D1 done | `packages/dynamodb` builds as `@nimbus/dynamodb` with ESM/CJS/types. | Build, typecheck, export-map tests. |
| D8.2 SDK integration and connection helpers | `pending` | D8.1 | Endpoint-defaulted `DynamoDBClient`; connection-string builder; tenant selection. Smoke selftest against a local Nimbus server. | Selftest passes. |
| D8.3 Parity-test runner foundation | `pending` | D1 done | Runner spins up Nimbus + DynamoDB Local (Docker) and runs the same scenario list against both, diffing responses. | Runner executes the seeded scenario set with classification report. |
| D8.4 Parity-test corpus T0-T3 | `pending` | D8.3, D3 done | Scenarios for control plane, single-item ops, query/scan, batch/transact. Classification report ≥80% pass on covered shapes. | Classification report committed. |
| D8.5 Parity-test corpus T4-T6 | `pending` | D8.3, D6 done | Scenarios for GSI/LSI, streams, TTL, tagging. | Classification report committed. |
| D8.6 ExtendDB parity comparison | `pending` | D8.3 | ExtendDB run alongside DynamoDB Local when the local checkout builds. Divergences classified as `accept-extenddb-divergence` where Nimbus matches ExtendDB but not real DynamoDB. If ExtendDB cannot build, the failure and next action are recorded instead of silently skipping the column. | Classification report includes ExtendDB column or a recorded build/run failure with command output. |
| D8.7 Verification harness integration | `pending` | D8.3 | Five DynamoDB harness cases added to PR and nightly lanes (handshake/control-plane, item-CRUD, query-scan, transact, streams). | `cargo test -p nimbus-server verification_harness_pr` includes all five. |
| D8.8 External canary suite runner | `pending` | D8.3 | Add a path-referenced canary registry for large external suites. Initial entries cover ExtendDB Python/boto3 suites, ExtendDB Rust SDK suite, AWS CLI suite, and official SDK canary projects. Suites are pinned by path + commit/version and run against Nimbus by endpoint override. | Canary report records suite name, command, environment, SDK, upstream SHA/version, pass/fail/skip counts, artifacts, and lane (`pr`, `nightly`, or `manual`). |

### D9 Work Queue: Enterprise Readiness, Reliability, And Performance

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D9.1 Feature-parity coverage table | `pending` | D8.4, D8.5 | `docs/adapters/dynamodb/feature-coverage.md` lists every T0-T7 operation, modeled request/response fields, modeled exceptions, pagination shape, idempotency fields, and status (`implemented`, `classified-divergence`, `unsupported-deferred`). | Generated coverage table committed; 100% of supported operations have a row; 0 unclassified fields/exceptions for implemented operations. |
| D9.2 External suite quality audit and official SDK matrix | `pending` | D8.2, D8.4, D8.5, D8.8, D7.2 | Audit every accepted external suite for license, endpoint-overridden target support, real-DynamoDB/DynamoDB-Local/ExtendDB/Nimbus targetability, SDK used, operation coverage, skip policy, cleanup, determinism, runtime cost, and credential safety. AWS CLI, JS v3, Rust, Python, and practical Java clients each run supported operation-family scenarios against Nimbus with endpoint override and SigV4 mode recorded. | `docs/adapters/dynamodb/sdk-compatibility.md` records suite-quality audit, exact versions/SHAs, pass/fail/skip counts, auth mode, endpoint URL, and divergences; no supported operation fails through an official SDK due to Nimbus protocol drift. |
| D9.3 Failure injection and cancellation proof | `pending` | D8.4 | Malformed input, oversized payloads, invalid keys/tokens, dropped connections, pre-commit cancellation, post-commit cancellation, engine errors, storage errors, and timeout paths fail closed without panics or partial-success envelopes. | Focused failure-injection tests pass; report records 0 panics and 0 unclassified 5xx responses. |
| D9.4 Tenant isolation and auth isolation proof | `pending` | D7.3, D8.4, D8.5 | Two or more tenants/access keys cannot cross-read, cross-write, list, stream, tag, TTL-configure, or infer each other's tables. Wrong access key, wrong signature, and wrong tenant binding fail closed. | Tenant-isolation conformance tests pass; report records 0 cross-tenant visibility or mutation violations. |
| D9.5 Mixed-workload soak test | `pending` | D9.2, D9.3, D9.4 | Mixed SDK traffic runs for a fixed duration across reads, writes, conditional writes, queries, streams, TTL/tag metadata, and auth failures. | Soak report records duration, request count, error count by class, task count before/after, memory high-water mark, and 0 panics/task leaks/unclassified failures. |
| D9.6 Performance benchmark baseline | `pending` | D8.4, D8.5 | Benchmarks cover PutItem, GetItem, UpdateItem, Query, Scan, BatchGetItem, BatchWriteItem, TransactWriteItems, and Streams GetRecords across documented item sizes and concurrency levels. | Benchmark report records p50/p95/p99 latency, throughput, dataset size, item size, concurrency, storage backend, host hardware, commit SHA, and initial non-regression thresholds. |
| D9.7 Enterprise-readiness closeout document | `pending` | D9.1-D9.6 | `docs/adapters/dynamodb/enterprise-readiness.md` summarizes feature coverage, SDK matrix, reliability proof, tenant isolation proof, benchmark baseline, known divergences, deferred features, and operational limits. | Document committed; execution log links every proof artifact; final stale-reference/dependency-graph checks pass. |

## Source Evidence Map

| Source | Location | What it provides |
|--------|----------|-----------------|
| DynamoDB API reference | AWS docs | Operation request/response shapes, AttributeValue spec |
| DynamoDB JSON wire format | AWS docs | Wire envelope, error envelope, X-Amz-Target convention |
| AWS SigV4 spec | AWS docs | Canonical request, derived-key chain, signing algorithm |
| AWS DynamoDB Local | `amazon/dynamodb-local` Docker image | Behavioral ground truth |
| ExtendDB | `https://github.com/ExtendDB/extenddb` cloned at `/Users/jack/src/github.com/ExtendDB/extenddb` | AWS-blessed open reference adapter, divergence documentation (`docs/differences-from-dynamodb.md`), parity-test corpus (`tests/test_*.py`), structural twin (`crates/{server,auth,core,engine,storage,storage-postgres}`) |
| ExtendDB test strategy | `/Users/jack/src/github.com/ExtendDB/extenddb/tests/README.md`, `/Users/jack/src/github.com/ExtendDB/extenddb/tests/python/README.md`, `/Users/jack/src/github.com/ExtendDB/extenddb/docs/design/09-testing.md`, `/Users/jack/src/github.com/ExtendDB/extenddb/external-suites.sample.toml` | Dual-target testing against ExtendDB or real DynamoDB, boto3 primary suite, Rust SDK integration suite, external-suite-by-path pattern, and quality criteria for reference suites |
| `aws-sdk-rust` | `https://github.com/awslabs/aws-sdk-rust` | Canonical Rust client; SigV4 reference |
| `aws-sdk-js-v3` | `https://github.com/aws/aws-sdk-js-v3` | Canonical JS client; `@nimbus/dynamodb` SDK target |
| `boto3`/`botocore` | `https://github.com/boto/boto3` | Secondary parity target |
| `moto` (DynamoDB) | `https://github.com/getmoto/moto` | In-memory DynamoDB mock; edge-case reference |
| ExtendDB SigV4 module | `/Users/jack/src/github.com/ExtendDB/extenddb/crates/auth/src/sigv4/` | Vendored server-side SigV4 verification primitive |
| MongoDB adapter (completed) | `crates/nimbus-mongodb/` plus server listener shim | Structural twin: concrete adapter crate + server-owned listener composition |
| MongoDB adapter plan (archived) | `docs/plans/archive/mongodb-adapter-plan.md` | Template for this plan's shape |
| MongoDB hardening plan (archived) | `docs/plans/archive/mongodb-adapter-hardening-plan.md` | Audit-finding pattern to anticipate for hardening-plan follow-on |
| Firebase adapter (completed) | `crates/nimbus-firebase/` | Typed-scalar metadata precedent |
| Server extraction completion plan | `docs/plans/server-crate-extraction-completion-plan.md` | Current crate naming and facade rules for concrete adapter crates |
| Runtime-capability adapter boundary baseline | `docs/plans/archive/runtime-capability-adapter-boundary-plan.md` | Adapter/runtime ownership baseline |

## Execution Log

| Date | Item | Status | Description | Verification |
|------|------|--------|-------------|--------------|
| 2026-05-26 | — | `pending` | Plan created. Awaiting promotion to `in_progress` after the active architecture gates and release-readiness plan permit adapter-surface expansion. | — |
| 2026-05-26 | — | `pending` | ExtendDB cloned locally at `/Users/jack/src/github.com/ExtendDB/extenddb` (Apache-2.0, Rust workspace with `crates/{auth,bin,core,engine,server,storage,storage-postgres}`, Python parity corpus under `tests/`, divergence catalogue at `docs/differences-from-dynamodb.md`). Plan references updated to point at the local checkout. | `ls /Users/jack/src/github.com/ExtendDB/extenddb` |
| 2026-05-26 | — | `pending` | Surveyed ExtendDB crate-by-crate for reuse: `extenddb-core` (12,587 LOC, zero workspace deps, pure sync) selected as direct git-rev dependency to inherit AttributeValue, expression language, type model, validation, and error taxonomy. `extenddb-auth/sigv4/` (4 files, ~760 LOC) selected for verbatim vendoring under `auth/sigv4/`. `extenddb-storage` traits + `extenddb-engine` handlers + `extenddb-server` + `extenddb-storage-postgres` rejected (too coupled to ExtendDB's StorageEngine trait and PostgreSQL backend; adapter cost exceeds reimplementation cost). Plan updated with "Upstream Crate Reuse" section, dependency-wiring D0.0 added to the work queue, dependency-management list aligned, module structure annotated `[uses extenddb-core]` where appropriate, D1.1–D1.4 expression items converted from "build parser/evaluator" to "wire upstream parser/evaluator into Nimbus shim". | `find /Users/jack/src/github.com/ExtendDB/extenddb/crates -name '*.rs' \| xargs wc -l` = 49,397 total; per-crate breakdown recorded in plan. |
| 2026-05-28 | — | `pending` | Updated the plan after the adapter crate extraction and naming change. DynamoDB now targets the concrete `crates/nimbus-dynamodb` crate, keeps DynamoDB protocol dependencies out of `nimbus-server`, permits `nimbus-adapters` only as a default-empty optional facade, moves parity/spec tests under the concrete adapter crate, and adds crate-boundary verification gates. | Stale package/path audit recorded no old adapter-suffix crate names and no old server-local parity-test path. |
| 2026-05-28 | — | `pending` | Added enterprise compatibility and production-readiness gates: official SDK matrix, botocore model coverage, feature-parity coverage table, failure injection, tenant isolation proof, soak testing, performance baselines, and an external test reuse policy distinguishing source references, translated scenarios, and vendored fixtures. | `git diff --check -- docs/plans/dynamodb-adapter-plan.md`; stale old-path audit remained clean. |
| 2026-05-28 | — | `pending` | Re-reviewed the freshly updated ExtendDB checkout (`5f5e511`) and changed the plan from "small vendored fixtures only" to a Node-LTS-style external canary policy. ExtendDB has dual-target Python/boto3 tests, a Rust SDK integration test crate, external suite registry precedent, real-DynamoDB validation mode, and explicit supported-operation/difference docs. Large suites should run by pinned path/version against Nimbus, with selected scenarios translated into PR-stable Nimbus tests. | `git pull --ff-only` in ExtendDB; `find tests -type f`; `rg -n "def test_" tests \| wc -l` = 729; `rg -n "#\\[(tokio::test\|test)\\]" tests/rust/src \| wc -l` = 348; inspected `tests/README.md`, `tests/python/README.md`, `tests/rust/Cargo.toml`, `tests/rust/src/test_base.rs`, `docs/design/09-testing.md`, `external-suites.sample.toml`. |
