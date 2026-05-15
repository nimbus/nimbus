# Codebase Modularity And Maintainability Follow-On Control Plan

This is the canonical execution control plane for the next repo-wide
maintainability, modularity, canonical naming, and idiomatic-Rust cleanup
workstream after the completed plan archived at
`docs/plans/archive/codebase-modularity-and-maintainability-plan.md`.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `AGENTS.md`
- `docs/plans/archive/codebase-modularity-and-maintainability-plan.md`
- `crates/nimbus-bin/src/machine/mod.rs`
- `crates/nimbus-bin/src/machine/manager.rs`
- `crates/nimbus-bin/src/service/mod.rs`
- `crates/nimbus-bin/src/service/compose.rs`
- `crates/nimbus-bin/src/machine/api.rs`
- `crates/nimbus-sandbox/src/backends/oci/buildah.rs`
- `crates/nimbus-sandbox/src/backends/krun/vm.rs`
- `crates/nimbus-sandbox/tests/krun_linux_smoke.rs`
- `crates/nimbus-engine/src/tests/mutation_journal.rs`
- `crates/nimbus-engine/benches/embedded-provider-benchmarks.rs`
- `crates/nimbus-engine/benches/postgres-provider-benchmarks.rs`
- `crates/nimbus-engine/benches/mysql-provider-benchmarks.rs`
- `crates/nimbus-engine/benches/libsql-replica-provider-benchmarks.rs`
- `crates/nimbus-storage/src/libsql.rs`
- the current git worktree on `main`

Baseline verification status for this plan:

- the predecessor maintainability workstream completed and was archived at
  `docs/plans/archive/codebase-modularity-and-maintainability-plan.md`
- this follow-on control plane is being authored as a docs-only review and
  planning pass on 2026-04-19 from a clean worktree after reviewing the live
  post-cleanup codebase against the new file-size and ownership thresholds
- no new code verification is claimed by this planning pass
- every `CMF*` implementation item must record its own focused verification
  before it can be marked `done`

---

## Purpose

The previous maintainability wave landed the major CLI, provider, and krun
ownership splits. That work removed the old production god files and gave the
repo a much cleaner architecture map.

The next wave should stay disciplined and review-driven. It should not split
files just to reduce line counts. It should split or repackage code when the
current file still mixes too many concepts, hides ownership, keeps huge inline
regression suites inside thin roots, or duplicates the same benchmark harness
patterns across multiple giant files.

This follow-on wave has two goals:

- preserve the cleaner production architecture from the earlier wave while
  removing the remaining concept-mixed files
- repackage oversized regression and benchmark surfaces so large files are
  large for a reason, not because tests or benchmark scenarios never got their
  own homes

This is a maintainability and correctness roadmap, not a feature roadmap.

---

## Relationship To Other Plans

- Use `docs/plans/README.md` as the owning plan index.
- Use this plan for broad maintainability, readability, modularity,
  canonical-naming, and god-file cleanup work that is not already owned by
  another active plan.
- Use
  `docs/plans/archive/codebase-modularity-and-maintainability-plan.md` only
  for historical execution detail, predecessor checkpoints, and the landed
  ownership map this plan builds on.
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
- If work turns into feature behavior changes, protocol changes, benchmark
  methodology changes, install or distribution work, provider-product
  semantics, or platform-specific machine behavior, stop and move to the
  owning plan instead of stretching this cleanup plan across multiple streams.

---

## Scope

This plan covers:

- extraction of oversized inline regression suites from already-thin
  composition roots in `crates/nimbus-bin/src/machine/mod.rs`,
  `crates/nimbus-bin/src/machine/manager.rs`,
  `crates/nimbus-bin/src/service/mod.rs`, and
  `crates/nimbus-sandbox/src/backends/krun/vm.rs`
- concept-owned decomposition of
  `crates/nimbus-bin/src/service/compose.rs`
- concept-owned decomposition of
  `crates/nimbus-bin/src/machine/api.rs`
- concept-owned decomposition of
  `crates/nimbus-sandbox/src/backends/oci/buildah.rs`
- regression-suite packaging cleanup for
  `crates/nimbus-engine/src/tests/mutation_journal.rs`
  and `crates/nimbus-sandbox/tests/krun_linux_smoke.rs`
- benchmark harness modularization for the provider benchmark surfaces in
  `crates/nimbus-engine/benches/`
- follow-on doc, verification, and archive cleanup needed to keep this work
  resumable through handoff and compaction

This plan does not cover:

- new product features
- intentional CLI, route, wire, benchmark-result, or persistence behavior
  changes unless an item explicitly records them
- benchmark methodology changes that would make new results incomparable to the
  current suite
- install or distribution channel work
- rename work
- compatibility code for pre-launch behavior
- speculative performance rewrites that are not justified by ownership,
  readability, or maintainability

---

## Cleanup Invariants

These rules are mandatory for every item in this plan.

1. Preserve externally observable behavior by default.
   Machine CLI behavior, service CLI behavior, machine API behavior, sandbox
   helper behavior, regression semantics, and benchmark workload semantics stay
   unchanged unless a specific item explicitly records otherwise.

