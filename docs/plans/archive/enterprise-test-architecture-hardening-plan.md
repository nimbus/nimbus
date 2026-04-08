# Enterprise Test Architecture Hardening Control Plan

This is the canonical execution control plane for the next test-architecture
and harness-ownership pass.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `docs/research/tigerbeetle-code-reference.md`
- `.github/workflows/ci.yml`
- `scripts/verification-harness.sh`
- `crates/neovex-runtime/src/executor.rs`
- `crates/neovex-runtime/src/runtime/tests/`
- `crates/neovex-engine/src/tests/mutation_journal.rs`
- `crates/neovex-engine/src/tests/materialized_serving.rs`
- `crates/neovex-engine/src/tests/subscriptions.rs`
- `crates/neovex-engine/src/tests/queries.rs`
- `crates/neovex-testing/src/lib.rs`
- `crates/neovex-testing/src/eventual.rs`

Baseline verification status for this plan:

- the current live worktree already includes the completed deterministic test
  and harness hardening pass plus the in-progress `neovex-testing` crate rename
- this control plane is being authored as a docs-only review-and-planning pass
  on 2026-04-08 against that live worktree
- no new code verification is claimed by this planning pass
- every `TA*` implementation item must record its own focused verification
  before it can be marked `done`

---

## Purpose

The last hardening pass fixed the biggest determinism and CI-topology gaps:

- explicit runtime test profiles
- subprocess isolation for V8-sensitive runtime tests
- deterministic eventual helpers
- named repro metadata
- dedicated verification-harness lanes

What remains is the next layer of test architecture quality:

- the remaining runtime executor test root is still a real god file
- several engine and server test surfaces still use arbitrary sleep-based
  polling where concept-owned waiters or gates would be clearer
- `neovex-testing` now has the right name, but it does not yet own all of the
  canonical harness primitives we want contributors to reuse
- the verification harness still has no first-class runtime surface, even
  though runtime liveness and executor-accounting regressions are one of the
  most enterprise-sensitive areas in the repo

This plan exists to make the test architecture itself more idiomatic, more
concept-owned, and more trustworthy for a system that hosts other applications
and needs enterprise-grade confidence.

---

## Reference Posture

Use the following systems as style and verification references for this plan:

- TigerBeetle for harsh deterministic replay, liveness or safety campaigns, and
  a refusal to hide correctness gaps behind loose test infrastructure
- CockroachDB for workload generation, independent verification of observed
  state, and test architecture that treats correctness as a first-class product
  surface
- FoundationDB for simulation seams, seeded repros, and explicit fault or
  coordination primitives instead of ad hoc sleeps

What to borrow:

- explicit test taxonomy
- reproducible campaigns with stable case ids
- concept-owned harness primitives
- independent oracles where the implementation is easy to accidentally trust
- narrow and honest isolation categories instead of broad blanket workarounds

What not to copy literally:

- a full custom test runner
- a distributed-systems simulator that does not match Neovex's product
  architecture
- blanket single-threading or artificially constrained product behavior to make
  tests look stable

---

## Relationship To Other Plans

- Use `docs/plans/README.md` as the owning plan index.
- This plan supersedes the completed deterministic harness pass in
  `docs/plans/archive/deterministic-test-and-harness-hardening-plan.md`.
- This plan is separate from:
  `docs/plans/convex-demos-compatibility-plan.md`,
  `docs/plans/encryption-at-rest-plan.md`,
  `docs/plans/v8-locker-fork-plan.md`,
  `docs/plans/layered-admission-control-plan.md`,
  `docs/plans/pluggable-storage-backend-plan.md`,
  and `docs/plans/wasmtime-backend-plan.md`.
- If work turns into product runtime semantics, Convex compatibility feature
  work, encryption-at-rest implementation, admission control, storage-backend
  abstraction, or Wasmtime backend work, stop and move to the owning plan
  instead of stretching this pass across multiple streams.

---

## Scope

This plan covers:

- concept-owned modularization of the remaining executor test god file
- promotion of `neovex-testing` into the obvious home for shared harness
  primitives
- removal of remaining arbitrary polling sleeps from the highest-value runtime,
  engine, and server test surfaces
- a first-class runtime verification-harness surface with stable repro commands
  and CI ownership
- stronger runtime-executor and transport liveness campaigns where enterprise
  trust depends on accounting, fairness, cancellation, or resubscribe behavior
- documentation and archive closure for this pass

This plan does not cover:

