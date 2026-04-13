# CLI Reference

Neovex serves HTTP/WebSocket traffic by default and also exposes service
management subcommands:

```bash
neovex [flags]
```

```bash
neovex service config [--file compose.yaml] [--services]
```

```bash
neovex service up [service] [--file compose.yaml] [--tenant <tenant-id>]
```

```bash
neovex service down [service] [--file compose.yaml] [--tenant <tenant-id>]
```

```bash
neovex service list [--file compose.yaml] [--all-tenants]
```

```bash
neovex service inspect <service> [--file compose.yaml] [--tenant <tenant-id>]
```

```bash
neovex service logs <service> [--file compose.yaml] [--tenant <tenant-id>] [--follow]
```

```bash
neovex service ps <service> [--file compose.yaml] [--tenant <tenant-id>]
```

```bash
neovex --compose-file ./compose.yaml [--convex-app-dir ./app]
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

When no subcommand is provided, Neovex starts the server. On startup, it:

- initializes tracing
- loads the service with the configured data directory
- loads tenants with scheduled work
- starts the scheduler loop
- optionally loads the Convex registry from `--convex-app-dir`
- optionally loads declared sandbox-backed services from `--compose-file`
- loads license state from the explicit path, environment, default path, or
  built-in community defaults

When `--compose-file` is present, Neovex validates the Compose file through the
same M5 adapter used by `neovex service config`, lowers it into a typed
declared-service catalog, and wires that catalog into the server-owned sandbox
manager. With `--convex-app-dir`, `ctx.services.*` can activate those declared
services on first use. The explicit `neovex service up/down/...` commands share
that same Compose lowering, deterministic project identity, and project-scoped
backend root instead of inventing a second lifecycle control plane.

## Service Commands

The current M5 service-control-plane slice exposes Compose validation plus
explicit lifecycle control:

| Command | Meaning |
| --- | --- |
| `neovex service config` | parse `compose.yaml`, validate the supported subset, and print the resolved service plan as YAML |
| `neovex service config --file ./stack.yml` | validate a specific Compose file |
| `neovex service config --services` | list only service names, one per line |
| `neovex service up` | start all declared services for the deterministic local project tenant; active current services report `already_running` |
| `neovex service up db --tenant tenant-a` | start only service `db` for an explicit tenant |
| `neovex service down` | stop one current persisted sandbox per service identity for the deterministic local project tenant |
| `neovex service down db` | stop the current persisted sandbox for service `db` in the deterministic local project tenant |
| `neovex service list` | list persisted sandbox state for the deterministic local project tenant as YAML |
| `neovex service list --all-tenants` | list all persisted sandboxes under the project-scoped backend root |
| `neovex service inspect db` | inspect persisted sandbox details for service `db` in the deterministic local project tenant |
| `neovex service inspect db --tenant tenant-a` | inspect persisted sandbox details for service `db` in an explicit tenant |
| `neovex service logs db` | print the persisted `ctr.log` for service `db` in the deterministic local project tenant |
| `neovex service logs db --follow` | keep polling the persisted `ctr.log` for appended output |
| `neovex service ps db` | show the persisted PID snapshot and matching host `ps` rows for service `db` |

Current scope:

- validates `image` and `build` sources
- resolves `environment` plus `env_file`
- validates lowerable `command`, `entrypoint`, `working_dir`, and `user`
  process overrides
- validates lowerable lifecycle settings such as `restart` and
  `stop_grace_period` against the generic sandbox lifecycle seam
- validates the declared-service catalog handoff used by both the server-owned
  service manager and the explicit `service up` launch path
- lowers `ports`, restart policy, and CPU/memory limits into the resolved plan
- preserves `depends_on`, `healthcheck`, `volumes`, labels, and `x-neovex`
  metadata for follow-on M5 lifecycle commands and recovery drills
- warns on ignored fields such as `networks`, `privileged`, and `logging`
- resolves lifecycle commands against backend-owned persisted krun manifests and
  conmon logs under the project-scoped `control_data_dir`, not a separate
  CLI-owned service database
- derives a deterministic local project tenant id from the Compose project key,
  and uses that tenant by default for `service up` / `service down` /
  `service list` / `service inspect` / `service logs` / `service ps`

Remaining M5 gap:

- Linux-host end-to-end compose-backed serve proof and recovery-drill evidence
  remain tracked under
  [service-control-plane-plan](../plans/service-control-plane-plan.md)

Related references:

- [HTTP and WebSocket API](http-api.md)
- [Convex compatibility](../convex/compatibility.md)
