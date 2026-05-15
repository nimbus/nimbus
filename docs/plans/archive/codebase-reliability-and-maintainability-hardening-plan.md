# Codebase Reliability And Maintainability Hardening Control Plan

This is the canonical execution control plane for the next repo-wide
maintainability, reliability-posture, canonical naming, and idiomatic-Rust
hardening wave after the completed hotspot maintainability plan archived at
`docs/plans/archive/codebase-modularity-and-maintainability-hotspots-plan.md`.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `AGENTS.md`
- `docs/reference/verification-architecture.md`
- `docs/plans/archive/codebase-modularity-and-maintainability-hotspots-plan.md`
- `docs/plans/archive/codebase-modularity-and-maintainability-follow-on-plan.md`
- `docs/plans/archive/codebase-modularity-and-maintainability-plan.md`
- `crates/nimbus-runtime/src/runtime/tests/support.rs`
- `crates/nimbus-runtime/src/runtime/tests/cooperative.rs`
- `crates/nimbus-engine/src/tests/provider_fixtures.rs`
- `crates/nimbus-engine/src/tests/mutation_journal/queued.rs`
- `crates/nimbus-engine/src/tests/postgres_provider.rs`
- `crates/nimbus-engine/src/tests/subscriptions.rs`
- `crates/nimbus-engine/src/tests/materialized_serving.rs`
- `crates/nimbus-storage/src/tests/sqlite_foundation.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime.rs`
- `crates/nimbus-storage/src/libsql.rs`
- `crates/nimbus-storage/src/postgres.rs`
- `crates/nimbus-storage/src/mysql.rs`
- the current git worktree on `main`
- local implementation references reviewed on 2026-04-19:
  - `/Users/jack/src/github.com/tigerbeetle/tigerbeetle/README.md`
  - `/Users/jack/src/github.com/tigerbeetle/tigerbeetle/docs/TIGER_STYLE.md`
  - `/Users/jack/src/github.com/cockroachdb/cockroach/docs/tech-notes/roachtest-investigation-tips/README.md`
  - `/Users/jack/src/github.com/tokio-rs/turmoil/README.md`
  - `/Users/jack/src/github.com/quickwit-oss/tantivy/ARCHITECTURE.md`

Baseline verification status for this plan:

- the predecessor hotspot maintainability wave completed and remains archived at
  `docs/plans/archive/codebase-modularity-and-maintainability-hotspots-plan.md`
- this hardening control plane is being authored as a docs-only review and
  planning pass on 2026-04-19 from a clean worktree after reviewing the live
  post-cleanup codebase against the current file-size, ownership, and
  reliability-posture gaps
- no new code verification is claimed by this planning pass
- every `RMH*` implementation item must record its own focused verification
  before it can be marked `done`

---

## Purpose

The previous maintainability waves succeeded. The largest mixed-owner
production roots were thinned, proof surfaces were repackaged, and
`ARCHITECTURE.md` was reduced to a justified stable architecture root.

The next wave should not simply resume broad file splitting. The live review
shows a different gap:

- only one active source document remains above the 1,500-line threshold, and
  it is already explicitly justified
- the more meaningful remaining risk is uneven reliability posture across proof
  surfaces, especially around async waits, CI-sensitive timing, and large test
  files that still mix too many scenario families
- recent runtime and engine flakes were fixed correctly by replacing brittle
  timing assumptions with semantic waits, but that discipline is not yet a
  single canonical pattern across the repo

This plan therefore targets the next highest-signal work:

- make semantic waits, bounded time budgets, and checker-style state
  assertions the default pattern for critical async tests
- continue selected proof-surface modularization where the files are still
  concept-mixed even though they are below the hard size threshold
- write down the reliability posture and failure-investigation discipline so
  future fixes do not regress into ad hoc timeout inflation or one-off helpers

TigerBeetle is the north-star reference for the engineering posture here:
assertions as design tools, explicit bounds, and clean invariants. CockroachDB,
Turmoil, and Tantivy provide supporting reference points for failure
investigation, deterministic hardship, and crisp Rust ownership boundaries.

---

## Relationship To Other Plans

- Use `docs/plans/README.md` as the owning plan index.
- Use this plan for broad maintainability, readability, reliability hardening,
  modularity, canonical naming, threshold review, and god-file cleanup work
  that is not already owned by another active plan.
- Use
  `docs/plans/archive/codebase-modularity-and-maintainability-hotspots-plan.md`
  only for the completed hotspot wave's execution record, closeout
  justifications, and architecture-doc packaging baseline.
- Use
  `docs/plans/archive/codebase-modularity-and-maintainability-follow-on-plan.md`
  only for the completed follow-on wave's execution record and benchmark
  packaging baseline.
- Use
  `docs/plans/archive/codebase-modularity-and-maintainability-plan.md`
  only for the predecessor CLI, provider, and sandbox ownership split history.
- This plan is separate from
  `docs/plans/encryption-at-rest-plan.md`,
  `docs/plans/websocket-protocol-plan.md`,
  `docs/plans/localhost-server-security-plan.md`,
  `docs/plans/archive/system-tenant-api-plan.md`,
  `docs/plans/desktop-ui-plan.md`,
  `docs/plans/install-script-plan.md`,
  `docs/plans/distribution-plan.md`,
  `docs/plans/windows-machine-support-plan.md`,
  `docs/plans/wasmtime-backend-plan.md`,
  `docs/plans/wasi-agent-capabilities-plan.md`,
  `docs/plans/archive/nimbus-rename-plan.md`,
  and `docs/plans/archive/nimbus-rename-satellite-repos-plan.md`.
