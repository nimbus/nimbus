# Firebase Adapter Plan

Canonical execution plan for the Firebase/Firestore compatibility adapter:
Rust server-side protocol adapters, Neovex core primitive hardening, a
Firebase-shaped JavaScript SDK package, and compatibility tests against the
Firestore v1 gRPC and REST protocols.

This plan is deliberately architecture-first. Firebase is the second major
compatibility layer after Convex, so it is also the forcing function for
separating Neovex-native database primitives from adapter-specific protocol
translation. Work from this plan must reduce core logic living inside adapters;
when Firebase and Convex need the same behavior, promote that behavior to a
protocol-neutral Neovex primitive before adding another adapter-local copy.

## Context

Neovex already ships a deep Convex compatibility adapter covering protocol
routes, WebSocket subscriptions, JavaScript package behavior, V8 host bridge
operations, auth, and manifest loading. The Firebase adapter follows the same
registration style (`crates/neovex-server/src/adapters/firebase/` beside
`adapters/convex/`), but Firestore is a data API rather than a function runtime.
There is no uploaded JavaScript, function registry, bundle manifest, or V8 host
bridge.

That difference is important: the Firebase adapter should be mostly protocol
translation around shared database primitives. If the implementation starts to
grow adapter-local storage semantics, query planning, transaction bookkeeping,
field transform behavior, or subscription diffing that Convex or native Neovex
could also use, stop and add the shared seam first.

## Status

- **Plan status:** `done`
- **Control item:** `complete`
- **Status values:** `pending`, `in_progress`, `done`, `blocked`
- **Primary source of truth:** this file plus the current git worktree.
- **Checkpoint rule:** every work session that changes implementation state
  must update the roadmap item status, the phase ledger, and the execution log
  before stopping.

This is a control-plane plan. Future agents should not reconstruct Firebase
adapter progress from chat history. Start from `git status --short`, this
plan's status fields, and the execution log.

## Plan Ownership And Canonical Inputs

This is the active owner for Firebase-driven compatibility work and the
Neovex primitive hardening required to make that adapter clean. When this plan
conflicts with archived notes, prompts, or exploratory review output, follow
this plan plus the references at the end of the document.

Implementation work must keep the immediate source inputs open:

- Top-level repo references: `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
  and `docs/plans/README.md`.
- Convex/API compatibility references:
  `docs/reference/convex-ai-guidelines.md`, `docs/reference/cli.md`, and
  `docs/convex/compatibility.md`.
- Firebase protocol sources: Firestore v1 proto files and Firebase JS SDK
  source listed in the Source Evidence Map.
- Neovex seam sources: core types/mutations/query, engine execution units and
  subscriptions, server router/state/security, and the Convex adapter seams
  listed below.

## Current Assessed State

- F0.1 landed: `DocumentId` is now a validated string-backed key that supports
  caller-provided Firestore-style IDs plus generated ULID-style IDs. Remaining
  Firestore identity work is full resource-path and collection metadata, not
  leaf-key generation.
- F0.2 landed: raw Firestore collection IDs now live in
  `CollectionName` / `CollectionPath` / `DocumentPath`, while `TableName`
  stays the logical storage and schema identifier. Resource lookup metadata is
  stored in sidecar path bindings plus a collection-group index instead of
  delimiter-encoded table names or user document fields.
- `Mutation` and `MutationExecutionUnit` support mixed insert/update/delete
  staging, but not the protocol-neutral explicit-key set/patch/delete/verify/
  transform batch surface that Firestore requires.
- The current query model is single-table and basic-filter oriented. Firestore
  needs projections, composite filters, cursors, offsets, document ID ordering,
  collection groups, and explicit unsupported-feature errors.
- Convex creates and commits execution units within one mutation invocation.
  Firestore transactions require server-side transaction/session state across
  separate RPCs.
- The current subscription API emits result snapshots and commit metadata, not
  Firestore watch target-change state, resume tokens, or existence filters.
- Server route-family policy, CORS, and middleware tests have Convex coverage
  but no Firebase family yet.
- Browser full-SDK drop-in remains out of scope because the stock Firebase
  browser SDK uses WebChannel. `@neovex/firebase` may use gRPC-Web plus a
  documented WebSocket Listen transport.

## Control Plan Rules

1. Read `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
   `docs/plans/README.md`, and this plan before starting a roadmap item.
2. Run `git status --short` before choosing work. If the worktree is dirty,
   inspect the changed files and reconcile them with the current `in_progress`
   item or execution log before editing.
3. If any roadmap item is `in_progress`, resume that item. If none is
   `in_progress`, pick the first `pending` item in roadmap order whose hard
   dependencies are `done`.
4. Mark exactly one item `in_progress` before implementation. Do not advance
   another item until the active item is `done` or `blocked`.
5. Prefer one roadmap item per context window. If an item cannot fit with its
   relevant source, implementation, tests, and checkpoint loaded at once, split
   the item in this plan before starting it.
6. Firebase work that discovers shared database behavior in the Convex adapter
   must either promote that behavior into a Neovex primitive or add a design
   note here explaining why it remains adapter-specific.
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
- Run `make clippy` before any PR or after shared primitive work that touches
  `neovex-core`, `neovex-engine`, `neovex-storage`, or `neovex-server`
  behavior broadly.
- For JavaScript package work, run the relevant package build/typecheck/test
  command plus root `npm run typecheck` when exported API surfaces change.
- For docs-only control-plane edits, verify references and status consistency
  with `rg`/manual review; run `npm run docs:validate-refs:strict` if the
  script exists in the current checkout.

## Compatibility Tiers

The plan does not promise "all Firebase SDKs on day one." It uses explicit
tiers so scope and tests stay honest.

| Tier | Goal | Required transport/features |
|------|------|-----------------------------|
| T0 | Neovex primitive hardening | Protocol-neutral document keys, resource paths, mutation batches, query AST, transaction sessions, subscription snapshots |
| T1 | Firestore Lite / REST CRUD | REST `Commit`, `BatchGetDocuments`, `RunQuery`, serializer, paths, preconditions, basic queries |
| T2 | Full client SDK data path | Native gRPC, `Write` bidi stream, `Listen` bidi stream, transactions, watch aggregation |
| T3 | Query/admin breadth | Aggregations, collection groups, `BatchWrite`, Admin SDK compatibility matrix |
| Deferred | Stock browser SDK drop-in | WebChannel transport, emulator control endpoints, advanced pipeline/vector APIs |

The `@neovex/firebase` package may use gRPC-Web for browser unary and
server-streaming RPCs, plus a WebSocket transport for `Listen`. The stock
`firebase/firestore` browser SDK still uses WebChannel and remains deferred.

## Architecture Boundary Contract

### Neovex Core Owns

- Document identity and resource addressing.
- Atomic write batch semantics, including explicit document keys, upsert,
  overwrite, patch, delete, verify, and field transforms.
- Query representation and execution for adapter-neutral filters, ordering,
  cursors, projections, limits, offsets, and collection-group-style scans.
- Transaction/session lifecycle for data transactions: token creation, read set
  tracking, commit/rollback, timeout, principal binding, and cleanup.
- Subscription result snapshots and commit metadata sufficient for adapters to
  build protocol-specific watch messages.
- Storage-visible metadata needed for path/indexing behavior, kept separate
  from user document fields.
- Protocol-neutral error taxonomy that adapters can map into Convex, Firebase
  gRPC, Firebase REST, or Neovex-native responses.

### Adapters Own

- Protocol parsing and serialization: Firestore protobuf/JSON, Convex request
  envelopes, WebSocket frames, SDK-specific payloads.
- Protocol-specific auth extraction and identity mapping.
- Protocol-specific error envelope/status mapping.
- Transport mechanics: axum extractors, tonic request/response types,
  gRPC-Web, WebSocket framing, and SDK reconnection behavior.
- Compatibility quirks that are unique to one SDK and do not belong in the
  core data model.

### Convex Cleanup Rule

Before landing Firebase work that resembles existing Convex adapter logic,
compare the two paths:

- If the logic is about Neovex data semantics, move it to `neovex-core`,
  `neovex-engine`, or a shared server support module and thin the Convex call
  site.
- If the logic is about Convex protocol shape, runtime host calls, function
  manifests, or Convex auth envelope handling, keep it in the Convex adapter.
- If both answers seem plausible, add a short design note to this plan before
  implementation. The goal is fewer adapter-local databases hiding inside
  compatibility code.

## Required Core Primitive Work

### C0.1: Document Keys And Resource Paths

Neovex now accepts caller-provided leaf document keys via `DocumentId`, while
still generating ULID-shaped IDs by default. Firestore identity still carries
more structure than a leaf key: full collection/document alternation, nested
collection ancestry, and path-derived metadata.

Keep the shared document-key contract explicit:

- Engine APIs must support caller-provided document keys as well as generated
  keys.
- Generated Neovex and Convex IDs can remain ULID-shaped strings, but the core
  storage/key abstraction must not require every adapter to parse a ULID.
- Resource paths must preserve collection/document segment alternation and
  arbitrary valid segment text. Do not encode Firestore IDs into user fields.
- Any reserved storage metadata must live outside user document fields or in a
  collision-proof metadata namespace, not as `_parent_id` inside user data.
- F0.2 must not treat widened leaf keys as sufficient for nested-resource
  identity; path/collection metadata still owns collision-free subcollection
  semantics.

Decision already landed in F0.1: `DocumentId` is the canonical validated
string-backed leaf key. The remaining open identity decision is whether
collection/path metadata widens `TableName` or introduces a separate
collection/path identifier type for raw Firestore collection segments.

### C0.2: Collection Paths And Collection Groups

The old `parent__child` table convention is not sufficient. It loses ancestors
for `a/1/b/2/c/3`, collides with collection names containing `__`, and makes
`_parent_id` a user-field conflict.

Add an adapter-neutral path model:

- Store or index the full ancestor path for documents with nested collections.
- Represent the collection group (`landmarks`) separately from the full
  collection path (`cities/SF/landmarks`).
- Support deterministic table/index mapping without lossy delimiter tricks.
- Make collection group queries use path metadata or a core index, not a
  wildcard scan over table names.
- Resolve the collection-name boundary explicitly: either widen the
  storage-facing collection/table identifier or add a separate collection/path
  identifier type. Raw Firestore collection segments must not be forced through
  `validate_logical_name`.

### C0.3: Atomic Write Batch Primitive

`MutationExecutionUnit` already provides one-shot atomic staging and OCC for
mixed insert/update/delete, but its current public staging methods are too
Neovex-native for Firestore. Add a protocol-neutral batch operation layer over
the same engine-owned commit path:

- `Set { key, document, mode }`, where mode covers create-only, overwrite, and
  merge/mask. Overwrite must support create-if-missing atomically inside the
  execution unit.
- `Patch { key, field_patch, mask, precondition }`.
- `Delete { key, precondition, missing_ok }`, where default delete behavior can
  succeed on missing documents when no precondition is supplied.
- `Verify { key, precondition }`.
- `Transform { key, transforms, precondition }`, including transforms attached
  to `updateTransforms` and standalone `transform` writes.
- Ordered per-write results with commit/update times and transform results.

All Firestore writes in one `Commit` must stage into a single execution unit
and commit atomically. `BatchWrite` is different: it returns per-write statuses
and is not atomic.

Do not model overwrite by calling today's `update_document()` path or silent
delete by calling today's `delete_document()` path; the batch primitive must
express those semantics directly over staged writes.

### C0.4: Query AST And Planner Surface

The current Neovex query shape is single-table and basic-filter oriented. The
adapter should not implement a private Firestore query engine. Add or promote a
shared query representation that can express:

- `select`, `from`, `where`, repeated `orderBy`, `startAt`, `endAt`, `offset`,
  `limit`, and `limitToLast` behavior.
- Composite `AND`/`OR` filters.
- Field filters: equality, inequality, array membership, `IN`, `NOT_IN`,
  `ARRAY_CONTAINS_ANY`.
- Unary filters: `IS_NULL`, `IS_NOT_NULL`, `IS_NAN`, `IS_NOT_NAN`.
- Document-key ordering/filtering needed by Firestore `documentId()`,
  including implicit `__name__` tie-break ordering rules.
- Collection group scans over the path model from C0.2.

Unsupported proto fields such as vector `findNearest` must return a clear
`UNIMPLEMENTED`/`INVALID_ARGUMENT` mapping instead of being silently ignored.

### C0.5: Transaction Session Manager

Do not store raw `MutationExecutionUnit` values in the Firebase adapter. Native
gRPC `BeginTransaction` spans multiple RPC calls, so the lifecycle needs a
server/engine-owned session manager:

- Opaque transaction token maps to tenant, database, principal, read/write
  mode, snapshot, execution unit, and expiration time.
- Reads with a transaction token go through the tracked execution unit.
- `Commit` with a transaction token consumes the session and commits once.
- `Rollback`, disconnect, timeout, or auth mismatch drops the session.
- Tokens are unguessable, bounded in count/size, and never portable across
  tenants or principals.

The Firebase JS full SDK often uses write preconditions instead of
`BeginTransaction`, but native/Admin clients do use transaction RPCs. Both
paths must be supported or explicitly excluded from a tier.

### C0.6: Subscription Snapshot And Watch Support

Neovex subscriptions can be reused, but Firestore `Listen` needs a protocol
state machine on top of them. Promote shared pieces and keep Firebase-specific
state in the adapter:

- Core/subscription layer should expose stable result snapshots, deleted
  document hints, commit sequence/time, and enough metadata to diff a target.
- A shared helper may compute added/modified/removed changes between snapshots.
- Firebase adapter owns target IDs, resume tokens, existence filters, limbo
  labels, target reset/current/remove messages, and WebSocket/gRPC stream
  framing.
- Convex subscription transforms should not be copied; if a useful generic
  diff helper exists there, extract it.

### C0.7: Server Routing, Security, And CORS Families

Add a Firebase route family to local-server security classification before
opening `/v1/*` routes. The adapter must define:

- Which routes are public Firebase application-auth routes versus local
  server-access routes.
- Audit metadata for Firebase REST, gRPC, gRPC-Web, and WebSocket requests.
- CORS headers and content types required by Firebase REST/gRPC-Web, including
  Proto3 JSON over `text/plain` for REST plus:
  `authorization`, `content-type`, `google-cloud-resource-prefix`,
  `x-goog-request-params`, `x-goog-api-client`, `x-goog-api-key`,
  `x-firebase-gmpid`, `x-firebase-appcheck`, `x-grpc-web`, `grpc-timeout`, and
  response trailer exposure where needed.
- Middleware ordering for axum, tonic, `tonic-web`, auth, CORS, rate limits,
  and local origin checks.

The C0 sections are design contracts. The F0 work queue below is the
context-window execution grouping that turns those contracts into code.

## Protocol Specification

The adapter targets the Firestore v1 API:

