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
- `crates/neovex-bin/src/machine/mod.rs`
- `crates/neovex-bin/src/machine/manager.rs`
- `crates/neovex-bin/src/service/mod.rs`
- `crates/neovex-bin/src/service/compose.rs`
- `crates/neovex-bin/src/machine/api.rs`
- `crates/neovex-sandbox/src/backends/oci/buildah.rs`
- `crates/neovex-sandbox/src/backends/krun/vm.rs`
- `crates/neovex-sandbox/tests/krun_linux_smoke.rs`
- `crates/neovex-engine/src/tests/mutation_journal.rs`
- `crates/neovex-engine/benches/embedded-provider-benchmarks.rs`
- `crates/neovex-engine/benches/postgres-provider-benchmarks.rs`
- `crates/neovex-engine/benches/mysql-provider-benchmarks.rs`
- `crates/neovex-engine/benches/libsql-replica-provider-benchmarks.rs`
- `crates/neovex-storage/src/libsql.rs`
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
  `docs/plans/system-tenant-api-plan.md`,
  `docs/plans/desktop-ui-plan.md`,
  `docs/plans/install-script-plan.md`,
  `docs/plans/distribution-plan.md`,
  `docs/plans/windows-machine-support-plan.md`,
  `docs/plans/wasmtime-backend-plan.md`,
  `docs/plans/wasi-agent-capabilities-plan.md`,
  `docs/plans/nimbus-rename-plan.md`,
  and `docs/plans/nimbus-rename-satellite-repos-plan.md`.
- If work turns into feature behavior changes, protocol changes, benchmark
  methodology changes, install or distribution work, provider-product
  semantics, or platform-specific machine behavior, stop and move to the
  owning plan instead of stretching this cleanup plan across multiple streams.

---

## Scope

This plan covers:

- extraction of oversized inline regression suites from already-thin
  composition roots in `crates/neovex-bin/src/machine/mod.rs`,
  `crates/neovex-bin/src/machine/manager.rs`,
  `crates/neovex-bin/src/service/mod.rs`, and
  `crates/neovex-sandbox/src/backends/krun/vm.rs`
- concept-owned decomposition of
  `crates/neovex-bin/src/service/compose.rs`
- concept-owned decomposition of
  `crates/neovex-bin/src/machine/api.rs`
- concept-owned decomposition of
  `crates/neovex-sandbox/src/backends/oci/buildah.rs`
- regression-suite packaging cleanup for
  `crates/neovex-engine/src/tests/mutation_journal.rs`
  and `crates/neovex-sandbox/tests/krun_linux_smoke.rs`
- benchmark harness modularization for the provider benchmark surfaces in
  `crates/neovex-engine/benches/`
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
   `neovex-core` stays zero I/O.
   `neovex-runtime` stays zero workspace dependencies.
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
- The strongest remaining hotspots now fall into three categories:
  - thin production roots that still carry very large inline test modules
  - medium-sized production files that still mix too many concepts in one file
  - oversized benchmark and regression files that now need scenario-owned
    module layout rather than more inline growth
- The current hotspot map from the live worktree is:
  - `crates/neovex-bin/src/machine/mod.rs` is 3,538 lines total and the inline
    test module starts at line 203
  - `crates/neovex-bin/src/machine/manager.rs` is 2,167 lines total and the
    inline test module starts at line 473
  - `crates/neovex-bin/src/service/mod.rs` is 2,540 lines total and the inline
    test module starts at line 770
  - `crates/neovex-sandbox/src/backends/krun/vm.rs` is 1,520 lines total and
    its already-thin production root remains bundled with a large inline
    regression surface
  - `crates/neovex-bin/src/service/compose.rs` is 1,940 lines total and its
    inline test module starts at line 1,434
  - `crates/neovex-bin/src/machine/api.rs` is 1,758 lines total and its inline
    test module starts at line 1,062
  - `crates/neovex-sandbox/src/backends/oci/buildah.rs` is 1,723 lines total
    and its inline test module starts at line 1,047
  - `crates/neovex-engine/src/tests/mutation_journal.rs` is 1,869 lines
  - `crates/neovex-sandbox/tests/krun_linux_smoke.rs` is 1,581 lines
  - `crates/neovex-engine/benches/embedded-provider-benchmarks.rs` is 2,316
    lines
  - `crates/neovex-engine/benches/libsql-replica-provider-benchmarks.rs` is
    2,666 lines
  - `crates/neovex-engine/benches/postgres-provider-benchmarks.rs` is 4,244
    lines
  - `crates/neovex-engine/benches/mysql-provider-benchmarks.rs` is 4,346 lines