- If work turns into product behavior changes, protocol changes, benchmark
  methodology changes, platform-specific machine behavior, install or
  distribution work, or provider-product semantics, stop and move to the
  owning plan instead of stretching this hardening plan across multiple
  streams.

---

## Scope

This plan covers:

- canonical reliability-proof primitives for async tests across runtime,
  engine, storage, and sandbox crates
- checker-style invariant helpers for flake-prone runtime, mutation-journal,
  and external-provider proof surfaces
- scenario-owned repackaging of selected mixed-owner proof files that remain
  hard to extend or debug even though they sit below the 1,500-line threshold
- extraction of inline proof surfaces from selected production roots when that
  clarifies production ownership without behavior change
- reliability posture docs and CI failure-investigation playbooks so the
  engineering discipline survives handoff and compaction
- doc, verification, and archive cleanup needed to keep this workstream
  resumable

This plan does not cover:

- new product features
- intentional CLI, route, wire, provider, or persistence behavior changes
  unless an item explicitly records them
- benchmark methodology changes that would make new results incomparable to the
  current suite
- install or distribution channel work
- rename work
- compatibility code for pre-launch behavior
- speculative rewrites that are not justified by ownership, reliability, or
  maintainability

---

## Cleanup Invariants

These rules are mandatory for every item in this plan.

1. Preserve externally observable behavior by default.
   Runtime semantics, mutation-journal semantics, provider behavior, storage
   atomicity, machine and sandbox behavior, and verification output shape stay
   unchanged unless a specific item explicitly records otherwise.

2. Keep the core architecture invariants intact.
   `nimbus-core` stays zero I/O.
   `nimbus-runtime` stays zero workspace dependencies.
   All mutations still flow through `Service::apply_mutation` or its queued
   async journal path.
   Storage atomicity stays unchanged.

3. Prefer semantic waits over incidental timing.
   Do not rely on poll-count assumptions, scheduler luck, or short fixed
   sleeps when the test can wait on an explicit state transition instead.

4. Put limits on everything.
   Time budgets, queue depths, retry loops, and wait helpers should have clear
   bounds and clear error messages. When a wait is intentionally unbounded at a
   lower layer, assert the condition at the layer that owns the lifecycle
   contract.

5. Prefer concept-owned modules over helper piles.
   A successful split makes ownership easier to name, test, and debug locally.

6. Treat file size as a signal, not the goal.
   Files under 1,500 lines are usually acceptable unless they still mix too
   many concepts.
   Files from 1,500 through 1,999 lines need an explicit justification if they
   remain unsplit.
   Files at 2,000 lines or above must not remain as single mixed-owner files at
   item closeout; extract production ownership, proof ownership, benchmark
   ownership, or reference-doc ownership until the file has a clear reason to
   stay large.

7. Keep recent flake fixes canonical, not bespoke.
   If a test fix reveals a more general wait-budget or invariant-helper seam,
   prefer landing the shared abstraction instead of leaving crate-local
   one-offs behind.

8. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

---

## Current Assessed State

- Before this planning pass, the repo again had no active generic cleanup or
  reliability-hardening control plane, so future broad maintainability work
  needed a new active owner rather than another revival of archived plans.
- The previous maintainability waves succeeded. The old CLI, provider, krun,
  Compose, machine API, mutation-journal, smoke-test, and benchmark hotspots
  were already split into thinner production or proof surfaces.
- The remaining active line-count hotspot is `ARCHITECTURE.md` at 1,694 lines.
  It remains explicitly justified as the canonical stable architecture root and
  is not selected for another broad split in this wave.
- The largest active Rust code files now sit below the hard threshold:
  - `crates/nimbus-storage/src/libsql.rs` at 1,471 lines
  - `crates/nimbus-storage/src/mysql.rs` at 1,445 lines
  - `crates/nimbus-engine/src/tests/materialized_serving.rs` at 1,389 lines
  - `crates/nimbus-engine/src/tests/postgres_provider.rs` at 1,358 lines
  - `crates/nimbus-sandbox/src/backends/container/runtime.rs` at 1,328 lines
  - `crates/nimbus-storage/src/tests/sqlite_foundation.rs` at 1,320 lines
  - `crates/nimbus-storage/src/postgres.rs` at 1,287 lines
  - `crates/nimbus-engine/src/tests/subscriptions.rs` at 1,202 lines
- The storage provider roots are already module-based composition surfaces:
  `libsql.rs`, `postgres.rs`, and `mysql.rs` each delegate into provider,
  read, write, and storage modules rather than remaining single mixed-owner
  god files.
- Recent CI failures exposed a different class of cleanup debt:
  - runtime cooperative tests relied on timing or poll-count assumptions that
    were replaced with CI-aware semantic waits
  - a mutation-journal queued test assumed the worker would already be idle
    instead of waiting for the idle state transition
  - an external Postgres provider proof lane used a timeout budget that was too
    tight for coverage-heavy CI despite not being a performance-SLO test
- Those fixes were directionally correct, but the helper surfaces are still
  uneven:
  runtime now has `ci_sensitive_duration(...)`,
  engine has `external_provider_test_timeout(...)`,
  and several proof families still own local wait logic instead of a clearer
  shared posture.
- Several under-threshold proof files still mix scenario families more than is
  ideal for long-term maintenance:
  `postgres_provider.rs`,
  `subscriptions.rs`,
  `materialized_serving.rs`,
  `sqlite_foundation.rs`,
  and the inline test module in
  `crates/nimbus-sandbox/src/backends/container/runtime.rs`.
