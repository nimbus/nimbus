# DynamoDB Adapter — Divergences

Intentional, recorded differences between the Nimbus DynamoDB adapter and real
DynamoDB / the ExtendDB reference. Every entry has a rationale and a regression
test asserting the chosen behavior. The parity runner (D8) classifies any
unrecorded difference as `nimbus-divergence` and fails until it appears here.

Classifications: `nimbus-divergence` (Nimbus differs from both DynamoDB Local and
ExtendDB — must be justified here) and `accept-extenddb-divergence` (Nimbus
matches ExtendDB but not real DynamoDB).

## DDB-DIV-001 — Composite primary key size limit (`nimbus-divergence`)

**Real DynamoDB:** partition key ≤ 2,048 bytes + sort key ≤ 1,024 bytes
(≤ 3,072 raw bytes combined).

**Nimbus:** the composite key is encoded into a single `DocumentId`
(`<type><base64url(value)>` per segment, joined by `.`), and `DocumentId` is
capped at 1,500 bytes (`nimbus_core::validate_document_key`). base64url inflates
by ~33%, so the supported combined **raw** key is ~1,100 bytes. Keys whose
encoded form exceeds 1,500 bytes are rejected with `ValidationException`.

**Rationale:** raising the core `DocumentId` limit is a cross-cutting storage
change affecting every backend; the adapter accepts the tighter bound until a
real workload needs full-size DynamoDB keys. Most keys are far below this.

**Regression test:** `crates/nimbus-dynamodb/src/key.rs` →
`tests::rejects_oversize_key`.

**Status:** accepted (D0.3).

## DDB-DIV-002 — Sort-key ordering uses an order-preserving projection (planned)

Real DynamoDB orders sort keys by type (`N` numeric, `S` UTF-8 byte-wise, `B`
byte-wise). Nimbus's index/compare path runs numbers through `f64` and cannot
index binary, so the adapter projects each key/index attribute into an
order-preserving sortable string in `_pk`/`_sk` (and per-index `_gsi1_*` fields):
`S` → raw UTF-8, `N` → a full-precision lexicographically-sortable decimal
encoding, `B` → fixed-case hex. Range conditions evaluate that projection, not
the opaque `DocumentId`.

**Status:** projection lands in the D0.3 sortable-key follow-up; range execution
in D2.1. This entry will gain its regression test (type-correct ordering,
including >17-digit numeric ranges that `f64` would collapse) when the projection
lands.

## DDB-DIV-003 — Reserved `_ddb_` table-name prefix (`nimbus-divergence`)

**Real DynamoDB:** table names match `[a-zA-Z0-9_.-]{3,255}`; a `_ddb_` prefix is
allowed.

**Nimbus:** user table names beginning with `_ddb_` are rejected with
`ValidationException`. The adapter persists each table's `TableDescription` in a
tenant-scoped catalog table named `_ddb_catalog`; reserving the prefix prevents
a user table from colliding with adapter metadata.

**Regression test:** `crates/nimbus-dynamodb/src/commands/control_plane.rs` →
`tests::reserved_prefix_and_bad_key_schema_rejected`.

**Status:** accepted (D0.6).

## DDB-DIV-004 — Table is `ACTIVE` immediately (`nimbus-divergence`)

**Real DynamoDB:** CreateTable returns `CREATING`; the table transitions to
`ACTIVE` asynchronously, and clients poll a `table_exists` waiter.

**Nimbus:** table creation is synchronous, so CreateTable returns `ACTIVE` and
the first DescribeTable already reports `ACTIVE`. SDK waiters observe `ACTIVE`
on their first poll — strictly faster, with no behavioral regression for
clients (no `CREATING` state is ever exposed).

**Regression test:** `crates/nimbus-dynamodb/src/commands/control_plane.rs` →
`tests::create_then_describe_roundtrips`; SDK-level
`crates/nimbus-server/tests/dynamodb_spec/main.rs` →
`control_plane_roundtrip_through_official_sdk` (asserts `TableStatus::Active`).

**Status:** accepted (D0.6).

## DDB-DIV-005 — Item storage format + PutItem overwrite atomicity (`nimbus-divergence`)

**Real DynamoDB:** items are opaque to the storage engine; PutItem is an atomic
full replace.

**Nimbus:** each attribute is stored as its **AttributeValue wire-JSON**
(`{"N":"42"}`, `{"SS":[…]}`, …) in the shared `documents` table's `fields` map,
keyed by the composite-key `DocumentId` (D0.3). This is exactly lossless — `N`
precision, sets, binary, and nesting all survive. The engine's mutation path
(`Mutation::Insert { fields }`) carries only JSON `fields`, not the
`typed_fields` sidecar, so:

1. A non-DynamoDB adapter reading a DynamoDB-owned table sees DynamoDB-tagged
   JSON rather than clean projected values. DynamoDB tables are DynamoDB-owned,
   so this is acceptable.
2. PutItem's replace-on-overwrite is implemented as delete + insert (the engine
   exposes no atomic upsert / `Mutation::Replace`, and a bare insert errors on an
   existing key). A process crash strictly between the delete and the insert
   would leave the key absent.

**Rationale:** this keeps every write on the sanctioned engine-owned mutation
path with no change to the core `Mutation` enum (which ~21 sites and every
adapter depend on). The proper fix for atomic overwrite is a store-level
document upsert (or a `Mutation::Replace` variant), tracked as a follow-up
before the D9 enterprise-readiness gate.

**Regression test:** `crates/nimbus-dynamodb/src/attribute_value.rs` →
`tests::item_roundtrips_through_wire_json_fields` (lossless storage form);
`crates/nimbus-dynamodb/src/commands/item.rs` →
`tests::put_overwrite_fully_replaces_not_merges` (replace, not merge).

