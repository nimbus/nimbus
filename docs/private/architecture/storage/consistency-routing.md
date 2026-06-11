# Consistency Routing

Nimbus routes reads according to the guarantees a backend actually exposes. It
does not add fake replica layers to satisfy an adapter label.

## Backend Contract

| Backend | Strong read path | Eventual read path |
| --- | --- | --- |
| redb | local authoritative store | not_applicable |
| SQLite | local authoritative store | not_applicable |
| Postgres | tenant schema primary connection | not_applicable today |
| MySQL | tenant database primary connection | not_applicable today |
| libSQL replica | remote primary for writes plus freshness barrier/poll catch-up | local replica cache after refresh/freshness proof |

The `not_applicable` entries are intentional. A backend that has no separate
read surface must use the authoritative path for all adapter promises.

## Adapter Contract

| Adapter | Strong operations | Eventual operations |
| --- | --- | --- |
| Convex | mutations, actions that perform writes, query reads after required visibility | subscriptions may reuse committed snapshots; no fake replica routing |
| Firebase/Firestore | commits, transactions, consistency-selector reads that require strong state | listen resume/cached target replay may use retained snapshots after sequence proof |
| Cloud Functions | invocation-visible reads and writes | not_applicable |
| MongoDB | command reads/writes served by Nimbus state | not_applicable |
| Native HTTP/WebSocket | direct mutations and queries | embedded replica/debug surfaces only when explicitly selected |

Adapters ask the engine/storage layer for the required consistency class.
Storage decides whether there is a real eventual path. If there is not, the
request stays on the authoritative path.
