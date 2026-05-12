# Codebase Architecture And Maintainability Control Plan

Status: archived completed on 2026-04-21 after `ACM1` through `ACM7` landed
and the final closeout verification sweep passed.

This was the canonical execution control plane for the next repo-wide
architecture, modularity, readability, canonical naming, idiomatic-Rust, and
maintainability workstream after the completed waves archived under
`docs/plans/archive/`.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `AGENTS.md`
- `docs/reference/reliability-posture.md`
- `docs/reference/ci-failure-investigation.md`
- `docs/plans/archive/architecture-modularity-and-maintainability-plan.md`
- `docs/plans/archive/codebase-reliability-and-maintainability-hardening-plan.md`
- `docs/plans/archive/codebase-modularity-and-maintainability-hotspots-plan.md`
- `docs/plans/archive/codebase-modularity-and-maintainability-follow-on-plan.md`
- `docs/plans/archive/codebase-modularity-and-maintainability-plan.md`
- `crates/nimbus-engine/src/service/mod.rs`
- `crates/nimbus-engine/src/service/queries/query_api.rs`
- `crates/nimbus-engine/src/service/mutations/direct/api.rs`
- `crates/nimbus-engine/src/service/mutations/direct/execution.rs`
- `crates/nimbus-engine/src/service/mutations/journal.rs`
- `crates/nimbus-engine/src/service/subscriptions/bootstrap.rs`
- `crates/nimbus-engine/src/service/tenants.rs`
- `crates/nimbus-engine/src/persistence/provider.rs`
- `crates/nimbus-engine/src/persistence/executor.rs`
- `crates/nimbus-engine/src/persistence/query.rs`
- `crates/nimbus-engine/src/persistence/tenant.rs`
- `crates/nimbus-engine/src/persistence/tenant/writes.rs`
- `crates/nimbus-runtime/src/host.rs`
- `crates/nimbus-server/src/lib.rs`
- `crates/nimbus-server/src/router.rs`
- `crates/nimbus-server/src/state.rs`
- `crates/nimbus-server/src/adapters/convex/mod.rs`
- `crates/nimbus-server/src/adapters/convex/host_bridge/async_bridge/dispatch.rs`
- `crates/nimbus-server/src/execution/invocations/mod.rs`
- `crates/nimbus-bin/src/service/mod.rs`
- `crates/nimbus-bin/src/service/execution.rs`
- `crates/nimbus/src/lib.rs`
- `packages/nimbus/src/server.ts`
- `packages/convex/src/server.ts`
- the current git worktree on 2026-04-21

Baseline verification status for this plan:

- this control plane is being authored as a docs-only review and planning pass
  on 2026-04-21
- the worktree was clean when this plan was promoted
- no new code verification is claimed by this planning pass
- every `ACM*` implementation item must record its own focused verification
  before it can be marked `done`

---

## Purpose

The current architecture is real and worth preserving:

- `nimbus-runtime` still owns a narrow workspace-independent runtime contract
- `nimbus-server` still owns transport and compatibility integration
- `nimbus-engine` still owns the central coordination layer
- `nimbus-storage` still owns persistence semantics and backend behavior
- `TenantRuntime` still groups tenant-local operational state coherently

The better reading of the codebase today is not "the crate split failed."
Instead, the repo is paying coordination cost at the seams:

- sync, async, and cancellable service paths often repeat the same flow with
  slightly different execution mechanics
- server startup and router construction still expose an overload matrix rather
  than one clearly typed internal build pipeline
- provider and persistence facades still require wide enum-switch edits across
  multiple files
- Convex host-call dispatch still carries a triple-entry update burden across
  sync, cancellable, and async routing
- the `nimbus service ...` surface still spreads command declaration,
  orchestration, and presentation concerns across a broad subsystem
- `ARCHITECTURE.md` remains above the 1,500-line review threshold and needs an
  active owner rather than an implicit exception

This plan is therefore about simplification, not churn. The goal is not to
split lines to split lines. The goal is to group like concepts together, thin
roots that have started to regrow orchestration burden, and make the code
easier to extend, debug, and review without weakening the architectural center.

This is not a feature roadmap. It is a code-organization, seam-hardening, and
maintainability roadmap.

---

## Relationship To Other Plans

- Use `docs/plans/README.md` as the owning plan index.
- Use this archived plan only when a task needs the latest completed repo-wide
  architecture and maintainability wave's execution detail, closeout
  verification bundle, or governance baseline.
- Use `docs/reference/reliability-posture.md` and
  `docs/reference/ci-failure-investigation.md` together with this plan when a
  task needs proof-hygiene and CI-hardening guidance.
- Use `docs/plans/archive/architecture-modularity-and-maintainability-plan.md`
  only for the latest completed repo-wide maintainability wave's execution
  detail, closeout verification bundle, or governance baseline that this plan
  builds on.
- Use
  `docs/plans/archive/codebase-reliability-and-maintainability-hardening-plan.md`
  only for the completed reliability wave's execution detail and proof
  packaging baseline.
- Use
  `docs/plans/archive/codebase-modularity-and-maintainability-hotspots-plan.md`
  only for the latest completed hotspot wave's execution record and explicit
  size-justification history.
- Use
  `docs/plans/archive/codebase-modularity-and-maintainability-follow-on-plan.md`
  only for the completed follow-on maintainability wave's benchmark and proof
  packaging history.
