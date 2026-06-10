# DynamoDB Adapter — Feature Coverage (T0–T7)

Every DynamoDB operation the adapter recognizes, its modeled request/response
surface, modeled exceptions, pagination + idempotency shape, status, and the
test lane that proves it. Statuses:

- **implemented** — handled end-to-end and proven through the official SDK.
- **classified-divergence** — implemented, but with a recorded, tested behavior
  difference (see `divergences.md`); the operation still works.
- **unsupported-deferred** — recognized but intentionally not yet handled.

Every recognized operation has a row; every **implemented** row names a test
lane (no untested implemented operation). The behavior divergences live in
`divergences.md` (DDB-DIV-001…009) and the per-scenario parity verdicts in
`docs/plans/proof/dynamodb-adapter/parity-classification.md`.

Test-lane key: `spec` = `crates/nimbus-server/tests/dynamodb_spec` (official AWS
Rust SDK parity runner); `unit` = `crates/nimbus-dynamodb/src/commands/*` unit
tests; `harness` = `crates/nimbus-server/src/tests/dynamodb_wire.rs`.

## T0 — control plane

| Operation | Key request fields | Key response fields | Modeled exceptions | Pagination | Idempotency | Status | Test lane |
| --- | --- | --- | --- | --- | --- | --- | --- |
| CreateTable | TableName, KeySchema, AttributeDefinitions, BillingMode, StreamSpecification, LSI, GSI | TableDescription (TableStatus, TableArn, TableId, …) | ResourceInUseException, ValidationException | — | TableName key | classified-divergence (DDB-DIV-004 ACTIVE-immediately) | spec `control_plane_roundtrip`, harness |
| DescribeTable | TableName | Table (TableDescription) | ResourceNotFoundException | — | — | implemented | spec, harness |
| ListTables | ExclusiveStartTableName, Limit | TableNames, LastEvaluatedTableName | — | ExclusiveStartTableName→LastEvaluatedTableName | — | implemented | spec, harness |
| UpdateTable | TableName, BillingMode, StreamSpecification, GSI updates, DeletionProtection | TableDescription | ResourceNotFoundException, ValidationException | — | — | implemented | spec `control_plane_roundtrip`, unit |
| DeleteTable | TableName | TableDescription (DELETING) | ResourceNotFoundException | — | — | implemented | spec, harness |
| DescribeEndpoints | — | Endpoints (Address, CachePeriodInMinutes) | — | — | — | implemented | unit (`commands::discovery`) |
| DescribeLimits | — | Account/Table Max R/W CapacityUnits | — | — | — | implemented | spec, harness(listener) |

## T1 — single-item

| Operation | Key request fields | Key response fields | Modeled exceptions | Pagination | Idempotency | Status | Test lane |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PutItem | TableName, Item, ConditionExpression, ExpressionAttribute{Names,Values}, ReturnValues | Attributes | ConditionalCheckFailedException, ResourceNotFoundException, ValidationException | — | conditional-write preconditions | classified-divergence (DDB-DIV-005 wire-JSON storage) | spec, unit, harness |
| GetItem | TableName, Key, ProjectionExpression, ConsistentRead | Item | ResourceNotFoundException, ValidationException | — | — | implemented | spec, unit, harness |
| DeleteItem | TableName, Key, ConditionExpression, ReturnValues | Attributes | ConditionalCheckFailedException, ResourceNotFoundException, ValidationException | — | conditional preconditions | implemented | spec, unit, harness |
| UpdateItem | TableName, Key, UpdateExpression, ConditionExpression, ReturnValues | Attributes | ConditionalCheckFailedException, ResourceNotFoundException, ValidationException | — | conditional preconditions | implemented | spec, unit, harness |

## T2 — query / scan

| Operation | Key request fields | Key response fields | Modeled exceptions | Pagination | Idempotency | Status | Test lane |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Query | TableName, KeyConditionExpression, FilterExpression, ProjectionExpression, IndexName, Limit, ExclusiveStartKey, ScanIndexForward, Select | Items, Count, ScannedCount, LastEvaluatedKey | ResourceNotFoundException, ValidationException | ExclusiveStartKey→LastEvaluatedKey | — | classified-divergence (DDB-DIV-002 sortable projection) | spec, unit, harness |
| Scan | TableName, FilterExpression, ProjectionExpression, IndexName, Limit, ExclusiveStartKey, Segment, TotalSegments, Select | Items, Count, ScannedCount, LastEvaluatedKey | ResourceNotFoundException, ValidationException | ExclusiveStartKey→LastEvaluatedKey; parallel Segment/TotalSegments | — | implemented | spec, unit, harness |

## T3 — batch / transact

