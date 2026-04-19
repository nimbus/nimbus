# Targeted Domain Modularity Cleanup Control Plan

Archived on 2026-04-08 after `TD1` through `TD5` completed. This file is
preserved as a historical execution record and should not be resumed as a live
control plane. For current generic maintainability work, start at
`docs/plans/codebase-modularity-and-maintainability-plan.md`.

This is the canonical execution control plane for the next focused cleanup pass
after the archived queue-and-test-surface workstream.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `crates/neovex-runtime/src/runtime.rs`
- `crates/neovex-runtime/src/runtime/tests/cooperative.rs`
- `crates/neovex-runtime/src/runtime/tests/locker.rs`
- `crates/neovex-runtime/src/runtime/tests/warm_pool.rs`
- `crates/neovex-engine/src/tenant.rs`
- `crates/neovex-core/src/auth.rs`
- `packages/neovex/src/browser.ts`
- `packages/neovex/package.json`

Baseline verification status for this plan:

- the immediately preceding cleanup workstream was completed and archived as
  `docs/plans/archive/test-surface-and-queue-ownership-cleanup-plan.md`
- this control plane is being authored as a docs-only review-and-planning pass
  on 2026-04-08 after the prior cleanup handoff commit `da327b2`
- no new code verification is claimed by this planning pass
- every `TD*` implementation item must record its own focused verification
  before it can be marked `done`

---

## Purpose

The earlier cleanup passes removed the biggest production and test ownership
problems in the runtime queue, shared HTTP fixtures, and broad integration test
roots. The next cleanup pass should stay equally targeted: remove the one truly
unacceptable remaining god file, then split a small set of still-mixed domain
surfaces where the code is already conceptually separable.

This pass is intentionally not about chasing raw line counts. The target is a
small set of files where clear conceptual seams already exist and the current
shape still makes maintenance, feature work, and debugging harder than it needs
to be.

---

## Relationship To Other Plans

- Use `docs/plans/README.md` as the owning plan index.
- This plan is separate from:
  `docs/plans/v8-locker-fork-plan.md`,
  `docs/plans/archive/convex-demos-compatibility-plan.md`,
  `docs/plans/encryption-at-rest-plan.md`,
  `docs/plans/layered-admission-control-plan.md`,
  `docs/plans/pluggable-storage-backend-plan.md`,
  and `docs/plans/wasmtime-backend-plan.md`.
- If work turns into Locker-fork feature development, Convex compatibility
  feature work, encryption-at-rest implementation, layered admission control,
  storage backend abstraction, or Wasmtime backend work, stop and move to the
  owning plan instead of stretching this cleanup plan across multiple streams.

---

## Scope

This plan covers:

- extraction of the remaining inline runtime tests from
  `crates/neovex-runtime/src/runtime.rs`
- domain-facade extraction from `crates/neovex-engine/src/tenant.rs`
- conversion of `crates/neovex-core/src/auth.rs` into a directory module with
  clearer principal-versus-access ownership
- extraction of the HTTP client and browser utilities from
  `packages/neovex/src/browser.ts`
- follow-on doc, verification, and archive cleanup for this pass

This plan does not cover:

- new product features
- intentional route, wire, or API behavior changes unless explicitly recorded
- Locker-fork feature work or runtime scheduler redesign
- new test campaigns outside the moved surfaces
- compatibility code for pre-launch behavior

---

## Cleanup Invariants

These rules are mandatory for every item in this plan.

1. Preserve externally observable behavior by default.
   Runtime invocation, timeout, cancellation, auth, pooling, browser client
   requests, browser subscriptions, and access-policy behavior stay unchanged
   unless a specific item explicitly records otherwise.

2. Keep the core architecture invariants intact.
   `neovex-core` stays zero I/O.
   `neovex-runtime` stays zero workspace dependencies.
   All mutations still flow through `Service::apply_mutation` or its queued
   async journal path.
   Storage atomicity stays unchanged.

3. Prefer concept-owned module boundaries over helper piles.
   A successful split gives one concept a stable home instead of creating a new
   grab-bag module.

4. Keep composition roots thin once ownership moves out.
   `runtime.rs`, `tenant.rs`, `auth/mod.rs`, and `browser.ts` should remain as
   small facades or entrypoints once their owned concepts move out.

5. Preserve public surface and import ergonomics.
   Existing Rust public re-exports and JS public exports should keep working
   unless a cleanup-only rename is explicitly recorded.

