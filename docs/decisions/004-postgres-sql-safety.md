# 004 - Postgres SQL safety

- **Status:** accepted
- **Date:** 2026-05-27
- **Decision owner:** `nimbus/nimbus` maintainers
- **Parent plan:** `docs/plans/multi-backend-adapter-hardening-plan.md` MBA7

---

## Decision

Postgres storage may interpolate only reviewed, helper-produced identifiers.
All data values must be passed through `tokio-postgres` parameters.

Reviewed helper allowlist:

- `validate_identifier_input(value, label)`
- `tenant_schema_name(prefix, tenant_id)`
- `quote_identifier(identifier)`
- `quote_literal(value)`
- `qualified_table(schema_name, table_name)`
- `tenant_init_sql(schema_name)`
- `postgres_notification_channel_name(config)`
- `postgres_pool_application_name(config)`

## Invariants

- Tenant schema names are derived from a configured prefix plus a SHA-256 hash
  of `TenantId`; user-facing table names never become schema names.
- Metadata schema and tenant-schema prefix inputs are length-validated before
  use.
- Table references use `qualified_table`, which quotes both schema and fixed
  table identifiers.
- Values use `$1`, `$2`, ... parameters. Public `TableName` values are stored
  as data in shared tables.
- `quote_literal` is limited to internal DDL/channel setup where Postgres does
  not accept a normal bind parameter.

## Rejected Alternative

Per-table physical tables are rejected for the current Postgres layout. Stable
logical identity is handled by catalog data, not by exposing user names to DDL.