- Use
  `docs/plans/archive/codebase-modularity-and-maintainability-plan.md`
  only for predecessor-wave checkpoint history.
- This plan is separate from the active feature and platform plans such as
  `encryption-at-rest-plan.md`, `websocket-protocol-plan.md`,
  `localhost-server-security-plan.md`, `system-tenant-api-plan.md`,
  `desktop-ui-plan.md`, `install-script-plan.md`, `distribution-plan.md`,
  `windows-machine-support-plan.md`, `wasmtime-backend-plan.md`,
  `wasi-agent-capabilities-plan.md`, `nimbus-rename-plan.md`, and
  `nimbus-rename-satellite-repos-plan.md`.
- If work turns into feature behavior change, protocol change, storage semantic
  change, platform behavior change, release/distribution work, or rename work,
  stop and move to the owning plan instead of stretching this control plane.

---

## Scope

This plan covers:

- convergence of duplicated sync, async, and cancellable service flows toward
  smaller internal helpers and clearer ownership
- cleanup of engine persistence and provider capability seams so new backend or
  capability changes require fewer wide enum-switch edits
- simplification of server startup, router construction, and runtime invocation
  setup behind clearer typed internal builders
- cleanup of Convex host-call dispatch and runtime integration so operation
  families have one canonical home
- cleanup of the `nimbus service ...` subsystem so command declaration,
  orchestration, platform dispatch, and rendering are easier to navigate
- architecture-document packaging, public-facade curation, and governance
  updates needed to keep repo-wide cleanup resumable through handoff and
  compaction

This plan does not cover:

- new product features
- speculative rewrites that are not justified by ownership, readability, or
  maintainability
- intentional protocol or API redesign unless a specific item explicitly
  records and justifies it
- install or distribution work
- rename work
- compatibility layers for pre-launch behavior

---

## Cleanup Invariants

These rules are mandatory for every item in this plan.

1. Preserve behavior by default.
   Native HTTP and WebSocket behavior, runtime host-call behavior, storage
   atomicity, scheduler semantics, and CLI behavior stay unchanged unless a
   specific item explicitly records otherwise.

2. Keep the core architecture invariants intact.
   `nimbus-core` stays zero I/O.
   `nimbus-runtime` stays zero workspace dependencies.
   Every mutation still flows through the engine-owned mutation path.
   Storage atomicity stays unchanged.

3. Split by concept ownership, not by line count alone.
   Do not break files or lines apart just to make them shorter. Move code only
   when it creates a clearer ownership boundary, a better extension point, or a
   more local debugging surface.

4. Treat file size as a signal, not the goal.
   Files under 1,500 lines are usually acceptable if they keep one coherent
   ownership story.
   Files from 1,500 through 1,999 lines need an explicit justification in the
   owning active plan if they remain unsplit.
   Files at 2,000 lines or above must be decomposed or documented as a strong
   ownership-based exception.

5. Keep composition roots thin once ownership moves out.
   If a root already delegates through a concept-owned module tree, do not turn
   it back into a renamed switchboard.

6. Prefer canonical, local names over helper buckets.
   Favor concept-owned names such as `bootstrap.rs`, `provider.rs`, `read.rs`,
   `write.rs`, `state.rs`, `dispatch.rs`, or `builder.rs`. Avoid adding new
   `helpers.rs`, `common.rs`, `misc.rs`, or `utils.rs` files unless ownership
   is truly shared and obvious.

7. Favor a single canonical internal pattern per behavior family.
   Repeated cancellation, admission, routing, or wrapper logic should converge
   on one local pattern instead of drifting across adjacent modules.

8. Prefer explicit options and builders over overload matrices when the
   configuration space is already typed.

9. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

---

## Current Assessed State

- Before this planning pass, the repo had no active generic architecture and
  maintainability control plane even though the archived waves had established a
  strong baseline.
- The architectural center remains sound:
  - `Service` is still the engine coordination center
  - `TenantRuntime` still groups tenant-local operational state coherently
  - `HostBridge` still keeps the runtime contract narrow and workspace-free
  - the server still owns transport and compatibility integration
  - the storage layer still owns durable semantics and backend behavior
- The highest-cost maintainability issues are not giant central roots. They are
  repeated seam patterns and coordination width across adjacent modules.
- Representative current hotspot counts from the live tree:
  - `ARCHITECTURE.md` at 1,745 lines
  - `crates/nimbus-engine/src/persistence_config.rs` at 810 lines
  - `crates/nimbus-bin/src/service/mod.rs` at 770 lines
  - `crates/nimbus-bin/src/service/execution.rs` at 616 lines
  - `crates/nimbus-engine/src/service/queries/query_api.rs` at 478 lines
  - `crates/nimbus-server/src/adapters/convex/host_bridge/async_bridge/dispatch.rs`
    at 296 lines
  - `packages/nimbus/src/server.ts` at 548 lines
  - `packages/convex/src/server.ts` at 391 lines
- The largest active Rust production files above 1,000 lines are still mostly
  concept-cohesive backend or sandbox modules rather than the central roots:
  - `crates/nimbus-storage/src/mysql/backend.rs` at 1,236 lines
  - `crates/nimbus-sandbox/src/backends/oci/builder.rs` at 1,199 lines
  - `crates/nimbus-storage/src/postgres/write.rs` at 1,188 lines
  - `crates/nimbus-sandbox/src/backends/oci/network.rs` at 1,150 lines
  - `crates/nimbus-storage/src/postgres/backend.rs` at 1,122 lines