- There is no dedicated repo-owned reliability posture or CI failure
  investigation reference that tells future contributors how to prefer
  invariants, semantic waits, deterministic hardship, and evidence-first
  debugging.

---

## Current Review Findings

1. The next enterprise-trust gap is reliability posture, not just file size.
   The codebase has already removed the biggest production god files; the
   higher-value remaining work is to make correctness proofs more explicit,
   bounded, and resilient.

2. Recent flake fixes should become canonical patterns.
   Replacing incidental timing with semantic waits improved correctness, but
   those improvements still live as local fixes instead of one deliberate
   repository pattern.

3. Several proof roots are below threshold but still concept-mixed.
   The engine and storage test files selected in this plan are not too large by
   line count alone, but they still group too many scenario families into one
   maintenance surface.

4. The storage provider roots are notable but not the best next targets.
   `libsql.rs`, `postgres.rs`, and `mysql.rs` are large because they still own
   important provider-local types and bridges, but they already delegate into
   submodules and read as clearer ownership surfaces than the selected proof
   files above.

5. Reliability docs are now the missing control surface.
   The repo has architecture and verification references, but it lacks a
   focused document that explains the intended discipline for invariants,
   bounded waits, seeded hardship, and CI failure investigation.

---

## Success Criteria

This plan is successful only when all of the following are true:

- selected async proof surfaces use semantic waits, bounded time budgets, and
  clearer invariant helpers instead of incidental timing assumptions
- the recent runtime and engine flake-fix patterns have been generalized into
  canonical helper surfaces where that materially improves consistency
- `postgres_provider.rs`, `subscriptions.rs`, `materialized_serving.rs`, and
  `sqlite_foundation.rs` no longer keep broad scenario coverage in giant flat
  files
- selected inline tests are extracted from production roots where that clarifies
  production ownership without behavior change
- the repo has a focused reliability posture reference and a CI
  failure-investigation playbook
- every remaining active file above 1,500 lines has an explicit justification
  in the plan closeout notes, and no selected active file remains above 2,000
  lines for avoidable packaging reasons
- docs, plan status, and archive state accurately reflect the landed work

---

## Assessed But Not Selected

- `crates/nimbus-storage/src/libsql.rs` at 1,471 lines,
  `crates/nimbus-storage/src/mysql.rs` at 1,445 lines, and
  `crates/nimbus-storage/src/postgres.rs` at 1,287 lines are notable but
  already module-based provider roots. Revisit only if later work pushes new
  unrelated ownership into those roots.
- `ARCHITECTURE.md` at 1,694 lines remains above the review threshold, but the
  prior maintainability wave already extracted deeper reference detail and
  explicitly justified the remaining stable architecture root. Do not reopen it
  without a materially better reason than raw length.
- `docs/plans/windows-machine-support-plan.md` at 1,322 lines is a plan doc,
  not active production code, and is out of scope for this wave.
- Archived plan documents above the threshold are historical control-plane
  records and are out of scope. Do not rewrite archived plans just to reduce
  their line counts.

---

## Feature Preservation Matrix

| Surface | Preservation Requirement |
| --- | --- |
| Runtime proof surfaces | cooperative locker, timeout, cancellation, warm-pool, and bundle-integrity semantics stay unchanged while waits and helper placement improve |
| Mutation journal proofs | queued admission, drain, idle, durable-head, applied-head, and response-resolution semantics stay unchanged |
| External provider proofs | Postgres tenant lifecycle, direct CRUD, scheduler, journal, schema, and reopen semantics stay unchanged |
| Engine proof surfaces | subscriptions and materialized-serving semantics stay unchanged while scenario ownership becomes clearer |
| Storage proofs | SQLite WAL, cancellation, schema persistence, index rebuild, scheduler, cron, and journal semantics stay unchanged |
| Sandbox container backend | container launch, inspect, cleanup, networking, and inline test semantics stay unchanged if tests move out of the production root |
| Reliability docs | new posture and playbook docs stay aligned with the actual code and verification architecture; they must not conflict with `ARCHITECTURE.md` or `docs/reference/verification-architecture.md` |

---

## Control Plan Rules

1. Implement exactly one `RMH*` item at a time unless the plan explicitly says
   otherwise.
2. Do not skip ahead while an earlier eligible item remains `todo`.
3. Do not start a new item while another remains `in_progress`.
4. If the worktree is dirty, reconcile it to the owning item before proceeding.
5. Prefer scenario-owned proof modules over one giant test file.
6. Prefer shared semantic wait helpers over duplicated crate-local timeout
   guesses when the contract is genuinely shared.
7. If implementation reveals a materially better seam map, update this plan
   first, then implement it.
8. After every meaningful work burst, update the roadmap ledger, checkpoints,
   and execution log so handoff does not depend on chat memory.

---

## Verification Contract

Every implementation item in this plan must run its focused verification, plus:

- `cargo fmt --all --check`
- `cargo check --workspace`

Use these focused lanes as appropriate:

- `cargo test -p nimbus-runtime`
- `cargo test -p nimbus-engine mutation_journal`
- `cargo test -p nimbus-engine postgres_provider`
- `cargo test -p nimbus-engine subscriptions`
- `cargo test -p nimbus-engine materialized_serving`
- `cargo test -p nimbus-storage sqlite_foundation`
- `cargo test -p nimbus-sandbox`
- `cargo clippy -p nimbus-runtime --all-targets -- -D warnings`
- `cargo clippy -p nimbus-engine --all-targets -- -D warnings`
- `cargo clippy -p nimbus-storage --all-targets -- -D warnings`
- `cargo clippy -p nimbus-sandbox --all-targets -- -D warnings`

