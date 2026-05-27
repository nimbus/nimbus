# 006 - libSQL SQL safety

- **Status:** accepted
- **Date:** 2026-05-27
- **Decision owner:** `nimbus/nimbus` maintainers
- **Parent plan:** `docs/plans/multi-backend-adapter-hardening-plan.md` MBA7

---

## Decision

libSQL replica storage keeps the same shared-table data model as SQLite for
tenant data. User table names and document ids must be bound as libSQL
parameters, not interpolated into SQL text.

Reviewed helper allowlist:

- `tenant_namespace_name(prefix, tenant_id)`
- `validate_namespace_input(value, field)`
- `namespace_create_endpoint(admin_api_url, namespace)`
- `namespace_endpoint(admin_api_url, namespace)`
- `table_has_entries_remote(conn, table)` for fixed internal table names
- remote snapshot helpers that copy fixed internal tables between primary and
  replica cache

## Invariants

- Tenant namespaces are derived from a configured prefix plus the tenant id or a
  SHA-256 hash fallback, then validated as ASCII letters, digits, underscore,
  or hyphen.
- Namespace values used in admin API URLs come from `tenant_namespace_name` and
  `validate_namespace_input`.
- Data values use `?1`, `?2`, ... parameters. Public `TableName` values are
  stored as data in shared tables.
- Dynamic table-name SQL is limited to fixed internal table names owned by the
  snapshot/bootstrap code.
- Local replica caches reuse the SQLite safety posture for SQLCipher and index
  SQL.

## Rejected Alternative

Per-table libSQL namespaces or tables are rejected. The backend-specific
physical boundary is the tenant namespace plus local replica cache, not a
namespace/table per user table.
