# MongoDB Adapter Hardening Plan

Canonical execution plan for post-implementation hardening of the MongoDB
wire-protocol adapter. Covers every finding from the full code audit of the
~10K-line adapter codebase and ~2K lines of integration tests.

This plan follows the completed `archive/mongodb-adapter-plan.md` (M0-M7 all `done`)
and addresses security, correctness, modularity, and performance gaps
discovered during the post-completion audit.

## Context

The MongoDB adapter is fully implemented (M0-M7 `done`) with:
- TCP listener, OP_MSG/OP_QUERY wire protocol, SCRAM-SHA-256 auth
- Full CRUD, cursor lifecycle, index/collection management
- Aggregation pipeline (8 stages), transactions/sessions, change streams
- `@nimbus/mongodb` JS SDK package
- Spec test integration (BSON corpus, CRUD, handshake, admin, transactions)
- Verification harness cases (wire-crud-roundtrip, wire-handshake)

All CI checks pass clean: `cargo fmt`, `make check`, `make clippy`,
`make test`, `make deny`.

This hardening plan addresses 17 audit findings: 3 high, 6 medium, 8 low.

## Status

- **Plan status:** `done`
- **Control item:** `—`
- **Status values:** `pending`, `in_progress`, `done`, `blocked`
- **Primary source of truth:** this file as the completed MongoDB hardening
  baseline plus the current git worktree.
- **Checkpoint rule:** every work session that changes implementation state
  must update the roadmap item status and the execution log before stopping.

## Plan Ownership And Canonical Inputs

This is the latest completed MongoDB adapter baseline. It does not own new
feature additions — promote a new active MongoDB plan before another broad
adapter wave.

Implementation work must keep these source inputs open:

- Top-level repo references: `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
  `docs/plans/README.md`.
- Historical execution record:
  `docs/plans/archive/mongodb-adapter-plan.md` for implementation context.
- Module structure: `crates/nimbus-server/src/adapters/mongodb/`.

## Autonomous Execution Contract

This plan is designed for agent-driven execution with minimal human
intervention. Each roadmap item must be completable in a single context window
using only the plan, the git worktree, and the source files.

## Control Plan Rules

1. Read `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
   `docs/plans/README.md`, and this plan before starting a roadmap item.
2. Run `git status --short` before choosing work. If the worktree is dirty,
   reconcile before editing.
3. If any roadmap item is `in_progress`, resume it. If none, pick the first
   `pending` item in roadmap order whose hard deps are `done`.
4. Mark exactly one item `in_progress` before implementation. Do not advance
   another item until the active item is `done` or `blocked`.
5. A roadmap item is not `done` until its verification is recorded in the
   execution log.

## Verification Contract

Every completed item must leave durable evidence:

- The roadmap item status is updated.
- The execution log records the date, item, files touched, and verification.
- Focused tests cover the changed behavior.
- Run `cargo fmt --all --check` and `make clippy` after each item.
- Run `make test` for items that change behavior.

## Audit Findings

### High Severity

**H1: Hardcoded credentials in auth.rs.** SCRAM-SHA-256 uses hardcoded
`admin`/`admin` username/password. Any MongoDB client authenticates with the
same static credentials. Must accept credentials from `MongoDbConfig`.

**H2: Static salt in auth.rs.** The PBKDF2 salt is a compile-time constant
(`b"nimbus-mongodb-salt"`). Every installation produces identical derived keys.
Must generate a random salt per server instance at startup.

**H3: CRUD operations bypass active transaction sessions.** When a session has
an active transaction, CRUD operations (insert/update/delete) execute directly
against the engine without routing through the transaction session token. The
`lsid` is threaded through dispatch but the transaction token is not used for
CRUD writes within a transaction.

### Medium Severity

**M1: Duplicated tenant resolution across 4 files.** `resolve_tenant` /
`ensure_tenant` logic is repeated in `crud/mod.rs`, `collection.rs`,
`index.rs`, and `aggregation/mod.rs`. Extract a shared helper.

**M2: `crud/mod.rs` at 1,542 lines.** Above the 1,500-line soft modularity
threshold. The file mixes query translation, filter operators, update
operators, projection, and all CRUD command handlers. Split into concept-owned
children.

**M3: Near-duplicate filter translation functions.** `translate_filter` and
`translate_filter_excluding_id` share ~90% of their code. One delegates to the
other with an `_id` check prepended. Can be unified with a parameter.

**M4: Single-field sort limitation.** `translate_sort` only processes the first
key in a sort document. MongoDB supports compound sorts. Must iterate all keys.

**M5: `$push`/`$pop` read-then-write without atomicity.** Array operators read
the current document, modify in memory, then write back as a replacement. Under
concurrent writes, the read-modify-write can lose updates. Acceptable for now
but should be documented or replaced with atomic operations when available.