2. Keep the core architecture invariants intact.
   `nimbus-core` stays zero I/O.
   `nimbus-runtime` stays zero workspace dependencies.
   All mutations still flow through `Service::apply_mutation` or its queued
   async journal path.
   Storage atomicity stays unchanged.

3. Prefer concept-owned modules over helper piles.
   A successful split makes ownership easier to name, test, and debug locally.

4. Treat file size as a signal, not the goal.
   Files under 1,500 lines are usually acceptable unless they still mix too
   many concepts.
   Files from 1,500 through 1,999 lines need an explicit justification if they
   remain unsplit.
   Files at 2,000 lines or above must not remain as single mixed-owner files at
   item closeout; extract production ownership, test ownership, or benchmark
   ownership until the file has a clear reason to stay large.

5. Keep thin composition roots thin once ownership moves out.
   When a root is already conceptually clean, extracting its giant inline tests
   into sibling files counts as real maintainability work and is preferred over
   reopening the production split without cause.

6. Preserve benchmark and regression proof surfaces.
   If tests or benchmark workloads move, the moved coverage must remain easy to
   run, easy to map back to the owning concept, and comparable to prior proof
   bundles.

7. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

---

## Current Assessed State

- Before this planning pass, the repo again had no active generic cleanup or
  refactor control plane, so future broad maintainability work needed a new
  active owner rather than another revival of archived plans.
- The previous maintainability wave succeeded. The old CLI, provider, and krun
  production god files were already split into thinner module trees.
- The strongest remaining hotspots are now the provider benchmark surfaces,
  which still repeat shared harness ownership across multiple 2k to 4k files
  even after the earlier proof-packaging and production splits landed.
- Since kickoff, the first three roadmap slices have already reduced the
  original CLI hotspots into concept-owned or proof-owned surfaces:
  - `crates/nimbus-bin/src/machine/mod.rs`,
    `crates/nimbus-bin/src/machine/manager.rs`,
    `crates/nimbus-bin/src/service/mod.rs`, and
    `crates/nimbus-sandbox/src/backends/krun/vm.rs` now keep thin production
    roots with sibling regression files after `CMF1`
  - `crates/nimbus-bin/src/service/compose.rs` now reads as a 179-line
    composition root over `service/compose/raw.rs`, `lower.rs`, `parse.rs`,
    `warnings.rs`, `render.rs`, and `tests.rs` after `CMF2`
  - `crates/nimbus-bin/src/machine/api.rs` now reads as a 201-line composition
    root over `machine/api/routes.rs`, `capabilities.rs`, `binaries.rs`,
    `listener.rs`, `state.rs`, `logs.rs`, `process.rs`, and `tests.rs` after
    `CMF3`
  - `crates/nimbus-sandbox/src/backends/oci/buildah.rs` now reads as a 25-line
    composition root over `buildah/cli.rs`, `defaults.rs`, `inspect.rs`,
    `user.rs`, `render.rs`, and `tests.rs` after `CMF4`
  - `crates/nimbus-engine/src/tests/mutation_journal.rs` now reads as a 7-line
    composition root over `mutation_journal/cancellation.rs`,
    `applied_visibility.rs`, `queued.rs`, `subscriptions.rs`, and
    `support.rs` after `CMF5`
  - `crates/nimbus-sandbox/tests/krun_linux_smoke.rs` now reads as a 34-line
    smoke entrypoint over `krun_linux_smoke/launch.rs`, `inspect.rs`,
    `restart.rs`, `published_endpoints.rs`, `cleanup.rs`, and `support.rs`
    after `CMF6`
  - `crates/nimbus-engine/benches/provider_bench/common.rs` now owns the
    shared benchmark tenant-id, schema/query, directory-copy, round-override,
    and stats-formatting helpers, while
    `embedded-provider-benchmarks.rs` and
    `libsql-replica-provider-benchmarks.rs`
    now read as 1,817- and 1,987-line roots over local benchmark
    `report`, `support`, and `suite` modules after the first `CMF7` burst
  - `crates/nimbus-engine/benches/postgres-provider-benchmarks.rs` and
    `mysql-provider-benchmarks.rs` now read as 568- and 571-line composition
    roots over local benchmark `report`, `scenarios`, `suite`, `support`, and
    `workloads` modules after the `CMF7` closeout split
- The selected benchmark hotspot map is now closed out from the live worktree:
  - no selected provider benchmark root remains at or above the hard
    2,000-line threshold
  - `crates/nimbus-engine/benches/embedded-provider-benchmarks.rs` remains at
    1,817 lines with explicit justification: after the shared-helper plus
    local `report` and `support` extraction, the remaining bulk is the
    provider-owned embedded workload suite plus the small benchmark data
    models it operates on, so another split would mostly mirror a single
    provider benchmark tree rather than separate a distinct ownership mix
  - `crates/nimbus-engine/benches/libsql-replica-provider-benchmarks.rs`
    remains at 1,987 lines with explicit justification: the root now delegates
    report rendering, suite orchestration, and provider bootstrap into sibling
    modules, while the remaining lines are the cohesive libsql-replica
    workload definitions and benchmark models that own the provider-specific
    semantics together
  - `crates/nimbus-engine/benches/postgres-provider-benchmarks.rs` is 568
    lines
  - `crates/nimbus-engine/benches/mysql-provider-benchmarks.rs` is 571 lines