Before this whole workstream can be considered complete, run and record:

- `make check`
- `make test`
- `make clippy`
- `npm run test --workspaces --if-present`
- `npm run build --workspaces --if-present`
- `make ci` if practical

If environment restrictions block a command, do not silently skip it:

- run the best available focused verification
- retry with escalation when appropriate
- record the limitation in the execution log if it still cannot run

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| RMH0 | `done` | reviewed the live codebase, the archived maintainability waves, the recent CI flake-fix surfaces, and local reliability-oriented implementation references; promoted this hardening control plane as the new active owner for broad maintainability and enterprise-trust work | none | docs-only review and planning pass on 2026-04-19 |
| RMH1 | `done` | established the canonical reliability-proof primitive posture by extending `nimbus-testing` with shared CI-aware timing helpers, improving eventual-assertion timeout diagnostics, and mirroring the same timing-helper contract in runtime-local test support without violating the zero-workspace-dependency invariant | RMH0 | completed on 2026-04-19 with aligned `ci_or_local_duration`, env-parsed timing helpers, and verification-architecture ownership updates |
| RMH2 | `done` | hardened the flake-prone runtime cooperative and engine mutation-journal or external-provider proof surfaces around explicit state-transition waits, clearer bounded-time helpers, and checker-style invariants | RMH1 | completed on 2026-04-19 after normalizing the recent flake fixes into canonical helper-backed proof contracts and rerunning the focused runtime, engine, workspace-check, and clippy lanes |
| RMH3 | `done` | split `crates/nimbus-engine/src/tests/postgres_provider.rs` into scenario-owned provider regression modules with a thin root and local fixture support seam | RMH1, RMH2 | completed on 2026-04-19 after regrouping the provider proofs into lifecycle, CRUD, journal, and scheduler modules while preserving external-provider semantics |
| RMH4 | `done` | split `crates/nimbus-engine/src/tests/subscriptions.rs` and `materialized_serving.rs` into scenario-owned engine proof modules | RMH1, RMH2 | completed on 2026-04-19 after extracting behavior-owned module trees for cache, filtering, lifecycle, journal, retention, concurrency, reuse, and eviction semantics |
| RMH5 | `done` | split `crates/nimbus-storage/src/tests/sqlite_foundation.rs` into scenario-owned storage proof modules with a small local support seam | RMH1, RMH2 | completed on 2026-04-19 after regrouping WAL, cancellation, schema, journal, scheduler, and snapshot proofs without changing SQLite foundation semantics |
| RMH6 | `done` | extracted the inline tests from `crates/nimbus-sandbox/src/backends/container/runtime.rs` into a sibling proof surface with planning, lifecycle, and support ownership | RMH1 | completed on 2026-04-19 after moving the container runtime proof surface beside the root without changing backend behavior |
| RMH7 | `done` | wrote the reliability posture and CI failure-investigation reference docs, then reconciled indexes and architecture cross-links | RMH1 through RMH6 | completed on 2026-04-19 after landing stable reference docs and wiring them through `ARCHITECTURE.md`, `docs/README.md`, `docs/reference/verification-architecture.md`, `docs/plans/README.md`, and `AGENTS.md` |
| RMH8 | `done` | ran the full verification sweep, confirmed that no active `.rs` or `.md` files remain above 1,500 lines, and archived this completed control plane cleanly after flipping repo entrypoints to the stable reliability references plus archived historical context | RMH1 through RMH7 | completed on 2026-04-19 after green `make check` / `make test` / `make clippy` / `npm run test --workspaces --if-present` / `npm run build --workspaces --if-present` / `make ci`, plus a final post-archive `cargo fmt --all --check` and `cargo check --workspace` sweep |

---

## Dependency Graph

- `RMH1` is the recommended first slice because the next wave should settle the
  helper and wait-budget posture before it repackages the selected proof files.
- `RMH2` should follow `RMH1` so the recent runtime and engine flake fixes are
  normalized into explicit invariant patterns early in the wave.
- `RMH3`, `RMH4`, and `RMH5` should follow the helper posture work so the new
  module trees all use the same semantic-wait and bounded-budget discipline.
- `RMH6` can follow once the helper posture is settled; its main purpose is to
  keep the container production root thinner and align proof placement with the
  rest of the repo.
- `RMH7` should follow the code cleanup items so the docs describe the landed
  reliability posture rather than an intermediate state.
- `RMH8` closes the workstream after the selected proof, helper, and docs
  items land.

---

## Recommended Delivery Order

