# Typed Key Columns

Range scans must preserve the ordering semantics of the indexed field type.
Stringifying every value is not acceptable for ordered scans because `"10"`
sorts before `"2"` lexicographically.

## SQL Contract

SQL backends that push ordered scans into SQL use type-specific storage or
expressions:

| User key type | SQL posture | Ordering rule |
| --- | --- | --- |
| string | text expression or `sort_s` text column | lexicographic string ordering |
| numeric | numeric cast/expression or `sort_n` numeric column | numeric ordering, not string ordering |
| binary | blob expression or `sort_b` blob column when binary fields exist | bytewise ordering |

Backends may use generated columns or generated expressions instead of
materialized `sort_*` columns when the database can index them safely. The
invariant is the same: the query planner must compare values with the correct
typed representation.

## Backend Notes

- redb stores typed keys in native index encodings and does not need SQL
  columns.
- SQLite currently indexes JSON extraction expressions. It is acceptable only
  for typed fields whose SQLite comparison semantics match the schema field
  type; a binary field introduction must add a BLOB representation.
- Postgres routes number ranges through numeric casts and string ranges through
  text extraction.
- MySQL uses generated-column helpers for indexed fields and numeric
  expressions for numeric ranges.
- libSQL follows the SQLite-compatible layout for remote primary and local
  replica cache.

Binary user fields are not part of the current Nimbus `FieldType` enum. When
they are introduced, SQL backends must add a blob-backed sort representation
before advertising ordered binary range scans.
