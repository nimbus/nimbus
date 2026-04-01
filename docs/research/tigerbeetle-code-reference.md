# TigerBeetle Code Reference For Neovex

This note maps the TigerBeetle codebase to the Neovex roadmap, especially
Phase 6 durable-journal work.

It is a reference guide, not an execution plan and not a request to copy
TigerBeetle literally. Neovex remains a redb-backed reactive database with a
Neovex-owned logical durable journal.

## Local Checkout

This note was prepared against the local TigerBeetle checkout at:

- `/Users/jack/src/github.com/tigerbeetle/tigerbeetle`

If that path is unavailable, the same files can be read from the upstream repo:

- `https://github.com/tigerbeetle/tigerbeetle`

## Read These Files First

### 1. `docs/ARCHITECTURE.md`

Start here for the big picture.

What to take from it:

- the ground state is an append-only ordered log
- derived state is materialized from that log
- checkpoints plus replay rebuild in-memory state after crash
- determinism is treated as a storage and recovery feature, not just a testing
  convenience

For Neovex:

- this is the best reference for why Phase `6A/6B` should produce one logical
  ordered-history contract

### 2. `src/vsr.zig`

This is the architectural export surface. It shows the major seams clearly:

- `storage`
- `state_machine`
- `vsr/journal`
- `lsm/*`
- `aof`
- `testing/*`

For Neovex:

- use this file to understand how TigerBeetle separates ordered history,
  materialized state, and verification tooling

### 3. `src/vsr/journal.zig`

This is the most relevant low-level log file for Neovex journal work.

What to study:

- explicit journal slots and sequence discipline
- redundant metadata and recovery-oriented layout
- overlap handling for concurrent writes
- careful treatment of torn writes and misdirected reads or writes

For Neovex:

- borrow the discipline around visible order, durability boundaries, and
  recovery assumptions
- do not copy the exact physical ring-buffer layout; Neovex needs a logical
  tenant journal, not TigerBeetle's replica WAL structure

### 4. `src/state_machine.zig`

This is the clearest reference for "ordered history feeds deterministic derived
state".

What to study:

- state machine separate from replication and raw storage concerns
- forest-backed derived state
- explicit batch encoders and decoders
- typed data model on top of general storage primitives

For Neovex:

- this is the reference for making redb tables a materialized view of the
  durable journal in Phase `6B`

### 5. `src/lsm/forest.zig`

This file shows how TigerBeetle organizes many trees into one coherent derived
storage layer.

What to study:

- manifest-driven organization
- checkpoint and compaction orchestration
- explicit resource and memory budgeting
- deterministic tree-id and schema organization

For Neovex:

- useful if we later add a custom write-optimized materializer
- especially relevant for thinking about journal-to-materialized-state
  boundaries and replay safety
- now explicitly in roadmap scope for a future shadow-mode materializer with
  deterministic compaction

### 6. `src/lsm/tree.zig`

This is the core LSM tree implementation.

What to study:

- mutable plus immutable table staging
- manifest and compaction boundaries
- `scope_open` / `scope_close` persist-vs-discard semantics
- lookup and key-range discipline

For Neovex:

- useful only if we later build our own materializer
- not a reason to replace redb in Phase `6A`

### 7. `src/lsm/compaction.zig`

This is the key deterministic-compaction file.

What to study:

- how compaction inputs are selected from explicit state
- how output tables are produced from merge inputs
- how visibility changes are applied through manifest updates
- how compaction work is paced in explicit beats rather than ad hoc background
  timing

For Neovex:

- this is the most important TigerBeetle file for a future deterministic
  materializer
- use it to shape deterministic compaction triggers, input selection, and
  shadow-mode rebuild semantics

### 8. `src/lsm/manifest_log.zig`

This file shows how TigerBeetle keeps durable metadata about materialized state.

What to study:

- manifest-log durability invariants
- open, flush, and compaction behavior for metadata
- rules about not dropping the latest visible state
- interaction between checkpointing and manifest persistence

For Neovex:

- this is the best reference for journal-driven materializer metadata,
  checkpoint-safe manifest updates, and compaction bookkeeping

### 9. `src/vsr/superblock.zig`

This file anchors checkpoint and recovery invariants.

What to study:

- checkpoint header invariants
- monotonic state fields
- manifest references in checkpoint state
- rules about what recovery is allowed to reconstruct

For Neovex:

- this is the main reference for explicit checkpoint metadata and rebuild
  boundaries around a future custom materializer

