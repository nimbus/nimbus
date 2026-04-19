# Codebase Modularity And Maintainability Hotspots Control Plan

This is the canonical execution control plane for the next repo-wide
maintainability, modularity, canonical naming, and idiomatic-Rust cleanup
workstream after the completed follow-on plan archived at
`docs/plans/archive/codebase-modularity-and-maintainability-follow-on-plan.md`.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `AGENTS.md`
- `docs/plans/archive/codebase-modularity-and-maintainability-plan.md`
- `docs/plans/archive/codebase-modularity-and-maintainability-follow-on-plan.md`
- `crates/neovex-bin/src/machine/tests.rs`
- `crates/neovex-bin/src/service/tests.rs`
- `crates/neovex-bin/src/machine/manager/tests.rs`
- `crates/neovex-engine/benches/embedded-provider-benchmarks.rs`
- `crates/neovex-engine/benches/libsql-replica-provider-benchmarks.rs`
- `crates/neovex-engine/benches/postgres_provider_benchmarks/workloads.rs`
- `crates/neovex-engine/benches/mysql_provider_benchmarks/workloads.rs`
- `crates/neovex-sandbox/src/backends/container/runtime.rs`
- `crates/neovex-sandbox/src/backends/oci/builder.rs`
- `crates/neovex-bin/src/machine/client.rs`
- `crates/neovex-engine/src/tests/materialized_serving.rs`
- `crates/neovex-storage/src/libsql.rs`
- the current git worktree on `main`

Baseline verification status for this plan:

- the predecessor follow-on maintainability workstream completed and remains
  archived at
  `docs/plans/archive/codebase-modularity-and-maintainability-follow-on-plan.md`
- this hotspot control plane is being authored as a docs-only review and
  planning pass on 2026-04-19 from a clean worktree after reviewing the live
  post-cleanup codebase against the current file-size and ownership thresholds
- no new code verification is claimed by this planning pass
- every `CMH*` implementation item must record its own focused verification
  before it can be marked `done`

---

## Purpose

The previous maintainability waves succeeded. The large mixed-responsibility
production roots they targeted are now mostly thin composition surfaces or
clear provider-owned modules.

The next wave should stay selective. It should not split files just because
they are long. It should split files when they still mix too many concepts,
hide ownership, or keep benchmark and regression proof surfaces in giant
single files that are hard to extend and debug.

The current hotspot map is now different from the earlier waves:

- the highest-signal remaining Rust hotspots are oversized regression and
  benchmark files, not the old production god files
- the active documentation hotspot is `ARCHITECTURE.md`, which now sits at the
  threshold and mixes stable architecture with deeper reference material that
  can live in focused docs

This plan therefore targets the remaining active hotspots that most clearly
benefit long-term maintainability without inventing work where the ownership is
already clear.

---

## Relationship To Other Plans

- Use `docs/plans/README.md` as the owning plan index.
- Use this plan for broad maintainability, readability, modularity,
  canonical-naming, threshold review, and god-file cleanup work that is not
  already owned by another active plan.
- Use
  `docs/plans/archive/codebase-modularity-and-maintainability-follow-on-plan.md`
  only for the completed follow-on wave's execution record, closeout
  justifications, and benchmark-packaging baseline.
- Use
  `docs/plans/archive/codebase-modularity-and-maintainability-plan.md`
  only for older predecessor checkpoints and the earlier CLI/provider/sandbox
  ownership split history.
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

- scenario-owned repackaging of the oversized regression files
  `crates/neovex-bin/src/machine/tests.rs`,
  `crates/neovex-bin/src/service/tests.rs`, and
  `crates/neovex-bin/src/machine/manager/tests.rs`
- further provider benchmark modularization in
  `crates/neovex-engine/benches/`, especially the remaining
  1,500-to-1,999-line workload and benchmark-root files
- packaging cleanup for the active `ARCHITECTURE.md` document so the stable
  architecture root stays concise and deeper reference material moves into
  focused docs
- doc, verification, and archive cleanup needed to keep this workstream
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
- speculative rewrites that are not justified by ownership, readability, or
  maintainability

---

## Cleanup Invariants

These rules are mandatory for every item in this plan.

