# Architecture, Modularity, And Maintainability Control Plan

Status: archived completed on 2026-04-21 after `AMM1` through `AMM6` landed
and the final closeout verification sweep passed.

This was the canonical execution control plane for the next repo-wide
architectural cleanup, modularity, canonical naming, maintainability, and
idiomatic-Rust workstream after the completed maintainability and reliability
waves archived under `docs/plans/archive/`.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `AGENTS.md`
- `docs/reference/reliability-posture.md`
- `docs/reference/ci-failure-investigation.md`
- `docs/plans/archive/codebase-reliability-and-maintainability-hardening-plan.md`
- `docs/plans/archive/codebase-modularity-and-maintainability-hotspots-plan.md`
- `docs/plans/archive/codebase-modularity-and-maintainability-follow-on-plan.md`
- `docs/plans/archive/codebase-modularity-and-maintainability-plan.md`
- `crates/nimbus-engine/src/service/mod.rs`
- `crates/nimbus-engine/src/persistence_config.rs`
- `crates/nimbus-engine/src/persistence/tenant.rs`
- `crates/nimbus-engine/src/tenant.rs`
- `crates/nimbus-runtime/src/host.rs`
- `crates/nimbus-server/src/router.rs`
- `crates/nimbus-server/src/adapters/convex/host_bridge/bridge.rs`
- `crates/nimbus-server/src/adapters/convex/host_bridge/async_bridge/mod.rs`
- `crates/nimbus-storage/src/libsql.rs`
- `crates/nimbus-storage/src/mysql.rs`
- `crates/nimbus-storage/src/postgres.rs`
- `packages/codegen/src/main.mjs`
- `packages/nimbus/src/server.ts`
- `packages/nimbus/src/browser.ts`
- `packages/nimbus/src/react.ts`
- `packages/convex/src/server.ts`
- `packages/convex/src/browser.ts`
- `packages/convex/src/react.ts`
- the current git worktree on 2026-04-20

Baseline verification status for this plan:

- this control plane is being authored as a docs-only review and planning pass
  on 2026-04-20
- the worktree already contains in-flight non-plan changes, so this planning
  pass treats the live tree as the current architectural baseline instead of a
  clean-room snapshot
- no new code verification is claimed by this planning pass
- every `AMM*` implementation item must record its own focused verification
  before it can be marked `done`

---

## Purpose

The codebase is in a healthier state than the old god-file era. The central
architecture is real:

- `nimbus-runtime` still owns a narrow workspace-independent runtime contract
- `nimbus-server` still owns transport and compatibility integration
- `nimbus-engine` still owns the central coordinator and tenant runtime
- `nimbus-storage` still owns persistence backends and durable semantics

The next broad cleanup wave should not rewrite that center. The more honest
assessment is that complexity is pooling at the edges:

- persistence-provider roots are large and increasingly bridge-heavy
- service bootstrapping and router construction are growing configuration
  matrices
- the Convex compatibility surface is sprawling across many modules
- the JS SDKs still carry parallel surfaces where a canonical wrapper model
  would be cheaper to maintain
- the repo currently has no active generic cleanup control plane, which raises
  drift risk whenever cross-cutting maintainability work resumes

This plan therefore focuses on the next five high-signal architectural moves
rather than on raw line-count reduction for its own sake.

This is not a feature roadmap. It is a code-organization, boundary-hardening,
and maintainability roadmap.

---

## Relationship To Other Plans

- Use `docs/plans/README.md` as the owning plan index.
- Use this archived plan for the completed repo-wide architecture,
  maintainability, readability, modularity, canonical naming, code-pattern
  cleanup, and god-file cleanup wave when a task needs its execution record,
  governance baseline, or closeout verification bundle.
- Use `docs/reference/reliability-posture.md` and
  `docs/reference/ci-failure-investigation.md` together with this archived
  plan when a task needs the proof-hygiene and CI-hardening decisions that
  closed this wave.
- Use
  `docs/plans/archive/codebase-reliability-and-maintainability-hardening-plan.md`
  only for the completed reliability wave's execution record and closeout
  verification baseline.
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
  only for the predecessor CLI, provider, and sandbox split history.
- Promote a new active plan before landing another repo-wide maintainability
  or reliability-hardening wave unless some other active plan already owns
  the slice.
- This plan is separate from the active feature and platform plans such as
  `encryption-at-rest-plan.md`, `websocket-protocol-plan.md`,
  `localhost-server-security-plan.md`, `system-tenant-api-plan.md`,
  `desktop-ui-plan.md`, `install-script-plan.md`, `distribution-plan.md`,
  `windows-machine-support-plan.md`, `wasmtime-backend-plan.md`, and
  `wasi-agent-capabilities-plan.md`.
- If a cleanup slice turns into feature behavior change, protocol change,
  storage semantic change, platform behavior change, or release/distribution
  work, stop and move to the owning plan instead of stretching this control
  plane across multiple streams.

---

## Scope

This plan covers:

- architectural cleanup of persistence-provider seams and related engine
  facades