**Status:** accepted (D1.5); atomic-upsert follow-up tracked.

## DDB-DIV-006 — Single-shard streams (`nimbus-divergence`)

**Real DynamoDB:** a stream is a tree of shards that split and merge over the
table's lifetime; consumers walk parent→child shard lineage. ExtendDB models 4
shards.

**Nimbus:** a stream-enabled table exposes exactly **one** open shard with a
stable shard id (`shardId-00000000000000000000-<table_id>`) and no parent. Stream
sequence numbers are zero-padded `i64` strings.

**Rationale:** a single shard is sufficient for ordered change capture and is
actually simpler for consumers — every change record is totally ordered with no
cross-shard ordering or shard-lineage bookkeeping. DynamoDB's shard tree exists
to scale throughput across partitions, which Nimbus's storage model does not
require here.

**Regression test:** `crates/nimbus-dynamodb/src/commands/stream.rs` →
`tests::describe_stream_returns_single_open_shard`.

**Status:** accepted (D5.2).

## DDB-DIV-007 — Read-triggered stream-record retention (`nimbus-divergence`)

**Real DynamoDB:** stream records are retained for 24 hours and then expire
automatically (time-driven), independent of whether any consumer reads them.

**Nimbus:** records past the 24h window are never returned by `GetRecords`
(read-time eviction), and their backing storage is reclaimed on the next
`GetRecords` poll of that stream. A stream that is never polled keeps its
already-expired records on disk until the next poll, rather than having them
swept by a background timer. The iterator always advances past expired records,
so a re-poll never stalls. The stream's sequence high-water mark is persisted in
a separate counter store (`_ddb_streamseq_<table>`), so reclaiming expired
records never resets sequence numbers — they stay monotonic for the life of the
stream, matching DynamoDB.

**Rationale:** the adapter crate has no background scheduler of its own;
hanging retention off the read path keeps eviction observable and deterministic
(the consumer contract — "you never receive an expired record" — holds exactly)
without introducing a timer thread. The separate counter store is what makes
physical reclamation safe.

**Regression tests:** `crates/nimbus-dynamodb/src/commands/stream.rs` →
`tests::get_records_skips_expired_events_and_reclaims_their_storage` and
`tests::reclaiming_expired_events_preserves_the_monotonic_sequence`.

**Status:** accepted (D5.5); time-driven background compaction of never-polled
streams is a follow-up.

## DDB-DIV-008 — TTL attribute-name charset is unrestricted (`nimbus-divergence`)

**Real DynamoDB:** the TTL attribute name accepts any UTF-8 string (1–255
characters). ExtendDB restricts the charset as part of its SQL-injection defense.

**Nimbus:** accepts any UTF-8 TTL attribute name within the 1–255 character
bound — matching DynamoDB, not ExtendDB. Nimbus stores items in a document
engine with no SQL surface, so there is no injection vector to defend against by
narrowing the charset.

**Rationale:** matching the broader DynamoDB contract maximizes drop-in
compatibility for teams migrating real TTL configurations, and the ExtendDB
restriction's sole motivation (SQL safety) does not apply to Nimbus.

**Regression test:** `crates/nimbus-dynamodb/src/commands/ttl.rs` →
`tests::update_accepts_any_utf8_attribute_name`.

**Status:** accepted (D6.1).

## DDB-DIV-009 — No TTL modification cooldown (`nimbus-divergence`)

**Real DynamoDB:** TTL enable/disable is rate-limited — a table's TTL cannot be
re-toggled more than a small number of times within a fixed interval (~1 hour),
and the table transitions through an asynchronous ENABLING/DISABLING state. A
too-fast change is rejected with a `ValidationException`.

**Nimbus:** every `UpdateTimeToLive` takes effect immediately (no async
ENABLING/DISABLING state — consistent with DDB-DIV-004's ACTIVE-immediately
control plane) and there is no cooldown. Re-enabling, disabling, and changing
the attribute name in rapid succession all succeed.

**Rationale:** the DynamoDB cooldown exists to bound the cost of a
fleet-wide background reconfiguration that Nimbus does not perform — TTL state
here is a single catalog-doc write. Removing the cooldown makes the adapter
predictable for tests and migrations without weakening any correctness or
isolation guarantee.

**Regression test:** `crates/nimbus-dynamodb/src/commands/ttl.rs` →
`tests::update_is_idempotent_with_no_cooldown`.

**Status:** accepted (D6.1).

## DDB-DIV-010 — ConsistentRead on a GSI query is served, not rejected (`nimbus-divergence`)

**Real DynamoDB:** a `Query` (or `Scan`) on a **global** secondary index with
`ConsistentRead=true` is rejected with `ValidationException` — GSIs in DynamoDB
are maintained asynchronously and only support eventually-consistent reads.

**Nimbus:** accepts `ConsistentRead=true` on a GSI query and serves the result.
Nimbus maintains index entries inside the same storage transaction as the base
write (no async index propagation), so every index read is already strongly
consistent — there is nothing to reject.

**Rationale:** the DynamoDB rejection exists only because its GSIs lag the base
table; Nimbus has no such lag, so honoring the flag is a strict upgrade that
never returns stale data. Accepting (rather than rejecting) is also the more
permissive, drop-in-compatible choice: a client that never sets the flag is
unaffected, and a client that does gets a stronger guarantee instead of an
error. (D4.4 decision: accept-and-serve over match-DynamoDB-rejection.)

**Regression test:** `crates/nimbus-dynamodb/src/commands/query.rs` →
`tests::query_gsi_with_consistent_read_is_served_consistently`.

**Status:** accepted (D4.4).
