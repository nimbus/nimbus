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
| Storage backend | Configured at the Neovex server level (`NEOVEX_TENANT_PROVIDER`). The MongoDB adapter inherits whatever backend is active. |

## Known Limitations

See [MongoDB adapter hardening plan](../../plans/mongodb-adapter-hardening-plan.md)
for the current coverage and planned work.

## Related Docs

- [MongoDB adapter hardening plan](../../plans/mongodb-adapter-hardening-plan.md)
- [Demo: mongodb/node](../../../demos/mongodb/node/)
- [Convex adapter](../convex.md) (the alternative SDK-based development model)
