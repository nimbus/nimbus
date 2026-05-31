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
(27/27 parity scenarios) and `aws-sdk-dynamodbstreams`, in both lookup and strict
SigV4 modes, by endpoint override. **JS v3**, **boto3 (Python)**, **AWS CLI**,
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

## Tenant & auth isolation

Two access keys bound to two tenants cannot cross-read, cross-write, list,
TTL-configure, tag, or infer each other's tables — even with identical table
names. Unbound keys and wrong signatures fail closed (`tenant-isolation.md`).
The catalog, TTL, tag, and stream stores are per-tenant; only the access-key
store is global (and maps each key to exactly one tenant).

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
- **Stream sequence counter:** monotonic and persisted; making the counter bump
  atomic under concurrent writers is a follow-up (single-node correct today).
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