- service bootstrapping, typed configuration, and router-construction cleanup
- Convex compatibility boundary cleanup in the server/runtime integration layer
- JS SDK and compatibility-wrapper de-duplication
- canonical file naming, module ownership, size-governance, and thin-root
  rules
- documentation and control-plane updates needed to keep the work resumable
  through handoff and compaction

This plan does not cover:

- new product features
- intentional API or protocol redesign unless a specific item explicitly
  records it
- storage semantic changes beyond structural cleanup
- speculative rewrites that are not justified by ownership, readability,
  maintainability, or future-feature placement
- archived-plan cleanup done only to lower historical document line counts

---

## Cleanup Invariants

These rules are mandatory for every item in this plan.

1. Preserve externally observable behavior by default.
   Native HTTP and WebSocket behavior, runtime host-call behavior, storage
   atomicity, scheduler semantics, and SDK surface behavior stay unchanged
   unless a specific item explicitly records otherwise.

2. Keep core architecture invariants intact.
   `nimbus-core` stays zero I/O.
   `nimbus-runtime` stays zero workspace dependencies.
   All mutations still flow through the engine-owned mutation path.
   Storage atomicity stays unchanged.

3. Split by concept ownership, not by line count alone.
   Do not break files or lines apart just to make them shorter. Move code only
   when doing so produces clearer ownership boundaries, cleaner extension
   points, or more local debugging.

4. Treat file size as a signal, not the goal.
   Files under 1,500 lines are usually acceptable if they are concept-cohesive.
   Files from 1,500 through 1,999 lines need an explicit justification in the
   owning active plan if they remain unsplit.
   Files at 2,000 lines or above must be decomposed or explicitly documented
   as an exception with a strong ownership-based reason. "It still works" is
   not enough.

5. Keep composition roots thin once ownership moves out.
   If a root already delegates through a concept-owned module tree, do not
   turn it back into a renamed god file.

6. Prefer canonical, local names over generic helper buckets.
   Favor concept-owned module names like `scheduled_jobs.rs`, `provider.rs`,
   `dispatch.rs`, or `state.rs`. Avoid adding new `helpers.rs`, `common.rs`,
   `misc.rs`, or `utils.rs` files unless the ownership is truly shared and
   obvious.

7. Prefer local support seams over repo-wide piles.
   Keep test support, parsing helpers, wrapper flows, and small builders near
   the concept that owns them unless there is a clear multi-call-site reuse
   boundary.

8. Favor readable grouping over mechanical formatting.
   Do not introduce vertical churn, line breaks, or micro-extractions that
   make related concepts harder to read together.

9. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

---

## Current Assessed State

- The codebase currently has no active generic architecture-maintainability
  control plane. The last broad cleanup and reliability waves are archived.
- The central architecture remains sound:
  - `Service` is still the engine coordination center
  - `TenantRuntime` still groups tenant-local read, mutation, lifecycle, and
    subscription state
  - `HostBridge` still keeps the runtime contract narrow and workspace-free
  - the server still owns transport and runtime integration
- There are now no active `.rs`, `.ts`, `.mjs`, or active-plan `.md` files
  above the 1,500-line threshold today.
- The next largest active code files are:
  - `crates/nimbus-storage/src/mysql/backend.rs` at 1,236 lines
  - `crates/nimbus-sandbox/src/backends/oci/builder.rs` at 1,199 lines
  - `crates/nimbus-storage/src/postgres/write.rs` at 1,188 lines
  - `crates/nimbus-engine/src/tests/queries.rs` at 1,170 lines
- `AMM1` removed the temporary `libsql.rs` threshold exception by extracting
  replica-cache freshness and provider-backend helper ownership into concept
  modules and by shrinking `crates/nimbus-engine/src/persistence/tenant.rs`
  into a thin capability root.
- `AMM2` moved provider selection, encryption bootstrap, and route/build
  wiring behind typed startup seams, so the engine and CLI no longer keep the
  full bootstrap matrix inline.
- `AMM3` bounded the broadest remaining Convex seams:
  - `ConvexHostBridgeScope` and `ConvexHostBridgeInvocation` now separate
    durable bridge scope from per-call invocation metadata, so
    `ConvexHostBridge` no longer rebuilds that width at every caller
  - `execution/runtime_backed/invoke/context.rs` now owns the canonical
    runtime-backed invocation bundle for `Service`, registry, tenant, and
    runtime-service snapshots
  - runtime-backed route handlers and subscription-transform reevaluation now
    reuse shared context objects instead of threading the same wide argument
    set through each call path
- `AMM4` materially thinned the JS compatibility wrapper:
  - `packages/convex/src/server.ts` now delegates query, mutation, action,
    table, schema, and HTTP wrapper behavior through `nimbus/server` while
    keeping the narrower Convex-facing type aliases and helper names
  - `packages/convex/src/browser.ts` now re-exports shared client-state and
    auth-fetcher types from `nimbus/browser` and drops redundant pass-through
    method wrappers
  - `packages/codegen` remains aligned with one canonical runtime contract by
    continuing to emit `convex/*` imports whose implementation now resolves
    through thinner compatibility wrappers
