# MBA0 ExtendDB Pattern Map

Date: 2026-05-27

ExtendDB source: `/Users/jack/src/github.com/ExtendDB/extenddb`

This map records which ExtendDB patterns are direct inputs to the MBA plan and
where Nimbus must adapt them instead of copying them mechanically.

| MBA | ExtendDB source | Pattern | Nimbus target | Adoption posture |
|-----|-----------------|---------|---------------|------------------|
| MBA1 | `docs/technical-debt.md` | Central technical-debt ledger with IDs, categories, locations, priority, and origin. | `docs/technical-debt.md`, `docs/README.md`, `AGENTS.md` | Direct documentation pattern, adjusted to Nimbus category and owner fields; seed only Nimbus-owned actionable debt. |
| MBA2 | `crates/storage/src/lib.rs` | Focused storage traits: `TableEngine`, `DataEngine`, `MetadataEngine`, `StreamEngine`, `WorkerStore`, `BackupEngine`; composite `StorageEngine`; composite `CatalogStore`. | `crates/nimbus-storage/src/traits/` and current `async_storage` boundary | Source-backed design pattern, but not a taxonomy import. Split only around Nimbus call-site pressure and real ownership boundaries. |
| MBA3 | `crates/storage/src/server_components.rs`; `crates/storage-postgres/src/lib.rs` | Backend factory registration through `inventory::collect!` and `inventory::submit!`. | Adapter and backend registration in `nimbus-server`, `nimbus-engine`, and `nimbus-storage` | Source-backed option only. Nimbus should keep an explicit typed registry unless backends/adapters become cross-crate plugin-style registrations. |
| MBA4 | `crates/storage/src/hooks.rs`; `crates/storage-postgres/src/lib.rs` | Backend-owned runtime hooks spawn provider-coupled workers and expose backend info. | Replace `ProviderBackgroundTask` branches in `crates/nimbus-engine/src/service/provider_hints.rs` | Direct pattern for provider-coupled workers only; engine-owned generic workers stay in the engine. |
| MBA5 | `tests/conftest.py`; `tests/test_auth_error_fidelity.py` | Same test body can target real DynamoDB or ExtendDB through an endpoint environment variable. | `tests/dual-target/<adapter>/` using `NIMBUS_TEST_TARGET` | Direct testing pattern, staged with narrow fidelity tests first and real-service targets in credentialed nightly/weekly lanes. |
| MBA6 | `docs/manuals/02-design-guide.md` | Credential and catalog state are not cached; only operational knobs and immutable encryption key are cached. | `docs/decisions/00X-auth-caching-policy.md` plus auth-code audit | Source-backed policy input for security-sensitive auth/policy state. Nimbus must classify existing operational/data caches instead of deleting them blindly. |
| MBA7 | `docs/adr/0002-sql-injection-defense.md`; `crates/storage-postgres/src/data/mod.rs` | Two-tier SQL defense: validate identifiers before storage, bind values, funnel dynamic identifiers through named helpers. | One SQL-safety ADR per Nimbus SQL backend | Direct security pattern, but the gate must use helper allowlists and documented exemptions because Nimbus has fixed internal SQL plus multiple dialects. |
| MBA8 | `crates/server/src/handler.rs` | Per-request segment timings recorded for auth, authz, throttle, dispatch, response, and total. | `docs/operating/latency-budgets.md` and request/runtime hot paths | Timing pattern is direct; budget values and WARN thresholds require Nimbus baseline evidence first. |
| MBA9 | `docs/manuals/02-design-guide.md`; `crates/storage/src/lib.rs` | Object-safe storage traits use `BoxFuture`; auth traits use `async_trait`; dyn compatibility is tested. | `docs/architecture/trait-conventions.md` and trait audit | Direct convention source for object-erased traits only. Static-dispatch async traits do not need BoxFuture conversions. |
| MBA10 | `docs/manuals/02-design-guide.md`; `crates/storage-postgres/src/data/mod.rs`; `crates/storage-postgres/src/create_table.rs`; Convex `crates/value/src/table_mapping.rs` and `crates/value/src/id_v6.rs` | ExtendDB maps user-facing table names to UUID-backed physical data names (`table_id`, `index_id`). Convex maps user-facing table names to internal `TabletId`/`TableNumber` identities and encodes the table number in current developer document IDs. | Stable logical table identity plus backend-owned physical-layout matrix | Adopt the logical identity lesson; keep shared physical storage by default. Per-table physical SQL tables remain a measured backend-specific optimization, not the cross-backend invariant. |
| MBA11 | `crates/storage-postgres/src/data/mod.rs` | Sort keys use typed string/number/binary storage so range ordering is correct. | SQL index/range scan storage for user-typed keys | Direct correctness pattern for SQL ordered scans; non-SQL/native-key backends document their encoding instead. |
| MBA12 | `docs/technical-debt.md` (`F-2`) and DynamoDB `ConsistentRead` semantics | ExtendDB tracks eventually-consistent read routing as known debt. | `docs/architecture/storage/consistency-routing.md` | Inspiration only. Nimbus must define per-adapter contracts and route only across real backend consistency surfaces. |
| MBA13 | `crates/storage/src/lib.rs` (`StreamCapture`); `docs/manuals/02-design-guide.md` | Engine/application constructs stream capture metadata; storage persists stream records atomically with data writes. | `docs/architecture/adapters/event-capture.md` and subscription/trigger paths | Direct atomicity pattern, preserving Nimbus's current engine/storage trigger ownership while keeping adapter wire shapes out of storage. |

## Source Notes

ExtendDB's documentation and code are not perfectly worded in every historical
comment. For example, some comments still mention `_ddb_<TableName>`, while
the current implementation creates table and index IDs with UUIDs and routes
through `data_table_name(table_id)` / `index_table_name(index_id)`. MBA work
should trust current code plus the design guide over stale comments.

ExtendDB's storage traits are DynamoDB-shaped. Nimbus must not import that
vocabulary wholesale because Nimbus's core model is tenant/document/journal
storage shared by multiple adapters.