6. Keep test coverage obvious.
   Moving runtime tests out of `runtime.rs` is good only if the new files still
   make it obvious where a scenario belongs.

7. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

---

## Current Assessed State

- The repo no longer has many top-level production god files, but one true
  outlier remains: `crates/neovex-runtime/src/runtime.rs` at 2718 lines.
- That runtime root is no longer large because of production complexity alone.
  Most of its size is still inline test ownership that already has a proven
  extraction pattern under `crates/neovex-runtime/src/runtime/tests/`.
- `crates/neovex-engine/src/tenant.rs` is no longer an architectural monolith,
  but its main `impl TenantRuntime` block is still a dense flat facade that can
  be grouped cleanly by already-existing subsystem ownership.
- `crates/neovex-core/src/auth.rs` still mixes principal identity, policy
  structures, predicate evaluation, read-planner compilation, and tests in one
  file even though principal context and access-policy evaluation are distinct
  concepts.
- `packages/neovex/src/browser.ts` still combines two largely separate browser
  client concerns: stateless HTTP request handling and stateful WebSocket/live
  query orchestration, plus a tail of browser utility helpers.

---

## Current Review Findings

1. `crates/neovex-runtime/src/runtime.rs` is the remaining must-fix god file.
   It is 2718 lines long, with a small production entrypoint and a very large
   inline `#[cfg(test)]` module. The repo already has the right extraction
   pattern in `runtime/tests/cooperative.rs`, `runtime/tests/locker.rs`, and
   `runtime/tests/warm_pool.rs`; the remaining inline tests should follow it.

2. `crates/neovex-engine/src/tenant.rs` is a good domain-facade extraction
   target.
   The file is only 523 lines, but it contains a 62-method flat `impl
   TenantRuntime` block. Most of those methods are already pure delegation into
   `document_cache`, `materialized_reads`, `mutation`, `subscription_delivery`,
   or `query_planning` subsystems, so grouped facade files are a natural next
   step.

3. `crates/neovex-core/src/auth.rs` still mixes two different domains.
   `PrincipalContext` and principal snapshotting are conceptually independent
   from access-policy evaluation and read-planner compilation, but they share
   one file today. A directory module split can separate principal identity from
   access control without changing public re-exports.

4. `packages/neovex/src/browser.ts` still holds two clients and several helpers
   in one file.
   `NeovexHttpClient` is a stateless request layer. `NeovexClient` is a
   stateful WebSocket/live-query orchestrator. The bottom-of-file browser
   helpers are independent utility concerns. That separation is already visible
   in the code and should become explicit in the module layout.

5. Several other large files were reviewed and are not the right next targets
   for this pass.
   `crates/neovex-runtime/src/executor.rs` remains large, but it is tied more
   directly to ongoing runtime scheduler and Locker work.
   `crates/neovex-runtime/src/metrics.rs` already delegates to submodules.
   `crates/neovex-engine/src/service/queries/planner/mod.rs` is already a
   composition root over planner submodules.
   `crates/neovex-engine/src/evaluator/tests.rs` is large but cohesive.
   `crates/neovex-engine/src/service/scheduler/tests.rs` is large but stable and
   not the highest-value maintenance seam right now.

---

## Success Criteria

This plan is successful only when all of the following are true:

- `runtime.rs` is no longer a test-owned god file
- `tenant.rs` reads as a thin facade over grouped domain facades
- `auth.rs` becomes a clearer principal-plus-access module tree
- `browser.ts` reads as a browser client entrypoint instead of a full
  implementation pile
- naming, visibility, and file placement are more canonical and consistent
- no unintentionally observable behavior changes are introduced
- the plan can be archived cleanly once the workstream completes

---

## Assessed But Not Selected

- `crates/neovex-runtime/src/executor.rs`
  still large, but overlaps more directly with runtime scheduler and Locker
  work than this cleanup pass should
- `crates/neovex-runtime/src/metrics.rs`
  already decomposed into concept-owned submodules and no longer the highest
  value target
- `crates/neovex-engine/src/service/queries/planner/mod.rs`
  already functions as a composition root over planner submodules
- `crates/neovex-engine/src/evaluator/tests.rs`
  large but cohesive as a single-subject evaluator regression surface
- `crates/neovex-engine/src/service/scheduler/tests.rs`
  borderline large but stable, already concept-owned, and not the best
  maintainability return for this pass

