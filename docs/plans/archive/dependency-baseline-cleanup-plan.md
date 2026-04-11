# Dependency Baseline Cleanup Plan

This archived plan records the verified dependency-baseline cleanup that
followed the storage-hardening closeout and restored a clean `make deny` /
`make ci` baseline without re-expanding Neovex's `libsql` dependency surface.

It should not be resumed as live progress state.

## Status

- **Status:** `completed`
- **Archived on:** `2026-04-11`
- **Activation source:** promoted on `2026-04-11` after the storage-hardening
  closeout surfaced a real dependency-baseline cleanup slice that should not
  stay as ad hoc follow-up work
- **Scope:** historical dependency-baseline cleanup record only; do not revive
  this completed plan as a live progress tracker
- **Owner:** storage/dependency cleanup follow-up
- **Last updated:** `2026-04-11`

## Purpose

This plan owned the post-storage-hardening dependency-baseline cleanup needed
to restore a clean `cargo deny` / `make ci` result without weakening Neovex's
dependency policy. Its immediate focus was the `libsql` dependency chain that
pulled in an advisory-affected `rustls-webpki` lane and the
`CDLA-Permissive-2.0`-licensed `webpki-roots` crate even though Neovex's remote
`libsql` usage does not require `libsql`'s builtin TLS stack.

## Relationship To Other Plans

- `docs/plans/archive/storage-layer-hardening-plan.md` is completed historical
  context. This plan owns the remaining dependency-baseline cleanup discovered
  during its closeout verification.
- `docs/plans/archive/sqlite-replica-provider-plan.md` is completed historical
  context for the current `libsql` provider topology. This plan must preserve
  that provider's behavior while tightening its dependency shape.
- `docs/plans/storage-provider-contracts-and-observability-plan.md` is the
  active owner of the next storage follow-up workstream. This archived record
  does not own ongoing storage naming, observability, or schema-metadata
  cleanup.

## Current Assessed State

- The original `libsql -> hyper-rustls 0.25 -> rustls-webpki 0.102.8` and
  `webpki-roots` deny failures are fixed in the live worktree.
- The current workspace now depends on `libsql` with
  `default-features = false, features = ["remote"]`.
- Neovex uses `libsql` only for remote Hrana access. The repo no longer uses
  `libsql` embedded replication or local libsql-backed SQLite access.
- Neovex already centralizes remote `libsql` database construction in
  `crates/neovex-storage/src/libsql.rs::open_remote_database`.
- `Cargo.lock` has now been tightened so the former `rand 0.9.2` and
  `rand 0.10.0` advisory lanes resolve to `0.9.3` and `0.10.1`; only the
  transitive `rand 0.8.5` lane remains, and it is covered by the narrow
  evidence-backed ignore recorded here.
- The final closeout verification is now complete: `cargo fmt --all --check`,
  `make check`, `make deny`, and `make ci` all pass in the live worktree.
- `RUSTSEC-2026-0097` remains ignored narrowly for the transitive `rand 0.8.5`
  lane only, backed by the documented feature-graph and logger-path evidence in
  this record.
- Remaining `cargo deny` duplicate warnings are tolerated upstream major-line
  splits rather than unresolved Neovex-owned version drift.

## Current Review Findings

- `libsql 0.9.30` enables `hyper-rustls` with the `webpki-roots` feature in its
  published manifest even though the runtime connector path uses
  `.with_native_roots()`.
- Because Neovex only uses remote `libsql`, the canonical seam is to let
  Neovex own connector construction and to disable `libsql`'s builtin TLS
  feature set entirely.
- That cleanup succeeded: the old `libsql` advisory/license chain no longer
  appears in the workspace graph.
- The `rand` advisory follow-up was broader dependency-baseline work and should
  not be misattributed to the now-removed `libsql` TLS path.
