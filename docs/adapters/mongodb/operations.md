# Operations & Configuration

## Supported Operations

| Category | Commands |
|---|---|
| CRUD | `insert`, `find`, `update`, `delete` |
| Cursors | `getMore`, `killCursors` (paginated result iteration) |
| Aggregation | `aggregate` pipeline (`$match`, `$project`, `$group`, `$sort`, `$limit`, `$skip`, etc.) |
| Indexes | `createIndexes`, `dropIndexes`, `listIndexes` |
| Sessions | `startSession`, `commitTransaction`, `abortTransaction` |
| Change streams | `watch()` for real-time subscription to document changes |
| Admin | `hello`, `ping`, `buildInfo`, `listDatabases`, `listCollections` |
| Auth | `saslStart`, `saslContinue` (SCRAM-SHA-256 handshake) |

## Configuration

| Setting | Description |
|---|---|
| Authentication | SCRAM-SHA-256 with configurable credentials. |
| Storage backend | Configured at the Nimbus server level (`NIMBUS_TENANT_PROVIDER`). The MongoDB adapter inherits whatever backend is active. |

## Storage Semantics

MongoDB CRUD, find, transaction, index, and change-stream operations share the
engine-owned Nimbus storage path. The adapter does not keep an adapter-local
transaction log or change log. Committed writes update latest document rows,
index effects, MVCC version rows, and the tenant event journal atomically.

Current MongoDB reads and cursors remain current-state protocol operations.
Change streams are backed by the shared journal/CDC posture, and transaction
sessions stage pending writes in the engine so read-your-writes and conflict
checks stay consistent with the native and Firebase paths.

## Known Limitations

See [MongoDB adapter hardening plan](../../plans/archive/mongodb-adapter-hardening-plan.md)
for the current coverage and planned work.

## Related Docs

- [MongoDB adapter hardening plan](../../plans/archive/mongodb-adapter-hardening-plan.md)
- [Demo: mongodb/node](../../../demos/mongodb/node/)
