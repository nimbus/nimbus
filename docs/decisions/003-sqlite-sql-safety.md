# 003 - SQLite SQL safety

- **Status:** accepted
- **Date:** 2026-05-27
- **Decision owner:** `nimbus/nimbus` maintainers
- **Parent plan:** `docs/plans/multi-backend-adapter-hardening-plan.md` MBA7

---

## Decision

SQLite storage uses fixed shared tables for tenant data. User table names,
document ids, field values, schema JSON, commit records, and scheduler payloads
must be bound as SQL parameters, never interpolated into SQL text.

The only reviewed dynamic SQL shapes are:

- `sqlite_index_name(table, index_name)` for generated index identifiers
- `sanitize_identifier_component(input)` for identifier components inside
  generated index names
- `json_extract_expr(field)` for JSON path expressions built from schema-owned
  index fields
- `validate_path_for_sql(path)` for SQLCipher `ATTACH DATABASE` path literals
- defensive quoting of table names read back from `sqlite_master` during
  encryption validation

## Invariants

- Public `TableName` values are stored in the `documents.table_name` column and
  bound as `?1`, not used as physical table identifiers.
- Generated SQLite index names are derived from schema names, sanitized to
  ASCII alphanumeric plus underscores, and quoted before use.
- JSON-path field interpolation escapes double quotes and is limited to index
  fields from the active schema.
- SQLCipher attach paths reject non-UTF-8 paths and single quotes before they
  enter SQL text.
- Fixed migration/bootstrap DDL may be static SQL strings.

## Rejected Alternative

Per-user-table physical DDL is rejected for the current storage layout. It
would require a larger identifier-validation surface without improving the
current shared-table query path.
