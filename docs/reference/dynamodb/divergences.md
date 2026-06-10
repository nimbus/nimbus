---
title: DynamoDB divergences
description: Recorded behavioral differences between the Nimbus DynamoDB endpoint and AWS DynamoDB.
sidebar:
  order: 2
---

Nimbus intentionally differs from AWS DynamoDB in the places listed here.
Every divergence is deliberate, documented, and pinned by a regression test
in the Nimbus codebase, so the behavior is stable — none of these are
accidental gaps. Divergence codes (`DDB-DIV-*`) are stable identifiers and
are not necessarily contiguous.

Everything not listed here follows the DynamoDB contract as exercised by
the official AWS SDK — see
[feature coverage](/reference/dynamodb/feature-coverage/) for the operation
surface and [SDK compatibility](/reference/dynamodb/sdk-compatibility/) for
how that is verified.

## DDB-DIV-001 — Smaller composite primary-key size limit

- **AWS:** partition key up to 2,048 bytes plus sort key up to 1,024 bytes.
- **Nimbus:** the composite key is encoded into a single internal document
  ID capped at 1,500 bytes. The encoding inflates the raw key by roughly a
  third, so the supported combined raw key size is about 1,100 bytes. A key
  whose encoded form exceeds the cap is rejected with
  `ValidationException`.

Most real-world keys are far below this bound. If you rely on
near-maximum-size DynamoDB keys, this is the one hard limit to check before
migrating.

## DDB-DIV-002 — Sort-key ordering uses an order-preserving encoding

- **AWS:** orders sort keys natively by type — numeric for `N`, UTF-8
  byte-wise for `S`, byte-wise for `B`.
- **Nimbus:** encodes each key and index value into an order-preserving
  sortable form: `S` as raw UTF-8, `N` as a full-precision sortable decimal
  encoding, `B` as fixed-case hex. Range conditions and ordering evaluate
  that encoding.

The observable effect is type-correct ordering, including exact numeric
ordering beyond the precision a 64-bit float can represent. Clients see
correctly ordered results; no query changes are needed.

## DDB-DIV-003 — The `_ddb_` table-name prefix is reserved

- **AWS:** allows table names matching `[a-zA-Z0-9_.-]{3,255}`, including a
  `_ddb_` prefix.
- **Nimbus:** rejects user table names beginning with `_ddb_` with
  `ValidationException`. The endpoint stores its own per-tenant metadata
  under that prefix, and reserving it prevents collisions with user tables.

## DDB-DIV-004 — Tables are `ACTIVE` immediately

- **AWS:** `CreateTable` returns `CREATING`; the table becomes `ACTIVE`
  asynchronously and clients poll a waiter.
- **Nimbus:** table creation is synchronous. `CreateTable` returns
  `ACTIVE`, and the first `DescribeTable` already reports `ACTIVE`. SDK
  waiters succeed on their first poll; the `CREATING` state is never
  exposed.

This is strictly faster for clients and requires no code changes.

## DDB-DIV-005 — Item storage format

- **AWS:** items are opaque to the storage engine.
- **Nimbus:** each attribute is stored in its DynamoDB wire form
  (`{"N":"42"}`, `{"SS":[...]}`, and so on) inside Nimbus's shared document
  storage. The representation is exactly lossless — numeric precision,
  sets, binary values, and nesting all survive round trips.

Two consequences:

- `PutItem` overwrite remains a single atomic replace; there is no window
  in which an item is partially written.
- A non-DynamoDB Nimbus surface reading a table created through the
  DynamoDB endpoint sees DynamoDB-tagged JSON rather than plain values.
  Tables created through this endpoint are best treated as owned by it.

## DDB-DIV-006 — One shard per stream

- **AWS:** a stream is a tree of shards that split and merge over the
  table's lifetime; consumers track parent-to-child lineage.
- **Nimbus:** a stream-enabled table exposes exactly one open shard with a
  stable shard ID and no parent.

Every change record is totally ordered in that one shard, so consumers need
no cross-shard ordering or lineage bookkeeping. The shard-iterator API is
unchanged.

## DDB-DIV-007 — Read-triggered stream-record retention

- **AWS:** stream records expire 24 hours after they are written,
  on a background timer.
- **Nimbus:** the 24-hour contract is enforced at read time. `GetRecords`
  never returns a record older than 24 hours, and the storage behind
  expired records is reclaimed on the next poll of that stream. A stream
  that is never polled keeps already-expired records on disk until its next
  poll.

Iterators always advance past expired records, and sequence numbers are
persisted separately, so they stay monotonic for the life of the stream —
reclamation never resets them.

## DDB-DIV-009 — No TTL modification cooldown

- **AWS:** TTL enable/disable is rate-limited (roughly one change per
  hour), and the table passes through asynchronous `ENABLING`/`DISABLING`
  states.
- **Nimbus:** every `UpdateTimeToLive` takes effect immediately, and rapid
  re-toggling succeeds. There is no asynchronous TTL state, consistent with
  tables being `ACTIVE` immediately (DDB-DIV-004).

This makes TTL configuration predictable in tests and migrations without
weakening any correctness guarantee.

## DDB-DIV-010 — `ConsistentRead` on a GSI query is served, not rejected

- **AWS:** rejects `ConsistentRead=true` on a global secondary index with
  `ValidationException`, because AWS GSIs are maintained asynchronously.
- **Nimbus:** accepts the flag and serves the read. Nimbus maintains index
  entries in the same storage transaction as the base write, so every index
  read is already strongly consistent — there is nothing to reject.

A client that never sets the flag is unaffected; a client that does gets a
stronger guarantee instead of an error.

## No provisioned-capacity throttling

Nimbus does not model provisioned read/write capacity, so requests are
never throttled against configured capacity units. Retry logic written for
AWS capacity throttling is harmless — it simply never triggers for
capacity reasons.