- broad production refactors unrelated to test or harness architecture
- splitting already concept-owned engine test files just because they are long
- a JS testing rewrite
- a whole new test runner
- intentional changes to product defaults, route behavior, or wire semantics
  unless explicitly recorded

---

## Test Architecture Invariants

1. Treat the test architecture as production architecture.
   Harness ownership, deterministic signals, and repro paths are part of the
   product trust story, not cleanup afterthoughts.

2. Do not split files just to reduce line counts.
   Split only where concept ownership becomes clearer and future contributors
   would know where new tests or helpers belong.

3. Do not change product defaults to stabilize tests.
   Tests must select explicit profiles or harness categories when they depend on
   non-default behavior.

4. Prefer deterministic signals and gates over arbitrary sleeps.
   Remaining sleeps must model intentional elapsed time, not poll for eventual
   state when the harness can observe that state directly.

5. Keep the verification harness honest.
   If a surface is important enough to need special runtime isolation or a
   dedicated campaign lane, encode that explicitly instead of hiding it inside
   generic workspace test lanes.

6. Keep shared harness ownership obvious.
   Shared helpers belong in `neovex-testing` or a clear local runtime
   `test_support` module, not duplicated ad hoc across large test files.

7. Treat the current git worktree as baseline reality.
   Reconcile dirty worktree state before starting any implementation item.

---

## Current Assessed State

- `crates/neovex-runtime/src/runtime/tests/` already demonstrates the right
  pattern for concept-owned extracted runtime tests: a thin composition root,
  shared support, and per-domain test files.
- `crates/neovex-runtime/src/executor.rs` still keeps a small production root
  and roughly 1.7k lines of inline tests plus shared support, which makes it
  the last remaining true test-architecture god file in the runtime crate.
- The large engine test files
  (`tests/mutation_journal.rs`, `tests/materialized_serving.rs`,
  `tests/subscriptions.rs`, `tests/queries.rs`) are already concept-owned and
  should be treated as cohesive subsystem surfaces, not line-count emergencies.
- `neovex-testing` already owns reusable fixtures, eventual helpers, repro
  metadata, runtime profiles, and deterministic harness entrypoints, but it is
  not yet the obvious home for every remaining shared wait or liveness helper.
- CI already distinguishes runtime lane, workspace lane, and verification
  harness lanes, but the verification harness still only knows `storage`,
  `engine`, and `server`.

---

## Current Review Findings

1. `crates/neovex-runtime/src/executor.rs` is now the clearest remaining test
   god file.
   It holds a thin production composition root plus a large inline `tests`
   module with natural clusters: cooperative execution model behavior, worker
   router and affinity behavior, blocking/lifecycle behavior, and queue or
   permit or fairness behavior.

2. Remaining sleep-based waits are still concentrated in important subsystem
   tests.
   Current hotspots include
   `crates/neovex-engine/src/tests/mutation_journal.rs`,
   `crates/neovex-engine/src/tests/subscriptions.rs`,
   `crates/neovex-engine/src/tests/queries.rs`,
   `crates/neovex-engine/src/tests/materialized_serving.rs`,
   `crates/neovex-engine/src/service/scheduler/tests.rs`,
   `crates/neovex-server/tests/reactive_loop/socket/subscriptions.rs`,
   and several server scheduling/runtime-bridge tests.

3. The current verification harness has no runtime surface.
   Runtime queue accounting, cooperative concurrent dispatch, integrity-after-
   success behavior, and warm-pool isolation are still reproduced through unit
   test names and subprocess helpers, not through the same `pr`/`nightly`/`repro`
   harness story used for storage, engine, and server.

4. `neovex-testing` is named correctly now, but its ownership story can still
   be sharpened.
   Contributors still need clearer homes for subsystem-specific waiters, gates,
   and campaign metadata so new test code does not drift back into local,
   copy-pasted helpers.

5. The current CI topology is close to correct, but runtime verification is
   still split between the dedicated runtime workspace lane and ad hoc
   subprocess-isolated test functions instead of a more explicit runtime-harness
   category.

---

## Assessed But Not Selected

- Splitting the large engine concept test files just because they are long.
  They are already concept-owned by subsystem and would mostly gain indirection,
  not clarity.

- Replacing Rust's default test runner.
  The current gaps are ownership and determinism, not an immediate runner
  limitation.

- Broad JS testing redesign.
  The current high-value work is in Rust runtime, engine, server, and shared
  harness surfaces.

- Blanket CI serialization.
  Temporary narrow serialization can be honest for a specific harness lane, but
  "run more things single-threaded" is not an acceptable architecture answer.

