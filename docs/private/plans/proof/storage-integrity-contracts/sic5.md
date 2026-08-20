# SIC5 — Provider qualification matrix

Base: `codex/sic-sic4` @ `65aa7a52` (SIC4 work, PR #287).
Machine: darwin 24.6.0, aarch64, rustc 1.96.1.

## 1. The gap

Finding F5: the provider suites proved different things for different
providers, and nothing named the difference. A dimension a provider never
exercised looked identical to a dimension it exercised and passed, because the
only evidence either way was the presence or absence of a test function that no
inventory tracked.

Two failure shapes followed, and `sic5-failbefore.txt` shows both on the
baseline. A registered scenario could be deleted with the suite still green
(part B): PostgreSQL simply stopped proving historical index visibility and no
diagnostic reported the loss. And a remote provider's whole suite passed
vacuously whenever its fixture was absent — the wrapper ran, found no fixture,
and returned success — so an unconfigured lane was indistinguishable from a
qualified one. That second shape is the invariant-12 hazard directly:
"unavailable provider or host lanes are UNVERIFIED, never green."

## 2. The contract

`crates/nimbus-storage/src/tests/provider_contract_matrix.rs` holds a closed
42-cell product: six providers by seven semantic dimensions, each cell either
`Qualified(<test name>)` or `NotOwned(<reason>)`.

```rust
enum Dimension {
    AtomicEffects,        // effects commit together or not at all
    CommitterFencing,     // a stale committer cannot apply
    ConditionalAdmission, // sequence reuse is admitted or rejected by content
    JournalProgress,      // durable head and applied head round-trip
    DurableRecovery,      // durable-but-unapplied records replay
    WriteIsolation,       // a pending prefix blocks a zero-write
    PositionParity,       // the materialized position matches the reference
}
```

Position parity is SIC4's `MaterializedPosition` reused as the cross-provider
oracle: every provider must reach the same digest for the same pinned history,
so the matrix's last row is a genuine equivalence and not six independent
self-consistency checks.

`NotOwned` is a declared position, not a hole. redb, memory, and SQLite hold no
committer lease, so there is no stale owner to fence; the matrix says so and
cross-checks the claim against
`impl_unsupported_fenced_durable_apply!` in `traits/provider_impls.rs`.

## 3. Why it cannot go vacuous

`provider_contract_matrix_is_complete` runs six checks, and each one closes a
distinct way the matrix could quietly stop meaning anything:

| # | Check | The drift it catches |
|---|---|---|
| 1 | Roster equals the `impl_durable_journal!` and `impl_point_write!` registrations | A seventh provider ships unqualified |
| 2 | The product is closed and each pair appears exactly once | A dimension is dropped for one provider |
| 3 | Fencing ownership agrees with `impl_committer_lease_store!` and `impl_unsupported_fenced_durable_apply!` | A provider gains or loses a lease and the matrix still claims the old story |
| 4 | Every `Qualified` cell names a `fn` that exists in the test tree | A scenario is deleted or renamed |
| 5 | The matrix agrees with the profile `diagnostics.rs` publishes | The operator-visible guarantee drifts from the proven one |
| 6 | redb, memory, and SQLite are always `Available` and never `Unverified` | The whole gate degrades to UNVERIFIED and reports nothing |

Checks 1, 3, 4, and 5 read the real source text at runtime through
`CARGO_MANIFEST_DIR`, following the SIC3 structural-gate precedent — never
compile-time `env!`, which the F2 test-taxonomy check bans.

Check 5 is the one that had to be rewritten. The first draft restated the
per-provider profile as a `match` inside the test, so flipping redb's published
profile to `FENCED` in `diagnostics.rs` passed the gate: the test was comparing
the matrix against itself. `published_profile` now parses the store's `impl`
block in `diagnostics.rs` and fails on any profile constant it does not know.
`sic5-failbefore.txt` part F records the edit that used to pass and now fails.

## 4. Unavailable lanes report UNVERIFIED

`Availability` separates the two reasons a lane cannot run — the cargo feature
is off, or the feature is on and the fixture environment is absent — and
`status()` degrades a `Qualified` cell to `Unverified` in either case. A
`NotOwned` cell stays `NotOwned`, because not owning a guarantee is a fact
about the provider, not about this host.

`provider_contract_matrix_reports_unavailable_lanes_as_unverified` asserts the
rule for PostgreSQL, MySQL, and libSQL: when the lane cannot run, no cell of it
may read `Qualified`. `sic5-matrix.txt` is the report from this machine — the
three local providers fully qualified, the three remotes UNVERIFIED with the
reason and the scenario each row is waiting on.

Nothing in the gate probes a live provider. The published profile is a
compile-time constant per store type, per plan step 5, so reading capabilities
costs no request.

## 5. Shared scenarios

`crates/nimbus-storage/src/tests/contract_scenarios.rs` is new and always
compiled. It holds the provider-independent bodies — journal progress
round-trip, durable recovery replay, and materialized position parity against a
pinned record set — so the six providers run the same assertions rather than
six paraphrases of them. `MaterializedPositionOracle` is the test-only bridge
over each store's inherent `export_materialized_journal_snapshot`.

Twelve wrappers are new: redb and memory gained progress, recovery, position,
and the two PPSC cases; SQLite gained progress and position; PostgreSQL, MySQL,
and libSQL each gained position parity.

The two new SQLite wrappers joined the `sqlite_write_observation` serial group.
They open their own stores, and without it they added write load that pushed
`sqlite_resident_writer_coexists_with_concurrent_point_writers` past its 5s busy
timeout on a loaded host. The concurrency assertion was not weakened.

## 6. Diagnostics

`StorageCapabilities` and `StorageHealthDiagnostic` carry a
`SemanticContractProfile` — the seven dimensions as `Qualified` or `NotOwned` —
with two constants: `FENCED` for the SQL-backed providers and `LOCAL_UNFENCED`
for the three local ones. `MemoryTenantStore` gained a `storage_capabilities`
method so all six providers publish through the same surface, and
`TableBackendLayout` gained `InMemoryKeyspaceByTableId` to describe it
honestly.

`docs/private/operating/storage-backends.md` documents the profile, the
backend-to-profile table, and the UNVERIFIED rule. That runbook sits outside
the force-tracked `docs/private` subset, so the update stays local rather than
joining this pull request.

## 7. Verification

| Command | Result |
|---|---|
| `cargo test -p nimbus-storage --lib provider_contract_matrix -- --nocapture` | 2 passed; report in `sic5-matrix.txt` |
| same, `--features libsql,mysql,postgres` | 2 passed; remotes UNVERIFIED |
| `cargo test -p nimbus-storage --lib memory_conformance` | 21 passed |
| `cargo test -p nimbus-storage sqlite_foundation::journal` | 34 passed, 5 consecutive runs |
| `cargo test -p nimbus-storage materialized_position` (all features) | 8 passed |
| fail-before D, E, F | each fails the gate; `sic5-failbefore.txt` |
| plan verifier | condition 12 green; `Summary: 12 passed, 1 failed`, the remaining failure is SIC6's; `sic5-verifier.txt` |
| `make ci` | `MAKE_RC=2`; 7448 run, 7445 passed, 3 failed. All three are wall-clock budget tests in unrelated crates and each passes alone; see `sic5-ci-lanes.txt` |