| Operation | Key request fields | Key response fields | Modeled exceptions | Pagination | Idempotency | Status | Test lane |
| --- | --- | --- | --- | --- | --- | --- | --- |
| BatchGetItem | RequestItems (Keys, ProjectionExpression, ConsistentRead) | Responses, UnprocessedKeys | ResourceNotFoundException, ValidationException | UnprocessedKeys re-drive | — | implemented | spec, unit |
| BatchWriteItem | RequestItems (PutRequest/DeleteRequest) | UnprocessedItems | ResourceNotFoundException, ValidationException | UnprocessedItems re-drive | — | implemented | spec, unit |
| TransactGetItems | TransactItems (Get) | Responses | ResourceNotFoundException, ValidationException, TransactionCanceledException | — | snapshot read | implemented | spec, unit |
| TransactWriteItems | TransactItems (Put/Update/Delete/ConditionCheck), ClientRequestToken | (empty) / CancellationReasons | TransactionCanceledException (+ CancellationReasons), ValidationException | — | ClientRequestToken; condition preconditions | implemented | spec, unit, harness |

## T4 — secondary indexes

| Operation | Coverage | Modeled exceptions | Status | Test lane |
| --- | --- | --- | --- | --- |
| CreateTable LSI | LocalSecondaryIndexes with projection | ValidationException | implemented | spec `local_secondary_index_query` |
| UpdateTable GSI (create/update/delete) | GlobalSecondaryIndexUpdates | ValidationException, ResourceInUseException | implemented | spec `global_secondary_index_crud` |
| Query on index (IndexName) | LSI + GSI query honoring the projected attribute set | ResourceNotFoundException, ValidationException | implemented | spec `gsi_query_projection`, unit |

## T5 — streams

| Operation | Key request fields | Key response fields | Modeled exceptions | Pagination | Status | Test lane |
| --- | --- | --- | --- | --- | --- | --- |
| (StreamSpecification on Create/Update) | StreamEnabled, StreamViewType | LatestStreamArn, LatestStreamLabel | ValidationException | — | implemented | spec `stream_specification` |
| DescribeStream | StreamArn, Limit, ExclusiveStartShardId | StreamDescription (Shards, StreamStatus, StreamViewType) | ResourceNotFoundException | shard pagination (single shard) | classified-divergence (DDB-DIV-006 single shard) | spec, unit, harness |
| GetShardIterator | StreamArn, ShardId, ShardIteratorType, SequenceNumber | ShardIterator | ResourceNotFoundException, ValidationException | iterator | implemented | spec, unit |
| GetRecords | ShardIterator, Limit | Records (eventName, dynamodb, userIdentity), NextShardIterator | ExpiredIteratorException-shape, ValidationException | NextShardIterator | classified-divergence (DDB-DIV-007 read-triggered retention) | spec, unit, harness |
| ListStreams | TableName, Limit, ExclusiveStartStreamArn | Streams, LastEvaluatedStreamArn | ResourceNotFoundException | ExclusiveStartStreamArn→LastEvaluatedStreamArn | implemented | spec, unit |

## T6 — TTL / tagging

| Operation | Key request fields | Key response fields | Modeled exceptions | Status | Test lane |
| --- | --- | --- | --- | --- | --- |
| UpdateTimeToLive | TableName, TimeToLiveSpecification (Enabled, AttributeName) | TimeToLiveSpecification | ResourceNotFoundException, ValidationException | classified-divergence (DDB-DIV-008 charset, DDB-DIV-009 no cooldown) | spec, unit |
| DescribeTimeToLive | TableName | TimeToLiveDescription (Status, AttributeName) | ResourceNotFoundException | implemented | spec, unit |
| TagResource | ResourceArn, Tags | (empty) | ResourceNotFoundException, ValidationException | implemented | spec, unit |
| UntagResource | ResourceArn, TagKeys | (empty) | ResourceNotFoundException, ValidationException | implemented | spec, unit |
| ListTagsOfResource | ResourceArn, NextToken | Tags, NextToken | ResourceNotFoundException, ValidationException | single page (NextToken always null) | implemented | spec, unit |

## T7 — auth / SigV4

| Capability | Behavior | Modeled exceptions | Status | Test lane |
| --- | --- | --- | --- | --- |
| Access-key → tenant resolution | Lookup mode (default) | UnrecognizedClientException, MissingAuthenticationToken, IncompleteSignature | implemented | spec, unit |
| SigV4 strict verification | Canonical request + derived key + constant-time compare; ±15-min window | InvalidSignatureException, UnrecognizedClientException (expired) | implemented | spec `strict_mode_*`, unit |
| Persisted access-key management | put/rotate/delete/lookup/list in the system-tenant store | ResourceNotFoundException | implemented | spec `persisted_signed_key_*`, unit (`key_management`) |

## Unsupported / deferred

The adapter never emits `ProvisionedThroughputExceededException` (Nimbus does
not model provisioned-capacity throttling) — recorded as a deferred divergence
in `divergences.md`'s notes. No other recognized operation is deferred: the full
T0–T7 surface above is **implemented**.
