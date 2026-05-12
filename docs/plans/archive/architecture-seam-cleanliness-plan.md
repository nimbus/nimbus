# Plan: Architecture Seam Cleanliness

Canonical control plane for the completed repo-wide architecture and
modularity cleanup wave.

This plan replaces the earlier ad hoc findings list with the same execution
style used by the more recent control plans: a live findings ledger, explicit
ownership rules, context-window sizing, roadmap item status, and a running
execution log.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `AGENTS.md`
- `docs/architecture/testing/reliability-posture.md`
- `docs/architecture/testing/ci-failure-investigation.md`
- the current git worktree on `2026-04-27`

---

## Status

- **Status:** `done`
- **Primary owner:** repo-wide architecture seam cleanup
- **Execution order:** completed baseline for the 2026-04-27 broad
  architecture/modularity/reliability cleanup wave; promote a new active plan
  before landing another repo-wide seam pass
- **Verification posture:** every implementation item must record focused proof
  lanes before broader workspace verification

## Why This Exists

Two broad architecture reviews found a mix of still-live issues and stale
historical findings. The repo has improved materially since the first audit:

- `TenantRuntime` is already split into owned submodules
- several old server/runtime seam issues have already been closed by later
  control plans
- some previously oversized test roots have already been reduced or moved

But a set of real cleanliness issues remains:

- test-only hooks still leak into a production dependency edge
- a few core seams still expose too much internal structure
- storage/provider abstractions still export or duplicate more than they should
- runtime invocation and admission responsibilities remain partly conflated
- some oversized roots and blanket allowances still need cleanup or explicit
  justification

This plan resolves the live issues only. It is not a changelog for already
completed cleanup waves.

## Relationship To Other Plans

- `docs/plans/archive/deployment-auth-runtime-boundary-plan.md`
  is the completed baseline for deploy/auth/runtime snapshot ownership.
- `docs/plans/archive/repo-architecture-and-seam-hardening-plan.md`
  is the completed repo-wide baseline for Firebase/Cloud Functions/runtime ABI
  cleanup.
- `docs/plans/archive/server-runtime-canonicalization-plan.md`
  is the completed baseline for durable lifecycle metadata and prior
  composition-root cleanup.
- `docs/plans/archive/runtime-capability-adapter-boundary-plan.md`
  is the completed baseline for provider-neutral runtime capability ownership.

This plan is the next maintainability wave on top of those baselines, not a
restart of them.

## Control Plan Rules

1. Keep the findings ledger truthful.
   If a prior finding is no longer live, remove or retire it instead of
   preserving stale debt.

2. Prefer explicit ownership boundaries over helper piles.
   Split by concept ownership, not by raw line count alone.

3. Pre-launch direct cleanup is preferred.
   Delete or narrow old surfaces rather than preserving compatibility shims.

4. Verify the seam, not just the file.
   Each item should prove the architectural contract that changed.

5. Do not touch the in-flight MongoDB adapter implementation owned elsewhere.
   MongoDB-related findings may be documented, but this plan does not own that
   adapter’s implementation wave.

## Live Findings Ledger

### High

No remaining high-priority findings are still live after this wave.

### Medium

| ID | Finding | Current evidence |
| --- | --- | --- |
| `ASC-M4` | Trigger-delivery and trigger-invocation persistence still duplicate backend-specific SQL and row encoding across SQLite/Postgres/MySQL, but this wave now documents that duplication as an explicit provider-owned dialect seam instead of accidental shared debt | `crates/neovex-storage/src/{sqlite,postgres,mysql}/trigger_*.rs` |
| `ASC-M7` | `RuntimePolicy` still owns limits, admission semaphore, and metrics together; accepted current trade-off because it remains one coherent runtime admission object | `crates/neovex-runtime/src/limits.rs` |

### Low

| ID | Finding | Current evidence |
| --- | --- | --- |
| `ASC-L2` | Oversized non-generated harness roots still exist: `start/tests.rs` (`1676`) remains explicitly justified in this plan, and `packages/firebase/src/selftest.mjs` (`3925`) remains a strong ownership-based exception until a future JS harness wave owns its decomposition | live line counts |
| `ASC-L3` | `neovex-testing` still depends directly on `neovex-storage`; retained and documented so deterministic harnesses can exercise real provider-backed stores without engine/server backedges | `crates/neovex-testing/Cargo.toml` |

