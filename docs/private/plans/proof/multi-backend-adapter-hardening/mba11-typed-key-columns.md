# MBA11 Typed-Key Storage Proof

status: done

current_ordering_coverage: string numeric
binary_ordering: future_contract

## Current Code Evidence

- redb range scans use native index encodings through
  `crates/nimbus-storage/src/index/scan/range.rs`.
- SQLite SQL range scans are generated in
  `crates/nimbus-storage/src/sqlite/schema.rs` and select from shared
  `documents` with JSON extraction expressions.
- Postgres range scans branch on `FieldType::String` versus
  `FieldType::Number` in `crates/nimbus-storage/src/postgres/backend.rs` and
  use text extraction or numeric casts accordingly.
- MySQL range scans branch on `FieldType::String` versus `FieldType::Number`
  in `crates/nimbus-storage/src/mysql/backend.rs` and use generated-column
  helpers plus numeric expressions.
- libSQL follows the SQLite-compatible remote/local layout.

## Contract

SQL storage must not collapse ordered user keys to one string column. The
canonical contract is documented in
`docs/architecture/storage/typed-key-columns.md`: string keys use text
ordering and numeric keys use numeric ordering today. Binary keys must use blob
ordering when binary fields are introduced.

Current Nimbus schemas support string, number, boolean, array, object, and any
fields; they do not yet expose a binary field type. Binary ordering is therefore
documented as the required posture for the first backend that adds binary user
fields, not as a fake current coverage claim.

## Tests

Existing storage range tests include numeric ordering coverage in
`crates/nimbus-storage/src/index/tests.rs`. SQLite's SQL query-plan tests assert
the runtime SQL shape in
`crates/nimbus-storage/src/tests/sqlite_foundation/schema.rs`. SQL backend
range paths are covered through the provider suites and the final MBA14
verification commands.