1. `RMH1` — canonical reliability-proof primitives
2. `RMH2` — runtime and engine invariant hardening
3. `RMH3` — Postgres provider proof packaging
4. `RMH4` — subscriptions and materialized-serving proof packaging
5. `RMH5` — SQLite foundation proof packaging
6. `RMH6` — container runtime inline-test extraction
7. `RMH7` — reliability posture docs and investigation playbook
8. `RMH8` — verification and archive closeout

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| RMH0 | done | start `RMH1` by inventorying the existing wait-budget, eventual-assertion, fault-gate, and CI-duration helpers across runtime, engine, storage, sandbox, and `nimbus-testing`, then choose the smallest canonical shared seam that improves consistency without forcing artificial coupling |
| RMH1 | done | extended `nimbus-testing` with `ci_or_local_duration`, `duration_ms_env_or`, and `usize_env_or`, improved eventual-assertion timeout diagnostics, aligned runtime-local timing helper names with the shared contract, replaced the engine’s bespoke external-provider timeout helper with the canonical shared helper, and updated `docs/reference/verification-architecture.md` so the landed ownership map matches the code. |
| RMH2 | done | normalized the selected runtime cooperative and engine mutation-journal or external-provider proof surfaces onto explicit helper-backed state-transition waits, clearer bounded future helpers, and locally testable invariant checks; the focused runtime, engine, workspace-check, and clippy lanes all passed after the hardening changes landed. |
| RMH3 | done | replaced the monolithic `crates/nimbus-engine/src/tests/postgres_provider.rs` proof surface with a thin root plus local `lifecycle`, `crud`, `journal`, `scheduler`, and `support` modules, preserving the existing scenarios while making provider fixture ownership and scenario boundaries explicit. |
| RMH4 | done | replaced the monolithic `subscriptions.rs` and `materialized_serving.rs` proof surfaces with thin roots plus behavior-owned submodules so cache, filtering, lifecycle, journal, retention, concurrency, reuse, and eviction semantics each live beside the scenarios that prove them. |
| RMH5 | done | replaced the monolithic `crates/nimbus-storage/src/tests/sqlite_foundation.rs` proof surface with a thin root plus `foundation`, `cancellation`, `schema`, `journal`, `scheduler`, `snapshot`, and `support` modules so each storage invariant lives beside the scenarios that prove it. |
| RMH6 | done | moved the inline container-runtime proof block into a sibling test tree so the production root keeps launch and cleanup behavior ownership while the new local proof files own planning and lifecycle scenarios. |
| RMH7 | done | landed stable repo-owned references for reliability posture and CI failure investigation, then reconciled all architecture and plan indexes so those docs are discoverable without replacing the deeper verification topology reference. |
| RMH8 | done | closed out the workstream with a full green verification sweep, an active-tree size audit that found no remaining `.rs` or `.md` files above 1,500 lines, and an archive-state doc reconciliation that moved this control plane into `docs/plans/archive/` while flipping `AGENTS.md` and `docs/plans/README.md` to the stable reliability references plus historical-plan guidance. |

---

## Work Items

### RMH0. Baseline review and hardening plan promotion

#### Outcome

- Completed during this planning pass.

### RMH1. Establish canonical reliability-proof primitives

#### Implementation plan

1. Inventory the existing helper seams for waits, eventual assertions,
   CI-sensitive time budgets, stress env parsing, and deterministic fault gates
   across runtime, engine, storage, sandbox, and `nimbus-testing`.
2. Consolidate the helpers that are genuinely shared into a clearer canonical
   surface without introducing unnecessary cross-crate coupling.
3. Expected seams:
   - semantic wait or eventual-assertion helpers
   - CI-aware time-budget selection helpers
   - deterministic stress-env parsing helpers
   - shared fault-gate or pause-handle helpers where the contract is the same
   - clearer error messages for time-budget failures

#### Focused verification

- `cargo test -p nimbus-runtime`
- `cargo test -p nimbus-engine mutation_journal`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-runtime --all-targets -- -D warnings`
- `cargo clippy -p nimbus-engine --all-targets -- -D warnings`

#### Acceptance criteria

- selected proof families no longer rely on scattered bespoke wait-budget
  helpers when a clearer shared seam exists
- the resulting helper surface keeps ownership explicit instead of creating a
  generic helper pile
- no externally observable behavior changes

### RMH2. Harden flake-prone runtime and engine proof surfaces around explicit invariants

#### Implementation plan

1. Revisit the recent runtime cooperative, mutation-journal, and external
   provider proof fixes and normalize them into explicit state-based waits.
2. Add checker-style helpers where they make the contract easier to read and
   debug.
3. Expected surfaces:
   - `crates/nimbus-runtime/src/runtime/tests/cooperative.rs`
   - `crates/nimbus-runtime/src/runtime/tests/support.rs`
   - `crates/nimbus-engine/src/tests/mutation_journal/queued.rs`
   - `crates/nimbus-engine/src/tests/provider_fixtures.rs`
   - any immediately adjacent support modules needed to keep the invariants
     local and testable

#### Focused verification

- `cargo test -p nimbus-runtime`
- `cargo test -p nimbus-engine mutation_journal`
- `cargo test -p nimbus-engine postgres_provider`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-runtime --all-targets -- -D warnings`
- `cargo clippy -p nimbus-engine --all-targets -- -D warnings`

#### Acceptance criteria

- no selected proof waits on incidental poll counts or luck when an explicit
  state transition is available
- bounded time budgets remain explicit and CI-aware where needed
- failure messages point at the violated state contract, not just a timeout

### RMH3. Split `postgres_provider.rs` into scenario-owned provider proof modules

#### Implementation plan

1. Keep the provider production surfaces unchanged.
2. Replace the flat
   `crates/nimbus-engine/src/tests/postgres_provider.rs` proof surface with a
   local module tree grouped by provider scenario ownership.
3. Expected seams:
   - tenant lifecycle and reopen behavior
   - direct CRUD and query behavior
   - scheduler and cron behavior
   - durable journal and snapshot behavior
   - external fixture and timeout support helpers

#### Focused verification