- The persistence provider seam is explicit, but still edit-wide:
  `PersistenceProvider`, `TenantPersistenceExecutor`, and
  `TenantPersistence` all require repeated variant dispatch across multiple
  files.
- The engine service surface is feature-rich but still repeats flow structure
  across blocking, async, and cancellable entrypoints in queries, mutations,
  scheduler access, tenant lifecycle, and subscription bootstrap.
- The server construction surface is bounded better than earlier waves, but it
  still exposes many `with_*` combinations around router and serve paths.
- The Convex compatibility boundary is healthier than it used to be, but the
  host-call dispatch path still requires the same operation family to be wired
  through sync, cancellable, and async cases.
- `packages/convex` is already a thin compatibility wrapper in important places
  and is no longer the primary structural concern for this wave.

---

## Current Review Findings

1. The crate split is healthy enough to preserve.
   The next wave should simplify seam behavior and ownership around the center,
   not reopen the center itself.

2. The main maintainability tax is duplicated flow shape.
   Query, mutation, scheduler, tenant, and subscription entrypoints frequently
   repeat the same coordination pattern with different execution mechanics.

3. Persistence abstraction is explicit but still expensive to extend.
   Adding or changing one capability often means touching provider enum
   wrappers, executor wrappers, and persistence wrappers in parallel.

4. Server and Convex integration currently lean on overload and dispatch
   matrices more than on builder- or operation-owned shapes.

5. The `nimbus service ...` subsystem is usable, but its declaration,
   resolution, orchestration, and presentation responsibilities still span a
   broad surface that is harder to read than it needs to be.

6. `ARCHITECTURE.md` needs an explicit active owner.
   At 1,745 lines it is above the review threshold, so it must either get
   thinner for a reason or remain justified in this plan's threshold ledger.

7. The JS compatibility story is in a better place than the Rust seam story.
   That makes JS wrapper work a lower-priority cleanup target unless it is
   directly touched by a higher-priority item.

---

## Top 6 Architectural Moves

### ACM1. Normalize persistence-provider capabilities and delegation surfaces

Primary targets:

- `crates/nimbus-engine/src/persistence/provider.rs`
- `crates/nimbus-engine/src/persistence/executor.rs`
- `crates/nimbus-engine/src/persistence/query.rs`
- `crates/nimbus-engine/src/persistence/tenant.rs`
- `crates/nimbus-engine/src/persistence/tenant/writes.rs`
- adjacent provider-local capability seams as needed

Desired outcome:

- provider and persistence capability ownership is easier to name
- repeated enum-switch delegation shrinks or centralizes
- adding or changing one backend capability becomes a smaller edit

### ACM2. Converge duplicated engine service flows behind smaller internal helpers

Primary targets:

- `crates/nimbus-engine/src/service/queries/query_api.rs`
- `crates/nimbus-engine/src/service/mutations/direct/api.rs`
- `crates/nimbus-engine/src/service/mutations/direct/execution.rs`
- `crates/nimbus-engine/src/service/mutations/journal.rs`
- `crates/nimbus-engine/src/service/subscriptions/bootstrap.rs`
- `crates/nimbus-engine/src/service/scheduler/access.rs`
- `crates/nimbus-engine/src/service/tenants.rs`

Desired outcome:

- public surfaces can stay ergonomic, but the internal flow shape becomes more
  canonical
- cancellation and operation-guard patterns read the same way across features
- future feature work has one clearer place to slot new sync/async variants

### ACM3. Simplify server construction and routing into one typed internal pipeline

Primary targets:

- `crates/nimbus-server/src/lib.rs`
- `crates/nimbus-server/src/router.rs`
- `crates/nimbus-server/src/state.rs`

Desired outcome:

- one typed internal build pipeline owns route and serve construction
- public wrappers stay thin and intention-revealing
- new routing features do not require another combinatorial overload family

### ACM4. Bound Convex host-call dispatch and runtime invocation wiring

Primary targets:

- `crates/nimbus-server/src/adapters/convex/host_bridge/async_bridge/dispatch.rs`
- `crates/nimbus-server/src/adapters/convex/mod.rs`
- `crates/nimbus-server/src/execution/invocations/mod.rs`
- adjacent `adapters/convex/host_bridge/` operation families
- `crates/nimbus-runtime/src/host.rs` only as needed to preserve a narrow,
  stable contract

Desired outcome:

- host-call operations have one canonical home per concept family
- sync, cancellable, and async wiring shares more structure
- adding a host operation becomes less error-prone and easier to review

### ACM5. Thin the service orchestration subsystem around clearer command ownership

Primary targets:

- `crates/nimbus-bin/src/service/mod.rs`
- `crates/nimbus-bin/src/service/execution.rs`
- `crates/nimbus-bin/src/service/lifecycle.rs`
- `crates/nimbus-bin/src/service/project.rs`
- `crates/nimbus-bin/src/service/render.rs`

Desired outcome:

- command declaration, orchestration, platform dispatch, and rendering each
  have a clearer home
- the `service` subsystem is easier to debug without tracing broad cross-file
  helper flow
- future service and machine-management work has more obvious placement

### ACM6. Repackage architecture docs, public facades, and governance surfaces

Primary targets:

- `ARCHITECTURE.md`
- `crates/nimbus/src/lib.rs`
- `AGENTS.md`
- `docs/plans/README.md`
- `packages/nimbus/src/server.ts` and `packages/convex/src/server.ts` only if
  touched by related cleanup

Desired outcome:

- `ARCHITECTURE.md` stays canonical but no longer carries avoidable reference
  depth
- public facade surfaces are more clearly curated
- repo entrypoints point future contributors at one live maintainability owner

---

## Success Criteria

This plan is successful only when all of the following are true:

- the central engine/runtime/server/storage split remains intact
- duplicated sync, async, and cancellable coordination logic is materially
  reduced or normalized
- persistence/provider capability changes require fewer wide switchboard edits
- server construction and Convex host-call wiring are easier to extend without
  overload or dispatch sprawl
- the `nimbus service ...` subsystem has clearer concept ownership
- `ARCHITECTURE.md` is either thinner for a reason or explicitly justified in
  the threshold ledger
- active files between 1,500 and 1,999 lines, if any remain, have explicit
  justifications in this plan
- no active code file remains at or above 2,000 lines without a recorded
  ownership-based exception and a concrete decomposition decision
- repo entrypoints point to this active control plane rather than defaulting to
  archived-plan archaeology

---

## Feature Preservation Matrix

| Surface | Preservation Requirement |
| --- | --- |
| Engine mutation and query semantics | service coordination, mutation ordering, tenant lifecycle, and read visibility stay unchanged while internal helpers are simplified |
| Storage semantics | document writes, index updates, journal behavior, scheduler behavior, and durable commit rules stay unchanged while provider and persistence facades are thinned |
| Runtime inversion seam | `HostBridge` stays the runtime contract and `nimbus-runtime` keeps zero workspace dependencies |
| Native server routes | HTTP and WebSocket behavior stays unchanged unless a specific item explicitly records a behavior change |
| Convex compatibility behavior | runtime-backed query, mutation, action, scheduler, and subscription behavior stays unchanged while internal dispatch and construction surfaces are simplified |
| CLI behavior | `nimbus service ...` behavior stays unchanged unless a specific item explicitly records and justifies a pre-launch cleanup |
| Public facade surfaces | facade exports may become more intentional, but any breaking cleanup must be explicit and justified rather than accidental |

---

## Control Plan Rules

1. Implement exactly one `ACM*` item at a time unless the plan explicitly says
   otherwise.
2. Do not start a later item while an earlier eligible item is still
   `in_progress`.
3. If the worktree is dirty, reconcile the changes to the owning item before
   taking new scope.
4. Do not split files or lines mechanically; every structural move must improve
   concept ownership, extension clarity, or debug locality.
5. Do not mark an item `done` without recording focused verification.
6. If implementation reveals a better seam map than this plan currently
   describes, update the plan first, then implement the new shape.
7. Record any active-file threshold exception at the same time as the code or
   docs that require it.
8. Update the roadmap ledger, checkpoints, and execution log in the same change
   set as the code or docs.

---

## Verification Contract

Every implementation item in this plan must run:

- focused verification for the touched subsystem
- `cargo fmt --all --check`
- `cargo check --workspace`

Use these focused lanes as appropriate:

- for persistence/provider cleanup:
  `cargo test -p nimbus-engine`
  `cargo test -p nimbus-storage`
- for engine service-flow cleanup:
  `cargo test -p nimbus-engine`
- for server-construction or Convex host-call cleanup:
  `cargo test -p nimbus-server`
  `cargo test -p nimbus-runtime`
- for service CLI cleanup:
  `cargo test -p nimbus-bin`
- for JS or facade cleanup:
  `npm run test --workspaces --if-present`
  `npm run build --workspaces --if-present`

Before closing the workstream, run and record:

- `make check`
- `make test`
- `make clippy`
- `npm run test --workspaces --if-present`
- `npm run build --workspaces --if-present`
- `make ci` if practical

If environment restrictions block a command, do not silently skip it:

- run the best focused alternative
- retry with escalation when appropriate
- record the limitation in `Execution Log`

---

## Threshold Exception Ledger

| File | Line Count | Justification | Next Review Trigger |
| --- | --- | --- | --- |
| none active | — | `ACM6` reduced `ARCHITECTURE.md` from 1,745 lines to 1,384 by moving persistence-engine depth into `docs/reference/persistence-engine-baseline.md`, so the active threshold-exception ledger is currently clear | add an entry when any active file crosses 1,500 lines or needs an explicit ownership-based exception |

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| ACM0 | `done` | completed the live architectural review, selected the next six high-signal cleanup moves, promoted this document as the active generic maintainability control plane, and updated repo entrypoints to point to it | none |
| ACM1 | `done` | centralized persistence-provider and tenant-persistence delegation through persistence-owned match macros, provider opened-tenant adapters, and store-owned provider/executor pairing so backend capability edits no longer require the same repeated switch boilerplate across provider, executor, query, snapshot, and tenant capability files | ACM0 |
| ACM2 | `done` | normalized engine service-flow setup through shared tenant-operation helpers, direct-mutation wrapper helpers, query runtime loaders, and shared subscription-bootstrap snapshot evaluation so sync/async/cancellable entrypoints now reuse smaller internal flow seams instead of hand-rolling the same setup repeatedly | ACM0, ACM1 |
| ACM3 | `done` | simplify server construction and routing into one typed internal pipeline | ACM0, ACM2 |
| ACM4 | `done` | bound Convex host-call dispatch and runtime invocation wiring behind smaller operation-owned seams | ACM0, ACM3 |
| ACM5 | `done` | thin the service orchestration subsystem around clearer command ownership | ACM0, ACM3 |
| ACM6 | `done` | repackage architecture docs, public facades, and governance surfaces | ACM0, ACM1, ACM2, ACM3, ACM4, ACM5 |
| ACM7 | `done` | ran the final verification sweep, reconciled repo entrypoints with the landed state, and archived this plan after the closeout verification bundle passed, including an escalated `make ci` rerun after a sandbox-only `cargo deny` advisory-db lock failure | ACM1, ACM2, ACM3, ACM4, ACM5, ACM6 |

