---
title: Configuration
description: Flag, environment variable, and config-file key cross-reference for the Nimbus server.
sidebar:
  label: Configuration
  order: 3
---

This page is the complete configuration cross-reference for `nimbus start`:
every flag, its environment variable (when one exists), its config-file key
(when one exists), its default, and what it controls.

## Resolution order

Settings resolve as CLI flag, then environment variable, then config file.
The JSON config file is named by `--config <path>` or the `NIMBUS_CONFIG`
environment variable (the flag wins when both are set).

Only storage and encryption settings have config-file keys, and all of them
live under the top-level `persistence` object. Network, runtime-limit, app,
compose, and license settings are CLI and environment only. Unknown keys are
rejected — both at the top level and inside `persistence` — so a typo fails
startup instead of being silently ignored.

```json
{
  "persistence": {
    "tenant_provider": "sqlite",
    "data_dir": "./data"
  }
}
```

Config-file keys are snake_case; enum values (`tenant_provider`,
`encryption_key_provider`) are kebab-case, matching the CLI spelling.
The environment variables additionally accept underscore spellings of those
enum values (for example `libsql_replica`).

## Network and bind

These settings have no environment variable and no config-file key.

| Flag | Default | Meaning |
| --- | --- | --- |
| `--host` | `127.0.0.1` | Interface to listen on; defaults to loopback for local safety. |
| `--port` | `8080` | TCP port to listen on. |
| `--allow-network` | off | Opt-in required for any non-loopback bind. |
| `--systemd-socket-activation` | off | Inherit the TCP listener from systemd instead of binding `--host`/`--port`. |

### Non-loopback binds

`nimbus start` refuses any `--host` outside the loopback range
(`127.0.0.1`, `::1`, or `localhost`) unless `--allow-network` is set. With
the flag set, a second gate still applies: the local admin token must have
been explicitly rotated at least once (`nimbus auth rotate-admin`), or the
bind is refused; a rotation older than 30 days logs a startup warning but
does not block. See [Hardening](/operators/hardening/).

### systemd socket activation

With `--systemd-socket-activation`, the server takes its listener from
systemd (Unix only). It requires `LISTEN_FDS=1` and a `LISTEN_PID` matching
the server process, and consumes exactly one inherited socket. The
`--host`/`--port` flags are not used to bind; the inherited listener's
address is checked against the same `--allow-network` and admin-token
freshness gates. See [Deploy on Linux](/operators/deploy-linux/).

## Core storage

| Flag | Environment variable | Config key (`persistence.`) | Default |
| --- | --- | --- | --- |
| `--config` | `NIMBUS_CONFIG` | — | none |
| `--data-dir` | `NIMBUS_DATA_DIR` | `data_dir` | `./data` |
| `--control-data-dir` | `NIMBUS_CONTROL_DATA_DIR` | `control_data_dir` | the data directory |
| `--tenant-provider` | `NIMBUS_TENANT_PROVIDER` | `tenant_provider` | `sqlite` |

`--data-dir` holds embedded tenant databases; `--control-data-dir`
overrides where the local control plane lives (it defaults to the data
directory).

`tenant_provider` accepts `sqlite`, `libsql-replica`, `redb`, `postgres`,
or `mysql`. Flags belonging to a provider other than the selected one are
rejected at startup. See
[Storage backends](/operators/storage-backends/).

## Postgres (`--tenant-provider postgres`)

| Flag | Environment variable | Config key (`persistence.`) | Default |
| --- | --- | --- | --- |
| `--postgres-url` | `NIMBUS_POSTGRES_URL` | `postgres_url` | required |
| `--postgres-metadata-schema` | `NIMBUS_POSTGRES_METADATA_SCHEMA` | `postgres_metadata_schema` | `nimbus_provider` |
| `--postgres-tenant-schema-prefix` | `NIMBUS_POSTGRES_TENANT_SCHEMA_PREFIX` | `postgres_tenant_schema_prefix` | `tenant_` |
| `--postgres-min-connections` | `NIMBUS_POSTGRES_MIN_CONNECTIONS` | `postgres_min_connections` | pool default |
| `--postgres-max-connections` | `NIMBUS_POSTGRES_MAX_CONNECTIONS` | `postgres_max_connections` | pool default |

