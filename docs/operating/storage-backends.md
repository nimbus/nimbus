# Storage Backends

Nimbus abstracts persistence behind the `--tenant-provider` flag. Each tenant gets an isolated namespace regardless of which backend you choose. The storage backend is transparent to client applications -- all adapters (Convex, Firebase, MongoDB, Native) work identically across all backends.

## Choosing a Backend

| Backend | Best for | Flag | External service? |
|---------|----------|------|-------------------|
| SQLite | Local dev, single-node production, simplest setup | `--tenant-provider sqlite` (default) | No |
| Postgres | Production with existing Postgres infrastructure | `--tenant-provider postgres` | Yes |
| MySQL | Production with existing MySQL infrastructure | `--tenant-provider mysql` | Yes |
| libSQL / Turso | Edge replicas with Turso-backed remote primary | `--tenant-provider libsql-replica` | Yes |
| redb | Retained legacy embedded backend | `--tenant-provider redb` | No |

## SQLite (default)

Zero-config embedded storage. Each tenant gets its own SQLite database file in the data directory.

```bash
nimbus start --data-dir ./data
# Equivalent to: nimbus start --data-dir ./data --tenant-provider sqlite
```

- One database file per tenant in `<data-dir>/tenants/`
- Expression indexes derived from table schema definitions
- Full ACID transactions
- Optional encryption at rest (see [Encryption](#encryption-at-rest))

## Postgres

Connect to an existing PostgreSQL instance. Each tenant gets an isolated schema.

```bash
nimbus start \
  --tenant-provider postgres \
  --postgres-url "postgresql://user:pass@localhost:5432/nimbus" \
  --postgres-metadata-schema nimbus_metadata \
  --postgres-tenant-schema-prefix tenant_
```

| Flag | Env var | Default | Purpose |
|------|---------|---------|---------|
| `--postgres-url` | `NIMBUS_POSTGRES_URL` | (required) | Connection string |
| `--postgres-metadata-schema` | `NIMBUS_POSTGRES_METADATA_SCHEMA` | `nimbus_metadata` | Cross-tenant metadata schema |
| `--postgres-tenant-schema-prefix` | `NIMBUS_POSTGRES_TENANT_SCHEMA_PREFIX` | `tenant_` | Per-tenant schema name prefix |
| `--postgres-min-connections` | `NIMBUS_POSTGRES_MIN_CONNECTIONS` | (driver default) | Minimum pool size |
| `--postgres-max-connections` | `NIMBUS_POSTGRES_MAX_CONNECTIONS` | (driver default) | Maximum pool size |

## MySQL

Connect to an existing MySQL instance. Each tenant gets an isolated database.

```bash
nimbus start \
  --tenant-provider mysql \
  --mysql-url "mysql://user:pass@localhost:3306/nimbus" \
  --mysql-metadata-database nimbus_metadata \
  --mysql-tenant-database-prefix tenant_
```

| Flag | Env var | Default | Purpose |
|------|---------|---------|---------|
| `--mysql-url` | `NIMBUS_MYSQL_URL` | (required) | Connection string |
| `--mysql-metadata-database` | `NIMBUS_MYSQL_METADATA_DATABASE` | `nimbus_metadata` | Cross-tenant metadata database |
| `--mysql-tenant-database-prefix` | `NIMBUS_MYSQL_TENANT_DATABASE_PREFIX` | `tenant_` | Per-tenant database name prefix |
| `--mysql-min-connections` | `NIMBUS_MYSQL_MIN_CONNECTIONS` | (driver default) | Minimum pool size |
| `--mysql-max-connections` | `NIMBUS_MYSQL_MAX_CONNECTIONS` | (driver default) | Maximum pool size |

## libSQL / Turso

Replica-connected SQLite with a remote primary (typically Turso). Local reads, remote writes.

```bash
nimbus start \
  --tenant-provider libsql-replica \
  --libsql-url "libsql://your-db.turso.io" \
  --libsql-auth-token "<turso-token>" \
  --libsql-replica-cache-dir ./cache
```

| Flag | Env var | Purpose |
|------|---------|---------|
| `--libsql-url` | `NIMBUS_LIBSQL_URL` | Remote primary URL |
| `--libsql-auth-token` | `NIMBUS_LIBSQL_AUTH_TOKEN` | Authentication token |
| `--libsql-admin-url` | `NIMBUS_LIBSQL_ADMIN_URL` | Admin API URL (optional) |
| `--libsql-admin-auth-header` | `NIMBUS_LIBSQL_ADMIN_AUTH_HEADER` | Admin auth header (optional) |
| `--libsql-metadata-namespace` | `NIMBUS_LIBSQL_METADATA_NAMESPACE` | Metadata namespace |
| `--libsql-tenant-namespace-prefix` | `NIMBUS_LIBSQL_TENANT_NAMESPACE_PREFIX` | Tenant namespace prefix |
| `--libsql-replica-cache-dir` | `NIMBUS_LIBSQL_REPLICA_CACHE_DIR` | Local replica cache directory |

## redb

Retained embedded key-value backend. Supported during the provider-model transition.

```bash
nimbus start --tenant-provider redb --data-dir ./data
```

## Tenant Isolation

Every tenant is fully isolated at the storage level -- separate data,
separate indexes, no cross-tenant visibility. The isolation boundary depends
on which backend is active:

| Backend | Isolation boundary |
|---|---|
| SQLite | File per tenant |
| Postgres | Schema per tenant |
| MySQL | Database per tenant |
| libsql | Namespace per tenant |
| redb | Directory per tenant |

There is no operation that can read or write across tenant boundaries.
Tenants are auto-created on first access (`ensure_tenant`). No upfront
provisioning is needed for development.

How tenants are addressed depends on which adapter the client connects
through. The MongoDB adapter maps database names to tenant IDs, the Convex
adapter maps deployment URLs, and the Native HTTP API uses the
`X-Tenant-Id` header. Regardless of the addressing mechanism, the storage
isolation is identical.

## Environment Variables

All CLI flags have `NIMBUS_*` environment variable equivalents. Environment variables are overridden by CLI flags. Example:

```bash
export NIMBUS_TENANT_PROVIDER=postgres
export NIMBUS_POSTGRES_URL="postgresql://user:pass@localhost:5432/nimbus"
nimbus start
```

A JSON configuration file can also be provided via `--config` or `NIMBUS_CONFIG`:

```json
{
  "persistence": {
    "tenant_provider": "postgres",
    "postgres_url": "postgresql://user:pass@localhost:5432/nimbus",
    "postgres_metadata_schema": "nimbus_metadata",
    "postgres_tenant_schema_prefix": "tenant_"
  }
}
```

## Encryption at Rest

Nimbus supports optional encryption at rest for embedded backends (SQLite, redb). See the [Encryption reference](encryption.md) for setup, key providers, and migration workflows.

```bash
nimbus start \
  --encryption-key-provider master-key-file \
  --encryption-master-key-file /path/to/32-byte-key
```

Supported key providers: `master-key-file`, `key-dir`, `aws-kms`.

## Control Plane Storage

Cross-tenant metadata (usage tracking, licensing) is stored in a separate embedded redb database at `<data-dir>/control/nimbus-control.db`. This is independent of the tenant provider selection. Override with `--control-data-dir` or `NIMBUS_CONTROL_DATA_DIR`.

## Development Sandbox

A `compose.yaml` at the project root provides Postgres and MySQL for local development:

```bash
docker compose up -d postgres
nimbus start --tenant-provider postgres --postgres-url "postgresql://nimbus:nimbus@localhost:5432/nimbus"
```

## Related Docs

- [CLI reference](cli.md) -- all flags and defaults
- [Encryption reference](encryption.md) -- key management and migration
- [Provider topologies](../architecture/storage/provider-topologies.md) -- architecture details
- [Persistence engine baseline](../architecture/storage/persistence-engine-baseline.md) -- backend layouts
