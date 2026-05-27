# MBA7 SQL Safety ADR Proof

posture: parameterized_values_helper_owned_identifiers

## ADR Coverage

| Backend | ADR | Identifier helpers | Value binding |
| --- | --- | --- | --- |
| SQLite | `docs/decisions/003-sqlite-sql-safety.md` | `sqlite_index_name`, `sanitize_identifier_component`, `json_extract_expr`, `validate_path_for_sql` | rusqlite `?1` parameters |
| Postgres | `docs/decisions/004-postgres-sql-safety.md` | `validate_identifier_input`, `tenant_schema_name`, `quote_identifier`, `quote_literal`, `qualified_table`, `tenant_init_sql` | tokio-postgres `$1` parameters |
| MySQL | `docs/decisions/005-mysql-sql-safety.md` | `validate_identifier_input`, `tenant_database_name`, `quote_identifier`, `qualified_table`, `mysql_index_key_part`, `tenant_init_statements` | mysql_async `?` parameters |
| libSQL | `docs/decisions/006-libsql-sql-safety.md` | `tenant_namespace_name`, `validate_namespace_input`, fixed internal table snapshot helpers | libSQL `?1` parameters |

## Audit Notes

Current SQL backends use shared document tables keyed by public table name as
data. This means normal document operations do not need dynamic physical table
identifiers. The remaining dynamic SQL is DDL or fixed internal-table plumbing,
and each backend owns the helper that validates or quotes the identifier before
interpolation.

The MBA7 decision intentionally does not add an `inventory`-style registry or a
cross-backend SQL builder. The safety boundary is easier to audit when each
backend documents its own allowed interpolation helpers and keeps all user data
on parameterized paths.
