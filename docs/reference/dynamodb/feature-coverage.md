---
title: DynamoDB feature coverage
description: Every DynamoDB operation the Nimbus endpoint supports, with its request surface, errors, and pagination behavior.
sidebar:
  order: 1
---

The Nimbus DynamoDB endpoint implements the full operation set below,
grouped into tiers T0–T7. Requests are dispatched on the standard
`X-Amz-Target` header (`DynamoDB_20120810.*` and
`DynamoDBStreams_20120810.*`); an unrecognized target is rejected with an
explicit error.

Status values:

- **Implemented** — supported end to end.
- **Implemented, with divergence** — supported, with a recorded behavioral
  difference from AWS DynamoDB. Each `DDB-DIV-*` code is documented on the
  [divergences](/reference/dynamodb/divergences/) page.

## T0 — Control plane

| Operation | Request surface | Response surface | Errors | Status |
| --- | --- | --- | --- | --- |
| `CreateTable` | `TableName`, `KeySchema`, `AttributeDefinitions`, `BillingMode`, `StreamSpecification`, `LocalSecondaryIndexes`, `GlobalSecondaryIndexes` | `TableDescription` with `TableStatus`, `TableArn`, `TableId` | `ResourceInUseException`, `ValidationException` | Implemented, with divergence (DDB-DIV-004: tables are `ACTIVE` immediately) |
| `DescribeTable` | `TableName` | `Table` | `ResourceNotFoundException` | Implemented |
| `ListTables` | `ExclusiveStartTableName`, `Limit` | `TableNames`, `LastEvaluatedTableName`; paginated | — | Implemented |
| `UpdateTable` | `TableName`, `BillingMode`, `StreamSpecification`, GSI updates, deletion protection | `TableDescription` | `ResourceNotFoundException`, `ValidationException` | Implemented |
| `DeleteTable` | `TableName` | `TableDescription` (`DELETING`) | `ResourceNotFoundException` | Implemented |
| `DescribeEndpoints` | — | `Endpoints` | — | Implemented |
| `DescribeLimits` | — | account and table capacity maxima | — | Implemented |

## T1 — Single-item

| Operation | Request surface | Response surface | Errors | Status |
| --- | --- | --- | --- | --- |
| `PutItem` | `TableName`, `Item`, `ConditionExpression`, `ExpressionAttributeNames`/`Values`, `ReturnValues` | `Attributes` | `ConditionalCheckFailedException`, `ResourceNotFoundException`, `ValidationException` | Implemented, with divergence (DDB-DIV-005: storage format) |
| `GetItem` | `TableName`, `Key`, `ProjectionExpression`, `ConsistentRead` | `Item` | `ResourceNotFoundException`, `ValidationException` | Implemented |
| `UpdateItem` | `TableName`, `Key`, `UpdateExpression`, `ConditionExpression`, `ReturnValues` | `Attributes` | `ConditionalCheckFailedException`, `ResourceNotFoundException`, `ValidationException` | Implemented |
| `DeleteItem` | `TableName`, `Key`, `ConditionExpression`, `ReturnValues` | `Attributes` | `ConditionalCheckFailedException`, `ResourceNotFoundException`, `ValidationException` | Implemented |

Conditional writes are checked and committed in one storage transaction, so
a condition cannot be raced between its check and its commit.

## T2 — Query and scan

| Operation | Request surface | Response surface | Errors | Status |
| --- | --- | --- | --- | --- |
| `Query` | `TableName`, `KeyConditionExpression`, `FilterExpression`, `ProjectionExpression`, `IndexName`, `Limit`, `ExclusiveStartKey`, `ScanIndexForward`, `Select` | `Items`, `Count`, `ScannedCount`, `LastEvaluatedKey`; paginated | `ResourceNotFoundException`, `ValidationException` | Implemented, with divergence (DDB-DIV-002: sort-key encoding) |
| `Scan` | `TableName`, `FilterExpression`, `ProjectionExpression`, `IndexName`, `Limit`, `ExclusiveStartKey`, `Segment`, `TotalSegments`, `Select` | `Items`, `Count`, `ScannedCount`, `LastEvaluatedKey`; paginated, parallel scan supported | `ResourceNotFoundException`, `ValidationException` | Implemented |

## T3 — Batch and transactions