- `cargo test -p nimbus-engine postgres_provider`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-engine --all-targets -- -D warnings`

#### Acceptance criteria

- the Postgres provider proof surface is easier to navigate by scenario family
- no selected file remains above the hard 2,000-line threshold
- external-provider semantics remain unchanged

### RMH4. Split `subscriptions.rs` and `materialized_serving.rs` into scenario-owned engine proof modules

#### Implementation plan

1. Keep the engine production surfaces unchanged.
2. Replace the flat proof surfaces
   `crates/nimbus-engine/src/tests/subscriptions.rs`
   and
   `crates/nimbus-engine/src/tests/materialized_serving.rs`
   with local module trees grouped by reactive and serving scenario ownership.
3. Expected seams:
   - cache behavior and invalidation
   - filtered and limited subscription behavior
   - mutation-driven re-evaluation behavior
   - materialized-read serving, replay, and freshness behavior
   - local support helpers

#### Focused verification

- `cargo test -p nimbus-engine subscriptions`
- `cargo test -p nimbus-engine materialized_serving`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-engine --all-targets -- -D warnings`

#### Acceptance criteria

- both proof surfaces are easier to navigate by scenario family
- file ownership is clearer without changing behavior
- no selected engine proof file remains above the hard 2,000-line threshold

### RMH5. Split `sqlite_foundation.rs` into scenario-owned storage proof modules

#### Implementation plan

1. Keep the storage production surfaces unchanged.
2. Replace the flat
   `crates/nimbus-storage/src/tests/sqlite_foundation.rs`
   proof surface with a local module tree grouped by storage scenario
   ownership.
3. Expected seams:
   - WAL and foundation behavior
   - cancellation and queueing behavior
   - schema and index persistence behavior
   - scheduler and cron behavior
   - journal and snapshot behavior
   - local support helpers

#### Focused verification

- `cargo test -p nimbus-storage sqlite_foundation`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-storage --all-targets -- -D warnings`

#### Acceptance criteria

- the SQLite foundation proof surface is easier to navigate by scenario family
- storage foundation semantics remain unchanged
- no selected storage proof file remains above the hard 2,000-line threshold

### RMH6. Extract inline tests from `container/runtime.rs`

#### Implementation plan

1. Keep `crates/nimbus-sandbox/src/backends/container/runtime.rs` as the
   production root for container backend behavior.
2. Move the inline `mod tests` proof surface into a sibling module tree when
   that makes the production root easier to own and the proof surface easier to
   navigate.
3. Expected seams:
   - production launch, inspect, and cleanup ownership stay in the root
   - proof helpers and scenarios move beside the root in a clearer local test
     tree

#### Focused verification

- `cargo test -p nimbus-sandbox`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-sandbox --all-targets -- -D warnings`

#### Acceptance criteria

- the production root owns production behavior rather than inline test detail
- container backend behavior remains unchanged
- proof placement aligns better with the rest of the repo

### RMH7. Write reliability posture and CI failure-investigation reference docs

#### Implementation plan

1. Add a focused reliability posture reference under `docs/reference/` that
   explains the intended discipline for assertions, semantic waits, bounded
   time budgets, deterministic hardship, and when to centralize helpers.
2. Add a CI failure-investigation playbook that explains how to gather
   artifacts, build a timeline, correlate events, inspect code ownership, and
   avoid cargo-cult timeout increases.
3. Update cross-links, `docs/plans/README.md`, and `AGENTS.md` so the new docs
   are discoverable and clearly subordinate to the stable architecture and
   verification references.

#### Focused verification

- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- the repo has a focused reliability posture reference and an evidence-first CI
  investigation playbook
- the new docs do not conflict with `ARCHITECTURE.md` or
  `docs/reference/verification-architecture.md`
- broad maintainability guidance now points at the correct active plan

### RMH8. Docs, verification, and archive closeout

#### Implementation plan

1. Update `AGENTS.md`, `docs/plans/README.md`, and any touched doc indexes so
   the landed ownership map is discoverable.
2. Run the full verification sweep required by this plan.
3. Record explicit closeout justifications for any remaining active file above
   1,500 lines.
4. Archive this control plane once all items are complete and future broad
   maintainability work needs a newly promoted plan.

#### Focused verification

- `make check`
- `make test`
- `make clippy`
- `npm run test --workspaces --if-present`
- `npm run build --workspaces --if-present`
- `make ci` if practical

#### Acceptance criteria

- the docs reflect the landed ownership map and reliability posture
- the full verification sweep is recorded
- every remaining active file above 1,500 lines has an explicit justification
- this plan can move to `docs/plans/archive/` cleanly

---

## Execution Log