- Several notable production files are now below the threshold and already
  organized around acceptable seams:
  - `crates/neovex-storage/src/libsql.rs` is 1,471 lines and already delegates
    through `provider`, `storage`, `read`, `write`, `remote`, and `transport`
  - `crates/neovex-storage/src/mysql.rs` is 1,445 lines and already acts as a
    thin composition root
  - `crates/neovex-storage/src/postgres.rs` is 1,287 lines and already acts as
    a thin composition root

---

## Current Review Findings

1. `crates/neovex-bin/src/machine/mod.rs`,
   `crates/neovex-bin/src/machine/manager.rs`,
   `crates/neovex-bin/src/service/mod.rs`, and
   `crates/neovex-sandbox/src/backends/krun/vm.rs` are no longer the old
   production god files, but they still violate the new size thresholds as
   whole files because their large regression suites remain inline. The
   production ownership is mostly acceptable; the packaging is not.

2. `crates/neovex-bin/src/service/compose.rs` still mixes several concepts in
   one file.
   It owns raw Compose document decoding, project-name normalization,
   service-plan lowering, sandbox-launch lowering, port and env parsing,
   duration parsing, warning generation, and YAML rendering in one module.

3. `crates/neovex-bin/src/machine/api.rs` is still a concept-mixed machine API
   surface.
   It owns router construction, listener setup, systemd socket activation,
   capability probing, helper-binary resolution, sandbox lifecycle handlers,
   persisted-state refresh, log tailing, process inspection, and error mapping
   in one file.

4. `crates/neovex-sandbox/src/backends/oci/buildah.rs` still combines too many
   responsibilities.
   The same file owns the `buildah` CLI wrapper, mount-session lifecycle,
   inspect and image-config parsing, rootfs user resolution, env and exposed
   port merging, and command-failure rendering.

5. `crates/neovex-engine/src/tests/mutation_journal.rs` is a useful but
   oversized regression suite.
   The file is already scenario-rich and valuable, but the journal visibility,
   cancellation, queueing, and subscription scenarios should live in
   scenario-owned submodules with shared helpers instead of one long test file.

6. `crates/neovex-sandbox/tests/krun_linux_smoke.rs` is a similarly oversized
   scenario suite.
   Its size is now mainly proof packaging debt rather than a production
   architecture problem, but it still needs clearer scenario grouping and local
   helpers.

7. The provider benchmark surfaces are now the clearest remaining 2k to 4k
   files in the repo.
   `embedded-provider-benchmarks.rs`,
   `postgres-provider-benchmarks.rs`,
   `mysql-provider-benchmarks.rs`, and
   `libsql-replica-provider-benchmarks.rs`
   all repeat the same benchmark-config, report-rendering, workload-lane, and
   environment-bootstrap patterns. That duplication makes the benchmark suite
   harder to extend and harder to keep consistent across providers.

8. `crates/neovex-storage/src/libsql.rs` is now the main "leave it alone for
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

- `crates/neovex-storage/src/libsql.rs` at 1,471 lines is below the threshold
  and already reads as a concept-owned replica freshness composition root over
  its new submodules. Revisit only if later work pushes new unrelated
  ownership into the root.
- `crates/neovex-storage/src/mysql.rs` at 1,445 lines and
  `crates/neovex-storage/src/postgres.rs` at 1,287 lines are below the
  threshold and already aligned to the clearer provider layout from the
  previous wave.