- Several notable production files are now below the threshold and already
  organized around acceptable seams:
  - `crates/nimbus-storage/src/libsql.rs` is 1,471 lines and already delegates
    through `provider`, `storage`, `read`, `write`, `remote`, and `transport`
  - `crates/nimbus-storage/src/mysql.rs` is 1,445 lines and already acts as a
    thin composition root
  - `crates/nimbus-storage/src/postgres.rs` is 1,287 lines and already acts as
    a thin composition root

---

## Current Review Findings

1. The provider benchmark surfaces are now the clearest remaining 2k to 4k
   files in the repo.
   `embedded-provider-benchmarks.rs`,
   `postgres-provider-benchmarks.rs`,
   `mysql-provider-benchmarks.rs`, and
   `libsql-replica-provider-benchmarks.rs`
   all repeat the same benchmark-config, report-rendering, workload-lane, and
   environment-bootstrap patterns. That duplication makes the benchmark suite
   harder to extend and harder to keep consistent across providers.

2. `crates/nimbus-storage/src/libsql.rs` is now the main "leave it alone for
   now" example.
   It is below the threshold and its remaining root-level ownership is the
   replica freshness state machine plus shared low-level helpers. That is a
   cohesive concept and does not need another split unless new work expands it.

---

## Success Criteria

This plan is successful only when all of the following are true:

- the oversized thin roots no longer keep giant inline regression suites inside
  the same file as their production composition logic
- `service/compose.rs`, `machine/api.rs`, and `buildah.rs` read as concept-owned
  module trees instead of mixed-responsibility files
- `mutation_journal.rs` and `krun_linux_smoke.rs` are easier to navigate by
  scenario group, helper ownership, and failure mode
- the provider benchmark suite has a clearer shared harness layout and no
  longer repeats the same control-flow boilerplate across 2k to 4k files
- every file left above 1,500 lines has an explicit justification in the plan
  closeout notes, and no selected file remains above 2,000 lines for avoidable
  packaging reasons
- docs, plan status, and archive state accurately reflect the landed work

---

## Assessed But Not Selected

- `crates/nimbus-storage/src/libsql.rs` at 1,471 lines is below the threshold
  and already reads as a concept-owned replica freshness composition root over
  its new submodules. Revisit only if later work pushes new unrelated
  ownership into the root.
- `crates/nimbus-storage/src/mysql.rs` at 1,445 lines and
  `crates/nimbus-storage/src/postgres.rs` at 1,287 lines are below the
  threshold and already aligned to the clearer provider layout from the
  previous wave.
- `crates/nimbus-bin/src/machine/client.rs`,
  `crates/nimbus-sandbox/src/backends/container/runtime.rs`,
  `crates/nimbus-sandbox/src/backends/oci/builder.rs`, and
  `crates/nimbus-sandbox/src/backends/oci/network.rs`
  are notable but not first-wave targets for this pass because they are below
  the threshold and the reviewed ownership seams are less urgent than the files
  selected above.
- Larger engine, storage, and server regression files below the threshold can
  stay out of this wave unless implementation reveals a stronger concept-mix
  problem than the current review found.

---

## Feature Preservation Matrix

| Surface | Preservation Requirement |
| --- | --- |
| Machine CLI | root layout, record files, lock semantics, status rendering, SSH/SCP behavior, and command UX stay unchanged |
| Service CLI | Compose loading, backend selection, forwarded machine API behavior, lifecycle operations, logs, and `ps` output stay unchanged |
| Machine API | route shapes, helper-binary capability reporting, log and process inspection semantics, and listener behavior stay unchanged |
| OCI/buildah helpers | image launch preparation, mount-session behavior, rootfs user resolution, env/port/default merging, and error rendering stay unchanged |
| Engine mutation-journal tests | regression semantics and failure-mode coverage stay unchanged even if tests move into submodules |
| Sandbox smoke tests | smoke-test semantics and proof coverage stay unchanged even if scenario files move |
| Provider benchmarks | workload definitions, output shape, and provider-to-provider comparability stay unchanged unless a benchmark item explicitly records otherwise |

---

## Control Plan Rules

1. Implement exactly one `CMF*` item at a time unless the plan explicitly says
   otherwise.
2. Do not skip ahead while an earlier eligible item remains `todo`.
3. Do not start a new item while another remains `in_progress`.
4. If the worktree is dirty, reconcile it to the owning item before proceeding.
5. If a production root is already clean, prefer test extraction over
   reopening the production split without cause.
6. If implementation reveals a materially better seam map, update this plan
   first, then implement it.
7. After every meaningful work burst, update the roadmap ledger,
   checkpoints, and execution log so handoff does not depend on chat memory.

---

## Verification Contract

Every implementation item in this plan must run its focused verification, plus:

- `cargo fmt --all --check`
- `cargo check --workspace`

Use these focused lanes as appropriate:

- `cargo test -p nimbus-bin machine`
- `cargo test -p nimbus-bin service`
- `cargo test -p nimbus-sandbox`
- `cargo test -p nimbus-engine mutation_journal`
- `cargo check -p nimbus-engine --benches`
- `cargo clippy -p nimbus-bin --all-targets -- -D warnings`
- `cargo clippy -p nimbus-sandbox --all-targets -- -D warnings`
- `cargo clippy -p nimbus-engine --all-targets -- -D warnings`

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
| CMF0 | `done` | reviewed the live post-cleanup codebase, applied the new line-count thresholds, and promoted this follow-on maintainability control plane | none | docs-only review and planning pass on 2026-04-19 |
| CMF1 | `done` | extract oversized inline regression suites from already-thin roots in `machine/mod.rs`, `machine/manager.rs`, `service/mod.rs`, and `krun/vm.rs` | CMF0 | completed on 2026-04-19 with the production roots reduced to thin composition surfaces and the moved regression suites preserved in sibling test modules |
| CMF2 | `done` | split `crates/nimbus-bin/src/service/compose.rs` into concept-owned project, lowering, parsing, and rendering seams | CMF0 | completed on 2026-04-19 with a thin `compose.rs` root and dedicated `raw`, `lower`, `parse`, `warnings`, `render`, and `tests` modules |
| CMF3 | `done` | split `crates/nimbus-bin/src/machine/api.rs` into router, listener, capability, lifecycle, logs, process, and helper-resolution seams | CMF0 | completed on 2026-04-19 with a thin `api.rs` root and dedicated `routes`, `capabilities`, `binaries`, `listener`, `state`, `logs`, `process`, and `tests` modules |
| CMF4 | `done` | split `crates/nimbus-sandbox/src/backends/oci/buildah.rs` into CLI/session, inspect/defaults, rootfs-user, parsing, and render/error seams | CMF0 | completed on 2026-04-19 with a thin `buildah.rs` root and dedicated `cli`, `defaults`, `inspect`, `user`, `render`, and `tests` modules |
| CMF5 | `done` | repackage `crates/nimbus-engine/src/tests/mutation_journal.rs` into scenario-owned submodules with shared helpers | CMF0 | completed on 2026-04-19 with a thin `mutation_journal.rs` root plus dedicated `cancellation`, `applied_visibility`, `queued`, `subscriptions`, and `support` modules |
| CMF6 | `done` | repackage `crates/nimbus-sandbox/tests/krun_linux_smoke.rs` into scenario-owned smoke modules and helpers | CMF1 recommended first | completed on 2026-04-19 with a thin `krun_linux_smoke.rs` entrypoint plus dedicated `launch`, `inspect`, `restart`, `published_endpoints`, `cleanup`, and `support` modules |
| CMF7 | `done` | modularize the provider benchmark harness under `crates/nimbus-engine/benches/` and split the 2k to 4k benchmark files into shared harness plus provider-owned workloads | CMF2 through CM4 recommended first | completed on 2026-04-19 with shared `provider_bench/common.rs`, modularized provider-local harness trees, and explicit justification recorded for the near-threshold embedded and libsql roots |
| CMF8 | `done` | update docs, run the full verification sweep, and archive this completed follow-on plan cleanly | CMF1 through CM7 | completed on 2026-04-19 after the docs were switched to archive references, the full verification sweep passed, and this plan was moved into `docs/plans/archive/` |

---

## Dependency Graph

- `CMF1` is the recommended first slice because it removes the largest
  packaging debt from already-thin roots without reopening settled production
  architecture.
- `CMF2`, `CMF3`, and `CMF4` are the main remaining production concept-mix
  items and should settle before the benchmark wave so the benchmark layout can
  follow the same naming discipline.
- `CMF5` can run after `CMF0`, but it is easier once the repo has already
  established the new pattern of moving large inline proof surfaces into
  scenario-owned files.
- `CMF6` should usually follow `CMF1` so the krun-side production root and
  smoke-side regression surfaces evolve in a consistent way.
- `CMF7` should usually follow the production cleanup items because the shared
  harness naming and ownership patterns are clearer after the production seams
  settle.
- `CMF8` closes the workstream after the selected production, regression, and
  benchmark cleanup slices land.

---

## Recommended Delivery Order

