---
title: Storage
description: A crate-level tour of nimbus-storage — the five persistence providers, per-tenant physical isolation, the single-transaction atomicity invariant, index lifecycle, and encryption at rest.
sidebar:
  label: Storage
  order: 5
---

`crates/nimbus-storage` is the persistence layer: five interchangeable
providers behind one contract, each keeping every tenant physically
isolated and each enforcing the same atomicity invariant with its own
native transaction machinery. How to select and configure a backend is
covered in [storage backends](/operators/storage-backends/); this page is
about how the layer is built.

## The provider abstraction

The engine consumes storage through two enums in
`crates/nimbus-engine/src/persistence/`: a provider enum that opens and
deletes tenant stores, and a per-tenant persistence enum with one variant
per backend — redb, SQLite, libSQL replica, Postgres, and MySQL. Behind
those variants, `crates/nimbus-storage/src/traits/mod.rs` defines the
capability traits each backend implements: document reads and writes,
index-maintaining mutations, schema replacement, commit-log access, and
scheduler state. The embedded default is SQLite
(`crates/nimbus-storage/src/async_storage/engine.rs` declares SQLite as
the default embedded provider kind, with redb as the alternative).

## Five providers, one contract

| Provider | Tenant isolation unit | Document format at rest |
| --- | --- | --- |
| SQLite (default) | Database file per tenant | JSON text columns |
| redb | Database file per tenant | MessagePack values |
| Postgres | Schema per tenant | JSON text columns |
| MySQL | Database per tenant | JSON text columns |
| libSQL replica | Namespace per tenant | JSON text columns |

There is deliberately no single at-rest serialization format — it is a
per-backend choice. The SQL-family backends store documents as JSON text
(`data_json` plus a `typed_fields_json` column for typed index keys),
while redb stores MessagePack-encoded documents
(`crates/nimbus-storage/src/document_codec.rs`). What *is* uniform is the
commit log: on every backend, commit-log records are MessagePack blobs
serialized and integrity-validated by
`crates/nimbus-storage/src/commit_log.rs`.

**SQLite** (`crates/nimbus-storage/src/sqlite.rs`) creates one database
file per tenant containing relational tables for the table catalog,
documents, schemas, the commit log, and scheduler state. Writes run on a
dedicated connection under `BEGIN IMMEDIATE` transactions; reads come
from a pool of read-only connections with snapshot semantics
(`crates/nimbus-storage/src/sqlite/config.rs`).

**redb** (`crates/nimbus-storage/src/store.rs`) is the retained embedded
key-value backend: one file per tenant with typed key-value tables for
documents, index entries, schemas, the commit log, scheduler state, and
metadata. Index entries live in an ordered keyspace so range scans are
key-prefix scans.

**Postgres** (`crates/nimbus-storage/src/postgres/`) provisions one
schema per tenant carrying the same logical tables, plus a shared
metadata schema for the tenant registry. Per-tenant advisory locks
serialize writers.

**MySQL** (`crates/nimbus-storage/src/mysql/`) provisions one database
per tenant, with a shared metadata database for the registry — the same
shape as Postgres at a different isolation granularity.

