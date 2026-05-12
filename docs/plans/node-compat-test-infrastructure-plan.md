# Node Compatibility Test Infrastructure Plan

Completed companion implementation plan for the Node compatibility harness that
supported `docs/plans/node-lts-compatibility-plan.md` during NLC and now serves
as the checked-in evidence baseline for the successor follow-on wave.

---

## Status

- **Status:** `done`
- **Primary completed scope:** this plan for Node-upstream harness architecture,
  manifests, reporting, canaries, and fixture lifecycle
- **Historical parent owner:** `docs/plans/node-lts-compatibility-plan.md`
- **Completed successor baseline:** `docs/plans/node-compat-future-lanes-and-correctness-plan.md`
- **Relationship:** completed companion baseline; the future lane-scaling and
  correctness follow-on wave has now also completed under the successor plan,
  so both documents serve as baselines rather than active owners
- **Post-closeout audit note:** a later evidence audit found remaining
  hardening work that should not be treated as completed by this historical
  plan: suite-wide denominator reporting, full machine-readable expectations,
  fixture sync tooling, and decomposition of the still-large `node_compat.rs`
  owner file. That active work is now owned by
  `docs/plans/node-compat-evidence-hardening-plan.md`.
- **Readiness:** Phases 1 through 4 are now live on the current worktree:
  machine-readable lane provenance, carried family catalogs, deterministic
  report/oracle/canary artifact generation, profile-aware claim mapping, and
  the nightly evidence workflow are all checked in under the
  `crates/neovex-runtime/src/runtime/tests/node_compat_manifests/`,
  `scripts/node_compat/`, and `tests/node-compat/` roots
- **Hard constraint:** preserve `neovex-runtime`'s zero-workspace-dependency
  invariant and the existing global runtime-suite serialization model

## Current State

- Final phase reached: `Phase 4 complete`
- Closed state: the NLC10 evidence substrate is present, but the later
  hardening audit narrowed the claim: this plan is a completed baseline, not a
  proof that every original ideal closeout gate is fully done. The checked-in harness has machine-readable lane
  provenance and carried family catalogs, deterministic manifest-driven plan
  and observed-result reports, canonical package/framework canary entrypoints
  for both `Application` and `Tooling`, a local representative Node22 oracle
  artifact path, and a nightly evidence workflow that runs the five carried
  seeded slice replays, both canary profiles, a version-matched
  Node20 / Node22 / Node24 oracle sweep, dashboard aggregation, and retained
  artifact upload. The generated local dashboard reflects the measured
  closeout baseline instead of a scaffolding state: two green canary profile
  reports, seeded slice evidence, and a representative local Node22 oracle
  artifact.
- Follow-on boundary: the successor-scope Phase 5 and Phase 6 follow-on wave
  is now complete as well. Any additional Node26+ onboarding or new
  correctness-expansion work needs a fresh active plan instead of silently
  extending this completed baseline.

## Objective

Replace the current macro-heavy, monolithic Node compatibility harness with a
manifest-driven, provenance-backed, report-producing test infrastructure that:

- preserves current passing behavior while making expectations explicit
- scales to Node 20 / 22 / 24 today and future lanes such as Node 26 without
  routine Rust harness edits
- distinguishes upstream Node line status, Neovex lane role, and public support
  claims
- adds trust-grade evidence through expected-failure accounting, package and
  framework canaries, and a shadow-oracle lane against real Node

## Relationship To Active NLC Plan

This plan is a focused companion to
`docs/plans/node-lts-compatibility-plan.md`, not a competing roadmap.

Ownership split:

- **NLC plan owns:** runtime semantics, module-family closeout, support-state
  truth, Deno-family fork decisions, and public contract changes.
- **This plan owns:** harness architecture, fixture catalog/provenance, suite
  taxonomy, expected-failure semantics, structured reporting, package-canary
  wiring, and harness self-verification.

Execution mapping:

| Companion phase | NLC linkage | Notes |
| --- | --- | --- |
| Phase 1 | Can start during NLC8 | Schema, topology, profile/capability modeling, and provenance can land while active NLC8 runtime-family work continues. |
| Phase 2 | Starts after Phase 1 stabilizes; can still run during NLC8 | File split and fixture dedup should not wait for NLC10, but they do depend on the Phase 1 data model being settled first. |
| Phase 3 | Must be in place before NLC10 broad closeout | Structured reporting, expectation semantics, and oracle lanes are prerequisites for durable NLC10 evidence and dashboards. |
| Phase 4 | Direct NLC10 implementation slice | Reuses and expands the existing `tests/node-compat/networking-canaries/` root into the broader package/framework claim map already required by NLC. |
| Phase 5 | Precedes Node26+ onboarding and informs the next NLC successor plan | Makes harness onboarding for future lanes data-driven without claiming zero runtime-semantic work. |
| Phase 6 | Coordinated with active NLC8-NLC9 owner work | Touches loader/runtime correctness, so implementation sequencing must coordinate with the active NLC owner before semantics move. |

Control-plane rule:

- family manifests and failure inventories under
  `docs/architecture/runtime/node-lts-compat/` remain NLC-owned closeout
  artifacts
- this plan may introduce machine-readable harness manifests and reports, but
  it must not become a second competing support-state authority

## Scope

- Replace `node_compat.rs` batch arrays and macros with manifest-driven fixture
  declarations.
- Introduce per-lane provenance tracking, sync tooling, and upstream diff
  reporting.
- Reorganize fixtures into a canonical-plus-overrides layout that reduces
  checked-in duplication while keeping exact upstream sources auditable.
- Add a Node-core-harness-inspired suite taxonomy that separates suite kind,
  execution class, stability, capabilities, platform restrictions, and runtime
  profile.
- Add expected-failure, skip, flaky, and unexpected-pass semantics with machine
  readable reasons.
- Produce structured per-lane / per-slice / per-profile reports that feed NLC
  manifests, failures, and future dashboards.
- Add harness self-verification through golden tests and a shadow-oracle lane
  against real Node 20 / 22 / 24.
- Introduce a supplementary behavioral test tier — Neovex-authored tests that
  verify behaviors Node's own suite does not comprehensively cover from the
  perspective of an alternative runtime: module resolution bridge, builtin
  import completeness, global injection fidelity, process object shape,
  resource safety, and framework-motivated patterns. This tier is distinct
  from both vendored upstream fixtures and framework canaries.
- Expand package/framework canaries into a pinned, profile-aware trust layer.
- Make future lane onboarding a data-only operation for manifest, sync, and
  reporting paths.

## Non-Goals

- Replacing `docs/plans/node-lts-compatibility-plan.md` as the broad Node
  compatibility owner.
- Claiming full Node parity from harness refactoring alone.
- Removing the existing runtime-suite serialization lock or making embedded
  runtime fixtures concurrent by default.
- Adding workspace dependencies to `neovex-runtime`.
- Promoting Node 20 back to a primary runtime contract or implying that Node 24
  / 26 preview lanes are already public support claims.
- Introducing speculative compatibility surfaces outside the current NLC and
  research direction.

## Current State Baseline

Measured from the live worktree on 2026-05-08. These numbers are intentionally
not copied from older research snapshots because the current NLC8 worktree is
already moving.