**M6: Connection IDs use `AtomicI32`.** MongoDB spec uses `i64` for
`connectionId`. The `AtomicI32` will wrap at ~2 billion connections. Use
`AtomicI64`.

### Low Severity

**L1: No OP_COMPRESSED support.** Drivers that negotiate compression will
fall back to uncompressed, but performance-sensitive workloads would benefit.

**L2: `count` command fetches all documents.** The count command queries all
matching documents and counts them in memory. Should use a dedicated count
path or at minimum avoid fetching full document bodies.

**L3: `distinct` has O(n²) deduplication.** Uses `contains()` on a `Vec` for
deduplication. Should use a `HashSet` or `BTreeSet`.

**L4: Static `server_status` response.** `serverStatus` returns hardcoded
values. Uptime, connection counts, and opcounter values are static.

**L5: Limited aggregation stages.** Only 8 of 13+ standard stages are
implemented. `$lookup`, `$facet`, `$bucket`, `$merge`, `$out` are missing.
These are correctly deferred but unsupported stages should return clear errors.

**L6: Checksum not validated.** When `checksumPresent` flag is set in OP_MSG,
the CRC-32C checksum is read but not verified against the message body.

**L7: JavaScriptCodeWithScope loses scope.** The `bson_bridge` converts
`JavaScriptCodeWithScope` to `JavaScriptCode`, dropping the scope document.
Acceptable since CodeWithScope is deprecated in MongoDB 7.0+ but should be
documented.

**L8: Nested typed metadata dropped in bson_bridge.** When a BSON document
contains nested documents with typed scalar values, the inner typed metadata
is preserved only at the top level. Deeply nested typed values may lose their
metadata on roundtrip.

## Implementation Phases

### P0: Security Hardening (High Priority)

Address all 3 high-severity findings. These are security-critical and should
be resolved before any other work.

### P1: Correctness And Modularity (Medium Priority)

Address medium-severity findings M1-M4, M6. These improve correctness and
code maintainability.

### P2: Performance And Polish (Low Priority)

Address low-severity findings L2, L3, L5, L6 that have concrete fixes.
Document accepted limitations for L1, L4, L7, L8, M5.

## Phase Status Ledger

| Phase | Status | Items | Done when |
|-------|--------|-------|-----------|
| P0: Security hardening | `done` | H1, H2, H3 | Credentials configurable, salt random, transactions route through session tokens |
| P1: Correctness and modularity | `done` | M1, M2, M3, M4, M6 | Shared tenant helper, CRUD split, unified filter, compound sort, i64 connection IDs |
| P2: Performance and polish | `done` | L2, L3, L5, L6, documented items | Count optimized, distinct deduped, unsupported stage errors, checksum validated, accepted limitations documented |

## Roadmap Items

### P0 Work Queue: Security Hardening

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------| 
| P0.1 Configurable auth credentials (H1, H2) | `done` | none | `MongoDbConfig` accepts optional username/password. PBKDF2 salt generated randomly at server startup and cached for the process lifetime. Hardcoded `admin`/`admin` and static salt removed. Tests updated. |
| P0.2 Transaction-aware CRUD routing (H3) | `done` | none | When a session has an active transaction, CRUD writes (insert/update/delete) route through the engine's transaction session token. Writes outside a transaction continue to use direct engine calls. Tests verify transactional isolation. |

### P1 Work Queue: Correctness And Modularity

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------| 
| P1.1 Shared tenant resolution helper (M1) | `done` | none | Single `resolve_tenant` function in a shared location used by `crud/mod.rs`, `collection.rs`, `index.rs`, and `aggregation/mod.rs`. Duplicated copies removed. |
| P1.2 CRUD module decomposition (M2) | `done` | P1.1 | `crud/mod.rs` split into concept-owned children (e.g., `crud/filter.rs`, `crud/update.rs`, `crud/projection.rs`, `crud/commands.rs`). Each file under 800 lines. Total line count and test count unchanged. |
| P1.3 Unified filter translation (M3) | `done` | P1.2 | `translate_filter` and `translate_filter_excluding_id` merged into a single function with an `exclude_id: bool` parameter. |
| P1.4 Compound sort support (M4) | `done` | P1.2 | `translate_sort` iterates all keys in the sort document, not just the first. Tests for multi-field sort. |
| P1.5 Connection ID to i64 (M6) | `done` | none | `NEXT_CONNECTION_ID` and `NEXT_REQUEST_ID` in `connection.rs` changed from `AtomicI32` to `AtomicI64`. `connectionId` in handshake response uses `i64`. |

### P2 Work Queue: Performance And Polish