- `crates/neovex-bin/src/machine/client.rs`,
  `crates/neovex-sandbox/src/backends/container/runtime.rs`,
  `crates/neovex-sandbox/src/backends/oci/builder.rs`, and
  `crates/neovex-sandbox/src/backends/oci/network.rs`
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

- `cargo test -p neovex-bin machine`
- `cargo test -p neovex-bin service`
- `cargo test -p neovex-sandbox`
- `cargo test -p neovex-engine mutation_journal`
- `cargo check -p neovex-engine --benches`
- `cargo clippy -p neovex-bin --all-targets -- -D warnings`
- `cargo clippy -p neovex-sandbox --all-targets -- -D warnings`
- `cargo clippy -p neovex-engine --all-targets -- -D warnings`

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
| CMF1 | `todo` | extract oversized inline regression suites from already-thin roots in `machine/mod.rs`, `machine/manager.rs`, `service/mod.rs`, and `krun/vm.rs` | CMF0 | preserve all current root behavior while moving tests into sibling files or scenario modules |
| CMF2 | `todo` | split `crates/neovex-bin/src/service/compose.rs` into concept-owned project, lowering, parsing, and rendering seams | CMF0 | keep Compose load and render semantics unchanged |
| CMF3 | `todo` | split `crates/neovex-bin/src/machine/api.rs` into router, listener, capability, lifecycle, logs, process, and helper-resolution seams | CMF0 | keep machine API routes and helper behavior unchanged |
| CMF4 | `todo` | split `crates/neovex-sandbox/src/backends/oci/buildah.rs` into CLI/session, inspect/defaults, rootfs-user, parsing, and render/error seams | CMF0 | keep buildah-backed launch semantics unchanged |
| CMF5 | `todo` | repackage `crates/neovex-engine/src/tests/mutation_journal.rs` into scenario-owned submodules with shared helpers | CMF0 | preserve journal visibility and cancellation regression coverage exactly |
| CMF6 | `todo` | repackage `crates/neovex-sandbox/tests/krun_linux_smoke.rs` into scenario-owned smoke modules and helpers | CMF1 recommended first | preserve krun smoke semantics and proof readability |
| CMF7 | `todo` | modularize the provider benchmark harness under `crates/neovex-engine/benches/` and split the 2k to 4k benchmark files into shared harness plus provider-owned workloads | CMF2 through CM4 recommended first | keep benchmark workload definitions and output shape comparable |
| CMF8 | `todo` | update docs, run the full verification sweep, and archive this completed follow-on plan cleanly | CMF1 through CM7 | close out only after all selected files satisfy the threshold rules or have explicit justification |

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
| CMF1 | pending | move the large inline tests out of `machine/mod.rs`, `machine/manager.rs`, `service/mod.rs`, and `krun/vm.rs` into sibling files or scenario modules while keeping the current root APIs intact |
| CMF2 | pending | map `service/compose.rs` into project loading, raw model types, lowerers, parser helpers, and render helpers before moving code |
| CMF3 | pending | map `machine/api.rs` into router/listener, capability resolution, lifecycle handlers, log/process helpers, and binary-resolution seams before moving code |
| CMF4 | pending | map `buildah.rs` into CLI/session lifecycle, inspect/defaults parsing, rootfs user resolution, and render/error seams before moving code |
| CMF5 | pending | group the `mutation_journal.rs` scenarios by cancellation, queueing, visibility, and subscription behavior before extracting helpers |
| CMF6 | pending | group the krun smoke scenarios by launch, inspect, restart, and cleanup behavior before extracting helpers |
| CMF7 | pending | design a shared provider benchmark harness for config parsing, report rendering, workload lanes, and environment bootstrap before moving provider-specific workloads |
| CMF8 | pending | rerun the full verification sweep, update docs for archive state, and move this plan into `docs/plans/archive/` once all selected items land |

---

## Work Items

### CMF0. Baseline review and follow-on plan promotion

#### Outcome

- Completed during this planning pass.