1. `CMF1` — thin-root regression extraction
2. `CMF2` — Compose service-plan split
3. `CMF3` — machine API split
4. `CMF4` — OCI/buildah split
5. `CMF5` — engine mutation-journal regression packaging
6. `CMF6` — krun smoke regression packaging
7. `CMF7` — provider benchmark harness modularization
8. `CMF8` — docs, verification, and archive

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| CMF0 | done | start `CMF1` by extracting the inline regression suites from the already-thin CLI and krun roots without changing their production ownership map |
| CMF1 | done | moved the large inline tests out of `machine/mod.rs`, `machine/manager.rs`, `service/mod.rs`, and `krun/vm.rs` into sibling files while keeping the current root APIs intact; the roots now measure 203, 473, 770, and 319 lines respectively |
| CMF2 | done | split `service/compose.rs` into a 179-line composition root plus `raw.rs`, `lower.rs`, `parse.rs`, `warnings.rs`, `render.rs`, and `tests.rs`, preserving Compose load, warning, and render behavior while moving the regression coverage beside the new surface |
| CMF3 | done | split `machine/api.rs` into a 201-line composition root plus `routes.rs`, `capabilities.rs`, `binaries.rs`, `listener.rs`, `state.rs`, `logs.rs`, `process.rs`, and `tests.rs`, preserving route shape, helper reporting, and listener behavior while moving the proof surface beside the new tree |
| CMF4 | done | split `buildah.rs` into a 25-line composition root plus `cli.rs`, `defaults.rs`, `inspect.rs`, `user.rs`, `render.rs`, and `tests.rs`, preserving buildah launch preparation, mount-session behavior, rootfs-user resolution, and env/port/default parsing while moving the proof surface beside the new helper tree |
| CMF5 | done | split `mutation_journal.rs` into a 7-line composition root plus `cancellation.rs`, `applied_visibility.rs`, `queued.rs`, `subscriptions.rs`, and `support.rs`, preserving the existing journal regression semantics while grouping durable fault injection and scenario setup under local support ownership |
| CMF6 | done | split `krun_linux_smoke.rs` into a 34-line entrypoint plus `launch.rs`, `inspect.rs`, `restart.rs`, `published_endpoints.rs`, `cleanup.rs`, and `support.rs`, preserving the ignored Linux smoke scenarios while moving shared config, probe, manifest, host-command, and cleanup helpers into a local support module |
| CMF7 | done | extracted `provider_bench/common.rs`, split the embedded and libsql roots into local helper modules, then reduced the Postgres and MySQL roots to 568 and 571 lines over provider-local `report`, `scenarios`, `suite`, `support`, and `workloads` modules; the embedded and libsql roots stay at 1,817 and 1,987 lines with explicit closeout justification because their remaining bulk is cohesive provider-owned workload definition rather than mixed bootstrap or reporting ownership |
| CMF8 | done | updated `AGENTS.md` and `docs/plans/README.md` for archive state, ran the full verification sweep, and archived this plan under `docs/plans/archive/` after confirming every selected root now satisfies the threshold rules or has explicit justification |

---

## Work Items

### CMF0. Baseline review and follow-on plan promotion

#### Outcome

- Completed during this planning pass.

### CMF1. Extract oversized inline regression suites from thin roots

#### Implementation plan

1. Keep the production roots in
   `crates/nimbus-bin/src/machine/mod.rs`,
   `crates/nimbus-bin/src/machine/manager.rs`,
   `crates/nimbus-bin/src/service/mod.rs`, and
   `crates/nimbus-sandbox/src/backends/krun/vm.rs`
   as the public composition surfaces.
2. Move the large inline test modules into sibling files or scenario-owned
   test modules with clear ownership and stable imports.
3. Keep the existing root ergonomics, module visibility, and test semantics
   unchanged.

#### Focused verification

- `cargo test -p nimbus-bin machine`
- `cargo test -p nimbus-bin service`
- `cargo test -p nimbus-sandbox`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-bin --all-targets -- -D warnings`
- `cargo clippy -p nimbus-sandbox --all-targets -- -D warnings`

#### Acceptance criteria

- the selected roots no longer keep giant inline regression suites in the same
  file as the production composition logic
- file ownership is easier to navigate without reopening settled production
  architecture
- machine, service, and krun behavior remain unchanged

### CMF2. Split `service/compose.rs` into concept-owned modules

#### Implementation plan

1. Keep `crates/nimbus-bin/src/service/compose.rs` or a successor
   `service/compose/` directory as the public Compose service-plan surface.
2. Separate raw Compose document types, project/service lowering, parser
   helpers, warning helpers, and rendering helpers into clearer modules.
3. Preserve Compose loading, warning, and rendered plan semantics.

#### Focused verification

- `cargo test -p nimbus-bin service`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-bin --all-targets -- -D warnings`

#### Acceptance criteria

- the Compose loader is easier to navigate by concept
- service lowering and parser helpers have canonical homes
- rendered config and warning behavior remain unchanged

### CMF3. Split `machine/api.rs` into concept-owned machine API modules

#### Implementation plan

1. Keep `crates/nimbus-bin/src/machine/api.rs` or a successor directory module
   as the machine API composition root.
2. Separate router/listener setup, capability reporting, lifecycle handlers,
   persisted-state refresh, log/process helpers, and binary-resolution helpers
   into clearer modules.
3. Preserve route shape, listener behavior, forwarded container semantics, and
   helper-binary reporting.

#### Focused verification

- `cargo test -p nimbus-bin machine`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-bin --all-targets -- -D warnings`

#### Acceptance criteria

- the machine API surface is easier to follow by responsibility
- router and lifecycle behavior remain unchanged
- helper and process-inspection logic are easier to test locally

### CMF4. Split `oci/buildah.rs` into concept-owned backend helper modules

#### Implementation plan

1. Keep `crates/nimbus-sandbox/src/backends/oci/buildah.rs` or a successor
   directory module as the public buildah helper surface.
2. Separate the CLI/session wrapper, inspect/defaults parsing, rootfs-user
   resolution, env/port parsing, and render/error helpers into clearer modules.
3. Preserve buildah launch preparation and mount-session semantics exactly.

#### Focused verification

- `cargo test -p nimbus-sandbox`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-sandbox --all-targets -- -D warnings`

#### Acceptance criteria

- buildah helper ownership is easier to name and navigate
- session lifecycle and image defaults remain unchanged
- parsing and error-rendering helpers are no longer buried in one file

### CMF5. Repackage `mutation_journal.rs` into scenario-owned regression modules

#### Implementation plan