---

## Success Criteria

This plan is successful only when all of the following are true:

1. The executor test root follows the same concept-owned pattern as the runtime
   test root.

2. The remaining high-value polling sleeps in runtime, engine, and server
   tests are replaced with deterministic waiters or explicit modeled delays.

3. `neovex-testing` is the obvious place to add new shared runtime, transport,
   scheduling, and eventual-assertion helpers.

4. Runtime liveness and executor-accounting campaigns have the same stable
   repro story as the existing storage, engine, and server harness surfaces.

5. CI and local verification entrypoints surface the same runtime campaign
   categories and repro names that developers use locally.

6. The resulting architecture is easier to extend without reintroducing god
   files, duplicated wait loops, or unnamed fragile scenarios.

---

## Feature Preservation Matrix

| Surface | Must Stay Stable |
| --- | --- |
| Runtime semantics | Invocation, timeout, cancellation, fairness, pooling, bundle integrity, and warm-pool behavior stay unchanged. |
| Engine semantics | Mutation journal, materialized serving, queries, subscriptions, and scheduler semantics stay unchanged. |
| Server and transport semantics | Native HTTP, WebSocket bootstrap, auth change, unsubscribe, disconnect cleanup, and Convex-compatible flows stay unchanged. |
| Verification entrypoints | `make check`, `make test`, `make clippy`, and `bash scripts/verification-harness.sh ...` remain the canonical verification entrypoints. |
| Product defaults | Production defaults stay a product decision, not a test-harness workaround. |

---

## Control Plane Rules

### Status model

- `todo`: not started
- `in_progress`: actively being implemented; keep exactly one item in this
  state during a single autonomous run unless this plan explicitly allows
  otherwise
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification is recorded in the
  execution log

### Recovery loop for every new session or post-compaction resume