---

## Dependency Graph

- `ACM0` is the current review and plan-promotion pass.
- `ACM1` should come first because persistence capability cleanup reduces
  repeated switchboard logic that later service items otherwise have to build
  around.
- `ACM2` should follow `ACM1` so engine flow convergence can rely on the
  cleaned persistence seam.
- `ACM3` should follow `ACM2` so server construction cleanup happens against a
  more canonical engine surface.
- `ACM4` should follow `ACM3` because Convex runtime integration should target
  the simplified server build and invocation pipeline rather than an interim
  shape.
- `ACM5` can follow `ACM3` in parallel only if its write scope stays confined
  to the `service` subsystem; otherwise keep it sequential after `ACM4`.
- `ACM6` is the public-facade and documentation sweep that codifies the final
  ownership and threshold decisions after the structural moves land.
- `ACM7` is the closeout verification and archive pass.

---

## Recommended Delivery Order

1. `ACM1` — persistence/provider capability cleanup
2. `ACM2` — engine service-flow convergence
3. `ACM3` — server construction and router cleanup
4. `ACM4` — Convex host-call dispatch cleanup
5. `ACM5` — service orchestration subsystem cleanup
6. `ACM6` — architecture docs, public facades, and governance cleanup
7. `ACM7` — verification and closeout

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| ACM0 | done; completed the live architectural review, promoted this document as the active generic maintainability owner, and updated `docs/plans/README.md` plus `AGENTS.md` to point future work here first | start `ACM1` by inventorying repeated capability delegation across `persistence/provider.rs`, `executor.rs`, `query.rs`, and `tenant/*.rs` and choosing the smallest concept-owned cleanup that reduces edit width without changing storage behavior |
| ACM1 | done; persistence-owned match macros now centralize provider, executor, store, and snapshot delegation, provider open/create flows normalize through typed opened-tenant adapters, and provider/store mismatch checking now lives with `TenantPersistence` instead of being re-expanded across multiple files | start `ACM2` by inventorying repeated sync/async/cancellable wrapper shapes across `service/queries/query_api.rs`, `service/mutations/direct/{api,execution}.rs`, `service/mutations/journal.rs`, `service/subscriptions/bootstrap.rs`, `service/scheduler/access.rs`, and `service/tenants.rs` and choose the first concept-owned convergence helper |
| ACM2 | done; engine service-flow setup now reuses shared tenant-operation helpers in `service/tenants.rs`, mutation wrapper helpers in `service/mutations/direct/api.rs`, query runtime loaders in `service/queries/query_api.rs`, and one snapshot-evaluation helper in `service/subscriptions/bootstrap.rs`, which reduces repeated sync/async/cancellable setup without hiding behavior | start `ACM3` by inventorying the current `build_router*` and `serve*` overload families across `crates/nimbus-server/src/{lib,router,state}.rs` and selecting the typed internal build pipeline that can own route and serve construction |
| ACM3 | done; `AppState` now builds from one typed `AppStateConfig`, router construction now flows through `RouterBuildConfig` and its capability-owned builder methods, and `serve*` wrappers now all funnel through one internal `serve_with_router_config` path so new route or serve features can plug into a single typed pipeline instead of another overload family | start `ACM4` by inventorying the remaining sync, cancellable, and async host-call dispatch seams across `adapters/convex/host_bridge/async_bridge/dispatch.rs`, `adapters/convex/mod.rs`, and `execution/invocations/mod.rs` and selecting the first operation-owned extraction |
| ACM4 | done; host-call routing now classifies operations into function, query-builder, query-read, document, and scheduler families, each family owns its own sync/cancellable/async dispatch surface, and runtime invocation options plus nested cross-runtime invocation setup now flow through smaller canonical helpers instead of inline mode reconstruction | start `ACM5` by inventorying the current responsibility split across `crates/nimbus-bin/src/service/{mod,execution,lifecycle,project,render}.rs` and choosing the first concept-owned extraction that makes command declaration versus orchestration easier to follow |
| ACM5 | done; `commands.rs` now owns the `nimbus service ...` CLI surface, `service/mod.rs` has shrunk into the orchestration root, and service-local models now live beside execution, lifecycle, process, and render code so the subsystem is easier to navigate by concept ownership instead of root-file accretion | start `ACM6` by inventorying what must stay in `ARCHITECTURE.md`, what `crates/nimbus/src/lib.rs` should still re-export, and whether `AGENTS.md` plus `docs/plans/README.md` already point future maintainability work at the right live owner |
| ACM6 | done; `ARCHITECTURE.md` now keeps the crate map and invariant-level architecture while persistence-engine depth lives in `docs/reference/persistence-engine-baseline.md`, `docs/README.md` points at that new reference, and `crates/nimbus/src/lib.rs` now groups facade exports by concern so the public surface reads more intentionally without changing behavior | start `ACM7` by running the full closeout verification sweep, then archive or retire this plan only after `docs/plans/README.md`, `AGENTS.md`, and the active-plan index all match the landed state |
| ACM7 | done; `make check`, `make test`, `make clippy`, the JS workspace test/build lanes, and an escalated `make ci` rerun are green, `AGENTS.md` plus `docs/plans/README.md` now treat this document as an archived baseline instead of the live owner, and this plan now lives under `docs/plans/archive/` as the closeout record | promote a new active maintainability plan before the next repo-wide cleanup wave unless another active plan already owns the slice |

