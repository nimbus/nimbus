# Postgres Listener Reconnect Schema Recovery Plan

Archived execution record for the small Postgres provider reconnect correctness
follow-up found during archival review of the completed storage follow-up
workstream.

Reviewed against:

- `docs/plans/README.md`
- `docs/plans/archive/storage-provider-contracts-and-observability-plan.md`
- `crates/neovex-engine/src/service/provider_hints.rs`
- `crates/neovex-engine/src/service/schema.rs`
- `crates/neovex-engine/src/tests/postgres_provider.rs`
- `crates/neovex-storage/src/postgres.rs`

---

## Status

- Archived
- Owner: Postgres reconnect correctness follow-up
- Last updated: 2026-04-11

## Purpose

Close the remaining correctness gap in the Postgres provider hint worker:
when the LISTEN connection drops and later reattaches, loaded tenants must
recover missed schema changes as authoritatively as they recover missed journal
progress.

## Relationship To Other Plans

- `docs/plans/archive/storage-provider-contracts-and-observability-plan.md`
  remains archived historical context. This plan exists because archival review
  found one remaining reconnect correctness gap that should not be silently
  treated as complete inside that archived record.
- `docs/plans/archive/postgres-storage-provider-plan.md` remains historical
  context for the original Postgres provider implementation.

## Current Assessed State

- Ordinary Postgres notification flow refreshes loaded runtime schema and
  journal state correctly.
- Listener reattach now performs authoritative schema and journal catch-up for
  already-loaded tenants, so missed schema notifications during LISTEN downtime
  no longer leave runtime schema snapshots or store-local schema caches stale.
- The focused reconnect regression now proves this by performing a schema write
  and a document write while the listener is disconnected and then asserting
  both recover after reconnect.

## Invariants

- `Service::apply_mutation` remains the only mutation path.
- Reconnect recovery must use authoritative provider state, not best-effort
  inference from runtime state.
- Fix the reconnect boundary without widening this work into unrelated provider
  refactors.
- Historical archived plans stay historical; this plan owns the new execution
  pass.

## Success Criteria

1. Loaded Postgres tenants recover missed schema changes after listener
   reattachment, not just missed journal progress.
2. There is a focused regression test that performs a schema write while the
   listener is disconnected and proves the loaded runtime refreshes after
   reconnect.
3. The archived storage follow-up record is updated to note that this extra
   reconnect correctness fix landed after archival review.

## Verification Contract

- `cargo fmt --all --check`
- `cargo check -p neovex-engine`
- `cargo test -p neovex-engine postgres_provider -- --nocapture`

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| PR1 | `done` | make listener reattach perform authoritative schema and journal recovery for loaded tenants | none | complete |
| PR2 | `done` | add a reconnect regression that proves missed schema notifications are recovered | PR1 | complete |
| PR3 | `done` | rerun focused verification and update historical plan/docs to reflect the fix | PR1, PR2 | archived |

## Execution Log

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-11 | PR1 | `in_progress` | Promoted a small active follow-up after archival review found that the Postgres reconnect sweep still recovered missed journal state but not missed schema changes for loaded tenants. | code review across `provider_hints.rs`, `schema.rs`, `postgres.rs`, and `postgres_provider.rs` | patch the attach path and add the missing regression |
| 2026-04-11 | PR1 | `done` | Updated the Postgres listener-attach recovery path so every successful reattach performs authoritative schema and journal catch-up for already-loaded tenants. That keeps runtime schema snapshots and store-local schema caches correct even when schema notifications were missed during LISTEN downtime. | `cargo fmt --all --check`; `cargo check -p neovex-engine` | land the focused reconnect regression |
| 2026-04-11 | PR2 | `done` | Expanded the reconnect regression to perform an external schema write while the listener is disconnected and to prove the loaded runtime regains both schema and journal state after the listener reconnects. | `cargo test -p neovex-engine postgres_provider -- --nocapture` | update the historical storage follow-up record and archive this plan |
| 2026-04-11 | PR3 | `done` | Archived the follow-up plan and updated the storage follow-up historical record so future reviews see the corrected final state instead of the earlier premature closeout. | `cargo fmt --all --check`; `cargo check -p neovex-engine`; `cargo test -p neovex-engine postgres_provider -- --nocapture` | archived |
