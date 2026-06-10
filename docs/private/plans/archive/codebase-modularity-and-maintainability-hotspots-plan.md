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
- `crates/nimbus-bin/src/machine/tests.rs`
- `crates/nimbus-bin/src/service/tests.rs`
- `crates/nimbus-bin/src/machine/manager/tests.rs`
- `crates/nimbus-engine/benches/embedded-provider-benchmarks.rs`
- `crates/nimbus-engine/benches/libsql-replica-provider-benchmarks.rs`
- `crates/nimbus-engine/benches/postgres_provider_benchmarks/workloads.rs`
- `crates/nimbus-engine/benches/mysql_provider_benchmarks/workloads.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime.rs`
- `crates/nimbus-sandbox/src/backends/oci/builder.rs`
- `crates/nimbus-bin/src/machine/client.rs`
- `crates/nimbus-engine/src/tests/materialized_serving.rs`
- `crates/nimbus-storage/src/libsql.rs`
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

- scenario-owned repackaging of the oversized regression files
  `crates/nimbus-bin/src/machine/tests.rs`,
  `crates/nimbus-bin/src/service/tests.rs`, and
  `crates/nimbus-bin/src/machine/manager/tests.rs`
- further provider benchmark modularization in
  `crates/nimbus-engine/benches/`, especially the remaining
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
- `ARCHITECTURE.md` is now 1,694 lines after the provider-topology and
  verification-architecture reference material moved into
  `docs/reference/provider-topologies.md` and
  `docs/reference/verification-architecture.md`
- `crates/nimbus-bin/src/machine/tests.rs` has already been repackaged into a
  19-line composition root over scenario-owned `machine/tests/` modules, so it
  no longer counts as an active hotspot.
- `crates/nimbus-bin/src/service/tests.rs` has already been repackaged into a
  39-line composition root over scenario-owned `service/tests/` modules for
  parse/help, rendered state, logs/process helpers, lifecycle, forwarded
  machine API behavior, and support ownership.
- `crates/nimbus-bin/src/machine/manager/tests.rs` has already been repackaged
  into a 57-line composition root over lifecycle-owned `machine/manager/tests/`
  modules for provider/bootstrap, launch/image, helper resolution,
  readiness/startup, stop/cleanup, ports/state, SSH/SCP, attestation, and
  support ownership.
- The provider-local Postgres and MySQL workload packaging has landed.
  `crates/nimbus-engine/benches/postgres_provider_benchmarks/workloads.rs`
  and `mysql_provider_benchmarks/workloads.rs` are now 27-line composition
  roots over workload-family modules:
  - Postgres: `crud.rs` (153), `journal.rs` (226), `reads.rs` (493),
    `subscription.rs` (391), and `tenant.rs` (343)
  - MySQL: `crud.rs` (153), `journal.rs` (220), `reads.rs` (493),
    `subscription.rs` (391), and `tenant.rs` (388)
- The provider benchmark cleanup is not finished yet. The remaining benchmark
  hotspot files still mix config, workload models, orchestration, fixtures,
  and provider-owned workload logic in single roots that are harder to extend
  than they need to be.
- The embedded provider benchmark root has also been repackaged.
  `crates/nimbus-engine/benches/embedded-provider-benchmarks.rs` is now a
  79-line composition root over `config.rs` (168), `models.rs` (180),
  `suite.rs` (97), `workloads.rs` (700), `fixtures.rs` (412),
  `scenarios.rs` (248), `support.rs` (152), and `report.rs` (208).
- The libsql-replica benchmark root has now been repackaged as well.
  `crates/nimbus-engine/benches/libsql-replica-provider-benchmarks.rs` is now
  a 90-line composition root over `config.rs` (240), `models.rs` (126),
  `suite.rs` (50), `workloads.rs` (670), `fixtures.rs` (669),
  `scenarios.rs` (240), `support.rs` (232), and `report.rs` (269).
- `ARCHITECTURE.md` is no longer carrying the deepest provider-topology or
  verification-harness detail. Those references now live in
  `docs/reference/provider-topologies.md` and
  `docs/reference/verification-architecture.md`. The root still sits above the
  1,500-line review threshold, but that is now explicitly justified: it
  intentionally remains the one canonical stable architecture root for the
  crate map, architecture invariants, key data flows, persistence engine
  layouts, and durable design-decision record.
