# DynamoDB Adapter — Enterprise-Readiness Closeout

The `nimbus-dynamodb` adapter exposes a DynamoDB-compatible HTTP surface
(JSON-1.0) on a dedicated listener. This document summarizes its feature
coverage, SDK compatibility, reliability proofs, isolation guarantees,
performance baseline, known divergences, and operational limits for teams
evaluating it as a DynamoDB replacement.

## Feature coverage

The full DynamoDB tier set T0–T7 is implemented and proven through the official
AWS SDK:

- **T0 control plane** — CreateTable, DescribeTable, ListTables, UpdateTable,
  DeleteTable, DescribeEndpoints, DescribeLimits.
- **T1 single-item** — PutItem, GetItem, UpdateItem, DeleteItem (with
  ConditionExpression, UpdateExpression, ReturnValues).
- **T2 query/scan** — Query + Scan with KeyCondition/Filter/Projection
  expressions, pagination (ExclusiveStartKey/LastEvaluatedKey), parallel scan.
- **T3 batch/transact** — BatchGetItem, BatchWriteItem, TransactGetItems,
  TransactWriteItems (atomic, with CancellationReasons).
- **T4 secondary indexes** — LSI + GSI create/update/query with projection.
- **T5 streams** — StreamSpecification, DescribeStream, GetShardIterator,
  GetRecords, ListStreams (with TTL-attributed REMOVE records).
- **T6 TTL + tagging** — Update/DescribeTimeToLive + a background sweeper;
  TagResource/UntagResource/ListTagsOfResource.
- **T7 auth** — access-key→tenant lookup, strict SigV4 verification, and a
  persisted, rotatable access-key store.

Per-operation fields, exceptions, and test lanes: `feature-coverage.md`.

## SDK compatibility

Every supported operation works through the official **Rust** `aws-sdk-dynamodb`
(27/27 parity scenarios) and `aws-sdk-dynamodbstreams` under the default strict
SigV4 verification (the parity fixtures bind the SDK's secret and verify every
signature end to end), by endpoint override. **JS v3**, **boto3 (Python)**, **AWS CLI**,
and **Java v2** are wired the same way (endpoint override + access key);
their lanes are recorded in `sdk-compatibility.md`. No supported operation fails
through an official SDK due to protocol drift.

## Reliability

- **Fail-closed:** malformed input, unknown operations, bad/oversize keys,
  missing/unbound credentials, condition-failed transactions, and bad
  signatures all map to modeled 4xx errors — **0 panics, 0 unhandled 5xx, no
  partial-success envelopes** (`failure-injection.md`).
- **Soak:** a 2620-operation mixed workload (reads/writes/conditional
  writes/queries/metadata/auth failures) ran with **0 panics, 0 task leaks, 0
  unhandled 5xx** (`soak.md`).
- **Shared storage guarantees:** DynamoDB single-item, batch, transaction, TTL,
  and stream-visible writes lower through the same Nimbus engine/storage path
  as native and Firebase requests. A committed write updates latest document
  rows, index effects, MVCC version rows, and the tenant event journal
  atomically; stream records are derived from that shared durable history rather
  than an adapter-local log.

## Tenant & auth isolation

Two access keys bound to two tenants cannot cross-read, cross-write, list,
TTL-configure, tag, or infer each other's tables — even with identical table
names. Unbound keys and wrong signatures fail closed (`tenant-isolation.md`).
The catalog, TTL, tag, and stream stores are per-tenant; only the access-key
store is global (and maps each key to exactly one tenant).

## Transport security & authentication posture

- **Strict SigV4 is the default.** `AuthMode::Strict` verifies the full SigV4
  signature (canonical request, derived signing key, constant-time compare) and
  rejects requests outside the ±15-minute timestamp window. Bind production keys
  with `DynamoDbConfig::with_signed_access_key`.
- **Request body is signature-bound.** Strict verification rejects a request
  unless `x-amz-content-sha256` equals `sha256(body)`, so the body cannot be
  tampered with under a captured signature (and `UNSIGNED-PAYLOAD` is refused for
  DynamoDB operations).