| Metric | Live value | Notes |
| --- | --- | --- |
| `crates/neovex-runtime/src/runtime/tests/node_compat.rs` size | 7,139 lines | Well above the repo's 2,000-line decomposition threshold |
| `#[test]` functions | 232 | Counted from the live file |
| `#[ignore]` annotations | 60 | Must be migrated intentionally, not dropped |
| Batch constant arrays | 45 | Current Rust-owned slice declaration surface |
| Fixture files: `node20/` | 1,291 | Checked-in per-lane upstream fixtures |
| Fixture files: `node22/` | 1,236 | Primary canonical lane today |
| Fixture files: `node24/` | 1,479 | Preview lane, currently broader and still noisy |
| Shared fixture files: `test/` | 158 | Neovex-owned shared fixtures and helper files |
| Byte-identical files: `node20` vs `node22` | 1,023 of 1,178 overlapping | Strong signal that checked-in duplication can be reduced |
| Byte-identical files across all three lanes | 739 of 1,166 overlapping | Supports canonical-plus-overrides layout |
| Fixture disk footprint | 20 MB | Current checked-in baseline |
| Supplementary behavioral tests | 0 | Deno and Bun both maintain large supplementary surfaces; see the research doc for branch-specific measured breakdowns |

Current implementation observations that matter to this plan:

- `node_compat.rs` still owns `NodeCompatLane`, `NodeCompatBatchEntry`, fixture
  materialization, reporting, 9 macros, and 45 batch constants in one file.
- `execute_upstream_node_compat_test_with_extra_files()` still acquires
  `acquire_runtime_suite_lock()` and performs process-global `TERM` /
  `NODE_OPTIONS` mutation under that lock.
- `write_node_compat_bundle()`, `execute_manifested_node_compat_test()`, and
  `execute_upstream_node_compat_test_with_extra_files()` are the behavioral
  center of the current harness and should survive as named concepts even if
  signatures evolve.
- Named harness behaviors already exist in code as ad hoc string constants:
  process-exit sentinel, interactive terminal modeling, DNS default result
  order overrides, GC exposure, pending-deprecation handling, and process
  lifecycle drain.
- `RuntimeCompatibilityTarget` currently exposes only
  `WebStandardIsolate` and `Node22`, while all current Node-compat fixture lanes
  execute through `RuntimeLimits::application_node22()`.
- `runtime/bootstrap/source.rs` still hardcodes Node22-shaped bootstrap
  behavior such as `process.version`.
- `module_loader.rs` already has ESM-side bare package and `node:` builtin
  logic, but the harness does not yet make CJS and ESM bare-builtin proof a
  first-class invariant.
- The live worktree already contains initial NLC artifacts and the existing
  `tests/node-compat/networking-canaries/` root; this plan should normalize and
  expand that progress rather than replace it with a second parallel layout.
- The entire Node compat test surface consists of vendored upstream fixtures.
  There are no Neovex-authored supplementary tests covering module resolution
  bridge, builtin import completeness, global injection fidelity, or process
  object shape — behaviors that both Deno and Bun test extensively because
  Node's own suite does not comprehensively exercise them from the perspective
  of an alternative runtime.

## Key Invariants To Preserve

- `neovex-runtime` must keep zero workspace dependencies.
- Embedded fixture execution must stay serialized behind
  `acquire_runtime_suite_lock()` until a later plan explicitly proves that
  process-global env mutation and tempdir semantics can be removed safely.
- Existing passing behavior must be preserved through the refactor; data-model
  cleanup does not justify semantic regressions.
- Every current `#[ignore]` must map to an explicit manifest expectation:
  `skip`, `expected_failure`, `flaky`, or `known_issue`.
- Prelude and postlude behavior must survive as named harness data, including:
  - process-exit sentinel handling
  - interactive terminal modeling
  - DNS result-order overrides
  - pending-deprecation modeling
  - process lifecycle drain
  - GC exposure where currently required
- Preview-lane top-level skip capture must remain explicit lane data, not an
  accidental side effect of one code path.
- The harness may scale to future lanes without Rust edits for manifest, sync,
  and reporting, but this plan must not imply that future runtime semantics
  will require zero implementation work.
- Vendored upstream fixtures must remain unmodified — behavioral adaptations
  use preludes, postludes, or per-lane overrides, not source edits.
  Supplementary tests are Neovex-authored and CAN be modified freely.
- Package/framework canaries that need npm packages or broader repo-owned test
  helpers must live outside `neovex-runtime` if that avoids violating crate
  dependency rules.

## Dependencies And Ordering

1. **Foundation before structure.** The manifest schema, taxonomy, provenance
   files, and lane metadata must exist before `node_compat.rs` can be split
   safely, otherwise the refactor only moves hardcoded sprawl around.
2. **Structure before quality controls.** Expected-failure accounting,
   structured reporting, and oracle verification depend on stable manifests and
   fixture resolution.
3. **Quality and supplementary tests before trust claims.** Supplementary
   behavioral tests land in Phase 3 alongside expected-failure accounting and
   reporting, because they prove bridge and injection correctness that
   vendored fixtures cannot. Package/framework canaries should land only
   after the harness can explain pass / fail / skip / unexpected-pass results
   consistently and the supplementary tier proves behavioral fidelity.
4. **Scale after schema stabilization.** Node 26+ onboarding must be designed
   after the manifest and report shape are settled, otherwise the data model
   will churn again.
5. **Correctness changes coordinate with NLC.** Version-target runtime support
   and CJS / ESM bare-builtin parity touch active runtime semantics, so their
   implementation order must be negotiated with the active NLC item owner.

## Roadmap / Phases

### Phase 1: Foundation — Provenance, Manifest, and Taxonomy

#### 1.1 Harness catalog, schema, and named behavior registry

- **What changes**
  - Add a thin composition root under
    `crates/neovex-runtime/src/runtime/tests/node_compat/` and move the
    machine-readable harness types into concept-owned modules such as:
    `catalog.rs`, `taxonomy.rs`, and `preludes.rs`.
  - Add a checked-in manifest data root under
    `crates/neovex-runtime/src/runtime/tests/node_compat_manifests/` with:
    - `schema.json`
    - `lanes/node20.json`, `lanes/node22.json`, `lanes/node24.json`
    - `fixtures/core-semantics.json`, `process-and-timing.json`,
      `streams-and-local-io.json`, `networking.json`, `loader-context.json`
    - `preludes.json`
  - Replace macro-owned `NodeCompatBatchEntry` sprawl with a data model that
    includes:
    - test tier (`upstream_vendored`, `supplementary`, `canary`) — see
      Phase 3.5 for supplementary tier details
    - supplementary category for the supplementary tier
      (`builtin_completeness`, `module_bridge`, `global_injection`,
      `process_shape`, `resource_safety`, `framework_pattern`)
    - lane config
    - fixture source
    - extra files
    - expectation state (`pass`, `skip`, `expected_failure`, `flaky`,
      `known_issue`)
    - named preludes and postludes
    - runtime profile (`Application`, `Tooling`)
    - execution class (`parallel`, `sequential`, `watchpoint`,
      `expected_failure`, `oracle_only`)
    - capability requirements (`tty`, `main-thread`, `crypto`,
      `bundle-root-fs`, `loopback-net`, `external-net`, `dns-result-order`,
      `gc-exposed`, `child-process`, `worker-threads`)
    - platform restrictions where needed
  - Promote current string-coded harness behaviors into named catalog entries
    rather than keeping them implicit in `match` arms.
  - Keep `write_node_compat_bundle()`,
    `execute_manifested_node_compat_test()`, and
    `execute_upstream_node_compat_test_with_extra_files()` as named harness
    concepts, but make them consume a resolved `NodeCompatExecutionPlan`
    instead of raw macro arrays plus optional string snippets.