- The workstream closed through governance and closeout:
  - `AGENTS.md` and `docs/plans/README.md` now treat this archived plan as the
    latest governance baseline for thin-root discipline, concept-owned naming,
    helper-bucket avoidance, threshold exceptions, and wrapper-first JS
    compatibility guidance
  - `AMM6` lifted the AWS KMS dependency chain to `aws-config 1.8.16` and
    `aws-sdk-kms 1.105.0`, removed the legacy `rustls-webpki 0.101.7`
    `cargo deny` path, and made `nimbus-storage` own the Hyper features its
    `aws-kms` support already depended on implicitly
  - the full closeout verification contract is now green, so this plan can
    archive cleanly instead of remaining as a live control-plane blocker

---

## Current Review Findings

1. The architectural center is not the problem.
   The next broad wave should not reopen the engine/runtime/storage/server
   split itself. The better move is to simplify the boundaries around that
   center.

2. Persistence, bootstrap, and Convex runtime-call seams are materially
   healthier after `AMM1`, `AMM2`, and `AMM3`.
   Provider roots, engine persistence facades, service startup, router wiring,
   CLI config assembly, host-bridge setup, and runtime-backed invocation
   paths now route through narrower typed seams.

3. The JS compatibility surface is materially healthier after `AMM4`.
   `packages/convex` now delegates more of its server and browser surface
   through canonical `packages/nimbus` implementations instead of carrying as
   much copy-forward logic.

4. The remaining cleanup became closeout-first rather than seam-first, and is
   now complete.
   `AMM6` resolved the last blocker by lifting the AWS KMS chain to
   `aws-config 1.8.16` and `aws-sdk-kms 1.105.0`, selecting the
   `default-https-client` / `rt-tokio` lane instead of the legacy
   `tls-rustls` path, and clearing the `cargo deny` failure on
   `rustls-webpki 0.101.7`.

5. `AMM5` and `AMM6` codified the seam decisions that landed.
   The repo entrypoints now treat this archived plan as the latest
   governance baseline for thin roots, typed bootstrap pipelines, bounded
   runtime-call contexts, wrapper-first JS compatibility guidance, and the
   final closeout verification record; future broad cleanup work must promote
   a new active plan instead of silently reviving this one.

---

## Top 5 Architectural Moves

### AMM1. Normalize persistence-provider roots and the engine persistence facade

Make the provider roots and the engine-side `TenantPersistence` facade more
intentional before more provider features or encryption-follow-on work expand
them further.

Primary targets:

- `crates/nimbus-storage/src/libsql.rs`
- `crates/nimbus-storage/src/mysql.rs`
- `crates/nimbus-storage/src/postgres.rs`
- `crates/nimbus-engine/src/persistence/tenant.rs`
- adjacent provider-local `provider.rs`, `read.rs`, `write.rs`, and `storage.rs`
  modules as needed

Desired outcome:

- provider roots become unmistakably composition-only or retain only one clear
  ownership reason for staying large
- engine-side delegation shrinks toward a smaller, capability-oriented facade
  instead of one long backend switchboard
- any active file that crosses 1,500 lines during the work must record a
  justification in this plan before landing

### AMM2. Separate service bootstrapping, persistence policy, and router wiring

Keep `Service` focused on runtime coordination rather than on becoming the main
home for provider-selection and bootstrapping policy.

Primary targets:

- `crates/nimbus-engine/src/service/mod.rs`
- `crates/nimbus-engine/src/persistence_config.rs`
- `crates/nimbus-bin/src/serve/config.rs`
- `crates/nimbus-server/src/router.rs`

Desired outcome:

- provider and encryption setup flow through a clearer build pipeline
- typed configuration stays grouped by concept instead of by one mega-enum
- router construction prefers a smaller options model or builder surface over a
  combinatorial overload matrix

### AMM3. Bound the Convex compatibility subsystem behind smaller server facades

Reduce sprawl in the compatibility layer without weakening the runtime
inversion seam.

Primary targets:

- `crates/nimbus-server/src/adapters/convex/`
- especially `host_bridge/`, `execution/`, `registry/`, and subscription
  transform surfaces
- `crates/nimbus-server/src/adapters/convex/host_bridge/bridge.rs`

Desired outcome:

- the compatibility layer reads as a small number of bounded subdomains
  instead of one large tree of adjacent concerns
- host-bridge construction and execution wiring lose avoidable argument width
  and coordination burden
- sync, async, and cancellable wrapper logic keep one canonical home per
  concept family

### AMM4. Make `packages/convex` a thinner compatibility wrapper over canonical JS surfaces

Keep the JS story cheap to evolve by consolidating on canonical `nimbus`
implementations and generated contracts.

Primary targets:

- `packages/nimbus/src/server.ts`
- `packages/nimbus/src/browser.ts`
- `packages/nimbus/src/react.ts`
- `packages/convex/src/server.ts`
- `packages/convex/src/browser.ts`
- `packages/convex/src/react.ts`
- `packages/codegen/src/main.mjs`