### CMF1. Extract oversized inline regression suites from thin roots

#### Implementation plan

1. Keep the production roots in
   `crates/neovex-bin/src/machine/mod.rs`,
   `crates/neovex-bin/src/machine/manager.rs`,
   `crates/neovex-bin/src/service/mod.rs`, and
   `crates/neovex-sandbox/src/backends/krun/vm.rs`
   as the public composition surfaces.
2. Move the large inline test modules into sibling files or scenario-owned
   test modules with clear ownership and stable imports.
3. Keep the existing root ergonomics, module visibility, and test semantics
   unchanged.

#### Focused verification

- `cargo test -p neovex-bin machine`
- `cargo test -p neovex-bin service`
- `cargo test -p neovex-sandbox`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-bin --all-targets -- -D warnings`
- `cargo clippy -p neovex-sandbox --all-targets -- -D warnings`

#### Acceptance criteria

- the selected roots no longer keep giant inline regression suites in the same
  file as the production composition logic
- file ownership is easier to navigate without reopening settled production
  architecture
- machine, service, and krun behavior remain unchanged

### CMF2. Split `service/compose.rs` into concept-owned modules

#### Implementation plan

1. Keep `crates/neovex-bin/src/service/compose.rs` or a successor
   `service/compose/` directory as the public Compose service-plan surface.
2. Separate raw Compose document types, project/service lowering, parser
   helpers, warning helpers, and rendering helpers into clearer modules.
3. Preserve Compose loading, warning, and rendered plan semantics.

#### Focused verification

- `cargo test -p neovex-bin service`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-bin --all-targets -- -D warnings`

#### Acceptance criteria

- the Compose loader is easier to navigate by concept
- service lowering and parser helpers have canonical homes
- rendered config and warning behavior remain unchanged

### CMF3. Split `machine/api.rs` into concept-owned machine API modules

#### Implementation plan

1. Keep `crates/neovex-bin/src/machine/api.rs` or a successor directory module
   as the machine API composition root.
2. Separate router/listener setup, capability reporting, lifecycle handlers,
   persisted-state refresh, log/process helpers, and binary-resolution helpers
   into clearer modules.
3. Preserve route shape, listener behavior, forwarded container semantics, and
   helper-binary reporting.

#### Focused verification

- `cargo test -p neovex-bin machine`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-bin --all-targets -- -D warnings`

#### Acceptance criteria

- the machine API surface is easier to follow by responsibility
- router and lifecycle behavior remain unchanged
- helper and process-inspection logic are easier to test locally

### CMF4. Split `oci/buildah.rs` into concept-owned backend helper modules

#### Implementation plan

1. Keep `crates/neovex-sandbox/src/backends/oci/buildah.rs` or a successor
   directory module as the public buildah helper surface.
2. Separate the CLI/session wrapper, inspect/defaults parsing, rootfs-user
   resolution, env/port parsing, and render/error helpers into clearer modules.
3. Preserve buildah launch preparation and mount-session semantics exactly.

#### Focused verification

- `cargo test -p neovex-sandbox`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-sandbox --all-targets -- -D warnings`

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

- `cargo test -p neovex-engine mutation_journal`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-engine --all-targets -- -D warnings`

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

- `cargo test -p neovex-sandbox`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-sandbox --all-targets -- -D warnings`

#### Acceptance criteria

- the krun smoke suite is easier to navigate and extend
- smoke semantics stay unchanged
- proof helpers are local to the owning scenarios

### CMF7. Modularize the provider benchmark harness

#### Implementation plan

1. Keep the provider benchmark suite under
   `crates/neovex-engine/benches/`.
2. Extract shared config parsing, report rendering, workload-lane definitions,
   and environment/bootstrap helpers into common benchmark modules.
3. Leave provider-specific workload setup and provider-specific environment
   details close to the benchmark that owns them.
4. Preserve benchmark workload slugs, markdown output shape, and provider
   comparability.

#### Focused verification

- `cargo check -p neovex-engine --benches`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-engine --all-targets -- -D warnings`

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