- gRPC service `google.firestore.v1.Firestore`.
- REST endpoints under `/v1/projects/{projectId}/databases/{databaseId}`.
- Proto3 JSON for REST, including the Firebase JS SDK's `Content-Type:
  text/plain` request shape.
- Native protobuf messages for gRPC and WebSocket binary frames.

### RPC Scope By Tier

| RPC | Type | Tier | Notes |
|-----|------|------|-------|
| `Commit` | Unary | T1/T3 | Atomic writes; T1 covers set/patch/delete/verify plus transform recognition/error mapping, executable transforms land in F3.4 |
| `BatchGetDocuments` | Server streaming | T1 | REST + gRPC document reads, transaction reads by T2 |
| `RunQuery` | Server streaming | T1/T3 | Basic filters in T1, broader query surface in T3; REST has root and parent-scoped bindings |
| `RunAggregationQuery` | Server streaming | T3 | REST + gRPC; count/sum/avg require engine aggregation support |
| `Write` | Bidirectional streaming | T2 | Required for full JS SDK write pipeline |
| `Listen` | Bidirectional streaming | T2 | Required for `onSnapshot` |
| `BeginTransaction` | Unary | T2 | Requires C0.5 session manager |
| `Rollback` | Unary | T2 | Requires C0.5 session manager |
| `GetDocument` | Unary | T2 | Native gRPC clients |
| `CreateDocument` | Unary | T2 | Caller-provided `document_id` or server-assigned ID path |
| `UpdateDocument` | Unary | T2 | Single-doc patch |
| `DeleteDocument` | Unary | T2 | Single-doc delete |
| `ListDocuments` | Unary | T2 | Collection listing, pagination, masks, and `show_missing`/read-mode handling or explicit unsupported errors |
| `ListCollectionIds` | Unary | T3 | Requires path metadata |
| `BatchWrite` | Unary | T3 | Non-atomic per-write statuses |
| `PartitionQuery` | Unary | Deferred | Sharded query partitioning |
| `ExecutePipeline` | Server streaming | Deferred | REST-mapped newer pipeline/vector API surface |

### Shared Adapter Logic

Each RPC should have one decoded operation implementation, but tonic and axum
extractors should remain thin transport adapters:

- tonic methods receive `tonic::Request<T>` and return protobuf responses or
  streams.
- axum REST handlers receive path/header/body extractors and return Proto3 JSON.
- Both call shared `firebase::ops::*` functions after decoding tenant,
  database, principal, and request body.
- Shared functions must not know about axum `Json<T>` or tonic
  `Request<T>`.

### Resource Paths

Firestore has multiple REST path shapes. Do not implement a single generic
`{documentPath}:{rpc}` route for every RPC.

Database-level RPCs:

```text
/v1/projects/{projectId}/databases/{databaseId}/documents:commit
/v1/projects/{projectId}/databases/{databaseId}/documents:batchGet
/v1/projects/{projectId}/databases/{databaseId}/documents:beginTransaction
/v1/projects/{projectId}/databases/{databaseId}/documents:rollback
/v1/projects/{projectId}/databases/{databaseId}/documents:batchWrite
```

Parent-scoped query/list RPCs:

```text
/v1/projects/{projectId}/databases/{databaseId}/documents:runQuery
/v1/projects/{projectId}/databases/{databaseId}/documents/{parentPath}:runQuery
/v1/projects/{projectId}/databases/{databaseId}/documents:listCollectionIds
/v1/projects/{projectId}/databases/{databaseId}/documents/{parentPath}:listCollectionIds
```

Document-specific unary REST routes, when implemented, use the document
resource name shape:

```text
/v1/projects/{projectId}/databases/{databaseId}/documents/{documentPath}
```

Rules:

- `projectId` maps to the Neovex tenant only after validation and decoding.
- `(default)` is the only supported database in the first release. Named
  databases must return a clear `INVALID_ARGUMENT` unless a later plan maps
  them to tenant namespaces.
- Path segments are URL-decoded exactly once. Slash separates path segments and
  cannot be part of a single document ID segment.
- Dots, Unicode, and other Firestore-valid segment text must not be rejected by
  Neovex logical-name validation.

### Value Serialization

Support the Firestore `Value` oneof:

- `nullValue`, `booleanValue`, `integerValue`, `doubleValue`,
  `timestampValue`, `stringValue`, `bytesValue`, `referenceValue`,
  `geoPointValue`, `arrayValue`, and `mapValue`.
- Preserve i64 `integerValue` precision.
- Preserve special doubles: `NaN`, `Infinity`, `-Infinity`.
- Truncate timestamps to Firestore's microsecond storage precision.
- Validate Firestore size/reserved-name constraints where they affect writes.
- Reject or explicitly mark unsupported newer pipeline/function/reference
  expression values when they appear in user write payloads.

The adapter may use tagged Neovex JSON for bytes, timestamps, references, and
geo points, but those tags are serialization details. Core storage should not
need to understand Firebase-specific JSON wrappers unless they become
Neovex-native value types.

### StructuredQuery Translation

The query translator must parse every `StructuredQuery` field even if some are
not yet executable:

- `select`
- `from`
- `where`
- `orderBy`
- `startAt`
- `endAt`
- `offset`
- `limit`
- `findNearest`

Unsupported fields must produce explicit compatibility errors. Silent ignore is
not allowed.

### Write Translation

Firestore `Write` supports these operation forms:

- `update`
- `delete`
- `verify`
- `transform`

It also supports:

- `updateMask`, where masked fields missing from the payload are deleted.
- `updateTransforms`, which apply after the update.
- `currentDocument` preconditions: `exists` and `updateTime`.

Field transforms include:

- server timestamp
- increment
- maximum
- minimum
- append missing elements
- remove all from array

The plan's implementation phases may support these incrementally, but the
parser and error mapping must recognize all of them from the start.

### Listen And Write Streams

`Listen` and `Write` are bidirectional gRPC streams. They should use idiomatic
tonic patterns:

- Accept `Streaming<Request>`.
- Return a `Stream<Item = Result<Response, Status>>`.
- Use `tokio::select!` to drive inbound client messages, subscription/write
  result updates, cancellation, and stream close.
- Honor Firestore's consistent-snapshot rule for `Listen`: only advance stream
  read time when a `targetChange` with `read_time` applies to all targets.
- Clean up all target subscriptions, write sessions, and background tasks on
  disconnect.

Browser `Listen` support uses a Neovex WebSocket endpoint because gRPC-Web does
not support bidirectional streaming. The WebSocket contract must be specified
before F2 implementation:

- One binary protobuf message per WebSocket frame.
- Client frames are serialized `ListenRequest`.
- Server frames are serialized `ListenResponse`.
- Auth is established during the HTTP upgrade using the same principal mapping
  as other Firebase routes.
- Stream-level failures close the socket with a documented close code; target
  failures stay on the stream as `targetChange.REMOVE` with `cause`.
- JSON debug framing is allowed only in tests or dev tooling.

## Context Window Budget

A context window is one self-contained agent pass with a narrow owner, local
code/docs context, implementation, focused verification, and checkpointed plan
state. Large roadmap items should be split until each window can finish with a
clear artifact and a small verification bundle.

| Phase | Scope | Context windows |
|-------|-------|-----------------|
| F0 | Core primitive and boundary hardening | 6-9 |
| F1 | Firestore REST/Lite CRUD | 5-7 |
| F2 | Native gRPC, Write stream, Listen stream | 8-12 |
| F3 | Query breadth, transforms, collection groups | 5-8 |
| F4 | JavaScript SDK package | 6-9 |
| F5 | Admin/SDK compatibility, demo, docs | 3-5 |
| Buffer | Upstream SDK alignment and hardening | 3-4 |
| **Total** | | **36-54** |

The budget intentionally tracks context-window slices because this work is
primarily constrained by architecture boundaries, verification checkpoints, and
avoiding oversized agent passes that bury core semantics inside adapter code.

## Implementation Phases

### F0: Core Primitive And Boundary Hardening

Location: primarily `crates/neovex-core/`, `crates/neovex-engine/`, and shared
server support modules.

Context window budget: 6-9 focused windows.

- Add/widen document-key support for caller-provided keys.
- Add resource path metadata for nested collection paths and collection groups.
- Add protocol-neutral atomic write batch operations over the existing engine
  commit path.
- Add field transform operation types, even if some initially return
  `UNIMPLEMENTED`.
- Add query AST coverage for Firestore-shaped queries without putting query
  planning in the Firebase adapter.
- Add transaction session manager.
- Add reusable subscription snapshot/diff support.
- Add Firebase route family, CORS headers, and middleware ordering tests.
- Audit Convex adapter call sites touched by these seams and thin any
  duplicated core logic.

Exit gate: no Firebase CRUD implementation starts until explicit document keys,
collection/path metadata, and set/patch/delete/verify semantics have core or
shared-server homes.

### F1: Firestore REST And Lite CRUD

Location: `crates/neovex-server/src/adapters/firebase/`.

Context window budget: 5-7 focused windows.

- Register `pub(crate) mod firebase;` in `adapters/mod.rs`.
- Add `FirebaseConfig` and `with_firebase()` to `RouterBuildConfig`.
- Add Firebase state to `AppState` only for config and adapter runtime state;
  data transactions belong to the transaction manager from F0.
- Add REST routes for `Commit`, `BatchGetDocuments`, and `RunQuery`.
- Add Proto3 JSON serializer/deserializer.
- Add Firebase resource parser using the F0 path model.
- Implement `Commit` via the F0 atomic batch primitive.
- Implement basic `BatchGetDocuments` and `RunQuery`.
- Map all errors to Firestore REST JSON status format.

Exit gate: Firestore Lite-style REST CRUD works with explicit document IDs,
arbitrary valid Firestore path segments, preconditions, and missing-document
semantics.

### F2: Native gRPC, Write Stream, And Listen Stream

Context window budget: 8-12 focused windows.

- Add tonic/prost bindings. Prefer compiling vendored googleapis protos with
  `tonic-build` if crate drift becomes a maintenance risk; otherwise pin and
  audit `googleapis-tonic-google-firestore-v1`.
- Implement the Firestore tonic service trait with unimplemented RPCs returning
  `Status::unimplemented`.
- Integrate tonic routes with axum after defining middleware order.
- Add `tonic-web` for unary and server-streaming browser RPCs.
- Implement `Write` stream handshake, stream tokens, write result ordering, and
  write pipeline semantics.
- Implement `Listen` stream target lifecycle, resume tokens, existence filters,
  target resets, current snapshots, consistent-snapshot `read_time` handling,
  remove/delete distinctions, and cleanup.
- Add the WebSocket `Listen` transport with the framing contract above.
- Implement `GetDocument`, `CreateDocument`, `UpdateDocument`,
  `DeleteDocument`, `BeginTransaction`, and `Rollback`.
- Implement `ListDocuments` request-field handling for pagination, ordering,
  masks, `show_missing`, and read selectors, or return explicit unsupported
  errors where the tier intentionally excludes a field.

Exit gate: the Firebase JS full SDK data path can write via `Write`, listen via
`Listen`, and run basic transactions against native gRPC in Node.js.

### F3: Query Breadth, Transforms, And Collection Groups

Context window budget: 5-8 focused windows.

- Implement array filters, `IN`, `NOT_IN`, `ARRAY_CONTAINS_ANY`, `OR`, unary
  null/NaN filters, projections, offsets, repeated ordering, cursor edge cases,
  and `documentId()`.
- Implement `limitToLast` by reversing order and restoring response order where
  needed.
- Implement collection group queries over the F0 path model.
- Implement `RunAggregationQuery` for count, sum, and average.
- Implement field transforms: server timestamp, increment, maximum, minimum,
  array union, and array remove.
- Implement `BatchWrite` with per-write statuses.
- Implement `ListCollectionIds` from path metadata.

Exit gate: query and field behavior has focused Rust tests plus Firebase SDK
test coverage for the supported matrix.

### F4: JavaScript SDK Package

Location: `packages/firebase/`.

Context window budget: 6-9 focused windows.

- Package name: `@neovex/firebase`.
- Entry points compatible with common `firebase/app` and `firebase/firestore`
  imports where practical.
- App initialization and emulator connection helpers.
- References: `collection`, `doc`, `collectionGroup`.
- Reads and writes: `getDoc`, `getDocs`, `addDoc`, `setDoc`, `updateDoc`,
  `deleteDoc`, `writeBatch`.
- Queries: `query`, `where`, `orderBy`, `limit`, `limitToLast`, cursors,
  `and`, `or`.
- Listeners: `onSnapshot`, snapshots, metadata fields, `docChanges`.
- Transactions: `runTransaction`.
- Field values: server timestamp, increment, array union/remove, delete field,
  and explicit unsupported handling for vector APIs until F3 supports them.
- Browser transport: gRPC-Web for unary/server-streaming; WebSocket for
  `Listen`.

Exit gate: Neovex's SDK self-test covers CRUD, queries, listeners, batches,
transactions, transforms, and reconnect behavior.

### F5: Admin/SDK Compatibility And Documentation

Context window budget: 3-5 focused windows.

- Build a compatibility matrix for Web, Node, Admin Node, Go, Java, Python, and
  mobile/native SDKs.
- Decide which Admin SDK flows are in scope for first release:
  `BatchWrite`, transactions, recursive delete-like behavior, import/export,
  and emulator control endpoints.
- Add migration guide from Firebase config/imports to Neovex.
- Add security rules migration guidance to Neovex-native auth/access control.
- Add demo app under `demos/firebase/`.

## Testing Strategy

### Layer 1: Core Primitive Tests

- Explicit document keys and generated keys.
- Arbitrary valid Firestore segment strings.
- Nested resource paths and collection group metadata.
- Atomic set/patch/delete/verify/transform batches.
- Missing-document delete and precondition failures.
- Transaction token timeout, rollback, auth mismatch, and commit consumption.
- Snapshot diff helper behavior.

### Layer 2: Serialization And Path Tests

- Every Firestore `Value` type supported or explicitly rejected.
- i64 precision, special doubles, timestamp precision, bytes, references,
  geo points, nested maps/arrays, reserved field names, and size limits.
- REST URL decoding and gRPC `database` field parsing.
- Named database rejection.

### Layer 3: Query Translation Tests

- All `StructuredQuery` fields are parsed.
- Supported fields translate into core query AST.
- Unsupported fields return exact errors.
- Composite filters, unary filters, document key filters, ordering, offsets,
  cursors, projections, collection groups, and limit-to-last edge cases.

### Layer 4: RPC Contract Tests

Use both transports:

- REST tests send Proto3 JSON and headers exactly as Firebase REST sends them,
  including `Content-Type: text/plain` when applicable.
- gRPC tests use a generated Firestore gRPC client and serialized protobuf
  messages, not hand-built JSON approximations.
- WebSocket Listen tests send binary protobuf frames.

Minimum scenarios:

- Commit set, overwrite, merge, patch, delete, verify, and transform
  recognition/execution as supported by the current tier.
- Atomic failure rolls back all writes.
- BatchGet found/missing mix.
- RunQuery filters/order/cursors/offset/limit.
- Write stream handshake, stream tokens, multiple batches, permanent error.
- Listen add/remove target, concurrent targets, target ID reuse, resume token,
  existence filter mismatch, reset, delete/remove distinctions.
- Transaction begin/read/commit/rollback/timeout/conflict.
- Large document rejection near Firestore's 1 MiB limit.
- Unicode collection/document IDs and names containing `__`.
- Deep subcollection paths and collection group queries.

### Layer 5: `@neovex/firebase` SDK Tests

Mirror the `packages/convex/selftest.mjs` pattern:

- Initialize app and connect to a local Neovex server.
- CRUD with explicit IDs and generated IDs.
- Query filters, ordering, cursors, collection groups.
- Listeners on documents and queries, including reconnect.
- Write batches and transactions.
- Field transforms.
- Browser transport tests for gRPC-Web and WebSocket Listen.

### Layer 6: Firebase JS SDK Integration Tests

Use the local Firebase JS SDK checkout as a compatibility score, not as an
early phase gate. Node.js tests use `@grpc/grpc-js` and exercise the real full
SDK remote store, including `Write` and `Listen` streams. Browser/Karma tests
use WebChannel and are deferred unless WebChannel is implemented.

Early task: catalog tests into:

- Expected pass for current tier.
- Expected fail because feature is deferred.
- Expected skip because it needs WebChannel, emulator control endpoints,
  security rules endpoints, or unsupported Admin behavior.

The first upstream targets should be narrow subsets of smoke, batch writes,
queries, cursors, fields, and transactions after the matching tier exists. Do
not claim `smoke.test.ts` passes in F1 unless listener and full SDK write paths
are also available.

### Layer 7: Verification Harness

Add Firebase server cases to the verification harness:

- `firebase-rest-commit-set-and-read`
- `firebase-grpc-write-stream-basic`
- `firebase-listen-document-change`
- `firebase-query-with-filters`
- `firebase-transaction-conflict`
- `firebase-collection-group-query`

### Layer 8: Architecture Boundary Regression Tests

Prefer executable regression tests that prevent regression into adapter-local
core logic. Use review checklist items only when a meaningful automated
assertion is not practical:

- Firestore and Convex both use the shared write batch primitive where their
  semantics overlap.
- Convex subscriptions and Firebase Listen use shared snapshot/diff helpers
  where appropriate, with protocol-specific envelopes outside the helper.
- No Firebase-reserved user field is required for path semantics.
- New query operations are exposed through core query types, not private
  Firebase-only filtering loops.

## Key Architectural Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Adapter registration | Module-based adapter beside Convex | Matches current `adapters/mod.rs` pattern without inventing a registry trait too early |
| Core before adapter | F0 is mandatory | Firebase exposes gaps in document identity, paths, transactions, and queries that should not live in adapter code |
| Document identity | Explicit core document keys | Firestore requires caller-provided IDs; adapter lookup shims would leak storage semantics |
| Subcollections | Resource path metadata | Avoids lossy `parent__child` and `_parent_id` collisions |
| Mutation path | Shared atomic write batch over engine commit path | Preserves Neovex storage atomicity and avoids separate adapter mutation paths |
| Transactions | Engine/server session manager | Multi-RPC transactions need lifecycle, TTL, auth binding, and cleanup |
| gRPC | tonic service | Native Firestore clients use gRPC; unsupported RPCs return `UNIMPLEMENTED` |
| REST | Secondary transport | Useful for Lite SDK, curl, and contract tests |
| Browser Listen | WebSocket protobuf frames for `@neovex/firebase` | gRPC-Web does not support bidi streams |
| Stock browser SDK | WebChannel deferred | Requires a separate proprietary transport compatibility plan |
| Error mapping | Protocol-specific envelope over shared error taxonomy | Keeps core errors reusable |
| Firebase tests | Compatibility score | Upstream tests are broad and should not be overpromised before matching tiers exist |

## Deferred Or Out Of Scope

- Cloud Functions compute triggers and HTTP handlers: document-change triggers
  (`onDocumentCreated`, `onDocumentUpdated`, `onDocumentDeleted`,
  `onDocumentWritten`), framework-style CloudEvent handlers
  (`functions.cloudEvent()`), HTTP handlers (`functions.http()`, `onRequest`,
  `onCall`), and scheduled functions are covered by the follow-on plan at
  `docs/plans/archive/firebase-cloud-functions-plan.md`. That plan activates after this
  plan's F3 phase is `done` and supports both `firebase-functions/v2` and
  `@google-cloud/functions-framework` authoring surfaces over a shared trigger
  registry, durable delivery, standard Firestore CloudEvent types, and
  generalized runtime artifact/deploy contract.
- Firebase Realtime Database.
- Firebase Auth backend, beyond accepting/verifying tokens as application auth.
- Firestore security rules engine.
- Firebase Storage, Hosting, Analytics, Crashlytics.
- Stock browser `firebase/firestore` WebChannel drop-in.
- Emulator control endpoints unless needed for a chosen compatibility tier.
- `PartitionQuery` and `ExecutePipeline` until the core query surface supports
  their semantics.
- Import/export and managed Admin operations unless a later Admin plan adds
  them.

## Risks

**R1: Firestore resource identity mismatch (Critical, F0).** Leaf document keys
are widened, but nested collections still require collision-free
path/collection metadata. Mitigation: treat F0.2 as the remaining identity gate
before CRUD.

**R2: Adapter-local core logic regression (High, all phases).** Firebase could
repeat the historical Convex pattern of accumulating core behavior in adapter
modules. Mitigation: enforce the Architecture Boundary Contract and F0 exit
gate; promote shared write/query/transaction/subscription semantics first.

**R3: Write and Listen stream state machines (High, F2).** Full SDK writes and
listeners depend on bidi streams, stream tokens, target IDs, resume tokens, and
cleanup. Mitigation: implement `Write` and `Listen` before claiming full SDK
compatibility; add stream contract tests.

**R4: Query semantics exceed current Neovex query primitives (High, F0-F3).**
Firestore includes composite filters, unary filters, projections, offsets,
document ID ordering, and collection groups. Mitigation: widen the core query
AST and return explicit errors for unsupported fields.

**R5: Field transforms are real engine semantics (High, F1-F3).** Increment,
maximum/minimum, array transforms, and server timestamps require atomic
read-modify-write behavior. Mitigation: model transforms in the write batch
primitive, not as adapter-side JSON patches, and keep transform
recognition/error mapping distinct from executable transform support so T1/F3
gates stay consistent.

**R6: Proto dependency drift (Medium, F2).** Pre-generated
`googleapis-tonic-google-firestore-v1` bindings are convenient but
community-maintained. Mitigation: pin and audit the crate, add proto drift CI,
or compile vendored googleapis protos with `tonic-build`.

**R7: Browser Listen transport ambiguity (Medium, F2/F4).** WebSocket framing
must be exact enough for the SDK package. Mitigation: freeze the binary
protobuf frame contract before implementation and test reconnect/error cases.

**R8: Middleware/security integration (Medium, F0/F2).** axum, tonic,
`tonic-web`, CORS, local origin checks, app auth, and audit logging can apply
in the wrong order. Mitigation: add Firebase route families and middleware
ordering tests before exposing routes.

**R9: Upstream SDK test breadth (Medium, F5).** Firebase integration tests
cover offline/cache/client behavior, emulator quirks, WebChannel, and features
outside a Firestore protocol adapter. Mitigation: use a tiered compatibility
score with explicit skip/fail reasons.

**R10: Transaction sessions can pin snapshots and memory (Medium, F0/F2).**
Cross-RPC transactions may hold execution units, read sets, and auth-scoped
state open longer than normal request lifetimes. Mitigation: bound session
count/TTL, reuse existing server-owned session cleanup patterns, and test
timeout/disconnect cleanup before exposing transaction RPCs.

## Phase Status Ledger

| Phase | Status | Context budget | Start condition | Done when |
|-------|--------|----------------|-----------------|-----------|
| F0: Core primitive hardening | `done` | 6-9 context windows | `F0.1` selected | `F0.1` through `F0.8` are `done` and shared primitives have focused Rust tests |
| F1: REST adapter vertical slice | `done` | 5-7 context windows | F0 is `done` | REST `Commit`, `BatchGetDocuments`, and `RunQuery` pass contract tests |
| F2: gRPC and streaming adapter | `done` | 8-12 context windows | F1 is `done` | tonic routes, `Write`, `Listen`, and WebSocket Listen pass stream contract tests |
| F3: Firestore semantic breadth | `done` | 5-8 context windows | F2 is `done` | advanced query/write/admin semantics pass compatibility tests |
| F4: `@neovex/firebase` SDK | `done` | 6-9 context windows | F2 is `done`; F3 items as needed | package builds, typechecks, and exercises REST/gRPC-Web/WebSocket transports |
| F5: Compatibility documentation | `done` | 3-5 context windows | F3/F4 baseline is usable | matrix, upstream test catalog, demo, and migration guide are published |

## Roadmap Items

Each item is intended to fit in one focused context window. If an item cannot
fit with the relevant source context, implementation, tests, and checkpoint
update loaded at once, split it before starting.

### F0 Work Queue: Core Primitive Hardening

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| F0.1 Document-key design and implementation | `done` | none | Core supports caller-provided document keys plus generated keys without adapter-local lookup shims. | Focused `neovex-core`/`neovex-engine` tests for arbitrary Firebase IDs, generated IDs, invalid IDs, and existing Convex/native ID behavior. |
| F0.2 Resource path and collection group metadata model | `done` | F0.1 | Full Firestore resource paths, nested collections, and collection group metadata are represented without delimiter tricks or user-field collisions, and raw collection segments no longer depend on `validate_logical_name`. | Tests for `cities/SF`, `a/1/b/2/c/3`, collection names containing `__`, dots, and Unicode, Unicode document IDs, and collection-group lookup metadata. |
| F0.3 Atomic write batch primitive | `done` | F0.1, F0.2 | Shared write batch surface models set, patch, delete, verify, transform, preconditions, overwrite-create, missing-document delete semantics, ordered results, and atomic commit. | Engine/storage tests for mixed operations, rollback on failure, missing/existing preconditions, overwrite-on-missing, delete-missing-without-precondition, transform result ordering, and storage/index/commit-log atomicity. |
| F0.4a Query AST surface expansion | `done` | F0.2 | `neovex-core` query types can represent query source (`from` / collection group), projections, repeated ordering, cursors, offset, limit, composite/unary filter metadata, and explicit unsupported placeholders such as `find_nearest` without adapter-local side channels. | Focused `neovex-core` tests for query source selection, projections, repeated `order_by`, cursor bounds, offset/limit serialization, collection-group source metadata, composite/unary filter roundtrips, and explicit unsupported `find_nearest` handling. |
| F0.4b1 Structured query lowering and unsupported-feature gate | `done` | F0.4a | Engine accepts the widened structured-query AST through a dedicated lowering path, preserves current table-query behavior for the supported single-source/single-order subset, and returns explicit errors instead of silently dropping projections, collection groups, composite/unary filters, repeated ordering, cursor bounds, offsets, document-ID sentinels, or `find_nearest`. | Focused `neovex-engine` tests for simple structured-query success plus explicit rejection of projections, collection groups, composite/unary filters, repeated `order_by`, cursor bounds, offsets, document ID / `__name__`, and `find_nearest`. |
| F0.4b2 Engine richer ordering/cursor/projection adoption | `done` | F0.4b1 | Structured-query execution adopts the first richer shared planner surface needed for Firebase: supported repeated-order validation, cursor/offset plumbing, projection shaping, and the remaining explicit unsupported boundaries are canonicalized for deferred Firestore semantics. | Focused `neovex-engine` tests for repeated ordering behavior, cursor/offset plumbing, projection shaping, preserved legacy `Query` behavior, and canonical remaining unsupported errors. |
| F0.5 Transaction session manager | `done` | F0.3 | Engine/server support transaction tokens across RPCs with TTL, principal binding, read tracking, commit, rollback, and cleanup. | Tests for BeginTransaction -> read -> Commit, rollback, expired token, wrong principal, conflict retry/error mapping, and cleanup. |
| F0.6a Subscription snapshot envelope | `done` | F0.4b2 | Shared subscription output carries a protocol-neutral stable snapshot envelope with full result documents, deleted-document hints, covered sequence, and commit metadata so adapters do not scrape transport-local JSON arrays to build watch state. | Focused engine/server subscription tests for bootstrap snapshots, mutation-driven snapshots, deleted-document hints, commit timestamps, and concurrent subscribers still receiving stable full snapshots. |
| F0.6b Subscription diff helper and change classification | `done` | F0.6a | Shared helpers classify added/modified/removed changes and ordering shifts between successive snapshots without Firebase-specific watch state embedded in the engine. | Tests for added/modified/removed changes, empty snapshots, ordering changes, and stable behavior across consecutive deliveries. |
| F0.7 Firebase route family, CORS, middleware tests | `done` | none | Server policy classifies Firebase REST/gRPC/gRPC-Web/WebSocket routes and applies auth/origin/CORS/audit middleware in the intended order, including the Firebase REST `text/plain` body shape and SDK headers. | Server tests for route family classification, allowed Firebase headers, `text/plain` REST preflight, gRPC-Web preflight, local-origin policy, and audit categorization. |
| F0.8 Convex adapter shared-logic audit | `done` | F0.1-F0.7 as relevant | Convex adapter code that owns general database semantics is promoted or explicitly documented as adapter-specific. | Source audit notes plus focused Convex compatibility tests for any promoted behavior. |

### F0.8 Audit Notes

- `crates/neovex-server/src/adapters/convex/execution/sync_ops/queries.rs` is
  adapter-specific request/result shaping. It maps Convex `get`, `first`, and
  `unique` contracts onto shared service query calls and keeps Convex-only null
  and duplicate-match behavior out of `neovex-core` / `neovex-engine`.
- `crates/neovex-server/src/adapters/convex/host_bridge/pagination.rs` stays
  adapter-local because it synthesizes opaque Convex runtime `paginate()`
  cursors from Convex JSON payloads (`_id` plus projected field values). This
  is not Firestore cursor semantics and should not become a shared query
  primitive.
- `crates/neovex-server/src/adapters/convex/host_bridge/bridge.rs` keeps the
  per-invocation runtime mutation execution unit and nested-runtime budget for
  Convex function execution. It is intentionally separate from the shared
  cross-RPC transaction/session manager added in `F0.5`.
- `crates/neovex-server/src/adapters/convex/subscriptions/transforms/planner.rs`
  and the surrounding `subscriptions/transforms/` tree stay adapter-specific
  because they implement Convex WebSocket transform contracts (`identity`,
  `get`, `first`, `unique`, runtime re-eval) on top of the shared subscription
  snapshot/diff surfaces from `F0.6a` / `F0.6b`.
- The audit did not find remaining Convex adapter code that still owns shared
  Firestore-facing document identity, resource-path metadata, atomic write
  batches, structured query AST/planner semantics, transaction sessions, or
  subscription diff classification. Those semantics now live in the shared
  core/engine/server seams landed in `F0.1` through `F0.7`.

### F1 Work Queue: REST Adapter Vertical Slice

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| F1.1 Firebase module scaffold and router registration | `done` | F0 done | `crates/neovex-server/src/adapters/firebase/` exists, routes are config-gated, and `AppState` registration follows the Convex pattern without copying Convex internals. | Server compile/check plus router tests proving Firebase disabled/enabled route behavior. |
| F1.2 Proto3 JSON serializer | `done` | F1.1 | Firestore `Value` JSON maps to/from Neovex values for all supported oneof cases with explicit unsupported cases. | Roundtrip tests for null, bool, integer string, double, timestamp, string, bytes, reference, geo point, array, map, NaN, and infinity. |
| F1.3 Firestore resource parser | `done` | F1.1 | REST and gRPC resource names parse into project, database, document path, collection path, and collection group identifiers. | Parser tests for `(default)`, named database rejection, URL-escaped IDs, collection names with dots/Unicode, nested paths, trailing slashes, and malformed names. |
| F1.4a Commit request parser and batch translation | `done` | F0.3, F1.2, F1.3 | REST `Commit` JSON parses into shared atomic-write request primitives (`AtomicWriteBatch`, bound write keys, preconditions, masks, and transform metadata) with explicit unsupported errors instead of adapter-local write shims. | Contract tests for set/create/patch/delete/verify parsing, merge masks, invalid resource names, transaction field gating, and transform recognition/error mapping. |
| F1.4b Commit execution and Firestore response mapping | `done` | F1.4a | Parsed commit batches execute through the shared mutation or transaction/session path and return Firestore-shaped `writeResults` / `commitTime` plus atomic rollback/error behavior. | Contract tests for successful set/patch/delete/verify commit results, atomic rollback on failure, transaction token handling, and REST error envelope mapping. |
| F1.5 BatchGetDocuments REST handler | `done` | F1.2, F1.3, F1.4b | REST `BatchGetDocuments` returns found/missing documents in SDK-compatible JSON form and honors transaction/read-time handling that exists at this tier. | Contract tests for found, missing, duplicates, nested paths, transaction token, and error status. |
| F1.6a RunQuery routing and parent-path support | `done` | F0.4b2, F1.2, F1.3 | REST `RunQuery` translates the supported `StructuredQuery` subset to shared query execution for both root `documents:runQuery` and document-parent `documents/{document_path}:runQuery` routes, then streams/returns expected JSON responses. | Contract tests for root and document-parent `RunQuery` from, where, orderBy, cursor, offset, limit, empty result, and unsupported-operator errors. |
| F1.6b RunQuery missing-index surfacing | `done` | F0.4b2, F1.6a | Shared structured-query execution surfaces explicit missing-index failures for supported Firestore compound queries instead of silently relying on full scans. | Contract tests for compound-query missing-index errors and success once the matching schema index exists. |
| F1.7 REST error mapper | `done` | F1.4-F1.6b | Firebase REST errors consistently use google RPC-style status strings, messages, and optional details. | Error mapping tests for all gRPC status codes used by the adapter and SDK parsing spot checks. |

### F2 Work Queue: gRPC And Streaming Adapter

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| F2.1 tonic/prost bindings and service scaffold | `done` | F1 done | Firestore gRPC service compiles from pinned audited protos or vendored `tonic-build`, and unimplemented RPCs return `UNIMPLEMENTED`. | Cargo check plus generated-service smoke tests and proto drift note. |
| F2.2 tonic + axum + tonic-web routing | `done` | F2.1, F0.7 | gRPC, gRPC-Web, REST, and WebSocket routes share the port with correct middleware order. | Server integration tests for CORS/auth/rate-limit/local-origin behavior across REST, gRPC, and gRPC-Web. |
| F2.3 Write bidi stream | `done` | F0.3, F2.2 | `Write` implements handshake, stream tokens, ordered write requests, replay/resume semantics, and per-message responses through the shared atomic write batch path. | tonic stream tests for handshake, empty token, retry token, mixed writes, errors, and stream closure. |
| F2.4a Listen target lifecycle and bootstrap snapshot | `done` | F0.6b, F2.2 | Native gRPC `Listen` accepts add/remove target requests, assigns/echoes target IDs, translates one shared subscription bootstrap snapshot into Firestore target/document messages, and cleans up registrations when targets or streams end. | tonic stream tests for add target, assigned target ID, initial snapshot document changes, remove target, and stream-closure cleanup. |
| F2.4b1 Listen resume registry and stream-consistent tokens | `done` | F2.4a | `Listen` accepts one-target resume selectors across stream reconnects, reconciles client resume state against server-owned target state or emits `RESET` when restart is required, and keeps stream-consistent resume-token / `read_time` advancement explicit. | tonic stream tests for resume after reconnect, stale/mismatched resume token reset, and monotonic `NO_CHANGE` token / `read_time` advancement. |
| F2.4b2a Listen concurrent targets and per-target routing | `done` | F2.4b1 | `Listen` supports multiple active query targets on one stream, concurrent add/remove bookkeeping, target ID assignment, and per-target bootstrap/update/remove routing without leaking registrations across targets. | tonic stream tests for concurrent targets, interleaved per-target updates, target removal cleanup, and same-document routing across multiple targets. |
| F2.4b2b Listen existence filters, once targets, and bounded backpressure | `done` | F2.4b2a | `Listen` honors `expected_count` / `once`, emits existence filters or reset fallback when client cache counts diverge, and keeps slow-consumer stream fan-out bounded. | tonic stream tests for existence filters, once-target auto-remove, reset fallback, and slow-consumer backpressure. |
| F2.5a WebSocket Listen framing and shared session bridge | `done` | F2.4b2b | Browser `Listen` WebSocket upgrades speak one-binary-protobuf-frame-per-message, and both gRPC `Listen` and WebSocket `Listen` run through the same server-owned target/session implementation. | Binary protobuf frame tests for add/remove/resume/reset over WebSocket plus gRPC regression checks proving the shared session still works. |
| F2.5b WebSocket security/browser smoke and protocol doc | `done` | F2.5a | WebSocket Listen origin/auth failure coverage, browser-style smoke behavior, close-code mapping, and the protocol document are frozen for `@neovex/firebase`. | WebSocket security/origin tests, browser-style smoke test, documented close-code assertions, and protocol doc update. |
| F2.6a Shared unary gRPC wrappers for existing REST and transaction flows | `done` | F2.1-F2.5b | Firestore gRPC `Commit`, `BatchGetDocuments`, `RunQuery`, `BeginTransaction`, and `Rollback` delegate to the same shared adapter/engine implementations already exercised by REST and the transaction session manager, without REST-only JSON extraction duplicated inside tonic handlers. | gRPC client contract tests for `Commit`, `BatchGetDocuments`, `RunQuery`, `BeginTransaction`, `Rollback`, shared error mapping parity, and explicit unsupported coverage where translation is intentionally deferred. |
| F2.6b1 Point CRUD gRPC surface | `done` | F2.6a | `GetDocument`, `CreateDocument`, `UpdateDocument`, and `DeleteDocument` map Firestore document/resource semantics onto the shared path/write/query primitives, including `document_id` handling, field masks, and explicit precondition behavior through the shared commit/read seams instead of Firebase-local storage logic. | gRPC client contract tests for point reads, create/update/delete success paths, generated-vs-explicit document IDs, precondition failures, and shared error/status mapping. |
| F2.6b2 ListDocuments gRPC surface | `done` | F2.6b1 | `ListDocuments` maps Firestore parent/collection selectors onto shared path/query primitives, with explicit unsupported handling for phase-1 pagination/order/show_missing/read selector gaps where needed. | gRPC client contract tests for `ListDocuments` happy paths plus pagination/order/mask/show_missing/read selectors or explicit `UNIMPLEMENTED`/`INVALID_ARGUMENT` responses. |

### F3 Work Queue: Firestore Semantic Breadth

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| F3.1 Advanced filters and query fields | `done` | F0.4b2, F1/F2 query paths | Composite filters, unary filters, array operators, `IN`, `NOT_IN`, offsets, projections, document ID sentinel behavior, and implicit `__name__` ordering/tie-break rules are implemented or rejected canonically. | Query compatibility tests covering each operator, `documentId()` / `__name__` behavior, and invalid-combination Firestore errors. |
| F3.2a Shared collection-group query execution | `done` | F0.2, F0.4b2 | Shared engine structured-query execution plus Firebase REST/gRPC `RunQuery` can execute collection-group queries against the existing path-binding metadata, with full document-path `__name__` filters/order/cursors and no table-name wildcard hacks. | Focused REST and gRPC `RunQuery` tests across many ancestors, duplicate collection IDs, deeply nested paths, ordering, cursors, and deletes on a metadata-capable provider. |
| F3.2b1 SQLite/libsql path-metadata parity for collection groups | `done` | F0.2, F3.2a | The default embedded SQLite path and libsql replica path persist/read the same resource-path metadata as Redb, so collection-group queries no longer depend on a Redb-only store capability for the default SQL-backed deployment paths. | Storage/provider tests proving path-binding write/delete/read parity plus collection-group query tests on SQLite and libsql. |
| F3.2b2 Postgres/MySQL path-metadata parity for collection groups | `done` | F3.2b1 | Postgres and MySQL persist/read the same resource-path metadata as the other providers, or the project records a deliberate provider-support boundary before claiming collection-group parity across all SQL backends. | Provider tests for path-binding write/delete/read parity plus an explicit support decision note for Postgres/MySQL. |
| F3.3 Aggregation queries | `done` | F3.1 | `RunAggregationQuery` supports count and any intentionally supported sum/avg behavior with clear unsupported errors. | Contract tests for count, aliases, empty result, filtered result, transaction/read-time, and unsupported aggregations. |
| F3.4a Numeric and array field transforms | `done` | F0.3 | Increment, maximum/minimum, array union/remove, and transform-only writes execute atomically through the shared engine batch path with ordered transform results on the current shared value model; unsupported typed-scalar gaps fail explicitly instead of being silently accepted. | Tests for numeric coercion, integer/double behavior, missing fields, arrays, transform result order, rollback on unsupported typed-scalar transforms, and concurrent writes. |
| F3.4b1 Shared typed scalar value foundation | `done` | F3.4a | `neovex-core`, storage providers, and engine documents persist shared typed scalar metadata for the Firestore-only transform values Neovex cannot represent in plain JSON alone, so adapters can stay transport-only around a reusable primitive. | Focused core/storage/engine tests for typed scalar persistence, SQL/redb roundtrip parity, document readback, and rollback safety without user-field collisions. |
| F3.4b2 ServerTimestamp and special-double transform adoption | `done` | F3.4b1 | Server timestamp plus NaN/Infinity max/min behavior execute through the shared typed scalar primitive and roundtrip correctly across engine, storage, REST, and gRPC surfaces without adapter-local shims. | Tests for serverTimestamp readback and transform results, NaN/Infinity max/min semantics, timestamp precision, and transport roundtrip parity. |
| F3.5a ListCollectionIds from path metadata | `done` | F0.2, F2.6 | `ListCollectionIds` reads the shared path metadata model for root and nested parents, without table-name tricks or adapter-local lookup shims. | Contract tests for root and nested parents, duplicate suppression, ordering, pagination if implemented, and shared error mapping across REST and gRPC. |
| F3.5b BatchWrite non-atomic per-write statuses | `done` | F0.2, F0.3, F2.6 | `BatchWrite` executes Firestore writes independently with per-write statuses/results, explicit duplicate-document rejection, and no accidental reuse of the atomic `Commit` contract. | Contract tests for partial success, per-write status ordering, labels passthrough/ignore behavior, duplicate-document rejection, and shared error mapping. |

### F4 Work Queue: `@neovex/firebase` SDK

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| F4.1 Package scaffold | `done` | F1 done | `packages/firebase` or chosen package path builds as `@neovex/firebase` with ESM/CJS/types aligned to repo JS conventions. | Package build, typecheck, export map tests, and root workspace install/build check. |
| F4.2a Reference and path primitive surface | `done` | F4.1, F0.2 | The SDK exposes protocol-neutral Firestore app/database identity plus `doc`, `collection`, and `collectionGroup` references with canonical path validation and nesting rules, without transport logic or table-name assumptions leaking into user APIs. | SDK unit tests for root and nested refs, collection-group refs, odd/even path validation, and subpath composition across `@neovex/firebase` entry points. |
| F4.2b CRUD transport vertical slice | `done` | F4.2a, F1 | `getDoc`, `setDoc`, `updateDoc`, `deleteDoc`, and `addDoc` map the reference surface onto the live REST vertical slice with shared auth/header handling and generated-ID behavior. | SDK unit tests plus local server smoke tests for point CRUD, generated IDs, path encoding, and shared error mapping. |
| F4.3a Query constraint primitive surface | `done` | F4.2b, F0.4a, F0.4b | The SDK exposes `query`, `where`, `orderBy`, `limit`, cursor constraints, and `documentId()` over collection and collection-group refs as protocol-neutral structured-query descriptors, with explicit unsupported guards for nested fields and duplicate singleton constraints. | SDK tests and typecheck coverage for constraint composition, chaining, collection-group/documentId queries, duplicate limit rejection, and cursor validation. |
| F4.3b Query execution snapshots and metadata | `done` | F4.3a, F1.6, F2.6 | `getDocs`, `QuerySnapshot`, `QueryDocumentSnapshot`, query metadata, and result decoding map the builder surface onto REST/gRPC `RunQuery` without inventing another path model. | SDK tests plus local server smoke tests for collection and collection-group queries, empty results, cursor/limit execution, and snapshot metadata flags. |
| F4.3c1 Equality helpers | `done` | F4.3b | `refEqual`, `queryEqual`, and `snapshotEqual` compare Firestore SDK references, queries, and snapshots by canonical identity plus materialized result shape without requiring converter support first. | SDK tests for equal/unequal refs, collection-group refs, query constraints, query snapshots, and document snapshots. |
| F4.3c2 Converters and typed data conversion | `done` | F4.3c1 | `withConverter` and converter-backed `data()` flows match the targeted compatibility tier across refs, queries, and snapshots. | SDK tests for converter roundtrips, typed data conversion, and converter-aware snapshot/query surfaces. |
| F4.4a1 Browser protobuf codegen/runtime foundation | `done` | F2.2, F2.5 | A browser-safe generated Firestore protobuf schema/runtime foundation exists for both unary gRPC-Web and binary Listen WebSocket clients, reusing the pinned vendored Firestore protos and modern TS/ESM tooling instead of hand-rolled encoders or Node-only APIs. | Package/browser build checks plus focused encode/decode tests for unary payloads and Listen WebSocket handshake messages. |
| F4.4a2 Browser unary gRPC-Web transport foundation | `done` | F4.4a1, F2.2 | Browser unary Firestore transport uses the shared generated protobuf foundation with a modern gRPC-Web client stack against the existing tonic-web server surface, with auth/header handling and explicit compatibility boundaries documented. | Browser integration tests for unary `Commit`, `BatchGetDocuments`, and `RunQuery` plus auth/header behavior and error mapping. |
| F4.4b1 Browser Listen/watch API and binary session bridge | `done` | F4.4a1, F2.5 | The SDK exposes the first browser watch surface (`onSnapshot` / unsubscribe plus initial snapshot decoding) over the documented binary-protobuf WebSocket `Listen` transport, sharing the existing protobuf foundation instead of inventing a WebChannel shim. | SDK/browser tests for document/query watch bootstrap, binary frame encode/decode, unsubscribe cleanup, and shared snapshot conversion. |
| F4.4b2a Browser Listen resume token retention and reconnect bootstrap | `done` | F4.4b1 | Browser Listen/watch retains per-target `resume_token` / `read_time` state, reconnects a dropped one-target session through the shared binary WebSocket bridge, and reuses bootstrap snapshot conversion without pulling auth/header policy into the reconnect core. | Browser integration tests for resume-token capture, reconnect with delta/bootstrap behavior, unsubscribe-versus-reconnect cleanup, and retained snapshot continuity. |
| F4.4b2b1 Browser Listen close-code mapping and bounded retry policy | `done` | F4.4b2a | Browser Listen/watch classifies documented WebSocket close codes into stable Firebase errors and applies bounded reconnect/backoff for retryable network closures on top of the retained reconnect core, without changing the public watch API or coupling the policy to unary gRPC-Web transport. | Browser integration tests for fatal close-code mapping, retry-versus-no-retry behavior, bounded backoff exhaustion, and retained snapshot continuity across retryable reconnects. |
| F4.4b2b2 Browser Listen browser-safe auth upgrade transport | `done` | F4.4b2b1 | Browser Listen/watch establishes a deliberate upgrade-time auth/header transport for the WebSocket path that matches the Firebase application-auth boundary and works within browser handshake limits, instead of assuming unary header behavior carries over unchanged. | Browser integration tests for auth token propagation, refresh-or-fail behavior across reconnect, and local-server/origin compatibility with the chosen upgrade contract. |
| F4.5a1 WriteBatch client surface and atomic multi-write commit | `done` | F4.2b | SDK `writeBatch`, `WriteBatch.set/update/delete/commit`, and multi-write commit requests lower onto the existing shared Firestore commit path so batched writes stay atomic without inventing a second client/server write flow. | SDK tests for batched set/update/delete request shapes and atomic commit responses; local server smoke tests for multi-write commit semantics. |
| F4.5a2a Transaction point-read/write client flow | `done` | F0.5, F4.5a1 | SDK `runTransaction` plus `Transaction.get/set/update/delete` map onto server transaction sessions across begin/read/commit/rollback for point reads and staged writes, with explicit retry/rollback behavior instead of adapter-local transaction state. | SDK tests for transaction success/retry/rollback/conflict plus local server smoke tests for REST and gRPC-Web begin/read/commit/rollback semantics. |
| F4.5a2b Transaction query reads and richer read helpers | `done` | F1.6, F2.6, F4.5a2a | Transactional query reads and richer `Transaction.get(...)` query overloads execute through the shared transaction-session `RunQuery` surface instead of falling back to adapter-local state or rejecting the selector. | SDK/server tests for transactional `RunQuery` success, explicit unsupported-selector coverage that remains deferred, query retry semantics, and a shared transaction-snapshot primitive test proving begin-time reads stay stable across external updates. |
| F4.5b1 FieldValue sentinel descriptors and write lowering | `done` | F3.4b2, F4.5a2b | SDK `FieldValue` sentinels (`serverTimestamp`, `increment`, array transforms, delete sentinel) exist as canonical client primitives and lower into the shared transform/delete write shapes across direct writes, `writeBatch`, and `runTransaction` without ad hoc request mutation. | SDK tests for sentinel request shapes in direct writes, batches, and transactions plus explicit invalid-combination coverage for merge masks, update paths, and unsupported nesting. |
| F4.5b2 Transform result compatibility and end-to-end smoke | `done` | F4.5b1 | Sentinel writes round-trip cleanly through the live server across REST and gRPC-Web, with transform result coverage and any remaining unsupported edges frozen explicitly instead of implied by SDK behavior. | Local server smoke plus package tests for `serverTimestamp`, numeric/array transforms, delete sentinel handling, and transform-result parity across direct writes, batches, and transactions. |

### F5 Work Queue: Compatibility Documentation

| Item | Status | Hard deps | Completion gate | Verification evidence |
|------|--------|-----------|-----------------|-----------------------|
| F5.1 SDK compatibility matrix | `done` | F1-F4 baseline | Public matrix lists supported SDKs, transports, APIs, gaps, and tier labels. | Matrix reviewed against implemented tests and Source Evidence Map. |
| F5.2 Firebase upstream test catalog | `done` | F4 baseline | Firebase JS SDK integration tests are cataloged into pass/fail/skip groups with reasons and commands. | Recorded upstream test command, representative pass logs, and skip rationale for emulator/WebChannel/offline-only tests. |
| F5.3 Demo app | `done` | F4.2-F4.4 | Demo exercises the supported SDK package against local Neovex with documented startup. | Demo smoke test and browser/local server instructions verified. |
| F5.4 Migration guide | `done` | F5.1-F5.3 | Guide explains compatibility tiers, setup, transport differences, data model caveats, and known gaps. | Docs review for all links, examples, and unsupported-feature statements. |

## Execution Log

| Date | Item | Status | Notes | Verification |
|------|------|--------|-------|--------------|
| 2026-04-25 | F5.4 Migration guide | `done` | Published `docs/reference/firebase-migration-guide.md` as the practical migration path from Firestore apps onto the current Neovex surface. The guide covers import moves to `@neovex/firebase`, local setup against a Neovex server, REST-versus-gRPC-Web unary selection, the browser WebSocket `Listen` bridge, current compatibility boundaries, and explicit guidance for migrating Firestore Security Rules intent into Neovex-owned auth and authorization checks. Added guide links from `docs/README.md` and the compatibility matrix so the runnable path and the exact support matrix stay connected. With the migration guide in place, the Firebase adapter control plan is complete. | Manual docs review via `sed -n '1,260p' docs/reference/firebase-migration-guide.md`; `rg -n "firebase-migration-guide|Firebase migration guide" docs/README.md docs/reference/firebase-compatibility.md docs/reference/firebase-migration-guide.md`; plan/status review via `rg -n "Plan status|Control item|F5: Compatibility documentation|F5\\.3 Demo app|F5\\.4 Migration guide" docs/plans/firebase-adapter-plan.md`. |
| 2026-04-25 | F5.3 Demo app | `done` | Added a runnable browser demo at `demos/firebase/html/` using the shipped `@neovex/firebase` package against a local Neovex server. Wired the demo into the root workspace, demos index, and demos README so the final migration docs can point at a concrete executable example instead of abstract setup notes. The demo exercises emulator redirection, REST or gRPC-Web unary reads and writes, WebSocket `Listen`, write batches, transactions, deletes, and supported `FieldValue` transforms. | `'/Applications/Codex.app/Contents/Resources/node' ./node_modules/typescript/bin/tsc -p demos/firebase/html/tsconfig.json --noEmit`; `npm run test --workspace firebase-html`; `npm run build --workspace firebase-html`; `npm run dev --workspace firebase-html`; `cargo run -p neovex-bin -- start --port 8080`; `curl -i -sS http://127.0.0.1:5176/ | sed -n '1,20p'`; `curl -i -sS http://127.0.0.1:8080/demos/ | sed -n '1,20p'`. |
| 2026-04-25 | F5.3 Demo app | `in_progress` | Started the demo/documentation closeout slice after completing the compatibility matrix and upstream test catalog. The next implementation step is to add a browser-facing `demos/firebase/` app that uses the shipped `@neovex/firebase` package against a local Neovex server, then wire the demo into the demos index and the docs so the migration guide can reference a concrete runnable example instead of abstract setup text. | Startup source refresh of `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`, `docs/plans/README.md`, and `docs/plans/firebase-adapter-plan.md`; `git status --short`; targeted review of `docs/reference/{firebase-compatibility,firebase-upstream-test-catalog}.md`, `demos/README.md`, `demos/index.html`, and `packages/firebase/src/firestore.ts`. |
| 2026-04-25 | F5.4 Migration guide | `in_progress` | Started the final Firebase adapter closeout slice after the runnable browser demo landed. The remaining work is to publish a practical migration guide covering import moves, local setup, transport differences, current compatibility boundaries, and explicit guidance for moving Firestore Security Rules intent into Neovex-owned auth and authorization checks. | Source review of `docs/reference/{firebase-compatibility,firebase-websocket-listen}.md`, `docs/README.md`, `demos/README.md`, `demos/index.html`, and `packages/firebase/src/{app,firestore}.ts`. |
| 2026-04-25 | F5.2 Firebase upstream test catalog | `done` | Closed the upstream catalog once the local Firebase JS SDK checkout was turned into a real compatibility signal instead of an environment-only blocker. Added a minimal `config/project.json`, ran `yarn build:deps`, installed a Firebase-supported `node@22` runtime locally, and documented that the current upstream Firestore node harness needs `NODE_OPTIONS="--experimental-transform-types"` in this checkout to parse parameter-property-heavy TypeScript files under modern Node. The catalog now records both representative upstream pass lanes (`database.test.ts --grep "doc\\(\\) will auto generate an ID"` and `validation.test.ts --grep "Collection paths"`) and the stock Node smoke lane reaching a live local Neovex server at `127.0.0.1:8080`, where it fails for a real compatibility reason: repeated `GrpcConnection RPC 'Write' stream ... 12 UNIMPLEMENTED` errors on the upstream bidi write path. `F5.2` is no longer blocked on harness setup; the next queued slice is `F5.3 Demo app`. | `yarn install --frozen-lockfile --ignore-engines` in `~/src/github.com/firebase/firebase-js-sdk`; `yarn build:deps` in `~/src/github.com/firebase/firebase-js-sdk/packages/firestore`; created `~/src/github.com/firebase/firebase-js-sdk/config/project.json`; `brew install node@22`; `target/debug/neovex start --port 8080 --data-dir /tmp/neovex-firebase-upstream`; `PATH="/opt/homebrew/opt/node@22/bin:$PATH" NODE_OPTIONS="--experimental-transform-types" ../../node_modules/.bin/ts-node ./scripts/run-tests.ts --main=test/register.ts --emulator --grep "doc\\(\\) will auto generate an ID" test/integration/api/database.test.ts`; `PATH="/opt/homebrew/opt/node@22/bin:$PATH" NODE_OPTIONS="--experimental-transform-types" ../../node_modules/.bin/ts-node ./scripts/run-tests.ts --main=test/register.ts --emulator --grep "Collection paths" test/integration/api/validation.test.ts`; `PATH="/opt/homebrew/opt/node@22/bin:$PATH" ../../node_modules/.bin/ts-node ./scripts/run-tests.ts --main=test/register.ts --emulator test/integration/api/smoke.test.ts` (reached Neovex, failed on `Write` stream `12 UNIMPLEMENTED`). |
| 2026-04-25 | F5.2 Firebase upstream test catalog | `blocked` | Wrote down the upstream Firestore test catalog in `docs/reference/firebase-upstream-test-catalog.md`, including the checked-out corpus path, the upstream `test:node` / `test:browser` / `test:lite` commands, first-pass file buckets, and the staged order for an eventual compatibility wave. The remaining completion-gate evidence is blocked on the local upstream Firebase workspace environment rather than on Neovex behavior: after an initial `ts-node`-missing failure, `yarn install --frozen-lockfile --ignore-engines` populated the checkout under Node `25.9.0`, but the representative `yarn test:node test/integration/api/smoke.test.ts` run still failed before reaching Neovex because the upstream workspace is not built (`Cannot find module '@firebase/app/dist/index.cjs.js'`). Next concrete action: rerun the upstream checkout under a Firebase-supported Node runtime (`18`, `20`, or `22`), run `yarn build:deps`, then rerun the focused `smoke.test.ts` node lane and promote the recorded result into the catalog before reopening `F5.2`. | `sed -n '1,220p' ~/src/github.com/firebase/firebase-js-sdk/packages/firestore/package.json`; `sed -n '1,220p' ~/src/github.com/firebase/firebase-js-sdk/packages/firestore/test/integration/api/README.md`; `rg --files ~/src/github.com/firebase/firebase-js-sdk/packages/firestore/test/integration/api`; `yarn test:node test/integration/api/smoke.test.ts` (failed initially: `ts-node: command not found`); `yarn install --frozen-lockfile --ignore-engines` in `~/src/github.com/firebase/firebase-js-sdk`; `yarn test:node test/integration/api/smoke.test.ts` (failed before Neovex: missing `@firebase/app/dist/index.cjs.js`). |
| 2026-04-25 | F5.2 Firebase upstream test catalog | `in_progress` | Closed `F5.1` after publishing a public Firebase compatibility reference and docs index entry grounded in the landed `@neovex/firebase` exports, server contract tests, and the Source Evidence Map. The next documentation slice catalogs the upstream Firebase JS SDK integration corpus into expected-pass, expected-fail, and expected-skip groups so Neovex can track real compatibility progress without overclaiming WebChannel, offline, or broader Admin SDK behavior. The local upstream checkout exists at `~/src/github.com/firebase/firebase-js-sdk`; next action is to inspect the Firestore package test commands and representative API test files, then capture a runnable node-side command or record the exact environment blocker if the checkout is missing dependencies. | Source review of `~/src/github.com/firebase/firebase-js-sdk/packages/firestore/{package.json,test/integration/api/README.md}` plus integration corpus inventory under `packages/firestore/test/integration/api/`; local run attempt planned next from the upstream checkout. |
| 2026-04-25 | F5.1 SDK compatibility matrix | `done` | Published `docs/reference/firebase-compatibility.md` as the public compatibility reference for the landed Firebase adapter. The doc names the current tier snapshot, supported first-party `@neovex/firebase` runtimes and transports, the API areas covered by the package today, and the explicit boundaries around stock browser SDK drop-in, named databases, offline/persistence APIs, bundle APIs, and unclaimed Admin/mobile SDK parity. Added the reference to `docs/README.md` so the matrix is part of the stable docs index rather than buried inside the control plan. Next step: continue with `F5.2 Firebase upstream test catalog`. | Manual docs review of `docs/reference/firebase-compatibility.md`; `rg -n "firebase-compatibility" docs/README.md docs/reference/firebase-compatibility.md docs/plans/firebase-adapter-plan.md`; `git diff -- docs/README.md docs/reference/firebase-compatibility.md docs/plans/firebase-adapter-plan.md`. |
| 2026-04-25 | F5.1 SDK compatibility matrix | `in_progress` | Started the compatibility-documentation wave after confirming `F4` is complete and no other Firebase roadmap item remained active. This slice is turning the implemented `@neovex/firebase` package plus the live REST/gRPC/gRPC-Web/WebSocket server surface into a public compatibility reference that names supported runtimes, transports, covered APIs, explicit gaps, and the difference between Neovex's first-party SDK and deferred stock Firebase SDK/browser-drop-in claims. Next action: publish a reference doc and docs-index entry sourced from the implemented package exports, selftests, server contract tests, and the Source Evidence Map before deciding whether the upstream-test catalog in `F5.2` fits safely in the same run. | Startup source review of `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`, `docs/plans/README.md`, `docs/plans/firebase-adapter-plan.md`, and `docs/prompts/firebase-adapter-start.md`; `git status --short`; dirty-file inspection for Firebase docs/package files; targeted review of the `F5` plan section, `packages/firebase/src/{firestore,selftest}.mjs`, `packages/firebase/package.json`, `docs/reference/firebase-websocket-listen.md`, and Firebase protocol/source-evidence notes in this plan. |
| 2026-04-25 | F4.5b2 Transform result compatibility and end-to-end smoke | `done` | Closed the remaining transform-compatibility gap by extending the package selftest in two directions. First, the in-process package suite now covers gRPC-Web commit compatibility for sentinel writes across direct writes, batches, and transactions, including commit responses that carry `transformResults` so the browser transport path proves it can round-trip Firestore transform payloads without depending on request-shape-only assertions. Second, the live smoke flow in `packages/firebase/src/selftest.mjs --smoke-base-url` now runs one shared sentinel scenario against a real Neovex server over both REST and gRPC-Web: merge-time `deleteField`, `serverTimestamp`, `increment`, `arrayUnion`, `arrayRemove`, batch commits, and transaction commits all round-trip into stored document state with the expected deletes, numeric updates, array mutations, and timestamp-shaped reads. `F4` is now complete; the next queued item is the documentation wave starting at `F5.1 SDK compatibility matrix`. | `npm run test --workspace @neovex/firebase`; `npm run build --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run typecheck`; `cargo test -p neovex-server firebase_sdk_crud_selftest_smoke -- --nocapture`. |
| 2026-04-25 | F4.5b2 Transform result compatibility and end-to-end smoke | `in_progress` | Closed `F4.5b1` after landing canonical modular `FieldValue` sentinels in `packages/firebase/src/firestore.ts` plus shared write extraction that separates ordinary fields, delete masks, and transform payloads before lowering them through the existing commit builders. The next slice is live compatibility proof instead of more request-shape work: extend the package/runtime smoke to run sentinel writes against the local server over REST, add gRPC-Web coverage that tolerates Firestore `transformResults`, and freeze any remaining unsupported edge semantics explicitly in tests instead of leaving them implicit. | `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run typecheck`; `npm run build`. |
| 2026-04-25 | F4.5b1 FieldValue sentinel descriptors and write lowering | `done` | Added canonical SDK `FieldValue` primitives in `packages/firebase/src/firestore.ts` (`deleteField`, `serverTimestamp`, `increment`, `arrayUnion`, `arrayRemove`) and rewired the write builders so direct writes, `writeBatch`, and `runTransaction` all flow through one shared extraction pass that lowers ordinary document fields, delete-mask paths, and transform payloads into the existing Firestore commit request shape without ad hoc request mutation. The package selftest now covers direct/batch/transaction sentinel request shapes plus explicit invalid-combination coverage for overwrite-time `deleteField`, merge-field subtree sentinels, overlapping update paths, array nesting, and root/CJS re-exports; the TypeScript fixture now exercises the new sentinel surface too. | `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run typecheck`; `npm run build`. |
| 2026-04-25 | F4.5b1 FieldValue sentinel descriptors and write lowering | `in_progress` | Closed `F4.5a2b`, then sized the old `F4.5b` row before continuing. The next slice is the SDK sentinel surface itself: inspect `packages/firebase/src/firestore.ts` write encoding/materialization seams against the already-landed shared transform primitives in `crates/neovex-core/src/write_batch.rs` and `crates/neovex-engine/src/service/execution_units/batch.rs`, then land canonical `FieldValue` descriptors plus direct/batch/transaction lowering before taking the end-to-end transform smoke sweep in `F4.5b2`. | `cargo test -p neovex-engine transaction_session --lib`; `cargo test -p neovex-server transaction_selector_with_pinned_snapshot --lib`; `cargo test -p neovex-server firebase_grpc_unary_requests_reject_deferred_selectors --lib`; `cargo test -p neovex-server firebase_sdk_crud_selftest_smoke --lib`; `cargo check -p neovex-storage -p neovex-engine -p neovex-server`; `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `cargo fmt --all`; `cargo fmt --all --check`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,unary,write_stream,mod}.rs`). |
| 2026-04-25 | F4.5a2b Transaction query reads and richer read helpers | `done` | Added a shared structured-query-on-transaction seam instead of a Firebase-local query shim. `crates/neovex-engine/src/service/execution_units/reads.rs` now evaluates structured queries against the execution unit’s pinned snapshot plus staged rows, `crates/neovex-engine/src/service/transactions.rs` exposes transactional structured-query helpers, and the Firebase REST/gRPC `RunQuery` paths now accept active `transaction` selectors and route them through that shared engine/session path. `@neovex/firebase` now supports `Transaction.get(query)` alongside point reads, and the package selftest covers REST and gRPC-Web query reads inside `runTransaction()`. While verifying the new contract, the REST pinned-snapshot test exposed that SQLite read snapshots were not actually starting a read transaction; I fixed that shared primitive in `crates/neovex-storage/src/sqlite/config.rs` and added a transaction-session snapshot regression test in `crates/neovex-engine/src/service/transactions.rs` so begin-time reads stay stable across external updates. | `cargo test -p neovex-engine mutation_execution_unit_structured_query_reads_staged_rows --lib`; `cargo test -p neovex-engine transaction_session --lib`; `cargo test -p neovex-server transaction_selector_with_pinned_snapshot --lib`; `cargo test -p neovex-server firebase_grpc_unary_requests_reject_deferred_selectors --lib`; `cargo test -p neovex-server firebase_sdk_crud_selftest_smoke --lib`; `cargo check -p neovex-storage -p neovex-engine -p neovex-server`; `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `cargo fmt --all`; `cargo fmt --all --check`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,unary,write_stream,mod}.rs`). |
| 2026-04-25 | F4.5a2a Transaction point-read/write client flow | `done` | Added first-class transaction support to `@neovex/firebase` without inventing adapter-local transaction state. `packages/firebase/src/firestore.ts` now exposes `runTransaction`, `Transaction`, and bounded retry/rollback behavior on top of the existing shared commit and batch-get seams; the new client loop stages writes locally, enforces Firestore’s read-before-write rule, retries retryable `ABORTED` conflicts, and rolls back callback failures or read-only transactions cleanly. To support the default REST transport, the server now exposes thin Firebase REST `documents:beginTransaction` and `documents:rollback` handlers plus request parsing in `transaction_request.rs`, while gRPC-Web reuses the already-generated unary surface. The package selftest now covers REST and gRPC-Web transaction request shapes, retries, rollback, root/CJS exports, and TypeScript surface, and the existing live smoke lane now verifies transactional reads/writes/rollback against the local server. | `cargo fmt --all`; `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run typecheck`; `cargo test -p neovex-server firebase_rest_begin_transaction_and_rollback_manage_session_tokens --lib`; `cargo test -p neovex-server firebase_sdk_crud_selftest_smoke --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check`. |
| 2026-04-25 | F4.5a2a Transaction point-read/write client flow | `in_progress` | Split the old `F4.5a2` row after sizing the current server seams: shared transaction sessions already support `BeginTransaction`, transactional point reads via `BatchGetDocuments`, transactional `Commit`, and `Rollback`, but both REST and gRPC `RunQuery` still reject `transaction` / `newTransaction`. The active slice is therefore the clean point-read/write client loop only: add thin REST begin/rollback handlers beside the existing gRPC unary path, teach the SDK to drive begin/read/commit/rollback over both unary transports, and land `runTransaction` with bounded retry/rollback semantics for point reads and staged writes before widening to transactional query reads. | Plan/status reconciliation against `packages/firebase/src/firestore.ts`, `packages/firebase/src/internal/grpc-web.ts`, `crates/neovex-server/src/adapters/firebase/{mod.rs,batch_get_request.rs,run_query_request.rs,grpc/unary.rs}`, and `crates/neovex-engine/src/service/transactions.rs`. |
| 2026-04-25 | F4.5a2 Transaction client surface and retry/rollback flow | `in_progress` | Closed `F4.5a1` after adding `writeBatch()` plus `WriteBatch.set/update/delete/commit` to `@neovex/firebase`, reusing the existing atomic Firestore commit path instead of introducing a second batch wire contract. The package selftest now covers multi-write request shapes, committed-batch reuse rejection, root/CJS re-exports, and the local smoke lane now verifies batched multi-document commits against the live server. The next action is to size the SDK transaction loop against the server’s existing `BeginTransaction` / transactional point-read / `Commit` / `Rollback` seams: inspect how much of the client read surface can be reused inside a transaction, then decide whether `runTransaction` can land in one pass or needs a narrower split between read helpers and retry/rollback orchestration. | `npm run test --workspace @neovex/firebase`; `npm run build --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `cargo test -p neovex-server firebase_sdk_crud_selftest_smoke --lib`; `cargo fmt --all --check`. |
| 2026-04-25 | F4.5a1 WriteBatch client surface and atomic multi-write commit | `done` | Added `WriteBatch` to `packages/firebase/src/firestore.ts` as a thin SDK-owned batching surface over the already-landed atomic commit contract. Batched `set`, `update`, and `delete` operations now collect ordinary Firestore commit writes, enforce same-Firestore ownership, and reject reuse after `commit()`, while the existing REST/gRPC-Web commit transport keeps the multi-write request atomic. The package selftest covers request shapes and root/CJS exports, and the existing smoke harness now validates multi-document batched commits end to end. | `npm run test --workspace @neovex/firebase`; `npm run build --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `cargo test -p neovex-server firebase_sdk_crud_selftest_smoke --lib`; `cargo fmt --all --check`. |
| 2026-04-25 | F4.5a1 WriteBatch client surface and atomic multi-write commit | `in_progress` | Closed `F4.4b2b2` after landing a browser-safe Listen auth-upgrade contract that stays inside the settled local security posture: `@neovex/firebase` now offers the fixed `neovex.firebase.listen.v1` subprotocol plus an optional base64url auth offer in `Sec-WebSocket-Protocol`, retries one-target reconnects with forced token refresh on `1008 unauthenticated`, and the server validates the auth-offer/header contract while only echoing the fixed Listen protocol. I split the old `F4.5a` row because `writeBatch` and `runTransaction` are different risks: batched writes can reuse the existing atomic commit path immediately, while transaction retries still need a larger begin/read/commit/rollback client loop. Next action: add `writeBatch()` / `WriteBatch` to `packages/firebase/src/firestore.ts`, lower multi-write batches onto the existing shared commit request, and cover batched set/update/delete request shapes plus local smoke semantics before starting `F4.5a2`. | `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `cargo check -p neovex-server`; `cargo test -p neovex-server firebase_listen_websocket_accepts_loopback_browser_origin_and_bootstraps --lib`; `cargo test -p neovex-server firebase_routes_remain_application_surfaces_without_local_admin_auth --lib`; `cargo test -p neovex-server firebase_websocket_bad_origin_is_rejected_before_auth --lib`; `cargo fmt --all --check` (reported formatting diffs); `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F4.4b2b2 Browser Listen browser-safe auth upgrade transport | `done` | The browser Listen/auth contract now avoids URL tokens while staying within browser handshake limits and the settled localhost security posture. `packages/firebase/src/internal/listen-websocket.ts` now resolves fixed plus auth subprotocol offers before opening the socket, refreshes auth once on `1008 unauthenticated`, and keeps network retry policy separate from auth refresh. `packages/firebase/src/firestore.ts` now encodes bearer tokens into the optional `neovex.firebase.auth.<base64url-token>` subprotocol offer, while `crates/neovex-server/src/adapters/firebase/grpc/listen_websocket.rs` validates any offered auth token against `Authorization` without echoing it back and explicitly selects only `neovex.firebase.listen.v1`. The reference contract in `docs/reference/firebase-websocket-listen.md` and focused local-server tests now cover the selected subprotocol plus origin compatibility. | `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `cargo check -p neovex-server`; `cargo test -p neovex-server firebase_listen_websocket_accepts_loopback_browser_origin_and_bootstraps --lib`; `cargo test -p neovex-server firebase_routes_remain_application_surfaces_without_local_admin_auth --lib`; `cargo test -p neovex-server firebase_websocket_bad_origin_is_rejected_before_auth --lib`; `cargo fmt --all --check` (reported formatting diffs); `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F4.4b2b1 Browser Listen close-code mapping and bounded retry policy | `done` | Split the old `F4.4b2b` row after sizing showed two different remaining risks: the documented close-code/retry policy can land cleanly on today’s shared WebSocket contract, while browser-safe auth during the HTTP upgrade needs an explicit server/SDK contract. `packages/firebase/src/internal/listen-websocket.ts` now classifies `1003` / `1008` as fatal Firebase errors, retries only retryable network/internal closes (`1006`, `1011`) with a bounded backoff budget, and tears down listeners cleanly once that budget is exhausted instead of reconnecting forever. `packages/firebase/src/firestore.ts` now removes termination hooks on fatal transport errors, and the package selftest covers policy-close mapping, unsupported-frame mapping, bounded retry exhaustion, and the previously landed reconnect continuity path under the new timer-based retry policy. I advanced `F4.4b2b2` to `in_progress`, but stopped before code there because the next step needs a deliberate browser-safe auth upgrade contract; today’s Firebase server routes still bind `PrincipalContext::anonymous()`, and a WebSocket upgrade cannot simply reuse unary request headers. Next action: inspect the existing Firebase application-auth boundary in the server and choose one explicit browser-safe upgrade contract before implementing token propagation. | `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run build`. |
| 2026-04-25 | F4.4b2a Browser Listen resume token retention and reconnect bootstrap | `done` | Retained reconnect state now lives in the browser Listen transport seam instead of leaking into public watch APIs. `packages/firebase/src/internal/listen-websocket.ts` now stores per-target `resume_token` / `read_time`, automatically reopens dropped one-target sessions, and rebuilds `addTarget` frames from the retained cursor, while `packages/firebase/src/firestore.ts` reuses the same snapshot conversion/state maps across reconnect bootstrap by suppressing observer emission until the resumed stream reaches `CURRENT`. In-band `REMOVE` and `RESET` target changes now terminate the session instead of accidentally triggering reconnect loops. Focused selftests now cover resume-token reconnects, retained query snapshot continuity, unsubscribe-versus-reconnect cleanup, and `read_time` fallback when no resume token is available. Next action: layer browser-safe auth transport, close-code/error mapping, and bounded retry policy on top of the retained reconnect core for `F4.4b2b`. | `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run build`. |
| 2026-04-25 | F4.4b1 Browser Listen/watch API and binary session bridge | `done` | Added the first browser watch surface in `packages/firebase/src/firestore.ts` and a dedicated binary Listen socket bridge in `packages/firebase/src/internal/listen-websocket.ts`. `@neovex/firebase` now exposes `onSnapshot` plus `Unsubscribe` / `SnapshotObserver` over the documented `GET /google.firestore.v1.Firestore/Listen` transport, reusing the generated protobuf foundation for one-target add/remove frames while sharing the existing snapshot decoding/model layer for `DocumentSnapshot` and `QuerySnapshot`. Focused selftests now cover document/query bootstrap snapshots, binary ListenRequest encode/decode, unsubscribe cleanup via `removeTarget`, root re-exports, and TypeScript surface coverage through the package fixture. I split the old `F4.4b2` row before continuing because the remaining work spans two different risks: first retained resume-token/reconnect core, then browser auth/close-code/network policy. Next action: add retained `resume_token` / `read_time` state to the new watch bridge and reconnect one-target sessions through the shared WebSocket transport before touching browser auth/header policy. | `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run build`. |
| 2026-04-25 | F4.4b1 Browser Listen/watch API and binary session bridge | `in_progress` | Closed `F4.4a1` and `F4.4a2`, then split the old `F4.4b` row because the browser watch surface is too broad for one safe pass. The current protobuf foundation now exists in `packages/firebase/src/gen/` behind a package-owned `codegen:proto` script and `src/internal/protobuf.ts`, while unary browser transport now has an opt-in `experimentalUnaryTransport: "grpc-web"` path in `packages/firebase/src/firestore.ts` backed by Connect-Web over the existing tonic-web Firebase service, shared auth/header retry behavior, and explicit gRPC-Web error mapping. The next safe slice is browser watch API/session ownership: inspect the existing SDK snapshot/query types plus the documented WebSocket contract in `docs/reference/firebase-websocket-listen.md`, then add `onSnapshot` and one-target binary `Listen` session wiring before taking on resume/reconnect and failure-policy semantics in `F4.4b2`. | `npm run codegen:proto --workspace @neovex/firebase`; `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run typecheck`; focused sizing review of `docs/reference/firebase-websocket-listen.md` and `crates/neovex-server/src/adapters/firebase/grpc/listen_websocket.rs`. |
| 2026-04-25 | F4.4a2 Browser unary gRPC-Web transport foundation | `done` | Added the browser unary transport layer on top of the new protobuf foundation instead of creating a second request model. `packages/firebase/src/internal/grpc-web.ts` now owns a thin Connect-Web `createGrpcWebTransport(...)` bridge with merged Firebase headers, auth token refresh on `401`, and canonical gRPC-to-Firestore error mapping, while `packages/firebase/src/firestore.ts` reuses the existing REST Proto3-JSON request builders by converting them through `fromJson(...)` into protobuf `Commit`, `BatchGetDocuments`, and `RunQuery` messages and decoding server-streaming responses back through `toJson(...)`. The package selftest now covers gRPC-Web request framing, Commit retry-on-401 behavior, BatchGet/RunQuery execution, and permission-denied error mapping. | `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run typecheck`. |
| 2026-04-25 | F4.4a1 Browser protobuf codegen/runtime foundation | `done` | Added a package-owned Firestore protobuf foundation for browser transport work. `packages/firebase/package.json` now declares modern browser/runtime dependencies (`@bufbuild/protobuf`, `@connectrpc/connect`, `@connectrpc/connect-web`, and `@bufbuild/protoc-gen-es`), `packages/firebase/src/codegen-protos.mjs` regenerates TypeScript descriptors from the vendored proto tree under `crates/neovex-server/proto/google/...` using the same pinned Cargo-vendored `protoc` family the Rust server already relies on, and `packages/firebase/src/internal/protobuf.ts` exposes the generated service/message namespaces for internal SDK transport code. The package selftest now verifies bundleability plus protobuf encode/decode roundtrips for Firestore Commit, Listen, and document message shapes. | `npm run codegen:proto --workspace @neovex/firebase`; `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`. |
| 2026-04-25 | F4.4a1 Browser protobuf codegen/runtime foundation | `in_progress` | Reconciled the earlier browser transport blocker by comparing the current codebase with upstream Firebase and modern browser protobuf/RPC tooling. Internal review confirmed the server already exposes the needed wire seams: pinned vendored Firestore protos plus vendored `protoc` in `crates/neovex-server/build.rs`, gRPC-Web routing via `tonic_web::GrpcWebLayer` in `crates/neovex-server/src/router.rs`, and binary-protobuf WebSocket Listen frames in `crates/neovex-server/src/adapters/firebase/grpc/listen_websocket.rs`, while `packages/firebase` is still REST-only and has no protobuf/grpc runtime dependencies. External research showed the upstream Firebase browser SDK uses WebChannel rather than gRPC-Web, the official `grpc-web` stack is reputable but older and unary/server-stream oriented, and the modern TS/browser path is Buf Protobuf-ES plus Connect-Web. The compatibility boundary for `@neovex/firebase` is therefore Firebase API/message semantics on top and modern shared Neovex protobuf/transport primitives underneath. Re-prioritized `F4.5a` back to pending and split the transport work into `F4.4a1` and `F4.4a2` so codegen/runtime foundation lands before transport helpers or transaction APIs depend on the wrong client stack. Next action: add generated browser-safe Firestore protobuf types from the pinned vendored protos, then layer Connect-Web unary transport and WebSocket Listen on the same message foundation. | Internal source inspection of `crates/neovex-server/{build.rs,src/router.rs,src/adapters/firebase/grpc/{mod.rs,unary.rs,listen_websocket.rs}}` and `packages/firebase/{package.json,src/firestore.ts}`; external comparison against the upstream Firebase JS SDK browser transport, the official `grpc-web` stack, Connect-Web, and Protobuf-ES. |
| 2026-04-25 | F4.5a Transactions and atomic write batches | `in_progress` | Closed `F4.3c1` and `F4.3c2` after landing equality helpers plus `withConverter` and converter-backed `data()` flows in `@neovex/firebase`. I split the remaining F4 work again because the next roadmap item exposed a real transport-foundation gap: the repo currently has no vendored JS protobuf/grpc-web runtime in package manifests or installed `node_modules`, while the server’s browser Listen path requires binary protobuf WebSocket frames. I marked that browser-transport foundation as blocked and moved the active control item to `F4.5a`, whose next action is to add thin REST `beginTransaction` / `rollback` handlers on the server and then wire SDK `runTransaction` plus `writeBatch` onto the existing commit/session primitives. | `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run build --workspace @neovex/firebase`; targeted `rg`/`sed` inspection across `packages/firebase`, `package.json`, `crates/neovex-server/src/router.rs`, `crates/neovex-server/src/adapters/firebase/{mod.rs,grpc/unary.rs,grpc/listen_websocket.rs}`, and `crates/neovex-server/proto/google/firestore/v1/firestore.proto`. |
| 2026-04-25 | F4.4a Browser protobuf/grpc-web transport foundation | `blocked` | The current checkout does not include a declared or installed browser-side Firestore protobuf/grpc-web runtime (`grpc-web`, `google-protobuf`, `protobufjs`, `@bufbuild/protobuf`, or similar), while the landed browser Listen route on the server expects binary protobuf `ListenRequest`/`ListenResponse` frames. Hand-rolling that transport layer from scratch in the SDK would be a risky detour, so the next concrete action is to choose and vendor an approved JS protobuf/grpc-web client foundation before implementing browser unary/Listen transport behavior. | Focused `rg` inspection across root/package manifests and `node_modules`, plus server-side Listen WebSocket contract inspection in `crates/neovex-server/src/adapters/firebase/grpc/listen_websocket.rs`. |
| 2026-04-25 | F4.3c2 Converters and typed data conversion | `done` | Added `withConverter` support plus converter-backed `data()` flows across document refs, collection refs, queries, and snapshots in `packages/firebase/src/firestore.ts`, keeping conversion logic in the shared SDK primitive layer instead of transport-specific code. Converted writes now lower through `toFirestore(...)`, converted reads materialize through `fromFirestore(...)`, and the selftest plus TypeScript fixture cover typed roundtrips, `withConverter(null)`, converter-aware query snapshots, and typed CRUD/query promises. | `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run build --workspace @neovex/firebase`. |
| 2026-04-25 | F4.3c1 Equality helpers | `done` | Added `refEqual`, `queryEqual`, and `snapshotEqual` to the shared SDK surface, comparing canonical Firestore identity plus structured query shape and snapshot contents without pushing that logic into transports or adapters. The package selftest now covers equal/unequal refs, collection-group refs, query constraints, document snapshots, and query snapshots across both ESM and CJS bundle surfaces. | `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run build --workspace @neovex/firebase`. |
| 2026-04-25 | F4.3b Query execution snapshots and metadata | `done` | Lowered the new SDK query descriptors onto the live Firebase `RunQuery` REST surface in `packages/firebase/src/firestore.ts`, adding `getDocs`, `QuerySnapshot`, `QueryDocumentSnapshot`, streamed JSON-line response decoding, and direct reconstruction of document refs from Firestore resource names. The package selftest now covers query request shapes, collection and collection-group result decoding, empty query snapshots, cursor/limit execution, and metadata defaults, while the existing local server smoke test now exercises collection, nested collection, and collection-group queries end to end. | `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run build --workspace @neovex/firebase`; `cargo test -p neovex-server firebase_sdk_crud_selftest_smoke --lib`; `cargo fmt --all --check`. |
| 2026-04-25 | F4.3a Query constraint primitive surface | `done` | Split the old `F4.3 Query constraints and snapshots` row before implementation because the supported query-builder surface is a smaller, cleaner slice than live result snapshots and converter/equality adoption. The SDK now exposes structured query-builder primitives that reuse the already-landed Firestore reference/path model instead of inventing a second path abstraction, and the package selftest covers composition, chaining, collection-group/documentId descriptors, duplicate-limit rejection, and cursor validation. | `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`. |
| 2026-04-25 | F4.2b CRUD transport vertical slice | `done` | Added a thin REST CRUD client inside `@neovex/firebase` on top of the existing Firebase adapter vertical slice: point reads now reuse `documents:batchGet`, writes reuse `documents:commit`, shared auth/header handling is centralized in the package transport, generated IDs are client-side and create-only, and the package selftest now covers request shapes, refresh-on-401 behavior, path encoding, and error mapping. The local server smoke lane also surfaced a shared nested patch gap, which was fixed in the engine-owned atomic patch primitive instead of papering it over in the adapter or SDK. | `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run build`; `cargo test -p neovex-engine atomic_write_batch_patch_updates_nested_field_paths --lib`; `cargo test -p neovex-server firebase_sdk_crud_selftest_smoke --lib`; `cargo check -p neovex-engine -p neovex-server`; `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F4.2a Reference and path primitive surface | `done` | Extended the new package scaffold with Firestore reference primitives in `packages/firebase/src/firestore.ts`, so `doc`, `collection`, and `collectionGroup` now expose root and nested resource identity directly in the SDK with path composition, collection/document parity validation, parent-link reconstruction, and collection-group segment validation. The package selftest now exercises runtime reference composition plus invalid-path rejection and typechecks the same subpath imports that future CRUD/query/listen layers will build on. | `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run build`. |
| 2026-04-25 | F4.2a Reference and path primitive surface | `in_progress` | Closed `F4.1` after scaffolding `packages/firebase` as `@neovex/firebase` with `app` / `firestore` entry points, selftest-driven ESM+CJS bundle verification, export-map assertions, and workspace build wiring through the repo root. Split the old `F4.2` row because the next safe slice is reference/path ownership: add `doc`, `collection`, and `collectionGroup` primitives plus canonical path validation first, then land live CRUD transport on top in a follow-up slice. | `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run build`. |
| 2026-04-25 | F4.1 Package scaffold | `done` | Added `packages/firebase` as the new `@neovex/firebase` workspace package, with `app` and `firestore` entry points, basic `initializeApp` / `getApp` / `deleteApp` plus `getFirestore` / `initializeFirestore` / emulator helpers, and a selftest that verifies the export map, ESM+CJS bundleability, runtime scaffolding, and TypeScript path-based imports across the package subpaths. The root workspace now includes the package and builds it through the normal npm workspace flow instead of a one-off script. | `npm run build --workspace @neovex/firebase`; `npm run test --workspace @neovex/firebase`; `npm run typecheck --workspace @neovex/firebase`; `npm run build`. |
| 2026-04-25 | F4.1 Package scaffold | `in_progress` | Closed `F3.5b` after wiring shared non-atomic `BatchWrite` execution and per-write status/result mapping across Firebase REST and gRPC, including duplicate-document rejection and labels-ignore behavior without accidentally reusing the atomic `Commit` contract. The next action is to inspect the existing JS workspace/package conventions and scaffold `@neovex/firebase` so the package surface, export map, and build/typecheck lanes match the rest of the monorepo before API work starts. | `cargo test -p neovex-server batch_write --lib`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,unary,write_stream,mod}.rs`, including longstanding `clippy::result_large_err`, `clippy::large_enum_variant`, and `clippy::too_many_arguments` findings). |
| 2026-04-25 | F3.5b BatchWrite non-atomic per-write statuses | `done` | Added a shared Firebase `BatchWrite` execution path that keeps Firestore writes explicitly non-atomic by lowering the request once, rejecting duplicate document targets up front, and executing each write through its own single-write `AtomicWriteBatch` so REST and gRPC can both return ordered `writeResults` plus per-write google RPC statuses without inventing adapter-local storage semantics. The REST adapter now exposes `documents:batchWrite`, tonic serves unary `BatchWrite`, labels are parsed and ignored explicitly, and shared error/status mapping is reused instead of leaking the atomic `Commit` contract into batch writes. | `cargo test -p neovex-server batch_write --lib`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,unary,write_stream,mod}.rs`, including longstanding `clippy::result_large_err`, `clippy::large_enum_variant`, and `clippy::too_many_arguments` findings). |
| 2026-04-25 | F3.5b BatchWrite non-atomic per-write statuses | `in_progress` | Closed `F3.5a` after landing a shared engine helper over the persisted `resource_path_bindings` metadata, plus REST/gRPC `ListCollectionIds` routes that reuse that helper for root and nested parents with stable ordering, duplicate suppression, and shared page-token behavior. The next action is to keep `BatchWrite` explicitly non-atomic: inspect the existing commit/lowering seams, add duplicate-document rejection before execution, and build shared per-write status/result mapping without accidentally routing the whole request through the atomic `Commit` path. | `cargo test -p neovex-engine list_collection_ids --lib`; `cargo test -p neovex-server list_collection_ids --lib`; `cargo check -p neovex-core -p neovex-storage -p neovex-engine -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs); `cargo fmt --all`; `cargo fmt --all --check`; `cargo test -p neovex-core resource_path --lib`; `cargo test -p neovex-server firebase --lib`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,unary,write_stream,mod}.rs`). |
| 2026-04-25 | F3.5a ListCollectionIds from path metadata | `done` | Added a shared `Service::list_collection_ids_for_parent(...)` helper over the protocol-neutral `ResourcePathBinding` metadata and exposed provider-agnostic binding scans from the storage snapshots, so collection-ID listing stays in Neovex primitives instead of adapter lookup shims. `neovex-core` path helpers now derive the immediate child collection beneath a root or ancestor document, and Firebase REST/gRPC `ListCollectionIds` now serve root plus nested parents with duplicate suppression, lexical ordering, and shared page-token behavior. | `cargo test -p neovex-engine list_collection_ids --lib`; `cargo test -p neovex-server list_collection_ids --lib`; `cargo check -p neovex-core -p neovex-storage -p neovex-engine -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs); `cargo fmt --all`; `cargo fmt --all --check`; `cargo test -p neovex-core resource_path --lib`; `cargo test -p neovex-server firebase --lib`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,unary,write_stream,mod}.rs`). |
| 2026-04-25 | F3.5a ListCollectionIds from path metadata | `in_progress` | Split the old `F3.5 BatchWrite and ListCollectionIds` row before implementation because the remaining work is two different contracts: `ListCollectionIds` can reuse the shared path-metadata seam from `F0.2`, while `BatchWrite` is a separate non-atomic execution model with per-write statuses. Next action: inspect `neovex-engine` snapshot/resource-path scan seams and the Firebase REST/gRPC unary surfaces, then add one shared list-collection-ids helper that both transports can call. | Targeted `rg`/`sed` inspection across `docs/plans/firebase-adapter-plan.md`, `crates/neovex-engine/src/persistence/snapshot.rs`, `crates/neovex-storage/src/*/resource_paths.rs`, `crates/neovex-server/src/adapters/firebase/mod.rs`, and `crates/neovex-server/proto/google/firestore/v1/firestore.proto`. |
| 2026-04-25 | F3.4b2 ServerTimestamp and special-double transform adoption | `done` | Finished the transport/runtime adoption slice on top of the new shared typed-scalar primitive. `crates/neovex-engine/src/service/execution_units/batch.rs` now executes `ServerTimestamp` and special-double extrema through shared typed metadata instead of adapter-local shims, while Firebase REST/gRPC serializers in `crates/neovex-server/src/adapters/firebase/{serializer,mod.rs,grpc/write_stream.rs,grpc/unary.rs,grpc/listen_stream.rs}` roundtrip typed transform results and document reads as canonical Firestore timestamp/double values. | `cargo test -p neovex-engine atomic_write_batch --lib`; `cargo test -p neovex-server firebase_write_stream_roundtrips --lib`; `cargo test -p neovex-server firebase_commit_roundtrips_typed_scalar_transform_results_and_document_reads --lib`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-core -p neovex-engine -p neovex-storage -p neovex-server`; `cargo fmt --all`; `cargo fmt --all --check`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,unary,write_stream,mod}.rs`). |
| 2026-04-25 | F3.4b1 Shared typed scalar value foundation | `done` | Landed the missed shared primitive-hardening layer instead of keeping Firestore-only semantics trapped in adapter code. `crates/neovex-core/src/{typed_scalar.rs,document.rs,write_batch.rs}` now model typed scalar metadata and stored transform values, `crates/neovex-engine/src/service/execution_units/batch.rs` and query projection paths preserve that metadata, and storage providers persist/read `typed_fields` alongside document JSON so timestamps and special doubles survive provider roundtrips without user-field collisions. | `cargo test -p neovex-core typed_scalar --lib`; `cargo test -p neovex-storage typed_scalar_metadata --lib`; `cargo test -p neovex-engine atomic_write_batch --lib`; `cargo check -p neovex-core -p neovex-engine -p neovex-storage -p neovex-server`; `cargo fmt --all`; `cargo fmt --all --check`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,unary,write_stream,mod}.rs`). |
| 2026-04-25 | F3.4b1 Shared typed scalar value foundation | `in_progress` | Pivoted back to the blocked typed-scalar gap after reviewing the current storage/query seams. The blocker is a missed primitive-hardening layer, not a Firebase-only edge case: SQL providers persist only plain `document.fields` JSON today, so true typed transform values need a shared core/storage/engine foundation before the adapter can stay thin. Next action: add a protocol-neutral typed-scalar primitive to `neovex-core`, thread it through document persistence and engine transform state, then land Firebase serialization/execution on top in `F3.4b2`. | Startup reconciliation of the dirty Firebase worktree; focused design review across `crates/neovex-core/src/document.rs`, `write_batch.rs`, `crates/neovex-engine/src/service/execution_units/batch.rs`, `crates/neovex-server/src/adapters/firebase/{serializer,mod.rs,grpc/write_stream.rs}`, and the SQL/redb document persistence seams. |
| 2026-04-25 | F3.4b Typed Firestore scalar value support for transforms | `blocked` | Closed `F3.4a` after landing shared engine execution for numeric and array transforms in `crates/neovex-engine/src/service/execution_units/batch.rs`, plus focused engine/Firebase REST/gRPC tests covering transform-only writes, `updateTransforms`, ordered `transform_results`, integer-vs-double extrema behavior, array union/remove equivalence, and rollback on unsupported typed-scalar transforms. The remaining `F3.4b` gap is now explicit: `neovex-core` documents still store `serde_json::Value`, so there is no protocol-neutral way to persist Firestore-only scalar values such as timestamps or NaN/Infinity across engine, storage, query, REST, and gRPC surfaces without a broader shared value-model expansion or a storage-visible typed-scalar metadata design. Next action: size that shared value-model work across `neovex-core`, `neovex-storage`, `neovex-engine`, and the Firebase serializers before implementation, then land true `serverTimestamp` plus special-double transform semantics on top of it. | `cargo test -p neovex-engine atomic_write_batch --lib`; `cargo test -p neovex-server firebase_commit --lib`; `cargo test -p neovex-server firebase_write_stream --lib`; `cargo check -p neovex-engine -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs in `crates/neovex-engine/src/service/execution_units/{batch,tests}.rs`); `cargo fmt --all`; `cargo fmt --all --check`; `make clippy` (still fails on the pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,unary,write_stream,mod}.rs`; this pass also fixed the one new `clippy::collapsible_if` finding in `execution_units/batch.rs` before rerunning). |
| 2026-04-25 | F3.3 Aggregation queries | `done` | Added a shared structured aggregation primitive to `neovex-core` / `neovex-engine` so Firestore aggregation behavior rides the existing structured-query engine instead of a Firebase-local query shim. `crates/neovex-engine/src/service/queries/structured.rs` now validates aggregation aliases/count bounds, trims count-only scans with `COUNT_UP_TO`, executes count aggregations across both single-collection and collection-group structured queries, and returns explicit unsupported errors for deferred `sum` / `avg`. On the server side, `crates/neovex-server/src/adapters/firebase/{mod.rs,run_aggregation_query_request.rs}` now serve REST `documents:runAggregationQuery` at root and document-parent scopes, while `crates/neovex-server/src/adapters/firebase/grpc/{mod.rs,unary.rs}` now expose native gRPC `RunAggregationQuery` with the same count-only semantics and deferred-selector errors. Focused tests cover alias defaulting, filtered counts, empty counts, parent-scoped aggregation routes, gRPC count responses, and explicit `transaction` / `read_time` / `sum` rejection. | `cargo test -p neovex-core query --lib`; `cargo test -p neovex-engine structured --lib`; `cargo test -p neovex-server run_aggregation_query_request --lib`; `cargo test -p neovex-server firebase_run_aggregation_query --lib`; `cargo test -p neovex-server firebase_grpc_run_aggregation_query --lib`; `cargo test -p neovex-server firebase_grpc_unary_requests_reject_deferred_selectors --lib`; `cargo check -p neovex-core -p neovex-engine -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs); `cargo fmt --all`; `cargo fmt --all --check`; `make clippy` (still fails on the pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,unary,write_stream,mod}.rs`, including longstanding `result_large_err`, `large_enum_variant`, and `too_many_arguments` findings); `cargo test -p neovex-server firebase --lib`. |
| 2026-04-25 | F3.3 Aggregation queries | `in_progress` | Closed `F3.2b2` after extending Postgres and MySQL with the same `resource_path_bindings` sidecar contract already used by Redb, SQLite, and libsql, including provider-local DDL, read-snapshot export, and execution-unit write/delete hooks. The next slice is `RunAggregationQuery`: add a shared aggregation primitive over the structured-query engine, keep count execution protocol-neutral, and return explicit unsupported errors for sum/avg until shared numeric aggregation semantics land. | Startup reconciliation of the dirty Firebase worktree; targeted diff review across the new Postgres/MySQL resource-path helpers plus the structured-query/Firebase unary seams that will own `RunAggregationQuery`. |
| 2026-04-25 | F3.2b2 Postgres/MySQL path-metadata parity for collection groups | `done` | Added `resource_path_bindings` sidecar persistence to the Postgres and MySQL providers, including tenant-init DDL, provider-local upsert/remove helpers, read-snapshot export, and execution-unit batch integration so insert/update/delete keep document/index/journal and path metadata atomic on every SQL backend. Postgres stores raw locator/document-path keys directly, while MySQL indexes SHA-256 digests and keeps the authoritative raw keys in blobs so Firestore path keys do not depend on InnoDB indexed-byte limits. The shared engine snapshot now exposes collection-group binding scans for Postgres/MySQL, and new provider plus engine tests prove path-binding round trips, atomic delete cleanup, and collection-group queries with full document-path cursors on both external SQL providers. | `cargo fmt --all`; `cargo check -p neovex-storage -p neovex-engine`; `cargo test -p neovex-storage postgres_resource_path_bindings_round_trip_without_table_name_delimiter_tricks --lib`; `cargo test -p neovex-storage postgres_execution_unit_batch_persists_and_removes_resource_path_bindings_atomically --lib`; `cargo test -p neovex-storage mysql_resource_path_bindings_round_trip_without_table_name_delimiter_tricks --lib`; `cargo test -p neovex-storage mysql_execution_unit_batch_persists_and_removes_resource_path_bindings_atomically --lib`; `cargo test -p neovex-engine typed_postgres_config_collection_group_queries_use_path_binding_metadata --lib`; `cargo test -p neovex-engine typed_mysql_config_collection_group_queries_use_path_binding_metadata --lib`; `cargo fmt --all --check`; `make clippy` (still fails only on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,unary,write_stream,mod}.rs`). |
| 2026-04-25 | F3.2b2 Postgres/MySQL path-metadata parity for collection groups | `in_progress` | Closed the SQLite/libsql slice and promoted the remaining SQL-provider parity work into the active control item. Follow-up inspection across `crates/neovex-storage/src/postgres/{config,backend,read,write}.rs` and `crates/neovex-storage/src/mysql/{backend,read,write}.rs` confirmed that both providers need the same new sidecar-table contract, but their tenant-init DDL, read-snapshot structs, and write-transaction helpers are still separate. Next action: add a `resource_path_bindings` sidecar table to Postgres/MySQL tenant init, extend the read snapshots so collection-group binding scans survive snapshot export, and thread execution-unit insert/update/delete through provider-local upsert/remove helpers before running the external-provider parity tests. | Plan checkpoint after completing the SQLite/libsql provider parity slice, plus targeted source inspection of the Postgres/MySQL schema/read/write seams. |
| 2026-04-25 | F3.2b1 SQLite/libsql path-metadata parity for collection groups | `done` | Added a `resource_path_bindings` sidecar table to the default SQLite schema and libsql replica/remote namespace flow, then threaded execution-unit insert/update/delete batches through the same path-binding upsert/remove contract Redb already used. `crates/neovex-storage/src/sqlite/resource_paths.rs` and `crates/neovex-storage/src/libsql/resource_paths.rs` now keep locator lookup, full document-path lookup, and collection-group scan metadata outside user documents while committing atomically with document/index/journal changes. The shared engine snapshot in `crates/neovex-engine/src/persistence/snapshot.rs` now exposes collection-group bindings for SQLite/libsql, the default Firebase REST/gRPC collection-group query tests in `crates/neovex-server/src/tests.rs` now run on embedded SQLite instead of a Redb-only provider, and a new libsql replica engine test proves collection-group queries use the replicated path metadata instead of a Redb-only capability. | `cargo check -p neovex-storage -p neovex-engine -p neovex-server`; `cargo test -p neovex-storage sqlite_resource_path_bindings_round_trip_without_table_name_delimiter_tricks --lib`; `cargo test -p neovex-storage sqlite_execution_unit_batch_persists_and_removes_resource_path_bindings_atomically --lib`; `cargo test -p neovex-storage libsql_execution_unit_batch_round_trips_resource_path_bindings --lib`; `cargo test -p neovex-server firebase_run_query_collection_group_uses_path_metadata_for_scope_ordering_cursors_and_deletes --lib`; `cargo test -p neovex-server firebase_grpc_run_query_supports_collection_group_cursors_with_full_document_names --lib`; `cargo test -p neovex-engine libsql_replica_collection_group_queries_use_path_binding_metadata --lib`; `cargo fmt --all` (required after one import-order diff); `cargo fmt --all --check`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,unary,write_stream,mod}.rs`). |
| 2026-04-25 | F3.2b1 SQLite/libsql path-metadata parity for collection groups | `in_progress` | Split the original provider-parity row because SQLite/libsql and the external SQL providers sit behind different storage mechanics and refresh paths. The active implementation target is the default SQL-backed path first: add the `F0.2` resource-path binding sidecar to `SqliteTenantStore` and the libsql replica/remote namespace flow so collection-group queries work on embedded SQLite and libsql without a Redb-only metadata dependency. Next action: add the sidecar tables plus read/write helpers, then validate collection-group query execution on SQLite and the libsql replica path before deciding whether Postgres/MySQL stay in the same phase slice. | Plan split after targeted source inspection of `crates/neovex-storage/src/{sqlite,libsql}/` plus the shared collection-group query execution seam in `crates/neovex-engine/src/persistence/snapshot.rs`. |
| 2026-04-25 | F3.2b Provider path-metadata parity for collection groups | `in_progress` | Split the original `F3.2` row after landing the shared collection-group execution path. The current blocker is not query semantics anymore; it is persistence parity. `crates/neovex-engine/src/persistence/snapshot.rs` now exposes collection-group binding scans only when the active provider can actually read the `F0.2` path-binding metadata, and the default embedded SQLite path still returns an explicit invalid-input error because only Redb currently persists those bindings. Next action: thread resource-path binding persistence/read APIs through `SqliteTenantStore`/libsql first, then decide whether Postgres/MySQL should land in the same slice or split again. | Discovery during implementation plus focused Redb-backed validation of the new collection-group execution path. |
| 2026-04-25 | F3.2a Shared collection-group query execution | `done` | Finished the first collection-group slice by adding a shared engine execution path in `crates/neovex-engine/src/service/queries/structured.rs` that fans one structured query across every bound collection path for a collection group, using the path metadata from `F0.2` instead of delimiter tricks. The query evaluator now treats `__name__` as a full document-path key for collection-group filters/order/cursors, and the Firebase REST/gRPC `RunQuery` surfaces in `crates/neovex-server/src/adapters/firebase/{mod.rs,grpc/unary.rs}` now return per-document Firestore resource names instead of assuming one fixed collection path per result set. Focused end-to-end tests in `crates/neovex-server/src/tests.rs` cover multi-ancestor collection groups, parent scoping, full-path cursors, and delete removal on Redb-backed services. | `cargo test -p neovex-engine structured --lib`; `cargo test -p neovex-server firebase_run_query_collection_group_uses_path_metadata_for_scope_ordering_cursors_and_deletes --lib`; `cargo test -p neovex-server firebase_grpc_run_query_supports_collection_group_cursors_with_full_document_names --lib`; `cargo test -p neovex-server firebase_run_query --lib`; `cargo test -p neovex-server firebase_grpc_run_query --lib`; `cargo check -p neovex-engine -p neovex-server`. |
| 2026-04-25 | F3.2 Collection group queries | `in_progress` | Started the second F3 semantic-breadth slice immediately after closing `F3.1`. The active design target is the shared structured-query execution and Firebase `RunQuery` lowering path so `CollectionSelector { all_descendants: true }` uses the path metadata introduced in `F0.2` instead of table-name wildcard tricks, and the resulting behavior is shared by REST and gRPC query surfaces before aggregation or SDK work layers on top. | `git status --short`; plan checkpoint after `F3.1`; targeted `sed`/`rg` inspection will focus on `crates/neovex-core/src/resource_path.rs`, `crates/neovex-storage/src/store/resource_paths.rs`, `crates/neovex-engine/src/service/queries/structured.rs`, `crates/neovex-engine/src/service/queries/documents.rs`, and the Firebase query lowering files in `crates/neovex-server/src/adapters/firebase/`. |
| 2026-04-25 | F3.1 Advanced filters and query fields | `done` | Finished the first F3 semantic-breadth slice by expanding the shared structured-query path in `crates/neovex-engine/src/service/queries/structured.rs` to handle composite `AND`/`OR`, unary null/NaN predicates, array and set-membership operators, canonical invalid-combination validation, document ID / `__name__` sentinel behavior, cursor normalization, offsets, projections, and implicit `__name__` tie-break ordering. Firebase REST and gRPC `RunQuery` lowering in `crates/neovex-server/src/adapters/firebase/run_query_request.rs` and `grpc/unary.rs` now preserve reference-valued document-name selectors/cursors so the richer semantics stay protocol-neutral instead of adapter-local. | `cargo test -p neovex-engine structured --lib`; `cargo test -p neovex-server run_query_request --lib`; `cargo test -p neovex-server firebase_run_query --lib`; `cargo test -p neovex-server firebase_grpc_run_query --lib`; `cargo check -p neovex-engine -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs in the new query code); `cargo fmt --all`; `cargo fmt --all --check`; `make clippy` (still fails on pre-existing Firebase gRPC lint debt in `crates/neovex-server/src/adapters/firebase/grpc/listen_stream.rs`, `grpc/write_stream.rs`, `grpc/mod.rs`, and older `grpc/unary.rs` helper signatures, not on the new query semantics). |
| 2026-04-25 | F3.1 Advanced filters and query fields | `in_progress` | Started the first F3 semantic-breadth slice after reconciling the current dirty Firebase worktree and the completed F2 handoff. The active implementation target is the shared `StructuredQuery` execution path in `crates/neovex-engine/src/service/queries/structured.rs` plus the existing Firebase `RunQuery` lowering seams so composite/unary filters, array/set-membership operators, document ID sentinel handling, and implicit `__name__` ordering land in shared query execution instead of adapter-local shims. | Startup reconciliation only: `git status --short`; required docs re-read (`AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`, `docs/plans/README.md`, `docs/plans/firebase-adapter-plan.md`, `docs/prompts/firebase-adapter-start.md`); targeted `sed`/`rg` source inspection across `neovex-core`, `neovex-engine`, `neovex-server`, and the vendored Firestore `query.proto`. |
| 2026-04-25 | F2.6b2 ListDocuments gRPC surface | `done` | Finished the last `F2` slice by wiring `ListDocuments` through the shared Firebase path/query helpers in `crates/neovex-server/src/adapters/firebase/grpc/unary.rs` and `crates/neovex-server/src/adapters/firebase/grpc/mod.rs`. The handler now lists one explicit collection beneath a root or document parent with shared resource parsing, stable `__name__ ASC` ordering, and response masks, while explicitly rejecting the deferred pagination, page-token, custom ordering, show-missing, and read-selector contract gaps instead of silently faking Firestore semantics. `F2: gRPC and streaming adapter` is now complete. Next queued item is `F3.1 Advanced filters and query fields`, but I stopped at the phase boundary before taking on the broader Firestore semantic-expansion wave. | `cargo check -p neovex-server`; `cargo test -p neovex-server firebase_grpc --lib`; `cargo test -p neovex-server firebase --lib`; `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F2.6b2 ListDocuments gRPC surface | `in_progress` | Closed `F2.6b1` after wiring tonic handlers for `GetDocument`, `CreateDocument`, `UpdateDocument`, and `DeleteDocument` through the shared Firebase commit/read helper seams instead of inventing CRUD-local storage logic. Point CRUD now reuses the existing write-batch lowering from `crates/neovex-server/src/adapters/firebase/grpc/write_stream.rs`, the shared path parser in `resource_names.rs`, and the shared commit/read helpers in `crates/neovex-server/src/adapters/firebase/mod.rs`, including explicit-id versus generated-id creation, response masks, transaction-backed `GetDocument`, and delete preconditions. Next action: finish the split `F2.6b2` slice by adding `ListDocuments` on top of the same path/query helpers, likely with explicit unsupported errors for deferred pagination/order/show-missing/read selectors. | `cargo check -p neovex-server`; `cargo test -p neovex-server firebase_grpc --lib`; `cargo test -p neovex-server firebase --lib`; `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F2.6b1 Point CRUD gRPC surface | `in_progress` | Split the old `F2.6b` row because point CRUD and `ListDocuments` are different risk layers. `GetDocument`, `CreateDocument`, `UpdateDocument`, and `DeleteDocument` can stay thin over the existing path parser plus shared commit/read helpers from `F1`/`F2.6a`, while `ListDocuments` still carries a larger pagination/order/show-missing selector contract. Next action: add tonic handlers for point CRUD on top of the shared Firebase helper seams and prove explicit-id create, masked read responses, update masks, delete preconditions, and canonical error mapping before widening to `ListDocuments`. | Split review of the Firestore CRUD/ListDocuments proto surfaces plus the new shared unary helper seams landed for `F2.6a`. |
| 2026-04-25 | F2.6a Shared unary gRPC wrappers for existing REST and transaction flows | `done` | Added shared Firebase execution helpers in `crates/neovex-server/src/adapters/firebase/mod.rs` so REST and gRPC now share commit, batch-get, run-query, begin-transaction, and rollback behavior instead of duplicating transport-local storage logic. Then replaced the tonic unary/server-streaming stubs in `crates/neovex-server/src/adapters/firebase/grpc/{mod,unary.rs}` with live handlers for `Commit`, `BatchGetDocuments`, `RunQuery`, `BeginTransaction`, and `Rollback`, reusing the existing write-batch lowering from `write_stream.rs`, direct protobuf-to-core structured-query lowering, and shared status mapping. `firebase_grpc_status(...)` now also maps shared missing-index errors to `FAILED_PRECONDITION` so the gRPC query surface matches the REST error contract at the status-code level. Next step: start the split `F2.6b1 Point CRUD gRPC surface` item. | `cargo check -p neovex-server`; `cargo test -p neovex-server firebase_grpc --lib`; `cargo test -p neovex-server firebase --lib`; `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F2.6a Shared unary gRPC wrappers for existing REST and transaction flows | `in_progress` | Split the old `F2.6` row after closing the browser Listen transport. The low-risk next slice is to reuse the existing Firebase REST/engine seams for unary gRPC `Commit`, `BatchGetDocuments`, `RunQuery`, `BeginTransaction`, and `Rollback` before taking on Firestore-only point CRUD and `ListDocuments` semantics. Next action: inspect the generated Firestore unary RPC surface and promote shared helper seams out of `crates/neovex-server/src/adapters/firebase/mod.rs` so tonic handlers can call the same commit/read/query/transaction logic without REST-specific JSON/body extraction. | Focused sizing review of the `F2.6` plan row, current `crates/neovex-server/src/adapters/firebase/grpc/mod.rs` stub surface, and the existing REST adapter entrypoints in `crates/neovex-server/src/adapters/firebase/mod.rs`. |
| 2026-04-25 | F2.5b WebSocket security/browser smoke and protocol doc | `done` | Closed the second browser Listen transport slice by adding loopback-browser smoke coverage, explicit close-code assertions, Firebase WebSocket local-security/origin tests, and Firebase WebSocket origin audit coverage, then documenting the production browser `Listen` contract in `docs/reference/firebase-websocket-listen.md`. The WebSocket path now has stable evidence for allowed loopback origins, rejected non-loopback origins, `1003` text-frame rejection, `1008` malformed-protobuf/policy closure, and `1011` bounded-backpressure closure without introducing a separate Firebase-local session path. Next step: start the split `F2.6a Shared unary gRPC wrappers for existing REST and transaction flows` item. | `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs in `crates/neovex-server/src/tests.rs`); `cargo fmt --all`; `cargo fmt --all --check`; `make clippy` (currently fails on existing Firebase gRPC lint debt such as `clippy::result_large_err` / `clippy::large_enum_variant` in `crates/neovex-server/src/adapters/firebase/grpc/{listen_stream,write_stream}.rs`). |
| 2026-04-25 | F2.5a WebSocket Listen framing and shared session bridge | `done` | Refactored `crates/neovex-server/src/adapters/firebase/grpc/listen_stream.rs` into a transport-neutral Listen session driven by a generic `Stream<Item = Result<ListenRequest, Status>>`, then added a thin binary-protobuf WebSocket adapter in `crates/neovex-server/src/adapters/firebase/grpc/listen_websocket.rs`. The router now mounts one shared `FirestoreGrpcService` instance on both gRPC and WebSocket `Listen`, so retained target state, resume tokens, and stream bookkeeping stay shared across browser and native reconnects instead of forking a Firebase-local session path. Next step: close the follow-on `F2.5b` slice for WebSocket origin/security coverage, browser smoke behavior, close-code assertions, and protocol documentation. | `cargo test -p neovex-server firebase_listen_websocket --lib`; `cargo test -p neovex-server firebase_listen --lib`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check`; `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F2.5a WebSocket Listen framing and shared session bridge | `in_progress` | Split the original `F2.5` item because the first risk is transport reuse: `crates/neovex-server/src/adapters/firebase/grpc/listen_stream.rs` should become a source-agnostic Listen session that both tonic and browser WebSocket transports can drive with the same target bookkeeping, while protocol docs, browser smoke behavior, and explicit security/close-code coverage are a second risk layer. Next action: refactor the Listen session off tonic-specific `Streaming<ListenRequest>` input, add a Firebase WebSocket upgrade handler on the Listen path, and prove binary protobuf add/remove/resume frames against the shared session before documenting and polishing the browser contract. | Focused source inspection of the current gRPC Listen implementation, WebSocket route/security seams, and WebSocket test helpers before choosing the split. |
| 2026-04-25 | F2.5 WebSocket Listen transport | `in_progress` | Advanced to the browser-facing Listen transport after closing the native gRPC stream contract. `crates/neovex-server/src/adapters/firebase/grpc/listen_stream.rs` now covers add/remove, resume, concurrent targets, once-target auto-remove, count-mismatch `ExistenceFilter` plus reset fallback, and explicit slow-consumer failure, so the next step is to inspect the existing WebSocket transport and auth seams and decide how thin the browser Listen framing layer can stay over that implementation. Next action: inspect the current WebSocket socket transport, Firebase route family wiring, and any existing protobuf/frame helpers before deciding whether `F2.5` needs a small framing/auth split. | Boundary checkpoint after closing the native gRPC Listen stream semantics. |
| 2026-04-25 | F2.4b2b Listen existence filters, once targets, and bounded backpressure | `done` | Completed the last native gRPC Listen semantics slice in `crates/neovex-server/src/adapters/firebase/grpc/listen_stream.rs` by honoring `once` targets, translating resume-count mismatches into count-only `ExistenceFilter` responses plus `RESET` fallback, and making slow-consumer behavior explicit with a bounded per-stream target-update queue that fails the stream with `RESOURCE_EXHAUSTED` instead of silently wedging shared subscriptions. Focused tonic coverage in `crates/neovex-server/src/tests.rs` now proves once-target auto-remove, resume-token `expected_count` mismatch recovery, and slow-consumer backpressure failure on top of the earlier multi-target/resume coverage. Next step: start `F2.5 WebSocket Listen transport`. | `cargo test -p neovex-server firebase_listen --lib`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs in `crates/neovex-server/src/adapters/firebase/grpc/listen_stream.rs` and `crates/neovex-server/src/tests.rs`); `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F2.4b2b Listen existence filters, once targets, and bounded backpressure | `in_progress` | Advanced to the final native gRPC Listen follow-up after closing concurrent target multiplexing. `crates/neovex-server/src/adapters/firebase/grpc/listen_stream.rs` now hosts multiple active targets plus per-target routing, but `expected_count` / `ExistenceFilter`, `once`, and explicit slow-consumer behavior still remain deferred because they change client-visible stream semantics and need a dedicated pass. Next action: inspect the generated `ExistenceFilter` / `BloomFilter` proto shapes plus the new multi-target session state to choose whether Firestore count mismatches should emit an existence filter, `RESET`, or both in phase order before wiring bounded fan-out behavior. | Risk boundary checkpoint after landing and verifying multi-target Listen routing. |
| 2026-04-25 | F2.4b2a Listen concurrent targets and per-target routing | `done` | Reworked `crates/neovex-server/src/adapters/firebase/grpc/listen_stream.rs` from a single-active-target session into a multiplexed per-target registry with one shared update queue, so one Firestore `Listen` stream can now host multiple query targets concurrently without introducing a Firebase-local storage path. The stream now keeps distinct target IDs, boots additional targets after the first one is already active, routes interleaved updates by target ID, preserves overlapping-query routing when the same document matches more than one target, and cleans up only the removed target while leaving the rest of the stream alive. Focused tonic coverage in `crates/neovex-server/src/tests.rs` now proves distinct server-assigned IDs across multiple active targets plus overlapping-target update/remove routing and cleanup. Next step: start the split `F2.4b2b Listen existence filters, once targets, and bounded backpressure` item. | `cargo test -p neovex-server firebase_listen --lib`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs in `crates/neovex-server/src/adapters/firebase/grpc/listen_stream.rs` and `crates/neovex-server/src/tests.rs`); `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F2.4b2a Listen concurrent targets and per-target routing | `in_progress` | Split the old `F2.4b2` row because concurrent target multiplexing is the next clean Listen risk slice, while `ExistenceFilter`, `once`, and slow-consumer behavior are a separate correctness boundary. `crates/neovex-server/src/adapters/firebase/grpc/listen_stream.rs` now has the retained resume registry from `F2.4b1`, but the active stream registry still assumes one target plus one update receiver. Next action: refactor the session to host multiple active query targets on a single stream and prove per-target add/remove/update routing before layering existence-filter and backpressure behavior. | Focused source inspection of the current single-target Listen registry plus Firestore `Target`, `TargetChange`, and `ExistenceFilter` proto semantics. |
| 2026-04-25 | F2.4b1 Listen resume registry and stream-consistent tokens | `done` | Completed the one-target resume slice in `crates/neovex-server/src/adapters/firebase/grpc/listen_stream.rs` by retaining the latest bootstrap/delivery snapshot and `read_time` per Firestore target identity across stream instances, accepting `resume_token` or `read_time` selectors for identical targets, and emitting `RESET` before a full bootstrap whenever the client selector is stale or unknown. Focused tonic coverage in `crates/neovex-server/src/tests.rs` now proves resume-after-reconnect delta delivery, stale-token reset-to-bootstrap behavior, and monotonic `NO_CHANGE` / `CURRENT` resume-token and `read_time` advancement. Next step: start the split `F2.4b2a Listen concurrent targets and per-target routing` item. | `cargo test -p neovex-server firebase_listen --lib`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs in `crates/neovex-server/src/adapters/firebase/grpc/listen_stream.rs`); `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F2.4b1 Listen resume registry and stream-consistent tokens | `in_progress` | Split the remaining Listen follow-up after `F2.4a` because reconnect/resume semantics need server-owned state that survives a stream instance, while concurrent target bookkeeping and backpressure are a separate risk level. `F2.4a` now owns one-target add/remove plus bootstrap/current delivery and single-target follow-up diffs inside `crates/neovex-server/src/adapters/firebase/grpc/listen_stream.rs`, but reconnect resume still needs a registry keyed by Firestore target identity and token plus an explicit `RESET` path when the client token no longer matches retained state. Next action: design the persisted listen-target registry and wire `resume_token` / `read_time` handling for one target across reconnects before widening to concurrent targets. | Plan split based on the landed one-target Listen implementation and the remaining reconnect-vs-multi-target risk boundary. |
| 2026-04-25 | F2.4a Listen target lifecycle and bootstrap snapshot | `done` | Replaced the generated Firestore `Listen` stub with a stateful tonic implementation in `crates/neovex-server/src/adapters/firebase/grpc/listen_stream.rs` that keeps all watch state on the shared engine subscription path. The stream now accepts one active query target, assigns or echoes Firestore target IDs, lowers the supported structured-query subset onto the shared subscription `Query` surface, translates the shared bootstrap snapshot into Firestore `ADD` / `DocumentChange` / `CURRENT` messages, forwards single-target follow-up diffs as `DocumentChange` / `DocumentDelete` / `DocumentRemove` plus `NO_CHANGE`, and drops the shared subscription cleanup handle on remove or stream close so registrations do not leak. Next step: start the split `F2.4b1 Listen resume registry and stream-consistent tokens` item. | `cargo check -p neovex-server`; `cargo test -p neovex-server firebase_listen --lib`; `cargo test -p neovex-server firebase --lib`; `cargo fmt --all --check` (reported formatting diffs in the new Listen files); `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F2.4a Listen target lifecycle and bootstrap snapshot | `in_progress` | Split the original `F2.4 Listen bidi stream` row because the full Listen contract spans two different risk levels. Source inspection confirmed the shared engine already exposes protocol-neutral subscription bootstrap/delivery snapshots in `crates/neovex-engine/src/service/subscriptions.rs` and `crates/neovex-engine/src/subscriptions/delivery.rs`, while the Firestore proto surface in `google.firestore.v1.ListenRequest` / `ListenResponse` still needs adapter-local target IDs, add/remove lifecycle, and message encoding before resume tokens, existence filters, and multi-target consistency become safe to layer on. Next action: add a server-owned Listen target registry plus tonic `Listen` handling for one-target add/remove, target ID assignment, bootstrap snapshot translation into Firestore target/document changes, and cleanup on remove/stream close. | Focused source inspection of the shared subscription bootstrap/delivery seams plus Firestore Listen/TargetChange/ExistenceFilter proto messages to size and split the next stream slice safely. |
| 2026-04-25 | F2.3 Write bidi stream | `done` | Replaced the generated Firestore `Write` stub with a stateful tonic implementation in `crates/neovex-server/src/adapters/firebase/grpc/write_stream.rs`, threaded shared `AppState` into the gRPC service/router, and kept all writes on the existing shared mutation path by lowering gRPC `Write` messages into `AtomicWriteBatch` requests. The stream now supports handshake stream IDs/tokens, bounded server-owned stream registry state, resume/replay from prior tokens, ordered write responses, missing-token validation, and atomic rollback on shared transform failures. Source review also confirmed the Firestore `Write` RPC itself does not carry transaction options, so transaction session handling remains on the unary `BeginTransaction` / `Commit` / `Rollback` path for later `F2.6` work instead of living in the bidi write transport. Next step: start the split `F2.4a Listen target lifecycle and bootstrap snapshot` item. | `cargo check -p neovex-server`; `cargo test -p neovex-server firebase_write_stream --lib`; `cargo test -p neovex-server firebase --lib`; `cargo fmt --all --check` (reported formatting diffs); `cargo fmt --all`; `cargo fmt --all --check`; `cargo check -p neovex-server`. |
| 2026-04-25 | F2.3 Write bidi stream | `in_progress` | Started the first native streaming write slice after closing the tonic route plumbing. Initial inspection covered `google.firestore.v1.WriteRequest` / `WriteResponse` handshake semantics in `crates/neovex-server/proto/google/firestore/v1/firestore.proto` plus the shared batch and transaction-session seams already available in `crates/neovex-core/src/write_batch.rs`, `crates/neovex-engine/src/service/execution_units/batch.rs`, and `crates/neovex-engine/src/service/transactions.rs`. Next action: design a server-owned write-stream registry keyed by Firestore `stream_id` and ack token that can stage `AtomicWriteBatch` execution and optional transaction-session begin/commit semantics without introducing a Firebase-local commit path. | Focused source inspection of the Firestore write-stream proto messages and shared batch / transaction-session entrypoints before selecting the next implementation seam. |
| 2026-04-25 | F2.2 tonic + axum + tonic-web routing | `done` | Closed the shared-port transport slice by mounting the generated Firestore service on the existing axum router under `/google.firestore.v1.Firestore/{*grpc_method}` and wrapping it with `tonic_web::GrpcWebLayer`, so HTTP/2 gRPC and gRPC-Web now share the same Firebase route family, listener, CORS/origin middleware, and local-server policy classification as the REST routes. Focused server tests now prove Firebase-enabled gRPC and gRPC-Web requests no longer fall through to `404`, existing loopback preflight headers remain available, enabled Firebase routes stay application surfaces instead of local-admin surfaces, and bad-origin audit records still preserve the transport-specific route family names. | `cargo test -p neovex-server firebase_enabled_routes_grpc_and_grpc_web_requests_to_firestore_service --lib`; `cargo test -p neovex-server firebase_routes_remain_application_surfaces_without_local_admin_auth --lib`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all` (required for the new transport test); `cargo fmt --all --check`. |
| 2026-04-25 | F2.1 tonic/prost bindings and service scaffold | `done` | Vendored the audited Firestore proto tree into `crates/neovex-server/proto/google/`, added a `build.rs` that runs `tonic-build` with generated default server stubs and vendored `protoc`, and introduced `crates/neovex-server/src/adapters/firebase/grpc/mod.rs` as the thin Firestore gRPC scaffold. The generated service now exposes the canonical `google.firestore.v1.Firestore` server name, compiles inside the server crate without premature router coupling, and returns canonical `UNIMPLEMENTED` responses for representative unary and server-streaming RPCs. Proto drift note: the repo now pins the copied Firestore proto inputs in git rather than depending on an external generator at build time. | `cargo test -p neovex-server firestore_grpc --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check`. |
| 2026-04-25 | F2.1 tonic/prost bindings and service scaffold | `in_progress` | Started the first gRPC slice after `F1` closed. The current server already classifies `/google.firestore.v1.Firestore/*` traffic for route-family security/CORS and has gRPC-Web header coverage, but there is no tonic service scaffold, no generated Firestore bindings, and no shared server-owned entrypoint for unimplemented RPCs yet. Next action: decide between a pinned pre-generated Firestore crate versus vendored `tonic-build`, then land the smallest audited scaffold that compiles and returns canonical `UNIMPLEMENTED` responses for the Firestore service methods. | Startup reconciliation of `git status --short`; inspection of the `F2.1` / risk notes and source-evidence references in `docs/plans/firebase-adapter-plan.md`; review of `crates/neovex-server/{Cargo.toml,src/lib.rs,src/router.rs,src/local_server/policy.rs,src/tests.rs}` plus the local Cargo cache for tonic/prost tooling. |
| 2026-04-25 | F1.7 REST error mapper | `done` | Closed `F1.6b` after adding shared compound-query index preflight in `crates/neovex-engine/src/service/queries/structured.rs`, which now rejects supported multi-field Firestore query shapes when the tenant schema lacks a matching index prefix instead of silently relying on full scans. Then finished the REST slice by replacing the ad-hoc Firebase error envelope in `crates/neovex-server/src/adapters/firebase/mod.rs` with a typed google RPC-style mapper, adding canonical status coverage for every core error class currently surfaced by the adapter, switching missing-index responses to `FAILED_PRECONDITION` with `google.rpc.PreconditionFailure` details, and updating focused server expectations accordingly. The `F1` phase is now complete; next item is `F2.1 tonic/prost bindings and service scaffold`. | `cargo test -p neovex-engine structured --lib`; `cargo test -p neovex-server firebase_run_query --lib`; `cargo check -p neovex-engine -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs in Firebase `runQuery` files); `cargo fmt --all`; `cargo fmt --all --check`; `make clippy`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check` (reported formatting diff in `crates/neovex-server/src/adapters/firebase/mod.rs`); `cargo fmt --all`; `cargo fmt --all --check`; `make clippy`. |
| 2026-04-25 | F1.6b RunQuery missing-index surfacing | `in_progress` | Closed `F1.6a` after wiring document-parent `.../documents/{document_path}:runQuery` handling through `crates/neovex-server/src/router.rs` and `crates/neovex-server/src/adapters/firebase/mod.rs`, reusing the shared Firestore resource parser to scope nested collection queries without storage-name delimiter tricks. Focused server coverage now proves the Firebase adapter serves both root and parent-document `RunQuery` routes. Next action: add a shared structured-query preflight that rejects supported Firestore compound queries when no matching schema index exists, then cover both the explicit error and the indexed success path before closing the split `F1.6` work. | `cargo test -p neovex-server firebase_run_query --lib`. |
| 2026-04-25 | F1.6a RunQuery routing and parent-path support | `in_progress` | Closed `F1.5`, then added adapter-local `RunQuery` request parsing in `crates/neovex-server/src/adapters/firebase/run_query_request.rs` and replaced the placeholder REST handler in `crates/neovex-server/src/adapters/firebase/mod.rs` with a live root `documents:runQuery` path for the currently supported structured-query subset. The adapter now decodes Firestore Proto3 JSON filter and cursor values into `neovex-core::StructuredQuery`, resolves the raw Firestore collection selector through the shared resource-name/path parser into a hashed storage table without leaking table-name tricks into the protocol layer, executes the shared engine structured-query path, and returns Firestore-style JSON-line query responses for matching documents or empty reads. Focused server coverage already exercises root-route `from`, `where`, `orderBy`, `startAt`, `offset`, `limit`, projection shaping, empty results, and unsupported operator errors. Split the original `F1.6` item because the remaining gap spans two different risks: `F1.6a` now owns document-parent `.../documents/{document_path}:runQuery` routing and collection-target resolution, while `F1.6b` will own shared missing-index surfacing once the transport slice is closed. Next action: add the document-parent route shape plus focused nested-collection coverage without coupling route parsing to storage naming. | `cargo test -p neovex-server run_query_request --lib`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs in `crates/neovex-server/src/adapters/firebase/{mod,run_query_request}.rs`); `cargo fmt --all`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check`. |
| 2026-04-25 | F1.5 BatchGetDocuments REST handler | `done` | Added adapter-local `BatchGetDocuments` request parsing in `crates/neovex-server/src/adapters/firebase/batch_get_request.rs` and replaced the placeholder REST handler in `crates/neovex-server/src/adapters/firebase/mod.rs` with live point-read execution over the shared path/resource primitives from `F0.2` and the transaction-session manager from `F0.5`. The handler now derives stable storage locators from `DocumentPath`, returns found/missing Firestore document envelopes as JSON-line responses, elides duplicate document names, applies top-level field masks, and validates transaction tokens or unsupported consistency selectors without introducing adapter-local lookup shims. Focused server tests now cover enabled-route behavior, found/missing responses, duplicate elision, nested document paths, active-versus-inactive transaction tokens, and explicit `readTime` error mapping. | `cargo test -p neovex-server batch_get_request --lib`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs in `crates/neovex-server/src/adapters/firebase/{batch_get_request,mod}.rs`); `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F1.5 BatchGetDocuments REST handler | `in_progress` | Closed `F1.4b` after replacing the Firebase `commit` preview path with live execution in `crates/neovex-server/src/adapters/firebase/mod.rs`. The handler now validates route/body database alignment, derives stable storage locators from shared Firestore `DocumentPath` values via hashed collection-path table ids, executes parsed `AtomicWriteBatch` requests through the engine mutation or transaction-session path, and returns Firestore-shaped `writeResults` / `commitTime` plus google RPC-style REST error envelopes. Focused HTTP tests in `crates/neovex-server/src/tests.rs` now cover successful commit responses, atomic rollback on failure, transaction token commits, malformed JSON, and enabled-route registration. Next action: add adapter-local `BatchGetDocuments` request parsing plus response serialization on top of the same deterministic path-to-locator helper and point-read/transaction-session engine paths, starting with found/missing/duplicate document coverage before deciding whether unsupported read selectors need an explicit error helper in this phase. | Checkpoint after `F1.4b` verification; no `F1.5` code landed yet in this row. |
| 2026-04-25 | F1.4b Commit execution and Firestore response mapping | `done` | Replaced the Firebase `commit` placeholder with a live REST handler in `crates/neovex-server/src/adapters/firebase/mod.rs` that parses Proto3 JSON commit bodies, validates route/body database identity, resolves `DocumentPath` values into bound write keys, executes the shared atomic write batch or transaction-session commit path, and returns Firestore-shaped `writeResults` / `commitTime` responses. Added focused server coverage for successful commits, atomic rollback on failure, transaction token handling, malformed JSON, and enabled-route registration, and removed the obsolete preview-only commit payload helper from `crates/neovex-server/src/adapters/firebase/commit_request.rs`. Next step: start `F1.5 BatchGetDocuments REST handler`. | `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs in `crates/neovex-server/src/adapters/firebase/mod.rs`); `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F1.4b Commit execution and Firestore response mapping | `in_progress` | Closed `F1.4a` after adding `crates/neovex-server/src/adapters/firebase/commit_request.rs`, which parses Firestore REST `Commit` JSON into shared `AtomicWriteBatch` primitives through an injected write-key resolver and validates update/delete/verify/transform oneof rules, masks, preconditions, transaction bytes, and transform operands against the shared serializer/resource parser seams. The live Firebase `commit` placeholder now accepts Firebase-style `text/plain` JSON bodies, returns `400` for malformed commit requests, and exposes the lowered batch preview on valid requests while still intentionally stopping at `501`. Next action: add a real engine-backed write-key resolver for `DocumentPath` -> `WriteKey::Bound`, then execute the lowered batch through the shared mutation/transaction path and map `AtomicWriteBatchOutcome` into Firestore `CommitResponse` plus REST status envelopes. | `cargo test -p neovex-server commit_request --lib`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs in `commit_request.rs` / `firebase/mod.rs`); `cargo fmt --all`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check`. |
| 2026-04-25 | F1.4a Commit request parser and batch translation | `done` | Added adapter-local Firestore `Commit` request models and lowering logic in `crates/neovex-server/src/adapters/firebase/commit_request.rs`, keeping the Firestore wire contract adapter-local while translating valid requests into shared `AtomicWriteBatch` primitives via an injected resolver. Supported writes now parse set/overwrite, patch masks, delete, verify, and transform recognition; invalid oneof shapes, database mismatches, malformed preconditions/transaction bytes, and unsupported transform operands fail explicitly before execution. Next step: start `F1.4b Commit execution and Firestore response mapping`. | `cargo test -p neovex-server commit_request --lib`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs in `commit_request.rs` / `firebase/mod.rs`); `cargo fmt --all`; `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check`. |
| 2026-04-25 | F1.4a Commit request parser and batch translation | `in_progress` | Closed `F1.3` after adding a Firebase resource-name parser in `crates/neovex-server/src/adapters/firebase/resource_names.rs` that maps Firestore database/document/parent resources into shared `DocumentPath` / `CollectionPath` primitives, rejects named databases, and decodes REST path segments exactly once. Split the old `F1.4 Commit REST handler` item into `F1.4a` request translation and `F1.4b` execution/response mapping because the full commit surface spans distinct parsing versus execution/error risks. Next action: inspect the Firestore `Write` / `CommitRequest` shapes and add adapter-local request types plus lowering into the shared atomic write batch primitives before touching live storage execution. | `cargo test -p neovex-server resource_names --lib`; `cargo check -p neovex-server`; `cargo test -p neovex-server firebase --lib`; `cargo fmt --all --check` (reported formatting diffs in `resource_names.rs`); `cargo fmt --all`; `cargo fmt --all --check`; `cargo check -p neovex-server`. |
| 2026-04-25 | F1.3 Firestore resource parser | `done` | Added `crates/neovex-server/src/adapters/firebase/resource_names.rs` with adapter-local parsers for Firestore database names, document names, parent resource names, collection targets, and REST percent-decoded path segments while keeping the parsed outputs on the shared `CollectionName` / `CollectionPath` / `DocumentPath` primitives from `F0.2`. The parser explicitly rejects named databases, malformed/trailing-slash resource names, and encoded slashes, while preserving dots, Unicode, and `__`-containing collection segments. Next step: start the split `F1.4a Commit request parser and batch translation` item. | `cargo test -p neovex-server resource_names --lib`; `cargo check -p neovex-server`; `cargo test -p neovex-server firebase --lib`; `cargo fmt --all --check` (reported formatting diffs in `resource_names.rs`); `cargo fmt --all`; `cargo fmt --all --check`; `cargo check -p neovex-server`. |
| 2026-04-25 | F1.3 Firestore resource parser | `in_progress` | Closed `F1.2` after adding an adapter-local Firestore Proto3 JSON serializer in `crates/neovex-server/src/adapters/firebase/serializer.rs` that roundtrips supported `Value` oneof cases, preserves explicit unsupported cases for Firestore-only types, and is now exercised by the live Firebase placeholder handlers instead of existing only in tests. Next action: inspect the Firestore resource-name contracts in the upstream proto/REST docs, then add a parser module that decodes project/database/document resource names into the shared `CollectionPath` / `DocumentPath` primitives from `F0.2`, rejecting named databases and malformed/trailing-slash paths explicitly. | `cargo test -p neovex-server firestore_value --lib`; `cargo check -p neovex-server`; `cargo test -p neovex-server firebase --lib`; `cargo fmt --all --check` (reported serializer formatting diffs); `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F1.2 Proto3 JSON serializer | `done` | Added an adapter-local Firestore Proto3 JSON serializer/deserializer in `crates/neovex-server/src/adapters/firebase/serializer.rs` with a typed `FirestoreValue` / `FirestoreDouble` model, explicit unsupported errors for Firestore-only or not-yet-supported value kinds, and roundtrip coverage for the supported wire types. The Firebase placeholder responses now expose the serializer contract through live code so the adapter surface already depends on the new conversion seam before `Commit`, `BatchGetDocuments`, or `RunQuery` parsing begins. Next step: start `F1.3 Firestore resource parser`. | `cargo test -p neovex-server firestore_value --lib`; `cargo check -p neovex-server`; `cargo test -p neovex-server firebase --lib`; `cargo fmt --all --check` (reported serializer formatting diffs); `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F1.2 Proto3 JSON serializer | `in_progress` | Closed `F1.1` after adding the thin Firebase adapter scaffold in `crates/neovex-server/src/adapters/firebase/`, optional Firebase registration in router/AppState/serve options, and config-gated database-level REST route skeletons that 404 when disabled and surface placeholder handlers when enabled. Next action: inspect the current Neovex document/value representation against Firestore `Value` Proto3 JSON rules, then add a focused serializer module with explicit unsupported cases and roundtrip coverage before request parsing or REST handlers depend on it. | `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check` (reported formatting diff in `tests.rs`); `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F1.1 Firebase module scaffold and router registration | `done` | Added `crates/neovex-server/src/adapters/firebase/mod.rs` with a minimal `FirebaseConfig` marker and placeholder database-level REST handlers, registered the module from `adapters/mod.rs`, and threaded optional Firebase configuration through `RouterBuildConfig`, `AppState`, and `ServeOptions` without copying Convex runtime internals. The router now merges the initial Firebase REST route skeleton only when Firebase is enabled, and focused tests prove the routes return `404` when disabled and `501` placeholder responses when enabled while preserving the earlier Firebase local-security/CORS coverage. Next step: start `F1.2 Proto3 JSON serializer`. | `cargo test -p neovex-server firebase --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check` (reported formatting diff in `tests.rs`); `cargo fmt --all`; `cargo fmt --all --check`. |
| 2026-04-25 | F1.1 Firebase module scaffold and router registration | `in_progress` | Started the first REST adapter slice after closing `F0`. The current server already has Firebase route-family security/CORS coverage, but no adapter module, no AppState registration, and no config-gated route tree. Next action: add a thin `adapters/firebase/` scaffold with `FirebaseConfig`, thread optional Firebase registration through `RouterBuildConfig`, `AppState`, and `ServeOptions`, register the initial REST route shapes only when enabled, and add focused router tests for disabled-versus-enabled behavior before serializer or request parsing work begins. | Startup reconciliation of `git status --short`; inspection of the active plan rows, `crates/neovex-server/src/{adapters/mod.rs,state.rs,router.rs,lib.rs}`, prior `F0.7` router/security diffs, and existing route/registry tests. |
| 2026-04-25 | F0.8 Convex adapter shared-logic audit | `done` | Audited the remaining Convex adapter seams after `F0.7` and did not find leftover shared Firestore-facing database primitives that still needed promotion. The only suspicious hotspots were documented explicitly in the `F0.8 Audit Notes`: Convex `get` / `first` / `unique` result shaping, runtime `paginate()` cursor synthesis over Convex JSON payloads, per-invocation runtime mutation execution units, and Convex subscription transform planning all remain intentionally adapter-specific on top of the shared core/engine/server seams landed in `F0.1` through `F0.7`. Added one code comment in `crates/neovex-server/src/adapters/convex/host_bridge/pagination.rs` to make that pagination boundary explicit. Next step: start `F1.1 Firebase module scaffold and router registration`. | `cargo test -p neovex-server query_shapes --lib`; `cargo fmt --all --check`. |
| 2026-04-25 | F0.8 Convex adapter shared-logic audit | `in_progress` | Closed `F0.7` after landing request-aware Firebase REST/gRPC/gRPC-Web/WebSocket route classification in the shared local-server policy layer, Firebase project/tenant extraction for audit metadata, and the widened Firestore REST / gRPC-Web CORS allowlist and trailer exposure in `neovex-server`. Focused server tests now cover Firebase REST `text/plain` preflight, gRPC-Web preflight and exposed gRPC trailers, route-family classification, Firebase application-surface routing under local-server security, and transport-specific origin audit records. Next action: inspect `crates/neovex-server/src/adapters/convex/` plus the shared engine/server seams it depends on, identify any remaining database semantics that should be promoted out of the Convex adapter before Firebase copies them, and either land the promotion or record the adapter-specific boundary explicitly in the plan notes. | `cargo test -p neovex-server firebase --lib`; `cargo test -p neovex-server route_family --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs); `cargo fmt --all`; `cargo fmt --all --check`; `make clippy`. |
| 2026-04-25 | F0.7 Firebase route family, CORS, middleware tests | `done` | Added request-aware Firebase transport classification to `crates/neovex-server/src/local_server/policy.rs` so the shared security layer can distinguish Firestore REST, gRPC, gRPC-Web, and the reserved Listen WebSocket family before route handlers exist. Promoted Firebase tenant extraction into shared audit middleware via `tenant_id_from_request(...)`, widened the global CORS contract in `crates/neovex-server/src/router.rs` for Firestore REST/gRPC-Web headers plus gRPC trailer exposure, and added focused server tests in `crates/neovex-server/src/tests.rs`, `tests/local_server_security.rs`, and `tests/local_audit.rs` covering preflight behavior, local-origin policy, application-surface routing, and transport-specific audit categorization. Next step: start `F0.8 Convex adapter shared-logic audit`. | `cargo test -p neovex-server firebase --lib`; `cargo test -p neovex-server route_family --lib`; `cargo check -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs); `cargo fmt --all`; `cargo fmt --all --check`; `make clippy`. |
| 2026-04-25 | F0.7 Firebase route family, CORS, middleware tests | `in_progress` | Resumed the next unblocked Firebase core hardening item after reconciling the dirty worktree and confirming no other roadmap row was active. The route-family and local-security seams are still path-only today, so this slice is widening them into request-aware Firebase REST/gRPC/gRPC-Web/WebSocket classification plus focused CORS/audit coverage without opening the actual `/v1/*` adapter routes yet. Next action: update the shared local-server classifier and tenant/audit extraction for Firebase request metadata, widen the global CORS header/exposed-trailer policy for Firestore REST and gRPC-Web, and add focused server tests for preflight, local-origin policy, and audit categorization. | Startup source review of `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`, `docs/plans/README.md`, `docs/plans/firebase-adapter-plan.md`, `docs/prompts/firebase-adapter-start.md`; `git status --short`; dirty-file inspection for the prior `F0.6` subscription/server changes and active plan file; inspection of `crates/neovex-server/src/router.rs`, `crates/neovex-server/src/local_server/{policy,middleware,audit}.rs`, `crates/neovex-server/src/tests{,.rs,/local_audit.rs,/local_server_security.rs}`, and the localhost security plan/source-evidence rows for `F0.7`. |
| 2026-04-24 | F0.7 Firebase route family, CORS, middleware tests | `pending` | `F0.6a` and `F0.6b` are complete, so the next unblocked core hardening item is the Firebase route-family and middleware test slice. Next action: inspect the existing local-server security route classification, CORS/header handling, gRPC-Web preflight coverage, and adapter registration seams in `crates/neovex-server` before deciding whether `F0.7` can land as one server-test pass or should be split into route-family classification versus middleware/preflight assertions. | Checkpoint only; no `F0.7` code or verification landed in this row. |
| 2026-04-24 | F0.6b Subscription diff helper and change classification | `done` | Added protocol-neutral snapshot diff types in `neovex-core` (`SubscriptionDocumentChangeKind`, `SubscriptionDocumentChange`, `SubscriptionSnapshotDiff`) plus `diff_subscription_snapshots(...)`, which deterministically emits removals in prior order and additions/modifications in current order while preserving old/new indices for ordering shifts. The helper operates only on successive `SubscriptionResultSnapshot`s, so Firebase Listen can classify added/modified/removed changes without pushing watch-target state into the engine. Next step: start `F0.7 Firebase route family, CORS, middleware tests`. | `cargo test -p neovex-core subscription --lib`; `cargo check -p neovex-core -p neovex-engine -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs); `cargo fmt --all`; `cargo fmt --all --check`; `make clippy`. |
| 2026-04-24 | F0.6b Subscription diff helper and change classification | `in_progress` | Closed `F0.6a` after landing a shared `SubscriptionResultSnapshot` envelope in `neovex-core`, threading it through engine delivery, and updating the server/Convex forwarding paths to serialize from full documents instead of transport-local JSON arrays. The next safe slice is a protocol-neutral diff helper over successive snapshots so future Firebase Listen routing can classify added/modified/removed documents and ordering shifts without keeping Firebase-specific watch state inside the engine. Next action: add shared snapshot-diff types and helpers in `neovex-core`, then cover added/modified/removed, empty, reordering, and consecutive-diff behavior with focused tests before touching adapter routing. | Checkpoint after `F0.6a` verification; no `F0.6b` code landed yet in this row. |
| 2026-04-24 | F0.6a Subscription snapshot envelope | `done` | Added a protocol-neutral `SubscriptionResultSnapshot` and `SubscriptionCommitMetadata` surface in `neovex-core`, threaded snapshot delivery through engine `SubscriptionUpdate`, and updated the plain WebSocket plus Convex subscription forwarders to serialize from full `Document` snapshots rather than engine-local JSON arrays. Bootstrap snapshots now preserve `covered_sequence`, mutation-driven snapshots expose stable commit sequence/time metadata, deleted-document hints stay outside user document fields, and runtime subscription bridging now distinguishes bootstrap by `request_id` so coalesced no-commit catch-up snapshots still forward correctly. Next step: start `F0.6b Subscription diff helper and change classification`. | `cargo test -p neovex-core subscription --lib`; `cargo test -p neovex-engine subscriptions --lib`; `cargo test -p neovex-server subscriptions --lib`; `cargo check -p neovex-core -p neovex-engine -p neovex-server`; `cargo fmt --all --check` (reported formatting diffs); `cargo fmt --all`; `cargo fmt --all --check`; `make clippy`. |
| 2026-04-24 | F0.6a Subscription snapshot envelope | `in_progress` | Split the original `F0.6 Subscription snapshot/diff support` item because the current gap spans two different ownership risks. The first safe slice is a protocol-neutral snapshot envelope in shared core/engine subscription output: full result documents, deleted-document hints, covered sequence, and commit metadata that adapters can consume without transport-local JSON scraping. The later diff/helper slice will need retained prior-snapshot state and change classification, so it now lives in `F0.6b`. Next action: add shared snapshot metadata types, thread them through engine `SubscriptionUpdate` delivery, and adapt existing Convex/WebSocket forwarders plus focused subscription tests. | Sizing review of `crates/neovex-engine/src/service/subscriptions.rs`, `crates/neovex-engine/src/subscriptions/{delivery,dependencies,queue,registry}.rs`, `crates/neovex-engine/src/tests/subscriptions/*.rs`, `crates/neovex-server/src/execution/subscriptions.rs`, `crates/neovex-server/src/ws/socket/transport.rs`, and Convex subscription forwarders after closing `F0.5`. |
| 2026-04-24 | F0.5 Transaction session manager | `done` | Added a protocol-neutral transaction session surface in `neovex-core` (`TransactionSessionToken`, `TransactionSessionMode`, `TransactionSession`) and an engine-owned bounded session registry in `neovex-engine::Service` so future Firebase RPCs can exchange opaque transaction tokens instead of storing raw `MutationExecutionUnit` values in the adapter. The new service lifecycle binds tokens to tenant + principal, enforces TTL expiry and cleanup, routes transactional point reads through the pinned execution unit, reuses the shared atomic write-batch path for transactional commit, consumes sessions on commit/rollback/mismatch, and preserves OCC conflict reporting for cross-RPC reads plus final commit. Query-session integration was intentionally left out of this item after sizing showed the current execution-unit query contract needs its own follow-on treatment before it can promise the same pinned-snapshot semantics as point reads. Next step: size and likely split `F0.6 Subscription snapshot/diff support` into a stable shared snapshot envelope slice versus a separate diff/helper adoption slice before changing subscription delivery code. | `cargo test -p neovex-core transaction --lib`; `cargo test -p neovex-engine transaction_session --lib`; `cargo check -p neovex-core -p neovex-engine`; `cargo fmt --all --check` (reported formatting diffs); `cargo fmt --all`; `cargo fmt --all --check`; `make clippy`. |
| 2026-04-24 | F0.5 Transaction session manager | `in_progress` | Closed `F0.4b2` after landing the richer structured-query execution slice. The next core gap is a protocol-neutral engine-owned transaction session manager that keeps opaque transaction tokens, pinned execution units, principal/tenant binding, TTL expiry, and cleanup out of the Firebase adapter so future `BeginTransaction` / transactional reads / `Commit` / `Rollback` RPCs reuse shared service state. Next action: add shared transaction token/mode types in `neovex-core`, then thread a bounded session registry through `neovex-engine::Service` with read, commit, rollback, expiry, and focused lifecycle tests. | Startup sizing review of `crates/neovex-engine/src/service/execution_units/`, `crates/neovex-engine/src/service/queries/`, `crates/neovex-engine/src/service/mutations/`, `crates/neovex-core/src/dependency.rs`, and existing server/session cleanup patterns after `F0.4b2` verification. |
| 2026-04-24 | F0.4b2 Engine richer ordering/cursor/projection adoption | `done` | Replaced the narrow structured-query lowering shim with a richer engine-owned execution path in `crates/neovex-engine/src/service/queries/structured.rs`. Structured queries now support repeated `order_by`, cursor bounds, offset, limit, and projection shaping on top of the existing authorized base-query path, while collection groups, composite/unary filters, raw collection-to-table mapping, nested field paths, document-ID sentinel behavior, and `find_nearest` still fail explicitly instead of being silently ignored. Added focused end-to-end query coverage in `crates/neovex-engine/src/tests/queries.rs` so the preserved legacy `Query` behavior and the new structured-query surface stay aligned. Next step: start `F0.5 Transaction session manager`. | `cargo test -p neovex-engine queries --lib`; `cargo fmt --all`; `cargo fmt --all --check`; `make clippy`. |
| 2026-04-24 | F0.4b2 Engine richer ordering/cursor/projection adoption | `in_progress` | Started the next structured-query engine slice after closing `F0.4b1`. This follow-on item now owns the broader evaluator work: repeated-order execution rather than simple rejection, structured cursor/offset lowering beyond the legacy opaque pagination cursor, and projection shaping without adapter-local masking. Next action: inspect `crates/neovex-engine/src/evaluator/ordering.rs`, `evaluator/pagination.rs`, `evaluator/cursor.rs`, and read-tracking/paginated-query seams together to decide whether tuple cursor encoding and repeated-order evaluation can land without destabilizing legacy `PaginatedQuery`. | Checkpoint-only sizing review after `F0.4b1` verification and `make clippy`; no new code landed for `F0.4b2` in this window. |
| 2026-04-24 | F0.4b1 Structured query lowering and unsupported-feature gate | `done` | Added an engine-owned structured-query lowering path in `crates/neovex-engine/src/service/queries/structured.rs` plus new `Service::query_documents_structured*` entrypoints. The engine now lowers the current supported single-source/single-order/field-filter/limit subset into the legacy planner `Query`, executes it through the existing read path, and fails fast with explicit `InvalidInput` errors for projections, collection groups, composite/unary filters, repeated ordering, cursor bounds, offsets, document-ID sentinel usage, raw collection-to-table mapping, and `find_nearest`. Next step: continue with `F0.4b2 Engine richer ordering/cursor/projection adoption`. | `cargo test -p neovex-engine structured --lib`; `cargo fmt --all --check`; `make clippy`. |
| 2026-04-24 | F0.4b1 Structured query lowering and unsupported-feature gate | `in_progress` | Closed `F0.4a` after landing the parser-facing `StructuredQuery` family in `neovex-core`, then split the original `F0.4b` item because the next gap spans two different risks: first, lowering the new AST into the legacy engine query path without silently dropping fields; second, teaching the planner/evaluator richer repeated-order/cursor/projection behavior. The next safe slice is a dedicated engine lowering path that executes the current single-source/single-order subset and explicitly rejects projections, collection groups, composite/unary filters, repeated ordering, cursor bounds, offsets, document-ID sentinels, and `find_nearest`. | Sizing review of `crates/neovex-engine/src/service/queries/`, `crates/neovex-engine/src/tests/queries.rs`, and the new `neovex-core` structured-query types after F0.4a verification. |
| 2026-04-24 | F0.4a Query AST surface expansion | `done` | Added a parser-facing structured-query AST beside the legacy planner query in `neovex-core`: `StructuredQuery`, `CollectionSelector`, `Projection`, `FieldReference`, structured field/composite/unary filters, repeated structured ordering, cursor bounds, and explicit `find_nearest` placeholders. The legacy `Query` stayed intact so `F0.4b` can adopt the widened surface through an explicit lowering step instead of a repo-wide query literal rewrite. Next step: start `F0.4b1 Structured query lowering and unsupported-feature gate`. | `cargo test -p neovex-core query --lib`; `cargo fmt --all` (to apply rustfmt changes in `crates/neovex-core/src/query.rs`); `cargo test -p neovex-core query --lib`; `cargo fmt --all --check`. |
| 2026-04-24 | F0.4a Query AST surface expansion | `in_progress` | Split the original `F0.4 Query AST hardening` item because the current gap spans both the core AST shape and the engine/planner’s unsupported-feature behavior. The next safe slice is widening `neovex-core::Query` into a parser-facing structured-query surface first, including source selection, projections, repeated ordering, cursor/offset/limit fields, and explicit placeholders for deferred features like `find_nearest`. Next action: inspect `crates/neovex-core/src/query.rs`, engine query-preparation/evaluator seams, and Firestore `StructuredQuery` fields side by side, then land the widened core AST plus focused serde/validation tests before touching planner behavior. | Item split checkpoint after F0.3 verification review; targeted sizing pass over `crates/neovex-core/src/query.rs`, `crates/neovex-engine/src/service/queries/`, and the F0.4 plan/source-evidence sections. |
| 2026-04-24 | F0.3 Atomic write batch primitive | `done` | Added a shared atomic write-batch surface in `neovex-core` (`AtomicWrite`, `AtomicWriteBatch`, `WriteKey`, `WritePrecondition`, `WriteSetMode`, transform modeling, and ordered batch outcomes) and implemented engine execution-unit support for set/patch/delete/verify semantics plus explicit transform rejection until F3.4. The redb execution-unit batch path now co-commits optional resource-path metadata with document/index writes so path bindings remain outside user fields and roll back with the same storage transaction on failure. Next step: start `F0.4a Query AST surface expansion`. | `cargo check -p neovex-core -p neovex-engine -p neovex-storage`; `cargo test -p neovex-core write_batch --lib`; `cargo test -p neovex-engine atomic_write_batch --lib`; `cargo test -p neovex-storage failed_batch_rolls_back_document_indexes_bindings_and_commit_log --lib`; `cargo fmt --all --check` (reported formatting diffs); `cargo fmt --all`; `cargo fmt --all --check`; `make clippy`. |
| 2026-04-24 | F0.3 Atomic write batch primitive | `in_progress` | Started the shared write-batch hardening pass after confirming F0.2 landed and no other roadmap item was active. Inspecting the existing `MutationExecutionUnit` staging/commit seams, persistence-provider batch application path, and Firestore `write.proto` / Firebase JS SDK mutation serialization so set/patch/delete/verify/transform semantics can stage through the engine-owned atomic commit path without a Firebase-only write path. | Startup source review of `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`, `docs/plans/README.md`, `docs/plans/firebase-adapter-plan.md`, `docs/prompts/firebase-adapter-start.md`; `git status --short`; dirty-file inspection for touched core/engine/storage/doc-plan files; targeted review of Firestore `write.proto`, Firebase JS SDK `serializer.ts`, `persistent_stream.ts`, and `mutation.ts`; inspection of `neovex-core` mutation/document/resource-path types plus engine execution-unit/persistence/storage write seams. |
| 2026-04-24 | F0.2 Resource path and collection group metadata model | `in_progress` | Started the shared resource-path hardening pass after confirming F0.1 landed and no other roadmap item was active. Inspecting `neovex-core` path/type boundaries, engine execution-unit/query seams, redb/SQL storage metadata ownership, and Firestore proto path/query sources before implementation. | Startup source review of `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`, `docs/plans/README.md`, `docs/plans/firebase-adapter-plan.md`, `docs/reference/convex-ai-guidelines.md`, `docs/reference/cli.md`, and `docs/convex/compatibility.md`; `git status --short`; dirty-file inspection for the touched core/engine/storage/doc-plan files; targeted review of Firestore `firestore.proto`, `query.proto`, and `document.proto`. |
| 2026-04-24 | F0.2 Resource path and collection group metadata model | `done` | Added a protocol-neutral raw path model in `neovex-core` (`CollectionName`, `CollectionPath`, `DocumentPath`, `DocumentLocator`, `ResourcePathBinding`) and kept `TableName` as the logical storage/schema identifier. Added redb-side sidecar binding tables plus collection-group scan keys so full Firestore document paths, nested subcollections, and collection-group lookup metadata are stored without `parent__child` naming or `_parent_id`-style user fields. Next step: start `F0.3 Atomic write batch primitive` on top of the new locator/path binding model. | `cargo test -p neovex-core resource_path --lib`; `cargo test -p neovex-storage store::resource_paths --lib`; `cargo check -p neovex-core -p neovex-storage`; `cargo fmt --all --check`; `make clippy`. |
| 2026-04-24 | F0.1 Document-key design and implementation | `in_progress` | Started the shared document-key hardening pass after reconciling the dirty control-plane worktree. Inspecting `neovex-core` document identity, engine mutation/execution-unit inserts, storage key encoding, and Convex adapter call sites before implementation. | Startup source review; `git status --short`; dirty-file inspection (`AGENTS.md`, `docs/plans/README.md`, `docs/plans/firebase-adapter-plan.md`, `docs/prompts/firebase-adapter-review.md`, `docs/prompts/firebase-adapter-start.md`). |
| 2026-04-24 | F0.1 Document-key design and implementation | `done` | Widened `DocumentId` to validated caller-provided string keys while preserving generated ULID-backed IDs, threaded optional insert IDs through the shared engine mutation path and Convex runtime/native service entrypoints, updated storage/index/scheduler key encoding for variable-length IDs, and added focused test/bench fallout fixes for non-`Copy` document IDs. Next step: start `F0.2 Resource path and collection group metadata model`. | `cargo test -p neovex-core mutation_insert_rejects_invalid_document_key_during_deserialization --lib`; `cargo test -p neovex-storage firestore_style --lib`; `cargo test -p neovex-engine service_insert_document_with_explicit_id_round_trips_firestore_style_key --lib`; `cargo test -p neovex-server convex_named_mutation_can_use_runtime_only_handler --lib`; `cargo fmt --all --check`; `make clippy`. |
| 2026-04-24 | Control-plane conversion | `done` | Converted the Firebase adapter plan to active status-ledger execution and added `AGENTS.md` / `docs/plans/README.md` routing. | `git diff --check`; `rg` status/reference consistency checks. |
| 2026-04-24 | Plan refresh after F0.1 audit | `done` | Reconciled the control-plane plan with the landed F0.1 code: updated the assessed state and source evidence map for string-backed `DocumentId`, clarified that F0.2 now owns the remaining collection/path identity gap and the `TableName` boundary, tightened write/query/stream contracts, expanded Firebase transport header/body requirements, and refreshed the next-agent start prompt to target `F0.2`. | Docs-only manual review of `docs/plans/firebase-adapter-plan.md` and `docs/prompts/firebase-adapter-start.md`; `rg` consistency checks against `crates/neovex-core/src/types.rs`, `crates/neovex-engine/src/service/execution_units/staging.rs`, Firebase proto files, and Firebase SDK transport sources. |

## Source Evidence Map

### Local Neovex Source

- `crates/neovex-core/src/types.rs:55-91` and
  `crates/neovex-core/src/resource_path.rs:7-55` - `TableName` intentionally
  remains the logical storage/schema identifier while `CollectionName` owns raw
  slash-free Firestore collection segments that do not satisfy
  `validate_logical_name`.
- `crates/neovex-core/src/resource_path.rs:58-145` and
  `crates/neovex-core/src/resource_path.rs:164-283` - `CollectionPath`,
  `DocumentPath`, `DocumentLocator`, and `ResourcePathBinding` model full
  ancestor paths, collection groups, and storage lookup metadata without
  delimiter-based table encoding.
- `crates/neovex-core/src/types.rs:100-149` and
  `crates/neovex-core/src/types.rs:210-232` - `DocumentId` is now a validated
  string-backed leaf key with generated ULID defaults plus a 1500-byte,
  slash-free, NUL-free validation boundary.
- `crates/neovex-storage/src/keys.rs:20-48` and
  `crates/neovex-storage/src/keys.rs:67-84` - resource-path storage keys use
  explicit length-prefixed encodings for locators, full document paths, and
  collection-group prefixes instead of lossy delimiter tricks.
- `crates/neovex-storage/src/store/resource_paths.rs:16-52` and
  `crates/neovex-storage/src/store/resource_paths.rs:55-267` - redb now stores
  sidecar path bindings keyed by internal document locator, plus reverse
  document-path lookup and collection-group scan bindings that future CRUD and
  query work can reuse.
- `crates/neovex-core/src/mutation.rs:12-26` - current mutation enum only has
  insert/update/delete and no explicit-key set/verify/transform operation.
- `crates/neovex-core/src/query.rs:6-13` and
  `crates/neovex-core/src/query.rs:43-53` - current query model is single-table
  with one optional order, one limit, and basic comparison filters.
- `crates/neovex-engine/src/service/execution_units/staging.rs:18-52` and
  `crates/neovex-engine/src/service/execution_units/staging.rs:55-112` -
  execution-unit staging now accepts explicit insert IDs, but update/delete
  still require existing documents and do not yet model Firestore overwrite or
  delete-missing semantics.
- `crates/neovex-engine/src/service/execution_units/staging.rs:156-188` -
  staged writes already coalesce ordered mixed writes and are the right
  foundation for a protocol-neutral batch layer.
- `crates/neovex-engine/src/service/mutations/direct/api.rs:18-205` - shared
  service mutation APIs now thread optional insert IDs through the normal engine
  mutation path instead of using adapter-local shims.
- `crates/neovex-engine/src/service/execution_units/commit.rs:20-65` -
  execution-unit commit owns atomic apply, conflict checks, cache invalidation,
  and subscription processing.
- `crates/neovex-server/src/adapters/convex/host_bridge/bridge.rs:57-138` -
  Convex creates and commits execution units per mutation invocation, not as a
  cross-RPC transaction store.
- `crates/neovex-engine/src/service/subscriptions.rs:189-207` and
  `crates/neovex-engine/src/subscriptions/delivery.rs:15-30` - current
  subscription API emits result snapshots and commit metadata, not Firestore
  target-change state.
- `crates/neovex-server/src/router.rs:46-174` - current router build pattern
  and Convex merge point.
- `crates/neovex-server/src/router.rs:296-314` - current CORS header allowlist,
  which must be expanded for Firebase REST/gRPC-Web.
- `crates/neovex-server/src/state.rs:18-58` and
  `crates/neovex-server/src/state.rs:79-134` - current AppState and
  ActiveConvexRegistry pattern.
- `crates/neovex-server/src/local_server/policy.rs:5-73` - current local route
  family classifier has Convex families but no Firebase family.
- `crates/neovex-server/src/adapters/convex/` - comparison point for adapter
  cleanup; shared database semantics found here should be promoted before
  Firebase duplicates them.

### Local Firebase SDK And Proto Source

Local checkout: `~/src/github.com/firebase/firebase-js-sdk`.

- `packages/firestore/src/protos/google/firestore/v1/firestore.proto:98-123`
  - REST bindings for `BatchGetDocuments`, `BeginTransaction`, `Commit`, and
  `Rollback`.
- `packages/firestore/src/protos/google/firestore/v1/firestore.proto:134-143`
  - `RunQuery` root and nested-parent REST bindings.
- `packages/firestore/src/protos/google/firestore/v1/firestore.proto:204-220`
  - `Write` and `Listen` are streaming methods and proto comments note
  gRPC/WebChannel availability rather than normal REST use.
- `packages/firestore/src/protos/google/firestore/v1/firestore.proto:236-250`
  - `BatchWrite` is non-atomic with per-write status semantics.
- `packages/firestore/src/protos/google/firestore/v1/firestore.proto:527-540`
  - `Commit` writes are atomic and ordered.
- `packages/firestore/src/protos/google/firestore/v1/document.proto:83-165`
  - Firestore `Value` oneof, precision/size constraints, and non-writeable
  pipeline/function expression values.
- `packages/firestore/src/protos/google/firestore/v1/query.proto:32-42` and
  `packages/firestore/src/protos/google/firestore/v1/query.proto:333-438` -
  `StructuredQuery` execution order and fields including `offset`, `limit`,
  and `find_nearest`.
- `packages/firestore/src/protos/google/firestore/v1/write.proto:33-178` -
  `Write`, `update_mask`, preconditions, and all field transform forms.
- `packages/firestore/src/remote/rest_connection.ts:98-183` and
  `packages/firestore/src/remote/rest_connection.ts:197-207` - Firebase JS SDK
  REST header/body contract, including `Content-Type: text/plain`,
  `X-Firebase-GMPID`, resource-prefix headers, auth/app-check propagation, and
  URL construction.
- `packages/firestore/src/platform/node/grpc_connection.ts:43-76` - Node SDK
  gRPC metadata headers and database path handling.
- `packages/firestore/src/api/credentials.ts:484-491` - App Check uses the
  `x-firebase-appcheck` header.
- `packages/firestore/src/remote/remote_store.ts:660-780` -
  full SDK write pipeline starts and feeds the persistent write stream.
- `packages/firestore/src/remote/persistent_stream.ts:650-704` - Listen stream
  watch/unwatch messages.
- `packages/firestore/src/remote/persistent_stream.ts:725-885` - Write stream
  handshake, stream token, and mutation request behavior.
- `packages/firestore/src/remote/serializer.ts:654-670` - Listen read time
  only advances a consistent snapshot when it applies to all targets.
- `packages/firestore/src/remote/serializer.ts:673-710` - mutation
  serialization to update/delete/patch/verify plus transforms/preconditions.
- `packages/firestore/src/remote/serializer.ts:909-1022` - query target and
  aggregation request serialization.
- `packages/firestore/src/core/transaction.ts:63-115` - JS transactions read
  via BatchGet and commit mutations plus verify operations.
- `packages/firestore/test/integration/api/` - upstream API integration test
  corpus used as a tiered compatibility score.

## References

### Firebase / Firestore Protocol

- [Firestore v1 gRPC service definition](https://cloud.google.com/firestore/docs/reference/rpc/google.firestore.v1)
- [Firestore v1 REST API](https://cloud.google.com/firestore/docs/reference/rest)
- [`google/firestore/v1/firestore.proto`](https://github.com/googleapis/googleapis/blob/master/google/firestore/v1/firestore.proto)
- [`google/firestore/v1/document.proto`](https://github.com/googleapis/googleapis/blob/master/google/firestore/v1/document.proto)
- [`google/firestore/v1/query.proto`](https://github.com/googleapis/googleapis/blob/master/google/firestore/v1/query.proto)
- [`google/firestore/v1/write.proto`](https://github.com/googleapis/googleapis/blob/master/google/firestore/v1/write.proto)
- [Firestore data model](https://firebase.google.com/docs/firestore/data-model)
- [Firestore transactions](https://firebase.google.com/docs/firestore/manage-data/transactions)
- [Firestore field transforms](https://firebase.google.com/docs/firestore/manage-data/add-data#server_timestamp)
- [tonic-web limitations](https://docs.rs/tonic-web/latest/tonic_web/#limitations)
- [`googleapis-tonic-google-firestore-v1` docs.rs](https://docs.rs/googleapis-tonic-google-firestore-v1/latest/googleapis_tonic_google_firestore_v1/google/firestore/v1/index.html)
- [`bouzuya/googleapis-tonic`](https://github.com/bouzuya/googleapis-tonic)

### Firebase JS SDK Source

Local checkout: `~/src/github.com/firebase/firebase-js-sdk`.

- `packages/firestore/src/remote/rest_connection.ts` - REST RPC mapping and headers.
- `packages/firestore/src/platform/node/grpc_connection.ts` - Node gRPC transport.
- `packages/firestore/src/remote/persistent_stream.ts` - Listen and Write streams.
- `packages/firestore/src/remote/remote_store.ts` - write pipeline and watch handling.
- `packages/firestore/src/remote/serializer.ts` - values, mutations, query targets.
- `packages/firestore/src/core/transaction.ts` - JS transaction behavior.
- `packages/firestore/test/integration/api/` - upstream integration tests.

### Neovex References

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/reference/convex-ai-guidelines.md`
- `docs/reference/cli.md`
- `docs/convex/compatibility.md`
- `crates/neovex-core/src/types.rs`
- `crates/neovex-core/src/mutation.rs`
- `crates/neovex-core/src/query.rs`
- `crates/neovex-engine/src/service/execution_units/`
- `crates/neovex-engine/src/service/subscriptions.rs`
- `crates/neovex-server/src/router.rs`
- `crates/neovex-server/src/state.rs`
- `crates/neovex-server/src/local_server/policy.rs`
- `crates/neovex-server/src/adapters/convex/`