Desired outcome:

- `packages/nimbus` owns the canonical implementation and richer Nimbus-native
  surface
- `packages/convex` wraps or re-exports wherever behavior is the same
- type and API drift between the two packages becomes harder to introduce

### AMM5. Institutionalize canonical naming, module ownership, and size governance

Keep future cleanup from regressing into ad hoc naming or new god files.

Primary targets:

- `AGENTS.md`
- `docs/plans/README.md`
- the active hotspots touched by `AMM1` through `AMM4`

Desired outcome:

- module names, support seams, and thin-root rules become explicit repo
  policy
- file-size thresholds are enforced as ownership heuristics rather than as
  arbitrary style rules
- future cleanup work starts from one active control plane instead of
  resurrecting archived plans

---

## Success Criteria

This plan is successful only when all of the following are true:

- the next cleanup wave lands without changing core product semantics by
  accident
- provider roots and engine persistence facades have clearer ownership and less
  delegation sprawl
- service construction, persistence policy, and router setup read as one clean
  build pipeline
- the Convex compatibility tree is easier to navigate and reason about as a
  bounded subsystem
- `packages/convex` is materially thinner and more obviously derived from
  canonical `packages/nimbus` surfaces
- active files between 1,500 and 1,999 lines, if any remain, have explicit
  justifications in the plan closeout notes
- no active code file remains at or above 2,000 lines without a
  recorded, ownership-based exception and a concrete decomposition decision
- docs and repo entrypoints point to this active control plane instead of
  telling future agents to author a new generic one from scratch

---

## Feature Preservation Matrix

| Surface | Preservation Requirement |
| --- | --- |
| Engine mutation and query semantics | service coordination, tenant lifecycle, mutation ordering, and read visibility stay unchanged while roots and config surfaces are repackaged |
| Storage-provider semantics | provider-local CRUD, scheduler, journal, schema, refresh, and durability behavior stay unchanged while provider roots and facades are thinned |
| Runtime inversion seam | `HostBridge` stays the runtime contract and `nimbus-runtime` keeps zero workspace dependencies |
| Native server routes | HTTP and WebSocket behavior stays unchanged unless a specific item explicitly records a behavior change |
| Convex compatibility behavior | runtime-backed query, mutation, action, scheduler, and subscription semantics stay unchanged while internal facades are simplified |
| JS SDK and compat surfaces | public API behavior and type meaning stay compatible unless a specific item explicitly records and justifies a breaking pre-launch cleanup |

---

## Control Plan Rules

1. Implement exactly one `AMM*` item at a time unless the plan explicitly says
   otherwise.
2. Do not start a later item while an earlier eligible item is still
   `in_progress`.
3. If the worktree is dirty, reconcile those changes to the owning item before
   taking new scope.
4. Do not split files or lines mechanically; every structural move must make
   concept ownership easier to name.
5. Do not mark an item `done` without recording focused verification.
6. If implementation reveals a better seam map than this plan currently
   describes, update the plan first, then implement the new shape.
7. Record any active-file line-count exception at the same time as the code
   that requires it.
8. Update the roadmap ledger, checkpoints, and execution log in the same
   change set as the code or docs.

---

## Verification Contract

Every implementation item in this plan must run:

- focused verification for the touched subsystem
- `cargo fmt --all --check`
- `cargo check --workspace`

Use these focused lanes as appropriate:

- for provider-root or persistence-facade work:
  `cargo test -p nimbus-storage`
  `cargo test -p nimbus-engine`
- for service bootstrap, config, or router cleanup:
  `cargo test -p nimbus-engine`
  `cargo test -p nimbus-server`
  `cargo test -p nimbus-bin`
- for Convex compatibility cleanup:
  `cargo test -p nimbus-server`
  `cargo test -p nimbus-runtime`
- for JS SDK and codegen cleanup:
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

No active threshold exceptions remain after `AMM1`; `crates/nimbus-storage/src/libsql.rs`
now sits at 868 lines and the provider or engine roots touched in this pass
are below the 1,500-line review threshold.

Any later exception should append another row to this table.

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| AMM0 | `done` | completed the live architectural review, selected the next five high-signal moves, and promoted this document as the active generic maintainability control plane | none |
| AMM1 | `done` | normalized persistence-provider roots and shrank the engine persistence facade around clearer concept ownership and smaller delegation surfaces | AMM0 |
| AMM2 | `done` | separated service bootstrapping, persistence policy, and router wiring into a clearer build pipeline | AMM0, AMM1 |
| AMM3 | `done` | bounded the Convex compatibility subsystem behind smaller server facades, shared runtime invocation contexts, and cleaner host-bridge construction seams | AMM0, AMM2 |
| AMM4 | `done` | consolidated the JS SDK and compatibility layers so `packages/convex` delegates more behavior and shared types through canonical `packages/nimbus` surfaces | AMM0, AMM3 |
| AMM5 | `done` | landed the canonical naming, module-ownership, and size-governance sweep across the active repo entrypoints | AMM0, AMM1, AMM2, AMM3, AMM4 |
| AMM6 | `done` | lifted the AWS KMS dependency chain off the vulnerable legacy Rustls path, reran the full verification sweep, reconciled the repo entrypoints, and archived this plan as completed | AMM1, AMM2, AMM3, AMM4, AMM5 |