- `ARCHITECTURE.md` is now the active documentation hotspot. Its root still
  contains the stable crate map and invariants, but it also carries deeper
  provider-topology, testing, and verification-harness ownership detail that
  can move into focused reference docs without weakening the canonical
  architecture contract.

---

## Current Review Findings

1. The selected benchmark hotspot work is now complete.
   The Postgres/MySQL workload trees, the embedded benchmark root, and the
   libsql-replica benchmark root are now all packaged by concept instead of
   leaving 1,500-to-1,999-line benchmark roots in place.

2. `ARCHITECTURE.md` has become thinner by extracting reference detail,
   not by deleting architectural substance.
   The stable crate map, invariants, and data flows stay in the root, while
   detailed provider-topology and verification-architecture material now lives
   in focused docs under `docs/reference/`.

3. Several notable sub-threshold files are worth keeping in view, but they are
   not the highest-signal next-wave targets.
   The newly split Postgres/MySQL workload modules plus the embedded and
   libsql-replica benchmark modules are all below the hard threshold and now
   read as cohesive ownership surfaces, so they do not need further breakup in
   this wave.
   `crates/nimbus-sandbox/src/backends/container/runtime.rs`,
   `crates/nimbus-sandbox/src/backends/oci/builder.rs`,
   `crates/nimbus-bin/src/machine/client.rs`,
   `crates/nimbus-engine/src/tests/materialized_serving.rs`, and
   `crates/nimbus-storage/src/libsql.rs` are all below the threshold and read
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

- `crates/nimbus-engine/src/tests/materialized_serving.rs` at 1,389 lines is
  below the threshold and already reads as a single concept-owned proof surface
  for materialized-serving behavior. Revisit only if later work pushes new
  unrelated ownership into it.
- `crates/nimbus-storage/src/libsql.rs` at 1,471 lines is below the threshold
  and already reads as a concept-owned replica freshness composition root over
  its existing submodules.
- `crates/nimbus-sandbox/src/backends/container/runtime.rs` at 1,328 lines,
  `crates/nimbus-sandbox/src/backends/oci/builder.rs` at 1,199 lines, and
  `crates/nimbus-bin/src/machine/client.rs` at 1,120 lines are notable but
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