| Operation | Request surface | Response surface | Errors | Status |
| --- | --- | --- | --- | --- |
| `BatchGetItem` | `RequestItems` with `Keys`, `ProjectionExpression`, `ConsistentRead` | `Responses`, `UnprocessedKeys` | `ResourceNotFoundException`, `ValidationException` | Implemented |
| `BatchWriteItem` | `RequestItems` with `PutRequest`/`DeleteRequest` | `UnprocessedItems` | `ResourceNotFoundException`, `ValidationException` | Implemented |
| `TransactGetItems` | `TransactItems` (`Get`) | `Responses` (snapshot read) | `ResourceNotFoundException`, `ValidationException`, `TransactionCanceledException` | Implemented |
| `TransactWriteItems` | `TransactItems` (`Put`/`Update`/`Delete`/`ConditionCheck`), `ClientRequestToken` | empty on success; `CancellationReasons` on cancellation | `TransactionCanceledException`, `ValidationException` | Implemented |

`TransactWriteItems` is atomic: either every item commits or none does, and
a cancellation reports per-item `CancellationReasons`.

## T4 — Secondary indexes

| Capability | Coverage | Errors | Status |
| --- | --- | --- | --- |
| Local secondary indexes | Declared at `CreateTable`, with projections | `ValidationException` | Implemented |
| Global secondary indexes | Created, updated, and deleted through `UpdateTable` (`GlobalSecondaryIndexUpdates`) | `ValidationException`, `ResourceInUseException` | Implemented |
| `Query` with `IndexName` | LSI and GSI queries honoring the projected attribute set | `ResourceNotFoundException`, `ValidationException` | Implemented, with divergence (DDB-DIV-010: `ConsistentRead` on a GSI is served, not rejected) |

Index entries are maintained in the same storage transaction as the base
write, so index reads are never stale.

## T5 — Streams

| Operation | Request surface | Response surface | Errors | Status |
| --- | --- | --- | --- | --- |
| `StreamSpecification` on `CreateTable`/`UpdateTable` | `StreamEnabled`, `StreamViewType` | `LatestStreamArn`, `LatestStreamLabel` | `ValidationException` | Implemented |
| `DescribeStream` | `StreamArn`, `Limit`, `ExclusiveStartShardId` | `StreamDescription` | `ResourceNotFoundException` | Implemented, with divergence (DDB-DIV-006: single shard) |
| `GetShardIterator` | `StreamArn`, `ShardId`, `ShardIteratorType`, `SequenceNumber` | `ShardIterator` | `ResourceNotFoundException`, `ValidationException` | Implemented |
| `GetRecords` | `ShardIterator`, `Limit` | `Records`, `NextShardIterator` | expired-iterator and validation errors | Implemented, with divergence (DDB-DIV-007: read-triggered retention) |
| `ListStreams` | `TableName`, `Limit`, `ExclusiveStartStreamArn` | `Streams`, `LastEvaluatedStreamArn`; paginated | `ResourceNotFoundException` | Implemented |

Batch and transactional writes emit stream records like single-item writes;
a transaction's records are part of its atomic commit. TTL deletions emit
`REMOVE` records.

## T6 — TTL and tagging

| Operation | Request surface | Response surface | Errors | Status |
| --- | --- | --- | --- | --- |
| `UpdateTimeToLive` | `TableName`, `TimeToLiveSpecification` | `TimeToLiveSpecification` | `ResourceNotFoundException`, `ValidationException` | Implemented, with divergence (DDB-DIV-009: no modification cooldown) |
| `DescribeTimeToLive` | `TableName` | `TimeToLiveDescription` | `ResourceNotFoundException` | Implemented |
| `TagResource` | `ResourceArn`, `Tags` | empty | `ResourceNotFoundException`, `ValidationException` | Implemented |
| `UntagResource` | `ResourceArn`, `TagKeys` | empty | `ResourceNotFoundException`, `ValidationException` | Implemented |
| `ListTagsOfResource` | `ResourceArn`, `NextToken` | `Tags`; single page | `ResourceNotFoundException`, `ValidationException` | Implemented |

Expired items are reclaimed by a background TTL sweeper (default cadence
60 seconds, configurable or disablable on the endpoint configuration).

## T7 — Authentication

| Capability | Behavior | Errors | Status |
| --- | --- | --- | --- |
| Access-key → tenant resolution | Each access key ID is bound to exactly one tenant; unknown keys fail closed | `UnrecognizedClientException`, `MissingAuthenticationToken`, `IncompleteSignature` | Implemented |
| Strict SigV4 verification (default) | Full canonical-request verification with constant-time comparison and a ±15-minute timestamp window | `InvalidSignatureException`, `UnrecognizedClientException` | Implemented |
| Persisted access-key management | Keys can be stored, rotated, deleted, and listed in a server-side store; listings never include the secret | `ResourceNotFoundException` | Implemented |

## Not modeled

Nimbus has no provisioned-capacity model: requests are not throttled
against configured read/write capacity units, so capacity-driven retry
loops written for AWS simply never trigger. See
[divergences](/reference/dynamodb/divergences/) for the complete list of
recorded behavioral differences.