- The remaining `rand 0.8.5` lane is currently transitive through
  `num-bigint` and `phf_generator`; latest available direct dependency lines in
  the live worktree do not remove it.
- The advisory text for `RUSTSEC-2026-0097` requires `rand`'s `log` and
  `thread_rng` features plus a custom `log` logger path. `cargo tree -e
  features -i rand@0.8.5 --workspace` confirms the live `0.8.5` lane does not
  enable `log`, and repo code inspection shows Neovex uses
  `tracing_subscriber` rather than a custom `log::Log` implementation.
- `cargo tree -d --workspace` now reports a broader duplicate-version warning
  set. Most of those splits are upstream major-line boundaries rooted in
  `libsql`, `tokio-postgres`, `mysql_async`, and testcontainers, not in
  inconsistent Neovex workspace pins.
- The clearest directly-owned duplicate at plan start was
  `tokio-tungstenite`: Neovex pinned `0.27` for test fixtures while the server
  stack already resolved `0.28` through `axum`. This plan lifted the direct
  workspace pin to `0.28`.
- The sandbox cannot complete final `cargo deny` verification because the
  advisory database lock under `~/.cargo` is read-only there; final verification
  must use the repo's `make deny` path outside the sandbox or record an
  environmental blocker.

## Cleanup Invariants

- Do not weaken the deny policy to paper over fixable dependency problems.
- Preserve current `libsql` provider semantics, including remote-primary writes,
  provider-owned local snapshot caches, reconnect drills, and namespace
  management.
- Keep the provider-owned transport seam explicit: Neovex owns connector/config
  selection; `libsql` remains a remote SQL protocol client, not the canonical
  TLS/config authority.
- Do not introduce filesystem-shaped replica coupling or reintroduce
  embedded-replica code paths.

## Control Plane Rules

- Work one roadmap item at a time.
- Keep this plan aligned with the worktree after every meaningful burst.
- If final deny verification is blocked by environment restrictions, record the
  exact limitation before stopping.

## Canonical Design Decisions

### `libsql` is remote-only in Neovex

Neovex should depend on `libsql` with `default-features = false` and only the
remote feature set required by the current provider/tests. Embedded replication,
sync, and builtin TLS are out of scope for the live provider topology.

### Neovex owns the connector seam

Neovex should provide the connector passed into `Builder::connector(...)` for
both plain HTTP and HTTPS paths. That keeps dependency shape, trust-root policy,
and transport behavior explicit at the Neovex boundary instead of inheriting
upstream defaults accidentally.

### Deny policy stays strict

If a clean technical fix exists, prefer dependency or feature-shape cleanup over
advisory ignores or license allow-list expansion.

If an advisory's documented trigger conditions are not present in the live
workspace and a practical upstream/version fix is not yet available, a narrow,
fully documented ignore is acceptable. Any such ignore must be backed by
feature-graph evidence and code inspection, not by assumption.

## Success Criteria

- `libsql` no longer pulls `rustls-webpki 0.102.8` or `webpki-roots` into the
  Neovex dependency graph.
- Replica-provider behavior and tests still pass with the cleaned-up connector
  path.
- `make deny` passes without adding `CDLA-Permissive-2.0` to the allow list and
  without suppressing `RUSTSEC-2026-0049`.
- If `RUSTSEC-2026-0097` cannot be removed transitively yet, any remaining
  ignore is narrow, evidence-backed, and documented in this plan.
- The plan, plan index, and agent entrypoint reflect the live owner of this
  cleanup workstream.

## Verification Contract

- Focused dependency inspection:
  - `cargo tree -i rustls-webpki --workspace`
  - `cargo tree -i webpki-roots --workspace`
- Focused provider verification:
  - `cargo test -p neovex-storage libsql_provider -- --nocapture`
  - `cargo test -p neovex-engine sqlite_replica_provider -- --nocapture`
- Required workspace verification:
  - `cargo fmt --all --check`
  - `make check`
  - `make deny`