---

## Dependency Graph

- `AMM0` is the current review and plan-promotion pass.
- `AMM1` should come first because persistence seams are the broadest remaining
  structural drag and they influence later service-boot decisions.
- `AMM2` should follow once the persistence seam is clearer so service and
  router cleanup do not freeze a temporary provider matrix into place.
- `AMM3` should follow `AMM2` so server-side compatibility cleanup happens
  against the stabilized build and routing surface.
- `AMM4` should follow `AMM3` because JS wrapper cleanup should point at the
  consolidated canonical server/runtime contract rather than an intermediate
  shape.
- `AMM5` is the governance and sweep pass that codifies the resulting naming,
  size, and thin-root rules after the main structural moves land.
- `AMM6` is the closeout verification and archive pass.

---

## Recommended Delivery Order

1. `AMM1` — persistence seams
2. `AMM2` — service bootstrap and router cleanup
3. `AMM3` — Convex compatibility facades
4. `AMM4` — JS wrapper consolidation
5. `AMM5` — canonical naming and size-governance sweep
6. `AMM6` — verification and closeout

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| AMM0 | done; completed the repo-wide architectural review, identified the next five high-signal moves, and promoted this document as the active owner for generic architecture and maintainability work | start `AMM1` by mapping the current provider roots and `TenantPersistence` delegation surface into concept-owned seams and deciding whether the engine-side facade should shrink by capability grouping, provider-local wrapper extraction, or both |
| AMM1 | done; decomposed `libsql.rs` into `libsql/backend.rs` and `libsql/freshness.rs`, moved MySQL and Postgres shared backend helpers behind provider-local `backend.rs` modules, and split `crates/nimbus-engine/src/persistence/tenant.rs` into capability-owned `reads`, `writes`, `journal`, `scheduler`, and `schema` modules | start `AMM2` by mapping `Service` construction, persistence config, encryption bootstrap, and router overloads into a smaller build pipeline that treats the landed persistence seams as stable inputs |
| AMM2 | done; moved typed provider selection into `crates/nimbus-engine/src/persistence_config.rs`, extracted `crates/nimbus-engine/src/service/bootstrap.rs` so `Service` no longer owns the startup matrix, split CLI encryption versus provider resolution in `crates/nimbus-bin/src/serve/config.rs`, and normalized the server route-build overloads behind one internal `RouterBuildConfig` in `crates/nimbus-server/src/router.rs` | start `AMM3` by inventorying `crates/nimbus-server/src/adapters/convex/host_bridge/`, `execution/`, `registry/`, and subscription transform surfaces and choosing the smallest bounded facades that materially reduce coordination width |
| AMM3 | done; introduced `ConvexHostBridgeScope` plus `ConvexHostBridgeInvocation`, extracted `execution/runtime_backed/invoke/context.rs` as the canonical runtime-backed invocation bundle, and routed runtime-backed handlers plus subscription transforms through shared context objects instead of repeating the same constructor and call width at each site | start `AMM4` by inventorying `packages/nimbus`, `packages/convex`, and `packages/codegen` for wrapper, alias, and re-export opportunities that preserve the current JS surface behavior |
| AMM4 | done; replaced most of `packages/convex/src/server.ts` with a thin typed wrapper over `nimbus/server`, re-exported shared browser client-state and auth-fetcher types from `nimbus/browser`, and removed redundant Convex client pass-through methods so the compatibility package is thinner without changing generated `convex/*` imports or public helper names | start `AMM5` by reconciling `AGENTS.md`, `docs/plans/README.md`, and the control plan language with the thin-root, naming, and size-governance rules proven out by `AMM1` through `AMM4` |
| AMM5 | done; updated `AGENTS.md` and `docs/plans/README.md` so the active maintainability plan now explicitly owns thin-root discipline, concept-owned naming, helper-bucket avoidance, threshold exceptions, and wrapper-first JS compatibility guidance for future cleanup work | start `AMM6` by running the final verification sweep, reconciling any last doc drift, and preparing the plan for archive or retirement only after everything is green |
| AMM6 | done; lifted the AWS KMS dependency chain to `aws-config 1.8.16` and `aws-sdk-kms 1.105.0`, removed the legacy `rustls-webpki 0.101.7` path by selecting the explicit `default-https-client` / `rt-tokio` lane, made `nimbus-storage` own the Hyper features its `aws-kms` tests and libsql transport already required, reran the full verification contract, and archived this plan as completed | none; future broad maintainability or reliability-hardening work must promote a new active plan |

---

## Work Items

### AMM0. Baseline review and plan promotion

Completed during this planning pass.

Acceptance criteria:

- the next cleanup wave is grounded in the live architecture instead of in old
  hotspot assumptions
- the workstream has one active control plane
- file-size and ownership rules are explicit before code movement begins