1. Reread this plan's `Execution Log`, `Roadmap Status Ledger`, `Dependency
   Graph`, `Recommended Delivery Order`, and `Implementation Checkpoints`, then
   inspect the current git worktree.
2. If any item is `in_progress`, resume it first.
3. If the worktree is dirty, reconcile the changes to an owning item before
   choosing new scope.
4. Implement exactly one roadmap item at a time unless this plan explicitly
   says otherwise.
5. Do not "fix tests" by changing product defaults or widening blanket CI
   serialization unless that temporary containment is recorded with an exit
   condition in this plan.
6. Update this plan's ledger and execution log in the same change set as the
   code or docs.

---

## Verification Contract

### Minimum verification for every implementation item

- targeted tests for the touched harness or test surface
- targeted tests for every moved or hardened invariant
- `cargo fmt --all --check`

For Rust items that change runtime, engine, server, shared harness behavior, or
CI entrypoints, also run the focused commands recorded below for that item.

### Repo-wide verification before closing the plan

- `make check`
- `make test`
- `make clippy`
- `bash scripts/verification-harness.sh pr storage`
- `bash scripts/verification-harness.sh pr engine`
- `bash scripts/verification-harness.sh pr server`
- `bash scripts/verification-harness.sh pr runtime` once that surface exists
- `make ci` if practical

If environment, advisory-db locks, shared Cargo artifacts, or CI-only tools
block a command, record that limitation explicitly in the execution log instead
of silently skipping it.

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| TA0 | `done` | reviewed the current runtime, engine, server, and shared harness surfaces and identified the next high-value test-architecture and enterprise-hardening seams | none | docs-only review and planning pass on 2026-04-08 |
| TA1 | `done` | split `crates/neovex-runtime/src/executor.rs` test ownership into concept-owned files | none | completed on 2026-04-08 with focused runtime verification |
| TA2 | `done` | promote `neovex-testing` as the canonical home for the next layer of shared waiters, gates, and campaign helpers | TA1 recommended first | completed on 2026-04-08 with shared fault-gate consolidation and focused compile verification |
| TA3 | `done` | replace the remaining high-value arbitrary polling sleeps with deterministic waiters or explicit modeled delays | TA2 | completed on 2026-04-08 with focused engine/server/runtime verification |
| TA4 | `done` | add a first-class runtime verification-harness surface with stable repro entrypoints | TA1, TA2 | completed on 2026-04-08 with explicit runtime PR/nightly/repro categories wired through the local launcher and CI |
| TA5 | `done` | add stronger runtime-executor and transport liveness or safety campaigns on the explicit harness surfaces | TA2 through TA4 | completed on 2026-04-08 with runtime fairness/accounting cases and server transport-liveness harness campaigns |
| TA6 | `done` | updated the docs, ran the full verification sweep, and archived the completed plan cleanly | TA1 through TA5 | completed on 2026-04-08 with full repo, harness, and CI verification |

---

## Dependency Graph

- `TA1` is the recommended first slice because `executor.rs` is the clearest
  remaining god file and already has a proven extraction pattern in the runtime
  test tree.
- `TA2` should follow `TA1` so the promoted shared helpers match the executor
  test taxonomy instead of getting invented twice.
- `TA3` should follow `TA2` because deterministic waiter cleanup is easier once
  the shared helper surface is explicit.
- `TA4` depends on `TA1` and `TA2` because a runtime verification-harness
  surface needs stable test categories and shared repro helpers.
- `TA5` depends on the earlier items because liveness campaigns should sit on
  the hardened harness architecture, not on ad hoc local helpers.
- `TA6` closes the workstream after the modularization, harness promotion,
  campaign, and CI work land.

---

## Recommended Delivery Order

1. `TA1` — executor test extraction
2. `TA2` — `neovex-testing` shared harness promotion
3. `TA3` — deterministic waiter cleanup
4. `TA4` — runtime verification-harness surface
5. `TA5` — stronger runtime and transport campaigns
6. `TA6` — docs, verification, and archive

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| TA0 | done | start `TA1` by extracting the executor test clusters into `executor/tests/` with a shared support module |
| TA1 | done | executor test ownership now lives in `executor/tests/` with shared test hosts and bundle helpers in `executor/tests/support.rs`; focused runtime verification is green |
| TA2 | done | `neovex-testing` now owns the shared `faults` harness module (`BlockingFaultInjector`, `ArmedBlockingFaultInjector`), and engine/server test surfaces consume those canonical gates instead of local duplicates |
| TA3 | done | engine mutation-journal, query, subscription, materialized-serving, scheduler, runtime queue-fairness, and reactive-loop transport waits now use shared eventual helpers, explicit task pending assertions, or clearly documented modeled delays |
| TA4 | done | `scripts/verification-harness.sh`, CI, `docs/README.md`, and `ARCHITECTURE.md` now expose a first-class `runtime` harness surface with explicit PR/nightly/repro campaigns over named runtime cases while keeping subprocess isolation runtime-owned |
| TA5 | done | runtime verification now includes named fairness/accounting campaigns, and the server harness now owns transport-liveness campaigns for websocket auth changes, disconnect cleanup, scheduler publication, and runtime fairness rejection paths with stable repro routing |
| TA6 | done | docs now describe the final runtime/server harness topology, the full repo and harness verification sweep is green, and the completed control plane is ready to archive with live entrypoints removed |

---

## Work Items

### TA0. Baseline review and hotspot map

#### Outcome

- Completed during this planning pass.

### TA1. Split `crates/neovex-runtime/src/executor.rs` test ownership

#### Implementation plan

1. Keep `executor.rs` as the production composition root plus shared
   `#[cfg(test)]` helpers that genuinely need to remain there.
2. Extract the inline executor tests into concept-owned files under
   `crates/neovex-runtime/src/executor/tests/`, likely including:
   router or affinity,
   cooperative model,
   queue or permits or fairness,
   and blocking or lifecycle or timeout.
3. Introduce a shared `executor/tests/support.rs` if the existing helper hosts,
   bundle writers, and runtime context helpers should not all live in the root
   test module.
4. Follow the same `use super::*;` pattern already established in
   `runtime/tests/`.

#### Focused verification

