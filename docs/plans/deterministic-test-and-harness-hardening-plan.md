# Deterministic Test And Harness Hardening Control Plan

This is the canonical execution control plane for the next verification and
test-architecture hardening pass.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `docs/research/tigerbeetle-code-reference.md`
- `docs/plans/archive/verification-harness-plan.md`
- `.github/workflows/ci.yml`
- `crates/neovex-runtime/src/runtime/facade.rs`
- `crates/neovex-runtime/src/limits.rs`
- `crates/neovex-runtime/src/executor.rs`
- `crates/neovex-runtime/src/runtime/tests/cooperative.rs`
- `crates/neovex-runtime/src/runtime/tests/bundle_integrity.rs`
- `crates/neovex-server/src/adapters/convex/registry/loading.rs`
- `crates/neovex-server/src/tests/auth/websocket_auth.rs`
- `crates/neovex-server/src/tests/convex_runtime/http_routes/demo_flow/helpers.rs`
- `crates/neovex-server/src/tests/scheduling/convex_scheduling/execution/public_schedule_after.rs`
- `crates/neovex-server/src/tests/scheduling/cron_and_history.rs`
- `crates/neovex-storage/src/simulation.rs`
- `crates/neovex-storage/src/simulation/tests.rs`
- `crates/neovex-test-support/src/lib.rs`
- `crates/neovex-test-support/src/blocking_fault_injector.rs`
- `crates/neovex-test-support/src/service_fixture.rs`
- `crates/neovex-test-support/src/websocket_fixture.rs`

Baseline verification status for this plan:

- the current live worktree already contains the completed targeted-domain
  modularity refactor and its archive handoff, but that work has not yet been
  recommitted as a clean baseline
- this control plane is being authored as a docs-only review-and-planning pass
  on 2026-04-08 against the live worktree
- no new code verification is claimed by this planning pass
- every `TH*` implementation item must record its own focused verification
  before it can be marked `done`

---

## Purpose

Neovex already has strong correctness infrastructure:

- deterministic storage simulation seams
- named verification-harness seeds and repro commands
- broad storage, engine, server, and runtime test coverage
- PR and nightly verification-harness CI lanes

What it does not yet have is one coherent, TigerBeetle-style testing
architecture.

The current gaps are architectural, not numerical:

- some tests still depend on moving runtime defaults instead of explicit test
  profiles
- some V8-sensitive tests already document that they require subprocess
  isolation, but the harness does not actually enforce that
- some server and runtime tests still poll with wall-clock sleeps instead of
  deterministic waiters or explicit simulation seams
- CI currently works around part of the runtime isolation problem by running the
  whole runtime crate single-threaded, while coverage still exercises a
  different topology
- there is still too much pressure to "fix tests" by changing product defaults
  or generic fixture defaults instead of hardening the harness

This plan exists to make the test and harness architecture itself explicit,
idiomatic, deterministic, and trustworthy enough for enterprise-critical
software.

---

## Reference Posture

Use the following systems as style and verification references for this plan:

- TigerBeetle for harsh deterministic replay, crash, recovery, and liveness or
  safety campaigns over real logic
- CockroachDB for randomized workload generation, consistency checking, and the
  habit of independently verifying derived or observed state instead of trusting
  a single execution path
- FoundationDB for deterministic simulation seams, seeded replay, and
  architecture-level fault injection

What to borrow:

- explicit reproducibility
- adversarial workload generation
- consistency oracles
- independent state verification
- failure-heavy rather than example-heavy test posture

What not to copy literally:

- TigerBeetle's single-threaded product execution model
- CockroachDB's distributed topology and raft-specific machinery
- FoundationDB's exact simulator structure

Neovex should borrow the verification attitude from those systems while keeping
its own redb-backed, runtime-hosting, reactive-database architecture.

---

## Relationship To Other Plans

- Use `docs/plans/README.md` as the owning plan index.
- This plan is separate from:
  `docs/plans/v8-locker-fork-plan.md`,
  `docs/plans/convex-demos-compatibility-plan.md`,
  `docs/plans/encryption-at-rest-plan.md`,
  `docs/plans/layered-admission-control-plan.md`,
  `docs/plans/pluggable-storage-backend-plan.md`,
  and `docs/plans/wasmtime-backend-plan.md`.
- `docs/plans/archive/verification-harness-plan.md` is useful historical
  context, but this plan stands on the current codebase, current CI topology,
  and current test-harness hotspots.
