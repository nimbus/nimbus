---
title: MongoDB tenant isolation
description: How MongoDB database and collection names map onto Nimbus tenants and tables.
sidebar:
  order: 3
---

The MongoDB endpoint has no database catalog of its own. MongoDB names map
directly onto Nimbus's tenant and table model.

## Name mapping

| MongoDB concept | Nimbus concept |
| --- | --- |
| Database name | Tenant ID |
| Collection name | Table name within the tenant |
| Document | Document within the table |

| Database name in the command | Resolved tenant |
| --- | --- |
| Any ordinary name, e.g. `myapp` | The tenant `myapp` |
| `admin`, `local`, `config` | The tenant `default` |
| No database specified | The tenant `default` |

The reserved MongoDB system database names (`admin`, `local`, `config`) are
folded into `default` so that driver bookkeeping commands — which address
`admin` — never create or touch a tenant of that name.

## Tenant names

Because a database name becomes a tenant ID, it must be a valid tenant ID:

| Constraint | Value |
| --- | --- |
| Characters | ASCII letters, digits, `_`, `-` |
| Length | 1–128 characters |

Commands addressed to a database name outside these constraints fail with a
validation error.

## Tenant lifecycle

- Tenants are created automatically the first time a data command addresses
  them. There is no separate provisioning step through the MongoDB
  endpoint.
- Creation is idempotent: concurrent first writes to the same database name
  resolve to the same tenant.
- Collections are likewise created on first insert. Schema is optional — a
  collection accepts any document shape unless a schema has been set on the
  underlying table.

## Isolation properties

- Each tenant is a separate storage namespace. A query in one database can
  never read or match documents in another, and cursor, index, and
  transaction state never crosses tenants.
- A transaction is scoped to a single tenant. Operations for a different
  database within the same transaction are not part of that transaction.
- Data written through the MongoDB endpoint is the tenant's data, not a
  copy: it is immediately visible to every other Nimbus surface addressing
  the same tenant, and writes from other surfaces are visible to MongoDB
  clients.

## Authentication scope

The endpoint authenticates one SCRAM-SHA-256 credential pair, configured
when the server enables the MongoDB listener. Authentication is
endpoint-wide: an authenticated connection may address any database name,
and therefore any tenant reachable through this endpoint. Tenant isolation
is a data-namespacing boundary between tenants, not a per-database
authorization scheme between clients sharing the endpoint.