---

## Work Items

### ACM0. Baseline review and plan promotion

Completed during this planning pass.

Acceptance criteria:

- the next cleanup wave is grounded in the live codebase rather than archived
  hotspot assumptions
- the repo has one active generic maintainability control plane
- file-size and ownership rules are explicit before code movement begins

### ACM1. Normalize persistence-provider capabilities and delegation surfaces

#### Implementation plan

1. Audit repeated backend switching across `provider.rs`, `executor.rs`,
   `query.rs`, and `tenant/*.rs`.
2. Extract or regroup capability-owned wrappers so provider changes require
   fewer parallel edits.
3. Keep provider-local semantics close to the backend that owns them.
4. Record any retained 1,500-to-1,999-line exception in the threshold ledger
   at the same time as the code.

#### Focused verification

- `cargo test -p nimbus-engine`
- `cargo test -p nimbus-storage`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-engine --all-targets -- -D warnings`
- `cargo clippy -p nimbus-storage --all-targets -- -D warnings`

#### Acceptance criteria

- repeated provider-switch logic is materially reduced or better centralized
- persistence capability ownership is easier to name
- storage semantics stay unchanged

### ACM2. Converge duplicated engine service flows behind smaller internal helpers

#### Implementation plan

1. Inventory repeated sync/async/cancellable flow shapes across the selected
   service modules.
2. Extract the narrowest concept-owned helpers that reduce duplication without
   hiding behavior.
3. Keep cancellation, admission, and operation-guard semantics explicit.

#### Focused verification

- `cargo test -p nimbus-engine`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-engine --all-targets -- -D warnings`

#### Acceptance criteria

- engine flow patterns are more canonical across feature families
- public ergonomics stay intact
- future feature work has clearer placement

### ACM3. Simplify server construction and routing into one typed internal pipeline

#### Implementation plan

1. Collapse avoidable route and serve overload sprawl behind one typed internal
   build pipeline.
2. Keep public wrappers thin and intention-revealing.
3. Avoid rebuilding another overload matrix as the replacement.

#### Focused verification

- `cargo test -p nimbus-server`
- `cargo test -p nimbus-runtime`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-server --all-targets -- -D warnings`
- `cargo clippy -p nimbus-runtime --all-targets -- -D warnings`

#### Acceptance criteria

- router and serve construction read as one coherent pipeline
- new server features have a clearer place to plug in
- route behavior stays unchanged

### ACM4. Bound Convex host-call dispatch and runtime invocation wiring

#### Implementation plan

1. Inventory which operation families still require triple-entry sync,
   cancellable, and async wiring.
2. Move those families behind smaller operation-owned seams or shared dispatch
   helpers.
3. Keep `HostBridge` narrow and workspace-independent.

#### Focused verification

- `cargo test -p nimbus-server`
- `cargo test -p nimbus-runtime`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-server --all-targets -- -D warnings`
- `cargo clippy -p nimbus-runtime --all-targets -- -D warnings`

#### Acceptance criteria

- host-call routing is easier to extend and review
- runtime invocation wiring loses avoidable coordination width
- compatibility behavior stays unchanged

### ACM5. Thin the service orchestration subsystem around clearer command ownership

#### Implementation plan

1. Separate command declaration, orchestration, surface resolution, and
   rendering concerns where they still sprawl.
2. Prefer concept-owned modules over broad service-root helper flows.
3. Keep platform-specific logic close to the platform resolution layer.

#### Focused verification