---

## Feature Preservation Matrix

- Runtime invocation, bundle integrity, timeout, cancellation, heap-limit,
  nested-call, and host-bridge semantics must remain unchanged.
- Runtime warm-pool, retained-runtime reuse, and cooperative execution behavior
  must remain unchanged.
- Tenant lifecycle, mutation-admission, mutation-journal, materialized read,
  query planning, and subscription-delivery semantics must remain unchanged.
- Access-policy validation, evaluation, read-filter compilation, and policy
  revision fingerprint semantics must remain unchanged.
- Browser HTTP request, auth refresh, WebSocket subscription, reconnect, and
  public export semantics must remain unchanged.
- Existing runtime, engine, core, and JS selftest coverage must remain intact
  even when tests or utilities move into new files.

---

## Control Plane Rules

1. This document is the durable control plane for this cleanup workstream.
2. Update this plan before or during every meaningful implementation burst.
3. Keep exactly one `TD*` item `in_progress` at a time.
4. Do not skip forward while an earlier eligible item is still `todo`.
5. If an item spans multiple sessions, leave it `in_progress` and update its
   checkpoint instead of starting the next item.
6. Record verification in `Execution Log` before marking an item `done`.
7. If a blocker appears, record it in the ledger and execution log before
   stopping.
8. Treat the roadmap plus the git worktree as the source of execution state.

---

## Verification Contract

Every Rust implementation item in this plan must:

1. run its focused verification before it is marked `done`
2. run `cargo fmt --all --check`
3. run `cargo check --workspace`
4. run the appropriate focused crate tests and clippy checks for the changed
   surface
5. record any environment limitation explicitly in `Execution Log`

The browser-client item in this plan must:

1. run `npm run test --workspace neovex`
2. run any package or workspace build/typecheck entrypoint that becomes
   available or relevant as part of the split
3. record any environment limitation explicitly in `Execution Log`

Before archiving this plan, also run:

- `make check`
- `make test`
- `make clippy`
- `npm run test --workspace neovex`
- `npm run build --workspaces --if-present`
- `make ci` if practical

If `make ci` cannot complete because `cargo deny` or advisory-db locking is not
available in the environment, record that limitation explicitly rather than
silently skipping it.

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| TD0 | `done` | reviewed the current post-queue-and-test-surface architecture and identified the next high-value targeted cleanup seams in `runtime.rs`, `tenant.rs`, `auth.rs`, and `browser.ts` | none | docs-only review and planning pass on 2026-04-08 |
| TD1 | `done` | extracted the remaining inline runtime tests from `crates/neovex-runtime/src/runtime.rs` into concept-owned `runtime/tests/*.rs` modules and reduced `runtime.rs` to the shared harness plus module declarations | none | completed on 2026-04-08 with focused runtime verification |
| TD2 | `done` | split `crates/neovex-engine/src/tenant.rs` into grouped domain-facade modules and reduced the root to tenant structure, constructors, lifecycle, and cross-domain diagnostics | TD1 recommended first | completed on 2026-04-08 with focused engine verification |
| TD3 | `done` | converted `crates/neovex-core/src/auth.rs` into an `auth/` directory module with principal identity in `mod.rs`, access-policy ownership in `access.rs`, and extracted tests in `tests.rs` | TD1 and TD2 recommended first | completed on 2026-04-08 with focused core verification |
| TD4 | `done` | extracted the browser HTTP client into `packages/neovex/src/http-client.ts` and shared browser helpers into `packages/neovex/src/browser-utils.ts` while keeping `browser.ts` as the browser-client entrypoint and public re-export surface | TD1 through TD3 recommended first | completed on 2026-04-08 with JS selftest and workspace build verification |
| TD5 | `done` | updated the docs, completed the full verification sweep, and archived the completed plan cleanly | TD1 through TD4 | completed on 2026-04-08; `make ci` hit an environment-only `cargo deny` advisory-db lock limitation on a read-only path |

---

## Dependency Graph

- `TD1` is the recommended first slice because it is the only must-fix god
  file and already has a proven extraction pattern.
- `TD2` should usually follow `TD1` because it is low-risk mechanical facade
  work over already-separated tenant subsystems.
- `TD3` should usually follow `TD2` because the auth split is still
  straightforward but involves a filesystem module reorganization.
- `TD4` is largely independent of the Rust items, but it is easiest to do after
  the Rust cleanup slices have established momentum and confidence.