1. Preserve externally observable behavior by default.
   Machine CLI behavior, service CLI behavior, machine manager lifecycle
   semantics, benchmark workload behavior, markdown output shape, and the
   architecture ownership map stay unchanged unless a specific item explicitly
   records otherwise.

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
   item closeout; extract production ownership, test ownership, benchmark
   ownership, or reference-doc ownership until the file has a clear reason to
   stay large.

5. Keep proof surfaces easy to navigate.
   Large regression and benchmark suites should group by scenario family,
   workload family, or helper ownership rather than one flat file.

6. Keep `ARCHITECTURE.md` architecture-focused.
   The root architecture doc should own the stable crate map, seams,
   invariants, and major data flows. Deeper reference material can move into
   focused docs under `docs/reference/` when that keeps the root clearer.

7. Treat the current git worktree as baseline reality.
   Resume from the code and this plan's ledger, not from chat memory.

---

## Current Assessed State

- Before this planning pass, the repo again had no active generic cleanup or
  refactor control plane, so future broad maintainability work needed a new
  active owner rather than another revival of archived plans.
- The previous maintainability waves succeeded. The old CLI, provider, krun,
  Compose, machine API, mutation-journal, and smoke-test hotspots were already
  split into thinner production or proof surfaces.
- The strongest remaining active-code hotspots are now concentrated in files
  that sit directly on or above the current review thresholds:
  - `crates/neovex-bin/src/machine/tests.rs` is 3,323 lines
  - `crates/neovex-bin/src/service/tests.rs` is 1,765 lines
  - `crates/neovex-bin/src/machine/manager/tests.rs` is 1,678 lines
  - `crates/neovex-engine/benches/embedded-provider-benchmarks.rs` is
    1,817 lines
  - `crates/neovex-engine/benches/libsql-replica-provider-benchmarks.rs` is
    1,987 lines
  - `crates/neovex-engine/benches/postgres_provider_benchmarks/workloads.rs`
    is 1,602 lines
  - `crates/neovex-engine/benches/mysql_provider_benchmarks/workloads.rs` is
    1,641 lines
  - `ARCHITECTURE.md` is 1,997 lines
- `crates/neovex-bin/src/machine/tests.rs` is now the clearest hard-threshold
  violation in active Rust code. It mixes CLI parse/help coverage, record and
  state persistence, machine OS image or upgrade behavior, output rendering,
  transfer and SSH semantics, forwarded machine API status, and startup-failure
  flows in one file.
- `crates/neovex-bin/src/service/tests.rs` mixes CLI parse/help coverage,
  config and render behavior, log or process helpers, lifecycle start or stop
  flows, backend loading, and forwarded machine API behavior in one file.
- `crates/neovex-bin/src/machine/manager/tests.rs` mixes provider capability
  contracts, bootstrap identity behavior, image materialization, helper
  resolution, readiness waits, stop or cleanup behavior, SSH and SCP command
  behavior, port allocation, and attestation metadata in one file.
- The provider benchmark cleanup is not finished yet. The current bench tree is
  much better than before, but the remaining big files still mix config,
  workload models, orchestration, fixtures, and provider-owned workload logic
  in ways that are harder to extend than they need to be.
- `ARCHITECTURE.md` is now the active documentation hotspot. Its root still
  contains the stable crate map and invariants, but it also carries deeper
  provider-topology, testing, and verification-harness ownership detail that
  can move into focused reference docs without weakening the canonical
  architecture contract.

---

## Current Review Findings

1. `crates/neovex-bin/src/machine/tests.rs` must be split.
   At 3,323 lines it is far above the hard threshold, and its current shape is
   a mixed regression surface rather than one cohesive scenario family.

2. `crates/neovex-bin/src/service/tests.rs` and
   `crates/neovex-bin/src/machine/manager/tests.rs` are both strong
   maintainability targets.
   They are over 1,500 lines, clearly mix distinct scenario families, and have
   enough local helper ownership to justify module trees rather than one flat
   file each.

3. The remaining provider benchmark files now form the main benchmark hotspot
   cluster.
   `postgres_provider_benchmarks/workloads.rs` and
   `mysql_provider_benchmarks/workloads.rs` are both above 1,500 lines, while
   `embedded-provider-benchmarks.rs` and
   `libsql-replica-provider-benchmarks.rs` still keep too much config, model,
   and workload ownership in one root file.

