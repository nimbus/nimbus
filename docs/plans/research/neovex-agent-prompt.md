# Nimbus: Agent Prompt

## Mission

Build **nimbus**, a single-binary reactive document database in Rust.

This prompt is the **phase 1 implementation plan**. It should be read alongside:

- `docs/reactive-database-research-guide.md` for the long-term architecture
- `docs/horizontal-scaling-architecture-spec.md.pdf` for the long-term scaling
model

Those research docs are the north star. This file is the deliberately narrower
execution plan that maximizes the chance of shipping a working v1 in one shot.

This first pass is **not** "Convex feature parity." It is a tightly scoped,
one-shot vertical slice that proves the architecture works:

1. A tenant can be created.
2. A WebSocket client can subscribe to a query for one table.
3. An HTTP request can insert a document into that same tenant.
4. The server re-evaluates the query after commit.
5. The WebSocket client automatically receives the updated results.

If that loop works end-to-end under automated test, this pass is successful.

## One-Shot Success Criteria

The final deliverable must include:

1. A Rust workspace that compiles on stable Rust.
2. A runnable binary: `cargo run -p nimbus-bin -- serve --port 8080 --data-dir ./data`
3. A passing end-to-end integration test proving:
  `subscribe -> HTTP insert -> automatic WebSocket push`
4. Clean crate boundaries and testable modules, without over-abstracting v1.

## Non-Negotiable Scope

### Build in this pass

- Single binary
- Rust
- `redb` for embedded storage
- One redb database file per tenant
- Schemaless JSON-like documents
- Single-table queries only
- HTTP mutations
- WebSocket subscriptions
- Table-level invalidation only
- Full-result pushes on change
- Persistent commit log per tenant

### Do not build in this pass

- Schema parser or schema validation
- Secondary indexes
- Query planner crate
- Full-text search
- Vector search
- File storage
- Cron / job scheduling
- Authentication beyond explicit tenant identification
- Multi-node routing
- Edge replicas
- WASM/plugin runtime
- Differential result patches
- WebSocket mutations
- A generic pluggable storage abstraction

The fastest path to a working v1 is a concrete `redb` implementation with a
thin engine layer above it.

## Preflight Clarifications

These clarifications exist to remove ambiguity before code generation.

### Rust baseline

- Use stable Rust.
- Use Rust edition `2024` for all workspace crates unless a dependency forces a
different choice.

### Scope priority

- The only absolutely mandatory mutation for the first green end-to-end test is
`Insert`.
- Define `Update` and `Delete` types in `nimbus-core`, but do not let them
delay the first green path.
- If needed, it is acceptable to fully wire only `Insert` first, then add
`Update` / `Delete` after the reactive loop test is already passing.

### Tenant lifecycle semantics

- Tenants must be created explicitly via `POST /api/tenants`.
- Read, write, query, and WebSocket subscription requests for a missing tenant
must return `404` rather than implicitly creating the tenant.
- The engine may lazily **open** an existing tenant database file into memory,
but must not lazily **create** a tenant on access.

### Deterministic result ordering

- Query results must be deterministic for testability.
- If `query.order` is present, sort by that field and use `DocumentId` as the
tie-breaker.
- If `query.order` is absent, sort by `DocumentId` ascending.
- Missing fields should evaluate as non-matching for filters.

### System field encoding

- `DocumentId` should serialize externally as a ULID string.
- `_creationTime` should serialize as milliseconds since Unix epoch.
- HTTP and WebSocket payloads should expose `_id` and `_creationTime` in result
documents.

### Subscription semantics

- A successful `subscribe` request must send an initial
`subscription_result` message immediately.
- Later reactive pushes use the same message shape.
- The initial response should include the caller's `request_id`.
- Subsequent reactive pushes should omit `request_id`.

### API sharp edges

- Do not use `GET` with a JSON request body anywhere.
- Keep request and response payloads simple, explicit, and stable.
- Prefer `404` for missing tenant/document, `400` for malformed input, and
`500` only for true internal errors.

## Reference Repos Reviewed

Use these as implementation references when helpful, but do **not** copy their
complexity into nimbus v1.

### `redb`

Path:
`/Users/jack/src/github.com/cberner/redb`

Takeaways:

- `redb` is the right fit for v1 because it gives us a single writer with many
concurrent readers and serializable isolation.
- The `InMemoryBackend` in `tests/basic_tests.rs` is ideal for fast unit tests.
- A single `documents` table plus prefix-scanned keys is simpler than trying to
invent dynamic redb table definitions for user tables in v1.

Files consulted:

- `README.md`
- `docs/design.md`
- `examples/multithread.rs`
- `tests/basic_tests.rs`

### `mini-redis`

Path:
`/Users/jack/src/github.com/tokio-rs/mini-redis`

Takeaways:

- The server should keep transport concerns thin and push logic into shared
state / service code.
- For WebSockets, use split send/receive tasks and an outbound channel instead
of trying to write to the socket from many places.
- Integration tests should bind to `127.0.0.1:0`, spawn the server in-process,
and then use real clients against the live port.

Files consulted:

- `src/server.rs`
- `src/db.rs`
- `src/bin/server.rs`
- `tests/server.rs`

### `convex-backend`

Path:
`/Users/jack/src/github.com/get-convex/convex-backend`

Takeaways:

- Keep routing / transport separate from the runtime / service layer.
- The WebSocket sync path should have explicit message receive and message send
loops rather than ad hoc socket access from deep business logic.
- A single deployable backend binary is a reasonable target, but nimbus v1
must stay dramatically smaller than Convex's production architecture.

Files consulted:

- `README.md`
- `crates/local_backend/src/router.rs`
- `crates/local_backend/src/subs/mod.rs`
- `self-hosted/advanced/running_binary_directly.md`

### How to use references

- Use references to borrow patterns, not to import architecture wholesale.
- Do not try to mirror Convex's crate graph, protocol surface, or product
breadth.
- Do not read whole repositories unless blocked; use the local file reading
order later in this prompt.

## Additional GitHub References To Use Intentionally

These are useful references, but they should be consulted **selectively**.

### Local-first rule

Prefer the local checkout paths below over remote browsing. The implementation
agent should read from local disk first because it is faster, stable for the
session, and avoids unnecessary network work. Use the GitHub URL only as
fallback provenance if the local checkout is missing or clearly stale.

### Use during phase 1 implementation

#### `tokio-rs/axum`

Local path:
`/Users/jack/src/github.com/tokio-rs/axum`

