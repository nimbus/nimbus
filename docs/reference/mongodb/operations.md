---
title: MongoDB operations
description: The exact command, filter, update, and aggregation surface of the Nimbus MongoDB endpoint.
sidebar:
  order: 2
---

Served by default on `127.0.0.1:27017` (loopback only); opt out with
`--no-mongodb`; SCRAM credentials.

The Nimbus MongoDB endpoint implements a defined subset of the MongoDB
command surface. Anything outside this page is rejected with an explicit
error — an unknown command returns `CommandNotFound` (code 59), and an
unsupported operator inside a supported command returns an error naming the
operator. Nothing is silently ignored.

## Commands

### Handshake and diagnostics — no authentication required

| Command | Notes |
| --- | --- |
| `hello`, `isMaster` | Reports a standalone, writable server |
| `buildInfo` | Reports server version `7.0.0` |
| `ping` | |
| `whatsmyuri` | |
| `getParameter` | |
| `serverStatus` | |
| `connectionStatus` | |
| `getCmdLineOpts` | |
| `getFreeMonitoringStatus` | |
| `getLog` | |

### Authentication

| Command | Notes |
| --- | --- |
| `saslStart` | SCRAM-SHA-256 only; other mechanisms are rejected |
| `saslContinue` | |

Every command below this point requires an authenticated connection.

### CRUD

| Command | Notes |
| --- | --- |
| `insert` | Creates the collection (and tenant) on first use |
| `find` | See [filter operators](#filter-operators) |
| `update` | See [update operators](#update-operators); supports upsert |
| `delete` | |
| `findAndModify` | |
| `count` | |
| `distinct` | |

### Aggregation

| Command | Notes |
| --- | --- |
| `aggregate` | See [aggregation pipeline](#aggregation-pipeline); `$changeStream` pipelines are rejected with `CommandNotSupported` |

### Collections and databases

| Command | Notes |
| --- | --- |
| `create` | |
| `drop` | |
| `listCollections` | |
| `listDatabases` | |

### Indexes

| Command | Notes |
| --- | --- |
| `createIndexes` | |
| `dropIndexes` | |
| `listIndexes` | |

### Cursors

| Command | Notes |
| --- | --- |
| `getMore` | |
| `killCursors` | |

### Sessions and transactions

| Command | Notes |
| --- | --- |
| `startSession` | At most 128 concurrent sessions per connection |
| `endSessions` | |
| `refreshSessions` | |
| `commitTransaction` | A conflicting concurrent write fails the commit with `WriteConflict` |
| `abortTransaction` | |

Transactions are started with the standard `startTransaction: true` field on
the first operation in the session. A transaction is scoped to one database
(one Nimbus tenant).

## Filter operators

Supported in `find`, `update`, `delete`, `findAndModify`, `count`, and
`$match`:

| Operator | Meaning |
| --- | --- |
| implicit equality | `{ field: value }` |
| `$eq` | equals |
| `$ne` | not equals |
| `$gt` | greater than |
| `$gte` | greater than or equal |
| `$lt` | less than |
| `$lte` | less than or equal |

All other filter operators — including `$in`, `$nin`, `$or`, `$and`,
`$not`, `$nor`, `$regex`, and `$exists` — are rejected with a `BadValue`
error naming the operator. Top-level `$`-prefixed keys in a filter are
likewise rejected.

## Update operators

| Operator | Notes |
| --- | --- |
| `$set` | |
| `$unset` | |
| `$rename` | |
| `$setOnInsert` | Applied only on upsert-insert |
| `$currentDate` | |
| `$inc` | |
| `$min` | |
| `$max` | |
| `$mul` | |
| `$addToSet` | Supports `$each` |
| `$push` | Supports `$each` |
| `$pull` | |
| `$pullAll` | |
| `$pop` | |
| `$bit` | |

## Aggregation pipeline

### Stages

| Stage | Notes |
| --- | --- |
| `$match` | Same operator surface as [filter operators](#filter-operators) |
| `$sort` | |
| `$limit` | |
| `$skip` | |
| `$project` | |
| `$addFields` | |
| `$count` | |
| `$group` | See accumulators below |
| `$unwind` | |

An unrecognized stage fails the pipeline with an error naming the stage.
`$changeStream` is rejected with `CommandNotSupported` — change streams are
not available through the MongoDB endpoint.

### `$group` accumulators

| Accumulator |
| --- |
| `$sum` |
| `$avg` |
| `$min` |
| `$max` |
| `$first` |
| `$last` |
| `$push` |
| `$addToSet` |

## Server identity and limits

| Property | Value |
| --- | --- |
| Reported server version | `7.0.0` |
| Wire protocol versions | 0–21 (`OP_MSG`) |
| Max BSON document size | 16,777,216 bytes (16 MiB) |
| Max message size | 48,000,000 bytes |
| Max write batch size | 100,000 operations |
| Logical session timeout | 30 minutes |
| Network binding | Loopback only; non-loopback binds are refused |