- If work turns into Locker-fork runtime feature development, Convex
  compatibility feature work, encryption-at-rest implementation, admission
  control, pluggable storage backends, or Wasmtime backend work, stop and move
  to the owning plan instead of stretching this hardening pass across multiple
  streams.

---

## Scope

This plan covers:

- explicit test execution profiles and runtime-policy ownership
- real isolation for V8-sensitive and cooperative runtime tests
- deterministic waiter and eventual-assertion helpers in place of sleep-based
  polling where the code already exposes better seams
- promotion of `neovex-test-support` and related local test-support modules into
  canonical harness ownership surfaces
- reproducible liveness and safety campaigns for the currently shaky runtime and
  transport flows
- CI and coverage topology that reflects the harness architecture instead of
  hiding it
- documentation and archive closure for this pass

This plan does not cover:

- new product features
- intentional route, wire, or public API behavior changes unless explicitly
  recorded
- blanket "run everything single-threaded" as a permanent correctness answer
- changing product defaults just to make tests pass
- a wholesale replacement of Rust's default test runner
- a broad JS testing rewrite outside the harness surfaces touched here

---

## Test Hardening Invariants

These rules are mandatory for every item in this plan.

1. Preserve product behavior by default.
   Runtime invocation, timeout, cancellation, pooling, subscription, auth,
   scheduling, and recovery semantics stay unchanged unless a specific item
   explicitly records otherwise.

2. Do not change product defaults to placate tests.
   If a scenario needs `RunToCompletion`, `StartupSnapshotCache`, `WarmPool`,
   custom tenant limits, or a special scheduler topology, the test or test
   harness must state that explicitly.

3. Treat CI serialization as containment, not completion.
   A crate-wide `--test-threads=1` workaround is not the target architecture.
   If temporary serialization is still needed, record why and narrow it to the
   smallest honest scope.

4. Prefer deterministic signals, clocks, and gates over wall-clock sleeps.
   Use `Notify`, manual clocks, seeded harness events, fault gates, or explicit
   activity signals wherever the system already exposes them.

5. Keep repro metadata stable and explicit.
   Failing generated or adversarial tests should emit stable case ids, named
   profiles, and rerun commands.

6. Make isolation requirements explicit.
   If a test relies on subprocess isolation, global V8 state isolation, or a
   special execution profile, encode that in the harness instead of leaving it
   as a comment or CI quirk.

7. Keep `neovex-test-support` and local test-support modules concept-owned.
   Shared harness logic should live in clear test-support homes instead of being
   duplicated ad hoc across individual tests.

8. Treat the current git worktree as baseline reality.
   Reconcile dirty worktree state before starting a new implementation item.

---

## Current Assessed State

- `neovex-storage::simulation` already provides deterministic clocks, named
  signals, seeded fault injection, generated task-history replay, and stable
  verification-harness seed corpora.
- `docs/README.md` already documents PR, nightly, and single-case repro entry
  points for the verification harness.
- `neovex-test-support` already centralizes HTTP, WebSocket, service, and
  blocking-fault fixtures, but it does not yet own explicit runtime test
  profiles, eventual-assertion helpers, or isolation conventions.
- The runtime tests are now concept-owned files, but the cooperative tests still
  rely on a comment that says they should run in subprocesses instead of a real
  isolation harness.
- CI already has a verification-harness matrix, but the main test job currently
  works around runtime instability by running `neovex-runtime` single-threaded
  while the coverage job still uses a whole-workspace topology.
- Several runtime and server tests still rely on `RuntimeLimits::default()`,
  `RuntimePolicy::default()`, or fixed `sleep(Duration::from_millis(50))`
  polling in scenarios where the semantic intent is more specific than "whatever
  the product default is today."

---

## Current Review Findings

1. `crates/neovex-runtime/src/runtime/tests/cooperative.rs` already documents
   that cooperative locker tests should run in subprocesses, but the harness
   still runs them as ordinary in-process unit tests.

2. `.github/workflows/ci.yml` currently serializes `neovex-runtime` tests with
   `--test-threads=1`, while coverage still runs `cargo llvm-cov --workspace`.
   That means local, CI, and coverage do not currently exercise one explicit
   shared test topology.

3. Runtime and server tests still lean on moving defaults.
   `RuntimeLimits::default()` and `RuntimePolicy::default()` appear throughout
   runtime tests, executor tests, and server/runtime bridges, and
   `ConvexRegistry::from_manifest_paths(...)` currently bakes in a "safe"
   `RunToCompletion + StartupSnapshotCache` runtime policy that appears to be
   compensating for harness instability rather than expressing an intentional
   product default.