4. `ARCHITECTURE.md` should become thinner by extracting reference detail,
   not by deleting architectural substance.
   The stable crate map, invariants, and data flows should stay in the root;
   detailed provider-topology and verification-architecture material can move
   into focused docs under `docs/reference/`.

5. Several notable sub-threshold files are worth keeping in view, but they are
   not the highest-signal next-wave targets.
   `crates/neovex-sandbox/src/backends/container/runtime.rs`,
   `crates/neovex-sandbox/src/backends/oci/builder.rs`,
   `crates/neovex-bin/src/machine/client.rs`,
   `crates/neovex-engine/src/tests/materialized_serving.rs`, and
   `crates/neovex-storage/src/libsql.rs` are all below the threshold and read
   as more cohesive than the selected hotspots above.

---

## Success Criteria

This plan is successful only when all of the following are true:

- `machine/tests.rs`, `service/tests.rs`, and `machine/manager/tests.rs` no
  longer keep their broad regression coverage in giant flat files
- the provider benchmark tree no longer leaves selected 1,500-to-1,999-line
  files mixing config, models, orchestration, and workload ownership in a
  single file
- `ARCHITECTURE.md` is a clearer stable architecture root with deeper reference
  detail extracted into focused docs where appropriate
- every remaining active file above 1,500 lines has an explicit justification
  in the plan closeout notes, and no selected active file remains above 2,000
  lines for avoidable packaging reasons
- docs, plan status, and archive state accurately reflect the landed work

---

## Assessed But Not Selected

- `crates/neovex-engine/src/tests/materialized_serving.rs` at 1,389 lines is
  below the threshold and already reads as a single concept-owned proof surface
  for materialized-serving behavior. Revisit only if later work pushes new
  unrelated ownership into it.
- `crates/neovex-storage/src/libsql.rs` at 1,471 lines is below the threshold
  and already reads as a concept-owned replica freshness composition root over
  its existing submodules.
- `crates/neovex-sandbox/src/backends/container/runtime.rs` at 1,328 lines,
  `crates/neovex-sandbox/src/backends/oci/builder.rs` at 1,199 lines, and
  `crates/neovex-bin/src/machine/client.rs` at 1,120 lines are notable but
  below the threshold and less urgent than the selected proof and docs
  hotspots.
- Archived plan documents above the threshold are historical control-plane
  records and are out of scope for this wave. Do not rewrite archived plans
  just to reduce their line counts.

---

## Feature Preservation Matrix

| Surface | Preservation Requirement |
| --- | --- |
| Machine CLI tests | parse/help behavior, record-file semantics, status rendering, SSH/SCP behavior, image/OS flows, and startup-failure semantics stay unchanged |
| Service CLI tests | config loading, tenant resolution, backend selection, forwarded machine API behavior, lifecycle operations, logs, and `ps` semantics stay unchanged |
| Machine manager tests | provider capability, bootstrap identity, image materialization, helper resolution, readiness waits, stop/cleanup behavior, SSH/SCP semantics, port allocation, and attestation semantics stay unchanged |
| Provider benchmarks | workload definitions, CLI slugs, markdown output shape, workload-lane semantics, and provider-to-provider comparability stay unchanged unless a benchmark item explicitly records otherwise |
| Architecture docs | stable crate map, data-flow, invariant, and ownership statements stay accurate while deeper reference detail moves into focused docs |

---

## Control Plan Rules

1. Implement exactly one `CMH*` item at a time unless the plan explicitly says
   otherwise.
2. Do not skip ahead while an earlier eligible item remains `todo`.
3. Do not start a new item while another remains `in_progress`.
4. If the worktree is dirty, reconcile it to the owning item before proceeding.
5. Prefer scenario-owned module trees over one giant test or benchmark file.
6. If implementation reveals a materially better seam map, update this plan
   first, then implement it.
7. After every meaningful work burst, update the roadmap ledger, checkpoints,
   and execution log so handoff does not depend on chat memory.

---

## Verification Contract

Every implementation item in this plan must run its focused verification, plus:

- `cargo fmt --all --check`
- `cargo check --workspace`

Use these focused lanes as appropriate:

- `cargo test -p neovex-bin machine`
- `cargo test -p neovex-bin service`
- `cargo check -p neovex-engine --benches`
- `cargo clippy -p neovex-bin --all-targets -- -D warnings`
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
| CMH0 | `done` | reviewed the live post-follow-on codebase, applied the current 1,500/2,000-line threshold rules, and promoted this hotspot maintainability control plane | none | docs-only review and planning pass on 2026-04-19 |
| CMH1 | `todo` | split `crates/neovex-bin/src/machine/tests.rs` into scenario-owned machine CLI regression modules | CMH0 | first hard-threshold Rust hotspot and the clearest next slice |
| CMH2 | `todo` | split `crates/neovex-bin/src/service/tests.rs` into scenario-owned service CLI regression modules | CMH0 | follows the same proof-surface pattern as `CMH1` with lower line-count pressure |
| CMH3 | `todo` | split `crates/neovex-bin/src/machine/manager/tests.rs` into lifecycle-owned manager regression modules | CMH0 | should follow the CLI proof-surface cleanup so the machine-side naming pattern is settled first |
| CMH4 | `todo` | split `crates/neovex-engine/benches/postgres_provider_benchmarks/workloads.rs` and `mysql_provider_benchmarks/workloads.rs` into provider-owned workload module trees | CMH0 | establishes the benchmark workload naming pattern before reopening the remaining large benchmark roots |
| CMH5 | `todo` | split `crates/neovex-engine/benches/embedded-provider-benchmarks.rs` into a thinner benchmark root plus concept-owned modules | CMH4 | should follow the Postgres/MySQL workload packaging so the benchmark module pattern is already proven |
| CMH6 | `todo` | split `crates/neovex-engine/benches/libsql-replica-provider-benchmarks.rs` into a thinner benchmark root plus concept-owned modules | CMH4 | should follow the same benchmark pattern as `CMH5` while keeping libsql-replica coordination local |
| CMH7 | `todo` | repackage `ARCHITECTURE.md` into a thinner stable architecture root plus focused reference docs | CMH1 through CMH6 | should describe the landed proof and benchmark ownership map rather than race ahead of it |
| CMH8 | `todo` | update docs, run the full verification sweep, and archive this completed hotspot plan cleanly | CMH1 through CMH7 | closeout item once every selected hotspot is complete |

---

## Dependency Graph

- `CMH1` is the recommended first slice because it addresses the clearest
  remaining hard-threshold violation in active Rust code.
- `CMH2` and `CMH3` should follow `CMH1` so the large machine/service proof
  surfaces settle into a consistent scenario-owned naming pattern.
- `CMH4` should land before `CMH5` and `CMH6` so the provider-local workload
  module pattern is established before the remaining large benchmark roots are
  thinned further.
- `CMH5` and `CMH6` can then follow the same benchmark packaging discipline
  while keeping provider-specific semantics close to the provider that owns
  them.
- `CMH7` should follow the code cleanup items so the extracted architecture
  references describe the landed ownership map rather than a mid-refactor
  state.
- `CMH8` closes the workstream after the selected regression, benchmark, and
  docs cleanup slices land.

---

## Recommended Delivery Order

1. `CMH1` — machine CLI regression packaging
2. `CMH2` — service CLI regression packaging
3. `CMH3` — machine manager regression packaging
4. `CMH4` — Postgres and MySQL benchmark workload packaging
5. `CMH5` — embedded benchmark root packaging
6. `CMH6` — libsql-replica benchmark root packaging
7. `CMH7` — architecture doc packaging
8. `CMH8` — docs, verification, and archive

---

## Implementation Checkpoints

