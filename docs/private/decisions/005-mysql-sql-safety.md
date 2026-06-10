# 005 - MySQL SQL safety

- **Status:** accepted
- **Date:** 2026-05-27
- **Decision owner:** `nimbus/nimbus` maintainers
- **Parent plan:** `docs/plans/multi-backend-adapter-hardening-plan.md` MBA7

---

## Decision

MySQL storage may interpolate only reviewed, helper-produced identifiers. All
data values must be passed through `mysql_async` parameters.

Reviewed helper allowlist:

- `validate_identifier_input(value, label)`
- `tenant_database_name(prefix, tenant_id)`
- `quote_identifier(identifier)`
- `qualified_table(database_name, table_name)`
- `mysql_index_key_part(identifier, prefix_chars)`
- `tenant_init_statements(database_name)`

## Invariants

- Tenant database names are derived from a configured prefix plus a SHA-256 hash
  of `TenantId`; user-facing table names never become database names.
- Metadata database and tenant-database prefix inputs are length-validated.
- Table references use `qualified_table`, which backtick-quotes both database
  and fixed table identifiers.
- Values use `?` parameters. Public `TableName` values are stored as data in
  shared tables.
- Generated index key parts use `mysql_index_key_part` so identifier quoting
  and prefix length policy stay together.

## Rejected Alternative

Per-user-table MySQL DDL is rejected. It would increase identifier surface area
and does not improve the current shared-table transaction contract.