- `cargo test -p nimbus-bin machine`
- `cargo test -p nimbus-bin service`
- `cargo check -p nimbus-engine --benches`
- `cargo clippy -p nimbus-bin --all-targets -- -D warnings`
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
| CMH0 | `done` | reviewed the live post-follow-on codebase, applied the current 1,500/2,000-line threshold rules, and promoted this hotspot maintainability control plane | none | docs-only review and planning pass on 2026-04-19 |
| CMH1 | `done` | split `crates/nimbus-bin/src/machine/tests.rs` into scenario-owned machine CLI regression modules | CMH0 | completed on 2026-04-19 with a 19-line composition root over parse/help, records/state, render, OS/image, transfer/SSH, forwarded machine API, startup-failure, and support modules |
| CMH2 | `done` | split `crates/nimbus-bin/src/service/tests.rs` into scenario-owned service CLI regression modules | CMH0 | completed on 2026-04-19 with a 39-line composition root over parse/help, rendered state, logs/process, lifecycle, forwarded machine API, and support modules |
| CMH3 | `done` | split `crates/nimbus-bin/src/machine/manager/tests.rs` into lifecycle-owned manager regression modules | CMH0 | completed on 2026-04-19 with a 57-line composition root over provider/bootstrap, launch/image, helper resolution, readiness/startup, stop/cleanup, ports/state, SSH/SCP, attestation, and support modules |
| CMH4 | `done` | split `crates/nimbus-engine/benches/postgres_provider_benchmarks/workloads.rs` and `mysql_provider_benchmarks/workloads.rs` into provider-owned workload module trees | CMH0 | completed on 2026-04-19 with 27-line composition roots over workload-family modules for CRUD, reads, journal, subscription, and tenant behavior |
| CMH5 | `done` | split `crates/nimbus-engine/benches/embedded-provider-benchmarks.rs` into a thinner benchmark root plus concept-owned modules | CMH4 | completed on 2026-04-19 with a 79-line composition root over config, models, suite, workloads, fixtures, scenarios, support, and report modules |
| CMH6 | `done` | split `crates/nimbus-engine/benches/libsql-replica-provider-benchmarks.rs` into a thinner benchmark root plus concept-owned modules | CMH4 | completed on 2026-04-19 with a 90-line composition root over config, models, suite, workloads, fixtures, scenarios, support, and report modules |
| CMH7 | `done` | repackage `ARCHITECTURE.md` into a thinner stable architecture root plus focused reference docs | CMH1 through CMH6 | completed on 2026-04-19 by extracting provider-topology and verification-architecture detail into `docs/reference/` while keeping the architecture root canonical and explicitly justified above 1,500 lines |
| CMH8 | `done` | updated docs, ran the full verification sweep, and archived this completed hotspot plan cleanly | CMH1 through CMH7 | completed on 2026-04-19 with a green closeout sweep, archive-state pointer reconciliation, and explicit carry-forward guidance to promote a new active plan before another broad maintainability wave |

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
| CMH0 | done | start `CMH1` by mapping the scenario families currently mixed together in `crates/nimbus-bin/src/machine/tests.rs` and extracting a local `machine/tests/` module tree without changing the machine CLI contract |
| CMH1 | done | split `crates/nimbus-bin/src/machine/tests.rs` into a 19-line composition root plus `tests/parse_help.rs` (784 lines), `records_state.rs` (699), `render.rs` (752), `os_image.rs` (256), `transfer_ssh.rs` (167), `forwarded_api.rs` (337), `startup_failures.rs` (230), and `support.rs` (98), preserving machine CLI behavior while eliminating the old 3,323-line mixed proof surface |
| CMH2 | done | split `crates/nimbus-bin/src/service/tests.rs` into a 39-line composition root plus `tests/parse_help.rs` (304 lines), `render_state.rs` (139), `logs_process.rs` (187), `lifecycle.rs` (133), `forwarded_api.rs` (607), and `support.rs` (375), preserving service CLI behavior while eliminating the old 1,765-line mixed proof surface |
| CMH3 | done | split `crates/nimbus-bin/src/machine/manager/tests.rs` into a 57-line composition root plus `tests/provider_bootstrap.rs` (138 lines), `launch_image.rs` (342), `helper_resolution.rs` (136), `readiness_startup.rs` (265), `stop_cleanup.rs` (164), `ports_state.rs` (135), `ssh_scp.rs` (195), `attestation.rs` (53), and `support.rs` (215), preserving manager semantics while eliminating the old 1,678-line mixed proof surface |
| CMH4 | done | split the Postgres and MySQL provider workload roots into 27-line composition surfaces over workload-family modules: Postgres `crud.rs` (153), `journal.rs` (226), `reads.rs` (493), `subscription.rs` (391), `tenant.rs` (343); MySQL `crud.rs` (153), `journal.rs` (220), `reads.rs` (493), `subscription.rs` (391), `tenant.rs` (388). Benchmark slugs, workload semantics, and markdown report shape remained unchanged while removing both 1,600-line mixed-owner workload files. |
| CMH5 | done | split `crates/nimbus-engine/benches/embedded-provider-benchmarks.rs` into a 79-line composition root over `config.rs` (168), `models.rs` (180), `suite.rs` (97), `workloads.rs` (700), `fixtures.rs` (412), `scenarios.rs` (248), `support.rs` (152), and `report.rs` (208). The old 1,817-line mixed-owner benchmark root is gone, every new embedded benchmark module is below the hard threshold, and benchmark semantics plus markdown output stayed unchanged. |
| CMH6 | done | split `crates/nimbus-engine/benches/libsql-replica-provider-benchmarks.rs` into a 90-line composition root over `config.rs` (240), `models.rs` (126), `suite.rs` (50), `workloads.rs` (670), `fixtures.rs` (669), `scenarios.rs` (240), `support.rs` (232), and `report.rs` (269). The old 1,987-line mixed-owner benchmark root is gone, every new libsql-replica benchmark module is below the hard threshold, and benchmark semantics plus markdown output stayed unchanged. |
| CMH7 | done | extracted the deepest provider-topology and verification-harness material out of `ARCHITECTURE.md` into `docs/reference/provider-topologies.md` and `docs/reference/verification-architecture.md`, updated `docs/README.md` to surface those new references, and reduced the architecture root from 1,997 lines to 1,694. The root remains above the 1,500-line review threshold, but that is now explicitly justified because it still owns the crate map, invariants, key data flows, persistence engine layouts, and durable design-decision record as the canonical stable architecture document. |
| CMH8 | done | completed the closeout sweep, reconciled `AGENTS.md` plus `docs/plans/README.md` to treat this hotspot wave as archived historical context, and archived the control plane cleanly after recording the green verification bundle (`make check`, `make test`, `make clippy`, `npm run test --workspaces --if-present`, `npm run build --workspaces --if-present`, and `make ci`) |