1. Keep the mutation-journal regression coverage in the engine test surface.
2. Group scenarios by cancellation, queued mutation behavior, applied-visibility
   waits, and subscription/journal coordination.
3. Move shared setup helpers into a local support module instead of repeating
   them across one giant test file.

#### Focused verification

- `cargo test -p nimbus-engine mutation_journal`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-engine --all-targets -- -D warnings`

#### Acceptance criteria

- the mutation-journal regression suite is easier to navigate by failure mode
- coverage stays intact
- helpers are clearer without weakening the proof surface

### CMF6. Repackage `krun_linux_smoke.rs` into scenario-owned smoke modules

#### Implementation plan

1. Keep the current krun smoke semantics and proof intent unchanged.
2. Group launch, inspect, restart, published-endpoint, and cleanup scenarios
   into clearer smoke modules with local helpers.
3. Preserve the same platform guards, fixtures, and high-value assertions.

#### Focused verification

- `cargo test -p nimbus-sandbox`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-sandbox --all-targets -- -D warnings`

#### Acceptance criteria

- the krun smoke suite is easier to navigate and extend
- smoke semantics stay unchanged
- proof helpers are local to the owning scenarios

### CMF7. Modularize the provider benchmark harness

#### Implementation plan

1. Keep the provider benchmark suite under
   `crates/nimbus-engine/benches/`.
2. Extract shared config parsing, report rendering, workload-lane definitions,
   and environment/bootstrap helpers into common benchmark modules.
3. Leave provider-specific workload setup and provider-specific environment
   details close to the benchmark that owns them.
4. Preserve benchmark workload slugs, markdown output shape, and provider
   comparability.

#### Focused verification