- **The lookup escape hatch is loopback-only.** `DynamoDbConfig::insecure_dev_auth`
  skips signature verification for local development; the server **refuses to
  bind it to a non-loopback address**. Never expose it on a routable interface.
- **TLS termination is required in production.** SigV4 provides request
  authentication and integrity but **not confidentiality** — the listener speaks
  plaintext HTTP. Deploy it behind a TLS-terminating proxy (or mesh) so
  credentials and payloads are encrypted in transit. SigV4 also has no per-request
  nonce: its only replay bound is the ±15-minute clock window, which TLS plus a
  trusted network boundary are expected to backstop.
- **Request-body cap.** The listener caps request bodies at 16 MiB (aligned to
  DynamoDB's 400 KB item × 25-item `BatchWriteItem` limits) and returns
  `413 Payload Too Large` before buffering or parsing — an oversized payload
  cannot force a large pre-authentication allocation.
- **Credential store.** Access keys never bind to a reserved Nimbus-internal
  tenant (`_nimbus_*`) — both the `put_access_key` write path and the request
  resolution path refuse it, so a request can't pivot to the global access-key
  catalog. `list_access_keys` returns a secret-free `RedactedAccessKey` view; the
  secret access key is never read back over a listing surface. At rest, the
  access-key documents ride the platform `LocalEncryptionConfig` envelope
  encryption like all other data — **enable it in production** (or use an
  external database with its own at-rest encryption); the adapter adds no bespoke
  secret cipher.

## Performance baseline

In-process p50 latency on an Apple M2 Max (full table in
`performance-baseline.md`): GetItem ~6µs, BatchGetItem ~6µs, GetRecords ~18µs,
TransactWriteItems ~31µs, Scan ~50µs, Query ~132µs, and the durable-commit write
families (PutItem/UpdateItem/BatchWriteItem) ~740µs p50. Non-regression
thresholds are set at 2× p99.

## Known divergences

Ten recorded, tested divergences (`divergences.md`): DDB-DIV-001 (1500-byte key
cap), -002 (sortable key projection), -003 (`_ddb_` reserved prefix), -004
(tables ACTIVE immediately), -005 (wire-JSON item storage), -006 (single stream
shard), -007 (read-triggered stream retention), -008 (TTL attribute charset),
-009 (no TTL modification cooldown), -010 (GSI ConsistentRead served, not
rejected). Each carries a regression test and is classified in the parity report.

## Operational limits & deferred work

- **Throttling:** Nimbus never emits `ProvisionedThroughputExceededException`
  (no provisioned-capacity model).
- **Streams:** one shard per stream; record retention is reclaimed on read.
  BatchWriteItem and TransactWriteItems emit stream records like the single-item
  writes; a transaction's events are folded into its atomic commit.
- **Stream sequence counter:** monotonic and persisted; the event and the
  advanced high-water counter are written in a single atomic batch (the event
  uses `Create` mode keyed by its sequence, so a concurrent writer that claims a
  number first loses the commit and retries), giving gap-free, no-duplicate
  sequence numbers under concurrency.
- **TTL sweeper:** read/timer-driven reclamation; a never-polled, never-swept
  stream retains expired records until its next poll.
- **Ground-truth parity lanes** (DynamoDB Local, ExtendDB) are Docker/Postgres
  gated; their run status and next actions are recorded in
  `compatibility-suites.md` and the parity report rather than skipped silently.

## Proof index

- `feature-coverage.md`, `sdk-compatibility.md`, `divergences.md`,
  `compatibility-suites.md` (this directory)
- `docs/plans/proof/dynamodb-adapter/`: `parity-classification.md`,
  `failure-injection.md`, `tenant-isolation.md`, `soak.md`,
  `performance-baseline.md`
- Tests: `crates/nimbus-server/tests/dynamodb_spec` (27 SDK scenarios),
  `crates/nimbus-dynamodb/tests/` (failure-injection, tenant-isolation, soak),
  `crates/nimbus-server/src/tests/dynamodb_wire.rs` (5 harness cases),
  `crates/nimbus-dynamodb/benches/operations.rs` (latency baseline).