---

## Work Items

### CMH0. Baseline review and hotspot plan promotion

#### Outcome

- Completed during this planning pass.

### CMH1. Split `machine/tests.rs` into scenario-owned machine CLI modules

#### Implementation plan

1. Keep the machine production root and its current public surface unchanged.
2. Replace the flat `crates/nimbus-bin/src/machine/tests.rs` proof surface with
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

- `cargo test -p nimbus-bin machine`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-bin --all-targets -- -D warnings`

#### Acceptance criteria

- the machine CLI regression surface is easier to navigate by scenario family
- no selected machine test file remains above the hard 2,000-line threshold
- machine CLI behavior remains unchanged

### CMH2. Split `service/tests.rs` into scenario-owned service CLI modules

#### Implementation plan

1. Keep the service production root and its current public surface unchanged.
2. Replace the flat `crates/nimbus-bin/src/service/tests.rs` proof surface with
   a local module tree grouped by clear scenario ownership.
3. Expected seams:
   - CLI parse and help coverage
   - rendered config, inspect, and tenant-resolution behavior
   - log and process helper behavior
   - lifecycle start and stop behavior
   - backend loading and forwarded machine API behavior
   - local test support helpers

#### Focused verification

- `cargo test -p nimbus-bin service`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-bin --all-targets -- -D warnings`

#### Acceptance criteria

- the service CLI regression surface is easier to navigate by scenario family
- file ownership is clearer without changing behavior
- service CLI semantics remain unchanged

### CMH3. Split `machine/manager/tests.rs` into lifecycle-owned modules

#### Implementation plan

1. Keep the machine manager production tree and its current public surface
   unchanged.
2. Replace the flat
   `crates/nimbus-bin/src/machine/manager/tests.rs` proof surface with a local
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

- `cargo test -p nimbus-bin machine`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-bin --all-targets -- -D warnings`

#### Acceptance criteria

- the machine manager regression surface is easier to navigate by lifecycle
  ownership
- no selected machine-manager test file remains above the hard 2,000-line
  threshold
- provider and lifecycle behavior remain unchanged

### CMH4. Split the Postgres and MySQL benchmark workload files

#### Implementation plan

1. Keep the provider benchmark suite under
   `crates/nimbus-engine/benches/`.
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

- `cargo check -p nimbus-engine --benches`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-engine --all-targets -- -D warnings`

#### Acceptance criteria

- the Postgres and MySQL workload trees are easier to navigate by workload
  family
- benchmark slugs, workload semantics, and report shape remain unchanged
- every selected benchmark workload file now satisfies the threshold rules or
  has explicit justification

### CMH5. Split `embedded-provider-benchmarks.rs` into a thinner benchmark root

#### Implementation plan

1. Keep the embedded benchmark entrypoint under
   `crates/nimbus-engine/benches/`.
2. Thin `crates/nimbus-engine/benches/embedded-provider-benchmarks.rs` by
   moving non-entrypoint ownership into local modules.
3. Expected seams:
   - CLI/config parsing
   - workload and lane enums
   - benchmark report or measurement models
   - suite orchestration
   - fixtures and seed types
   - provider-owned workload helpers

#### Focused verification