- If practical after the dependency change:
  - `make ci`

## Known Risks

- Owning the HTTPS connector locally may require a small transport shim because
  `libsql`'s connector trait is built around Hyper 0.14-era connection types.
- Final deny verification may require escalation outside the sandbox because of
  advisory DB locking behavior.

## Roadmap Status Ledger

| Item | Status | Notes |
| --- | --- | --- |
| DB1 | done | `libsql` now uses a remote-only feature set and a Neovex-owned connector seam |
| DB2 | done | Focused provider verification and dependency graph checks passed |
| DB3 | done | `make deny` and `make ci` are green after the narrow `RUSTSEC-2026-0097` ignore, the `tokio-tungstenite 0.28` lift, and the libsql journal-recovery refresh-race fix uncovered during full-CI closeout |

## Dependency Graph

- DB1 -> DB2
- DB2 -> DB3

## Recommended Delivery Order

1. DB1
2. DB2
3. DB3

## Implementation Checkpoints

- [x] Confirm the exact deny failures and their dependency chain.
- [x] Confirm upstream `libsql` does not already fix the issue with a simple
  version bump.
- [x] Convert the workspace to a remote-only `libsql` dependency shape.
- [x] Land the Neovex-owned HTTPS connector path.
- [x] Re-run focused provider tests.
- [x] Re-run `make deny` and record the original `libsql`-lane result.
- [x] Record the post-rebase `make deny` blocker now that the remaining failure
  has shifted to `rand` advisories outside the removed `libsql` TLS lane.
- [x] Update `Cargo.lock` to the patched `rand 0.9.3` and `0.10.1` lines where
  upstream resolutions exist.
- [x] Confirm the remaining `rand 0.8.5` lane does not enable the advisory's
  `log` feature precondition and that Neovex does not install a custom
  `log::Log` implementation.
- [x] Inventory duplicate-version warnings and separate directly-owned cleanup
  candidates from upstream major-line splits.
- [x] Lift the direct `tokio-tungstenite` workspace pin to the already-resolved
  `0.28` line if the websocket fixtures compile cleanly against it.
- [x] Re-run `make deny` with the narrow `RUSTSEC-2026-0097` ignore and record
  the final baseline.
- [x] Observe `make ci` through completion or record the exact blocker if the
  full lane remains inconclusive.

## Work Items

### DB1. Remote-Only `libsql` Dependency Shape

Reduce the workspace `libsql` dependency to only the features Neovex uses
today, remove reliance on `libsql`'s builtin TLS dependency chain, and make the
connector path explicit in Neovex storage code.

### DB2. Dependency-Graph and Provider Verification

Prove that the advisory/license chain is gone and that the replica provider
still behaves correctly under the reconnect and snapshot-refresh paths.

### DB3. Closeout

Run workspace verification, update the plan ledger/checkpoints/execution log,
and archive or hand off only if the dependency baseline is actually clean.

