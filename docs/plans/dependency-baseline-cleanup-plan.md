# Dependency Baseline Cleanup Plan

## Status

- Active
- Owner: storage/dependency cleanup follow-up
- Last updated: 2026-04-11

## Purpose

Own the post-storage-hardening dependency-baseline cleanup needed to restore a
clean `cargo deny` / `make ci` result without weakening Neovex's dependency
policy. The immediate focus is the `libsql` dependency chain that currently
pulls in an advisory-affected `rustls-webpki` lane and the
`CDLA-Permissive-2.0`-licensed `webpki-roots` crate even though Neovex's remote
`libsql` usage does not require `libsql`'s builtin TLS stack.

## Relationship To Other Plans

- `docs/plans/archive/storage-layer-hardening-plan.md` is completed historical
  context. This plan owns the remaining dependency-baseline cleanup discovered
  during its closeout verification.
- `docs/plans/archive/sqlite-replica-provider-plan.md` is completed historical
  context for the current `libsql` provider topology. This plan must preserve
  that provider's behavior while tightening its dependency shape.

## Current Assessed State

- The original `libsql -> hyper-rustls 0.25 -> rustls-webpki 0.102.8` and
  `webpki-roots` deny failures are fixed in the live worktree.
- The current workspace now depends on `libsql` with
  `default-features = false, features = ["remote"]`.
- Neovex uses `libsql` only for remote Hrana access. The repo no longer uses
  `libsql` embedded replication or local libsql-backed SQLite access.
- Neovex already centralizes remote `libsql` database construction in
  `crates/neovex-storage/src/libsql.rs::open_remote_database`.
- After rebasing onto the latest `origin/main`, `make deny` is still not clean,
  but the remaining failure is now `RUSTSEC-2026-0097` through existing `rand`
  versions elsewhere in the graph rather than the removed `libsql` TLS lane.

## Current Review Findings

- `libsql 0.9.30` enables `hyper-rustls` with the `webpki-roots` feature in its
  published manifest even though the runtime connector path uses
  `.with_native_roots()`.
- Because Neovex only uses remote `libsql`, the canonical seam is to let
  Neovex own connector construction and to disable `libsql`'s builtin TLS
  feature set entirely.
- That cleanup succeeded: the old `libsql` advisory/license chain no longer
  appears in the workspace graph.
- The newly observed `rand` advisory failures are broader dependency-baseline
  work and should not be misattributed to the now-removed `libsql` TLS path.
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

## Success Criteria

- `libsql` no longer pulls `rustls-webpki 0.102.8` or `webpki-roots` into the
  Neovex dependency graph.
- Replica-provider behavior and tests still pass with the cleaned-up connector
  path.
- `make deny` passes without adding `CDLA-Permissive-2.0` to the allow list and
  without suppressing `RUSTSEC-2026-0049`.
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
| DB3 | in_progress | the `libsql` cleanup is verified, but after rebasing onto the latest `main` the remaining deny blocker is now `RUSTSEC-2026-0097` through transitive `rand` versions outside the removed `libsql` TLS lane |

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
- [ ] Observe `make ci` through completion or record the exact blocker if the
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