4. Critical-path tests still use wall-clock polling loops.
   Examples include runtime executor queue/fairness tests, websocket cleanup
   tests, demo-flow message arrival waits, and scheduler result waits.

5. The deterministic seed corpus is currently strongest for storage and the
   existing verification-harness surfaces, but the runtime queue, runtime
   integrity-after-success path, websocket auth resubscribe flow, and some
   scheduling/liveness flows are not yet owned by the same reproducible harness
   architecture.

6. The repo already has strong building blocks.
   The storage simulation seams, named seed corpus, blocking fault injector,
   service fixture, and WebSocket fixture mean this pass should harden the
   architecture around those pieces, not start over from scratch.

---

## Assessed But Not Selected

- Replacing Rust's default test runner wholesale.
  That may be worth revisiting later, but it is not required to fix the current
  determinism and ownership gaps.

- Broad JS SDK test-suite redesign.
  The immediate risk is in Rust runtime, server, harness, and CI behavior.

- A generalized fuzzing pass over every public API.
  This plan focuses first on explicit profiles, deterministic isolation,
  reproducible liveness campaigns, and CI alignment.

---

## Success Criteria

This plan is successful only when all of the following are true:

1. Semantics-heavy tests declare their runtime policy or harness profile
   explicitly unless they are specifically testing product defaults.

2. V8-sensitive and cooperative runtime tests run through a real isolation
   strategy owned by the harness, not by comments or a blanket crate-level CI
   workaround.

3. Critical-path server and runtime tests stop relying on arbitrary sleep-based
   polling when deterministic signals, manual clocks, or explicit eventual
   helpers are available.

4. Product defaults are no longer mutated or shadowed to make unstable tests
   pass; test support owns safety profiles where needed.

5. Runtime, transport, and liveness failures produce stable repro metadata,
   named profiles, or seed/case identifiers.

6. CI and coverage jobs execute a topology that matches the harness
   architecture closely enough that failures are reproducible locally.

7. The test and harness architecture is documented well enough that future
   contributors know where to put new deterministic campaigns, runtime profiles,
   and isolation-sensitive tests.

---

## Feature Preservation Matrix

| Surface | Must Stay Stable |
| --- | --- |
| Runtime semantics | Invocation, timeout, cancellation, pooling, auth refresh, warm-pool reuse, bundle integrity, and nested-call behavior stay unchanged. |
| Server and transport semantics | Native HTTP, WebSocket bootstrap, auth changes, unsubscribe, disconnect cleanup, and Convex-compatible flows stay unchanged. |
| Storage and verification harness semantics | Deterministic seeds, generated-task-history replay, fault scheduling, restart scheduling, and repro commands stay stable or become clearer. |
| Product defaults | Production defaults remain a product decision; tests should stop hijacking them indirectly. |
| Public test entrypoints | `make check`, `make test`, `make clippy`, and `bash scripts/verification-harness.sh ...` stay the canonical verification entrypoints. |

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
5. Do not "fix tests" by changing product defaults or blanket-serializing CI
   unless that temporary containment is recorded with an exit condition in this
   plan.
6. Update this plan's ledger and execution log in the same change set as the
   code or docs.

---

## Verification Contract

### Minimum verification for every implementation item

- targeted tests for the touched harness layer
- targeted tests for every invariant surfaced or moved by that item
- `cargo fmt --all --check`

For Rust items that change runtime, engine, server, or shared harness behavior,
also run the focused crate commands recorded below for that item.

### Repo-wide verification before closing the plan

- `make check`
- `make test`
- `make clippy`
- `bash scripts/verification-harness.sh pr storage`
- `bash scripts/verification-harness.sh pr engine`
- `bash scripts/verification-harness.sh pr server`
- `make ci` if practical

If environment, advisory-db locks, shared Cargo artifacts, or CI-only tools
block a command, record that limitation explicitly in the execution log instead
of silently skipping it.

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies | Gate Note |
| --- | --- | --- | --- | --- |
| TH0 | `done` | reviewed the current runtime, server, storage, and CI harness surfaces and identified the next high-value deterministic testing and harness-hardening seams | none | docs-only review and planning pass on 2026-04-08 |
| TH1 | `todo` | introduce canonical explicit test profiles and remove hidden reliance on moving runtime defaults | none | start here; it sets the semantic base for the rest of the plan |
| TH2 | `todo` | add real isolation for V8-sensitive and cooperative runtime tests | TH1 | follows naturally once profile ownership is explicit |
| TH3 | `todo` | replace critical sleep-based polling with deterministic waiters and eventual assertions | TH1 | can proceed once shared harness surfaces are identified |
| TH4 | `todo` | promote shared harness ownership and reproducible repro metadata through `neovex-test-support` and local runtime test support | TH1, TH3 | use the same named profiles and waiter conventions |
| TH5 | `todo` | add reproducible liveness and safety campaigns for current runtime and transport weak spots | TH1 through TH4 | build campaigns on the hardened harness surfaces |
| TH6 | `todo` | align CI and coverage topology with the harness architecture | TH2, TH4, TH5 | do this only after the harness categories are real |
| TH7 | `todo` | update docs, run the full verification sweep, and archive the completed plan cleanly | TH1 through TH6 | final closure only |