- `cargo test -p neovex-runtime --lib`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-runtime --all-targets -- -D warnings`

#### Acceptance criteria

- `executor.rs` is a thin production root plus only the unavoidable shared test
  scaffolding
- new executor tests have concept-owned homes that future contributors can
  extend without reopening one flat inline test root

### TA2. Promote `neovex-testing` into the next shared harness layer

#### Implementation plan

1. Audit the current remaining ad hoc wait, polling, and scenario helpers
   across runtime, engine, and server tests.
2. Move the helpers that are truly cross-surface into `neovex-testing` with
   clear concept ownership instead of expanding local copy-paste clusters.
3. Prefer concept-owned helper modules such as eventual assertions, transport
   waiters, scheduler waiters, runtime campaign metadata, or publication gates
   over one large catch-all helper file.
4. Keep runtime-only helpers local to `neovex-runtime::test_support` when the
   ownership is inherently runtime-specific.

#### Focused verification

- `cargo test -p neovex-testing --no-run`
- `cargo test -p neovex-engine --no-run`
- `cargo test -p neovex-server --no-run`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- contributors have an obvious shared home for the next layer of deterministic
  harness helpers
- local helpers shrink where ownership is actually shared
- runtime-only support stays runtime-owned instead of leaking upward

### TA3. Replace the remaining high-value polling sleeps

#### Implementation plan

1. Replace remaining arbitrary polling sleeps in the highest-value runtime,
   engine, and server test hotspots with deterministic waiters, gates, manual
   clocks, or explicit eventual helpers.
2. Keep deliberate modeled delays where the test is actually asserting timeout,
   cancellation, or wall-clock semantics instead of polling for state.
3. Focus on the current real hotspots first:
   `tests/mutation_journal.rs`,
   `tests/subscriptions.rs`,
   `tests/queries.rs`,
   `tests/materialized_serving.rs`,
   scheduler tests,
   reactive-loop subscription tests,
   and runtime executor wait loops that are still only polling.

#### Focused verification

- `cargo test -p neovex-engine`
- `cargo test -p neovex-server`
- `cargo test -p neovex-runtime --lib`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- remaining sleeps in these hotspots are either gone or clearly intentional
  modeled delays
- failure output is more deterministic and less timing-window sensitive

### TA4. Add a runtime verification-harness surface

#### Implementation plan

1. Extend `scripts/verification-harness.sh` and its CI topology with a
   first-class `runtime` surface once the runtime executor test taxonomy is
   explicit enough to support it honestly.
2. Define named runtime `pr` and, if warranted, `nightly` campaigns around the
   current enterprise-sensitive weak spots rather than relying only on unit test
   names.
3. Keep runtime isolation explicit: subprocess-sensitive runtime campaigns must
   still encode their isolation requirement in the harness rather than relying
   on CI quirks.
4. Make the local repro story match the CI story, including stable case ids and
   one-command reruns where practical.

#### Focused verification

- `cargo test -p neovex-runtime --lib`
- `bash scripts/verification-harness.sh pr runtime`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- runtime verification is a first-class harness surface instead of an ad hoc
  collection of isolated unit tests
- local and CI runtime repro commands use the same categories and names

### TA5. Add stronger runtime and transport campaigns

#### Implementation plan

1. Add or promote the highest-value liveness and safety campaigns on the new
   shared harness surfaces, especially around:
   executor load accounting,
   cooperative concurrent dispatch,
   integrity after prior success,
   fairness under permit pressure,
   websocket auth or resubscribe behavior,
   disconnect cleanup,
   and scheduler publication timing.
2. Where practical, use independent outcome oracles instead of trusting a
   single code path.
3. Stamp campaigns with stable case ids, explicit profiles, and honest repro
   commands.

#### Focused verification

- `cargo test -p neovex-runtime --lib`
- `cargo test -p neovex-server`
- `bash scripts/verification-harness.sh pr runtime`
- `bash scripts/verification-harness.sh pr server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- the current runtime and transport weak spots are covered by named, stable,
  reproducible campaigns
- failures surface enough context to debug without guesswork

### TA6. Docs and full verification closure

#### Implementation plan

1. Update `ARCHITECTURE.md`, `docs/README.md`, `docs/plans/README.md`,
   `AGENTS.md`, and any other relevant docs to describe the landed test
   architecture clearly.
2. Remove stale checkpoint text and ensure the ledger, dependency graph, and
   execution log match reality.
3. Archive the completed plan once all non-deferred work is done.

#### Focused verification

- `make check`
- `make test`
- `make clippy`
- `bash scripts/verification-harness.sh pr storage`
- `bash scripts/verification-harness.sh pr engine`
- `bash scripts/verification-harness.sh pr server`
- `bash scripts/verification-harness.sh pr runtime` once that surface exists
- `make ci` if practical

#### Acceptance criteria

- docs explain the landed test architecture and contribution rules
- the plan can be archived cleanly with no ledger or worktree mismatch

---