## Retired Or Reframed Findings

These older claims from the previous draft are no longer accurate in their old
form and should not be treated as current debt:

- the original `TenantRuntime` “~4,900 LOC single god file” claim is stale;
  the problem is now ownership breadth, not one huge file
- the older oversized-test list is stale; the current hotspots are different
- the old `neovex-bin machine/` oversized-submodule finding is stale in the
  current tree
- `execution_units/batch.rs` is still substantial but no longer above the
  control-plan hard threshold by itself
- `ASC-H1`, `ASC-M1`, `ASC-M2`, `ASC-M3`, `ASC-M5`, `ASC-M6`, `ASC-M8`,
  `ASC-M9`, `ASC-M10`, `ASC-L1`, `ASC-L4`, `ASC-L5`, and `ASC-L6` are closed
  in the live tree and should not be carried forward as open debt

## Current Assessed State

- the production `test-hooks` dependency leak is removed
- runtime invocation concurrency/session seams and generic query ABI naming are
  settled for the current baseline
- Convex bridge access is now encapsulated behind owned accessors
- engine mutation APIs and `TenantRuntime` access are materially narrower and
  more discoverable
- storage/provider ownership is narrower, embedded blocking write execution is
  shared across the async storage seam, and remaining backend SQL duplication
  is explicitly documented as provider-owned
- the remaining accepted trade-offs are explicit, not hidden debt

## Context Window Scale

| Band | Estimated context | Expected shape |
| --- | --- | --- |
| `S` | 8k-12k tokens, 6-10 files | one docs/guidance or narrow seam cleanup |
| `M` | 12k-18k tokens, 8-14 files | one adapter/runtime/provider ownership slice |
| `L` | 18k-28k tokens, 12-20 files | one cross-layer abstraction cleanup |
| `XL` | 28k-40k tokens, 18-30 files | only for storage-provider deduplication or large engine API reshapes |

Rule:

- if an item needs more than its estimated band, split the item in this plan
  before continuing

## Roadmap Summary

| Item | Focus | Depends on | Context estimate | Status |
| --- | --- | --- | --- | --- |
| `ASC1` | Findings audit, control-plane rewrite, and repo-guidance refresh | — | `S` | `done` |
| `ASC2` | Gate `test-hooks` out of production dependency paths | `ASC1` | `M` | `done` |
| `ASC3` | Runtime invocation/session seam cleanup | `ASC1` | `L` | `done` |
| `ASC4` | Convex bridge encapsulation and read-tracking ownership docs | `ASC1` | `M` | `done` |
| `ASC5` | Engine surface cleanup (`TenantRuntime` discoverability + mutation API narrowing) | `ASC1` | `XL` | `done` |
| `ASC6` | Storage/provider seam cleanup | `ASC1` | `XL` | `done` |
| `ASC7` | Peripheral modularity cleanup (`dead_code`, test roots, adapter docs, runtime backend injection) | `ASC1` | `L` | `done` |
| `ASC8` | Closeout docs, final verification, and baseline refresh | `ASC2`, `ASC3`, `ASC4`, `ASC5`, `ASC6`, `ASC7` | `L` | `done` |

## Roadmap

### ASC1 — Findings Audit, Control-Plane Rewrite, And Repo-Guidance Refresh

Goal: replace the stale architecture draft with a truthful live control plan.

- audit each inherited finding against the current tree
- retire stale claims and add any newly confirmed seam issues
- update `docs/plans/README.md` and `AGENTS.md` so broad architecture seam work
  points here while this wave is active

Completion gate:

- this plan uses the standard control-plane structure and repo guidance points
  to it as the active owner for broad architecture seam work

### ASC2 — Gate `test-hooks` Out Of Production Dependency Paths

Goal: keep test-only engine hooks out of the production `neovex-server`
dependency edge.

- remove `features = ["test-hooks"]` from the production
  `neovex-server -> neovex-engine` dependency edge
- if tests still need those hooks, re-enable them only through dev-only
  dependency resolution or another explicitly test-only path
- verify release/no-dev dependency graphs do not include the feature

Completion gate:

- release-oriented `neovex-server` builds do not pull `test-hooks`

### ASC3 — Runtime Invocation And Session Seam Cleanup

Goal: make runtime invocation behavior more explicit and less payload-shaped.

- centralize `session_id` validation closer to runtime bridge entry or a typed
  payload-validation seam
