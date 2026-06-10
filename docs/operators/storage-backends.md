---
title: Storage backends
description: Run Nimbus on SQLite, Postgres, MySQL, libSQL/Turso, or redb — flags, connection strings, and how to pick a backend.
sidebar:
  label: Storage backends
  order: 4
---

Nimbus selects its tenant storage backend with the `--tenant-provider` flag
on `nimbus start`. Tenants stay isolated at the storage level on every
backend, and clients connect the same way regardless of which one you pick —
the choice is purely a deployment decision.

This page shows how to run each backend. For the full flag ↔ environment
variable ↔ config-key table, see the
[configuration reference](/reference/configuration/).

## Choose a backend

| Backend | `--tenant-provider` | External service? | Pick it when |
| --- | --- | --- | --- |
| SQLite | `sqlite` (default) | No | Local development and single-node production with zero setup |
| Postgres | `postgres` | Yes | You already run PostgreSQL infrastructure |
| MySQL | `mysql` | Yes | You already run MySQL infrastructure |
| libSQL / Turso | `libsql-replica` | Yes | You want local replica reads against a remote libSQL primary (typically Turso) |
| redb | `redb` | No | You specifically need the retained embedded key-value backend; prefer SQLite otherwise |

## Run on SQLite (default)

No flags needed — `nimbus start` uses embedded SQLite with one database file
per tenant in the data directory (`<data-dir>/<tenant-id>.sqlite3`):

```bash
nimbus start --data-dir ./data
# Equivalent to: nimbus start --data-dir ./data --tenant-provider sqlite
```

The data directory defaults to `./data` when `--data-dir` is omitted.

## Run on Postgres

Point Nimbus at an existing PostgreSQL instance. Each tenant gets an
isolated schema:

```bash
nimbus start \
  --tenant-provider postgres \
  --postgres-url "postgresql://user:pass@db.example.com:5432/nimbus"
```

`--postgres-url` is required. Optional flags:

- `--postgres-metadata-schema` — cross-tenant metadata schema
  (default `nimbus_provider`).
- `--postgres-tenant-schema-prefix` — per-tenant schema name prefix
  (default `tenant_`).
- `--postgres-min-connections` / `--postgres-max-connections` — pool
  sizing; the pool's built-in sizing applies when unset.

## Run on MySQL

Point Nimbus at an existing MySQL instance. Each tenant gets an isolated
database:

```bash
nimbus start \
  --tenant-provider mysql \
  --mysql-url "mysql://user:pass@db.example.com:3306/nimbus"
```

`--mysql-url` is required. Optional flags:

- `--mysql-metadata-database` — cross-tenant metadata database
  (default `nimbus_provider`).
- `--mysql-tenant-database-prefix` — per-tenant database name prefix
  (default `tenant_`).
- `--mysql-min-connections` / `--mysql-max-connections` — pool sizing;
  the pool's built-in sizing applies when unset.

## Run on libSQL / Turso

Replica-connected SQLite: Nimbus keeps a local replica cache and connects to
a remote libSQL primary. Each tenant gets an isolated namespace:

```bash
nimbus start \
  --tenant-provider libsql-replica \
  --libsql-url "libsql://your-db.turso.io" \
  --libsql-auth-token "<token>" \
  --libsql-admin-url "https://admin.your-db.example.com" \
  --libsql-replica-cache-dir ./cache
```

Three flags are required: `--libsql-url` (the primary),
`--libsql-admin-url` (the admin API Nimbus uses to provision tenant
namespaces), and `--libsql-replica-cache-dir` (the local replica cache
root). Optional flags:

- `--libsql-auth-token` — auth token for the primary.
- `--libsql-admin-auth-header` — `Authorization` header value for the
  admin API.
- `--libsql-metadata-namespace` — metadata namespace
  (default `nimbus_provider`).
- `--libsql-tenant-namespace-prefix` — per-tenant namespace prefix
  (default `tenant_`).

## Run on redb

The retained embedded key-value backend. One database file per tenant in
the data directory (`<data-dir>/<tenant-id>.redb`):

```bash
nimbus start --tenant-provider redb --data-dir ./data
```

Prefer SQLite for new deployments unless you have a specific reason to use
redb.

## Configure via environment or config file

Every flag above has a `NIMBUS_*` environment variable and a JSON config-file
key. Precedence is CLI flag, then environment variable, then config file:

```bash
export NIMBUS_TENANT_PROVIDER=postgres
export NIMBUS_POSTGRES_URL="postgresql://user:pass@db.example.com:5432/nimbus"
nimbus start
```

Pass a JSON config file with `--config <path>` or `NIMBUS_CONFIG=<path>`.
Storage settings live under the `persistence` key:

```json
{
  "persistence": {
    "tenant_provider": "postgres",
    "postgres_url": "postgresql://user:pass@db.example.com:5432/nimbus"
  }
}
```

See the [configuration reference](/reference/configuration/) for every key.
Note that flags for a backend you have not selected are rejected at startup —
for example, `--postgres-url` without `--tenant-provider postgres` is an
error.

## Tenant isolation boundaries

Tenants never share storage, whichever backend is active:

| Backend | Isolation boundary |
| --- | --- |
| SQLite | Database file per tenant |
| Postgres | Schema per tenant |
| MySQL | Database per tenant |
| libSQL / Turso | Namespace per tenant |
| redb | Database file per tenant |

For the isolation model itself, see
[tenant isolation](/operators/tenant-isolation/) and the
[concept page](/concepts/tenant-isolation/).

## Control-plane database

Cross-tenant control-plane data is stored in a separate embedded redb
database, `nimbus-control.db`, inside the control data directory. That
directory defaults to the data directory; override it with
`--control-data-dir` or `NIMBUS_CONTROL_DATA_DIR`. The control plane stays
local even when tenant data lives in Postgres, MySQL, or libSQL.

## Encrypt local data

Nimbus can encrypt the local files it owns — embedded SQLite and redb tenant
databases, the control-plane database, and libSQL replica caches. With
Postgres or MySQL, tenant data lives in the external database and its
encryption is the database's responsibility; Nimbus still encrypts the local
control plane. See [encryption at rest](/operators/encryption/).