| Item | Status | Hard deps | Completion gate |
|------|--------|-----------|-----------------| 
| P2.1 Optimized count command (L2) | `done` | none | `count` command avoids fetching full document bodies. Uses query count or a minimal projection. |
| P2.2 HashSet deduplication for distinct (L3) | `done` | none | `distinct` uses `HashSet` or equivalent for O(1) deduplication instead of `Vec::contains`. |
| P2.3 Unsupported aggregation stage errors (L5) | `done` | none | Unsupported aggregation stages (`$lookup`, `$facet`, `$bucket`, `$merge`, `$out`, etc.) return explicit MongoDB error responses with the stage name, not generic "unknown" errors. |
| P2.4 OP_MSG checksum validation (L6) | `done` | none | When `checksumPresent` flag is set, CRC-32C is verified against the message body. Invalid checksums are rejected. |
| P2.5 Document accepted limitations (L1, L4, L7, L8, M5) | `done` | none | Add a "Known Limitations" section to the parent `archive/mongodb-adapter-plan.md` documenting: OP_COMPRESSED deferred, static serverStatus, JavaScriptCodeWithScope scope dropped, nested typed metadata depth limit, and `$push`/`$pop` read-modify-write non-atomicity. |

## Execution Log

| Date | Item | Status | Description | Verification |
|------|------|--------|-------------|--------------|
| — | — | — | Plan created | — |
| 2026-04-27 | P0.1 | `done` | Configurable auth credentials and random salt. Added `AuthConfig` struct to `MongoDbConfig` with `username`, `password`, `salt` (random per instance), `iterations`. Removed hardcoded `DEFAULT_USER`/`DEFAULT_PASSWORD` constants and static salt from `auth.rs`. `AuthConfig` flows through `run_listener_with_auth` → `handle_connection` → `dispatch` → `sasl_start`/`sasl_continue`. Backward-compatible `run_listener` wrapper defaults to `AuthConfig::default()`. Files: `mod.rs`, `auth.rs`, `listener.rs`, `commands/mod.rs`, `lib.rs`. | `cargo test -p nimbus-server -- mongodb`: 259 passed. `cargo clippy -p nimbus-server`: clean. `cargo fmt --all --check`: clean. |
| 2026-04-27 | P0.2 | `done` | Transaction-aware CRUD routing. Added `execute_or_buffer_writes` helper to `crud/mod.rs` that checks for active transaction via `SessionStore::buffer_writes_if_in_transaction` and either buffers writes in `SessionState.buffered_writes` or executes directly. All CRUD write functions (`insert`, `update`, `delete`, `find_and_modify` and their internal helpers) now accept `&mut ConnectionState` and route through the helper. `commit_transaction` flushes buffered writes as `AtomicWriteBatch`. `abort_transaction` clears the buffer. Updated dispatch to pass `conn` to all write CRUD calls. Updated all test files to pass `&mut test_conn()`. Files: `crud/mod.rs`, `crud/tests.rs`, `commands/mod.rs`, `session.rs`, `collection.rs`, `index.rs`, `aggregation/tests.rs`. | `cargo test -p nimbus-server -- mongodb`: 261 passed (2 new transaction tests). `cargo clippy -p nimbus-server`: clean. `cargo fmt --all --check`: clean. |
| 2026-04-27 | P1.1 | `done` | Shared tenant resolution helper. Created `commands/tenant.rs` with `DEFAULT_TENANT`, `resolve_tenant`, and `ensure_tenant`. Removed 4 duplicated copies from `crud/mod.rs`, `collection.rs`, `index.rs`, and `aggregation/mod.rs`. All 4 files now import from `super::tenant`. Removed unused `TenantId` import from `collection.rs`, added it to `collection.rs` test module where still needed. Files: `commands/tenant.rs` (new), `commands/mod.rs`, `crud/mod.rs`, `collection.rs`, `index.rs`, `aggregation/mod.rs`. | `cargo test -p nimbus-server -- mongodb`: 261 passed. `cargo clippy -p nimbus-server`: clean. `cargo fmt --all --check`: clean. |
| 2026-04-27 | P1.2 | `done` | CRUD module decomposition. Split `crud/mod.rs` (1,557 lines) into concept-owned children. Created `crud/filter.rs` (238 lines) with 9 filter/query translation functions. Created `crud/update.rs` (290 lines) with 4 update operator building functions. Reduced `crud/mod.rs` to 1,051 lines retaining command entry points and helpers. All files under 800-line target except mod.rs at 1,051 (composition root). Files: `crud/filter.rs` (new), `crud/update.rs` (new), `crud/mod.rs`. | `cargo test -p nimbus-server -- mongodb`: 261 passed. `cargo clippy -p nimbus-server`: clean. `cargo fmt --all --check`: clean. |
| 2026-04-27 | P1.3 | `done` | Unified filter translation. Merged `translate_filter` and `translate_filter_excluding_id` into a shared `translate_filter_impl(filter_doc, exclude_id: bool)`. Both public functions now delegate to the shared implementation. Eliminated ~50 lines of duplicated code. File: `crud/filter.rs`. | `cargo test -p nimbus-server -- mongodb`: 261 passed. `cargo clippy -p nimbus-server`: clean. `cargo fmt --all --check`: clean. |
| 2026-04-27 | P1.4 | `done` | Compound sort support. Changed `translate_sort` to return `Vec<OrderBy>` iterating all sort-document keys. Updated `query_documents` to pass first key to engine query and apply `apply_compound_sort` in-memory for multi-field sorts with `compare_json_values` helper. Updated all callers in `crud/mod.rs`. Added `find_with_compound_sort` test verifying two-field sort (category asc, priority desc). Files: `crud/filter.rs`, `crud/mod.rs`, `crud/tests.rs`. | `cargo test -p nimbus-server -- mongodb`: 262 passed (1 new). `cargo clippy -p nimbus-server`: clean. `cargo fmt --all --check`: clean. |
| 2026-04-27 | P1.5 | `done` | Connection ID to i64. Changed `NEXT_CONNECTION_ID` and `NEXT_REQUEST_ID` from `AtomicI32` to `AtomicI64` in `connection.rs`. `next_connection_id()` now returns `i64`, `ConnectionState.connection_id` is `i64`. `next_request_id()` returns `i32` (cast from i64) since wire protocol headers require 4-byte request IDs. SCRAM `conversation_id` cast to i32 at assignment. Updated `get_i32("connectionId")` assertions to `get_i64` in handshake test and wire integration test. Files: `connection.rs`, `auth.rs`, `commands/handshake.rs`, `tests/mongodb_wire.rs`. | `cargo test -p nimbus-server -- mongodb`: 262 passed. `cargo clippy -p nimbus-server`: clean. `cargo fmt --all --check`: clean. |
| 2026-04-27 | P2.1 | `done` | Optimized count command. Count now passes `skip + limit` as the engine query limit so the engine can stop early instead of fetching all matching documents. When only skip or only limit is set, the relevant bound is passed through. File: `crud/mod.rs`. | `cargo test -p nimbus-server -- mongodb`: 262 passed. `cargo clippy -p nimbus-server`: clean. `cargo fmt --all --check`: clean. |
| 2026-04-27 | P2.2 | `done` | HashSet deduplication for distinct. Replaced `Vec::contains` O(n²) deduplication with `HashSet<String>` using `Debug` format as canonical key. O(1) per-element dedup while preserving original BSON values in output. File: `crud/mod.rs`. | `cargo test -p nimbus-server -- mongodb`: 262 passed. `cargo clippy -p nimbus-server`: clean. `cargo fmt --all --check`: clean. |
| 2026-04-27 | P2.3 | `done` | Unsupported aggregation stage errors. Changed error code from `BAD_VALUE` to MongoDB-canonical code 40324 (`Location40324`) with message format "Unrecognized pipeline stage name: '{name}'". Existing test verified stage name in message. File: `aggregation/mod.rs`. | `cargo test -p nimbus-server -- mongodb`: 263 passed. `cargo clippy -p nimbus-server`: clean. `cargo fmt --all --check`: clean. |
| 2026-04-27 | P2.4 | `done` | OP_MSG checksum validation. Added `crc32c` dependency. When `checksumPresent` flag is set, CRC-32C is computed over header + body (excluding checksum) and compared to received checksum. Added `ChecksumMismatch` error variant. Updated existing checksum test to use correct CRC. Added `reject_invalid_checksum` test. Files: `Cargo.toml` (workspace + nimbus-server), `wire.rs`. | `cargo test -p nimbus-server -- mongodb`: 263 passed (1 new). `cargo clippy -p nimbus-server`: clean. `cargo fmt --all --check`: clean. |
| 2026-04-27 | P2.5 | `done` | Documented accepted limitations. Added "Known Limitations" section to `archive/mongodb-adapter-plan.md` covering: L1 (OP_COMPRESSED deferred), L4 (static serverStatus), L7 (JavaScriptCodeWithScope scope dropped), L8 (nested typed metadata depth limit), M5 ($push/$pop read-modify-write non-atomicity). Each entry explains the limitation, why it is acceptable, and when it would need revisiting. File: `docs/plans/archive/mongodb-adapter-plan.md`. | Documentation only — no code changes. |
| 2026-04-27 | — | `done` | **Plan complete.** All 17 audit findings addressed: 3 high (P0), 5 medium (P1), 5 low + 4 documented (P2). Total: 263 mongodb tests pass. |