- `cargo check -p nimbus-engine --benches`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-engine --all-targets -- -D warnings`

#### Acceptance criteria

- the embedded benchmark root reads as a thinner composition surface
- provider-owned workload semantics stay local and unchanged
- the selected root either falls below 1,500 lines or records an explicit
  closeout justification if it remains above that threshold for a good reason

### CMH6. Split `libsql-replica-provider-benchmarks.rs` into a thinner benchmark root

#### Implementation plan

1. Keep the libsql-replica benchmark entrypoint under
   `crates/nimbus-engine/benches/`.
2. Thin
   `crates/nimbus-engine/benches/libsql-replica-provider-benchmarks.rs`
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

- `cargo check -p nimbus-engine --benches`
- `cargo fmt --all --check`
- `cargo check --workspace`
- `cargo clippy -p nimbus-engine --all-targets -- -D warnings`

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
| 2026-04-19 | CMH0 | `done` | Reviewed the live repo after the completed follow-on maintainability wave and promoted this hotspot-focused plan as the new active control plane. The review found that the strongest remaining active hotspots are now `crates/nimbus-bin/src/machine/tests.rs` at 3,323 lines, `crates/nimbus-bin/src/service/tests.rs` at 1,765 lines, `crates/nimbus-bin/src/machine/manager/tests.rs` at 1,678 lines, the remaining provider benchmark files from 1,602 through 1,987 lines, and `ARCHITECTURE.md` at 1,997 lines. The review also confirmed that notable production files below the threshold are not yet as urgent as the selected proof, benchmark, and doc surfaces. | docs-only review; no new code verification claimed | start `CMH1` by mapping the scenario families currently mixed into `crates/nimbus-bin/src/machine/tests.rs` and extracting a local `machine/tests/` module tree |
| 2026-04-19 | CMH1 | `in_progress` | Reconciled the clean worktree and resumed the first eligible hotspot item from the live control plan. Began the machine CLI proof-surface split by mapping the scenario clusters currently mixed into `crates/nimbus-bin/src/machine/tests.rs` so the replacement `machine/tests/` tree can follow real ownership boundaries instead of line-count-only chunks. | `git status --short --branch`; plan reconciliation; targeted `sed` and `rg` reads over `crates/nimbus-bin/src/machine/tests.rs` and the owning machine module | extract the local `machine/tests/` module tree, wire the new support module, and then run the focused `nimbus-bin` verification lanes |
| 2026-04-19 | CMH1 | `done` | Repackaged `crates/nimbus-bin/src/machine/tests.rs` into a 19-line composition root over `tests/parse_help.rs`, `records_state.rs`, `render.rs`, `os_image.rs`, `transfer_ssh.rs`, `forwarded_api.rs`, `startup_failures.rs`, and `support.rs`. The old 3,323-line mixed proof surface is gone, every new machine test file is below the hard threshold, and the split now follows the intended scenario families without changing machine CLI behavior. | `cargo fmt --all`; `cargo test -p nimbus-bin machine`; `cargo check --workspace`; `cargo clippy -p nimbus-bin --all-targets -- -D warnings`; `cargo fmt --all --check` | start `CMH2` by mapping the scenario families currently mixed into `crates/nimbus-bin/src/service/tests.rs` and extracting a local `service/tests/` module tree |
| 2026-04-19 | CMH2 | `in_progress` | Began the next eligible hotspot item immediately after closing `CMH1`. The target is `crates/nimbus-bin/src/service/tests.rs`, which still mixes CLI parse/help coverage, rendered config and tenant resolution, log and process helpers, lifecycle flows, backend loading, and forwarded machine API behavior in one file. | plan reconciliation after closing `CMH1`; upcoming targeted `sed` and `rg` reads over `crates/nimbus-bin/src/service/tests.rs` and the owning service modules | extract the local `service/tests/` module tree and run the focused `nimbus-bin` service verification lanes |
| 2026-04-19 | CMH2 | `done` | Repackaged `crates/nimbus-bin/src/service/tests.rs` into a 39-line composition root over `tests/parse_help.rs`, `render_state.rs`, `logs_process.rs`, `lifecycle.rs`, `forwarded_api.rs`, and `support.rs`. The old 1,765-line mixed proof surface is gone, every new service test file is below the hard threshold, and the split now follows the intended service scenario families without changing the service CLI contract. | `cargo fmt --all`; `cargo test -p nimbus-bin service`; `cargo check --workspace`; `cargo clippy -p nimbus-bin --all-targets -- -D warnings`; `cargo fmt --all --check` | start `CMH3` by mapping the scenario families currently mixed into `crates/nimbus-bin/src/machine/manager/tests.rs` and extracting a local `machine/manager/tests/` module tree |
| 2026-04-19 | CMH3 | `in_progress` | Began the next eligible hotspot item immediately after closing `CMH2`. The target is `crates/nimbus-bin/src/machine/manager/tests.rs`, which now stands as the clearest remaining proof-surface hotspot and still mixes provider capability contracts, bootstrap identity, image materialization, helper resolution, readiness or startup interruption, stop or cleanup behavior, SSH or SCP coverage, port allocation or state refresh, attestation metadata, and local support helpers in one file. | plan reconciliation after closing `CMH2`; upcoming targeted `sed` and `rg` reads over `crates/nimbus-bin/src/machine/manager/tests.rs` and the owning manager modules | extract the local `machine/manager/tests/` module tree and run the focused manager-side `nimbus-bin` verification lanes |
| 2026-04-19 | CMH3 | `done` | Repackaged `crates/nimbus-bin/src/machine/manager/tests.rs` into a 57-line composition root over `tests/provider_bootstrap.rs`, `launch_image.rs`, `helper_resolution.rs`, `readiness_startup.rs`, `stop_cleanup.rs`, `ports_state.rs`, `ssh_scp.rs`, `attestation.rs`, and `support.rs`. The old 1,678-line mixed proof surface is gone, every new manager test file is below the hard threshold, and the split now follows the manager’s real lifecycle seams without changing machine-manager semantics. | `cargo fmt --all`; `cargo test -p nimbus-bin machine`; `cargo check --workspace`; `cargo clippy -p nimbus-bin --all-targets -- -D warnings`; `cargo fmt --all --check` | start `CMH4` by mapping the workload families currently mixed into the Postgres and MySQL provider benchmark files and extracting provider-local module trees |
| 2026-04-19 | CMH4 | `in_progress` | Began the next eligible hotspot item immediately after closing `CMH3`. The target is the pair of provider workload files `crates/nimbus-engine/benches/postgres_provider_benchmarks/workloads.rs` and `crates/nimbus-engine/benches/mysql_provider_benchmarks/workloads.rs`, which still mix CRUD, point-read, indexed-query, mixed-load, fixture or seed ownership, measurement recording, and local helpers in single provider files. | plan reconciliation after closing `CMH3`; upcoming targeted `sed` and `rg` reads over the Postgres/MySQL benchmark modules | extract provider-local workload module trees and run the focused benchmark verification lanes |
| 2026-04-19 | CMH4 | `done` | Repackaged both provider workload hotspots into 27-line composition roots over provider-local workload-family modules. Postgres now routes through `workloads/crud.rs`, `journal.rs`, `reads.rs`, `subscription.rs`, and `tenant.rs`; MySQL now mirrors the same family layout. The old 1,602-line and 1,641-line mixed-owner workload files are gone, every new workload-family file is below the hard threshold, and benchmark slugs plus markdown output shape stayed unchanged. | `cargo fmt --all`; `cargo check -p nimbus-engine --benches`; `cargo check --workspace`; `cargo clippy -p nimbus-engine --all-targets -- -D warnings`; `cargo fmt --all --check` | start `CMH5` by mapping the ownership seams currently mixed together in `crates/nimbus-engine/benches/embedded-provider-benchmarks.rs` |
| 2026-04-19 | CMH5 | `in_progress` | Began the next eligible hotspot item immediately after closing `CMH4`. The target is `crates/nimbus-engine/benches/embedded-provider-benchmarks.rs`, which still mixes CLI and config parsing, workload and lane models, report or measurement types, suite orchestration, fixtures or seed ownership, and provider-owned workload helpers in one 1,817-line benchmark root. | plan reconciliation after closing `CMH4`; upcoming targeted `sed` and `rg` reads over the embedded benchmark entrypoint and its existing sibling benchmark modules | extract a thinner embedded benchmark root plus concept-owned local modules, then run the focused benchmark verification lanes |
| 2026-04-19 | CMH5 | `done` | Repackaged `crates/nimbus-engine/benches/embedded-provider-benchmarks.rs` into a 79-line composition root over `embedded_provider_benchmarks/config.rs`, `models.rs`, `suite.rs`, `workloads.rs`, `fixtures.rs`, `scenarios.rs`, `support.rs`, and `report.rs`. The old 1,817-line mixed-owner benchmark root is gone, every new embedded benchmark module is below the hard threshold, and benchmark semantics plus markdown output stayed unchanged. | `cargo fmt --all`; `cargo check -p nimbus-engine --benches`; `cargo check --workspace`; `cargo clippy -p nimbus-engine --all-targets -- -D warnings`; `cargo fmt --all --check` | start `CMH6` by mapping the ownership seams currently mixed together in `crates/nimbus-engine/benches/libsql-replica-provider-benchmarks.rs` |
| 2026-04-19 | CMH6 | `in_progress` | Began the next eligible hotspot item immediately after closing `CMH5`. The target is `crates/nimbus-engine/benches/libsql-replica-provider-benchmarks.rs`, which still mixes CLI and config parsing, environment and admin-API setup, workload and lane models, report or measurement types, suite orchestration, fixtures or tenant resources, and replica-specific coordination in one 1,987-line benchmark root. | plan reconciliation after closing `CMH5`; upcoming targeted `sed` and `rg` reads over the libsql-replica benchmark entrypoint and its existing sibling benchmark modules | extract a thinner libsql-replica benchmark root plus concept-owned local modules, then run the focused benchmark verification lanes |
| 2026-04-19 | CMH6 | `done` | Repackaged `crates/nimbus-engine/benches/libsql-replica-provider-benchmarks.rs` into a 90-line composition root over `libsql_replica_provider_benchmarks/config.rs`, `models.rs`, `suite.rs`, `workloads.rs`, `fixtures.rs`, `scenarios.rs`, `support.rs`, and `report.rs`. The old 1,987-line mixed-owner benchmark root is gone, every new libsql-replica benchmark module is below the hard threshold, and benchmark semantics plus markdown output stayed unchanged. | `cargo fmt --all`; `cargo check -p nimbus-engine --benches`; `cargo check --workspace`; `cargo clippy -p nimbus-engine --all-targets -- -D warnings`; `cargo fmt --all --check` | start `CMH7` by mapping the stable-vs-reference ownership seams currently mixed together in `ARCHITECTURE.md` |
| 2026-04-19 | CMH7 | `in_progress` | Began the next eligible hotspot item immediately after closing `CMH6`. The target is `ARCHITECTURE.md`, which still mixes the canonical crate map and invariants with deeper provider-topology and verification-architecture reference detail that can move into focused docs without weakening the stable architecture root. | plan reconciliation after closing `CMH6`; upcoming targeted `sed` and `rg` reads over `ARCHITECTURE.md`, `docs/reference/`, and the plan/readme indexes that discover architecture docs | extract focused reference docs, cross-link them, and then run the focused workspace verification lanes for doc packaging |
| 2026-04-19 | CMH7 | `done` | Repackaged `ARCHITECTURE.md` into a thinner stable architecture root and two focused reference docs: `docs/reference/provider-topologies.md` and `docs/reference/verification-architecture.md`. The architecture root dropped from 1,997 lines to 1,694 lines, `docs/README.md` now surfaces the extracted references directly, and the remaining root length is explicitly justified because it still owns the canonical crate map, invariants, key data flows, persistence engine layouts, and durable design-decision record. | `cargo fmt --all --check`; `cargo check --workspace` | start `CMH8` by reconciling plan indexes and running the full verification sweep before archiving this control plane |
| 2026-04-19 | CMH8 | `in_progress` | Began the final closeout item immediately after closing `CMH7`. The remaining work is to reconcile doc indexes and control-plane pointers, run the full verification sweep, record any environmental limits, archive this completed hotspot plan, and leave the repo with no plan/code mismatch for the maintainability hotspot workstream. | plan reconciliation after closing `CMH7`; upcoming reads over `docs/plans/README.md`, `AGENTS.md`, and the active plan index plus the full verification command list | update doc indexes and active-plan pointers, run the closeout verification sweep, archive the completed plan, and reconcile final status fields |
| 2026-04-19 | CMH8 | `done` | Completed the hotspot-wave closeout. Updated `AGENTS.md` and `docs/plans/README.md` so future broad maintainability work treats this hotspot wave as archived historical context, moved the plan into `docs/plans/archive/`, and confirmed that every selected active file above 1,500 lines now has an explicit justification while no selected active file remains above 2,000 lines for avoidable packaging reasons. The first sandboxed `make ci` attempt hit a read-only advisory-db lock in `/Users/jack/.cargo`, so the closeout reran `make ci` with the needed external Cargo access and finished green. | `make check`; `make test`; `make clippy`; `npm run test --workspaces --if-present`; `npm run build --workspaces --if-present`; `make ci` | hotspot maintainability wave archived; promote a new active plan before landing another broad maintainability cleanup pass |