### AMM1. Normalize persistence-provider roots and engine persistence facades

#### Implementation plan

1. Audit the ownership still living in `libsql.rs`, `mysql.rs`, `postgres.rs`,
   and `persistence/tenant.rs`.
2. Extract or regroup code so roots remain composition-oriented and provider
   facades expose smaller, clearer capability groupings.
3. Keep provider-local types, bridge helpers, and cache ownership beside the
   provider that owns them.
4. Record any retained 1,500-to-1,999-line root justification in the
   threshold ledger at the same time as the code.

#### Focused verification

- `cargo test -p nimbus-storage`
- `cargo test -p nimbus-engine`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-storage --all-targets -- -D warnings`
- `cargo clippy -p nimbus-engine --all-targets -- -D warnings`

#### Acceptance criteria

- the selected provider roots and engine facades have a clearer ownership map
- new provider work has more obvious placement
- provider semantics and storage atomicity stay unchanged

### AMM2. Separate service bootstrapping, persistence policy, and router wiring

#### Implementation plan

1. Move provider selection, encryption bootstrap, and typed config concerns
   toward a clearer builder or bootstrap pipeline.
2. Reduce overload matrices in the server router surface where option structs
   or builder-style wiring better match the real ownership.
3. Keep `Service` focused on coordination rather than on configuration sprawl.

#### Focused verification

- `cargo test -p nimbus-engine`
- `cargo test -p nimbus-server`
- `cargo test -p nimbus-bin`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-engine --all-targets -- -D warnings`
- `cargo clippy -p nimbus-server --all-targets -- -D warnings`
- `cargo clippy -p nimbus-bin --all-targets -- -D warnings`

#### Acceptance criteria

- service construction reads as one clean pipeline
- router setup no longer relies on avoidable overload proliferation
- runtime behavior and route semantics stay unchanged

### AMM3. Bound the Convex compatibility subsystem behind smaller server facades

#### Implementation plan

1. Identify the smallest bounded subdomains inside `adapters/convex/` that can
   be made easier to navigate without product behavior change.
2. Reduce avoidable coordination width in bridge construction and host-call
   execution paths.
3. Keep sync, async, and cancellable wrapper flows canonical per concept.

#### Focused verification