## Execution Log

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-11 | DB1 | in_progress | Confirmed the live deny failures are rooted in `libsql`'s builtin TLS dependency shape; promoted this cleanup into its own active control plan. | `cargo tree -i rustls-webpki@0.102.8 --workspace`; `cargo tree -i webpki-roots@0.26.11 --workspace`; manifest inspection of `libsql 0.9.30` | Narrow `libsql` to remote-only features and land the Neovex-owned connector path |
| 2026-04-11 | DB1 | done | Reduced the workspace `libsql` dependency to `default-features = false, features = ["remote"]`, added a Neovex-owned native-TLS-backed connector, and updated direct `libsql` test helpers to use that seam. | `cargo check -p neovex-storage -p neovex-engine`; `cargo tree -i rustls-webpki --workspace`; `cargo tree -i webpki-roots --workspace` | Run focused provider tests and workspace verification |
| 2026-04-11 | DB2 | done | Verified the replica-provider lanes after the transport change; the vulnerable `rustls-webpki 0.102.8` lane and all `webpki-roots` nodes dropped out of the workspace graph. | `cargo test -p neovex-storage libsql_provider -- --nocapture`; `cargo test -p neovex-engine sqlite_replica_provider -- --nocapture`; `cargo tree -i rustls-webpki --workspace`; `cargo tree -i webpki-roots --workspace` | Run workspace verification and trim stale deny allow-list entries |
| 2026-04-11 | DB3 | in_progress | `cargo fmt --all --check`, `make check`, and `make deny` passed. Also removed stale `deny.toml` allow-list entries that no longer appear in the resolved graph. A full `make ci` run was started and observed through the long-running engine/provider test phase, but it had not completed by checkpoint time. | `cargo fmt --all --check`; `make check`; `make deny`; `make ci` (started, observed through workspace test execution; completion not yet observed) | Resume from the live `make ci` attempt or rerun/record the final full-CI outcome |
| 2026-04-11 | DB3 | reblocked | Rebased onto the latest `origin/main` and re-ran `make deny` outside the sandbox. The original `libsql` TLS-chain failures remain fixed, but the deny baseline is no longer clean because the updated graph now trips `RUSTSEC-2026-0097` through transitive `rand` 0.8.5 / 0.9.2 / 0.10.0 lanes. This is a real new dependency-baseline blocker, not a regression in the completed `libsql` cleanup. | `make deny` (rerun outside the sandbox after rebase to latest `origin/main`) | Keep this plan active for the new `rand` advisory cleanup or promote a narrower follow-on if ownership changes |
| 2026-04-11 | DB3 | narrowed | Updated `Cargo.lock` to the patched `rand 0.9.3` and `0.10.1` lines, documented and applied a narrow `RUSTSEC-2026-0097` ignore based on the advisory's unmet trigger conditions, and re-ran `make deny` cleanly. | `cargo tree -i rand@0.8.5 --workspace`; `cargo tree -e features -i rand@0.8.5 --workspace`; repo search for custom `log::Log` usage; `make deny` | Trim any directly-owned duplicate-version warnings that can be removed without larger upstream churn |
| 2026-04-11 | DB3 | improved | Lifted Neovex's direct `tokio-tungstenite` pin from `0.27` to `0.28`, removing the workspace-owned websocket duplicate while leaving upstream `libsql`/Postgres/MySQL/testcontainers major-line splits in place. | `cargo tree -d --workspace`; `cargo check -p neovex-testing -p neovex-server`; `cargo test -p neovex-server --test reactive_loop -- --nocapture`; `cargo test -p neovex-server --lib -- --nocapture`; `make deny` | Decide whether any remaining duplicate-version warnings are worth further direct cleanup or should stay as tolerated upstream splits while DB3 moves toward `make ci` |
| 2026-04-11 | DB3 | done | Cleared the stale single-flight `cargo test --workspace` blocker, fixed the replica-provider durable-journal refresh race that full `make ci` exposed in `libsql_durable_journal_recovery_refreshes_local_cache_from_remote_records`, and observed a full green `make ci` closeout. | `ps -ax -o pid,time,etime,state,command`; `kill` on the stale single-flight wrapper/cargo/test PIDs; `cargo fmt --all --check`; `cargo test -p neovex-storage libsql_durable_journal_recovery_refreshes_local_cache_from_remote_records -- --nocapture`; `cargo test -p neovex-storage libsql_provider -- --nocapture`; `make ci` | Archive this completed plan and leave future dependency follow-up to a newly promoted active plan if needed |
| 2026-04-11 | DB3 | archived | Archived the completed plan and updated `docs/plans/README.md` plus `AGENTS.md` so fresh agents treat this workstream as historical-only context instead of live progress state. | doc audit across this plan, `docs/plans/README.md`, and `AGENTS.md` | Historical record only; promote a new active plan if future dependency cleanup becomes necessary |