## MySQL (`--tenant-provider mysql`)

| Flag | Environment variable | Config key (`persistence.`) | Default |
| --- | --- | --- | --- |
| `--mysql-url` | `NIMBUS_MYSQL_URL` | `mysql_url` | required |
| `--mysql-metadata-database` | `NIMBUS_MYSQL_METADATA_DATABASE` | `mysql_metadata_database` | `nimbus_provider` |
| `--mysql-tenant-database-prefix` | `NIMBUS_MYSQL_TENANT_DATABASE_PREFIX` | `mysql_tenant_database_prefix` | `tenant_` |
| `--mysql-min-connections` | `NIMBUS_MYSQL_MIN_CONNECTIONS` | `mysql_min_connections` | pool default |
| `--mysql-max-connections` | `NIMBUS_MYSQL_MAX_CONNECTIONS` | `mysql_max_connections` | pool default |

For both Postgres and MySQL, `min_connections` may not exceed
`max_connections` when both are set.

## libSQL / Turso (`--tenant-provider libsql-replica`)

| Flag | Environment variable | Config key (`persistence.`) | Default |
| --- | --- | --- | --- |
| `--libsql-url` | `NIMBUS_LIBSQL_URL` | `libsql_url` | required |
| `--libsql-auth-token` | `NIMBUS_LIBSQL_AUTH_TOKEN` | `libsql_auth_token` | none |
| `--libsql-admin-url` | `NIMBUS_LIBSQL_ADMIN_URL` | `libsql_admin_url` | required |
| `--libsql-admin-auth-header` | `NIMBUS_LIBSQL_ADMIN_AUTH_HEADER` | `libsql_admin_auth_header` | none |
| `--libsql-metadata-namespace` | `NIMBUS_LIBSQL_METADATA_NAMESPACE` | `libsql_metadata_namespace` | `nimbus_provider` |
| `--libsql-tenant-namespace-prefix` | `NIMBUS_LIBSQL_TENANT_NAMESPACE_PREFIX` | `libsql_tenant_namespace_prefix` | `tenant_` |
| `--libsql-replica-cache-dir` | `NIMBUS_LIBSQL_REPLICA_CACHE_DIR` | `libsql_replica_cache_dir` | required |

## Encryption

| Flag | Environment variable | Config key (`persistence.`) | Default |
| --- | --- | --- | --- |
| `--encryption-key-provider` | `NIMBUS_ENCRYPTION_KEY_PROVIDER` | `encryption_key_provider` | unset (encryption disabled) |
| `--encryption-master-key-file` | `NIMBUS_ENCRYPTION_MASTER_KEY_FILE` | `encryption_master_key_file` | required for `master-key-file` |
| `--encryption-key-dir` | `NIMBUS_ENCRYPTION_KEY_DIR` | `encryption_key_dir` | required for `key-dir` |
| `--encryption-aws-kms-key-id` | `NIMBUS_ENCRYPTION_AWS_KMS_KEY_ID` | `encryption_aws_kms_key_id` | required for `aws-kms` |
| `--encryption-aws-region` | `NIMBUS_ENCRYPTION_AWS_REGION` | `encryption_aws_region` | AWS default chain |
| `--encryption-aws-endpoint-url` | `NIMBUS_ENCRYPTION_AWS_ENDPOINT_URL` | `encryption_aws_endpoint_url` | AWS default endpoint |

`encryption_key_provider` accepts `master-key-file`, `key-dir`, or
`aws-kms`. Provider-specific encryption options are valid only with their
provider, and any encryption option without `encryption_key_provider` set
is rejected at startup. The master key file must contain exactly 32 bytes
of key material.

The `nimbus encryption` admin commands resolve these same settings from
environment variables and the config file only (not from server flags).
See [Encryption at rest](/operators/encryption/).

## Runtime limits

These settings have no environment variable and no config-file key. The
defaults marked "derived" are computed at startup from the number of CPUs
available to the process, so `nimbus start --help` shows the concrete
values for your machine.

