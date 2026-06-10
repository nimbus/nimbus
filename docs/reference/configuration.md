---
title: Configuration
description: Flag, environment variable, and config-file key cross-reference for the Nimbus server.
sidebar:
  label: Configuration
  order: 3
---

This page currently covers storage and encryption configuration; the full
configuration reference is being built out.

## Resolution order

Settings resolve as CLI flag, then environment variable, then config file.
The JSON config file is named by `--config <path>` or the `NIMBUS_CONFIG`
environment variable. All keys below live under the top-level `persistence`
object; unknown keys in the config file are rejected.

```json
{
  "persistence": {
    "tenant_provider": "sqlite",
    "data_dir": "./data"
  }
}
```

## Core storage

| Flag | Environment variable | Config key (`persistence.`) | Default |
| --- | --- | --- | --- |
| `--config` | `NIMBUS_CONFIG` | — | none |
| `--data-dir` | `NIMBUS_DATA_DIR` | `data_dir` | `./data` |
| `--control-data-dir` | `NIMBUS_CONTROL_DATA_DIR` | `control_data_dir` | the data directory |
| `--tenant-provider` | `NIMBUS_TENANT_PROVIDER` | `tenant_provider` | `sqlite` |

`tenant_provider` accepts `sqlite`, `libsql-replica`, `redb`, `postgres`,
or `mysql`. Flags belonging to a provider other than the selected one are
rejected at startup.

## Postgres (`--tenant-provider postgres`)

| Flag | Environment variable | Config key (`persistence.`) | Default |
| --- | --- | --- | --- |
| `--postgres-url` | `NIMBUS_POSTGRES_URL` | `postgres_url` | required |
| `--postgres-metadata-schema` | `NIMBUS_POSTGRES_METADATA_SCHEMA` | `postgres_metadata_schema` | `nimbus_provider` |
| `--postgres-tenant-schema-prefix` | `NIMBUS_POSTGRES_TENANT_SCHEMA_PREFIX` | `postgres_tenant_schema_prefix` | `tenant_` |
| `--postgres-min-connections` | `NIMBUS_POSTGRES_MIN_CONNECTIONS` | `postgres_min_connections` | pool default |
| `--postgres-max-connections` | `NIMBUS_POSTGRES_MAX_CONNECTIONS` | `postgres_max_connections` | pool default |

`min_connections` may not exceed `max_connections` when both are set.

## MySQL (`--tenant-provider mysql`)

| Flag | Environment variable | Config key (`persistence.`) | Default |
| --- | --- | --- | --- |
| `--mysql-url` | `NIMBUS_MYSQL_URL` | `mysql_url` | required |
| `--mysql-metadata-database` | `NIMBUS_MYSQL_METADATA_DATABASE` | `mysql_metadata_database` | `nimbus_provider` |
| `--mysql-tenant-database-prefix` | `NIMBUS_MYSQL_TENANT_DATABASE_PREFIX` | `mysql_tenant_database_prefix` | `tenant_` |
| `--mysql-min-connections` | `NIMBUS_MYSQL_MIN_CONNECTIONS` | `mysql_min_connections` | pool default |
| `--mysql-max-connections` | `NIMBUS_MYSQL_MAX_CONNECTIONS` | `mysql_max_connections` | pool default |

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
is rejected at startup.

The `nimbus encryption` admin commands resolve these same settings from
environment variables and the config file only (not from server flags).

## Related pages

- [Storage backends](/operators/storage-backends/) — how to run each
  backend.
- [Encryption at rest](/operators/encryption/) — how to enable encryption
  and rotate keys.