- **Why this ordering**
  - Every later phase depends on a stable schema and taxonomy.
  - Without this step, file splitting only hides the same hardcoded policy in
    smaller Rust modules.

- **Verification**
  - `cargo test -p neovex-runtime node_compat_manifest_schema`
    - proves the manifest parser accepts valid data and rejects malformed data
  - `cargo test -p neovex-runtime node_compat_named_preludes`
    - proves named prelude / postlude resolution preserves current behavior
  - `cargo test -p neovex-runtime node_compat_preview_lane_top_level_skip`
    - proves preview-style top-level skip capture remains explicit lane data
  - `cargo fmt --all --check`
  - `make clippy`

- **Completion gate**
  - Every currently manifested fixture can be represented without Rust macro
    expansion.
  - Named prelude / postlude behavior is catalog-backed rather than inferred
    from ad hoc string comparisons.
  - The manifest distinguishes suite kind, execution class, profile,
    capabilities, and platforms as separate axes.

- **Risks**
  - The schema becomes too loose and recreates free-text drift.
    - Mitigation: keep reason codes and capability names enumerable where
      practical, with explicit validation.
  - The migration preserves data but not behavior.
    - Mitigation: add golden tests for the resolved execution plan before
      deleting macro-owned paths.

#### 1.2 Manifest topology and profile/capability model

- **What changes**
  - Make the manifest topology explicit instead of implying an unspecified join:
    - `schema.json` validates every file type
    - `lanes/<lane>.json` owns lane metadata only
    - `fixtures/<family>.json` owns self-contained per-fixture entries,
      including per-lane expectations and slice membership
    - `preludes.json` owns named prelude/postlude definitions and portable
      metadata such as whether a behavior is runtime-only, oracle-safe, or
      preview-lane-only
  - Make the supplementary-fixture path explicit at the same time:
    supplementary manifest entries live in `fixtures/supplementary*.json`,
    while the corresponding Neovex-authored fixture sources live under
    `crates/neovex-runtime/src/runtime/tests/node_compat_fixtures/supplementary/`.
  - Define deterministic loader semantics:
    1. load all known lane files
    2. load `preludes.json`
    3. load all `fixtures/*.json` files and concatenate entries
    4. validate that every fixture references an existing lane and named
       prelude/postlude
    5. reject duplicate fixture ids instead of merging partial fixture
       fragments from multiple files
  - Add an explicit profile/capability/execution model that future engineers do
    not have to reconstruct from scattered prose:

    | Axis | Meaning | Claim / scheduling rule |
    | --- | --- | --- |
    | `profiles` | Where the result counts: `Application`, `Tooling`, or both | `Tooling`-only results never roll up into `Application` claims |
    | `capabilities` | Host/runtime prerequisites such as `bundle-root-fs`, `crypto`, `loopback-net`, `tty`, `main-thread`, `child-process`, `external-net` | Missing capability restrictions must classify as structured expectation reasons, not generic runtime failures |
    | `executionClass` | Scheduling/reporting semantics such as `parallel`, `sequential`, `watchpoint`, `expected_failure`, `oracle_only` | `parallel` means "parallel-safe in principle"; it does not override the current runtime-suite serialization lock |

  - Require the manifest to model capability truth independently of public
    support claims so an `Application` fixture that needs `tty` or
    `child-process` can be represented honestly as a profile restriction rather
    than hidden in a free-text ignore reason.

- **Why this ordering**
  - Topology has to be explicit before provenance, reporting, or developer
    entrypoints can be implemented cleanly.
  - The harness cannot inspire trust if profiles, capabilities, and execution
    semantics remain inferred instead of declared.

- **Verification**
  - `cargo test -p neovex-runtime node_compat_manifest_topology`
    - proves lane files, fixture files, and prelude registry compose
      deterministically
  - `cargo test -p neovex-runtime node_compat_profile_capability_model`
    - proves profile, capability, and execution-class validation rejects
      ambiguous or contradictory entries

- **Completion gate**
  - The authoritative manifest topology is documented and implemented with
    deterministic load semantics.
  - Profile, capability, and execution-class modeling is explicit enough that a
    reader can derive how a fixture should be scheduled and how its result may
    contribute to public support claims.

- **Risks**
  - Over-normalizing the topology into many tiny files recreates join pain.
    - Mitigation: keep `lanes/` metadata-only, keep fixture entries
      self-contained in `fixtures/*.json`, and forbid partial fixture merges.
  - The profile/capability model drifts away from real NLC support claims.
    - Mitigation: keep claim interpretation NLC-owned, but require the harness
      to expose the raw profile and capability facts needed for those claims.

#### 1.3 Fixture provenance and sync metadata

- **What changes**
  - Add per-lane provenance metadata files that record:
    - exact upstream Node tag
    - exact commit
    - sync date
    - source directory under upstream `test/`
    - upstream lifecycle (`eol`, `lts`, `current`, etc.)
    - Neovex lane role (`validation`, `primary`, `preview`)
    - current runtime target (`node22` today)
  - Add repo-owned sync helpers under `scripts/node_compat/`, for example:
    - `sync-fixtures.sh`
    - `diff-fixtures.sh`
    - `validate-provenance.sh`
  - Make fixture updates emit a checked-in diff report so the lane change is
    reviewable before new files land.
  - Move current runtime `Flags:` heuristics into sync-time or manifest-time
    metadata generation where possible, especially `--pending-deprecation`.

- **Why this ordering**
  - Provenance must exist before dedup and materialization can remain
    enterprise-auditable.
  - Sync tooling should stabilize before any large fixture move makes diffs
    harder to review.

- **Verification**
  - `cargo test -p neovex-runtime node_compat_provenance_validation`
    - proves required provenance fields exist for each configured lane
  - `bash scripts/node_compat/validate-provenance.sh`
    - proves tags, commits, and lane metadata are internally consistent
  - `bash scripts/node_compat/diff-fixtures.sh --lane node22`
    - proves the diff report can be generated against the pinned upstream line

- **Completion gate**
  - Node 20 / 22 / 24 fixture trees all have exact provenance metadata.
  - A future fixture sync can be reviewed as a pinned upstream delta instead of
    "copied files changed somehow."

- **Risks**
  - Sync tooling fetches floating upstream state and weakens reproducibility.
    - Mitigation: require explicit pinned tags / commits and fail closed on
      missing provenance.

#### 1.4 Node-core-harness taxonomy adoption

- **What changes**
  - Model Node core suite types directly in the manifest:
    `parallel`, `sequential`, `pseudo-tty`, `internet`, `pummel`,
    `known_issues`, plus Neovex-specific `watchpoint`, `wpt`, and `canary`.
  - Add stability metadata similar to Node status semantics:
    `slow`, `flaky`, `expected_failure`, `known_issue`.
  - Keep execution class separate from suite taxonomy so a `parallel` suite
    item does not imply concurrent embedded execution while the global lock
    remains in force.