| Flag | Default | Meaning |
| --- | --- | --- |
| `--runtime-heap-mb` | `128` | V8 heap limit per runtime isolate, in megabytes. |
| `--runtime-initial-heap-mb` | `8` | Initial V8 heap size per runtime isolate, in megabytes. |
| `--runtime-timeout-secs` | `30` | Maximum wall-clock execution time for a runtime invocation, in seconds. |
| `--runtime-max-instances` | derived: CPU count | Maximum number of concurrent top-level runtime instances. |
| `--runtime-worker-threads` | derived: 2 × max instances | Number of runtime worker threads. |
| `--runtime-max-active-per-tenant` | derived: max instances − 1, at least 1 | Maximum active top-level runtime invocations per tenant. |
| `--runtime-max-in-flight-per-tenant` | derived: 2 × active per tenant, capped at worker threads | Maximum active plus parked top-level runtime invocations per tenant. |
| `--runtime-max-queued-per-tenant` | derived: equals the in-flight default | Maximum queued top-level runtime invocations per tenant. |
| `--runtime-max-nested-calls` | `64` | Maximum nested runtime `ctx.run*` invocations per request tree. |

## App directory and codegen

These settings have no environment variable and no config-file key.

| Flag | Default | Meaning |
| --- | --- | --- |
| `--app-dir` | none | App directory with generated runtime artifacts to serve at startup. |
| `--skip-codegen` | off | Skip automatic codegen before startup; manifests must be pre-built. |
| `--debug-node-apis` | off | Diagnose Node.js builtin imports during the codegen preflight. |

`nimbus start` does no source-tree discovery: without `--app-dir`, the
daemon starts with no app functions and waits for deploys to arrive
through the [deploy admin API](/reference/deploy-admin-api/). An explicit
`--app-dir` must contain a recognizable app surface (a `convex/` or
`nimbus/` source directory, a `firebase.json`, a Functions Framework
`package.json`, or a generated function manifest), or startup fails.

## Compose files

| Flag | Environment variable | Default | Meaning |
| --- | --- | --- | --- |
| `--compose-file` | `COMPOSE_FILE` | auto-discovery | Ordered Compose file list declaring sandbox-backed services; repeat the flag to merge overlays. |
| — | `COMPOSE_PATH_SEPARATOR` | `:` (Unix), `;` (Windows) | Separator used to split `COMPOSE_FILE` into multiple paths. |

When `--compose-file` is omitted, Nimbus uses `COMPOSE_FILE` when set,
then discovers a Compose file by walking up from the current directory.
Discovery checks each directory for `compose.yaml` (merging a sibling
`compose.override.yaml` when present), then `compose.yml`, then
`docker-compose.yaml` or `docker-compose.yml` (having both in one
directory is an error), and stops at the repository's `.git` boundary.

## License

| Flag | Environment variable | Default | Meaning |
| --- | --- | --- | --- |
| `--license-file` | `NIMBUS_LICENSE_FILE` | `~/.config/nimbus/license.json` when present | Path to a Nimbus license file. |

The flag wins over the environment variable. The default path honors
`XDG_CONFIG_HOME` when set (`$XDG_CONFIG_HOME/nimbus/license.json`) and is
used only when the file exists. With no license file at all, the server
runs with the built-in community license.

## Environment-only settings

| Environment variable | Meaning |
| --- | --- |
| `NIMBUS_DEPLOY_TOKEN` | Enables the [deploy admin API](/reference/deploy-admin-api/) and sets the expected deploy bearer token. Unset, every deploy request returns `401`. |
| `LISTEN_FDS`, `LISTEN_PID` | Set by systemd for `--systemd-socket-activation`; not set manually. |

## Related pages

- [Storage backends](/operators/storage-backends/) — how to run each
  backend.
- [Encryption at rest](/operators/encryption/) — how to enable encryption
  and rotate keys.
- [Hardening](/operators/hardening/) — the network-bind and admin-token
  gates in depth.
- [Deploy on Linux](/operators/deploy-linux/) — systemd units and socket
  activation.
- [Tenant isolation](/operators/tenant-isolation/) — what the tenant
  provider choice means operationally.
- [Self-host quickstart](/get-started/self-host/) — a minimal first
  configuration.
- [CLI reference](/reference/cli/) — the full `nimbus` command surface.