### 10. `src/aof.zig`

This file is useful because it makes the durability story explicit.

What to study:

- AOF as a logically useful but non-authoritative reconstruction aid
- explicit `write`, `sync`, and `checkpoint` boundaries
- the statement that AOF borrows durability from the WAL

For Neovex:

- helpful when reasoning about secondary logs and export formats
- a good reminder that Neovex should avoid inventing extra application-level
  logs unless they serve a distinct purpose

## Verification Files To Study

### `src/storage_fuzz.zig`

Why it matters:

- tests storage reads and writes under injected sector faults
- good reference for corruption-aware storage testing

For Neovex:

- inspires journal read/write corruption tests and recovery validation

### `src/state_machine_fuzz.zig`

Why it matters:

- looks for valid operations that crash on replay and cause crash loops

For Neovex:

- inspires replay-safety fuzzing for journal records and materialization logic

### `src/vopr.zig`

Why it matters:

- large simulation harness for protocol, failure, and recovery behavior

For Neovex:

- we should not copy the distributed simulator literally
- we should copy the mindset of harsh, failure-heavy, deterministic testing

### `src/integration_tests.zig`, `src/state_machine_tests.zig`

Why they matter:

- concrete end-to-end and state-machine correctness references

For Neovex:

- useful for shaping Phase `6A/6B` integration tests around replay,
  visibility, and ordering

## What Neovex Should Borrow

1. One ordered durable history should govern recovery and derived state.
2. Acknowledgment must follow durable ordering, not just in-memory acceptance.
3. Derived state should be rebuildable from checkpoints plus ordered history.
4. Recovery and corruption handling should be part of the architecture, not an
   afterthought.
5. Deterministic and fuzz-style tests should validate log ordering, replay, and
   crash behavior.

## What Neovex Should Not Copy Literally

1. The six-replica distributed consensus architecture.
2. The exact WAL ring-buffer and file-zone layout.
3. The single-threaded execution model as a hard requirement.
4. Static allocation as a project-wide rule.
5. The accounting-specific state machine and object model.

## Where TigerBeetle Is And Is Not The Right Reference

Use TigerBeetle as the closest implementation reference for:

- journal ordering and durability boundaries
- checkpoint plus replay recovery
- deterministic compaction and materializer design
- corruption, crash, and replay robustness testing

Do not treat TigerBeetle as the primary reference for:

- planner-enforced authorization
- schema-generated API design
- V8 versus WASM execution-surface decisions
- OCC conflict detection semantics

For those topics, the stronger references remain the research guide's Postgres
RLS, Firebase Rules, Convex, FoundationDB, Hasura, PostgREST, and Wasmtime
sources.

## Neovex Mapping

### Phase 6A: Durable mutation journal

Primary TigerBeetle references:

- `docs/ARCHITECTURE.md`
- `src/vsr/journal.zig`
- `src/aof.zig`
- `src/storage_fuzz.zig`

Borrow:

- no-hole visible order
- durable-then-ack discipline
- recovery-aware log design
- corruption-minded tests

Do not borrow literally:

- replica WAL slot layout
- consensus-specific terminology and machinery

### Phase 6B: Journal becomes authoritative history

Primary TigerBeetle references:

- `docs/ARCHITECTURE.md`
- `src/state_machine.zig`
- `src/lsm/forest.zig`
- `src/lsm/tree.zig`
- `src/lsm/compaction.zig`
- `src/lsm/manifest_log.zig`
- `src/vsr/superblock.zig`
- `src/state_machine_fuzz.zig`

Borrow:

- ordered history as source of truth
- derived state as materialized view
- checkpoint plus replay rebuild model
- deterministic compaction principles for a shadow-mode custom materializer
- explicit manifest and checkpoint invariants for recovery-safe compaction

### Phase 8: Streaming and replica paths

Primary TigerBeetle references:

- `docs/ARCHITECTURE.md`
- `src/aof.zig`
- `src/vopr.zig`

Borrow:

- explicit thinking about downstream consumers of ordered history
- strong verification around replay and reconstruction

## Project Decision Reminder

TigerBeetle is a design and verification reference for Neovex.

It is not:

- the implementation we are porting
- permission to replace redb in Phase `6`
- permission to replace the Neovex logical journal with a generic external log

The Neovex architecture decision remains:

- keep redb as the storage engine for this roadmap
- implement a Neovex-owned logical durable journal
- use TigerBeetle to sharpen ordering, durability, replay, and verification
  design