- **Why this ordering**
  - This taxonomy informs expectation handling, oracle classification, and
    future report slicing.

- **Verification**
  - `cargo test -p neovex-runtime node_compat_taxonomy_golden`
    - proves fixture paths and manifest entries normalize into the intended
      Node-style suite categories

- **Completion gate**
  - Suite kind, execution class, and capability restrictions no longer hide
    inside fixture paths or free-text reasons.

- **Risks**
  - Taxonomy and scheduling semantics get conflated.
    - Mitigation: keep `suiteKind` and `executionClass` as separate manifest
      fields and preserve the runtime-suite lock until a later plan re-proves
      concurrency.

#### 1.5 Developer entrypoints and Makefile surface

- **What changes**
  - Add one documented developer-entrypoint family that wraps the underlying
    `scripts/node_compat/` helpers instead of forcing engineers to memorize
    raw script paths. Preferred surface:
    - `make node-compat-sync LANE=node22`
    - `make node-compat-diff LANE=node22`
    - `make node-compat-report LANE=node22 SLICE=nlc3-core-semantics`
    - `make node-compat-oracle LANE=node22`
    - `make node-compat-canaries-bootstrap`
    - `make node-compat-canaries PROFILE=application`
  - Preserve `make verify-harness SURFACE=runtime` as the canonical focused
    verification lane for runtime-owned proof, with the new make targets
    serving as harness-specific helpers rather than replacements.
  - Keep the make aliases thin wrappers over `scripts/node_compat/*` so the
    lower-level scripts remain callable from CI and ad hoc local debugging.

- **Why this ordering**
  - The developer-entrypoint surface should be defined early so the rest of the
    plan has one canonical invocation vocabulary for sync, diff, oracle, and
    canary work.

- **Verification**
  - `make node-compat-sync LANE=node22 DRY_RUN=1`
    - proves the sync surface is discoverable through one canonical entrypoint
  - `make node-compat-oracle LANE=node22 SAMPLE=test/parallel/test-buffer-alloc.js`
    - proves the oracle lane can be invoked without bespoke shell knowledge

- **Completion gate**
  - The harness has one documented, repo-native entrypoint family for sync,
    diff, report, oracle, and canary workflows.
  - Engineers no longer need to rediscover bespoke shell commands to work on
    this surface safely.

- **Risks**
  - The repo accumulates redundant make wrappers that drift from the real
    scripts.
    - Mitigation: keep wrappers thin and make the script layer the single
      implementation owner.

### Phase 2: Structural — File Split and Deduplication

#### 2.1 Decompose `node_compat.rs` into concept-owned modules

- **What changes**
  - Replace the monolithic `crates/neovex-runtime/src/runtime/tests/node_compat.rs`
    root with a thin composition surface and concept-owned modules such as:
    - `mod.rs`
    - `bundle.rs`
    - `execute.rs`
    - `fixtures.rs`
    - `manifest.rs`
    - `preludes.rs`
    - `report.rs`
    - `oracle.rs`
    - `watchpoints.rs`
  - Preserve the current behavioral center by moving, not renaming away, the
    logic now centered on:
    - `write_node_compat_bundle`
    - `execute_manifested_node_compat_test`
    - `execute_upstream_node_compat_test_with_extra_files`
  - Delete macro-owned batch constants and one-off lane `#[test]` sprawl once
    manifest-backed wrappers prove parity.

- **Why this ordering**
  - The manifest and taxonomy must be settled first so the split has stable
    ownership seams instead of churning immediately afterward.

- **Verification**
  - `cargo test -p neovex-runtime node_compat_`
    - focused runtime test filter for the new module tree
  - `make verify-harness SURFACE=runtime`
    - proves the repo-owned runtime verification entrypoint still covers the
      moved harness surface
  - `cargo fmt --all --check`
  - `make clippy`

- **Completion gate**
  - No single Node-compat test harness file exceeds 2,000 lines without an
    explicit accepted exception.
  - The composition root is thin and behavior is grouped by concept ownership.

- **Risks**
  - Splitting the file accidentally changes fixture ordering or summary output.
    - Mitigation: add golden tests for report order and fixture selection
      before deleting the monolith.

#### 2.2 Canonical-plus-overrides fixture layout

- **What changes**
  - Reorganize
    `crates/neovex-runtime/src/runtime/tests/node_compat_fixtures/` into a
    dedup-friendly structure:
    - `canonical/node22/test/...`
    - `overrides/node20/...`
    - `overrides/node24/...`
    - `shared/test/...`
  - Add a materialization layer that reconstructs a lane-local bundle view from
    canonical files plus per-lane overrides at execution time.
  - Keep Neovex-owned helper files such as `test/common/index.js`,
    `fixtures.js`, and `tmpdir.js` under the shared root and reference them
    explicitly through manifest `extraFiles`.
  - Make fixture resolution explicit in one place so lane diffs remain
    auditable even after deduplication.

- **Why this ordering**
  - Deduplication is only safe after manifests can tell the runner exactly
    which canonical and override files belong to each fixture.

- **Verification**
  - `cargo test -p neovex-runtime node_compat_fixture_materialization`
    - proves a materialized lane reconstructs the same runtime file tree as the
      pre-refactor fixture source
  - `bash scripts/node_compat/diff-fixtures.sh --lane node20 --materialized`
    - proves materialized output matches the intended pinned upstream lane
  - a disk-usage check recorded in the execution log to show the new checked-in
    footprint versus the old 20 MB baseline

- **Completion gate**
  - Canonical-plus-overrides replaces triple-check-in duplication without
    losing exact reviewability of lane differences.
  - Shared helper files are no longer copied redundantly into multiple lane
    roots.

- **Risks**
  - Deduplication obscures where a lane-specific file came from.
    - Mitigation: provenance files and diff tooling must render the resolved
      source path for every override.

### Phase 3: Quality — Expected Failures, Reporting, and Harness Verification

#### 3.1 Expectation model with unexpected-pass detection

- **What changes**
  - Replace `#[ignore]` plus inline reason strings with manifest expectation
    data:
    - `skip`
    - `expected_failure`
    - `flaky`
    - `known_issue`
  - Require every non-pass expectation to carry a reason plus a classification
    such as:
    - `intentional_profile_restriction`
    - `known_runtime_gap`
    - `upstream_deno_gap`
    - `harness_issue`
    - `platform_limitation`
    - `preview_only`
    - `upstream_known_issue`
  - Make unexpected passes fail the lane by default except where `flaky`
    semantics intentionally downgrade them.
  - Add a one-time migration audit that maps all 60 current `#[ignore]`
    annotations into the new model before old wrappers are deleted.

- **Why this ordering**
  - Reports and oracle classification are only trustworthy if expectations are
    explicit and machine-readable first.

- **Verification**
  - `cargo test -p neovex-runtime node_compat_expectation_golden`
    - proves expectation parsing and unexpected-pass logic behave as designed
  - `cargo test -p neovex-runtime node_compat_ignore_migration_audit`
    - proves every historical ignore has a manifest replacement