| Date | Item | Status | Notes | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-19 | RMH0 | `done` | Reviewed the live repo after the completed hotspot maintainability wave and promoted this reliability-and-maintainability hardening plan as the new active control plane. The review confirmed that the main remaining gap is not another round of broad production-root splitting: it is the uneven reliability posture across async proof surfaces, plus several under-threshold but still concept-mixed engine and storage regression files. The planning pass also reviewed local implementation references from TigerBeetle, CockroachDB, Turmoil, and Tantivy to ground the next wave in explicit invariants, bounded waits, deterministic hardship, and crisp ownership boundaries. | docs-only review; no new code verification claimed | start `RMH1` by inventorying the existing wait-budget, eventual-assertion, and fault-gate helpers across runtime, engine, storage, sandbox, and `nimbus-testing` |
| 2026-04-19 | RMH1 | `in_progress` | Reconciled the docs-only RMH0 baseline and completed the helper-seam inventory across runtime, engine, storage, sandbox, `nimbus-testing`, and `docs/reference/verification-architecture.md`. The inventory confirmed that `nimbus-testing` already owns the canonical shared eventual and fault-gate helpers for engine-side proof surfaces, while runtime must remain self-contained because of the zero-workspace-dependency invariant. The selected direction is therefore to extend `nimbus-testing` with clearer timing-budget helpers and improved eventual-assertion diagnostics, then mirror the same contract in runtime-local test support instead of forcing artificial cross-crate coupling. | plan reconciliation; targeted `rg` and `sed` reads over runtime/engine/storage/sandbox helper surfaces; no code verification yet | land the helper APIs, migrate the selected runtime and engine proof families to them, and then run the RMH1 focused verification lanes |
| 2026-04-19 | RMH1 | `done` | Landed the helper posture selected during the inventory. `nimbus-testing` now owns the shared CI-aware timing-budget helpers (`ci_or_local_duration`, `duration_ms_env_or`, `usize_env_or`) alongside improved eventual-assertion timeout diagnostics, while `nimbus-runtime` mirrors the same timing-helper contract locally inside `runtime/tests/support.rs` so runtime stays free of workspace dependencies. Engine tests stopped using the bespoke external-provider timeout helper and instead use the canonical shared helper, and `docs/reference/verification-architecture.md` now reflects the landed helper ownership. | `cargo fmt --all --check`; `cargo test -p nimbus-runtime`; `cargo test -p nimbus-engine mutation_journal`; `cargo check --workspace`; `cargo clippy -p nimbus-runtime --all-targets -- -D warnings`; `cargo clippy -p nimbus-engine --all-targets -- -D warnings` | start `RMH2` by revisiting the recent runtime cooperative and engine journal/provider flake-fix surfaces and normalizing them into explicit state-transition waits plus clearer invariant helpers |
| 2026-04-19 | RMH2 | `in_progress` | Began the next eligible hardening slice immediately after closing RMH1. The target is the recent flake-fix surface itself: `runtime/tests/cooperative.rs`, `runtime/tests/support.rs`, `tests/mutation_journal/queued.rs`, and `tests/provider_fixtures.rs`, with adjacent support code included only where needed to make the state contracts explicit and locally testable. | RMH1 verification complete; upcoming targeted reads over the selected runtime and engine proof surfaces | replace incidental timing or loosely-described waits with explicit state-transition waits and checker-style helpers, then run the RMH2 focused verification lanes |
| 2026-04-19 | RMH2 | `done` | Hardened the selected runtime and engine proof surfaces around explicit contracts instead of ad hoc waits. Runtime-local support now exposes reusable semantic wait helpers without violating the zero-workspace-dependency invariant; cooperative runtime proofs use named state-transition waits where that genuinely clarifies the contract; engine mutation-journal proofs now share named bounded-wait helpers for pause, pending, and catch-up expectations; and external-provider proofs use a canonical bounded-future helper that reports clearer timeout failures. | `cargo fmt --all --check`; `cargo test -p nimbus-runtime`; `cargo test -p nimbus-engine mutation_journal`; `cargo test -p nimbus-engine postgres_provider`; `cargo check --workspace`; `cargo clippy -p nimbus-runtime --all-targets -- -D warnings`; `cargo clippy -p nimbus-engine --all-targets -- -D warnings` | start `RMH3` by extracting `postgres_provider.rs` into scenario-owned modules grouped around tenant lifecycle, direct CRUD, scheduler, durable journal, and notification or reopen behavior |
| 2026-04-19 | RMH3 | `in_progress` | Began the next eligible proof-packaging slice immediately after RMH2 verification. `crates/nimbus-engine/src/tests/postgres_provider.rs` is below the hard size threshold, so this item is about canonical ownership rather than emergency splitting: the next step is to map its mixed scenario families into lifecycle, CRUD, scheduler, journal, and notification-owned modules plus a small local support seam. | RMH2 verification complete; upcoming targeted reads over the current Postgres provider proof file and adjacent helpers | extract the local provider-proof module tree, preserve existing external-provider semantics, and rerun the focused Postgres-provider and engine verification lanes |
| 2026-04-19 | RMH3 | `done` | Repackaged the Postgres provider proof surface into a thin `postgres_provider.rs` root plus local `lifecycle`, `crud`, `journal`, `scheduler`, and `support` modules. The extracted `support.rs` now owns container/fixture setup, metadata-schema cleanup, backend-activity helpers, and shared table-schema utilities, while the scenario files keep tenant lifecycle, direct CRUD, journal synchronization, and scheduler recovery behavior close to the tests that prove them. | `cargo fmt --all --check`; `cargo test -p nimbus-engine postgres_provider`; `cargo check --workspace`; `cargo clippy -p nimbus-engine --all-targets -- -D warnings` | start `RMH4` by inventorying the mixed scenario families in `subscriptions.rs` and `materialized_serving.rs`, then extract the smallest local support seams that clarify reactive-query and materialized-serving ownership |
| 2026-04-19 | RMH4 | `in_progress` | Began the next eligible engine-proof packaging slice immediately after RMH3 verification. The target is the remaining concept-mixed engine proof surface in `subscriptions.rs` and `materialized_serving.rs`; the next step is to map cache, invalidation, filtering, replay, and freshness scenarios so the extracted module tree follows behavior ownership rather than arbitrary line-count slicing. | RMH3 verification complete; upcoming targeted reads over the current subscription and materialized-serving proof files | extract the scenario-owned engine proof modules, preserve reactive-query and serving semantics, and rerun the focused engine verification lanes |
| 2026-04-19 | RMH4 | `done` | Repackaged the remaining concept-mixed engine proof roots into thinner composition files plus behavior-owned module trees. `subscriptions.rs` now separates basic delivery, cache and invalidation, filter semantics, journal behavior, and tenant-lifecycle teardown proofs; `materialized_serving.rs` now separates reuse, retention, concurrency, and eviction semantics while leaving the underlying assertions intact. | `cargo fmt --all --check`; `cargo test -p nimbus-engine subscriptions`; `cargo test -p nimbus-engine materialized_serving`; `cargo check --workspace`; `cargo clippy -p nimbus-engine --all-targets -- -D warnings` | start `RMH5` by inventorying `sqlite_foundation.rs`, grouping WAL/cancellation/schema/scheduler/journal scenarios, and extracting the smallest local storage-proof support seam needed to keep those invariants readable |
| 2026-04-19 | RMH5 | `in_progress` | Began the next eligible storage-proof packaging slice immediately after RMH4 verification. The target is `crates/nimbus-storage/src/tests/sqlite_foundation.rs`; the next step is to map its WAL, cancellation, schema/index, scheduler, cron, and journal scenarios so the extracted module tree follows storage-behavior ownership instead of arbitrary line-count slicing. | RMH4 verification complete; upcoming targeted reads over the current SQLite foundation proof file and adjacent helpers | extract the scenario-owned storage proof modules, preserve SQLite foundation semantics, and rerun the focused storage verification lanes |
| 2026-04-19 | RMH5 | `done` | Repackaged the SQLite foundation proof surface into a thin `sqlite_foundation.rs` root plus local `foundation`, `cancellation`, `schema`, `journal`, `scheduler`, `snapshot`, and `support` modules. The extracted `support.rs` now owns the shared query-plan helper plus the small scheduler and snapshot fixture builders, while the scenario files keep WAL, cancellation, schema persistence, durable journal, scheduler, and rebuild semantics close to the tests that prove them. | `cargo fmt --all --check`; `cargo test -p nimbus-storage sqlite_foundation`; `cargo check --workspace`; `cargo clippy -p nimbus-storage --all-targets -- -D warnings` | start `RMH6` by inventorying the inline tests in `crates/nimbus-sandbox/src/backends/container/runtime.rs` and extracting the smallest sibling proof tree that leaves container runtime behavior untouched |
| 2026-04-19 | RMH6 | `in_progress` | Began the next eligible sandbox-proof extraction slice immediately after RMH5 verification. The target is the inline test surface inside `crates/nimbus-sandbox/src/backends/container/runtime.rs`; the next step is to map those scenarios into a sibling proof tree so the production root stays focused on container runtime behavior. | RMH5 verification complete; upcoming targeted reads over the current container runtime root and adjacent sandbox proof helpers | extract the sibling sandbox proof modules, preserve container launch and cleanup semantics, and rerun the focused sandbox verification lanes |
| 2026-04-19 | RMH6 | `done` | Extracted the inline `runtime.rs` container-backend tests into a sibling proof surface rooted at `runtime/tests.rs` with local `planning`, `lifecycle`, and `support` ownership. The production root now owns only container runtime behavior, while the sibling proof tree keeps the plan-only launch, status, and cleanup drills close to the supporting sample-spec helpers. | `cargo fmt --all --check`; `cargo test -p nimbus-sandbox`; `cargo check --workspace`; `cargo clippy -p nimbus-sandbox --all-targets -- -D warnings` | start `RMH7` by landing the stable reliability posture and CI investigation references, then wire them through the docs indexes and architecture references |
| 2026-04-19 | RMH7 | `done` | Landed `docs/reference/reliability-posture.md` and `docs/reference/ci-failure-investigation.md`, then updated `docs/README.md`, `ARCHITECTURE.md`, `docs/reference/verification-architecture.md`, `docs/plans/README.md`, and `AGENTS.md` so the new docs are discoverable and clearly subordinate to the stable architecture plus verification references. | `cargo fmt --all --check`; `cargo check --workspace` | start `RMH8` by running the full verification sweep, recording closeout justifications for any remaining >1,500-line active files, and archiving the completed plan cleanly |
| 2026-04-19 | RMH8 | `in_progress` | Began the closeout slice immediately after RMH7 verification. The remaining work is the full verification sweep, explicit near-threshold file justification, and archive-state documentation cleanup so the repo no longer points at this plan as an active control plane once it moves into `docs/plans/archive/`. | RMH7 verification complete; upcoming full verification sweep and closeout doc reconciliation | run the required workspace verification, record remaining >1,500-line file justifications, archive the plan, and update active-plan entrypoints accordingly |
| 2026-04-19 | RMH8 | `done` | Closed out the workstream by running the full repo verification sweep, auditing active `.rs` and `.md` file sizes, and confirming that no active file remains above 1,500 lines or near the 2,000-line hard threshold for avoidable packaging debt. Archived this plan to `docs/plans/archive/` and updated `AGENTS.md` plus `docs/plans/README.md` so future broad reliability or maintainability work starts from `docs/reference/reliability-posture.md` and `docs/reference/ci-failure-investigation.md`, using this archived plan only for historical execution detail and closeout evidence. | `make check`; `make test`; `make clippy`; `npm run test --workspaces --if-present`; `npm run build --workspaces --if-present`; `make ci`; `cargo fmt --all --check`; `cargo check --workspace` | workstream complete; promote a new active plan before the next repo-wide reliability or maintainability wave |