- `cargo check -p nimbus-engine --benches`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-engine --all-targets -- -D warnings`

#### Acceptance criteria

- the benchmark suite no longer repeats the same boilerplate across 2k to 4k
  files
- provider-specific details stay explicit
- benchmark CLI and report semantics stay comparable to the current suite

### CMF8. Docs, verification, and archive closeout

#### Implementation plan

1. Update `AGENTS.md`, `docs/plans/README.md`, and any touched reference docs
   so the landed ownership map is discoverable.
2. Run the full verification sweep required by this plan.
3. Archive this control plane once all items are complete and future generic
   cleanup work needs a newly promoted plan.

#### Focused verification

- `make check`
- `make test`
- `make clippy`
- `npm run test --workspaces --if-present`
- `npm run build --workspaces --if-present`
- `make ci` if practical

#### Acceptance criteria

- the docs reflect the landed ownership map
- the full verification sweep is recorded
- this plan can move to `docs/plans/archive/` cleanly

---

## Execution Log

| Date | Item | Status | Notes | Verification | Next Step |
| --- | --- | --- | --- | --- | --- |
| 2026-04-19 | CMF0 | `done` | Reviewed the live repo after the completed maintainability wave, applied the new thresholds of "under 1,500 usually okay unless concept-mixed", "1,500 to 1,999 needs explicit justification", and "2,000 or above must be broken down by ownership or proof packaging", and promoted this follow-on plan as the new active control plane. The review found that the old production god files were already split, so the next wave should focus on thin-root test extraction, the remaining concept-mixed production files (`service/compose.rs`, `machine/api.rs`, `buildah.rs`), oversized regression suites (`mutation_journal.rs`, `krun_linux_smoke.rs`), and the repeated provider benchmark harness files. | docs-only review; no new code verification claimed | start `CMF1` by extracting the oversized inline regression suites from the already-thin CLI and krun roots |
| 2026-04-19 | CMF1 | `in_progress` | Reconciled the live worktree and confirmed there is no pre-existing `in_progress` follow-on item. Began the first implementation slice by extracting the large inline regression suites out of `crates/nimbus-bin/src/machine/mod.rs`, `crates/nimbus-bin/src/machine/manager.rs`, `crates/nimbus-bin/src/service/mod.rs`, and `crates/nimbus-sandbox/src/backends/krun/vm.rs` while keeping those files as the production composition roots. | plan review plus `git status -sb`; targeted `sed`/`rg` reads over the four root files and their inline test module boundaries | mechanically move the inline `mod tests` blocks into sibling files, then run the focused `nimbus-bin` and `nimbus-sandbox` verification lanes |
| 2026-04-19 | CMF1 | `done` | Moved the inline regression suites out of `crates/nimbus-bin/src/machine/mod.rs`, `crates/nimbus-bin/src/machine/manager.rs`, `crates/nimbus-bin/src/service/mod.rs`, and `crates/nimbus-sandbox/src/backends/krun/vm.rs` into sibling files at `machine/tests.rs`, `machine/manager/tests.rs`, `service/tests.rs`, and `krun/vm/tests.rs`. The production roots stayed in place as the public composition surfaces, and their file sizes dropped to 203, 473, 770, and 319 lines respectively without changing behavior. | `cargo test -p nimbus-bin machine`; `cargo test -p nimbus-bin service`; `cargo test -p nimbus-sandbox`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p nimbus-bin --all-targets -- -D warnings`; `cargo clippy -p nimbus-sandbox --all-targets -- -D warnings` | start `CMF2` by mapping `crates/nimbus-bin/src/service/compose.rs` into raw model, lowering, parser, warning, and render seams before moving code |
| 2026-04-19 | CMF2 | `in_progress` | Began the next eligible production cleanup item immediately after closing `CMF1`. The target is `crates/nimbus-bin/src/service/compose.rs`, which still mixes raw Compose model decoding, project or service lowering, parser helpers, warning helpers, and rendering in one file. | `wc -l crates/nimbus-bin/src/service/compose.rs`; targeted `sed`/`rg` reads over the live Compose plan surface during the follow-on review and again after `CMF1` closed | extract the first concept-owned `service/compose/` modules without changing Compose load, warning, or rendered-plan semantics |
| 2026-04-19 | CMF2 | `done` | Repackaged `crates/nimbus-bin/src/service/compose.rs` into a thin 179-line composition root plus `service/compose/raw.rs`, `lower.rs`, `parse.rs`, `warnings.rs`, `render.rs`, and `tests.rs`. Raw Compose document decoding, lowering, scalar and lifecycle parsing, warning emission, render helpers, and the Compose regression suite now live in concept-owned files without changing the rendered plan, warning, or load semantics. | `cargo test -p nimbus-bin service`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p nimbus-bin --all-targets -- -D warnings` | start `CMF3` by mapping `crates/nimbus-bin/src/machine/api.rs` into router/listener, lifecycle, persisted-state refresh, log/process, and binary-resolution seams |
| 2026-04-19 | CMF3 | `in_progress` | Began the next eligible item immediately after closing `CMF2`. The target is `crates/nimbus-bin/src/machine/api.rs`, which still mixes router and listener setup, capability reporting, lifecycle handlers, persisted-state refresh, log/process helpers, and helper-binary resolution in one file. | `git status -sb`; follow-on plan ledger reconciliation; upcoming targeted reads over `machine/api.rs` | extract the first `machine/api/` concept-owned modules without changing route shapes, listener behavior, or helper reporting |
| 2026-04-19 | CMF3 | `done` | Repackaged `crates/nimbus-bin/src/machine/api.rs` into a thin 201-line composition root plus `machine/api/routes.rs`, `capabilities.rs`, `binaries.rs`, `listener.rs`, `state.rs`, `logs.rs`, `process.rs`, and `tests.rs`. Router setup, lifecycle handlers, helper-binary discovery, capability probing, persisted-state refresh, log/process helpers, and the machine API regression suite now live in concept-owned files without changing route shapes, listener behavior, or helper reporting. | `cargo test -p nimbus-bin machine`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p nimbus-bin --all-targets -- -D warnings` | start `CMF4` by mapping `crates/nimbus-sandbox/src/backends/oci/buildah.rs` into CLI/session, inspect/defaults, rootfs-user, parsing, and render/error seams |
| 2026-04-19 | CMF4 | `in_progress` | Began the next eligible production cleanup item immediately after closing `CMF3`. The target is `crates/nimbus-sandbox/src/backends/oci/buildah.rs`, which still mixes the CLI/session wrapper, mount lifecycle, inspect/default parsing, rootfs-user resolution, env/port parsing, and render/error helpers in one file. | `wc -l crates/nimbus-sandbox/src/backends/oci/buildah.rs`; updated plan hotspot map after the landed `CMF1` through `CMF3` seams | map `buildah.rs` into the first concept-owned backend helper modules without changing buildah-backed launch or mount-session semantics |
| 2026-04-19 | CMF4 | `done` | Repackaged `crates/nimbus-sandbox/src/backends/oci/buildah.rs` into a thin 25-line composition root plus `buildah/cli.rs`, `defaults.rs`, `inspect.rs`, `user.rs`, `render.rs`, and `tests.rs`. The buildah CLI wrapper, mount-session lifecycle, inspect/config decoding, rootfs-user resolution, env/port/default merging, command-failure rendering, and regression coverage now live in concept-owned files without changing buildah-backed launch or mount-session semantics. | `cargo test -p nimbus-sandbox`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p nimbus-sandbox --all-targets -- -D warnings` | start `CMF5` by grouping `crates/nimbus-engine/src/tests/mutation_journal.rs` into cancellation, queueing, applied-visibility, and subscription/journal scenario modules |
| 2026-04-19 | CMF5 | `in_progress` | Began the next eligible regression-packaging item immediately after closing `CMF4`. The target is `crates/nimbus-engine/src/tests/mutation_journal.rs`, which still keeps cancellation, queued mutation, applied-visibility, and subscription/journal coordination scenarios in one file with shared setup helpers mixed through the proof surface. | `wc -l crates/nimbus-engine/src/tests/mutation_journal.rs`; updated plan hotspot map after the landed `CMF4` buildah split | map `mutation_journal.rs` into scenario-owned modules and extract shared helpers without changing the regression semantics |
| 2026-04-19 | CMF5 | `done` | Repackaged `crates/nimbus-engine/src/tests/mutation_journal.rs` into a 7-line composition root plus `mutation_journal/cancellation.rs`, `applied_visibility.rs`, `queued.rs`, `subscriptions.rs`, and `support.rs`. Cancellation, queued mutation, applied-visibility, and subscription or journal coordination scenarios now live in concept-owned files, and the durable-append faulted-service setup moved into a local support helper without changing the regression semantics. | `cargo test -p nimbus-engine mutation_journal`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p nimbus-engine --all-targets -- -D warnings` | start `CMF6` by grouping `crates/nimbus-sandbox/tests/krun_linux_smoke.rs` into launch, inspect, restart, published-endpoint, and cleanup scenario modules |
| 2026-04-19 | CMF6 | `in_progress` | Began the next eligible regression-packaging item immediately after closing `CMF5`. The target is `crates/nimbus-sandbox/tests/krun_linux_smoke.rs`, which still keeps launch, inspect, restart, published-endpoint, and cleanup smoke scenarios plus shared setup helpers in one oversized proof surface. | `wc -l crates/nimbus-sandbox/tests/krun_linux_smoke.rs`; follow-on plan reconciliation after landing the `CMF5` mutation-journal split | map `krun_linux_smoke.rs` into scenario-owned smoke modules and local helpers without changing krun smoke semantics |
| 2026-04-19 | CMF6 | `done` | Repackaged `crates/nimbus-sandbox/tests/krun_linux_smoke.rs` into a 34-line smoke entrypoint plus `krun_linux_smoke/launch.rs`, `inspect.rs`, `restart.rs`, `published_endpoints.rs`, `cleanup.rs`, and `support.rs`. The ignored Linux smoke scenarios now live in concept-owned modules, and the repeated backend-config, probe, manifest, buildah, and cleanup plumbing moved into a local support module without changing the smoke semantics. | `cargo test -p nimbus-sandbox`; `cargo fmt --all`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p nimbus-sandbox --all-targets -- -D warnings` | start `CMF7` by mapping the repeated provider benchmark harness patterns across the four benchmark entrypoints |
| 2026-04-19 | CMF7 | `in_progress` | Began the next eligible benchmark-packaging item immediately after closing `CMF6`. The target is the provider benchmark harness under `crates/nimbus-engine/benches/`, where the embedded, libsql replica, Postgres, and MySQL benchmark entrypoints still repeat shared config parsing, report rendering, workload-lane orchestration, and environment bootstrap code across four large files. | `wc -l crates/nimbus-engine/benches/embedded-provider-benchmarks.rs crates/nimbus-engine/benches/libsql-replica-provider-benchmarks.rs crates/nimbus-engine/benches/postgres-provider-benchmarks.rs crates/nimbus-engine/benches/mysql-provider-benchmarks.rs`; follow-on plan reconciliation after landing the `CMF6` krun smoke split | design and extract a shared benchmark harness module tree before moving provider-specific workloads beside each provider entrypoint |
| 2026-04-19 | CMF7 | `in_progress` | Extracted `crates/nimbus-engine/benches/provider_bench/common.rs` for the shared benchmark tenant-id, schema/query, round-override, directory-copy, and stats-formatting helpers, then repackaged `embedded-provider-benchmarks.rs` into local `report.rs` and `support.rs` modules and `libsql-replica-provider-benchmarks.rs` into local `suite.rs`, `report.rs`, and `support.rs` modules. The embedded and libsql roots now measure 1,817 and 1,987 lines respectively while preserving the benchmark workload definitions, CLI slugs, and markdown output shape. | `cargo fmt --all`; `cargo fmt --all --check`; `cargo check -p nimbus-engine --benches`; `cargo check --workspace`; `cargo clippy -p nimbus-engine --all-targets -- -D warnings` | continue `CMF7` by splitting the remaining Postgres and MySQL benchmark roots and then revisit whether the embedded/libsql roots need one more concept split or can close with explicit justification |
| 2026-04-19 | CMF7 | `done` | Completed the provider benchmark harness modularization by reducing `postgres-provider-benchmarks.rs` and `mysql-provider-benchmarks.rs` to 568- and 571-line composition roots over provider-local `report.rs`, `scenarios.rs`, `suite.rs`, `support.rs`, and `workloads.rs` modules. The shared benchmark helpers now live in `provider_bench/common.rs`, the embedded and libsql roots remain at 1,817 and 1,987 lines with explicit closeout justification, and the benchmark CLI slugs, workload definitions, and markdown report shape stayed intact. | `cargo fmt --all`; `cargo check -p nimbus-engine --benches`; `cargo fmt --all --check`; `cargo check --workspace`; `cargo clippy -p nimbus-engine --all-targets -- -D warnings` | start `CMF8` by running the full repo verification sweep, updating the docs for archive state, and moving this plan into `docs/plans/archive/` once the closeout is recorded |
| 2026-04-19 | CMF8 | `done` | Updated `AGENTS.md` and `docs/plans/README.md` so broad maintainability work now treats this wave as archived history, then ran the full closeout verification bundle and archived this plan under `docs/plans/archive/`. Every selected root now sits below the hard 2,000-line threshold, and the remaining 1,500-to-1,999-line embedded and libsql benchmark roots have explicit closeout justification recorded in this plan. | `cargo fmt --all`; `cargo fmt --all --check`; `cargo check -p nimbus-engine --benches`; `cargo check --workspace`; `cargo clippy -p nimbus-engine --all-targets -- -D warnings`; `make check`; `make test`; `make clippy`; `npm run test --workspaces --if-present`; `npm run build --workspaces --if-present`; `cargo deny check`; `make ci` | no further maintainability follow-on items remain; promote a new active plan before starting another repo-wide cleanup wave |