- **Completion gate**
  - No active Node-compat truth depends on Rust `#[ignore]` metadata alone.
  - Unexpected passes are surfaced explicitly instead of silently disappearing.

- **Risks**
  - Teams overuse `expected_failure` as a softer skip.
    - Mitigation: keep skip and expected-failure categories distinct and fail
      review if a runnable test is hidden behind `skip`.

#### 3.2 Structured reporting for lanes, slices, profiles, and suites

- **What changes**
  - Add structured report generation, likely under `target/node-compat/`, with:
    - per-lane summaries
    - per-slice summaries
    - per-profile summaries
    - per-fixture results
    - unexpected-pass and oracle-drift summaries
  - Make report schema explicit and versioned.
  - Keep NLC checked-in family manifests and failure inventories as the public
    narrative layer, but generate their numeric inputs from these reports.
  - Report upstream line status, Neovex lane role, and public support claim as
    separate fields rather than one overloaded "status."

- **Why this ordering**
  - Structured reporting depends on the new manifest shape and expectation
    model.
  - Canary and dashboard work in later phases should consume report artifacts,
    not reimplement counting logic.

- **Verification**
  - `cargo test -p neovex-runtime node_compat_report_schema`
    - proves report serialization and versioning are stable
  - `cargo test -p neovex-runtime node_compat_report_golden`
    - proves lane / slice / profile summaries match expected fixture outcomes
  - `make verify-harness SURFACE=runtime`
    - proves the repo-owned harness entrypoint can emit the structured report

- **Completion gate**
  - The harness produces machine-readable outputs that can explain pass / fail /
    skip / unexpected-pass counts by lane, slice, suite, and profile.
  - Public docs no longer depend on parsing stderr summaries by hand.

- **Risks**
  - Report data starts carrying public-claim logic that belongs to NLC docs.
    - Mitigation: keep report fields factual and let NLC-owned docs interpret
      support-state claims.

#### 3.3 Harness self-verification and shadow-oracle comparison

- **What changes**
  - Add golden tests for:
    - manifest resolution
    - prelude / postlude selection
    - top-level skip capture
    - report serialization
    - expectation classification
  - Add a shadow-oracle lane that executes the same materialized fixture under
    real Node 20 / 22 / 24 and classifies drift as:
    - `oracle_pass_runtime_fail`
    - `oracle_pass_runtime_skip`
    - `oracle_skip_runtime_run`
    - `oracle_fail_both`
    - `oracle_harness_mismatch`
  - Invoke the oracle through explicit, version-matched Node subprocesses
    configured per lane, not through whichever `node` binary happens to be on
    `PATH`. The initial contract may use environment variables such as
    `NEOVEX_NODE20_BIN`, `NEOVEX_NODE22_BIN`, and `NEOVEX_NODE24_BIN`, with
    nightly CI wiring them explicitly.
  - Treat preludes/postludes as data with oracle portability metadata:
    portable behaviors may be replayed under real Node, while Neovex-only
    helpers such as process-exit sentinels, synthetic TTY env proxies, or
    embedded top-level skip capture must either be disabled in oracle mode or
    replaced with a Node-native equivalent. Oracle mismatch caused only by
    harness-only helpers must classify as `oracle_harness_mismatch`, not as a
    runtime failure.
  - Join oracle output with manifest expectation state so "expected failure in
    Neovex and failure under real Node" is classified differently from
    "expected pass in Neovex but failure only under Neovex."
  - Keep oracle runs serialized by default so they do not undercut the current
    process-global test model.
  - Add repo-owned helpers under `scripts/node_compat/` such as
    `oracle-run.sh` and `oracle-classify.sh`.
  - Start the oracle lane as nightly / manually dispatched evidence rather than
    a blocking per-commit gate, then tighten only after drift classes and
    environment assumptions are proven stable.

- **Why this ordering**
  - Oracle verification only becomes useful once manifests, materialization, and
    expectation states are stable.

- **Verification**
  - `cargo test -p neovex-runtime node_compat_oracle_classification`
    - proves drift classes are computed deterministically
  - `cargo test -p neovex-runtime node_compat_bundle_golden`
    - proves resolved bundle contents remain stable for representative fixtures
  - `bash scripts/node_compat/oracle-run.sh --lane node22 --fixture test/parallel/test-buffer-alloc.js`
    - proves the real-Node shadow path works on at least one representative
      fixture

- **Completion gate**
  - Harness logic is proven by its own golden tests, not only by broad fixture
    reruns.
  - Real-Node oracle evidence exists for representative lanes and can classify
    harness drift separately from runtime gaps.

- **Risks**
  - Oracle runs become flaky or environment-sensitive and get ignored.
    - Mitigation: keep the lane opt-in but deterministic, and classify
      environment blockers explicitly instead of masking them as runtime truth.

#### 3.4 CI integration and artifact retention

- **What changes**
  - Define a two-tier CI contract for this harness:
    - per-commit focused lane: preserve `make verify-harness SURFACE=runtime`
      and, when warranted, a touched-slice manifest replay
    - nightly broad lane: full multi-lane manifest replay, structured report
      generation, representative oracle sweep, and pinned canary runs
  - Make structured reports first-class CI artifacts so every broad lane keeps
    auditable machine-readable evidence even before a public dashboard exists.
  - Keep fixture sync and large oracle sweeps manually dispatchable so upstream
    corpus refresh work does not surprise per-commit contributors.
  - Document whether a lane is blocking, informational, nightly-only, or
    manually dispatched so engineers can interpret failures correctly.

- **Why this ordering**
  - Reporting, oracle behavior, and canaries need one explicit CI story before
    NLC10 can rely on them as durable closeout evidence.

- **Verification**
  - `make verify-harness SURFACE=runtime`
    - proves the per-commit focused lane remains canonical
  - `make node-compat-report LANE=node22 SLICE=nlc3-core-semantics`
    - proves the report artifact path is callable through the documented helper
  - `make node-compat-oracle LANE=node22 SAMPLE=test/parallel/test-buffer-alloc.js`
    - proves the nightly/manual oracle entrypoint is runnable through the same
      canonical surface
  - `make node-compat-canaries PROFILE=application`
    - proves the nightly canary entrypoint exists and is distinct from the
      focused runtime verification lane

- **Completion gate**
  - The plan defines which harness lanes run per-commit, nightly, and by manual
    dispatch.
  - Structured reports are retained as CI artifacts for broad runs.
  - Oracle and canary breadth is intentionally scoped rather than implied.

- **Risks**
  - CI grows too expensive and contributors stop trusting failures.
    - Mitigation: keep the blocking per-commit surface focused, and reserve the
      broad oracle/canary matrix for nightly or manual evidence gathering.

#### 3.5 Supplementary behavioral test tier