## Execution Log

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-08 | TA0 | done | Reviewed the current runtime executor test root, the concept-owned engine test surfaces, the shared `neovex-testing` crate, the verification-harness script, and the CI topology. Confirmed that the next high-value work is not another generic hardening pass but a focused test-architecture pass: executor test extraction, shared harness ownership promotion, remaining deterministic waiter cleanup, and a first-class runtime verification-harness surface. | docs-only review and planning pass; no new code verification claimed in this handoff | start `TA1` by extracting the executor test clusters into concept-owned files under `executor/tests/` |
| 2026-04-08 | TA1 | done | Extracted the remaining inline executor tests into concept-owned `executor/tests/` modules (`lifecycle`, `cooperative`, `router_affinity`, `queue_fairness`) with shared hosts, bundle writers, and request helpers in `executor/tests/support.rs`, leaving `executor.rs` as a thin production-plus-test-composition root. | `cargo test -p neovex-runtime --lib`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p neovex-runtime --all-targets -- -D warnings` | start `TA2` by auditing shared deterministic waiters, transport helpers, and campaign metadata that should move into `neovex-testing` |
| 2026-04-08 | TA2 | done | Promoted shared adversarial fault-gate ownership into `neovex-testing` via a concept-owned `faults` module, added `ArmedBlockingFaultInjector`, removed duplicate engine/server-local blocking fault injectors, and updated the architecture/docs to point contributors at the shared harness home. | `cargo test -p neovex-testing --no-run`; `cargo test -p neovex-engine --no-run`; `cargo test -p neovex-server --no-run`; `cargo fmt --all --check`; `cargo check --workspace` | start `TA3` by replacing the highest-value remaining arbitrary sleeps in engine/server hotspots with deterministic waiters or explicit modeled delays |
| 2026-04-08 | TA3 | done | Replaced the remaining arbitrary polling sleeps in the targeted engine/server/runtime hotspots with shared eventual helpers, deterministic pending-task assertions, and explicit cleanup waiters; kept only the scheduler and runtime wall-clock sleeps that are the actual modeled timing subject of the test. | `cargo fmt --all --check`; `cargo test -p neovex-runtime --lib`; `cargo test -p neovex-engine`; `cargo test -p neovex-server`; `cargo check --workspace` | start `TA4` by making runtime a first-class verification-harness surface with stable PR/nightly/repro categories |
| 2026-04-08 | TA4 | done | Added a first-class `runtime` verification-harness surface: runtime now owns explicit PR/nightly ignored corpus tests with stable case ids, `scripts/verification-harness.sh` and CI expose `runtime` alongside the other surfaces, and the verification docs now describe the runtime harness taxonomy and repro flow. | `cargo fmt --all`; `cargo test -p neovex-runtime --lib`; `bash scripts/verification-harness.sh pr runtime`; `bash scripts/verification-harness.sh repro runtime pr runtime-cooperative-concurrent-dispatch`; `cargo check --workspace`; `cargo fmt --all --check` | start `TA5` by adding the next named runtime and transport liveness campaigns on top of the explicit harness surfaces |
| 2026-04-08 | TA5 | done | Promoted the next weak spots into explicit named campaigns: the runtime harness now covers queue-limit rejection accounting and no-starvation fairness on top of the earlier integrity/concurrency cases, and the server harness now owns transport-liveness campaigns for websocket disconnect cleanup, auth-change resubscribe, scheduler publication, and runtime fairness rejection paths with exact-case repro routing. | `cargo fmt --all`; `cargo test -p neovex-runtime --lib`; `cargo test -p neovex-server`; `bash scripts/verification-harness.sh pr runtime`; `bash scripts/verification-harness.sh pr server` (sandbox bind denied locally; reran successfully with escalation); `bash scripts/verification-harness.sh repro runtime pr runtime-queue-limit-rejection-accounting`; `bash scripts/verification-harness.sh repro server pr websocket-auth-change-resubscribe` (reran with escalation for local bind permissions); `cargo check --workspace`; `cargo fmt --all --check` | finish `TA6` by updating the final docs, running the full verification sweep, and archiving the completed plan cleanly |
| 2026-04-08 | TA6 | done | Closed the workstream by updating the architecture and verification docs to describe the landed runtime and server harness topology, running the full repo and harness verification sweep, and removing the live plan entrypoints before archiving the completed control plane. | `cargo fmt --all --check`; `make check`; `make test`; `make clippy`; `bash scripts/verification-harness.sh pr storage`; `bash scripts/verification-harness.sh pr engine`; `bash scripts/verification-harness.sh pr server` (sandbox bind denied locally; reran successfully with escalation); `bash scripts/verification-harness.sh pr runtime`; `make ci` (initial sandbox run hit a read-only advisory-db lock path; reran successfully with escalation) | archive the completed plan in `docs/plans/archive/` and leave future work to a new active control plane instead of this finished pass |