- `cargo test -p nimbus-bin`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-bin --all-targets -- -D warnings`

#### Acceptance criteria

- the `service` subsystem is easier to navigate and debug
- command ownership is clearer
- CLI behavior stays unchanged

### ACM6. Repackage architecture docs, public facades, and governance surfaces

#### Implementation plan

1. Decide what must remain in `ARCHITECTURE.md` and what should move into
   focused reference docs.
2. Reconcile public facade exports with the current architectural ownership
   story.
3. Keep repo entrypoints aligned with the active plan and threshold rules.

#### Focused verification

- the best focused Rust or JS verification for any touched public facade
- `cargo fmt --all --check`
- `cargo check --workspace`
- `npm run test --workspaces --if-present`
- `npm run build --workspaces --if-present`

#### Acceptance criteria

- `ARCHITECTURE.md` is either thinner for a reason or explicitly justified
- public facades are more intentional
- repo docs point to one live maintainability owner

### ACM7. Verification and closeout

#### Implementation plan

1. Run the final verification sweep.
2. Reconcile this plan, `docs/plans/README.md`, `AGENTS.md`, and related
   references with the landed state.
3. Archive or retire this plan only after the workstream is actually complete.

#### Focused verification

- `make check`
- `make test`
- `make clippy`
- `npm run test --workspaces --if-present`
- `npm run build --workspaces --if-present`
- `make ci` if practical

#### Acceptance criteria

- the workstream closes with explicit verification
- docs and repo entrypoints match the landed state
- future agents can resume or review the work from docs and the worktree alone

---

## Execution Log

| Date | Item | Outcome | Verification | Next Step |
| --- | --- | --- | --- | --- |
| 2026-04-21 | ACM0 | `documented` | Authored a new active codebase architecture and maintainability control plane from a live review of the current repo, selected the next six high-signal cleanup moves, updated `docs/plans/README.md` so the plan index treats this document as the active generic cleanup owner, and updated `AGENTS.md` so future agents open this plan before relying on archived maintainability history | docs-only review; no code verification claimed for the planning pass | Start `ACM1` by inventorying repeated provider capability delegation and selecting the first smallest cleanup slice that reduces edit width without changing storage semantics |
| 2026-04-21 | ACM1 | `in_progress` | Completed the required startup procedure against `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`, `docs/plans/README.md`, this active plan, and the reliability references; reconciled `git status` to the promoted docs baseline (`AGENTS.md`, `docs/plans/README.md`, and this plan) and resumed from that worktree state instead of assuming a clean tree | startup doc review plus `git status --short` reconciliation; no code verification yet | Inspect `crates/nimbus-engine/src/persistence/provider.rs`, `executor.rs`, `query.rs`, `tenant.rs`, and `tenant/writes.rs` and implement the first concept-owned capability cleanup slice for `ACM1` |
| 2026-04-21 | ACM1 | `done` | Centralized repeated persistence/provider delegation into persistence-owned match macros, replaced repeated opened-tenant mapping helpers with typed `From<Opened*Tenant>` adapters, normalized provider create/open flows through one opened-tenant helper contract, and moved provider/store mismatch-to-executor pairing onto `TenantPersistence` so backend capability edits no longer require the same hand-written switch pattern across provider, executor, query, snapshot, and tenant capability files | `cargo test -p nimbus-engine`; `cargo test -p nimbus-storage`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p nimbus-engine --all-targets -- -D warnings`; `cargo clippy -p nimbus-storage --all-targets -- -D warnings` | Start `ACM2` by inventorying the duplicated sync/async/cancellable wrapper shapes across the selected engine service modules and extracting the first concept-owned convergence helper |
| 2026-04-21 | ACM2 | `in_progress` | Selected the next eligible item immediately after the `ACM1` closeout and promoted the engine service-flow convergence pass to active work so the control plan stays aligned with the live codebase and worktree | `ACM1` focused verification bundle already green; `ACM2` implementation verification not started yet | Inspect `service/queries/query_api.rs`, `service/mutations/direct/api.rs`, `service/mutations/direct/execution.rs`, `service/mutations/journal.rs`, `service/subscriptions/bootstrap.rs`, `service/scheduler/access.rs`, and `service/tenants.rs` and choose the first canonical internal helper shape for duplicated sync/async/cancellable orchestration |
| 2026-04-21 | ACM2 | `done` | Added a shared `with_tenant_runtime_operation` helper for operation-guard setup, rewired scheduler access and direct mutation execution through that helper, collapsed direct mutation wrapper boilerplate into shared immediate versus scheduled execution helpers, introduced shared query runtime loaders for query and pagination setup, and reused one snapshot-evaluation helper across sync and async subscription bootstrap paths so the engine service surface now reads with one more canonical flow shape across sync/async/cancellable entrypoints | `cargo test -p nimbus-engine`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p nimbus-engine --all-targets -- -D warnings` | Start `ACM3` by inventorying the `build_router*` and `serve*` overload surface across `crates/nimbus-server/src/lib.rs`, `router.rs`, and `state.rs` and extracting the typed internal build pipeline |
| 2026-04-21 | ACM3 | `in_progress` | Selected the next eligible item immediately after the `ACM2` closeout and promoted the server-construction cleanup pass to active work so the control plan stays synchronized before the next code changes land | `ACM2` focused verification bundle already green; `ACM3` implementation verification not started yet | Inspect `crates/nimbus-server/src/lib.rs`, `router.rs`, and `state.rs` and choose the internal build-pipeline extraction that can own route and serve construction while preserving the current public wrappers |
| 2026-04-21 | ACM3 | `done` | Introduced a typed `AppStateConfig`, collapsed router construction behind `RouterBuildConfig` and capability-owned builder methods, and routed every public `serve*` entrypoint through one internal `serve_with_router_config` path so route and serve construction now read as one typed internal pipeline rather than a growing overload matrix | `cargo test -p nimbus-server`; `cargo test -p nimbus-runtime`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p nimbus-server --all-targets -- -D warnings`; `cargo clippy -p nimbus-runtime --all-targets -- -D warnings` | Start `ACM4` by inventorying repeated host-call dispatch structure across `crates/nimbus-server/src/adapters/convex/host_bridge/async_bridge/dispatch.rs`, `adapters/convex/mod.rs`, and `execution/invocations/mod.rs` and extracting the first operation-owned convergence seam |
| 2026-04-21 | ACM4 | `in_progress` | Selected the next eligible item immediately after the `ACM3` closeout and promoted the Convex host-call dispatch cleanup pass to active work so the control plan stays synchronized before host-bridge wiring changes land | `ACM3` focused verification bundle already green; `ACM4` implementation verification not started yet | Inspect `crates/nimbus-server/src/adapters/convex/host_bridge/async_bridge/dispatch.rs`, `adapters/convex/mod.rs`, and `crates/nimbus-server/src/execution/invocations/mod.rs` and choose the first operation-owned extraction that reduces sync/async/cancellable dispatch width without widening `HostBridge` |
| 2026-04-21 | ACM4 | `done` | Reclassified Convex host calls into function, query-builder, query-read, document, and scheduler families, moved sync/cancellable/async dispatch behind those family-owned bridge methods instead of one giant central matrix, added canonical `RuntimeBundleInvocationOptions` constructors, and collapsed nested cross-runtime invocation setup behind one preparation helper so host-call routing and runtime invocation wiring are easier to extend without widening `HostBridge` | `cargo test -p nimbus-server`; `cargo test -p nimbus-runtime`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p nimbus-server --all-targets -- -D warnings`; `cargo clippy -p nimbus-runtime --all-targets -- -D warnings` | Start `ACM5` by inventorying responsibility sprawl across `crates/nimbus-bin/src/service/{mod,execution,lifecycle,project,render}.rs` and extracting the first concept-owned command-ownership seam |
| 2026-04-21 | ACM5 | `in_progress` | Selected the next eligible item immediately after the `ACM4` closeout and promoted the `nimbus service ...` subsystem cleanup pass to active work so the control plan stays synchronized before service-command ownership changes land | `ACM4` focused verification bundle already green; `ACM5` implementation verification not started yet | Inspect `crates/nimbus-bin/src/service/mod.rs`, `execution.rs`, `lifecycle.rs`, `project.rs`, and `render.rs` and choose the first extraction that clarifies command declaration versus orchestration ownership without changing CLI behavior |
| 2026-04-21 | ACM5 | `done` | Moved the CLI surface into a new `crates/nimbus-bin/src/service/commands.rs`, shrank `service/mod.rs` from 770 lines to 465 by leaving it as the orchestration root, and relocated execution, lifecycle, process, and render-owned models into the modules that actually own those behaviors so the `nimbus service ...` subsystem now reads by concept family rather than cross-file root accretion | `cargo test -p nimbus-bin`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p nimbus-bin --all-targets -- -D warnings` | Start `ACM6` by inventorying `ARCHITECTURE.md`, `crates/nimbus/src/lib.rs`, `AGENTS.md`, and `docs/plans/README.md` and deciding which doc/facade cleanup or threshold-justification slice lands first |
| 2026-04-21 | ACM6 | `in_progress` | Selected the next eligible item immediately after the `ACM5` closeout and promoted the architecture-doc and facade sweep to active work so the control plan stays synchronized before documentation-entrypoint or export cleanup lands | `ACM5` focused verification bundle already green; `ACM6` implementation verification not started yet | Inspect `ARCHITECTURE.md`, `crates/nimbus/src/lib.rs`, `AGENTS.md`, and `docs/plans/README.md` and choose the first focused change that reconciles public facades and repo entrypoints with the landed maintainability ownership map |
| 2026-04-21 | ACM6 | `done` | Trimmed `ARCHITECTURE.md` from 1,745 lines to 1,384 by moving persistence-engine, durable-journal, and serving-snapshot depth into the new `docs/reference/persistence-engine-baseline.md`, updated `docs/README.md` to point at that reference, and added concern-grouped facade comments in `crates/nimbus/src/lib.rs` so the public export surface reads more intentionally while the active threshold-exception ledger clears | `cargo test -p nimbus-bin`; `cargo fmt --all --check`; `cargo check --workspace`; `npm run test --workspaces --if-present`; `npm run build --workspaces --if-present` | Start `ACM7` by running the repo-wide closeout verification sweep, reconciling plan indexes and agent entrypoints, and archiving or retiring this plan only after the docs match the landed state |
| 2026-04-21 | ACM7 | `in_progress` | Selected the final closeout item immediately after the `ACM6` doc and facade sweep so the workstream stays active until repo-wide verification and archive bookkeeping are complete | `ACM6` focused verification bundle already green; `ACM7` closeout verification not started yet | Run `make check`, `make test`, `make clippy`, `npm run test --workspaces --if-present`, `npm run build --workspaces --if-present`, and `make ci` if practical, then archive or retire this plan only after `docs/plans/README.md` and `AGENTS.md` reflect the final state |
| 2026-04-21 | ACM7 | `done` | Ran the final closeout verification sweep (`make check`, `make test`, `make clippy`, `npm run test --workspaces --if-present`, `npm run build --workspaces --if-present`, and `make ci`), recorded that the first sandboxed `make ci` attempt stopped at `cargo deny check` because `/Users/jack/.cargo/advisory-dbs/db.lock` was read-only, reran `make ci` successfully with escalation, updated `AGENTS.md` plus `docs/plans/README.md` to treat this work as archived baseline, and moved this control plane into `docs/plans/archive/` | `make check`; `make test`; `make clippy`; `npm run test --workspaces --if-present`; `npm run build --workspaces --if-present`; `make ci` (passed on escalated rerun after the sandbox-only advisory-db lock failure) | Promote a new active maintainability plan before the next repo-wide cleanup wave unless another active plan already owns the slice |