- **What changes**
  - Introduce a distinct test tier for Neovex-authored tests that verify
    behaviors Node's own suite does not comprehensively test from the
    perspective of an alternative runtime. These sit between vendored
    upstream fixtures (which prove API correctness) and framework canaries
    (which prove ecosystem compat).
  - Add the `testTier` field to the manifest schema with values
    `upstream_vendored`, `supplementary`, and `canary`, plus a required
    `supplementaryCategory` field for supplementary fixtures. Structured
    reports break out counts by tier and category so readers can distinguish
    "we pass N upstream Node tests" from "we pass M supplementary behavioral
    tests" and see which proof areas still have gaps.
  - Create initial supplementary fixtures covering six categories, informed
    by Deno's `unit_node` / `specs/node` supplementary surfaces and Bun's
    hundreds of self-authored `*.test` / `*.spec` files outside vendored
    `test/js/node/test/*`:

    | Category | What it proves | Precedent |
    | --- | --- | --- |
    | Builtin completeness | Every supported builtin importable via `require('X')`, `require('node:X')`, `import 'X'`, `import 'node:X'` in both CJS and ESM | Bun `stubs.test.js` |
    | Module resolution bridge | CJS `require()` of ESM modules, ESM `import` of CJS modules, `createRequire()`, conditional `exports` field resolution | Deno `require_esm_module_exports/`, `esm_dir_import/` |
    | Global injection fidelity | `__dirname`, `__filename` exist in CJS, not in ESM; `require` is a function in CJS | Bun `dirname.test.js` |
    | Process object shape | `process.version`, `process.versions`, `process.features`, `process.env` structure match the lane's target version | Deno `process_test.ts` |
    | Resource safety | `createWriteStream`/`createReadStream` don't leak FDs; `Buffer` ops handle detached `ArrayBuffer` | Bun `fs-leak.test.js`, `buffer-copy-fill-detach.test.ts` |
    | Framework-motivated patterns | `Module._compile` hooks (tsx/ts-node), `ServerResponse` wrapping (hono/express), worker eval mode (fflate) | Deno `specs/node/` framework regressions |

  - Supplementary tests are Neovex-authored and CAN be modified (unlike
    vendored upstream fixtures which must remain unmodified). They live under
    `crates/neovex-runtime/src/runtime/tests/node_compat_fixtures/supplementary/`
    or an equivalent supplementary root outside the vendored fixture tree.
  - Each supplementary test runs per lane and per profile, with per-lane
    expectations in the manifest. A supplementary test can be `skip` for a
    lane that doesn't support the tested behavior.
  - The builtin completeness test is the highest-priority initial fixture:
    it is small (~50 lines), runs in seconds, and would have caught the
    known ESM bare-specifier gap immediately.

- **Why this ordering**
  - Supplementary tests require the manifest schema (Phase 1), the
    expectation model (Phase 3.1), and structured reporting (Phase 3.2) to
    be useful. They don't require the fixture deduplication or oracle work.
  - Landing them in Phase 3 means they are available as evidence before the
    Phase 4 canary and Phase 5 scaling work begins.
  - They fill a real gap: Node's test suite proves API function correctness
    but not bridge correctness, and framework canaries prove ecosystem
    compatibility but not behavioral fidelity. The supplementary tier covers
    the middle ground.

- **Verification**
  - `cargo test -p neovex-runtime node_compat_supplementary_builtin_completeness`
    - proves all supported builtins are importable in all forms per lane
  - `cargo test -p neovex-runtime node_compat_supplementary_module_bridge`
    - proves CJS/ESM interop paths resolve correctly
  - `cargo test -p neovex-runtime node_compat_supplementary_global_injection`
    - proves `__dirname`/`__filename`/`require` injection is correct per
      module type
  - `cargo test -p neovex-runtime node_compat_supplementary_process_shape`
    - proves process object shape matches the lane's target version

- **Completion gate**
  - At least the builtin completeness, module resolution bridge, and global
    injection fixtures exist and run per lane.
  - Structured reports distinguish supplementary test results from upstream
    vendored fixture results.
  - The `testTier` and `supplementaryCategory` fields are enforced in the
    manifest schema.

- **Risks**
  - Supplementary tests become a dumping ground for ad hoc assertions.
    - Mitigation: require every supplementary fixture to map to one of the
      six named categories and to cite its precedent from Deno or Bun.
      Reject "interesting but uncategorized" additions.
  - Supplementary tests duplicate effort with vendored upstream fixtures.
    - Mitigation: supplementary tests cover behaviors that upstream fixtures
      do not already cover comprehensively from an alternative-runtime
      perspective (bridge, injection, completeness). If an upstream fixture
      already covers the behavior well enough, use the upstream fixture.

### Phase 4: Trust — Package and Framework Canaries

#### 4.1 Pinned canary registry and claim map

- **What changes**
  - Normalize the existing `tests/node-compat/networking-canaries/` root into a
    broader `tests/node-compat/` canary tree with a checked-in registry that
    maps:
    - package / framework name
    - pinned version
    - runtime profile
    - compatibility target / lane coverage
    - NLC family dependency
    - public claim dependency
  - Required initial mapped canaries should include the NLC-required set:
    `express`, `fastify`, `socket.io`, `undici`, `axios`, `jest`, `tsx`,
    `ts-node`, `prisma`, and `next`.
  - Keep Application and Tooling canaries separate so public support claims do
    not collapse profile differences.
  - Add a canonical bootstrap/install surface for pinned canary dependencies,
    owned by the top-level `Makefile` and backed by `scripts/node_compat/*`,
    rather than relying on undocumented ad hoc `npm ci --prefix ...`
    invocations.

- **Why this ordering**
  - Canaries need structured reporting and expectation states from Phase 3.
  - Reusing the current networking canary root lowers migration churn.

- **Verification**
  - `make node-compat-canaries-bootstrap`
    - proves pinned canary dependencies can be installed through the
      canonical repo-owned entrypoint
  - `make node-compat-canaries PROFILE=application`
    - proves the existing networking canary root still runs under the new
      registry shape through the canonical repo-owned entrypoint
  - future package-specific verification commands must be recorded in the
    execution log when new canary roots are added
  - `bash scripts/node_compat/validate-claims.sh`
    - proves every documented claim maps to at least one pinned canary

- **Completion gate**
  - Every package/framework claim in docs is backed by a pinned canary lane.
  - Green process exit alone is not the only assertion; each canary checks at
    least one real success condition and one relevant error or edge path when
    practical.

- **Risks**
  - Canary scope balloons into a second product matrix.
    - Mitigation: tie each canary to an NLC family or public claim and reject
      "interesting but unmapped" additions.

#### 4.2 Profile-aware truth and report integration

- **What changes**
  - Make canary reports emit the runtime profile and compatibility target they
    actually exercised.
  - Add an explicit rule that `Tooling`-only canaries must never be rendered as
    `Application` support claims.
  - Feed canary outcomes into the same structured reporting surface from
    Phase 3 so NLC closeout can cite one evidence bundle.

- **Why this ordering**
  - This is the trust bridge between harness mechanics and public contract
    truth.

- **Verification**
  - `cargo test -p neovex-runtime node_compat_claim_mapping_golden`
    - proves report-to-claim mapping logic keeps profile boundaries intact
  - package-root commands recorded during implementation must show profile-tagged
    results, not anonymous pass/fail lines

- **Completion gate**
  - Public support claims can be traced to concrete canary evidence by lane and
    by profile.

- **Risks**
  - Profile metadata gets added to reports but ignored in docs review.
    - Mitigation: fail claim-validation tooling when a claim lacks a matching
      profile-scoped canary record.

### Phase 5: Scale — Future Node Versions

#### 5.1 Data-only lane onboarding for Node 26+

