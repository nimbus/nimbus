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
| ExtendDB | `https://github.com/ExtendDB/extenddb` (Apache-2.0) | `/Users/jack/src/github.com/ExtendDB/extenddb` (cloned 2026-05-26) | AWS-maintained DynamoDB-compatible adapter. Reference implementation, parity-test target, divergence catalogue (`docs/differences-from-dynamodb.md`). **Source of `extenddb-core` direct dependency** (AttributeValue, expression language, type model, validation, error taxonomy) and **source of vendored SigV4 module** (5 files from `crates/auth/src/sigv4/`). See "Upstream Crate Reuse" below for the per-crate decision matrix. |
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
  parity-test seeds for `crates/nimbus-server/tests/dynamodb_spec/`.

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

- **Plan status:** `in_progress` (promoted 2026-05-29 at D0.0a; both promotion
  conditions satisfied — see below)
- **Active item:** D0.1b (promote `nimbus-core` typed-scalar for DynamoDB).
  D0.0a, D0.0, D0.1 are `done`.
- **Local-dev note:** `nimbus-server` needs the `nimbus-ui` dist to compile via
  direct cargo; in this worktree run `npm ci` + `make build-ui` once (done
  2026-05-29) — the dist is gitignored, so re-run after a fresh checkout.
- **Working branch:** `dynamodb-adapter` worktree at `../nimbus-dynamodb-adapter`.
- **Control item:** `scripts/verify-dynamodb-adapter.sh` (the `/goal`
  control-plane gate; scaffolded in D0.0a, proven green in D9.7). See
  [Goal Control Plane](#goal-control-plane) and [Completion Gate](#completion-gate).
- **Delivery:** runs on the `dynamodb-adapter` worktree branch (created in
  D0.0a) and lands via a PR to `main` at D9.7 — **never pushed to `main`
  directly**. See [Branch, CI, and PR workflow](#branch-ci-and-pr-workflow).
- **Status values:** `pending`, `in_progress`, `done`, `blocked`
- **Primary source of truth:** this file plus the current git worktree.
- **Checkpoint rule:** every work session that changes implementation state
  must update the roadmap item status, the phase status ledger, and the
  execution log before stopping.

Promote this plan from `pending` to `in_progress` only after the current
server crate extraction completion gates remain green and the release-readiness
plan does not require freezing the adapter surface. **Both conditions are
satisfied as of 2026-05-29:**

1. Server crate extraction —
   `docs/plans/archive/server-crate-extraction-completion-plan.md` closed
   2026-05-28 with `bash scripts/verify-server-crate-extraction-completion.sh`
   green.
2. No adapter-surface freeze — `docs/plans/final-nimbus-release-readiness-plan.md`
   is **completed** (a one-shot `v0.1.32` release cut, not a standing freeze) and
   imposes no adapter-surface freeze. Its single-binary-default principle ("the
   base `nimbus` binary must still work without the optional adapter installed")
   is already honored: the DynamoDB listener is optional and default-disabled,
   so the work is purely additive.

The promotion gate is therefore clear: D0.0a may flip the plan to `in_progress`
when the `/goal` runs. (If a new release cut is in flight when the closeout PR is
ready, hold the merge — not the branch work — until that release window closes.)

## Goal Control Plane

This plan is built to run under `/goal` autonomous execution. The stop
condition is machine-checkable, not prose-judged: the run is complete only when
every roadmap item is `done` and the control-plane verifier exits 0.

### Objective

When this plan is activated as a goal, use this objective:

Complete `docs/plans/dynamodb-adapter-plan.md` autonomously end to end. Success
means Nimbus ships a `nimbus-dynamodb` concrete adapter crate that serves the
DynamoDB HTTP/JSON wire protocol on its own port, covers compatibility tiers
T0–T7 (control plane, single-item ops + expressions, Query/Scan, batch +
transactions, secondary indexes, Streams, TTL + tagging, SigV4 strict mode) plus
the `@nimbus/dynamodb` SDK package (T8), proves every supported operation through
at least one official SDK client (AWS CLI, JS v3, Rust, Python) against an
endpoint override, classifies every divergence from DynamoDB Local / ExtendDB in
`docs/adapters/dynamodb/divergences.md` with a regression test, holds tenant
isolation across at least two access keys, commits failure-injection, soak, and
performance-baseline evidence, ships the enterprise-readiness closeout doc, keeps
DynamoDB protocol dependencies out of `nimbus-server`, and passes
`bash scripts/verify-dynamodb-adapter.sh` with `N passed, 0 failed`. Work one
roadmap item at a time in dependency order, mark exactly one item `in_progress`,
and record commands plus observed counts in the execution log before closing each
item. Treat any failing completion-gate condition as a stop condition, not a TODO.

### Branch, CI, and PR workflow

This wave runs on an **isolated worktree branch and lands via a PR to `main` — it
is never pushed to `main` directly** (same model as the completed
`node-dbus-binding` wave).

- **Isolation.** All DynamoDB work lands on a dedicated `dynamodb-adapter`
  worktree branch created at D0.0a, e.g.
  `git worktree add ../nimbus-dynamodb-adapter -b dynamodb-adapter`. Run every
  roadmap item from that worktree. `main` is actively churned by concurrent
  work; the branch keeps this wave conflict-free until the closeout PR. Commit
  per roadmap item (checkpoint the plan state in the same commit).
- **CI is verification, not local compile.** The heavy proof — dual-target
  parity against DynamoDB Local (Docker) and ExtendDB, the official-SDK matrix
  (AWS CLI / JS v3 / Rust / Python), and the nightly external-suite lanes —
  runs in CI, not on the dev host. A green local `make check`/`clippy` is
  necessary but not sufficient; the real evidence is a **green branch CI run**
  on the pushed `dynamodb-adapter` branch, captured by `gh run`.
- **PR as the last step.** After D0.0a–D9.7 land, the local verifier is
  `N passed, 0 failed`, and branch CI is green, open a PR
  `dynamodb-adapter → main` as the closeout action (D9.7). Never push DynamoDB
  commits directly to `main`.
- **Base is clean.** Unlike NDB, this wave's base crates (`nimbus-server`,
  `nimbus-tenant`, `nimbus-adapters`) are already on `main` (server extraction
  closed 2026-05-28), so the branch should reach **full** CI green with no
  base-branch-red waiver. If a base-branch failure is ever inherited, attribute
  it per the repo's base-branch-CI rule rather than treating the branch as
  broken.
- **Verifier vs. process.** The Completion Gate verifier is the durable
  machine-checkable gate and stays passable on `main` after merge. The
  push → green-branch-CI → PR sequence is the integration *process* the `/goal`
  drives; it is enforced by the goal, not encoded as an extra verifier
  condition.

### Suggested goal prompt

```text
/goal Complete docs/plans/dynamodb-adapter-plan.md autonomously end to end on a dedicated worktree branch — never push to main directly. First (D0.0a) create the worktree branch: `git worktree add ../nimbus-dynamodb-adapter -b dynamodb-adapter`, and do all work from there. Work one roadmap item at a time in dependency order (D0.0a control-plane scaffold first, then D0.0..D9.7), mark exactly one item in_progress, satisfy its completion gate, commit per item with the plan checkpoint, and record the commands run plus observed counts in the execution log before closing it. Ship the nimbus-dynamodb concrete adapter crate for DynamoDB tiers T0-T7 plus the @nimbus/dynamodb package, prove every supported operation through an official SDK client (AWS CLI / JS v3 / Rust / Python) against an endpoint override, classify every DynamoDB-Local/ExtendDB divergence in docs/adapters/dynamodb/divergences.md with a regression test, hold two-tenant isolation, and commit failure-injection, soak, performance-baseline, and enterprise-readiness evidence. Keep DynamoDB protocol dependencies out of nimbus-server. Done when every roadmap item is done, `bash scripts/verify-dynamodb-adapter.sh` exits 0 with "N passed, 0 failed", `cargo fmt --all --check` + `make clippy` + `make deny` + `make verify-third-party-attribution` + strict docs-reference validation + `git diff --check` all pass, the `dynamodb-adapter` branch is pushed and full CI is green on it, and a PR `dynamodb-adapter → main` is open (the final closeout action — do not merge it yourself).
```

The enumerated stop conditions the verifier enforces are listed under
[Completion Gate](#completion-gate). The verifier itself is scaffolded in D0.0a
(it must fail on every unimplemented gate the moment it is created) and proven
green in D9.7. Per the Branch, CI, and PR workflow above, the terminal action is
opening the `dynamodb-adapter → main` PR with branch CI green — not merging it.

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
- The MongoDB adapter proved the typed-scalar metadata pattern for preserving
  non-JSON value types, but the shared infrastructure **cannot be reused as-is**
  for DynamoDB. `nimbus_core::typed_scalar::TypedScalarValue`
  (`crates/nimbus-core/src/typed_scalar.rs`) is a closed enum whose variants are
  MongoDB/Firebase-shaped (`Timestamp`, `SpecialDouble`, `ObjectId`, `Binary`,
  `Decimal128`, `Regex`, `MongoTimestamp`, `MinKey`, `MaxKey`, `JavaScriptCode`).
  Only `Binary` maps cleanly to DynamoDB `B`. There is **no arbitrary-precision
  `N`** (`Decimal128` is a different repr/semantic) and **no Set types**
  (`SS`/`NS`/`BS`). Worse, `Document.typed_fields`
  (`crates/nimbus-core/src/document.rs:18`) is a flat
  `BTreeMap<String, TypedScalarValue>` keyed by top-level field name, so it
  **cannot carry a typed scalar nested inside a Map/List or inside a Set's
  members** — exactly the shape DynamoDB items take (MongoDB hardening finding L8
  documented this nesting loss). Reuse therefore requires a prerequisite
  cross-adapter promotion of the shared type infrastructure (new variants +
  nesting-capable representation), which touches the MongoDB and Firebase match
  arms. This is scheduled as its own roadmap item (D0.1b) gated **before** the
  AttributeValue codec (D0.2), not assumed as a freebie.
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
- **Storage layout is the shared Nimbus `documents(table_id, id)` table, not a
  physical table per DynamoDB table.** This is a deliberate divergence from
  ExtendDB, whose `storage-postgres` crate creates one UUID-named physical
  PostgreSQL table per logical table (`_ddb_<uuid>`) plus per-GSI/LSI physical
  tables. Three reasons keep Nimbus on the shared layout: (1) the cross-adapter
  data-sharing premise (Context → "Why DynamoDB") requires DynamoDB items to
  live in the same store every other adapter reads — per-table physical tables
  would fork that store; (2) the adapter targets every Nimbus backend
  (Postgres / MySQL / SQLite / libSQL / redb), while ExtendDB's per-table DDL is
  Postgres-only; (3) the table-name-reuse and rename-safety problems ExtendDB
  cites as its reason for UUID physical names are already solved in Nimbus by the
  stable `TableId` ULID catalog (`crates/nimbus-core/src/types.rs:103`,
  `crates/nimbus-storage/src/table_identity.rs`). This matches the MBA10
  decision (`docs/plans/archive/multi-backend-adapter-hardening-plan.md:84`),
  which considered ExtendDB's UUID-backed physical names and reserved per-table
  physical layout as a later, measured, backend-specific optimization behind
  `TableBackendLayout`. If a future benchmark proves the shared layout is a
  bottleneck for this adapter, that escape hatch — not a storage redesign — is
  the pre-decided path (see R12).

## Autonomous Execution Contract

This plan is designed for agent-driven execution with minimal human
intervention. Each roadmap item must be completable in a single context
window using only the plan, the git worktree, and the cloned reference repos.

### Startup Prompt

The autonomous objective and the `/goal` prompt are defined inline under
[Goal Control Plane](#goal-control-plane). D0.0a additionally writes
`docs/prompts/dynamodb-adapter-start.md` (modeled on the MongoDB startup prompt)
so the prompt is recoverable from a file as well as from this plan.

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
| `extenddb-core` | 13,810 (@`0448ca0`; was 12,587) | **Depend directly (git rev pin)** | Zero workspace deps (serde, serde_json, thiserror, base64, bigdecimal, time, uuid only), pure sync, no I/O. Ships `AttributeValue` with serde wire-format impls, the full expression language (~4,749 LOC non-test across tokenizer, parser, evaluator, update parser/evaluator, key condition, projection, resolver, plus a reserved-word catalogue of **573 reserved words in a 615-line file**), complete typed I/O envelopes (`PutItemInput/Output`, `Query`, `Scan`, `BatchWriteItem`, `TransactWriteItems`, `StreamRecord`), `DynamoDbError` taxonomy with AWS-fidelity error strings, a now-**1,440-LOC** `validation/mod.rs` (grew with the nesting-depth + key-validation fixes), limits and throttle types. All deps compatible with Nimbus's pinned versions. |
| `extenddb-auth/sigv4/` | ~773 | **Vendor the 5 source files** | `mod.rs` (13), `canonical.rs` (172), `parse.rs` (147), `signing_key.rs` (99), `verify.rs` (342). (The plan previously said 4 files — `mod.rs` was omitted.) Self-contained except for `axum::http::HeaderMap` (already a Nimbus dep) and `extenddb_core::error::DynamoDbError`. Preserve Apache-2.0 header per file; add ExtendDB to repo `NOTICE`. Vendoring avoids dragging in the policy module's IAM that Nimbus replaces with its own auth model. |
| `extenddb-auth/policy/` | 2,721 | **Skip** | IAM policy engine (statements, principals, condition operators). Nimbus has its own tenant/principal model. |
| `extenddb-storage` | 2,271 | **Reference only** | 6 RPITIT traits (`TableEngine`, `DataEngine`, `MetadataEngine`, `StreamEngine`, `WorkerStore`, `BackupEngine`), all `account_id`-scoped. Useful as a shape reference for what an item-store interface looks like; does not map onto Nimbus's `Service`, and its per-table-physical model is the storage layout Nimbus rejects (see Current Assessed State → storage layout). |
| `extenddb-engine` | 7,019 (was 6,730) | **Reimplement against `Service`** | Every handler takes `OperationContext { storage: Arc<dyn extenddb_storage::StorageEngine> }`. Reusing the handlers would require Nimbus storage to implement the full ExtendDB storage-engine surface (the 6 core RPITIT traits plus ~12 supporting stores — `CatalogStore`, `MetricsStore`, `RateLimitStore`, etc.) — more work than reimplementing the handlers, which are mostly validation + dispatch + serialize. The heavy lifting (expression evaluation, validation, AttributeValue codec) is in `extenddb-core` and is reused via that crate. |
| `extenddb-storage-postgres` | ~11,923 (was ~6,000) | **Skip** | PostgreSQL-specific **and architecturally divergent**: it maps each logical table to a UUID-named physical table (`_ddb_<uuid>`) with typed `pk` / `sk_s` / `sk_n` / `sk_b` columns plus per-GSI/LSI physical tables. Nimbus deliberately uses the shared `documents(table_id, id)` layout so the same data is reachable from every adapter (see Current Assessed State → storage layout and Key Architectural Decisions). Now ~2× the originally recorded size — reinforces the skip. |
| `extenddb-server` | 8,332 | **Skip** | Includes axum listener, management API (`server/src/management/*` IAM CRUD), web console. **The reusable X-Amz-Target dispatch shape is `extenddb_engine::dispatch()` at `crates/engine/src/lib.rs:292-387`** (one `match operation` over ~35 ops), NOT `server/src/handler.rs` (340 LOC, which only does metrics categorization before calling `dispatch`). Either way it is small enough to write fresh against Nimbus's `Service`. |
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
the moment D0.0 runs and record the sha in the execution log. Pin **at or after
`0448ca0`** (HEAD as of 2026-05-29): commits `c11fdb6`, `754f307`, `5ec827b`,
`9a1a1a6`, `7557eb1`, and `0448ca0` land DynamoDB-fidelity fixes (reversed key
conditions, no-op-upsert UpdateItem, omit-empty `Attributes`, malformed
`ExclusiveStartKey` handling, redundant-parens rejection, 32-level nesting-depth
limit) that Nimbus inherits for free through the dependency. An earlier pin would
force re-implementing them in the Nimbus shim.

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

Copy `crates/auth/src/sigv4/{mod.rs,canonical.rs,parse.rs,signing_key.rs,verify.rs}`
(all **5** files) into `crates/nimbus-dynamodb/src/auth/sigv4/` verbatim. Keep
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
`nimbus-server`, must not depend on `axum`, must not bind sockets, must not
accept `AppState`, and must expose narrow operation/dispatch entrypoints over
explicit capabilities such as `Arc<Service>`. This matches the established
convention: **no concrete adapter crate (`nimbus-mongodb`, `nimbus-firebase`,
`nimbus-convex`, `nimbus-cloud-functions`) depends on `axum` or owns a `Router`** —
each exposes protocol-translation/dispatch functions (e.g.
`nimbus_mongodb::commands::dispatch`) and `nimbus-server` owns all transport. The
DynamoDB X-Amz-Target surface is a single `POST /` plus a header switch, so the
adapter needs only a `dispatch(target, body, &Arc<Service>, &auth) -> response`
entrypoint, not a router; `nimbus-server` mounts that dispatch on the dedicated
port.

```
crates/nimbus-dynamodb/
├── Cargo.toml            # package name: nimbus-dynamodb (NO axum dependency)
├── src/
│   ├── lib.rs            # DynamoDbConfig, public API, dispatch entrypoint exports
│   ├── dispatch.rs       # X-Amz-Target -> handler dispatch over Arc<Service>; no axum, no socket bind
│   ├── wire.rs           # JSON request/response envelope, X-Amz-Target parsing, error envelope [uses extenddb-core error taxonomy]
│   ├── attribute_value.rs # extenddb_core::types::AttributeValue ↔ Nimbus value bridge (S/N/B/M/L/SS/NS/BS/BOOL/NULL roundtrip via typed-scalar metadata)
│   ├── key.rs            # partition+sort composite-key encoding, DocumentId mapping
│   ├── error.rs          # DynamoDbError → Nimbus error taxonomy mapping (both directions)
│   ├── auth/
│   │   ├── mod.rs        # AuthProvider entry, access-key → tenant resolution
│   │   └── sigv4/        # vendored from extenddb-auth/sigv4/ (5 files) — see Upstream Crate Reuse
│   │       ├── mod.rs
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
└── tests/                # adapter-local unit tests only (codec, key encoding, expression shim)
```

The end-to-end parity runner and SDK-shaped scenarios live under
`crates/nimbus-server/tests/dynamodb_spec/`, **not** in this crate. That mirrors
the existing `crates/nimbus-server/tests/mongodb_spec/` runner, whose executor
imports `nimbus_server::adapters_mongodb::listener` to spin up a real listener —
the same reason the DynamoDB parity runner (which drives a live HTTP/SigV4
endpoint through official SDK clients) must live where the listener composition
is. Keeping it in `nimbus-dynamodb/tests/` would force a `nimbus-server` dev-dep
and re-introduce the crate-boundary cycle this plan forbids.

Files may be split further per the modularity thresholds in `AGENTS.md`
(1500-line soft limit, 2000-line hard limit). New sub-modules should follow
concept-owned naming. The `expression.rs` shim is expected to stay small
(under 500 LOC) because all the heavy work is in `extenddb-core::expression`.

`crates/nimbus-server/src/adapters/dynamodb/` owns the transport composition,
mirroring `crates/nimbus-server/src/adapters/mongodb/{mod.rs,listener.rs}`. It
re-exports `DynamoDbConfig`, builds the small `axum` `POST /` route that reads
`X-Amz-Target` and calls `nimbus_dynamodb::dispatch(...)`, owns port
bind/spawn/shutdown, and calls
`nimbus_server::system_tenant::record_listener_state_async(&service, "dynamodb", "http", ...)`
(the step MongoDB performs at `construction.rs:167`). It must not contain
DynamoDB protocol parsing, AttributeValue conversion, expression evaluation,
SigV4 verification, operation dispatch, or parity-test logic — those stay in
`nimbus-dynamodb`.

### Boot Sequence Integration

The DynamoDB HTTP listener integrates into the server startup in
`crates/nimbus-server/src/construction.rs` (the real home of `ServeOptions` and
the `serve(listener, ServeOptions)` entrypoint — there is no
`serve_with_options`/`lib.rs` entrypoint; the plan's earlier references to those
names were wrong). Follow the MongoDB precedent at `construction.rs:63`
(`with_mongodb`) and `:164-191` (bind/spawn/abort):

1. Add `dynamodb_config: Option<nimbus_dynamodb::DynamoDbConfig>` to
   `ServeOptions` and a `.with_dynamodb(config)` fluent builder method (mirror
   `MongoDbConfig { bind_addr, auth }` with `DynamoDbConfig::new(port)` +
   `.with_auth(...)`).
2. In `serve(...)`, if `dynamodb_config` is `Some`, bind a separate
   `tokio::net::TcpListener` on the configured port (default 8000, matching
   DynamoDB Local), build the `axum` `POST /` route that calls
   `nimbus_dynamodb::dispatch`, and `tokio::spawn` it as a sibling task with an
   `Arc::clone(&service)` (matching `construction.rs:178`), then `.abort()` the
   handle after the primary HTTP server returns (matching `:189`). Call
   `record_listener_state_async(&service, "dynamodb", "http", ...)` before
   spawning.
3. In `crates/nimbus-bin/src/start/boot.rs`, add a `--dynamodb-port` CLI flag
   (default: disabled) that creates a `DynamoDbConfig` and passes it to
   `ServeOptions::with_dynamodb`. Note **no `--mongodb-port` flag exists today**,
   so there is no CLI precedent to copy — follow the
   `ServeOptions::new(...).with_*` composition shape at `boot.rs:156-173`.
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
- `axum` — used by **`nimbus-server`** for the `POST /` route + body/header
  extractors (already present there). `nimbus-dynamodb` does **not** depend on
  `axum`; it exposes a transport-agnostic `dispatch(...)` entrypoint. This
  matches every existing concrete adapter (none depend on `axum`).
- `base64` 0.22 (already present) — Binary attribute encoding.
- `serde_json` (already present) — JSON envelope.

`nimbus-dynamodb`'s workspace-crate deps are exactly `nimbus-core`,
`nimbus-engine`, and `nimbus-tenant` (path deps) plus the protocol crates above —
the same workspace-crate triple `nimbus-mongodb` and `nimbus-firebase` use.
Confirmed against `crates/nimbus-mongodb/Cargo.toml`: no `nimbus-server`, no
`axum`.

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
`crates/nimbus-server/tests/dynamodb_spec/` with a `mod.rs` that constructs a
shared scenario list, executes each scenario against Nimbus, then against
DynamoDB Local (and optionally ExtendDB) using the same wire client, and
diffs the responses. Reuse the **scenario-model and runner shape** of
`crates/nimbus-server/tests/mongodb_spec/` (`SpecTestFile` → `SpecTest` →
`Operation` with `expect_result`/`expect_error`, plus a `classify_operations`
supported/unsupported tally). Note one real difference: `mongodb_spec/` compares
Nimbus output to a **recorded expected result** (official MongoDB spec YAML), not
to a live reference server, so the **live dual-target diff harness** (run the
same scenario against Nimbus *and* DynamoDB Local/ExtendDB and diff at runtime)
is **net-new infrastructure**, not a copy of the existing runner. ExtendDB itself
runs dual-target against *real DynamoDB* (its tests treat real DynamoDB, not
DynamoDB Local, as the oracle) — see the External Test Adoption notes for how to
reconcile that with this plan's "DynamoDB Local primary" stance. Seed the
scenario list from the ExtendDB
Python parity corpus at
`/Users/jack/src/github.com/ExtendDB/extenddb/tests/` (each `test_*.py` is a
ready-made behavior set covering one operation family — item ops, query/scan,
batch, transact, conditional writes, streams, TTL, GSI, etc.). Translate the
scenarios into the Nimbus scenario model for deterministic PR coverage. Also
evaluate the full upstream suite for vendoring under the external test adoption
policy below; the plan must not reject copying reputable Apache-2.0 tests just
because the suite is large.

In addition to translated scenarios, add an external compatibility-suite runner
that can execute upstream test suites by path, modeled after ExtendDB's
`external-suites.toml` approach. The runner records the suite name, upstream
path, release tag or commit SHA, command, environment, client SDK, target
endpoint, pass/fail count, skipped tests, and artifacts. External suites are
allowed to be slower than PR lanes; they should run in nightly/manual lanes,
with a small critical subset promoted to PR once stable.

Every scenario records its provenance: DynamoDB Local probe, AWS SDK/CLI source
test, botocore model shape, ExtendDB test path, moto test path, or Nimbus-only
regression. Enterprise compatibility evidence is not accepted if it only proves
the Rust handler accepts a manually assembled JSON body. At least one official
SDK client must exercise each supported operation family before the tier can be
called complete.

### External Test Adoption And Canary App Policy

Nimbus needs its own tests and it should also reuse external compatibility
knowledge. Large known-good suites are valuable and should be treated like the
Node LTS fixture suites: pinned, repeatable corpora with provenance, refresh
commands, pass/fail/skip evidence, and release/SHA traceability. A canary is a
real app, framework, library, or SDK integration that already works against the
target platform; a test suite is not a canary. The default is not "copy only
small fixtures." The default is:

1. Run reputable external suites by reference first so Nimbus can measure
   compatibility quickly before copying code.
2. Vendor valuable suites into the Nimbus repository before a release claim
   when the source is reputable, license-compatible, endpoint-overridable, and
   maintainable. Prefer an upstream release tag; if no release includes the
   target corpus, pin the upstream commit SHA.
3. Translate important scenarios into Nimbus-native tests when they become
   product invariants or need deterministic PR coverage.
4. Keep a path-referenced external suite only as an interim proving lane, a
   very expensive nightly/manual lane, or for sources that cannot legally or
   operationally be copied.
5. Add canary apps separately from test suites: small real projects that use
   official SDKs or reputable DynamoDB libraries/frameworks against Nimbus by
   endpoint override, with their dependency versions and observed behavior
   recorded.

Use six explicit categories:

- `source-reference`: upstream source, SDK tests, or docs were read and cited,
  but no code or fixture was copied.
- `external-test-suite`: a large upstream or customer-like suite is run from
  its own checkout/package path against Nimbus using endpoint override. The
  suite is not copied into the Nimbus tree, but its commit/version, command,
  environment, pass/fail count, and exclusions are recorded.
- `vendored-suite`: a complete or substantial upstream suite is copied into
  Nimbus under `crates/nimbus-dynamodb/tests/upstream/<source>/` or another
  recorded adapter-owned testdata path. Its manifest records upstream release
  tag if available, otherwise commit SHA, sync date, selection command, license
  and NOTICE handling, local modifications, refresh command, owner, update
  cadence, lane, and last pass/fail/skip counts.
- `translated-scenario`: an upstream behavior case was rewritten into the
  Nimbus scenario model, with provenance recorded as the upstream repo path,
  test name, commit SHA, and any semantic changes.
- `vendored-fixture`: a small upstream fixture, golden request/response, SigV4
  vector, or focused test helper was copied into the Nimbus tree.
- `canary-app`: a real application, framework sample, library integration, or
  SDK project runs against Nimbus. It is not accepted as evidence unless it
  exercises Nimbus through a normal client configuration, records dependency
  versions, and asserts observable behavior rather than only process startup.

External test suites, vendored suites, and canary apps are required for
enterprise DynamoDB compatibility. Initial test-suite adoption targets:

- ExtendDB Python/boto3 suites from `/Users/jack/src/github.com/ExtendDB/extenddb/tests/`
  and `/Users/jack/src/github.com/ExtendDB/extenddb/tests/python/`. Run them by
  path first, then vendor the high-value compatible slices by ExtendDB release
  tag if available, otherwise by commit SHA.
- ExtendDB Rust SDK suite from
  `/Users/jack/src/github.com/ExtendDB/extenddb/tests/rust/`. Pin both the
  ExtendDB release/SHA and the AWS SDK Rust checkout/revision it uses; vendor
  the suite or a substantial compatible slice once the runner proves the
  endpoint-overridden behavior is stable.
- AWS CLI command corpus for supported T0-T7 operation families. If no
  official reusable corpus exists, keep a Nimbus-owned corpus that drives the
  official AWS CLI and records the CLI version.
- Official AWS SDK suites when the upstream repo exposes endpoint-overridable,
  license-compatible tests.

Initial canary-app targets:

- A JavaScript application using the official AWS SDK v3 DynamoDB client and
  document-client behavior against Nimbus.
- A Python application using boto3 client/resource APIs against Nimbus.
- A Rust application using `aws-sdk-dynamodb` against Nimbus.
- A practical Java v2 application when the Java client setup is available in
  the workspace.
- Any reputable DynamoDB library or framework discovered during D9.2 that is
  license-compatible, actively maintained, endpoint-overridable, and valuable
  enough to include in release evidence.

ExtendDB does not need a separate client SDK for Nimbus to reuse. Its README
states that unmodified AWS SDKs, CLI, and tools should work by changing the
endpoint, and its tests are built around that model. Nimbus should therefore
use ExtendDB as both a behavior corpus and an example of endpoint-overridden
official SDK verification.

Before a suite is accepted as an external or vendored suite, perform a quality
audit and record:

- Source reputation: maintainer, release cadence, and whether the suite is
  part of a real compatibility program rather than ad hoc examples.
- License and whether code is copied, referenced, or translated.
- Upstream release tag when available, otherwise commit SHA or package version.
- Whether the suite can run against real DynamoDB, DynamoDB Local, ExtendDB,
  and Nimbus by endpoint/config only.
- Which official client it uses, such as AWS CLI, boto3/botocore,
  `@aws-sdk/client-dynamodb`, `aws-sdk-dynamodb`, or AWS SDK for Java v2.
- Coverage by operation family, modeled errors, pagination, SigV4, retry
  behavior, streams, TTL, tags, transactions, and secondary indexes.
- Skip/xfail policy. Required suites must not hide supported-operation
  failures behind broad skips or expected failures.
- Cleanup/isolation behavior, credential handling, determinism, runtime cost,
  and whether tests can run in PR, nightly, or manual lanes.

Before a canary app is accepted as release evidence, record:

- The app/library/framework name, version, source, license, and dependency
  lockfile.
- Why it is representative of real DynamoDB usage.
- Which operation families it exercises and what assertions prove success.
- Endpoint override, auth mode, region, and SDK/client configuration.
- Whether it runs in PR, nightly, manual, or release-blocking lanes.

Vendoring is expected before release when all of these are true:

- The upstream license is compatible with the Nimbus repository license
  posture, such as Apache-2.0, MIT, or BSD-style terms.
- The copied artifact is stable enough to audit and maintain. A large suite
  must have an explicit owner, update cadence, license/NOTICE proof, and a
  repeatable refresh command.
- Original license headers are preserved, modifications are marked, and
  repo-root `NOTICE` coverage is updated when required.
- The test does not require upstream-private services, credentials, or brittle
  timing assumptions.
- The execution log records the upstream release tag or commit SHA and the
  reason the suite belongs in Nimbus rather than remaining only a referenced
  external suite.

Never copy GPL/AGPL or unknown-license tests into Nimbus. For those, use
`source-reference` or `external-test-suite` only if execution is legally and
operationally acceptable, or use `translated-scenario` without copying code.

Nimbus-owned tests are still required for Nimbus-specific guarantees:
tenant isolation, access-key binding, engine/storage atomicity, cancellation
boundaries, crate dependency boundaries, `_nimbus` ownership, performance
baselines, and soak/failure-injection behavior. External suites prove protocol
compatibility; canary apps prove realistic client usage; neither proves Nimbus
production safety by itself.

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

### Verification Evidence Conventions

Each roadmap item below carries a **Completion gate** (what must be true) and
**Verification evidence** (how it is proven). The evidence phrases are not
prose — they bind to concrete, repeatable commands and assertions defined here.
An item is not `done` until its evidence is recorded in the execution log with
the exact command run and the observed result (test count, exit code, or
classification counts). "Tests pass" without a named lane and count is not
acceptable (see `AGENTS.md` → Execution Quality).

- **"focused tests" / "tests for X"** — a named test lane under
  `crates/nimbus-dynamodb/` (e.g. `cargo test -p nimbus-dynamodb attribute_value::roundtrip`)
  that asserts a specific behavior (response shape, error `__type`, mutation
  result, classification), not merely that the call did not panic. The log
  records the module path and the passed/failed count.
- **"AWS SDK `<Op>` succeeds" / "AWS CLI `<cmd>` succeeds"** — the operation is
  driven through at least one official client (AWS CLI, `@aws-sdk/client-dynamodb`,
  `aws-sdk-dynamodb`, or `boto3`) configured with `--endpoint-url` / endpoint
  override against a live local Nimbus DynamoDB listener, the client exits 0, and
  an assertion compares the round-tripped response to the expected shape. The log
  records the client, its exact version, and the endpoint/auth mode. A handler
  that only accepts a hand-built JSON body does not satisfy this.
- **"parity diff clean"** — the parity runner
  (`crates/nimbus-server/tests/dynamodb_spec/`) executes the item's covered
  scenarios against Nimbus and against DynamoDB Local (and ExtendDB where it
  builds), and every covered scenario classifies as `pass` or as an explicitly
  recorded `accept-extenddb-divergence` / `nimbus-divergence` with a matching
  `docs/adapters/dynamodb/divergences.md` entry plus regression test. Zero
  unclassified diffs remain. The log records pass / divergence / skip counts.
- **"classification report committed"** — the parity runner writes a report
  artifact (per-scenario classification + provenance) that is committed under the
  adapter's testdata or proof path and referenced from the execution log.
- **"`cargo tree` / boundary clean"** — the named `cargo tree --edges normal`
  output is captured in the log and shows the asserted dependency present or
  absent, and the stale-reference audit returns zero hits for old
  `*-adapter`-suffix names and old server-local parity-test paths.

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

## Completion Gate

`bash scripts/verify-dynamodb-adapter.sh` exits 0 with a summary line
`N passed, 0 failed`. The verifier is scaffolded in D0.0a so it fails on every
unimplemented gate from day one, and each roadmap item turns its conditions green
as it lands. It must check at least the following (the count `N` is whatever the
final verifier enumerates; raise it as conditions are added, never lower the bar
to make a hard fixture pass):

1. Plan is `in_progress` or `archived` and every roadmap item (D0.0a..D9.7) is
   `done` at closeout; the phase status ledger and execution log agree.
2. `crates/nimbus-dynamodb` is a workspace member named `nimbus-dynamodb`;
   `cargo check -p nimbus-dynamodb` and `cargo check -p nimbus-server` are clean.
3. Crate boundary holds: `cargo tree -p nimbus-server --edges normal` shows
   `nimbus-dynamodb` and does **not** show `extenddb-core` or `nimbus-adapters`;
   the stale-reference audit returns zero `*-adapter`-suffix package/path/import
   hits and zero old server-local parity-test paths.
4. Attribution is clean: the `extenddb-core` git rev pin is recorded; every
   vendored SigV4 file keeps its Apache-2.0 header; repo-root `NOTICE` has one
   ExtendDB entry covering all copied/derived files; `make deny` and
   `make verify-third-party-attribution` are clean.
5. `docs/adapters/dynamodb/feature-coverage.md` lists every T0–T7 operation with
   status `implemented` / `classified-divergence` / `unsupported-deferred`;
   100% of supported operations have a row and 0 modeled fields/exceptions are
   unclassified for `implemented` rows.
6. Every `implemented` operation has request-shape, success-response,
   modeled-error, limit, and malformed-input test lanes that pass; every modeled
   exception asserts HTTP status, DynamoDB `__type`, message shape, and
   SDK-visible classification.
7. Pagination operations prove `LastEvaluatedKey`↔`ExclusiveStartKey` roundtrip,
   exhausted pagination, invalid-token rejection, and SDK paginator compatibility.
8. Batch and transaction operations prove partial-failure envelopes
   (`UnprocessedKeys` / `UnprocessedItems`), `CancellationReasons`, and atomicity
   boundaries.
9. `docs/adapters/dynamodb/sdk-compatibility.md` records the official SDK matrix
   (AWS CLI, JS v3, Rust, Python) with exact versions, endpoint URL, auth mode,
   and pass/fail counts; no supported operation fails through an official SDK due
   to Nimbus request-parsing, response-shape, SigV4, pagination, retry, or
   modeled-exception drift.
10. The parity runner emits a committed classification report; 0 unclassified
    diffs remain; every `nimbus-divergence` has a
    `docs/adapters/dynamodb/divergences.md` entry plus a regression test.
11. SigV4 strict mode verifies real SDK-signed requests and rejects malformed
    and expired signatures with the correct `__type`; lookup-only mode is gated
    behind the `DynamoDbConfig::auth_mode` toggle.
12. Tenant-isolation proof: ≥2 tenants/access keys cannot cross-read, cross-write,
    list, stream, tag, or TTL-configure each other's tables; wrong key, wrong
    signature, and wrong tenant binding fail closed; report records 0 violations.
13. Failure-injection and cancellation tests (malformed JSON, missing headers,
    unknown ops, oversized payloads, invalid keys/tokens, dropped connections,
    pre- and post-commit cancellation, engine/storage errors, timeouts) pass with
    0 panics, 0 task leaks, and 0 unclassified 5xx responses.
14. The mixed-workload soak report records duration, request count, error count
    by class, task count before/after, memory high-water mark, and 0 panics / 0
    task leaks / 0 unclassified failures.
15. A committed benchmark baseline covers PutItem, GetItem, UpdateItem, Query,
    Scan, BatchGetItem, BatchWriteItem, TransactWriteItems, and Streams
    GetRecords with p50/p95/p99 latency, throughput, dataset/item size,
    concurrency, storage backend, host hardware, commit SHA, and a non-regression
    threshold per family.
16. `@nimbus/dynamodb` builds with ESM/CJS/types and its selftest passes; root
    `npm run typecheck` is clean.
17. The five DynamoDB verification-harness cases (handshake/control-plane,
    item-CRUD, query-scan, transact, streams) are present in PR and nightly
    lanes; `cargo test -p nimbus-server verification_harness_pr` includes all five.
18. The external-suite registry and canary-app matrix are recorded with upstream
    release tag or commit SHA pins, commands, lanes (`pr`/`nightly`/`manual`),
    SDK/client, and pass/fail/skip counts.
19. `docs/adapters/dynamodb/enterprise-readiness.md` exists and links every proof
    artifact (coverage table, SDK matrix, reliability, tenant isolation, soak,
    benchmarks, divergences, deferred features, operational limits).
20. `cargo fmt --all --check`, `make clippy`, strict docs-reference validation,
    and `git diff --check` all pass.
21. The verifier itself rejects soft evidence: it fails if any `implemented`
    coverage row lacks a named test lane, if any parity diff is unclassified, or
    if a hand-written support number disagrees with the generated coverage/SDK
    artifacts.

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
  and the transport-agnostic `dispatch(...)` entrypoint. It does not own routing
  or `axum` — `nimbus-server` mounts the dispatch on its own `POST /` route.
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
- **Resolve the key-size conflict explicitly (parity-relevant).** DynamoDB
  allows partition key ≤2,048 bytes + sort key ≤1,024 bytes = 3,072 raw bytes;
  base64url inflates by ~33%, so the naive `<pk>.<sk>` encoding needs ~4,096
  bytes — far over Nimbus's hard 1,500-byte `validate_document_key` limit. The
  naive encoding therefore caps the real combined key at ~1,100 raw bytes and
  **will reject items real DynamoDB accepts.** This is not a footnote; pick one
  and record it: **(a)** accept the divergence — document the max supported key
  size, reject oversize with `ValidationException`, and add a
  `docs/adapters/dynamodb/divergences.md` entry + regression test (recommended
  initial path; raising the core limit is cross-cutting); or **(b)** raise the
  `nimbus-core` `DocumentId` limit (a shared-primitive change touching all
  storage — only if a real workload needs full-size DynamoDB keys). Do not
  silently inherit the 1,500-byte cap.
- **Preserve sort-key ordering semantics.** base64url of the raw sort segment is
  **not** equivalent to DynamoDB's type-specific sort order (N numeric, S UTF-8
  lexicographic, B byte-wise). Query sort-key range conditions
  (`BETWEEN`/`<`/`>`/`begins_with`) must therefore evaluate against the
  type-aware `_sk` body field, **not** against the encoded DocumentId. Record
  this so D2.1 does not accidentally range-scan on the opaque key.
  **`_pk`/`_sk` (and the per-index projected key fields below) must hold an
  order-preserving, type-faithful *sortable string* — not a JSON number or
  base64 — because Nimbus's index/compare path runs numbers through `f64`
  (~17 digits, lossy vs DynamoDB's 38) and cannot index binary at all (no
  `FieldType::Binary`):** store S as raw UTF-8, N as a lexicographically-sortable
  full-precision decimal string, and B as fixed-case hex. See "Key And Index
  Ordering: Numeric Precision And Binary Keys" under Upstream Review Insights.
  This adapter-local projection is a workaround for the pre-existing
  `docs/technical-debt.md` **T-005** (high) SQL numeric-ordering gap; it does not
  close T-005's generic SQL-backend fix.
- Recover the original `pk`/`sk` AttributeValues from the **typed-scalar
  metadata** (lossless) or by decoding the reversible base64url DocumentId —
  **not** from `_pk`/`_sk`, which now hold the sortable projection rather than the
  raw value. Range and equality on the storage path use the projection; response
  shaping and exact value reads use the metadata/DocumentId.
- **Project GSI/LSI key attributes the same way into their own per-index fields**
  (e.g. `_gsi1_pk`/`_gsi1_sk`), since each secondary index keys on different
  attributes than the table's `_pk`/`_sk`. D4.3 routes index-targeted Query/Scan
  through these projected fields, with the same S/N/B encoding rules.

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
| `DeleteTable` | T0 | Transitions the table to a Nimbus `deleting` lifecycle state, then reclaims rows. Physical removal is a **bulk delete over the shared `documents` table** (`DELETE WHERE table_id=…` + index cleanup + background reclamation), not an O(1) `DROP TABLE` as in ExtendDB's per-table model — an accepted trade for the rare-path drop (see storage-layout decision) |
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

There are **573 reserved words** (catalogued in `extenddb-core`'s 615-line
`expression/reserved_words.rs`). Clients normally use `#name` placeholders when
they collide. Contrary to an earlier note in this plan, ExtendDB **does** reject
bare reserved-word identifiers via `validate_no_reserved_words()` (raising
`ValidationException: Attribute name is a reserved keyword`), gated behind the
`enforce_reserved_keywords` limit flag — and real DynamoDB rejects them too.
Nimbus's decision: enable reserved-word rejection to match DynamoDB (inherited
for free through the `extenddb-core` dependency); cover it with a parity test so
the behavior is locked rather than assumed.

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
- Implement the `nimbus_dynamodb::dispatch(target, body, &Arc<Service>, &auth)`
  entrypoint (transport-agnostic, no `axum`). `nimbus-server` owns the `POST /`
  route that calls it, binds the socket, and supervises the task.
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
  prefix scan. Partition-key equality maps to a `DocumentId` prefix scan over
  the shared `documents` `(table_id, id)` key because `<pk-base64url>.` is a
  stable prefix — the partition selection therefore uses the primary-key index,
  not a full-table scan. Only the sort-key *range* evaluates against the typed
  `_sk` body field (base64url is not order-preserving — see D0.3). Then apply
  FilterExpression in-memory (or as a query AST filter when possible), with
  ProjectionExpression projection, ScanIndexForward sort order, Limit, and
  ExclusiveStartKey/LastEvaluatedKey pagination.
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
  time. Index-key ranges must preserve DynamoDB's type-specific ordering
  (N numeric at full precision, S byte-wise, B byte-wise). Nimbus's index path
  is `f64`-based and cannot index binary, so the adapter projects GSI/LSI key
  attributes into order-preserving sortable strings (S raw, N full-precision
  sortable decimal, B hex), exactly as for the base-table `_sk` in D0.3. See
  "Key And Index Ordering" in Upstream Review Insights and D4.3.
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

Location: `packages/dynamodb/` and `crates/nimbus-server/tests/dynamodb_spec/`.

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
  `crates/nimbus-server/tests/dynamodb_spec/` analogous to
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
shapes for tiers T0-T6; external compatibility suites pass where adopted;
official JS, Rust, Python, and AWS CLI client workflows pass against Nimbus for
every supported operation family; real canary apps pass for the selected
SDK/library matrix; verification harness includes the five DynamoDB cases in
PR and nightly lanes.

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
| Listener / transport ownership | `nimbus-dynamodb` exposes a transport-agnostic `dispatch(...)`; `nimbus-server` owns the `axum` `POST /` route, the dedicated port, and bind/spawn/shutdown | Matches every existing concrete adapter — none depend on `axum` or own a `Router`; `nimbus-server` owns all transport (`CLAUDE.md`). X-Amz-Target dispatch is a single route + header switch, so a router in the adapter is unnecessary. **Rejected alternative:** an `axum` `Router` factory inside `nimbus-dynamodb` (the plan's original design). Rejected because it adds an `axum` dep the convention forbids and because the parity runner would then need to bind the adapter router, splitting it from the `nimbus-server`-owned listener used by `mongodb_spec/`. Port isolation (collision avoidance) comes from the dedicated socket, not from where the route is constructed. |
| Default port | 8000 | Matches DynamoDB Local convention; AWS SDK `--endpoint-url http://localhost:8000` works out of the box |
| AttributeValue codec | Typed-scalar metadata (reused from F3.4b1 and MongoDB adapter) | N/B/SS/NS/BS need roundtrip fidelity through JSON storage; MongoDB adapter already proved this for Decimal128 and Binary |
| Physical storage layout | Shared Nimbus `documents(table_id, id)` (backend-owned) — **not** per-table UUID physical tables | ExtendDB's `storage-postgres` maps each logical table to a UUID-named physical table (`_ddb_<uuid>`) plus per-GSI/LSI physical tables; that model forks DynamoDB data into its own physical schema and **breaks this plan's one-backend / N-protocols premise** (Context → "Why DynamoDB": the same data must be reachable from a Convex mutation, a Firestore set, or a Mongo `insertOne`). It is also Postgres-only, whereas this adapter must run over every Nimbus backend. The table-name-reuse and rename-safety problems ExtendDB cites as its reason for UUID physical names are already solved in Nimbus by the stable `TableId` ULID catalog without per-table physical tables. Per-table physical layout stays a measured, backend-specific optimization gated on `TableBackendLayout` per MBA10 (`docs/plans/archive/multi-backend-adapter-hardening-plan.md:84`). |
| Composite key encoding | `<pk-base64url>.<sk-base64url>` | Reversible, fits within DocumentId rules (no `/`, no NUL) with a **documented max-key-size divergence**: DynamoDB's 3,072 B pk+sk exceeds Nimbus's 1,500 B `validate_document_key` limit (see D0.3 / R2); unambiguous separator |
| Expression parser | `extenddb-core` expression parser/evaluator through a Nimbus shim | Reuses the AWS-maintained open reference for the highest-risk protocol grammar and keeps Nimbus code focused on capability checks and storage bridging |
| Table-to-tenant mapping | Per-access-key tenant binding | DynamoDB has flat account namespace; Nimbus needs tenant scoping; binding is configured at adapter setup |
| Stream shard model | Single-shard per stream | Nimbus subscription model is non-sharded; sufficient for non-throughput-bound workloads. Document the divergence from **both** real DynamoDB's hierarchical shard tree **and** ExtendDB's stream design (which recommends N hash-based shards per table, default 4) |
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
- **Backup/Restore (`CreateBackup`, `DescribeBackup`, `ListBackups`,
  `DeleteBackup`, `RestoreTableFromBackup`).** Nimbus has its own backup story;
  deferred until customer demand. (ExtendDB implements these; Nimbus defers.)
- **PITR (`DescribeContinuousBackups`, `UpdateContinuousBackups`,
  `RestoreTableToPointInTime`).** Point-in-time recovery; depends on Nimbus's
  storage versioning roadmap. (ExtendDB implements these; Nimbus defers.)
- **Import/Export (`ImportTable`, `ExportTableToPointInTime`).** S3-backed
  bulk transfer; mocked-S3 path is possible but deferred. (ExtendDB implements a
  local FileSource/filesystem variant; Nimbus defers.)
- **STS (`AssumeRole`) / IAM.** ExtendDB exposes `AssumeRole` at its auth layer;
  Nimbus has no IAM/STS and replaces it with per-access-key tenant binding.
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
SS/NS/BS (sets) cannot roundtrip through plain JSON. The shared
`TypedScalarValue` enum proven for MongoDB Decimal128/Binary does **not** cover
DynamoDB `N`/`SS`/`NS`/`BS` and its flat top-level `typed_fields` map cannot
represent typed scalars nested inside `M`/`L`/Set members (see Current Assessed
State and MongoDB hardening finding L8). Mitigation: first land D0.1b to extend
`nimbus_core::typed_scalar` with the DynamoDB variants and a nesting-capable
representation (updating the MongoDB/Firebase match arms in the same change),
then build the codec on the promoted seam; add explicit unit tests for every
AttributeValue variant including deeply nested `M`/`L` carrying typed leaves and
Set members.

**R2: Composite-key encoding correctness (Critical, D0).** Reversibility
across the full UTF-8 plane, the 1500-byte Nimbus key limit (which is **tighter
than DynamoDB's 3,072-byte pk+sk budget** — see D0.3), and type-correct sort
ordering are all non-trivial. Mitigation: use base64url for both pk and sk
segments with a `.` separator; add property tests across the full Unicode plane;
make the key-size-budget divergence decision in D0.3 and reject oversize keys
with `ValidationException`; range-scan sort conditions against the typed `_sk`
body field, not the opaque encoded key.

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

**R12: Query/GSI performance over the shared `documents` table (Medium, D2/D4/D9).**
Nimbus stores all DynamoDB items in the shared `documents(table_id, id)` table
rather than per-table physical tables (see the storage-layout decision in
Current Assessed State and Key Architectural Decisions). A confirmed cost
(2026-05-29 storage audit): the engine **always re-sorts matches in memory**
(`finalize_query_documents` → `sort_documents`) and load-then-truncates for
`Limit`, so there is no storage-ordered streaming top-N — a hot partition forces
a `DocumentId` prefix scan plus an in-memory `_sk` sort/filter, and GSI/LSI
ranges go through expression / generated-column indexes (not materialized typed
columns), where ExtendDB's per-table model gets native typed-column B-tree
ordering for free. This is a deliberate trade for cross-adapter data sharing and
multi-backend portability, not a defect. Mitigation: benchmark Query/Scan/GSI on
large partitions in D9.6; if a partition or `DeleteTable` cost is *measured* as a
bottleneck, the pre-decided remedy is a per-table `TableBackendLayout` for the
DynamoDB lane (MBA10), gated on that evidence — not a storage redesign. (Key
*ordering correctness* — distinct from performance — is covered separately under
"Key And Index Ordering" in Upstream Review Insights.)

## Phase Status Ledger

| Phase | Status | Context budget | Start condition | Done when |
|-------|--------|----------------|-----------------|-----------|
| D0: Wire envelope, AttributeValue bridge, control plane | `in_progress` (D0.0a done 2026-05-29) | 6-8 context windows | Plan promoted to `in_progress` (server extraction completion gates green — closed 2026-05-28) and D0.0a verifier scaffolded | `nimbus-dynamodb` compiles as a concrete crate; server listener composition works; AWS CLI control plane (create/describe/list/update/delete) succeeds end-to-end |
| D1: Single-item ops + expression language | `pending` | 8-12 context windows | D0 is `done` | PutItem/GetItem/UpdateItem/DeleteItem with all expression kinds succeed; parity diff clean for covered shapes |
| D2: Query and Scan | `pending` | 5-7 context windows | D1 is `done` | Query/Scan with pagination succeed end-to-end; parity diff clean |
| D3: Batch and transactional ops | `pending` | 4-6 context windows | D1 is `done` | BatchGet/BatchWrite/TransactGet/TransactWrite succeed; failure paths return correct exceptions |
| D4: Secondary indexes (GSI, LSI) | `pending` | 5-7 context windows | D2 is `done` | CreateTable with GSI/LSI; UpdateTable adding/removing GSI; Query/Scan via IndexName |
| D5: Streams | `pending` | 4-6 context windows | D1 is `done` | DescribeStream/GetShardIterator/GetRecords work; insert/update/delete events appear with correct StreamViewType |
| D6: TTL, tagging, control-plane completion | `pending` | 3-5 context windows | D1 is `done` | TTL enable/describe/expire and tag CRUD work end-to-end |
| D7: SigV4 strict mode | `pending` | 3-5 context windows | D0 is `done` | SigV4Strict mode verifies SDK requests and rejects malformed signatures |
| D8: SDK + parity tests + verification harness | `pending` | 6-9 context windows | D1-D6 are `done` | `@nimbus/dynamodb` selftest passes; parity classification report covers ≥80% of SDK shapes for T0-T6; external compatibility suites and canary apps pass for supported operation families; harness includes the five DynamoDB cases |
| D9: Enterprise readiness, reliability, and performance closeout | `pending` | 4-7 context windows | D0-D8 are `done` | Feature coverage table, SDK matrix, reliability proof, tenant-isolation proof, soak report, benchmark baseline, and enterprise-readiness doc are committed; `bash scripts/verify-dynamodb-adapter.sh` prints `N passed, 0 failed` |

## Roadmap Items

Each item is intended to fit in one focused context window. If an item
cannot fit with the relevant source context, implementation, tests, and
checkpoint update loaded at once, split it before starting.

### D0 Work Queue: Wire Envelope, AttributeValue Bridge, Control Plane

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D0.0a Worktree branch + control-plane verifier scaffold | `done` (2026-05-29) | none | Dedicated worktree branch created (`git worktree add ../nimbus-dynamodb-adapter -b dynamodb-adapter`); **all subsequent work happens on that branch, never on `main`** (see Branch, CI, and PR workflow). `scripts/verify-dynamodb-adapter.sh` created following the repo verifier convention (`pass`/`fail` helpers, bold `N passed, N failed` summary, exit non-zero on any failure). It enumerates the [Completion Gate](#completion-gate) conditions and **fails on every unimplemented gate** at creation time (every condition red except the plan-structure checks). `docs/prompts/dynamodb-adapter-start.md` written from the [Goal Control Plane](#goal-control-plane) objective. Plan promoted to `in_progress` and the control item recorded. | `git worktree list` shows the `dynamodb-adapter` branch; `bash scripts/verify-dynamodb-adapter.sh` runs and exits non-zero with a `N passed, M failed` summary where `M > 0` (it must not pass before any work lands); plan-structure conditions (1 partial, plan parses) report green; start-prompt file exists; `git diff --check` clean. |
| D0.0 Scaffold `nimbus-dynamodb`, wire `extenddb-core`, and vendor SigV4 | `done` (2026-05-29) | D0.0a | `crates/nimbus-dynamodb` added as a workspace member with package name `nimbus-dynamodb`. `extenddb-core` added as git rev pin in workspace `Cargo.toml` and pulled into `crates/nimbus-dynamodb/Cargo.toml`. `crates/nimbus-server/Cargo.toml` depends only on `nimbus-dynamodb`, not on `extenddb-core`. All **5** SigV4 files (`mod.rs`, `canonical.rs`, `parse.rs`, `signing_key.rs`, `verify.rs`) from `crates/auth/src/sigv4/` vendored verbatim into `crates/nimbus-dynamodb/src/auth/sigv4/` with Apache-2.0 headers preserved. Repo-root `NOTICE` file created (or updated) with one ExtendDB entry. The pinned ExtendDB sha (≥`0448ca0`) recorded in execution log. | `cargo check -p nimbus-dynamodb` clean; `cargo check -p nimbus-server` clean; `cargo tree -p nimbus-dynamodb --edges normal` includes pinned `extenddb-core`; `cargo tree -p nimbus-server --edges normal` shows `nimbus-dynamodb` and not `extenddb-core`; `make deny` clean. |
| D0.1 Dispatch entrypoint, server listener composition, and X-Amz-Target switch | `done` (2026-05-29) | D0.0 | `nimbus-dynamodb` exposes a transport-agnostic `dispatch(target, body, &Arc<Service>, &auth)` entrypoint (no `axum`, no socket). `nimbus-server` adds `ServeOptions::with_dynamodb`, owns the `axum` `POST /` route, port 8000 bind/spawn/abort (mirroring `construction.rs:164-191`), and calls `record_listener_state_async(..., "dynamodb", "http", ...)`. Unknown/missing target returns the DynamoDB error envelope. | Focused `nimbus-dynamodb` tests for the dispatch matrix, missing target (`UnknownOperationException`), and malformed body (`SerializationException`); focused `nimbus-server` test for optional listener wiring + listener-state registration; `cargo tree -p nimbus-dynamodb --edges normal` shows no `axum`. |
| D0.1b Promote `nimbus-core` typed-scalar for DynamoDB | `pending` | D0.1 | `nimbus_core::typed_scalar::TypedScalarValue` extended with DynamoDB `N` (arbitrary-precision number), `SS`, `NS`, `BS` variants **and** a nesting-capable representation so typed scalars survive inside `M`/`L` and as Set members (the current flat top-level `typed_fields` map cannot — MongoDB hardening L8). MongoDB (`crates/nimbus-mongodb/src/bson_bridge.rs`) and Firebase (`crates/nimbus-firebase/src/serializer.rs`) match arms updated to handle or explicitly reject the new variants; their existing guard tests stay green. This is a shared-primitive promotion per Control Plan Rule 6. | `cargo test -p nimbus-core typed_scalar` asserts roundtrip for the new variants and nested placement; `cargo test -p nimbus-mongodb` and `cargo test -p nimbus-firebase` stay green (no regression in their match arms); `cargo check --workspace` clean. |
| D0.2 AttributeValue bridge | `pending` | D0.1b | `extenddb_core::types::AttributeValue` ↔ Nimbus value roundtrip for S/N/B/M/L/SS/NS/BS/BOOL/NULL using the promoted typed-scalar metadata. Empty sets and empty top-level docs rejected (delegated to `extenddb_core::validation`). | `cargo test -p nimbus-dynamodb attribute_value` asserts roundtrip equality for all 10 variants (S/N/B/M/L/SS/NS/BS/BOOL/NULL), arbitrary-precision N preserved through string encoding, B preserved through base64, and empty-set / empty-doc inputs rejected with `ValidationException`. Log records the passed/failed count. |
| D0.3 Composite primary-key encoding | `pending` | D0.2 | Reversible `(pk, sk)` ↔ DocumentId encoding via base64url segments. **Key-size-budget divergence decision recorded** (DynamoDB 3,072B pk+sk vs Nimbus 1,500B DocumentId — accept-and-reject-oversize is the recommended initial path; entry in `docs/adapters/dynamodb/divergences.md`). Oversize keys rejected with ValidationException. `_pk`/`_sk` body fields hold an **order-preserving, type-faithful sortable encoding** (S raw UTF-8, N full-precision sortable decimal string, B fixed-case hex) — **not** a JSON number or base64 — because Nimbus's index/compare path runs numbers through `f64` and cannot index binary (see "Key And Index Ordering" in Upstream Review Insights); the original AttributeValue is kept in typed-scalar metadata for response shaping and exact equality. | Property tests across the Unicode plane; size-limit rejection test asserting the documented max key size; divergence doc entry; a sort-key `BETWEEN`/`begins_with` range test asserting `_sk` type semantics (N numeric vs S byte-wise vs B byte-wise), **including a numeric range with >17-significant-digit values that f64 would collapse** and a binary-keyed range asserting byte-wise order. |
| D0.4 Error envelope and code mapping | `pending` | D0.1 | Errors return HTTP 4xx/5xx with `{ "__type": "...", "message": "..." }` envelope. Shared error taxonomy maps to DynamoDB codes. | Tests for every mapped error class. |
| D0.5 Tenant resolution from access key | `pending` | D0.1 | Access-key prefix or configured binding resolves to a Nimbus tenant. Unknown key rejected with AccessDeniedException. | Tests for known/unknown key, multiple tenants. |
| D0.6 Control plane: Create/Describe/Delete/List/UpdateTable | `pending` | D0.3, D0.5 | CreateTable accepts KeySchema and AttributeDefinitions; DescribeTable returns the TableDescription; DeleteTable transitions the table to the Nimbus `deleting` lifecycle state and reclaims its rows via a bulk delete over the shared `documents` table (not a physical `DROP TABLE`); ListTables paginates; UpdateTable handles StreamSpecification and table-class fields (GSI deferred to D4). | AWS CLI control-plane workflow succeeds end-to-end. |
| D0.7 DescribeEndpoints and DescribeLimits | `pending` | D0.6 | DescribeEndpoints returns the Nimbus listener URL; DescribeLimits returns stubbed account/table limits. | Focused tests for both shapes. |
| D0.8 SigV4 lookup-only auth | `pending` | D0.5 | Authorization header parsed via vendored `auth/sigv4/parse.rs`; access key extracted; signature is not verified yet (deferred to D7). Tenant principal threaded through dispatch. | Tests for valid/missing auth header, access-key extraction. |
| D0.9 Optional `nimbus-adapters` facade export | `pending` | D0.1 | `crates/nimbus-adapters` adds a default-off `dynamodb` feature that depends on and re-exports `nimbus-dynamodb`. `nimbus-server` continues to depend directly on `nimbus-dynamodb`. | `cargo check -p nimbus-adapters --features dynamodb`; `cargo tree -p nimbus-adapters --edges normal` has no default adapter deps; `cargo tree -p nimbus-server --edges normal` has no `nimbus-adapters`. |

### D1 Work Queue: Single-Item Operations And Expression Language

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D1.1 Expression shim wiring | `pending` | D0 done | `expression.rs` shim adapts `extenddb_core::expression::{tokenizer, parser, evaluator, update_evaluator, key_condition, projection, resolver}` to operate on Nimbus document values via the AttributeValue bridge. ExpressionAttributeNames / ExpressionAttributeValues resolution flows through the upstream resolver. | Smoke test that a representative ConditionExpression, UpdateExpression, ProjectionExpression, and KeyConditionExpression each parse and evaluate end-to-end against an in-memory item. |
| D1.2 ConditionExpression integration | `pending` | D1.1 | All comparison, logical, and function operators (`attribute_exists`, `attribute_not_exists`, `attribute_type`, `begins_with`, `contains`, `size`) evaluate correctly against Nimbus-stored items. Errors map to `ConditionalCheckFailedException`. | Coverage tests for every operator and function; parity classification clean against DynamoDB Local. |
| D1.3 UpdateExpression integration | `pending` | D1.1 | All actions (SET with `if_not_exists`/`list_append`/arithmetic, REMOVE, ADD on numeric and set, DELETE on set) apply correctly to Nimbus documents and respect the AttributeValue bridge. | `cargo test -p nimbus-dynamodb update_expression` asserts each of the four action kinds (SET incl. `if_not_exists`/`list_append`/arithmetic, REMOVE, ADD numeric+set, DELETE set) and at least one multi-action clause produce the expected document mutation; parity diff clean for the UpdateItem scenario set. |
| D1.4 ProjectionExpression integration | `pending` | D1.1 | Path-based field selector with dot/bracket notation works against Nimbus documents. | Tests for nested paths and array indexing. |
| D1.5 PutItem | `pending` | D1.2, D1.4 | PutItem with ConditionExpression, ReturnValues (NONE/ALL_OLD). ConditionalCheckFailedException with optional Item field. | AWS SDK PutItem succeeds; parity diff clean. |
| D1.6 GetItem | `pending` | D1.4 | GetItem with ProjectionExpression and ConsistentRead flag (accept-and-ignore). | AWS SDK GetItem succeeds; parity diff clean. |
| D1.7 DeleteItem | `pending` | D1.2 | DeleteItem with ConditionExpression and ReturnValues (NONE/ALL_OLD). | AWS SDK DeleteItem succeeds; parity diff clean. |
| D1.8 UpdateItem | `pending` | D1.3 | UpdateItem with full UpdateExpression action support, ConditionExpression, all four ReturnValues modes. ADD numeric maps to shared FieldTransformOperation; complex actions execute through the atomic write batch (genuinely atomic, not lossy RMW — MongoDB hardening M5). **Edge cases (see Upstream Review Insights):** no-directive call is a no-op upsert (`754f307`); `Some("")` errors `"The expression can not be empty;"`; UPDATED_NEW/UPDATED_OLD **omit** `Attributes` when empty (`5ec827b`) and leaf-wrap nested-path values. | AWS SDK UpdateItem succeeds for every action kind; no-op-upsert, omit-empty-`Attributes`, and leaf-path assertions pass; parity diff clean. |

### D2 Work Queue: Query And Scan

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D2.1 KeyConditionExpression and Query | `pending` | D1.4 | KeyConditionExpression compiles to a primary-key partition-equals-plus-optional-sort-range query; sort-range conditions evaluate against the typed `_sk` body field (type-correct ordering), not the opaque key (see D0.3). **Edge cases:** reversed comparisons (`:val <= sk` ≡ `sk >= :val`, `c11fdb6`) accepted; `<>`/NE on key conditions rejected; malformed `ExclusiveStartKey` → `ValidationException` with the Query-specific message (distinct from Scan's, `9a1a1a6`). ScanIndexForward, Limit, ExclusiveStartKey/LastEvaluatedKey pagination work. | AWS SDK Query succeeds with pagination; reversed-comparison, NE-rejection, and malformed-start-key assertions pass; parity diff clean. |
| D2.2 FilterExpression for Query | `pending` | D2.1 | FilterExpression applies after key selection. Select modes (ALL_ATTRIBUTES, SPECIFIC_ATTRIBUTES, COUNT) work. | Tests for filter + projection composition. |
| D2.3 Scan with FilterExpression and pagination | `pending` | D1.4 | Scan iterates a full table or segment with FilterExpression, ProjectionExpression, Limit, ExclusiveStartKey. | AWS SDK Scan succeeds; parity diff clean. |
| D2.4 Parallel Scan segments | `pending` | D2.3 | Segment/TotalSegments parameters partition the scan deterministically. | `cargo test -p nimbus-dynamodb scan::parallel` asserts that scanning all `TotalSegments` segments returns the full table exactly once (union = full set, pairwise-disjoint, no item dropped or duplicated) and that the partition is stable across repeated runs. |

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
| D4.3 Projection types and index-targeted Query/Scan | `pending` | D4.1, D4.2, D2.1, D2.3 | KEYS_ONLY/INCLUDE/ALL projection applied at response shaping. `IndexName` parameter routes Query/Scan to the index. **Index-key ordering must be type-correct:** GSI/LSI partition/sort keys must order by DynamoDB's type rules (N numeric at full precision, S UTF-8 byte-wise, B byte-wise) — the same requirement D0.3 imposes on `_sk`. Because Nimbus's index/compare path is `f64`-only and cannot index binary (no `FieldType::Binary`; see "Key And Index Ordering" in Upstream Review Insights), the adapter must project index key attributes into order-preserving sortable strings (S raw, N full-precision sortable decimal, B hex), or escalate to the core-promotion alternative. Record the choice. | Tests for each projection type and index-targeted read, **including a numeric-keyed GSI range query asserting numeric ordering at >17 significant digits (values f64 would collapse) and a binary-keyed GSI range asserting byte-wise order**. |
| D4.4 GSI ConsistentRead divergence decision | `pending` | D4.3 | Decision recorded in `docs/adapters/dynamodb/divergences.md`: either match DynamoDB's ValidationException rejection of `ConsistentRead=true` on a GSI Query, or accept-and-serve consistently as a Nimbus upgrade. | Test for the chosen behavior; divergence doc entry. |

### D5 Work Queue: Streams

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D5.1 StreamSpecification at CreateTable/UpdateTable | `pending` | D0.6 | StreamEnabled and StreamViewType (KEYS_ONLY/NEW_IMAGE/OLD_IMAGE/NEW_AND_OLD_IMAGES) recorded per table. | Tests for stream-enabled table description. |
| D5.2 DescribeStream and shard model | `pending` | D5.1 | Single-shard stream description per enabled table; shard ID stable. Sequence numbers use **i64** (not i32 — MongoDB hardening M6). | Test for shape; divergence doc entry noting single-shard diverges from both real DynamoDB's shard tree and ExtendDB's 4-shard design. |
| D5.3 GetShardIterator | `pending` | D5.2 | TRIM_HORIZON/LATEST/AT_SEQUENCE_NUMBER/AFTER_SEQUENCE_NUMBER iterator types. Iterator format opaque to clients. | Tests for each iterator type. |
| D5.4 GetRecords with StreamViewType shaping | `pending` | D5.3, D1.5, D1.7, D1.8 | GetRecords returns Records (≤1000) and NextShardIterator. Records shape matches the configured StreamViewType. | `cargo test -p nimbus-dynamodb streams::records` asserts each StreamViewType (KEYS_ONLY/NEW_IMAGE/OLD_IMAGE/NEW_AND_OLD_IMAGES) emits the correct record shape for INSERT/MODIFY/REMOVE events, `NextShardIterator` advances, and a ≤1000 record cap is enforced. |
| D5.5 ListStreams and retention | `pending` | D5.2 | ListStreams enumerates active streams (optionally filtered by TableName); records evicted past retention window. | Tests for enumeration and eviction. |

### D6 Work Queue: TTL, Tagging, Control-Plane Completion

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D6.1 UpdateTimeToLive and DescribeTimeToLive | `pending` | D0.6 | TTL attribute name enable/disable; descriptive response. **Divergence decisions recorded:** TTL attribute-name charset (Nimbus can match DynamoDB's any-UTF-8 since it has no SQL surface) and TTL-modification cooldown (DynamoDB enforces one). | Tests for enable/disable/describe roundtrip; divergence doc entries for charset + cooldown. |
| D6.2 TTL sweeper integration | `pending` | D6.1, D1.7 | Periodic engine-owned task deletes items whose TTL attribute is past current epoch seconds. Sweep interval configurable. TTL-originated REMOVE stream records carry `userIdentity:{type:"Service",principalId:"dynamodb.amazonaws.com"}` (matches real DynamoDB). | Tests for expired-item sweep; delete event in the stream carries the TTL `userIdentity` shape. |
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
| D8.6 ExtendDB parity comparison | `pending` | D8.3 | ExtendDB run alongside DynamoDB Local when the local checkout builds, pinned ≥`0448ca0`. Target setup is non-trivial (see Upstream Review Insights): `extenddb init` → HTTPS/TLS (`verify=False`) → `devtools/provision-test-credentials`, with `throttling_enabled=false` and `control_plane_delay_seconds=0`. Divergences classified as `accept-extenddb-divergence` where Nimbus matches ExtendDB but not real DynamoDB. If ExtendDB cannot build/run, the failure and next action are recorded instead of silently skipping the column. | Classification report includes ExtendDB column (with the init/TLS/credential setup commands recorded) or a recorded build/run failure with command output. |
| D8.7 Verification harness integration | `pending` | D8.3 | Five DynamoDB harness cases added to PR and nightly lanes (handshake/control-plane, item-CRUD, query-scan, transact, streams). | `cargo test -p nimbus-server verification_harness_pr` includes all five. |
| D8.8 External compatibility-suite runner | `pending` | D8.3 | Add a path-referenced and vendored-suite registry for large external compatibility suites. Initial entries cover ExtendDB Python/boto3 suites, ExtendDB Rust SDK suite, AWS CLI corpus, and official AWS SDK suites when available. Suites are pinned by release tag when available, otherwise by commit/version, and run against Nimbus by endpoint override. | Suite report records suite name, command, environment, SDK, upstream release/SHA/version, pass/fail/skip counts, artifacts, lane (`pr`, `nightly`, or `manual`), and whether the suite remains path-referenced or has been vendored. |
| D8.9 Canary application matrix | `pending` | D8.2, D8.8 | Add real app/library canaries distinct from test suites. Initial canaries cover JavaScript AWS SDK v3 document-client usage, Python boto3 client/resource usage, Rust `aws-sdk-dynamodb` usage, practical Java v2 usage when available, and any reputable DynamoDB library/framework selected by the D9.2 quality audit. | Canary report records app/library name, version, lockfile, endpoint/auth configuration, operation families exercised, assertions, pass/fail counts, lane, and release-blocking status. |

### D9 Work Queue: Enterprise Readiness, Reliability, And Performance

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| D9.1 Feature-parity coverage table | `pending` | D8.4, D8.5 | `docs/adapters/dynamodb/feature-coverage.md` lists every T0-T7 operation, modeled request/response fields, modeled exceptions, pagination shape, idempotency fields, and status (`implemented`, `classified-divergence`, `unsupported-deferred`). | Generated coverage table committed; 100% of supported operations have a row; 0 unclassified fields/exceptions for implemented operations. |
| D9.2 External suite quality audit, official SDK matrix, and canary-app selection | `pending` | D8.2, D8.4, D8.5, D8.8, D8.9, D7.2 | Audit every accepted external suite for license, endpoint-overridden target support, real-DynamoDB/DynamoDB-Local/ExtendDB/Nimbus targetability, SDK used, operation coverage, skip policy, cleanup, determinism, runtime cost, and credential safety. Audit canary apps separately for representativeness, maintenance, license, endpoint override, lockfile stability, and behavior assertions. AWS CLI, JS v3, Rust, Python, and practical Java clients each run supported operation-family scenarios against Nimbus with endpoint override and SigV4 mode recorded. | `docs/adapters/dynamodb/sdk-compatibility.md` records suite-quality audit, canary-app audit, exact versions/SHAs, pass/fail/skip counts, auth mode, endpoint URL, and divergences; no supported operation fails through an official SDK due to Nimbus protocol drift. |
| D9.3 Failure injection and cancellation proof | `pending` | D8.4 | Malformed input, oversized payloads, invalid keys/tokens, dropped connections, pre-commit cancellation, post-commit cancellation, engine errors, storage errors, and timeout paths fail closed without panics or partial-success envelopes. | Focused failure-injection tests pass; report records 0 panics and 0 unclassified 5xx responses. |
| D9.4 Tenant isolation and auth isolation proof | `pending` | D7.3, D8.4, D8.5 | Two or more tenants/access keys cannot cross-read, cross-write, list, stream, tag, TTL-configure, or infer each other's tables. Wrong access key, wrong signature, and wrong tenant binding fail closed. | Tenant-isolation conformance tests pass; report records 0 cross-tenant visibility or mutation violations. |
| D9.5 Mixed-workload soak test | `pending` | D9.2, D9.3, D9.4 | Mixed SDK traffic runs for a fixed duration across reads, writes, conditional writes, queries, streams, TTL/tag metadata, and auth failures. | Soak report records duration, request count, error count by class, task count before/after, memory high-water mark, and 0 panics/task leaks/unclassified failures. |
| D9.6 Performance benchmark baseline | `pending` | D8.4, D8.5 | Benchmarks cover PutItem, GetItem, UpdateItem, Query, Scan, BatchGetItem, BatchWriteItem, TransactWriteItems, and Streams GetRecords across documented item sizes and concurrency levels. | Benchmark report records p50/p95/p99 latency, throughput, dataset size, item size, concurrency, storage backend, host hardware, commit SHA, and initial non-regression thresholds. |
| D9.7 Enterprise-readiness closeout, verifier green, and PR | `pending` | D9.1-D9.6 | `docs/adapters/dynamodb/enterprise-readiness.md` summarizes feature coverage, SDK matrix, reliability proof, tenant isolation proof, benchmark baseline, known divergences, deferred features, and operational limits. Every [Completion Gate](#completion-gate) condition is implemented in `scripts/verify-dynamodb-adapter.sh` and green. Plan moved to `archive/` and `AGENTS.md` routing points at the archived baseline. **Final step (Branch, CI, and PR workflow):** push `dynamodb-adapter`, confirm full CI is green on the branch, then open a PR `dynamodb-adapter → main` as the closeout action. Do **not** push to `main` directly or self-merge the PR. | Document committed; execution log links every proof artifact; `bash scripts/verify-dynamodb-adapter.sh` prints `N passed, 0 failed` and exits 0; final stale-reference/dependency-graph checks pass; `cargo fmt --all --check`, `make clippy`, and `git diff --check` clean; branch pushed with green CI (`gh run` URL recorded); PR `dynamodb-adapter → main` open (URL recorded in the execution log). |

## Upstream Review Insights (ExtendDB `0448ca0` + Adapter Comparison, 2026-05-29)

ExtendDB was pulled `5f5e511 → 0448ca0` (6 DynamoDB-fidelity fixes) and reviewed
against the four existing concrete Nimbus adapters. Architecture/correctness
findings are folded into the relevant sections above (typed-scalar promotion
D0.1b, dispatch-not-router transport ownership, parity-runner location, composite
key-size divergence, LOC/SigV4/dispatch-pointer/reserved-word corrections). This
section is the durable index for the remaining limits, edge-case seeds, and
divergence decisions.

### Key And Index Ordering: Numeric Precision And Binary Keys (2026-05-29 storage audit)

A storage-layer audit (`crates/nimbus-storage/src/index/encoding.rs`,
`crates/nimbus-engine/src/evaluator/{ordering,filtering}.rs`, the SQL backends,
and `docs/architecture/storage/typed-key-columns.md`) found that Nimbus's
existing index/ordering path does **not** by itself satisfy DynamoDB key
ordering for two of the three key types. (Earlier plan text implied MBA11
`sort_s`/`sort_n`/`sort_b` typed columns close this — they are not materialized
columns and would not close it.) Ground truth:

- **Three ordering mechanisms, one authority.** redb compares an
  order-preserving byte encoding (`encode_index_value`); SQL backends index
  `json_extract` / `jsonb_extract_path_text` expressions (SQLite/Postgres) or
  generated VIRTUAL columns (MySQL) and range-filter with native SQL; and the
  engine **always re-sorts results in memory** (`finalize_query_documents` →
  `sort_documents`) and load-then-truncates for `Limit`. The final result order
  is therefore the engine's `compare_order_field`, while *which rows match* a
  range bound is the storage backend's comparison.
- **N (number) precision — real gap.** Every ordering path runs numbers through
  `as_f64()` (`encoding.rs:11`, `ordering.rs:66`, `filtering.rs:59`) and SQL
  casts to `DOUBLE` / `DOUBLE PRECISION` — ~15–17 significant digits, while
  DynamoDB `N` is exact to 38. High-precision numeric *sort keys* and *GSI/LSI
  numeric keys* would mis-order and collide (breaking range, equality, and
  uniqueness). D0.1b fixes *body* fidelity via typed-scalar metadata but does
  **not** touch the index/compare path. (Base-table key *equality* via the
  base64url DocumentId is byte-exact and unaffected.)
- **B (binary) — real gap.** `FieldType` has no `Binary` variant
  (`schema.rs:31`), `encode_index_value` rejects bytes (`encoding.rs:38`), and
  the engine compares only string/number. DynamoDB allows `B` partition/sort/
  GSI/LSI keys with byte-wise order; base64 in `_sk` is not order-preserving.
- **S (string) collation — conditional gap.** redb and the engine compare
  UTF-8 byte order (matches DynamoDB). But pushed-down SQL range bounds use the
  column/locale collation — Postgres locale, MySQL `utf8mb4_general_ci`
  (case-insensitive) — so a `BETWEEN`/`begins_with`/`<`/`>` selected via a SQL
  index can include/exclude rows differently from DynamoDB's byte-wise rule.

**Resolution (adapter-local, uniform across backends).** Project every DynamoDB
key/index attribute (`_sk`, GSI/LSI partition and sort keys) into an
order-preserving, type-faithful **sortable string** at write time, query ranges
against that projection, and keep the original AttributeValue in typed-scalar
metadata for response shaping and exact equality:

- **S** → store the raw UTF-8 string; ensure the comparison is byte-wise (the
  engine re-sort already is; for pushed-down SQL ranges use a binary collation —
  `COLLATE "C"` / `utf8mb4_bin` — or rely on the in-engine filter/sort).
- **N** → encode to a lexicographically-sortable decimal string that preserves
  full 38-digit precision (sign + ordered exponent + normalized mantissa);
  string ordering then equals numeric ordering at full precision and equality is
  an exact string match (no f64 collision).
- **B** → encode to fixed-case **hex** (order-preserving for unsigned byte-wise
  comparison; base64 is not); string ordering then equals byte-wise order.

This stays inside `nimbus-dynamodb` (no `nimbus-core` `FieldType::Binary` or
decimal-index change) and reuses the existing string-ordered path. The heavier
alternative — add `FieldType::Binary` + arbitrary-precision decimal to the core
index encoding and per-backend `NUMERIC`/`DECIMAL` casts (Postgres/MySQL get
38-digit precision natively; SQLite/libSQL/redb still need an encoding) — is a
shared-primitive promotion per MBA11's own "when binary fields are introduced"
note; defer it unless another adapter needs it. Either way, record the choice
and add tests: a numeric range with >17-significant-digit values that f64 would
collapse, and a binary-keyed range asserting byte-wise order.

### DynamoDB Hard Limits (ExtendDB enforces; Nimbus must validate)

The plan's validation lists previously mentioned only the Nimbus 1,500-byte key
cap, 100/25 batch sizes, and 100-op/4MB transaction limits. Add these, all
enforced by ExtendDB and visible to SDK parity tests:

| Limit | Value | Notes |
|-------|-------|-------|
| Max item size | **400 KB** (409,600 bytes) | DynamoDB number-byte-sizing formula applies (zero = 1 byte) |
| Attribute nesting depth | **32 levels** | Applies to **stored** values (Put/Update SET+if_not_exists/BatchWrite.Put/Transact.Put/Import); deep EAVs used **only** in ConditionExpression/legacy Expected are exempt |
| Partition key size | **2,048 bytes** | Exceeds Nimbus DocumentId budget — see D0.3 |
| Sort key size | **1,024 bytes** | Same |
| Attribute name size | **64 KB** | Top-level enforced (nested map keys are an upstream debt) |
| Table / index name | **3–255 chars**, `[a-zA-Z0-9_.-]` | |
| GSIs per table / LSIs per table | **20 / 5** | |
| Query/Scan response page | **1 MB** | A byte cap, distinct from `Limit` item count — the plan only mentioned `Limit` |
| Expression length (Filter/Projection/Condition) | **4 KB each** | |
| BatchGet 100 keys / BatchWrite 25 ops / Transact 100 items / 4 MB | per tier table | already covered |
| Tags per resource / key / value | **50 / 128 / 256** | |

### Parity Edge-Case Seeds (ready-made from ExtendDB tests)

Each maps to a roadmap item; translate into `dynamodb_spec/` scenarios. Sources:
`tests/test_item_operations.py`, `tests/python/test_scan_edge_cases.py`, and the
six new commit diffs.

- **D0.2 / D1 validation:** empty string/binary in key positions rejected
  (`5f5e511`); duplicate NS/BS members rejected; SET on a missing parent path
  rejected; arithmetic overflow (SET `+`, ADD, subtraction) rejected; EAN keys
  must start `#` and EAV keys must start `:`; nesting depth 32 (`0448ca0`).
- **D1.8 UpdateItem:** UpdateItem with **no directives** (TableName+Key only) is a
  **no-op upsert** (`754f307`); `Some("")` UpdateExpression still errors
  `"The expression can not be empty;"`; **omit `Attributes` entirely** (not `{}`)
  on UPDATED_NEW/UPDATED_OLD when the filtered map is empty (`5ec827b`);
  UPDATED_* returns leaf-wrapped values for nested paths/list indices.
- **D1.2 / D2.1 / D2.2 expressions:** reject **redundant parentheses** `((x))` in
  Key/Filter/Condition expressions, error names the expression type (`7557eb1`);
  reserved-word rejection (config-gated, enabled to match DynamoDB).
- **D2.1 Query:** accept **reversed** KeyConditionExpression comparisons
  (`:val <= sk` ≡ `sk >= :val`) (`c11fdb6`); **reject `<>`/NE** on key conditions.
- **D2.1 / D2.3 pagination:** malformed `ExclusiveStartKey` (wrong/extra/missing
  attr, wrong scalar type, empty `{}`, GSI start key missing index PK) →
  `ValidationException`, with **distinct Scan vs Query messages** (`9a1a1a6`,
  `test_scan_edge_cases.py`): Scan appends `: The provided key element does not
  match the schema`, Query does not.

### Divergence Decisions To Record (`docs/adapters/dynamodb/divergences.md`)

These ExtendDB/real-DynamoDB behaviors were unaddressed in the plan; each needs a
match/accept/diverge decision:

- **Access-key prefix format** (ExtendDB uses `AKIAEXTENDDB`/`ASIAEXTENDDB`) — pick
  Nimbus's own; wire into D0.5/D7.3.
- **CREATING→ACTIVE table-status transition** — SDK `table_exists` waiters poll
  for `ACTIVE`; decide whether `CreateTable` returns ACTIVE immediately or models
  the transition (D0.6).
- **DeletionProtectionEnabled** on UpdateTable/DeleteTable (D0.6/D6).
- **TTL REMOVE stream records** carry
  `userIdentity:{type:"Service",principalId:"dynamodb.amazonaws.com"}` (real
  DynamoDB matches; D5.4/D6.2).
- **TTL attribute-name charset** — Nimbus can match DynamoDB's any-UTF-8 (ExtendDB
  restricts it only for SQL-injection defense, which Nimbus does not have) (D6.1).
- **TTL modification cooldown** — DynamoDB enforces one; decision for D6.1.
- **Throttling / provisioned capacity** — Nimbus never emits
  `ProvisionedThroughputExceededException`; ExtendDB does by default. (D-deferred,
  but the parity target must be configured — see below.)
- **Numeric key/index precision** — Nimbus's index and ordering path is `f64`
  (~17 significant digits); DynamoDB `N` is exact to 38. Decision: project
  numeric key/index attributes to a full-precision order-preserving sortable
  string (recommended — see "Key And Index Ordering"), or accept-and-document
  the f64 precision ceiling with a regression test. Affects D0.3/D2.1/D4.
- **Binary key/index attributes** — Nimbus has no `FieldType::Binary` and cannot
  index binary today. Decision: project `B` key/index attributes to fixed-case
  hex (order-preserving) in the adapter, or promote `FieldType::Binary` into the
  core index encoding. Affects D0.3/D4.
- **String range collation on SQL backends** — pushed-down SQL range bounds use
  column/locale collation (Postgres locale, MySQL `utf8mb4_general_ci`
  case-insensitive), which diverges from DynamoDB's byte-wise `S` comparison.
  Decision: use a binary collation (`COLLATE "C"` / `utf8mb4_bin`) on projected
  key columns, or rely on the in-engine byte-wise re-sort. Affects D2.1/D4.

### ExtendDB As A Parity Target — Operational Setup (corrects D8.6)

D8.6's "launch ExtendDB on a scratch port" underestimates setup. ExtendDB
requires **`extenddb init` (prints an admin password) → HTTPS/TLS (self-signed,
clients run `verify=False`) → management-API credential provisioning**
(`devtools/provision-test-credentials`). For clean parity, set
`throttling_enabled=false` and `control_plane_delay_seconds=0` on the target,
else it emits throttling errors / CREATING delays Nimbus never will. Also note
ExtendDB's own test suite treats **real DynamoDB** (not DynamoDB Local) as the
oracle, with a no-xfail / no-target-branching discipline and exact
code+message+HTTP-status assertions — reconcile with this plan's "DynamoDB Local
primary" stance (run all three targets; classify any Local-vs-real difference
explicitly). Current ExtendDB test counts: **761** Python `def test_` (was 729),
**348** Rust. The aspirational `tests/{golden,comparison_rules,reference,java,
cpp,external}/` dirs from design-09 are **not present** — do not plan around them.

### Anticipated Hardening (fold in MongoDB adapter audit categories)

The MongoDB adapter needed a follow-on hardening wave
(`docs/plans/archive/mongodb-adapter-hardening-plan.md`). Design these in up
front instead of retrofitting: configurable auth secrets — never hardcode SigV4
keys/secret material or compile-time salts (H1/H2); route transaction writes
through the engine session token, not direct engine calls (H3); centralize tenant
resolution instead of copy-pasting per command file (M1); make `UpdateItem`
ADD/DELETE-set and atomic counters genuinely atomic, not read-modify-write that
loses concurrent updates (M5); use **i64** for stream sequence numbers, not i32
(M6); return precise modeled exceptions for unsupported ops/expressions, not
generic errors (L5); preserve nested/typed fidelity (L7/L8 — ties to D0.1b);
DescribeTable/DescribeLimits/DescribeEndpoints must not be fully canned (L4);
keep command files split by concept under the 1,500-line soft limit (M2).

### Tenant Binding Is A New Pattern

Existing adapters resolve tenant from a **namespace token in the request**
(MongoDB `$db` name; Firebase Firestore `project_id`), and MongoDB uses a single
static `AuthConfig` with no per-credential tenant binding. The plan's
**per-access-key → tenant** mapping has no precedent and needs its own design +
isolation tests. Reusable seam: `TenantIsolationContext::application(tenant_id,
PrincipalContext, surface)` + `ensure_tenant_matches` (in `nimbus-tenant`).

### Operation Surface Notes

ExtendDB actually implements ImportTable/ExportTable, Backup/Restore, **and PITR**
(`crates/engine/src/{import_export.rs,backup}`); the only major op family it does
**not** implement is **PartiQL** (→ `UnknownOperationException`). The plan
correctly defers all of these, but: add `RestoreTableToPointInTime` to the PITR
deferral line for completeness, and add one line that ExtendDB exposes
`AssumeRole`/STS at its server/auth layer (Nimbus has no IAM/STS and correctly
omits it). Stream shard model: ExtendDB's stream **design** recommends N
hash-based shards per table (default 4); the plan's single-shard choice therefore
diverges from **both** ExtendDB's design and real DynamoDB's hierarchical tree —
state that explicitly in the Key Decisions table and D5.2, and confirm the
single-shard divergence is accepted for non-throughput-bound workloads.

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
| Server extraction completion plan | `docs/plans/archive/server-crate-extraction-completion-plan.md` | Current crate naming and facade rules for concrete adapter crates |
| Runtime-capability adapter boundary baseline | `docs/plans/archive/runtime-capability-adapter-boundary-plan.md` | Adapter/runtime ownership baseline |
| Node LTS runtime trust plan | `docs/plans/node-lts-runtime-trust-plan.md` | Provenance precedent for vendored fixture corpora and separate app/library canaries |

## Execution Log

| Date | Item | Status | Description | Verification |
|------|------|--------|-------------|--------------|
| 2026-05-26 | — | `pending` | Plan created. Awaiting promotion to `in_progress` after the active architecture gates and release-readiness plan permit adapter-surface expansion. | — |
| 2026-05-26 | — | `pending` | ExtendDB cloned locally at `/Users/jack/src/github.com/ExtendDB/extenddb` (Apache-2.0, Rust workspace with `crates/{auth,bin,core,engine,server,storage,storage-postgres}`, Python parity corpus under `tests/`, divergence catalogue at `docs/differences-from-dynamodb.md`). Plan references updated to point at the local checkout. | `ls /Users/jack/src/github.com/ExtendDB/extenddb` |
| 2026-05-26 | — | `pending` | Surveyed ExtendDB crate-by-crate for reuse: `extenddb-core` (12,587 LOC, zero workspace deps, pure sync) selected as direct git-rev dependency to inherit AttributeValue, expression language, type model, validation, and error taxonomy. `extenddb-auth/sigv4/` (4 files, ~760 LOC) selected for verbatim vendoring under `auth/sigv4/`. `extenddb-storage` traits + `extenddb-engine` handlers + `extenddb-server` + `extenddb-storage-postgres` rejected (too coupled to ExtendDB's StorageEngine trait and PostgreSQL backend; adapter cost exceeds reimplementation cost). Plan updated with "Upstream Crate Reuse" section, dependency-wiring D0.0 added to the work queue, dependency-management list aligned, module structure annotated `[uses extenddb-core]` where appropriate, D1.1–D1.4 expression items converted from "build parser/evaluator" to "wire upstream parser/evaluator into Nimbus shim". | `find /Users/jack/src/github.com/ExtendDB/extenddb/crates -name '*.rs' \| xargs wc -l` = 49,397 total; per-crate breakdown recorded in plan. |
| 2026-05-28 | — | `pending` | Updated the plan after the adapter crate extraction and naming change. DynamoDB now targets the concrete `crates/nimbus-dynamodb` crate, keeps DynamoDB protocol dependencies out of `nimbus-server`, permits `nimbus-adapters` only as a default-empty optional facade, moves parity/spec tests under the concrete adapter crate, and adds crate-boundary verification gates. | Stale package/path audit recorded no old adapter-suffix crate names and no old server-local parity-test path. |
| 2026-05-28 | — | `pending` | Added enterprise compatibility and production-readiness gates: official SDK matrix, botocore model coverage, feature-parity coverage table, failure injection, tenant isolation proof, soak testing, performance baselines, and an external test reuse policy distinguishing source references, translated scenarios, and vendored fixtures. | `git diff --check -- docs/plans/dynamodb-adapter-plan.md`; stale old-path audit remained clean. |
| 2026-05-28 | — | `pending` | Re-reviewed the freshly updated ExtendDB checkout (`5f5e511`) and changed the plan from "small vendored fixtures only" to a Node-LTS-style external suite adoption policy plus a separate canary-app matrix. ExtendDB has dual-target Python/boto3 tests, a Rust SDK integration test crate, external suite registry precedent, real-DynamoDB validation mode, and explicit supported-operation/difference docs. Large reputable suites should run by pinned path/version first, then be vendored by upstream release tag or commit SHA before release when license-compatible and valuable. Canary evidence is reserved for real apps/framework/library integrations, not upstream test suites. | `git pull --ff-only` in ExtendDB; `find tests -type f`; `rg -n "def test_" tests \| wc -l` = 729; `rg -n "#\\[(tokio::test\|test)\\]" tests/rust/src \| wc -l` = 348; inspected `tests/README.md`, `tests/python/README.md`, `tests/rust/Cargo.toml`, `tests/rust/src/test_base.rs`, `docs/design/09-testing.md`, `external-suites.sample.toml`; compared terminology with `docs/plans/node-lts-runtime-trust-plan.md`. |
| 2026-05-29 | — | `pending` | Made the plan `/goal`-ready against the repo's verifier-gated control-plane convention. Added a `## Goal Control Plane` section (objective + paste-ready `/goal` prompt), a `## Completion Gate` section enumerating 21 machine-checkable conditions backed by `scripts/verify-dynamodb-adapter.sh`, and a `### Verification Evidence Conventions` subsection that binds "focused tests" / "AWS SDK X succeeds" / "parity diff clean" / "classification report committed" to concrete commands + assertions. Added roadmap item D0.0a (verifier scaffold that fails on every unimplemented gate + start-prompt file) as the first D0 row and made D0.0 depend on it; wired D9.7 closeout and the D9 ledger row to require the verifier prints `N passed, 0 failed`; updated the Status control item and noted the server-extraction promotion gate is satisfied (closed 2026-05-28). Tightened the bare "Tests for X" evidence cells on D0.2/D1.3/D2.4/D5.4 to name the test lane + assertion. Confirmed `nimbus-tenant`, `nimbus-adapters`, and `nimbus-mongodb` crates exist and the facade has no `dynamodb` feature yet (D0.9 remains correct). | `ls scripts/verify-*.sh` (convention survey); `tail scripts/verify-node-dbus-binding.sh` (summary-line shape `N passed, N failed`, exit-1-on-fail); `sed -n '1,40p' crates/nimbus-adapters/Cargo.toml`; `ls crates/nimbus-{tenant,adapters,mongodb}`. Plan is still `pending`; no implementation work started. |
| 2026-05-29 | — | `pending` | Pulled ExtendDB `5f5e511 → 0448ca0` (6 DynamoDB-fidelity fixes) and ran a two-track deep review (ExtendDB-vs-plan-claims + comparison against the four existing concrete adapter crates). **Architecture corrections:** (1) `nimbus-dynamodb` exposes a transport-agnostic `dispatch(...)` entrypoint with **no `axum` dep** — no concrete adapter owns a `Router`; `nimbus-server` owns the `POST /` route (rejected-alternative recorded in Key Decisions); (2) the parity runner moves to `crates/nimbus-server/tests/dynamodb_spec/` (mirrors `mongodb_spec/`, whose executor imports `nimbus_server`) and the live dual-target diff harness is net-new, not a copy; (3) added D0.1b to **promote `nimbus-core` typed-scalar** (DynamoDB `N`/`SS`/`NS`/`BS` + nesting) before the codec — `TypedScalarValue` is a closed MongoDB/Firebase enum with flat top-level keys and cannot represent DynamoDB nested/set typed scalars (verified `typed_scalar.rs:16-27`, `document.rs:18`); (4) composite key-size divergence — DynamoDB 3,072B pk+sk exceeds Nimbus's 1,500B `validate_document_key` (verified `types.rs:443`), plus sort-range must use typed `_sk`. **Factual fixes:** LOC (core 13,810/engine 7,019/storage-postgres ~11,923/policy 2,721), SigV4 is 5 files (+`mod.rs`), dispatch shape is `engine/src/lib.rs:292-387` not `server/handler.rs`, reserved words = 573 in a 615-line file and ExtendDB **does** reject them (config-gated), entrypoint is `serve()`/`construction.rs` not `serve_with_options`/`lib.rs`, pin ≥`0448ca0`. **New content:** hard-limits table (400KB item, 32 nesting depth, 1MB page, GSI 20/LSI 5, etc.), parity edge-case seed catalog, divergence decisions (access-key prefix, CREATING→ACTIVE waiters, DeletionProtection, TTL `userIdentity`/charset/cooldown, throttling), ExtendDB-as-parity-target operational setup (init/TLS/credentials, `throttling_enabled=false`), anticipated-hardening list from the MongoDB audit, tenant-binding-is-new note, operation-surface notes (ExtendDB implements Import/Export/Backup/PITR; only PartiQL absent). | `git -C /Users/jack/src/github.com/ExtendDB/extenddb pull --ff-only` (`5f5e511..0448ca0`); `git log --oneline 5f5e511..0448ca0`; `wc -l` per-crate; read `typed_scalar.rs`, `document.rs`, `types.rs:443`; `grep` confirmed no `axum` in any concrete adapter Cargo.toml; `crates/nimbus-server/tests/mongodb_spec/executor.rs:5` imports `nimbus_server::adapters_mongodb::listener`. Plan still `pending`; no implementation work started. `git diff --check` clean. |
| 2026-05-29 | — | `pending` | Recorded the storage-layout decision explicitly after a UUID-tables / ExtendDB comparison. Added a "Physical storage layout" row to Key Architectural Decisions (shared `documents(table_id, id)`, **not** per-table UUID physical tables) and a storage-layout bullet to Current Assessed State (cross-adapter premise + multi-backend portability + `TableId` already solving rename/name-reuse; cites MBA10 and `TableBackendLayout`). Sharpened the `extenddb-storage`/`-postgres` reuse-matrix rows to name the per-table-physical divergence as the reason for skip. Clarified `DeleteTable` (operation table + D0.6) as a bulk delete over the shared table via the `deleting` lifecycle state, not `DROP TABLE`. Made Query partition-equality an explicit `DocumentId` prefix scan (D2 phase + D2.1). Added a GSI/LSI type-correct-ordering requirement routed through MBA11 typed-column storage with a numeric-GSI-range test (D4 phase + D4.3). Added risk R12 (shared-table Query/GSI performance, with the per-table `TableBackendLayout` escape hatch). Corrected the composite-key-encoding decision row to stop claiming full-size keys "fit" (they exceed the 1,500 B DocumentId budget — see D0.3/R2). No design reversal: the shared-documents layout is the correct and required choice for this adapter. | Verified citations: `docs/plans/archive/multi-backend-adapter-hardening-plan.md:84` (MBA10), `crates/nimbus-storage/src/table_identity.rs:176` (`TableBackendLayout`), `crates/nimbus-core/src/types.rs:103,441` (`TableId` ULID + 1,500 B key limit), `docs/architecture/storage/typed-key-columns.md` (MBA11), ExtendDB `crates/storage-postgres/src/data/mod.rs:19` (`_ddb_` physical names). Plan still `pending`; no implementation work started. |
| 2026-05-29 | — | `pending` | Storage-layer audit of index/key ordering to resolve the D4.3 open item. **Corrected** the earlier (inaccurate) claim that MBA11 `sort_s`/`sort_n`/`sort_b` typed columns close the GSI ordering gap: those columns are not materialized (SQLite/Postgres use `json_extract` / `jsonb_extract_path_text` expression indexes, MySQL uses generated VIRTUAL columns, redb uses an order-preserving byte encoding), numbers run through `f64` everywhere (`index/encoding.rs:11`, `evaluator/ordering.rs:66`, `evaluator/filtering.rs:59`; SQL casts to DOUBLE), `FieldType` has no `Binary` variant (`schema.rs:31`) so binary keys are not indexable, and the engine always re-sorts in memory and load-then-truncates. Added a "Key And Index Ordering: Numeric Precision And Binary Keys" subsection (finding + adapter-local resolution: project key/index attributes to order-preserving sortable strings — S raw, N full-precision sortable decimal, B hex), tightened D0.3 (prose + roadmap row) and the D4 phase bullet / D4.3 row to require that encoding with f64-collision and binary-order tests, adjusted R12 to record the confirmed in-engine re-sort, and added three divergence-decision entries (numeric precision, binary keys, SQL string collation). | Verified: `crates/nimbus-storage/src/index/encoding.rs:5-42` (f64 numbers, binary rejected), `crates/nimbus-core/src/schema.rs:31-38` (`FieldType` has no Binary), `crates/nimbus-engine/src/evaluator/ordering.rs:31-66` + `filtering.rs:54-64` (f64 compare, mixed-type rejected), `docs/architecture/storage/typed-key-columns.md` (expression-or-column, binary unsupported). Plan still `pending`; no implementation work started. |
| 2026-05-29 | — | `pending` | Reconciled the `_sk` dual-definition introduced during the index-ordering edit: D0.3 now states `_pk`/`_sk` (and per-index `_gsi1_pk`/`_gsi1_sk` fields) hold the order-preserving sortable projection, while the original AttributeValues are recovered from typed-scalar metadata or the reversible base64url DocumentId — not from `_sk`. Added the GSI/LSI per-index projected-field model and a `docs/technical-debt.md` **T-005** cross-reference (the adapter-local projection is a workaround, not a closure of T-005's generic SQL fix). Then made the worktree/PR delivery model explicit (matching the completed `node-dbus-binding` wave): added a `### Branch, CI, and PR workflow` subsection to Goal Control Plane (isolated `dynamodb-adapter` worktree branch, CI-is-verification, PR-as-last-step, clean-base note, verifier-vs-process), updated the suggested `/goal` prompt to create the worktree first and open the `dynamodb-adapter → main` PR last (never push to `main`, never self-merge), added a Status "Delivery" bullet, folded worktree creation into D0.0a, and added push/branch-CI-green/open-PR to the D9.7 closeout gate. | `git diff --check` clean; verified the prior `_sk` contradiction grep returns no leftover "store original in `_pk`/`_sk`"; mirrored the NDB convention at `docs/plans/archive/node-dbus-client-binding-plan.md:269-294`. Plan still `pending`; no implementation work started. |
| 2026-05-29 | D0.0a | `done` | **Promoted the plan and scaffolded the control plane on the isolated `dynamodb-adapter` worktree branch.** Confirmed both promotion conditions (server-extraction closed 2026-05-28; release-readiness plan completed, no adapter-surface freeze). Created the worktree (`git worktree add ../nimbus-dynamodb-adapter -b dynamodb-adapter`) carrying the refined plan via a path-scoped stash (other unrelated dirty files left on `main`). Wrote `scripts/verify-dynamodb-adapter.sh` — a 23-check aggregate gate enumerating the Completion Gate (C1–C21 + a 2-check structural preflight) following the repo verifier convention (`pass`/`fail` helpers, bold `N passed, M failed` summary, exit 1 on any failure); it FAILS on every unimplemented gate today, with only plan-structure + `git diff --check` green. Fixed a self-caught false positive (C17 harness-case check was matching the plan-doc *mentions*; now searches only `crates`/`.github/workflows`). Wrote `docs/prompts/dynamodb-adapter-start.md` from the Goal Control Plane objective. Flipped Plan status → `in_progress`, D0 phase → `in_progress`, recorded the active item (D0.0). | `git worktree list` shows `dynamodb-adapter` at `../nimbus-dynamodb-adapter`; `bash scripts/verify-dynamodb-adapter.sh` → **`3 passed, 20 failed`, exit 1** (must not pass before work lands ✓; greens are plan-structure ×2 + `git diff --check`); `shellcheck` clean except intentional SC2016 regex-in-single-quotes; start-prompt file present. Next: D0.0 (crate scaffold + `extenddb-core` pin ≥`0448ca0` + vendor 5 SigV4 files + NOTICE). |
| 2026-05-29 | D0.0 | `in_progress` | **Groundwork / dependency de-risking** (the central D0.0 decision). Confirmed the `extenddb-core` **git-rev-pin (Option A) is viable** — `GIT_TERMINAL_PROMPT=0 git ls-remote https://github.com/ExtendDB/extenddb HEAD` returns `0448ca0…` with exit 0 and **no auth prompt**, so the repo is public over HTTPS and Cargo can fetch it locally and in CI (the vendor-everything fallback, Option C, is not needed). Pin rev = `0448ca066c86bddf2eb465092b8b6854923665ee`; `extenddb-core` package name confirmed. Captured the concrete-adapter Cargo shape from `crates/nimbus-mongodb/Cargo.toml` (`version.workspace`, `[lib] doctest=false`, `[lints] workspace`, path deps `nimbus-core`/`-engine`/`-tenant`) to mirror. **Adaptation flagged for vendoring:** ExtendDB SigV4 uses `axum::http::HeaderMap`, but the adapter must not depend on `axum` (architecture decision) — vendor against the `http` crate (`http::HeaderMap`, which axum re-exports) and mark the change per Apache-2.0 §4(b). | `git ls-remote https://github.com/ExtendDB/extenddb HEAD` → `0448ca0…` exit 0 (public, no auth); `git -C …/ExtendDB/extenddb rev-parse HEAD` = `0448ca066c86bddf2eb465092b8b6854923665ee`; `grep name …/extenddb/crates/core/Cargo.toml` = `extenddb-core`. **Remaining for D0.0 done:** create `crates/nimbus-dynamodb/{Cargo.toml,src/lib.rs}`, add to `[workspace] members` + `extenddb-core` to `[workspace.dependencies]`, vendor 5 SigV4 files (http-adapted), create `NOTICE`, `cargo check -p nimbus-dynamodb` clean, `cargo tree` shows `extenddb-core`, `make deny` clean. |
| 2026-05-29 | D0.0 | `done` | **Scaffolded `nimbus-dynamodb` and wired `extenddb-core` + vendored SigV4.** Added `crates/nimbus-dynamodb` (package `nimbus-dynamodb`, mirrors the mongodb Cargo shape: `version.workspace`, `[lib] doctest=false`, `[lints] workspace`) to `[workspace] members`; pinned `extenddb-core = { git = ".../ExtendDB/extenddb", rev = "0448ca066c86bddf2eb465092b8b6854923665ee" }` in `[workspace.dependencies]`. Crate deps: `extenddb-core`, `http`, `hmac`, `sha2`, `hex` (no `axum`, no `nimbus-server`). Vendored the **5** SigV4 files into `src/auth/sigv4/`; `canonical.rs` + `verify.rs` adapted `axum::http::HeaderMap` → `http::HeaderMap` with an Apache-2.0 §4(b) banner; all 5 keep their SPDX `Apache-2.0` headers; the 3 others are verbatim. Created repo-root `NOTICE`. Allowlisted the ExtendDB git source in `deny.toml`. `lib.rs` exposes `pub mod auth` (dispatch/config land in D0.1). Fixed two self-caught verifier bugs: `grep_q` mishandled a leading `-i` flag (now forwards all args to grep), and C4 grepped "Apache License" but the vendored files use the SPDX `Apache-2.0` identifier (now matches either). | `cargo check -p nimbus-dynamodb` clean (9.34s; fetched the git rev, compiled extenddb-core + deps); `cargo test -p nimbus-dynamodb` → **21 passed, 0 failed** (vendored SigV4 unit tests); `cargo tree -i extenddb-core` shows only `nimbus-dynamodb` consumes it; `cargo tree -p nimbus-server -i nimbus-dynamodb`/`-i extenddb-core` → "did not match" (boundary clean); `make deny` → **advisories/bans/licenses/sources ok**; `cargo fmt -p nimbus-dynamodb --check` clean; `git diff --check` clean; verifier → **`5 passed, 18 failed`** (C2 + C4 flipped green; C3 stays red until D0.1 wires the server). **Note:** `cargo check -p nimbus-server` cannot run via direct cargo here — it fails on the pre-existing `nimbus-ui` dist `build.rs` guard (LD7 local-dev contract: server builds route through `make`/CI, which build the SPA). This is unaffected by D0.0 (server does not depend on `nimbus-dynamodb`); workspace resolution is proven by `nimbus-dynamodb` checking clean. Server compile is verified by branch CI. Next: **D0.1** — `dispatch(target, body, &Arc<Service>, &auth)` entrypoint + `nimbus-server` `POST /` route + `ServeOptions::with_dynamodb` + `record_listener_state_async`. |
| 2026-05-29 | D0.1 | `in_progress` | **Adapter-side dispatch + wire envelope done** (server wiring is the remaining half). Added `wire.rs` (`extract_operation` — mirrors ExtendDB `request_helpers.rs`: missing target + auth → `UnknownOperationException`, missing target + no auth → `MissingAuthenticationToken`, wrong/absent prefix → `UnknownOperationException`; `render_error` — reuses `extenddb_core::DynamoDbError::{full_error_type,status_code,message,cancellation_reasons,condition_check_item}` so `__type` prefix, HTTP status, message-omission, and `CancellationReasons`/`Item` envelopes are parity-correct; `render_success`). Added `dispatch.rs` (`KNOWN_OPERATIONS` — the 26 T0–T7 ops; `dispatch` mirrors the real DynamoDB order: parse target → reject unknown op pre-auth → reject malformed JSON pre-auth → route; recognized ops hit a `not-yet-implemented` 500 placeholder that each later item replaces). `lib.rs` re-exports `dispatch`/`render_error`/etc. Added `serde_json` dep. Scoped `#[allow(clippy::collapsible_if)]` on the vendored `sigv4` module (held to upstream's lint baseline; verbatim copy not restructured). | `cargo test -p nimbus-dynamodb` → **33 passed, 0 failed** (21 SigV4 + 12 new: target extraction for data/streams prefixes, missing-target-with/without-auth, wrong-prefix, unknown-op, malformed-body→SerializationException, valid-body→placeholder, envelope `__type`+message-omission); `cargo clippy -p nimbus-dynamodb --all-targets -- -D warnings` clean; `cargo fmt -p nimbus-dynamodb --check` clean; `git diff --check` clean; verifier still `5 passed, 18 failed` (C3 stays red until the server is wired). **Remaining for D0.1 done:** add `DynamoDbConfig` + `ServeOptions::with_dynamodb` + `POST /` route calling `nimbus_dynamodb::dispatch` + bind/spawn/abort + `record_listener_state_async` in `nimbus-server`, and a server listener-wiring test. This leg requires the `nimbus-ui` dist (build via `make`) to compile-verify `nimbus-server` locally, or relies on branch CI. |
| 2026-05-29 | D0.1 | `in_progress` | Added `config.rs` with `DynamoDbConfig` (the type `ServeOptions::with_dynamodb` will consume; owned by the adapter per the boundary). Mirrors `MongoDbConfig`: `bind_addr` field, `DynamoDbConfig::new(port)` (localhost-only), `with_bind_addr`, `Default` = `127.0.0.1:8000` (DynamoDB Local convention). SigV4 `auth_mode` deferred to D7. Re-exported from `lib.rs`. | `cargo test -p nimbus-dynamodb` → **36 passed, 0 failed** (+3 config: default port-8000-loopback, `new` port, `with_bind_addr` override); `cargo clippy -p nimbus-dynamodb --all-targets -- -D warnings` clean; `cargo fmt --check` clean; `git diff --check` clean. **Remaining for D0.1 done (next chunk):** the `nimbus-server` leg — `ServeOptions::with_dynamodb` + `POST /` route → `dispatch` + bind/spawn/abort + `record_listener_state_async` + a server test. First action next window: `npm ci` + `make build-ui` to produce the `nimbus-ui` dist (absent in a fresh worktree) so `nimbus-server` compiles locally; this one-time unblock also enables the D8 harness + parity-runner work that lives in `nimbus-server/tests/`. |
| 2026-05-29 | D0.1 | `done` | **Server-side listener wiring done — D0.1 complete.** Built the `nimbus-ui` dist (`npm ci` → 512 pkgs, 4s; `make build-ui` → vite built in 785ms) so `nimbus-server` compiles via direct cargo (the LD7 build-contract blocker). Added `crates/nimbus-server/src/adapters/dynamodb/{mod.rs,listener.rs}`: `mod.rs` re-exports `DynamoDbConfig`; `listener.rs` builds a one-route `POST /` axum app (`router`) over `Arc<Service>` and `run_listener` serves it via `axum::serve`; the handler calls `nimbus_dynamodb::dispatch(&headers, &body)` and renders the `(status, json)` with `Content-Type: application/x-amz-json-1.0`. Registered `pub mod dynamodb` in `adapters/mod.rs`; added `nimbus-dynamodb` path dep to `nimbus-server/Cargo.toml`. Wired `construction.rs`: `dynamodb_config` field + `ServeOptions::with_dynamodb`; refactored `serve()` to collect sibling adapter listener handles into a `Vec` (cleanly supports mongodb + dynamodb together, replacing the single-listener early-return), binding the dynamodb listener, calling `record_listener_state_async(&service, "dynamodb", "http", ...)`, spawning `run_listener`, and aborting all handles after the main server returns. `_service` is threaded through the handler for when op handlers consume it (D0.5/D0.6). | `cargo check -p nimbus-server` clean (18s); `cargo test -p nimbus-server --lib adapters::dynamodb` → **2 passed, 0 failed** (oneshot through the real `router`: unknown target → 400 `UnknownOperationException` envelope; known target `PutItem` → 500 not-yet-implemented placeholder, proving the route wires into `dispatch`); `cargo clippy -p nimbus-server --lib -- -D warnings` clean (dropped a redundant `#[must_use]` flagged by `double_must_use`); `cargo fmt -p nimbus-server --check` clean; `git diff --check` clean; verifier → **`6 passed, 17 failed`** (C3 boundary flipped green: server depends on `nimbus-dynamodb`, no `axum`/`extenddb-core` leak). Next: **D0.1b** — promote `nimbus_core::typed_scalar` (DynamoDB `N`/`SS`/`NS`/`BS` + nesting) before the AttributeValue codec (D0.2). |    