---

## Dependency Graph

- `TH1` is the recommended first slice because it fixes the largest architectural
  ambiguity: tests currently depend on moving defaults and unowned profile
  choices.
- `TH2` should follow `TH1` because runtime isolation is easier to encode once
  the intended execution profiles are named.
- `TH3` can start after `TH1` because deterministic waiters need the same
  explicit harness semantics.
- `TH4` should follow `TH1` and `TH3` because shared test-support ownership is
  easier to land once the profile catalog and eventual-assertion patterns are
  clear.
- `TH5` depends on the earlier items because adversarial campaigns should be
  written on the hardened harness architecture, not on today's ad hoc helpers.
- `TH6` should only happen after the harness categories are real enough to map
  honestly into CI and coverage lanes.
- `TH7` closes the workstream after the hardening changes and CI topology land.

---

## Recommended Delivery Order

1. `TH1` — explicit runtime and harness test profiles
2. `TH2` — real runtime test isolation
3. `TH3` — deterministic waiters and eventual assertions
4. `TH4` — shared harness ownership promotion
5. `TH5` — critical liveness and safety campaigns
6. `TH6` — CI and coverage topology alignment
7. `TH7` — docs, verification, and archive

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| TH0 | done | start `TH1` by cataloging the runtime and harness profiles tests should use explicitly instead of inheriting product defaults |
| TH1 | todo | after landing explicit profiles, encode runtime isolation categories in the runtime harness |
| TH2 | todo | after runtime isolation is real, replace sleep-based polling with deterministic waiters and eventual assertions |
| TH3 | todo | move the new waiter/profile patterns into shared harness ownership surfaces |
| TH4 | todo | use the hardened harness to add reproducible liveness and safety campaigns |
| TH5 | todo | once campaigns and harness categories are real, align CI and coverage topology to them |
| TH6 | todo | close out the workstream with docs, full verification, and archive handoff |
| TH7 | todo | n/a |

---

## Work Items

### TH0. Baseline review and hotspot map

#### Outcome

- Completed during this planning pass.

### TH1. Introduce canonical explicit test profiles

#### Implementation plan

1. Introduce named runtime and harness profiles for test code instead of
   repeatedly spelling `RuntimeLimits { ..RuntimeLimits::default() }` or
   inheriting `RuntimePolicy::default()`.
2. Make semantic tests opt into the profile they mean:
   default behavior,
   `RunToCompletion + StartupSnapshotCache`,
   cooperative warm-pool,
   bounded fairness stress,
   or any other clearly named profile this pass requires.
3. Move any test-only "safe default" behavior out of generic product builders
   and into test fixtures or test support where appropriate.
4. Add dedicated tests for actual product defaults if they are important to
   preserve.

#### Focused verification

- `cargo test -p neovex-runtime --lib`
- `cargo test -p neovex-server`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-runtime --all-targets -- -D warnings`
- `cargo clippy -p neovex-server --all-targets -- -D warnings`

#### Acceptance criteria

- tests no longer rely on moving product defaults unless that is the actual
  subject of the test
- generic product constructors no longer carry test-harness compensation logic
- default behavior is covered by explicit default-focused tests

### TH2. Add real isolation for V8-sensitive runtime tests

#### Implementation plan

1. Add harness-owned runtime test isolation for cooperative, locker, or other
   global-V8-state-sensitive tests.
2. Make the "run each cooperative test in a subprocess" expectation true in
   code instead of leaving it as a comment.
3. Classify runtime tests by isolation requirement and keep the classification
   obvious to future contributors.
4. Narrow or remove the blanket CI `--test-threads=1` workaround once the
   isolation harness is real. If anything temporary remains, record it
   explicitly here.

#### Focused verification

- `cargo test -p neovex-runtime --lib`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-runtime --all-targets -- -D warnings`

#### Acceptance criteria