- **What changes**
  - Remove hardcoded lane branching from harness-owned manifest, sync, and
    reporting paths so a new lane is introduced by:
    - a new lane manifest
    - a provenance file
    - an override directory when needed
    - optional canary mapping
  - Add scaffolding helpers under `scripts/node_compat/`, for example
    `add-lane.sh`, that create the data skeleton for `node26`.
  - Keep `node22` as the primary runtime target until runtime semantics expand;
    the harness must allow `node26` as a lane without claiming `RuntimeCompatibilityTarget::Node26`
    already exists.

- **Why this ordering**
  - Lane onboarding should build on the final manifest / report shape, not
    precede it.

- **Verification**
  - `cargo test -p neovex-runtime node_compat_lane_registry_golden`
    - proves a synthetic `node26` lane can be registered without Rust harness
      edits
  - `bash scripts/node_compat/add-lane.sh node26 --dry-run`
    - proves lane scaffolding is data-only

- **Completion gate**
  - Adding a new Node lane requires no harness Rust changes for manifest, sync,
    or report paths.
  - The plan text and generated reports still distinguish lane onboarding from
    runtime-semantic readiness.

- **Risks**
  - Teams interpret "no Rust changes for a new lane" as "no runtime work."
    - Mitigation: keep runtime target and public support claim separate lane
      fields, and fail validation when a lane lacks an explicit role.

#### 5.2 Separate upstream lifecycle, lane role, and public support claim

- **What changes**
  - Add distinct lane metadata fields for:
    - upstream lifecycle (`eol`, `lts`, `current`, etc.)
    - Neovex lane role (`validation`, `primary`, `preview`)
    - public support state / claim dependency
  - Make structured reports and docs consume those fields separately.

- **Why this ordering**
  - Future Node line onboarding stays honest only if lifecycle and support
    labels are not collapsed together.

- **Verification**
  - `cargo test -p neovex-runtime node_compat_lane_metadata_golden`
    - proves the report keeps lifecycle, lane role, and support claim distinct

- **Completion gate**
  - Node 20 can remain a validation lane after EOL without confusing users into
    thinking it is still the primary runtime contract.

- **Risks**
  - One "status" field reappears in downstream summaries.
    - Mitigation: keep separate schema fields and reject report generation that
      attempts to merge them.

### Phase 6: Correctness — Version-Specific Behavior and Bare Specifiers

#### 6.1 Version-specific behavior and multi-target runtime support wiring

- **What changes**
  - Make lane manifests declare the runtime target they expect to execute
    against, even while that target remains `node22` for all current lanes.
  - Prepare the harness to consume future runtime-target expansions through the
    already-inspected seams in:
    - `crates/neovex-runtime/src/limits.rs`
    - `crates/neovex-runtime/src/runtime/bootstrap/source.rs`
    - `crates/neovex-runtime/src/runtime/bootstrap/state.rs`
  - Move lane-specific behavior differences out of ad hoc fixture wrappers and
    into explicit expectation or target-selection metadata.
  - Keep preview-only top-level skip capture and validation-only divergence
    behavior as manifest data rather than hardcoded lane branches.

- **Why this ordering**
  - This phase depends on the manifest, reporting, and scale work and overlaps
    active NLC runtime semantics, so it should not lead the refactor.

- **Verification**
  - `cargo test -p neovex-runtime node_compat_runtime_target_selection`
    - proves lane metadata resolves to the intended runtime target
  - `cargo test -p neovex-runtime node_compat_version_divergence_golden`
    - proves lane-specific expectations are explicit and deterministic

- **Completion gate**
  - Version-specific fixture truth is expressible without reopening harness
    structure.
  - The harness can adopt a future `RuntimeCompatibilityTarget` expansion
    through data plus narrowly-scoped runtime changes instead of another broad
    harness rewrite.

- **Risks**
  - Phase 6 implementation collides with active NLC runtime work.
    - Mitigation: gate semantics-touching edits behind explicit coordination
      with the current NLC item owner and keep non-semantic harness work in
      Phases 1-5 moving independently.

#### 6.2 Bare builtin specifier support in both CJS and ESM

- **What changes**
  - Add explicit fixture and canary coverage for:
    - ESM bare builtin imports (`import "fs"`)
    - CJS bare builtin requires (`require("fs")`)
    - `createRequire(...)`
    - `process.getBuiltinModule(...)`
    - explicit `node:` forms
  - Align harness expectations with the loader/runtime seams already visible in
    `crates/neovex-runtime/src/module_loader.rs`.
  - Add manifest metadata such as `resolverMode: esm | cjs | both` so failures
    can be reported precisely instead of as generic loader regressions.

- **Why this ordering**
  - Bare-builtin correctness is a high-value runtime truth problem, but it
    should be proven after the harness can express the relevant axes cleanly.

- **Verification**
  - `cargo test -p neovex-runtime node_compat_bare_builtin_resolution`
    - proves CJS and ESM bare-builtin paths are both covered
  - focused manifested fixture reruns for representative loader cases recorded
    by the active NLC owner during implementation

- **Completion gate**
  - Bare builtin support is a named proof surface in both CJS and ESM rather
    than an accidental side effect of only one loader path.

- **Risks**
  - The harness reports ESM success while CJS still drifts.
    - Mitigation: keep resolver mode explicit in manifests, reports, and canary
      mapping.

## Manifest Schema Proposal

Illustrative shapes only; exact field names may tighten during implementation,
but the topology below is the contract this plan expects:

- `schema.json` validates all manifest file types
- `lanes/<lane>.json` owns lane metadata only
- `fixtures/<family>.json` owns self-contained per-fixture entries with
  per-lane expectations
- `preludes.json` owns named prelude/postlude definitions plus oracle
  portability metadata

The loader concatenates `fixtures/*.json`, validates references against known
lanes and named preludes, and rejects duplicate fixture ids instead of merging
partial fragments from multiple files.

`lanes/node24.json`

```json
{
  "schemaVersion": 1,
  "id": "node24",
  "upstreamTag": "v24.x.y",
  "upstreamCommit": "abc123",
  "upstreamLifecycle": "lts",
  "laneRole": "preview",
  "runtimeTarget": "node22",
  "captureTopLevelSkip": true
}
```

`fixtures/core-semantics.json`

```json
{
  "schemaVersion": 1,
  "fixtures": [
    {
      "id": "test/parallel/test-buffer-alloc.js",
      "testTier": "upstream_vendored",
      "slice": "nlc3-core-semantics",
      "suiteKind": "parallel",
      "executionClass": "parallel",
      "profiles": ["application"],
      "capabilities": ["bundle-root-fs"],
      "platforms": ["darwin", "linux", "windows"],
      "resolverMode": "both",
      "source": {
        "canonical": "canonical/node22/test/parallel/test-buffer-alloc.js",
        "overrides": {
          "node20": "overrides/node20/test/parallel/test-buffer-alloc.js",
          "node24": "overrides/node24/test/parallel/test-buffer-alloc.js"
        }
      },
      "extraFiles": [
        {
          "runtimePath": "test/common/index.js",
          "fixturePath": "shared/test/common/index.js"
        }
      ],
      "preludes": ["pending_deprecation"],
      "postludes": [],
      "expectations": {
        "node22": { "state": "pass" },
        "node20": {
          "state": "expected_failure",
          "reason": "validation_lane_divergence",
          "unexpectedPass": "fail"
        },
        "node24": {
          "state": "skip",
          "reason": "preview_import_gap"
        }
      }
    }
  ]
}
```