| Item | Checkpoint | Next Step |
| --- | --- | --- |
| CMH0 | done | start `CMH1` by mapping the scenario families currently mixed together in `crates/neovex-bin/src/machine/tests.rs` and extracting a local `machine/tests/` module tree without changing the machine CLI contract |
| CMH1 | todo | split the machine CLI regression surface into scenario-owned modules for parse/help, rendering, machine records and state, OS/image flows, transfer/SSH, forwarded machine API behavior, startup failures, and local test support |
| CMH2 | todo | split the service CLI regression surface into scenario-owned modules for parse/help, rendered config and tenant resolution, logs/process helpers, lifecycle flows, forwarded machine API behavior, and local test support |
| CMH3 | todo | split the machine manager regression surface into lifecycle-owned modules for provider contracts, bootstrap identity, image materialization, helper resolution, readiness waits, stop/cleanup, port allocation, attestation, and local test support |
| CMH4 | todo | split the Postgres and MySQL benchmark workload files into provider-local module trees that separate workload families, fixture creation, sample execution, and local support without changing benchmark slugs or report shape |
| CMH5 | todo | thin the embedded benchmark root by moving config parsing, benchmark models, suite orchestration, fixtures or seeds, and provider-owned workload helpers into local modules while preserving benchmark semantics |
| CMH6 | todo | thin the libsql-replica benchmark root by moving config or environment parsing, benchmark models, suite orchestration, fixtures, and replica-specific workload helpers into local modules while preserving benchmark semantics |
| CMH7 | todo | keep `ARCHITECTURE.md` as the stable architecture root while extracting deeper provider-topology and verification-architecture detail into focused docs under `docs/reference/` |
| CMH8 | todo | update `AGENTS.md`, `docs/plans/README.md`, and any touched reference indexes for the landed ownership map, then run the full verification sweep and archive this plan |

---

## Work Items

### CMH0. Baseline review and hotspot plan promotion

#### Outcome

- Completed during this planning pass.

### CMH1. Split `machine/tests.rs` into scenario-owned machine CLI modules

#### Implementation plan

1. Keep the machine production root and its current public surface unchanged.
2. Replace the flat `crates/neovex-bin/src/machine/tests.rs` proof surface with
   a local module tree grouped by clear scenario ownership.
3. Expected seams:
   - CLI parse and help coverage
   - machine records, paths, and state persistence
   - render and output-shaping behavior
   - machine OS image and upgrade behavior
   - transfer and SSH helpers
   - forwarded machine API or startup-failure behavior
   - local test support helpers

#### Focused verification

- `cargo test -p neovex-bin machine`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-bin --all-targets -- -D warnings`

#### Acceptance criteria

- the machine CLI regression surface is easier to navigate by scenario family
- no selected machine test file remains above the hard 2,000-line threshold
- machine CLI behavior remains unchanged

### CMH2. Split `service/tests.rs` into scenario-owned service CLI modules

#### Implementation plan

1. Keep the service production root and its current public surface unchanged.
2. Replace the flat `crates/neovex-bin/src/service/tests.rs` proof surface with
   a local module tree grouped by clear scenario ownership.
3. Expected seams:
   - CLI parse and help coverage
   - rendered config, inspect, and tenant-resolution behavior
   - log and process helper behavior
   - lifecycle start and stop behavior
   - backend loading and forwarded machine API behavior
   - local test support helpers

#### Focused verification

- `cargo test -p neovex-bin service`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-bin --all-targets -- -D warnings`

#### Acceptance criteria

- the service CLI regression surface is easier to navigate by scenario family
- file ownership is clearer without changing behavior
- service CLI semantics remain unchanged

### CMH3. Split `machine/manager/tests.rs` into lifecycle-owned modules

#### Implementation plan

1. Keep the machine manager production tree and its current public surface
   unchanged.
2. Replace the flat
   `crates/neovex-bin/src/machine/manager/tests.rs` proof surface with a local
   module tree grouped by lifecycle or helper ownership.
3. Expected seams:
   - provider capability and bootstrap contract behavior
   - image materialization behavior
   - helper resolution behavior
   - readiness waits and startup interruption behavior
   - stop and cleanup behavior
   - SSH or SCP command behavior
   - port allocation and state refresh behavior
   - attestation metadata behavior
   - local test support helpers

#### Focused verification

- `cargo test -p neovex-bin machine`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-bin --all-targets -- -D warnings`

#### Acceptance criteria

- the machine manager regression surface is easier to navigate by lifecycle
  ownership
- no selected machine-manager test file remains above the hard 2,000-line
  threshold
- provider and lifecycle behavior remain unchanged

### CMH4. Split the Postgres and MySQL benchmark workload files

#### Implementation plan

1. Keep the provider benchmark suite under
   `crates/neovex-engine/benches/`.
2. Replace
   `postgres_provider_benchmarks/workloads.rs`
   and
   `mysql_provider_benchmarks/workloads.rs`
   with provider-local module trees grouped by workload family and local helper
   ownership.
3. Expected seams:
   - CRUD workload behavior
   - point-read workload behavior
   - indexed-query workload behavior
   - mixed-load workload behavior
   - fixture creation and seed freezing
   - sample exercise helpers and measurement recording

#### Focused verification

- `cargo check -p neovex-engine --benches`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-engine --all-targets -- -D warnings`