- `TD5` closes the workstream after all targeted refactors land.

---

## Recommended Delivery Order

1. `TD1` — runtime test extraction
2. `TD2` — tenant domain-facade split
3. `TD3` — auth principal/access split
4. `TD4` — browser HTTP client and utility extraction
5. `TD5` — docs, verification, and archive

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| TD0 | done | start `TD1` by mapping the remaining inline runtime tests into six concept-owned files that match the existing `runtime/tests/` pattern |
| TD1 | done | start `TD2` by mapping the existing `TenantRuntime` delegation methods into grouped facade files under `crates/neovex-engine/src/tenant/` |
| TD2 | done | start `TD3` by separating the principal-identity surface from the access-policy and read-filter compilation surface in `crates/neovex-core/src/auth.rs` |
| TD3 | done | start `TD4` by separating the stateless request layer and shared browser helpers from the stateful `NeovexClient` entrypoint |
| TD4 | done | start `TD5` by reconciling the docs, rerunning the repo-wide verification sweep, and archiving the completed plan |
| TD5 | done | workstream closed; keep this record archived and start any future cleanup pass from a newly promoted active plan instead of reviving this one |

---

## Work Items

### TD0. Baseline review and hotspot map

#### Outcome

- Completed during this planning pass.

### TD1. Extract the remaining inline `runtime.rs` tests by concept ownership

#### Implementation plan

1. Keep `crates/neovex-runtime/src/runtime.rs` as the production entrypoint plus
   the shared `#[cfg(test)]` helpers and mock hosts.
2. Extract the remaining inline tests into six concept-owned files under
   `crates/neovex-runtime/src/runtime/tests/`:
   `basic_invocation.rs`,
   `timeout_cancellation.rs`,
   `pool_reuse.rs`,
   `snapshot_lifecycle.rs`,
   `host_bridge.rs`,
   and `bundle_integrity.rs`.
3. Follow the existing `runtime/tests/cooperative.rs`,
   `runtime/tests/locker.rs`, and `runtime/tests/warm_pool.rs` pattern instead
   of inventing a new test structure.

#### Focused verification

- `cargo test -p neovex-runtime --lib`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-runtime --all-targets -- -D warnings`

#### Acceptance criteria

- `crates/neovex-runtime/src/runtime.rs` is no longer a 2700-line test-owned
  god file
- runtime tests live in obvious concept-owned files under `runtime/tests/`
- runtime behavior is unchanged

### TD2. Split `tenant.rs` into grouped domain facades

#### Implementation plan

1. Keep `TenantRuntime`, guard structs, constructors, lifecycle, and
   cross-domain diagnostics in `crates/neovex-engine/src/tenant.rs`.
2. Extract grouped facade files under `crates/neovex-engine/src/tenant/` for:
   document cache,
   materialized reads,
   mutation,
   subscription delivery,
   and query-planning metrics.
3. Keep the split mechanical: move grouped delegations without changing
   subsystem behavior.

#### Focused verification

- `cargo test -p neovex-engine`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-engine --all-targets -- -D warnings`

#### Acceptance criteria

- `tenant.rs` reads as a thin facade and composition root
- grouped tenant responsibilities live in clearer facade files
- tenant runtime behavior is unchanged

### TD3. Convert `auth.rs` into a principal-plus-access module tree

#### Implementation plan

1. Convert `crates/neovex-core/src/auth.rs` into `crates/neovex-core/src/auth/`
   with `mod.rs` as the public entrypoint.
2. Keep principal identity, principal snapshotting, and policy revision helpers
   in `auth/mod.rs`.
3. Move access-policy types, predicate evaluation, and read-planner compilation
   into `auth/access.rs`.
4. Move tests into `auth/tests.rs`.
5. Preserve public re-exports so downstream imports remain stable.

#### Focused verification

