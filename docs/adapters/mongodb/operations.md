# Operations & Configuration

## Supported Operations

| Category | Commands |
|---|---|
| CRUD | `insert`, `find`, `update`, `delete` |
| Cursors | `getMore`, `killCursors` (paginated result iteration) |
| Aggregation | `aggregate` pipeline (`$match`, `$project`, `$group`, `$sort`, `$limit`, `$skip`, etc.) |
| Indexes | `createIndexes`, `dropIndexes`, `listIndexes` |
| Sessions | `startSession`, `commitTransaction`, `abortTransaction` |
| Admin | `hello`, `ping`, `buildInfo`, `listDatabases`, `listCollections` |
| Auth | `saslStart`, `saslContinue` (SCRAM-SHA-256 handshake) |

## Configuration

| Setting | Description |
|---|---|
| Authentication | SCRAM-SHA-256 with configurable credentials. |
| Storage backend | Configured at the Nimbus server level (`NIMBUS_TENANT_PROVIDER`). The MongoDB adapter inherits whatever backend is active. |

## Storage Semantics

MongoDB CRUD, find, transaction, and index operations share the engine-owned
Nimbus storage path. The adapter does not keep an adapter-local transaction log
or change log. Committed writes update latest document rows, index effects,
MVCC version rows, and the tenant event journal atomically.

Current MongoDB reads and cursors remain current-state protocol operations.
MongoDB `watch()` / `$changeStream` is not exposed until it can be backed by the
same durable tenant journal cut model as Nimbus CDC. `$changeStream` requests
fail closed with `CommandNotSupported` instead of returning adapter-local
subscription diffs with weaker resume guarantees. Transaction sessions stage
pending writes in the engine so read-your-writes and conflict checks stay
consistent with the native and Firebase paths.

## Known Limitations

See [MongoDB adapter hardening plan](../../plans/archive/mongodb-adapter-hardening-plan.md)
for the current coverage and planned work.

- Change streams are not supported yet. They must be implemented against the
  SEQ changefeed bootstrap and `ChangefeedCursor` model before this adapter can
  claim MongoDB `watch()` semantics.

## Related Docs

- [MongoDB adapter hardening plan](../../plans/archive/mongodb-adapter-hardening-plan.md)
- [Demo: mongodb/node](../../../demos/mongodb/node/)