- V8-sensitive runtime tests use real isolation owned by the harness
- runtime failures have stable local repro paths
- CI no longer relies on a broad workaround where a narrower honest category can
  be used

### TH3. Replace critical sleep-based polling with deterministic waiters

#### Implementation plan

1. Add eventual-assertion and deterministic waiter helpers where the harness can
   already observe the relevant state.
2. Replace critical `sleep(Duration::from_millis(50))` polling in runtime
   executor tests, websocket cleanup tests, demo-flow helpers, and scheduler
   history waits with deterministic gates or clearer eventual helpers.
3. Prefer manual clocks, fault gates, `Notify`, activity signals, or explicit
   state observation over arbitrary sleep windows.

#### Focused verification

- `cargo test -p neovex-runtime`
- `cargo test -p neovex-server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- critical-path waits no longer rely on arbitrary sleeps where deterministic
  signals exist
- eventual assertions produce clear timeout context and remain reproducible

### TH4. Promote shared harness ownership and repro metadata

#### Implementation plan

1. Extend `neovex-test-support` and any local runtime test-support modules so
   they become the canonical homes for:
   explicit runtime test profiles,
   eventual assertions,
   transport fixtures,
   scenario metadata,
   and stable repro helpers.
2. Keep storage simulation as the seed authority, but route runtime and server
   liveness campaigns through the same stable repro-story where practical.
3. Remove ad hoc duplicated harness helpers once their concept-owned homes
   exist.

#### Focused verification

- `cargo test -p neovex-test-support`
- `cargo test -p neovex-storage`
- `cargo test -p neovex-server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- shared harness logic has clear homes
- repro metadata is consistent across storage, engine, server, and runtime
  campaigns
- new historically valuable failures can be checked in with stable case ids

### TH5. Add reproducible liveness and safety campaigns for current weak spots

#### Implementation plan

1. Add harness-owned campaigns for the current known shaky flows:
   runtime queue underflow or completion-accounting regressions,
   cooperative concurrent dispatch,
   bundle integrity after prior success,
   websocket auth change and resubscribe semantics,
   websocket disconnect cleanup,
   and scheduler result availability or publication timing.
2. Express those campaigns through explicit profiles, deterministic waiters, and
   stable repro metadata instead of ad hoc fixes.
3. Use the same harsh, deterministic mindset described in the TigerBeetle
   research note: real logic, seeded inputs, reproducible failure schedules, and
   clear safety or liveness invariants.

#### Focused verification

- `cargo test -p neovex-runtime --lib`
- `cargo test -p neovex-server`
- `bash scripts/verification-harness.sh pr server`
- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- the currently fragile runtime and transport scenarios are covered by named,
  reproducible campaigns
- failures surface enough deterministic context to debug without guesswork

### TH6. Align CI and coverage topology with the harness architecture

#### Implementation plan

1. Encode the real harness categories in CI:
   workspace tests,
   isolated runtime tests,
   verification-harness PR and nightly lanes,
   and coverage.
2. Make the coverage path either use the same harness categories or explicitly
   document any unavoidable difference.
3. Prefer CI output that includes the same repro commands, case ids, and
   profile names that developers use locally.

#### Focused verification

- `make check`
- `make test`
- `make clippy`
- `bash scripts/verification-harness.sh pr storage`
- `bash scripts/verification-harness.sh pr engine`
- `bash scripts/verification-harness.sh pr server`

#### Acceptance criteria

- CI structure mirrors the harness architecture closely enough that failures are
  reproducible locally
- any remaining serialization or topology difference is narrow, explicit, and
  justified

### TH7. Docs and full verification closure

#### Implementation plan

1. Update `ARCHITECTURE.md`, `docs/README.md`, `docs/plans/README.md`,
   `AGENTS.md`, and any other relevant docs to reflect the landed test-harness
   architecture.
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
- `make ci` if practical

#### Acceptance criteria

- docs explain the landed harness architecture and contribution rules
- the plan can be archived cleanly with no ledger or worktree mismatch

---

## Execution Log

| Date | Item | Outcome | Summary | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-08 | TH0 | done | Reviewed the current runtime, server, storage, test-support, and CI harness surfaces against the live worktree and the TigerBeetle research note. Confirmed that the next high-value work is not more ad hoc tests but a deterministic test-and-harness hardening pass centered on explicit profiles, runtime isolation, deterministic waiters, reproducible campaigns, and CI alignment. | docs-only review and planning pass; no new code verification claimed in this handoff | start `TH1` by cataloging explicit runtime and harness test profiles and removing hidden dependence on moving product defaults |