URL:
[https://github.com/tokio-rs/axum](https://github.com/tokio-rs/axum)

Use for:

- router organization
- handler/state extraction patterns
- WebSocket upgrade handling
- integration test server setup

Why:

- Nimbus v1 is built around Axum, so the official repo and examples are the
right primary source for HTTP/WebSocket idioms.

#### `snapview/tokio-tungstenite`

Local path:
`/Users/jack/src/github.com/snapview/tokio-tungstenite`

URL:
[https://github.com/snapview/tokio-tungstenite](https://github.com/snapview/tokio-tungstenite)

Use for:

- WebSocket client examples in tests
- connection setup and framing expectations in the integration harness

Why:

- The Nimbus end-to-end test should use a real WebSocket client, and this repo
is the canonical Rust reference for that layer.

### Use for phase 2 or later, not for the first vertical slice

#### `tokio-rs/turmoil`

Local path:
`/Users/jack/src/github.com/tokio-rs/turmoil`

URL:
[https://github.com/tokio-rs/turmoil](https://github.com/tokio-rs/turmoil)

Use for:

- deterministic simulation tests
- network fault injection
- clock/control over async tests

Why:

- Our research guide points toward simulation-heavy testing long term, but this
is not part of the one-shot phase-1 build.

#### `TimelyDataflow/differential-dataflow`

Local path:
`/Users/jack/src/github.com/TimelyDataflow/differential-dataflow`

URL:
[https://github.com/TimelyDataflow/differential-dataflow](https://github.com/TimelyDataflow/differential-dataflow)

Use for:

- future incremental view maintenance design
- understanding delta-based query updates

Why:

- Valuable for hot-path incremental reactivity later, but much too ambitious to
use as a direct implementation dependency in v1.

#### `mit-pdos/noria`

Local path:
`/Users/jack/src/github.com/mit-pdos/noria`

URL:
[https://github.com/mit-pdos/noria](https://github.com/mit-pdos/noria)

Use for:

- studying partially-stateful query maintenance
- future read-model/materialization architecture

Why:

- High-signal for architecture, low-signal for direct phase-1 implementation.

#### `electric-sql/electric`

Local path:
`/Users/jack/src/github.com/electric-sql/electric`

URL:
[https://github.com/electric-sql/electric](https://github.com/electric-sql/electric)

Use for:

- future shape/partial-replication ideas
- HTTP sync API design
- edge/client replication thinking

Why:

- Highly relevant to later sync/replication work, but outside the first
server-authoritative Nimbus slice.

#### `quickwit-oss/tantivy`

Local path:
`/Users/jack/src/github.com/quickwit-oss/tantivy`

URL:
[https://github.com/quickwit-oss/tantivy](https://github.com/quickwit-oss/tantivy)

Use for:

- future full-text search integration
- index lifecycle and search API patterns

Why:

- Best future reference for Nimbus search, but search is explicitly out of
scope for phase 1.

#### `databendlabs/openraft`

Local path:
`/Users/jack/src/github.com/databendlabs/openraft`

URL:
[https://github.com/databendlabs/openraft](https://github.com/databendlabs/openraft)

Use for:

- future consensus/log replication
- multi-node tenant placement or metadata coordination

Why:

- Matches the long-term scaling direction, but should not influence the first
single-node binary beyond preserving clean seams.

### Reference priority rule

During phase 1 implementation, the agent should prioritize references in this
order:

1. `redb`
2. `mini-redis`
3. `axum`
4. `tokio-tungstenite`
5. `convex-backend`

When useful, read them from these local paths in the same order:

1. `/Users/jack/src/github.com/cberner/redb`
2. `/Users/jack/src/github.com/tokio-rs/mini-redis`
3. `/Users/jack/src/github.com/tokio-rs/axum`
4. `/Users/jack/src/github.com/snapview/tokio-tungstenite`
5. `/Users/jack/src/github.com/get-convex/convex-backend`

Only consult the later-phase repos if the task explicitly moves into
simulation, search, sync replication, or clustering work.

### Phase 1 local file reading order

If the implementation agent needs examples, inspect these files first and only
go deeper if blocked:

1. `redb`

- `/Users/jack/src/github.com/cberner/redb/README.md`
- `/Users/jack/src/github.com/cberner/redb/docs/design.md`
- `/Users/jack/src/github.com/cberner/redb/tests/basic_tests.rs`

2. `mini-redis`

- `/Users/jack/src/github.com/tokio-rs/mini-redis/src/server.rs`
- `/Users/jack/src/github.com/tokio-rs/mini-redis/src/db.rs`
- `/Users/jack/src/github.com/tokio-rs/mini-redis/tests/server.rs`

3. `axum`

- `/Users/jack/src/github.com/tokio-rs/axum/examples/websockets/src/main.rs`
- `/Users/jack/src/github.com/tokio-rs/axum/examples/testing-websockets/src/main.rs`
- `/Users/jack/src/github.com/tokio-rs/axum/axum/src/extract/ws.rs`
- `/Users/jack/src/github.com/tokio-rs/axum/axum/src/docs/routing/with_state.md`

4. `tokio-tungstenite`

- `/Users/jack/src/github.com/snapview/tokio-tungstenite/README.md`
- `/Users/jack/src/github.com/snapview/tokio-tungstenite/examples/client.rs`
- `/Users/jack/src/github.com/snapview/tokio-tungstenite/tests/communication.rs`

5. `convex-backend`

- `/Users/jack/src/github.com/get-convex/convex-backend/README.md`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/local_backend/src/router.rs`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/local_backend/src/subs/mod.rs`

## Architectural Decisions

- **Language:** Rust
- **Storage:** `redb`
- **Tenant model:** database-per-tenant
- **Document model:** schemaless documents stored as MessagePack bytes
- **Reactive model:** table-level invalidation only
- **Network API:** HTTP for writes, WebSocket for subscriptions
- **Query model:** single-table scan with filter/order/limit
- **Commit model:** write transaction returns a `CommitEntry`; subscriptions are
re-evaluated only after commit succeeds

## Alignment With The Research Docs

The broader research docs recommend several long-term directions. For v1, use
them as guidance, but keep implementation scope intentionally smaller.

### Choices that already align

- **Embedded storage:** The research guide recommends `redb` as the most
practical Rust-first starting point. This plan uses `redb`.
- **Reactive engine:** The research guide recommends full query
re-evaluation first and incremental maintenance later. This plan uses full
re-evaluation.
- **Scaling model:** The scaling spec recommends database-per-tenant and an
append-only commit log. This plan uses both.
- **Transport split:** The research guide points toward a server-authoritative
sync model first. This plan uses HTTP writes and WebSocket subscription
pushes.

### Intentional short-term simplifications

- **No OCC read-set tracking yet:** The research guide highlights OCC plus
fine-grained dependency tracking as a strong long-term model. V1 uses
`redb`'s single-writer serializable transactions plus table-level
invalidation because it is much easier to land correctly in one shot.
- **No simulator-first architecture yet:** The research guide strongly argues
for deterministic simulation and swappable I/O. That is the right long-term
direction, but not the right first implementation target for this repo. V1
should still keep clean crate boundaries so clock/network/storage seams can be
introduced later without rewriting everything.
- **No schema-derived API yet:** The research guide points toward Hasura-style
schema-driven APIs plus WASM extensions. V1 stays schemaless and hand-written
at the transport layer.

### Future seams to preserve

Even though v1 is narrower, structure the code so these upgrades remain
straightforward later:

- replace table-level invalidation with read-set / range dependency tracking
- add secondary indexes without rewriting the server layer
- swap the query evaluator from full scans to planned/indexed execution
- stream commit log entries to replicas or edge readers
- introduce injectable clock/id/network traits for simulation-heavy testing

## Recommended Workspace Layout

Keep the workspace small. Five crates is enough for a clean v1.

```text
nimbus/
├── Cargo.toml
├── README.md
├── crates/
│   ├── nimbus-core/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── document.rs
│   │       ├── error.rs
│   │       ├── mutation.rs
│   │       ├── query.rs
│   │       └── types.rs
│   │
│   ├── nimbus-storage/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── commit_log.rs
│   │       ├── keys.rs
│   │       ├── store.rs
│   │       └── tests.rs
│   │
│   ├── nimbus-engine/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── evaluator.rs
│   │       ├── service.rs
│   │       ├── subscriptions.rs
│   │       ├── tenant.rs
│   │       └── tests.rs
│   │
│   ├── nimbus-server/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── http.rs
│   │       ├── protocol.rs
│   │       ├── state.rs
│   │       └── ws.rs
│   │
│   └── nimbus-bin/
│       └── src/
│           └── main.rs
│
└── tests/
    └── reactive_loop.rs
```

## Dependency Graph

```text
nimbus-bin
  └── nimbus-server
        ├── nimbus-engine
        │     ├── nimbus-storage
        │     └── nimbus-core
        └── nimbus-core
```

Important:

- `nimbus-engine` owns tenant lifecycle, query evaluation, mutation
application, and subscription fanout.
- `nimbus-server` should be thin glue around the engine.
- Do **not** create a separate tenant crate and a separate query crate in v1.
That split increases coordination cost and makes one-shot success less likely.

## Core Domain Model

### `nimbus-core`

This crate contains only pure types and errors. No network code. No `redb`
types. No Tokio types.

Implement:

- `TenantId`
- `DocumentId` backed by `Ulid`
- `TableName`
- `SequenceNumber`
- `Timestamp`
- `Document`
- `Query`
- `Filter`
- `OrderBy`
- `Mutation`
- `CommitEntry`
- `WriteOp`
- `Error`

### Document shape

Use a simple document type:

```rust
pub struct Document {
    pub id: DocumentId,
    pub table: TableName,
    pub creation_time: Timestamp,
    pub fields: serde_json::Map<String, serde_json::Value>,
}
```

Store system fields separately in Rust, but include `_id` and `_creationTime`
in JSON responses.

### Query shape

Single-table only:

- `table`
- `filters`
- `order`
- `limit`

Support only what is required for v1:

- equality filters
- simple comparison filters on numbers and strings
- ordering on numbers and strings
- optional limit

If a filter or ordering value is unsupported, return a normal user error rather
than trying to be clever.

### Mutation shape

Keep mutations small and explicit:

```rust
pub enum Mutation {
    Insert { table: TableName, fields: Map<String, Value> },
    Update { table: TableName, id: DocumentId, patch: Map<String, Value> },
    Delete { table: TableName, id: DocumentId },
}
```

`Insert` is mandatory for acceptance. `Update` and `Delete` should be included
if feasible, but the build order should get `Insert` working first.

## Storage Design

### `nimbus-storage`

This crate owns all `redb` interaction. Do not add a generic storage trait in
v1. Use a concrete store type and keep the API small.

Primary type:

```rust
pub struct TenantStore {
    // owns/open the redb database for one tenant
}
```

### redb tables

Use exactly these logical tables:

- `documents: &[u8] -> &[u8]`
- `commit_log: u64 -> &[u8]`
- `metadata: &str -> &[u8]`

### Key format

Document keys must be prefix-scannable:

```text
{table_name_utf8}\0{doc_id_16_bytes}
```

Put the helpers in `keys.rs`:

- `document_key(table, id) -> Vec<u8>`
- `table_prefix(table) -> Vec<u8>`
- `prefix_end(prefix) -> Option<Vec<u8>>`

`scan_table(table)` must use these helpers and be covered by unit tests before
the engine layer is written.

### Store API

Implement a small, concrete API:

- `open(path) -> TenantStore`
- `create_in_memory() -> TenantStore` for tests
- `insert(document) -> CommitEntry`
- `update(table, id, patch) -> CommitEntry`
- `delete(table, id) -> CommitEntry`
- `get(table, id) -> Option<Document>`
- `scan_table(table) -> Vec<Document>`
- `read_commit_log_from(sequence) -> Vec<CommitEntry>`
- `latest_sequence() -> SequenceNumber`

Sequence numbers should start at `1` for the first committed write.

Mutation methods must:

1. Open a write transaction.
2. Modify document state.
3. Append a `CommitEntry`.
4. Commit.
5. Return the committed `CommitEntry`.

Do not notify subscriptions from the storage crate.

### Testing guidance for storage

Use `redb::backends::InMemoryBackend` for fast unit tests. Use `tempfile` for a
small number of filesystem tests that prove reopening from disk works.

## Engine Design

### `nimbus-engine`

This crate is the heart of the v1 system. It owns:

- tenant registry
- query evaluation
- mutation orchestration
- subscription registration
- post-commit invalidation and fanout

### Main public type

Expose a single service object:

```rust
pub struct Service {
    // tenant registry and shared runtime state
}
```

This service should provide methods like:

- `create_tenant`
- `list_tenants`
- `delete_tenant`
- `insert_document`
- `query_documents`
- `list_documents`
- `subscribe`
- `unsubscribe`

### Tenant runtime

Each tenant should have:

```rust
pub struct TenantRuntime {
    pub store: Arc<TenantStore>,
    pub subscriptions: SubscriptionRegistry,
}
```

Keep tenants lazily opened and cached in memory.

### Query evaluation

`evaluator.rs` should do the simplest possible correct thing:

1. open a read transaction through the store
2. scan the table
3. apply filters
4. sort if needed
5. apply limit

No planner. No indexes. No join support.

### Subscription registry

Keep the registry simple and in-memory.

Each subscription needs:

- `subscription_id`
- query
- dependent table
- outbound sender for `ServerMessage`

Recommended structure:

```rust
pub struct SubscriptionRegistry {
    // small in-memory map keyed by subscription id
}
```

Important rules:

- Do not hold a lock while evaluating a query.
- Do not hold a lock while sending on a channel.
- Collect affected subscriptions first, drop the lock, then re-evaluate/send.

### Invalidation model

For v1, invalidation is **table level only**.

If a commit touches table `tasks`, then every subscription whose query targets
`tasks` is re-evaluated. This is intentionally coarse and acceptable for v1.

### Post-commit flow

Every write path in the engine must follow this order:

```text
apply mutation in storage
-> get CommitEntry
-> determine affected subscriptions
-> re-evaluate those queries using a fresh read transaction
-> send full result arrays to those subscriptions
-> return mutation response
```

Never send updates before the storage commit succeeds.

## Server Design

### `nimbus-server`

This crate should contain:

- protocol message structs
- Axum router
- HTTP handlers
- WebSocket handler
- shared `AppState`

Keep all business logic delegated to `Service`.

### HTTP API

Use these routes:

- `GET /health`
- `POST /api/tenants`
- `GET /api/tenants`
- `DELETE /api/tenants/:tenant_id`
- `POST /api/tenants/:tenant_id/documents`
- `GET /api/tenants/:tenant_id/documents/:table`
- `POST /api/tenants/:tenant_id/query`

Use these exact request/response shapes unless there is a compelling reason to
slightly improve naming consistency:

- `POST /api/tenants`
  - request: `{ "id": "demo" }`
  - response: `201 Created` with `{ "id": "demo" }`
- `GET /api/tenants`
  - response: `{ "tenants": ["demo"] }`
- `DELETE /api/tenants/:tenant_id`
  - response: `204 No Content`
- `POST /api/tenants/:tenant_id/documents`
  - request: `{ "table": "tasks", "fields": { "title": "Hello" } }`
  - response: `201 Created` with `{ "id": "<ulid>" }`
- `GET /api/tenants/:tenant_id/documents/:table`
  - response: `{ "data": [ ...documents ] }`
- `POST /api/tenants/:tenant_id/query`
  - request: query JSON
  - response: `{ "data": [ ...documents ] }`
- `GET /health`
  - response: `{ "ok": true }`

Important:

- Do **not** use `GET` with a JSON body for query execution.
- The write path for `POST /documents` must call the same engine mutation path
that drives subscription invalidation.

### WebSocket API

Endpoint:

- `GET /ws`

Tenant identification:

- Require `X-Tenant-Id` request header on the WebSocket upgrade request.

Client messages for v1:

```json
{ "type": "subscribe", "request_id": "1", "query": { ... } }
{ "type": "unsubscribe", "subscription_id": 1 }
```

Do not add additional WebSocket message types in phase 1 unless they are needed
for clean shutdown/error handling.

Server messages for v1:

```json
{
  "type": "subscription_result",
  "subscription_id": 1,
  "request_id": "1",
  "data": [...]
}
```

and

```json
{ "type": "error", "request_id": "1", "message": "..." }
```

The phase-1 end-to-end test should assert only against these message types.

### WebSocket implementation pattern

Follow the basic shape from `mini-redis` and Convex:

1. Split the socket into sender and receiver halves.
2. Create one outbound `mpsc::UnboundedSender<ServerMessage>` for that
  connection.
3. A send task serializes outbound messages and writes them to the socket.
4. A receive loop parses client messages and calls the engine.
5. On disconnect, unsubscribe all subscriptions created by that socket.

Do not let deep engine code touch the raw WebSocket directly.

## Binary Design

### `nimbus-bin`

Responsibilities:

- CLI parsing with `clap`
- initialize tracing
- build `Service`
- build Axum router
- bind a listener
- run the server

Recommended flags:

- `--port`
- `--data-dir`

## Dependency Guidance

Use stable versions from the current ecosystem.

Workspace dependencies should include:

```toml
axum = { version = "0.8", features = ["ws"] }
clap = { version = "4", features = ["derive"] }
futures = "0.3"
redb = "2"
rmp-serde = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tempfile = "3"
thiserror = "2"
tokio = { version = "1", features = ["full"] }
tokio-tungstenite = "0.27"
tracing = "0.1"
tracing-subscriber = "0.3"
ulid = { version = "1", features = ["serde"] }
reqwest = { version = "0.12", features = ["json"] }
```

`tokio-tungstenite` and `reqwest` can be dev-dependencies if desired.

## Build Order

Execute in this exact order. Do not skip ahead.

### Step 1: Workspace skeleton

Create all crate manifests and empty `lib.rs` / `main.rs` files.

Verify:

```bash
cargo check --workspace
```

### Step 2: `nimbus-core`

Implement all shared types, queries, mutations, and errors.

Verify:

```bash
cargo test -p nimbus-core
```

Add a few unit tests for:

- `DocumentId` parse/display roundtrip
- document JSON conversion
- basic query serialization

### Step 3: `nimbus-storage`

Implement:

- key helpers
- `TenantStore`
- commit log append/read
- table scan by prefix
- insert path first

Verify:

```bash
cargo test -p nimbus-storage
```

Required storage tests:

- insert then get
- insert multiple docs then scan table
- same store contains multiple logical tables without cross-contamination
- commit log sequence increments
- reopen from disk and read back data

### Step 4: `nimbus-engine`

Implement:

- tenant registry
- evaluator
- subscription registry
- insert + invalidation path

Verify:

```bash
cargo test -p nimbus-engine
```

Required engine tests:

- query returns all docs in table
- equality filter works
- ordering works
- limit works
- commit touching `tasks` only invalidates `tasks` subscriptions
- insert causes re-evaluation and outbound message send

### Step 5: `nimbus-server`

Implement:

- protocol types
- router
- HTTP handlers
- WebSocket handler

Verify:

```bash
cargo test -p nimbus-server
```

At minimum add:

- `health` route smoke test
- create tenant route test
- insert route test against a live `Service`

### Step 6: `nimbus-bin`

Implement CLI and startup.

Verify:

```bash
cargo build -p nimbus-bin
```

### Step 7: End-to-end test

Create `tests/reactive_loop.rs`.

This test must:

1. Start the real server on `127.0.0.1:0`
2. Create tenant `demo`
3. Open a WebSocket with `X-Tenant-Id: demo`
4. Send a `subscribe` message for table `tasks`
5. Receive the initial empty result
6. Send an HTTP insert for a `tasks` document
7. Receive the pushed updated result over WebSocket
8. Assert the inserted document is present

The integration test should also verify:

1. the first subscription result is an empty `data` array
2. the reactive push omits `request_id`
3. the inserted document contains `_id`, `_creationTime`, and `title`

Recommended test harness pattern:

- bind `TcpListener` to `127.0.0.1:0`
- spawn `axum::serve(listener, app)`
- use `tokio_tungstenite::connect_async` with an explicit HTTP request builder
so the WebSocket handshake includes `X-Tenant-Id: demo`
- use `reqwest::Client`
- use `tokio::time::timeout` around WebSocket receives

Verify:

```bash
cargo test --workspace
```

### Step 8: Final polish

Run:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

Fix the important issues only after the reactive loop is already green.

## Implementation Rules

### General

1. Prefer the smallest correct implementation over future-proof abstraction.
2. Compile after every meaningful step.
3. Fix errors immediately before moving on.
4. Avoid `unwrap()` outside tests.
5. Add doc comments to public types and functions.
6. Prefer explicit request/response structs over `serde_json::Value` in the
  server layer, except for document field payloads themselves.

### Concurrency

1. Do not hold any mutex or rwlock guard across `.await`.
2. Keep the subscription registry lock scope tiny.
3. Use channels to cross async boundaries, not shared socket handles.

### redb

1. Use a single database file per tenant.
2. Use in-memory backend for unit tests where possible.
3. Keep transaction scope tight.
4. Commit explicitly.

### API

1. WebSocket is for subscriptions only in v1.
2. HTTP is the canonical mutation path in v1.
3. Always return structured JSON errors.

## Definition of Done

This pass is done when all of the following are true:

1. `cargo test --workspace` passes
2. `cargo clippy --workspace --all-targets -- -D warnings` passes
3. The integration test proves reactive push over WebSocket after HTTP insert
4. The binary starts with a local data directory and serves `/health`

## What Not to Do

Avoid the common failure modes:

- Do not build schema/index/planner infrastructure "for later"
- Do not introduce a generic storage engine trait hierarchy
- Do not create more crates than listed above
- Do not put query evaluation logic in HTTP or WebSocket handlers
- Do not try to implement Convex's full sync protocol
- Do not let a duplicated prompt or stale copy become the source of truth

## The Moment of Truth

The project succeeds if this exact scenario works under automated test:

```text
WebSocket client subscribes to tasks
-> server sends []
-> HTTP inserts {"title":"Hello"} into tasks
-> server commits write to redb
-> server re-evaluates affected subscriptions
-> WebSocket client receives [{"_id": "...", "_creationTime": ..., "title": "Hello"}]
```

That is the proof that nimbus's core architecture is viable.