**libSQL replica** (`crates/nimbus-storage/src/libsql/`) splits reads
from writes: each tenant is a namespace on a remote libSQL primary
(provisioned through the primary's admin API), writes execute in
interactive immediate-mode transactions against the remote connection
(`crates/nimbus-storage/src/libsql/write.rs`), and reads are served from
a local replica file. Freshness is explicit, not assumed: the read path
(`crates/nimbus-storage/src/libsql/freshness.rs`) tracks replica
staleness, syncs behind barriers when a read requires newer state, and
reports freshness statistics rather than silently serving stale data.

## One transaction, three effects

The storage layer's load-bearing invariant: the document write, the index
entries it implies, and the commit-log record describing it are committed
in **one storage transaction**, on every backend. Each family uses its
own mechanism:

- **redb** — a single write transaction stages document, index, and
  commit-log table changes and commits them atomically
  (`crates/nimbus-storage/src/store/write/transaction.rs`).
- **SQLite** — the same effects execute inside one `BEGIN IMMEDIATE`
  transaction on the writer connection.
- **Postgres and MySQL** — one SQL transaction encloses the document
  rows, index rows, and the commit-log append; the transaction's `COMMIT`
  is the atomicity point (`crates/nimbus-storage/src/postgres/write.rs`).
- **libSQL replica** — the remote primary runs the same enclosing
  transaction in immediate mode, so atomicity holds at the primary and
  replicas only ever observe complete commits.

Because the commit log is inside the transaction, it is trustworthy as a
history: the engine uses it for optimistic-concurrency conflict checks
and commit fan-out, and recovery can replay it knowing every record
corresponds to a fully applied write. A related guard rides the same
transaction: every backend keeps a scheduled-execution dedup table keyed
by execution id, and a scheduled mutation registers its execution id in
the same transaction as its write — which is what makes scheduled-job
replay after a crash apply-at-most-once
(`crates/nimbus-storage/src/sqlite/write.rs` shows the pattern on the
default backend).

## Index maintenance and lifecycle

Index definitions live in table schemas
(`crates/nimbus-core/src/schema.rs`), and each carries a lifecycle state:
`Pending`, `Backfilling`, `Enabled` (the default), or `Deleting`. Writes
maintain entries for indexes in the `Backfilling` and `Enabled` states —
the `maintained_indexes` set — so an index being backfilled stays
consistent with concurrent writes, while a deleting index stops costing
anything.

Replacing a table's schema rebuilds index state synchronously inside the
same transaction that installs the schema. On redb,
`crates/nimbus-storage/src/schema_store.rs` removes superseded index
keys, scans the table's documents to compute keys for the new maintained
indexes, and writes the schema — one transaction, no window where schema
and indexes disagree. Schema replacement also preserves stable index
identity for definitions that did not change, so an unchanged index is
not needlessly rebuilt. On SQLite, maintained indexes additionally become
native expression indexes over the documents table
(`crates/nimbus-storage/src/sqlite/schema.rs`), letting SQLite's own
planner serve indexed reads.

## Encryption at rest

Encryption covers the local files Nimbus owns; external databases —
Postgres, MySQL, a remote libSQL primary — are encrypted by that
database, not by Nimbus. The operator workflow is documented in
[encryption at rest](/operators/encryption/); architecturally it is an
envelope model implemented in `crates/nimbus-crypto/src/`:

- Each protected database file gets its own random 256-bit data
  encryption key (DEK).
- A key provider wraps the DEK: a master-key-file provider that derives a
  per-file wrapping key via HKDF-SHA256
  (`crates/nimbus-crypto/src/master_key_file.rs`), a
  key-directory provider with one key file per protected subject, or an
  AWS KMS provider (a compile-time feature) that delegates wrapping to
  KMS.
- The wrapped DEK and its metadata live in a `.nimbus-enc` sidecar
  manifest next to the database file
  (`crates/nimbus-crypto/src/manifest.rs`), with the metadata
  bound into the AEAD so a tampered manifest fails to decrypt rather than
  silently misbehaving.

The DEK is applied where each backend can intercept I/O. SQLite tenant
files (and libSQL replica caches) use SQLCipher: the raw 32-byte DEK is
installed via a key pragma before any query, temporary storage is forced
to memory so plaintext never spills, and the key is verified at open
(`crates/nimbus-storage/src/sqlite/encryption.rs`). redb files — tenant
databases and the control-plane database — use an encrypting storage
backend (`crates/nimbus-storage/src/encrypted_redb.rs`) that transforms
every 4096-byte logical page with AES-256-GCM-SIV under a fresh random
nonce, binding the page position and format version into the AAD so
pages cannot be relocated or downgraded undetected.

## The control plane stays local

Cross-tenant state — tenant registry and usage accounting — lives in a
local embedded control-plane database, separate from every tenant store
(`crates/nimbus-engine/src/persistence/control.rs`). It remains on the
server's disk even when tenant data lives in Postgres, MySQL, or a remote
libSQL primary, and it is covered by the same redb page encryption when
encryption is enabled.

## Related pages

- [Engine and the mutation path](/concepts/architecture/engine-mutation-path/)
  — the layer that calls into these providers.
- [Storage backends](/operators/storage-backends/) — flags and connection
  strings for each provider.
- [Encryption at rest](/operators/encryption/) — enabling, rotating, and
  recovering keys.
- [Scaling](/concepts/scaling/) — how backend choice changes the scaling
  story.