- move `bypass_concurrency_limit` from runtime instance ownership to the
  invocation path
- decide whether the remaining `CtxDbQuery*` shared ABI names should become
  generic in this wave or in a scoped follow-on, and record that decision

Completion gate:

- invocation-scoped concurrency bypass is no longer a runtime instance field,
  and the `session_id` contract is more centralized than it is today

### ASC4 — Convex Bridge Encapsulation And Read-Tracking Ownership Docs

Goal: tighten the most sensitive adapter/runtime seam without another broad
runtime rewrite.

- make `ConvexHostBridge` internals private or narrower
- replace direct field access with owned accessors or capability methods
- document the split between shared read-tracking infrastructure and
  Convex-specific host-bridge read-recording code

Completion gate:

- adapter code no longer relies on direct `bridge.*` field reach-in for the
  targeted engine/runtime state

### ASC5 — Engine Surface Cleanup

Goal: reduce API sprawl and make the main tenant surface more discoverable.

- add subsystem-oriented `TenantRuntime` accessors or another ownership-first
  discoverability layer
- narrow or consolidate the direct mutation API so it no longer exposes 22
  near-duplicate public entry points

Completion gate:

- `TenantRuntime` presents clearer concept-owned entry points and the direct
  mutation API has materially fewer public variants

### ASC6 — Storage/Provider Seam Cleanup

Goal: make provider boundaries tighter and duplication more deliberate.

- narrow provider-specific public exports in `neovex-storage`
- decide whether persistence provider dispatch should stay macro-based or move
  to a narrower object seam, and implement or document that choice
- standardize the async write-boundary pattern where practical
- extract or document duplicated trigger-delivery / trigger-invocation /
  serialization seams across SQLite/Postgres/MySQL

Completion gate:

- provider ownership is narrower, and duplication/macros are either reduced or
  explicitly justified with current evidence

### ASC7 — Peripheral Modularity Cleanup

Goal: remove or justify the remaining low-priority seam smells.

- replace the blanket Cloud Functions `dead_code` allowance with targeted
  deferred-scope annotations
- document shared adapter expectations
- split or justify the live oversized non-generated test roots
- inject backend choice into run-to-completion worker-loop construction
- evaluate whether `neovex-testing` should continue depending directly on
  `neovex-storage` or whether that dependency needs narrowing/documentation

Completion gate:

- the remaining low-priority seam issues are either fixed or explicitly
  documented as accepted current trade-offs

### ASC8 — Closeout Docs, Final Verification, And Baseline Refresh

Goal: close the wave with accurate docs and verification evidence.

- update the plan ledger and execution log
- refresh `AGENTS.md`, `docs/plans/README.md`, and any affected architecture or
  reference docs
- run focused verification for each landed seam plus broader checks before
  closeout

Completion gate:

- docs reflect the landed architecture, proofs are recorded, and this plan can
  move from active owner to completed baseline language

## Execution Log

- `2026-04-27`: inherited draft audited and rewritten into the standard
  control-plane format. Live findings confirmed: `ASC-H1`, `ASC-H2`,
  `ASC-M1` through `ASC-M10`, and `ASC-L1` through `ASC-L6`. Older stale
  findings were retired or reframed. `ASC1` started and `ASC2` was queued next.
- `2026-04-27`: `ASC1` completed. Repo guidance in `AGENTS.md` and
  `docs/plans/README.md` now points broad architecture/modularity cleanup at
  this plan while it is active.
- `2026-04-27`: `ASC2` completed. `neovex-server` no longer enables
  `neovex-engine` `test-hooks` on its production dependency edge; the feature
  is now dev-only for server tests. Verified with
  `cargo tree -p neovex-server -e features --no-dev-dependencies | rg 'test-hooks|neovex-engine'`,
  `cargo check -p neovex-server`, and
  `cargo test -p neovex-server cloud_functions --lib`.
- `2026-04-27`: `ASC3` started. Landed invocation-scoped concurrency bypass in
  `RuntimeInvocationContext`, removed `bypass_concurrency_limit` from
  `NeovexRuntime`, added centralized `HostCallPayload::session_id()` access,
  and began renaming the shared query host ABI from Convex-shaped
  `CtxDbQuery*` names to generic `QueryBuilder*` / `QueryRead*` names.
  Focused proof so far:
  `cargo test -p neovex-runtime host_call --lib` and
  `cargo check -p neovex-runtime -p neovex-server`.