#### Acceptance criteria

- the Postgres and MySQL workload trees are easier to navigate by workload
  family
- benchmark slugs, workload semantics, and report shape remain unchanged
- every selected benchmark workload file now satisfies the threshold rules or
  has explicit justification

### CMH5. Split `embedded-provider-benchmarks.rs` into a thinner benchmark root

#### Implementation plan

1. Keep the embedded benchmark entrypoint under
   `crates/neovex-engine/benches/`.
2. Thin `crates/neovex-engine/benches/embedded-provider-benchmarks.rs` by
   moving non-entrypoint ownership into local modules.
3. Expected seams:
   - CLI/config parsing
   - workload and lane enums
   - benchmark report or measurement models
   - suite orchestration
   - fixtures and seed types
   - provider-owned workload helpers

#### Focused verification

- `cargo check -p neovex-engine --benches`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-engine --all-targets -- -D warnings`

#### Acceptance criteria

- the embedded benchmark root reads as a thinner composition surface
- provider-owned workload semantics stay local and unchanged
- the selected root either falls below 1,500 lines or records an explicit
  closeout justification if it remains above that threshold for a good reason

### CMH6. Split `libsql-replica-provider-benchmarks.rs` into a thinner benchmark root

#### Implementation plan

1. Keep the libsql-replica benchmark entrypoint under
   `crates/neovex-engine/benches/`.
2. Thin
   `crates/neovex-engine/benches/libsql-replica-provider-benchmarks.rs`
   by moving non-entrypoint ownership into local modules.
3. Expected seams:
   - CLI/config parsing
   - environment and admin-API setup
   - workload and lane enums
   - benchmark report or measurement models
   - suite orchestration
   - tenant fixtures and resource helpers
   - provider-owned replica coordination or workload helpers

#### Focused verification

- `cargo check -p neovex-engine --benches`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p neovex-engine --all-targets -- -D warnings`

#### Acceptance criteria

- the libsql-replica benchmark root reads as a thinner composition surface
- replica coordination semantics remain local and unchanged
- the selected root either falls below 1,500 lines or records an explicit
  closeout justification if it remains above that threshold for a good reason

### CMH7. Repackage `ARCHITECTURE.md` into a thinner stable root plus reference docs

#### Implementation plan

1. Keep `ARCHITECTURE.md` as the canonical stable architecture root.
2. Extract deeper provider-topology and verification-architecture material into
   focused docs under `docs/reference/` where that improves readability without
   weakening the ownership map.
3. Update cross-links, plan indexes, and doc references so the new docs are
   discoverable and unambiguous.

#### Focused verification

- `cargo fmt --all --check`
- `cargo check --workspace`

#### Acceptance criteria

- `ARCHITECTURE.md` remains the canonical stable architecture document
- extracted docs own the deeper reference detail clearly
- the resulting docs have no ownership-map conflicts or stale cross-links

### CMH8. Docs, verification, and archive closeout

#### Implementation plan

1. Update `AGENTS.md`, `docs/plans/README.md`, and any touched doc indexes so
   the landed ownership map is discoverable.
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
| 2026-04-19 | CMH0 | `done` | Reviewed the live repo after the completed follow-on maintainability wave and promoted this hotspot-focused plan as the new active control plane. The review found that the strongest remaining active hotspots are now `crates/neovex-bin/src/machine/tests.rs` at 3,323 lines, `crates/neovex-bin/src/service/tests.rs` at 1,765 lines, `crates/neovex-bin/src/machine/manager/tests.rs` at 1,678 lines, the remaining provider benchmark files from 1,602 through 1,987 lines, and `ARCHITECTURE.md` at 1,997 lines. The review also confirmed that notable production files below the threshold are not yet as urgent as the selected proof, benchmark, and doc surfaces. | docs-only review; no new code verification claimed | start `CMH1` by mapping the scenario families currently mixed into `crates/neovex-bin/src/machine/tests.rs` and extracting a local `machine/tests/` module tree |