- `cargo test -p neovex-core`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-core --all-targets -- -D warnings`

#### Acceptance criteria

- principal identity and access-policy logic no longer share one flat file
- public auth surface stays stable
- policy behavior and read-filter compilation semantics are unchanged

### TD4. Extract the browser HTTP client and utility helpers from `browser.ts`

#### Implementation plan

1. Move `NeovexHttpClient` and its related types into
   `packages/neovex/src/http-client.ts`.
2. Move the browser utility helpers into
   `packages/neovex/src/browser-utils.ts`.
3. Keep `packages/neovex/src/browser.ts` as the browser-client entrypoint that
   owns `NeovexClient`, `NeovexReactClient`, and the public re-export surface.
4. Preserve exports so `packages/convex` and other consumers continue to import
   from `neovex/browser`.

#### Focused verification

- `npm run test --workspace neovex`
- `npm run build --workspaces --if-present`

#### Acceptance criteria

- `browser.ts` reads as a browser entrypoint instead of a mixed implementation
  file
- HTTP request handling and browser utilities have clear homes
- browser client behavior and public exports are unchanged

### TD5. Docs and full verification closure

#### Implementation plan

1. Update `ARCHITECTURE.md` if the landed ownership map changes.
2. Update `docs/plans/README.md`, `AGENTS.md`, and any other entrypoint docs if
   plan ownership changes during the workstream.
3. Remove stale checkpoint text and ensure the ledger, dependency graph, and
   execution log match reality.
4. Archive the completed plan once all non-deferred work is done.

#### Focused verification

- `make check`
- `make test`
- `make clippy`
- `npm run test --workspace neovex`
- `npm run build --workspaces --if-present`
- `make ci` if practical

#### Acceptance criteria

- the docs reflect the landed ownership map
- the plan can be archived cleanly with no ledger/worktree mismatch

---

## Execution Log

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-08 | TD0 | done | Reviewed the live post-queue-and-test-surface architecture and validated the next targeted cleanup seams in `runtime.rs`, `tenant.rs`, `auth.rs`, and `packages/neovex/src/browser.ts`. Confirmed that these are real domain splits rather than arbitrary line-count targets, and authored this new active cleanup control plane to own the next pass. | docs-only review and planning pass; no new code verification claimed in this handoff | start `TD1` by mapping the remaining inline runtime tests into six concept-owned files under `crates/neovex-runtime/src/runtime/tests/` |
| 2026-04-08 | TD1 | done | Extracted the remaining inline runtime tests into six concept-owned files under `crates/neovex-runtime/src/runtime/tests/` and reduced `runtime.rs` from 2718 lines to a 502-line shared-harness root. | `cargo test -p neovex-runtime --lib`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p neovex-runtime --all-targets -- -D warnings` | start `TD2` by grouping the `TenantRuntime` delegation methods into domain facades under `crates/neovex-engine/src/tenant/` |
| 2026-04-08 | TD2 | done | Extracted grouped `TenantRuntime` delegation methods into five tenant facade files and reduced `tenant.rs` to the tenant structure, constructors, lifecycle, and cross-domain diagnostics root. Updated `ARCHITECTURE.md` to reflect the new tenant facade layer. | `cargo test -p neovex-engine`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p neovex-engine --all-targets -- -D warnings` | start `TD3` by converting `crates/neovex-core/src/auth.rs` into an `auth/` module tree with stable public re-exports |
| 2026-04-08 | TD3 | done | Converted the flat auth file into `auth/mod.rs`, `auth/access.rs`, and `auth/tests.rs`, keeping principal identity and policy-revision helpers at the module root while moving access-policy evaluation behind `access.rs`. Updated `ARCHITECTURE.md` to reflect the new core auth ownership map. | `cargo test -p neovex-core`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p neovex-core --all-targets -- -D warnings` | start `TD4` by extracting `NeovexHttpClient` and browser helpers from `packages/neovex/src/browser.ts` |
| 2026-04-08 | TD4 | done | Extracted the stateless HTTP request layer into `packages/neovex/src/http-client.ts` and the shared socket/auth/subscribe helpers into `packages/neovex/src/browser-utils.ts`, leaving `browser.ts` as the stateful browser-client entrypoint and public re-export surface. | `npm run test --workspace neovex`; `npm run build --workspaces --if-present` | start `TD5` by running the repo-wide verification sweep and archiving the completed plan cleanly |
| 2026-04-08 | TD5 | done | Completed the full closure sweep, verified the Rust and JS workspaces, updated the plan index and `AGENTS.md`, and archived this control plane as a completed historical record. | `make check`; `make test`; `make clippy`; `npm run test --workspace neovex`; `npm run build --workspaces --if-present`; `make ci` failed for an environment-only reason because `cargo deny` could not lock `/Users/jack/.cargo/advisory-dbs/db.lock` on a read-only path | no further action in this plan; promote a new active plan for any future cleanup pass |