- `2026-04-27`: `ASC3` completed and `ASC4` completed. Runtime concurrency
  bypass is now invocation-scoped, host-call payloads expose centralized
  `session_id()` access, the shared query host ABI now uses generic
  `QueryBuilder*` / `QueryRead*` naming, and Convex host-bridge session
  validation happens at the dispatch seam. `ConvexHostBridge` internal state is
  now private behind owned accessors, direct `bridge.*` reach-in was removed
  from the Convex host-bridge tree, and shared versus Convex-specific
  read-tracking ownership is now documented inline. Verified with
  `cargo check -p neovex-runtime -p neovex-server`,
  `cargo test -p neovex-runtime host_call --lib`,
  `cargo test -p neovex-server adapters::convex::tests::contracts --lib`,
  `cargo test -p neovex-server adapters::convex::tests::metrics --lib`, and
  `cargo test -p neovex-server cloud_functions --lib`.
- `2026-04-27`: `ASC5` started. Current confirmed scope: `TenantRuntime` is no
  longer a single god file, but it still acts as a broad subsystem hub, and
  `service/mutations/direct/api.rs` still exposes a wide sync/async/cancellable
  matrix of document mutation entry points that needs concept-owned narrowing
  rather than another helper pile.
- `2026-04-27`: `ASC5` completed. `TenantRuntime` now exposes owned
  subsystem-oriented accessors, schema replacement moved behind a named
  method, and the direct mutation API was narrowed around `MutationActor` and
  `AsyncMutationContext` instead of preserving the older principal/cancellable
  method matrix. Verified with `cargo check -p neovex-engine -p neovex-server`,
  `cargo test -p neovex-engine mutation_journal --lib`,
  `cargo test -p neovex-engine policy --lib`,
  `cargo test -p neovex-server cloud_functions --lib`, and
  `cargo test -p neovex-server adapters::convex::tests::contracts --lib`.
- `2026-04-27`: `ASC6` completed. Embedded blocking-store write execution is
  now shared between redb and SQLite through `async_storage::write`,
  provider-specific `Opened*Tenant` shapes are no longer re-exported from the
  top-level `neovex-storage` facade, and persistence provider dispatch is now
  explicitly documented as a deliberate typed-provider seam rather than an
  accidental macro pile. Remaining trigger cursor/invocation SQL duplication is
  now documented inline as a provider-owned dialect seam instead of implied
  generic debt. Verified with
  `cargo check -p neovex-storage -p neovex-engine -p neovex-server`,
  `cargo test -p neovex-storage --lib`,
  `cargo test -p neovex-engine mutation_journal --lib`, and
  `cargo test -p neovex-server cloud_functions --lib`.
- `2026-04-27`: `ASC7` completed. Removed the blanket Cloud Functions
  `dead_code` allowance, documented shared adapter expectations in
  `docs/architecture/server/adapter-expectations.md`, made run-to-completion backend
  selection injectable, documented the intentional `neovex-testing ->
  neovex-storage` dependency, and split Firebase Listen WebSocket tests out of
  `tests/firebase/listen.rs`, reducing the root from `1741` lines to `1342`.
  `crates/neovex-bin/src/start/tests.rs` (`1676`) is explicitly justified as a
  CLI/start harness root for this wave, and `packages/firebase/src/selftest.mjs`
  (`3925`) is recorded as a strong ownership-based exception until a future JS
  harness wave owns that decomposition. Verified with
  `cargo check -p neovex-runtime -p neovex-server`,
  `cargo test -p neovex-server firebase_listen_websocket --lib`, and
  `cargo test -p neovex-server cloud_functions --lib`.
- `2026-04-27`: `ASC8` completed. The plan ledger is now truthful, repo docs
  and guidance are aligned with the landed seams, and the final focused proof
  set passed: `cargo fmt --all --check`,
  `cargo check -p neovex-storage -p neovex-engine -p neovex-runtime -p neovex-server`,
  `cargo test -p neovex-storage --lib`,
  `cargo test -p neovex-engine mutation_journal --lib`,
  `cargo test -p neovex-server firebase_listen_websocket --lib`,
  `cargo test -p neovex-server cloud_functions --lib`, and
  `cargo clippy -p neovex-server --lib --tests -- -D warnings`.
