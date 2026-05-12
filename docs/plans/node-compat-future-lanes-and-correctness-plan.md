# Node Compatibility Future Lanes And Correctness Plan

Status: done

Completed successor baseline for post-`NLC10` Node-compatibility follow-on
work. This plan builds on the completed semantic closeout in
`docs/plans/node-lts-compatibility-plan.md` and the completed harness baseline
in `docs/plans/node-compat-test-infrastructure-plan.md`.

## Status

- **Plan status:** `done`
- **Control item:** `—`
- **Status values:** `pending`, `in_progress`, `done`, `blocked`
- **Primary source of truth:** this file plus the current git worktree
- **Completed prerequisites:**
  - `docs/plans/node-compatible-runtime-plan.md`
  - `docs/plans/node-lts-compatibility-plan.md`
  - `docs/plans/node-compat-test-infrastructure-plan.md`

## Objective

Keep Node-compatibility evidence trustworthy after the completed `NLC` wave by
making future lane onboarding and deeper correctness auditing explicit,
measured, and resumable.

This plan owns the follow-on work that is no longer appropriate to carry under
the completed `NLC` control plane:

- data-driven onboarding for future lanes such as Node26+
- clearer separation between upstream lifecycle metadata, lane role, and public
  claim status
- supplementary correctness evidence for version-specific behavior and the
  CJS/ESM/bare-specifier bridge

## Non-Goals

- Reopening completed `NLC` family closeout decisions without new runtime
  evidence.
- Claiming public Node26+ support from manifest plumbing alone.
- Treating successor harness work as a substitute for runtime semantics proof.

## Current Seam

- This successor wave is complete.
- The checked-in baseline now carries:
  - lane-keyed manifest sources that do not require new Rust fields for future
    lane ids
  - explicit upstream-line versus lane-role versus public-claim metadata across
    manifest, report, canary, oracle, and dashboard outputs
  - a supplementary correctness tier with green builtin, module-bridge, and
    global-injection proof slices plus one explicit version-specific
    expected-failure watchpoint slice
- Any additional Node-compatibility roadmap work now requires a new active
  plan rather than silently extending this completed successor wave.

## Roadmap

### NCF1 Manifest Lane-Key Generalization

**Status:** `done`

Generalize manifest/catalog/report seed resolution so future lane ids no longer
require new Rust fields in the manifest substrate.

Completion gate:

- manifest schema accepts lane-keyed future fixture sources
- catalog validation resolves lane-keyed sources without hardcoded
  `node20` / `node22` / `node24` field access
- topology/resolution proofs include a synthetic future-lane case
- current carried `node20` / `node22` / `node24` fixtures and reports remain
  green

### NCF2 Lane Lifecycle And Claim Separation

**Status:** `done`

Separate future-lane metadata into explicit upstream lifecycle, Neovex lane
role, and public-claim state so preview, validation, and future research lanes
do not masquerade as support claims.

Completion gate:

- lane metadata and report surfaces distinguish upstream line, execution role,
  and public claim state
- future lane metadata can be added without widening public support language
- docs and artifact outputs use the separated model consistently

### NCF3 Correctness Expansion

**Status:** `done`

Add supplementary correctness evidence for version-specific behavior and the
module-resolution bridge where vendored Node fixtures are not sufficient by
themselves.

Completion gate:

- supplementary fixture tier exists for successor-scope correctness probes
- at least one version-specific behavior slice and one bare-specifier bridge
  slice are checked in with reportable outcomes
- support-facing docs cite measured outcomes instead of aspirational parity

### NCF4 Successor Closeout

**Status:** `done`

Close the successor wave only after the future-lane substrate, lifecycle/claim
separation, and correctness supplements are all evidence-backed and routed
through the checked-in report/dashboard path.

Completion gate:

- active follow-on items are `done`
- routing docs point at the completed baselines and require a fresh active
  plan before any new Node-compat roadmap wave
- completed baselines and successor-scope follow-on boundaries are explicit

## Checkpoints

| Date | Item | Status | Notes | Verification |
|------|------|--------|-------|--------------|
| 2026-05-11 | NCF1 | `done` | Activated the successor plan, repointed routing docs to it, and generalized the manifest seed lane-source model so schema/catalog/resolution proofs accept synthetic future lane keys such as `node26` without new Rust fields. | `cargo test -p neovex-runtime node_compat_manifest_ -- --nocapture --test-threads=1` → `32 passed`, `1 ignored`; `cargo fmt --all --check`; `make clippy` |
| 2026-05-11 | NCF2 | `done` | Promoted lane metadata from checked-in JSON into the live manifest catalog and report substrate, then propagated that separated model through canary, oracle, dashboard, and generated artifact outputs so lane summaries now carry upstream fixture line, lane role, public-contract role, and runtime target/profile instead of flattening lanes to bare string ids. | `cargo test -p neovex-runtime node_compat_manifest_ -- --nocapture --test-threads=1` → `32 passed`, `1 ignored`; `cargo test -p neovex-runtime node_compat_oracle_ -- --nocapture --test-threads=1` → `2 passed`, `1 ignored`; host reruns of `make node-compat-canaries PROFILE=application`, `make node-compat-canaries PROFILE=tooling`, `make node-compat-oracle LANE=node22 SAMPLE=test/parallel/test-buffer-alloc.js NODE_BIN=/opt/homebrew/Cellar/node@22/22.22.2_2/bin/node`, and `make node-compat-dashboard` refreshed the evidence bundle |
| 2026-05-11 | NCF3 | `done` | Added the supplementary correctness tier as checked-in manifest families plus machine-readable report artifacts. `supplementary-builtin-completeness`, `supplementary-module-resolution-bridge`, and `supplementary-global-injection-fidelity` are green across `node20`, `node22`, and `node24`; `supplementary-process-release-shape` is an explicit expected-failure slice that captures the current cross-lane `process.version` / `process.release.lts` drift instead of hiding it. Support-facing runtime docs now cite those measured outcomes directly. | `cargo test -p neovex-runtime node_compat_manifest_ -- --nocapture --test-threads=1`; `cargo test -p neovex-runtime node_compat_supplementary_ -- --nocapture --test-threads=1`; `bash scripts/node-compat-report.sh --family loader-context-supplementary --slice supplementary-builtin-completeness --capture-live`; `bash scripts/node-compat-report.sh --family loader-context-supplementary-module-bridge --slice supplementary-module-resolution-bridge --capture-live`; `bash scripts/node-compat-report.sh --family loader-context-supplementary-global-injection --slice supplementary-global-injection-fidelity --capture-live`; `bash scripts/node-compat-report.sh --family process-and-timing-supplementary --slice supplementary-process-release-shape --capture-live` |
| 2026-05-11 | NCF4 | `done` | Closed the successor wave after the future-lane substrate, lifecycle/claim separation, and supplementary correctness evidence were all routed through the checked-in report path. Routing docs now treat this plan as a completed baseline rather than an active owner. | `cargo fmt --all --check`; `make clippy`; `git diff --check` |