- `cargo test -p nimbus-server`
- `cargo test -p nimbus-runtime`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-server --all-targets -- -D warnings`
- `cargo clippy -p nimbus-runtime --all-targets -- -D warnings`

#### Acceptance criteria

- the compatibility subsystem is easier to reason about as bounded facades
- host-bridge coordination width shrinks
- runtime inversion and compatibility behavior stay unchanged

### AMM4. Consolidate JS SDK and compatibility-wrapper surfaces

#### Implementation plan

1. Inventory the public surfaces where `packages/convex` can defer to
   `packages/nimbus` instead of maintaining parallel logic or types.
2. Keep `packages/codegen` aligned with one canonical runtime and SDK contract.
3. Prefer wrapper, re-export, and alias patterns over copy-forward divergence.

#### Focused verification

- `npm run test --workspaces --if-present`
- `npm run build --workspaces --if-present`

#### Acceptance criteria

- `packages/convex` is materially thinner
- canonical JS ownership is clearer
- public surface behavior stays compatible with the intended pre-launch cleanup

### AMM5. Canonical naming, module ownership, and size-governance sweep

#### Implementation plan

1. Reconcile naming, support-seam placement, and thin-root rules across the
   files touched by `AMM1` through `AMM4`.
2. Record any retained active-file size exception in the threshold ledger.
3. Update repo entrypoints so future cleanup work starts from this plan and the
   canonical size heuristics instead of from archived-plan archaeology.

#### Focused verification

- the best focused crate or workspace checks for the touched surfaces
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- naming and ownership rules are explicit and followed
- no new helper piles or renamed god files are introduced
- file-size exceptions, if any, are documented instead of implicit

### AMM6. Verification and closeout

#### Implementation plan

1. Run the final verification sweep.
2. Reconcile this plan, `docs/plans/README.md`, `AGENTS.md`, and any related
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
- plan indexes and repo entrypoints match the landed state
- future agents can resume or review the work from docs and the worktree alone

---

## Execution Log

| Date | Item | Outcome | Verification | Next Step |
| --- | --- | --- | --- | --- |
| 2026-04-20 | AMM0 | `documented` | Authored a new active architecture, modularity, and maintainability control plane from a live architectural review of the current codebase; selected the top five structural moves and codified the requested file-size and ownership rules | docs-only review; no code verification claimed for the planning pass | Update repo entrypoints to point at this new active plan, then begin `AMM1` with a provider-root and persistence-facade seam inventory |
| 2026-04-21 | AMM0 | `reconciled` | Updated the live control plane after commit `0675a0b4` landed optional encryption at rest; recorded the new `libsql.rs` threshold exception and clarified that encryption now raises the baseline persistence and bootstrapping complexity for `AMM1` and `AMM2` | docs-only reconciliation against the live worktree and latest commit; no code verification claimed for the planning pass | Use the revised baseline when handing the workstream to the implementation agent, with `libsql.rs` treated as an explicit `AMM1` target rather than an unrelated exception |
| 2026-04-21 | AMM1 | `in_progress` | Reconciled the live worktree to the active control-plane docs, marked the provider-root normalization item active, and started the focused ownership inventory for `libsql.rs`, `mysql.rs`, `postgres.rs`, and the engine tenant-persistence facade before any code movement | startup reconciliation only; no item verification claimed yet | Complete the seam inventory, then land the first provider-root or engine-facade decomposition with focused persistence verification |
| 2026-04-21 | AMM1 | `done` | Decomposed `libsql.rs` into `libsql/backend.rs` and `libsql/freshness.rs`, moved MySQL and Postgres shared backend helpers behind provider-local `backend.rs` modules, and split `crates/nimbus-engine/src/persistence/tenant.rs` into capability-owned `reads`, `writes`, `journal`, `scheduler`, and `schema` modules; reduced `crates/nimbus-storage/src/libsql.rs` to 868 lines and cleared the active threshold exception while preserving the landed encryption-backed replica behavior | `cargo test -p nimbus-storage`; `cargo test -p nimbus-engine`; `cargo clippy -p nimbus-storage --all-targets -- -D warnings`; `cargo clippy -p nimbus-engine --all-targets -- -D warnings`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace` | Start `AMM2` by mapping `Service` construction, persistence config, encryption bootstrap, and router wiring into a smaller build pipeline |
| 2026-04-21 | AMM2 | `in_progress` | Promoted the service bootstrap and router cleanup item immediately after AMM1 closeout so the plan stays aligned with the live delivery order; the next work burst will inspect `service/mod.rs`, `persistence_config.rs`, `serve/config.rs`, and `router.rs` together because provider selection and encryption bootstrap now form one current pipeline concern | status handoff only; no AMM2 verification claimed yet | Read the AMM2 targets and land the first builder/bootstrap or router extraction without changing public bootstrap or route semantics |
| 2026-04-21 | AMM2 | `done` | Moved typed provider selection and startup lowering into a private bootstrap plan in `crates/nimbus-engine/src/persistence_config.rs`, extracted `crates/nimbus-engine/src/service/bootstrap.rs` so `Service` no longer owns the startup matrix, split CLI encryption versus provider selection in `crates/nimbus-bin/src/serve/config.rs`, and normalized router construction behind one internal `RouterBuildConfig` in `crates/nimbus-server/src/router.rs` without changing public bootstrap or route semantics | `cargo test -p nimbus-engine` (one initial run hit the timing-sensitive `tests::materialized_serving::concurrency::materialized_surface_handles_concurrent_reads_and_writes`; the isolated rerun and a full rerun both passed); `cargo test -p nimbus-server`; `cargo test -p nimbus-bin`; `cargo clippy -p nimbus-engine --all-targets -- -D warnings`; `cargo clippy -p nimbus-server --all-targets -- -D warnings`; `cargo clippy -p nimbus-bin --all-targets -- -D warnings`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace` | Start `AMM3` by inventorying the Convex compatibility subsystem around host-bridge construction, execution routing, registry access, and subscription transforms |
| 2026-04-21 | AMM3 | `in_progress` | Promoted the Convex compatibility cleanup item immediately after AMM2 closeout so the plan stays aligned with the live delivery order; the next work burst will inspect `crates/nimbus-server/src/adapters/convex/host_bridge/`, `execution/`, `registry/`, and subscription transforms together because that subsystem now carries the broadest remaining coordination width | status handoff only; no AMM3 verification claimed yet | Read the AMM3 targets and land the first bounded facade or constructor-width reduction without changing Convex behavior |
| 2026-04-21 | AMM3 | `done` | Added `ConvexHostBridgeScope` and `ConvexHostBridgeInvocation` so bridge construction separates durable scope from per-call invocation metadata, extracted `crates/nimbus-server/src/adapters/convex/execution/runtime_backed/invoke/context.rs` as the canonical runtime-backed invocation bundle, and routed runtime-backed function handlers plus subscription transform reevaluation through shared context objects instead of rebuilding the same service, registry, tenant, and runtime-service argument set at each call site | `cargo test -p nimbus-server`; `cargo test -p nimbus-runtime`; `cargo clippy -p nimbus-server --all-targets -- -D warnings`; `cargo clippy -p nimbus-runtime --all-targets -- -D warnings`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace` | Start `AMM4` by inventorying `packages/nimbus`, `packages/convex`, and `packages/codegen` for wrapper or re-export consolidation opportunities |
| 2026-04-21 | AMM4 | `in_progress` | Promoted the JS wrapper-consolidation item immediately after AMM3 closeout so the roadmap stays aligned with the live worktree; the next work burst will inspect the browser, react, server, values, and codegen surfaces together because they now carry the broadest remaining compatibility drift risk | status handoff only; no AMM4 verification claimed yet | Read the AMM4 targets and land the first wrapper or alias consolidation without changing intended pre-launch JS behavior |
| 2026-04-21 | AMM4 | `done` | Replaced most of `packages/convex/src/server.ts` with thin typed wrappers over `nimbus/server`, re-exported shared browser state and auth-fetcher types from `nimbus/browser`, and removed redundant Convex client pass-through methods so the compatibility package now delegates more behavior through canonical `packages/nimbus` surfaces while preserving Convex helper names and generated import paths | `npm run test --workspaces --if-present`; `npm run build --workspaces --if-present`; `cargo fmt --all --check`; `cargo check --workspace` | Start `AMM5` by reconciling `AGENTS.md`, `docs/plans/README.md`, and active-plan ownership guidance with the patterns proven out by the first four items |
| 2026-04-21 | AMM5 | `in_progress` | Promoted the naming, thin-root, and size-governance sweep immediately after the JS wrapper closeout so the active plan remains the live governance source; the next work burst will inspect `AGENTS.md`, `docs/plans/README.md`, and any touched architecture notes for remaining drift against the codebase state | status handoff only; no AMM5 verification claimed yet | Read the governance entrypoints and land the smallest updates that make the active plan, repo guidance, and landed code tell the same ownership story |
| 2026-04-21 | AMM5 | `done` | Updated `AGENTS.md` and `docs/plans/README.md` so the active maintainability control plane now explicitly owns thin-root discipline, concept-owned naming, helper-bucket avoidance, threshold exceptions, and wrapper-first JS compatibility guidance for future cleanup work | `cargo fmt --all --check`; `cargo check --workspace` | Start `AMM6` by running the final verification sweep and reconciling the closeout or archive state from the worktree plus docs |
| 2026-04-21 | AMM6 | `in_progress` | Promoted the final verification and closeout item immediately after the governance sweep so the roadmap stays aligned with the live worktree; the next work burst will run the full verification contract, reconcile any remaining doc drift, and archive or retire this active plan only if the verification state is explicit and green | status handoff only; no AMM6 verification claimed yet | Run `make check`, `make test`, `make clippy`, the JS test or build lanes, and `make ci` if practical before closing the workstream |
| 2026-04-21 | AMM6 | `blocked` | Ran the final closeout verification sweep: `make check`, `make test`, `make clippy`, `npm run test --workspaces --if-present`, and `npm run build --workspaces --if-present` all passed, but `make ci` failed in `cargo deny` on `RUSTSEC-2026-0098` and `RUSTSEC-2026-0099` for `rustls-webpki 0.101.7` through `aws-smithy-http-client 1.1.12` in the AWS KMS chain. An initial sandboxed `make ci` also failed because `cargo deny` could not lock `~/.cargo/advisory-dbs/db.lock`; the escalated rerun removed that environment-only blocker and exposed the real advisory failure. Escalated `cargo update -p aws-config --dry-run` and `cargo update -p aws-smithy-http-client --dry-run` both reported `Locking 0 packages to latest compatible versions`, so there is no simple latest-compatible lockfile lift available from the current dependency ranges. | `make check`; `make test`; `make clippy`; `npm run test --workspaces --if-present`; `npm run build --workspaces --if-present`; `make ci` (failed in `cargo deny` on `RUSTSEC-2026-0098` and `RUSTSEC-2026-0099`); escalated `cargo update -p aws-config --dry-run`; escalated `cargo update -p aws-smithy-http-client --dry-run` | Keep `AMM6` active until the AWS dependency-chain advisories are resolved or an explicit deny-policy exception is approved and recorded; do not archive this plan yet |
| 2026-04-21 | AMM6 | `done` | Lifted the AWS KMS dependency chain to `aws-config 1.8.16` and `aws-sdk-kms 1.105.0`, disabled the `aws-sdk-kms` default feature set so the build now uses the explicit `default-https-client` / `rt-tokio` transport lane instead of the legacy `tls-rustls` path, and made `nimbus-storage` own the Hyper `client`, `server`, `http1`, and `runtime` features its libsql transport plus `aws-kms` test harness already required after the legacy AWS path stopped supplying them transitively. The updated graph no longer contains `rustls-webpki 0.101.7`, the focused `aws-kms` verification is green again, the full closeout sweep including `make ci` now passes, and the repo entrypoints have been reconciled so this plan can archive cleanly as the latest completed maintainability governance baseline. | `cargo update -p aws-config --precise 1.8.16`; `cargo update -p aws-sdk-kms --precise 1.105.0`; `cargo tree -p nimbus-storage -e features --features aws-kms -i aws-sdk-kms`; `cargo tree -p nimbus-storage --features aws-kms -i rustls-webpki@0.103.12`; `cargo test -p nimbus-storage --features aws-kms`; `cargo clippy -p nimbus-storage --all-targets --features aws-kms -- -D warnings`; `cargo fmt --all --check`; `cargo check --workspace`; `make check`; `make test`; `make clippy`; `npm run test --workspaces --if-present`; `npm run build --workspaces --if-present`; `make ci` | None; future repo-wide maintainability or reliability-hardening work must promote a new active plan instead of resuming this archived one |