`fixtures/supplementary.json` (example supplementary tier entry)

```json
{
  "schemaVersion": 1,
  "fixtures": [
    {
      "id": "supplementary/builtin-completeness.js",
      "testTier": "supplementary",
      "supplementaryCategory": "builtin_completeness",
      "slice": "supplementary-bridge",
      "suiteKind": "parallel",
      "executionClass": "parallel",
      "profiles": ["application", "tooling"],
      "capabilities": [],
      "platforms": ["darwin", "linux", "windows"],
      "resolverMode": "both",
      "source": {
        "canonical": "supplementary/builtin-completeness.js"
      },
      "extraFiles": [],
      "preludes": [],
      "postludes": [],
      "expectations": {
        "node22": { "state": "pass" },
        "node20": { "state": "pass" },
        "node24": { "state": "pass" }
      }
    }
  ]
}
```

## Verification Strategy

The harness refactor is only complete when it leaves durable, layered proof.
Counts and pass totals must be measured at execution time; they should never be
hardcoded into plan text or report expectations.

The focused `cargo test -p neovex-runtime ...` entries below are the target
proof surfaces this refactor should create or preserve. If implementation
chooses different final module names, it must update the table and land
matching focused coverage in the same change.

| Command | What it proves |
| --- | --- |
| `cargo test -p neovex-runtime node_compat_manifest_schema` | Manifest schema validation and parse-time guardrails |
| `cargo test -p neovex-runtime node_compat_fixture_materialization` | Canonical-plus-overrides resolution recreates the intended lane bundle |
| `cargo test -p neovex-runtime node_compat_expectation_golden` | Skip / expected-failure / flaky / unexpected-pass rules behave deterministically |
| `cargo test -p neovex-runtime node_compat_report_schema` | Structured report output remains versioned and parseable |
| `cargo test -p neovex-runtime node_compat_oracle_classification` | Real-Node shadow results are classified consistently |
| `cargo test -p neovex-runtime node_compat_bare_builtin_resolution` | CJS and ESM bare-builtin proof stays explicit |
| `cargo test -p neovex-runtime node_compat_supplementary_builtin_completeness` | All supported builtins importable in all specifier forms per lane |
| `cargo test -p neovex-runtime node_compat_supplementary_module_bridge` | CJS/ESM interop paths resolve correctly |
| `cargo test -p neovex-runtime node_compat_supplementary_global_injection` | `__dirname`/`__filename`/`require` injection correct per module type |
| `make node-compat-sync LANE=node22 DRY_RUN=1` | Pinned sync tooling is reachable through the canonical developer surface |
| `make node-compat-oracle LANE=node22 SAMPLE=test/parallel/test-buffer-alloc.js` | Shadow-oracle lane is reachable through one documented entrypoint |
| `make verify-harness SURFACE=runtime` | Repo-owned focused runtime harness lane still passes after the refactor |
| `cargo fmt --all --check` | Formatting remains repo-clean |
| `make clippy` | Lint and correctness baseline across the workspace |
| `make node-compat-canaries-bootstrap` | Pinned canary dependencies install through the canonical repo-owned entrypoint |
| `make node-compat-canaries PROFILE=application` | Existing canary surface still executes under the normalized trust layer |

Implementation note:

- if additional focused commands are introduced during the refactor, record them
  in the execution log with what they prove rather than replacing the table
  above with one-off shell history

## Risks And Mitigations

- **Risk:** Manifest migration loses subtle current behavior such as
  pending-deprecation or process-exit handling.
  - **Mitigation:** keep named behavior catalogs and bundle golden tests before
    deleting old code paths.
- **Risk:** Node-style `parallel` taxonomy is mistaken for immediate concurrent
  embedded execution.
  - **Mitigation:** preserve the runtime-suite lock and keep execution class
    separate from suite kind.
- **Risk:** Deduped fixtures become less auditable than the current duplicated
  trees.
  - **Mitigation:** require provenance files, materialized diffs, and override
    source reporting.
- **Risk:** Expected-failure metadata becomes a hiding place for real
  regressions.
  - **Mitigation:** unexpected passes fail by default and every non-pass state
    requires a structured reason class.
- **Risk:** Canaries overclaim Application support from Tooling-only proof.
  - **Mitigation:** make profile a first-class report and claim-map field.
- **Risk:** Lane-scalability work implies future runtime support that does not
  yet exist.
  - **Mitigation:** keep upstream lifecycle, lane role, runtime target, and
    public claim as distinct fields in manifests and reports.
- **Risk:** Supplementary test tier becomes a dumping ground for ad hoc
  assertions instead of a focused proof layer.
  - **Mitigation:** require every supplementary fixture to map to one of the
    six named categories (builtin completeness, module bridge, global
    injection, process shape, resource safety, framework patterns) and cite
    its Deno/Bun precedent. Reject uncategorized additions.
- **Risk:** Phase 6 collides with active NLC runtime work.
  - **Mitigation:** treat Phases 1-5 as independently actionable and gate
    semantics-touching changes behind NLC-owner coordination.

## Closeout / Completion Gates

This companion plan is complete only when all of the following are true:

1. `node_compat.rs` macro and batch sprawl has been replaced by a manifest-
   driven harness, and no Node-compat test root file exceeds repo modularity
   thresholds without an explicit accepted exception.
2. Node 20 / 22 / 24 fixture lanes each have exact provenance metadata,
   reproducible sync tooling, and diff reporting.
3. Running the sync tooling against pinned upstream tags recreates the checked-
   in fixture state or emits only a documented, auditable diff.
4. Canonical-plus-overrides fixture layout replaces the current duplicated lane
   trees while preserving auditable materialization.
5. All current `#[ignore]` behavior is represented explicitly in machine-
   readable expectations, with unexpected-pass detection in place.
6. Structured reports exist for per-lane, per-slice, per-profile, and per-
   suite results and feed NLC-owned manifests/failure inventories.
7. Golden tests and a real-Node shadow-oracle lane verify the harness itself,
   not only the runtime-under-test.
8. Per-commit and nightly CI lanes are explicitly wired to the canonical
   harness entrypoints, with structured reports retained as artifacts and
   oracle/canary breadth scoped intentionally.
9. Supplementary behavioral tests exist for at least the builtin completeness,
   module resolution bridge, and global injection categories, with per-lane
   execution and results reported separately from upstream vendored fixtures.
10. Package/framework canaries are pinned, profile-aware, mapped to public
   claims, and integrated with the structured reporting surface.
11. Adding a future Node lane such as Node 26 requires no Rust harness changes
    for manifest, sync, or reporting paths.
12. The global runtime-suite serialization lock remains intact unless a later
    explicit plan proves that embedded Node fixtures can execute safely without
    it.
13. `docs/plans/node-lts-compatibility-plan.md`,
    `docs/plans/node-compat-test-infrastructure-plan.md`, and
    `docs/plans/node-compat-future-lanes-and-correctness-plan.md` now all
    serve as completed Node-compat baselines; future roadmap work must start
    from a fresh active plan rather than reactivating one of these records.
