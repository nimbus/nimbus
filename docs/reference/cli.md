# CLI Reference

Neovex runs as a single server process:

```bash
neovex [flags]
```

For local development from source:

```bash
cargo run -p neovex-bin -- [flags]
```

## Core Flags

| Flag | Default | Meaning |
| --- | --- | --- |
| `--port` | `8080` | port to listen on |
| `--data-dir` | `./data` | data directory for tenant databases |
| `--convex-app-dir` | unset | optional app directory containing a generated `.neovex/convex/functions.json` manifest |
| `--license-file` | unset | optional explicit path to a Neovex license file |

If `--license-file` is not provided, Neovex next checks
`NEOVEX_LICENSE_FILE`, then `./.neovex/license.json`, and otherwise falls back
to the built-in community license.

## Persistence Flags

The tenant persistence mode is selected with `--tenant-provider`:

- `sqlite`
  embedded SQLite with one tenant file per tenant
- `redb`
  retained embedded redb provider
- `libsql-replica`
  libsql remote-primary provider family with a provider-owned local SQLite
  derivative cache
- `postgres`
  Postgres-backed external tenant persistence
- `mysql`
  MySQL-backed external tenant persistence

Provider-specific flags:

| Flag | Meaning |
| --- | --- |
| `--control-data-dir` | optional override for the local redb control-plane directory |
| `--libsql-url` | canonical libsql primary URL for `--tenant-provider=libsql-replica` |
| `--libsql-auth-token` | optional auth token for the libsql primary |
| `--libsql-admin-url` | required libsql admin/provisioning API URL for namespace lifecycle when `--tenant-provider=libsql-replica` |
| `--libsql-admin-auth-header` | optional `Authorization` header value for the libsql admin API |
| `--libsql-metadata-namespace` | provider metadata namespace for libsql replica routing |
| `--libsql-tenant-namespace-prefix` | prefix used when deriving per-tenant libsql namespaces |
| `--libsql-replica-cache-dir` | provider-owned local cache root for embedded derivative replica files |
| `--postgres-url` | canonical Postgres resource URL |
| `--postgres-metadata-schema` | provider metadata schema for Postgres routing |
| `--postgres-tenant-schema-prefix` | prefix used when deriving per-tenant Postgres schemas |
| `--postgres-min-connections` | optional minimum Postgres pool size |
| `--postgres-max-connections` | optional maximum Postgres pool size |
| `--mysql-url` | canonical MySQL resource URL |
| `--mysql-metadata-database` | provider metadata database for MySQL routing |
| `--mysql-tenant-database-prefix` | prefix used when deriving per-tenant MySQL databases |
| `--mysql-min-connections` | optional minimum MySQL pool size |
| `--mysql-max-connections` | optional maximum MySQL pool size |

The same values can also be supplied through the corresponding `NEOVEX_*`
environment variables or the JSON config file referenced by `--config` or
`NEOVEX_CONFIG`.

## Runtime Flags

| Flag | Default | Meaning |
| --- | --- | --- |
| `--runtime-heap-mb` | `128` | V8 heap limit per isolate in megabytes |
| `--runtime-initial-heap-mb` | `8` | initial V8 heap size per isolate in megabytes |
| `--runtime-timeout-secs` | `30` | maximum wall-clock runtime invocation time |
| `--runtime-max-instances` | available hardware parallelism | maximum concurrent top-level runtime instances |
| `--runtime-max-nested-calls` | `64` | maximum nested `ctx.run*` invocations per request tree |

## Startup Behavior

On startup, the server:

- initializes tracing
- loads the service with the configured data directory
- loads tenants with scheduled work
- starts the scheduler loop
- optionally loads the Convex registry from `--convex-app-dir`
- loads license state from the explicit path, environment, default path, or
  built-in community defaults

Related references:

- [HTTP and WebSocket API](http-api.md)
- [Convex compatibility](../convex/compatibility.md)
