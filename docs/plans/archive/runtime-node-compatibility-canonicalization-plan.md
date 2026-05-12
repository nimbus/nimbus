# Runtime Node Compatibility Canonicalization Plan

Status: in_progress

This plan owns the next Node runtime compatibility cleanup wave after the
Node20 / Node22 / Node24 evidence-hardening work. The goal is not to add a new
runtime implementation. The goal is to make the existing Node compatibility
system cohesive, idiomatic, discoverable, and trustworthy for both maintainers
and users.

The current work proved a large amount of behavior, but the project shape still
reads like several good subsystems placed beside each other:

- Rust execution harnesses live under `crates/neovex-runtime/src/runtime/tests`.
- durable evidence, schemas, classifications, canaries, and package fixtures
  have historically lived under `tests/node-compat`.
- orchestration scripts have historically lived under `scripts/node_compat`.
- generated and curated reports live under `docs/architecture/runtime`.
- Convex-compatible user-facing runtime configuration lives in
  `docs/adapters/convex/compatibility.md`.

That split is understandable historically, but it is not yet the canonical
shape we want for a project that pins multiple Node versions, verifies a
measured compatibility contract, and publishes developer-facing runtime docs.

## Status

- **Plan status:** `in_progress`
- **Primary owner:** this plan for Node runtime compatibility information
  architecture, path canonicalization, generated docs, report wording, and
  harness/data ownership cleanup
- **Completed baselines:**
  - `docs/plans/archive/node-compatible-runtime-plan.md`
  - `docs/plans/archive/node-lts-compatibility-plan.md`
  - `docs/plans/archive/node-compat-test-infrastructure-plan.md`
  - `docs/plans/archive/node-compat-evidence-hardening-plan.md`
  - `docs/plans/archive/node-compat-future-lanes-and-correctness-plan.md`
  - `docs/plans/archive/node-compat-supported-lanes-plan.md`
- **Current evidence root:** `tests/runtime/node/`
- **Current runtime harness root:**
  `crates/neovex-runtime/src/runtime/tests/node/`
- **Current script root:** `scripts/runtime/node/`
- **Current architecture support matrix:**
  `docs/architecture/runtime/node-compat-surface-matrix.md`
- **Current public Convex-compatible runtime contract:**
  `docs/adapters/convex/compatibility.md`

## Objective

Create one canonical Node runtime compatibility system with clear ownership:

- maintainers can find the Node runtime corpus, lane manifests,
  classifications, schemas, canaries, and generated evidence without knowing
  historical plan names
- Rust runtime tests remain close to the runtime internals they validate, but
  they stop owning durable product evidence or large fixture inventories
- user-facing docs explain how to select and use the Node.js runtime before
  diving into test-suite evidence
- generated Markdown reports under `docs/` are table-first, source-linked, and
  derived from checked-in JSON evidence
- support language uses clear user-facing statuses instead of internal
  shorthand such as "green" and "non-green"
- future lanes such as Node26 can be added by extending data and generated
  docs, not by adding another bespoke pile of Rust constants and prose

## Non-Goals

- Do not claim full Node compatibility. Every claim must stay tied to named
  fixtures, canaries, or explicit classifications.
- Do not weaken existing runtime tests, watchpoints, or expected-failure
  validation.
- Do not move compatibility evidence into a crate-private location just because
  the execution harness is Rust.
- Do not preserve old path names as long-lived compatibility shims. Neovex is
  pre-launch; use direct moves with clean references.
- Do not redesign runtime permissions in this plan. Permission-mode taxonomy is
  owned by `docs/plans/runtime-permission-modes-plan.md` if activated.

## Canonical Path Decisions

Use `node` for internal paths because the runtime domain is already clear. Use
`nodejs` for public docs because developers recognize "Node.js runtime" as the
product term.

Target layout:

```text
tests/runtime/node/
  README.md
  lanes/
  manifests/
  classifications/
  expectations/
  schemas/
  canaries/
    networking/
    tooling/
  fixtures/

crates/neovex-runtime/src/runtime/tests/node/
  mod.rs
  runner.rs
  bundle.rs
  manifest.rs
  report.rs
  oracle.rs
  canaries.rs
  supplementary.rs

scripts/runtime/node/
  refresh.py
  status.py
  inventory.py
  classifications.py
  sync.py
  expectations.py
  dashboard.py
  trends.py
  publish_evidence.py
  publish_docs.py
  oracle-run.sh
  canaries-bootstrap.sh
  canaries-run.sh
  validate-claims.sh

docs/runtimes/
  README.md
  nodejs/
    README.md
    configuration.md
    packages-and-bundling.md
    compatibility.md
    evidence/
      latest.md
      node20.md
      node22.md
      node24.md
```

The durable evidence root is `tests/runtime/node/`, not the runtime crate,
because schemas, package canaries, classifications, generated evidence inputs,
and public docs are repo-level compatibility assets. The runtime crate owns
execution helpers and assertions only.

## Documentation Model

The public documentation should read like a product runtime guide first and a
test dashboard second.

Primary user pages:

- `docs/runtimes/README.md`
  - runtime overview and links to available runtime families
- `docs/runtimes/nodejs/README.md`
  - how to opt into Node.js runtime behavior, including `"use node"`
  - default Node target: Node22
  - supported selectable targets: Node20, Node22, Node24
  - relationship to Convex-compatible actions
  - quick examples for `fs` and `node:fs`
- `docs/runtimes/nodejs/configuration.md`
  - `convex.json` `node.nodeVersion`
  - debug entrypoints such as `--debug-node-apis`
  - validation errors and how to fix them
- `docs/runtimes/nodejs/packages-and-bundling.md`
  - local `node_modules` staging
  - `node.externalPackages`
  - explicit packages and `["*"]`
  - what Neovex does not do at invocation time
- `docs/runtimes/nodejs/compatibility.md`
  - compact support-state matrix
  - profile and permission caveats
  - explicit non-claims
- `docs/runtimes/nodejs/evidence/*.md`
  - generated evidence snapshots by lane and latest aggregate

Architecture pages should remain under `docs/architecture/runtime/` and link
to the public docs. Public docs should link back to architecture evidence only
when a reader wants implementation-level detail.

## Report And Status Vocabulary

Internal Rust and JSON may continue to use `green` where it is already a
stable implementation term, but generated Markdown and user docs should prefer
clear status labels:

| Internal concept | User-facing label |
| --- | --- |
| `green` | `Passed` |
| `known red/gap` | `Expected failure / known gap` |
| `skipped/excluded` | `Skipped / excluded` |
| `unmanifested_or_unclassified` | `Unclassified` |
| `green/classified` | `Classified coverage` |
| `ratio` | `Official fixture pass rate` |

Avoid "non-green" in generated prose. If a test is not a pass claim, say
whether it is an expected failure, known gap, skipped/excluded fixture, or
unclassified fixture.

## Work Queue

| ID | Status | Slice | Completion criteria |
| --- | --- | --- | --- |
| RNC1 | done | Plan and ownership baseline | This plan is checked in, listed as active, and the current scattered roots are inventoried before file moves. |
| RNC2 | done | Documentation information architecture | `docs/runtimes/` and `docs/runtimes/nodejs/` exist with user-facing pages that explain runtime selection, config, packages, compatibility, and evidence links. |
| RNC3 | done | Generated docs pipeline | `scripts/runtime/node/publish_docs.py` or equivalent generates `docs/runtimes/nodejs/evidence/*.md` from status/dashboard/trend JSON. |
| RNC4 | done | Evidence root migration | `tests/node-compat/` moves to `tests/runtime/node/`, Makefile/script references update, schemas validate, and no stale path references remain outside archive/history. |
| RNC5 | done | Script root migration | `scripts/node_compat/` moves to `scripts/runtime/node/`, Make targets still provide the same developer entrypoints, and old script paths are gone. |
| RNC6 | done | Runtime harness module split | The large Node compatibility harness is decomposed under `crates/neovex-runtime/src/runtime/tests/node/` without changing behavior or weakening tests. |
| RNC7 | done | Dashboard language cleanup | Generated Markdown uses user-facing labels and table-first sections; JSON field compatibility is preserved only where useful for scripts. |
| RNC8 | done | Lane growth workflow | Adding or refreshing Node20 / Node22 / Node24 / future Node lanes is documented as one command path from upstream tag to regenerated docs. |
| RNC9 | done | Final audit and archive readiness | Path references, docs links, Make targets, schema validation, focused runtime tests, and generated docs are verified; completed Node plans remain archived history. |

## RNC1 Baseline Inventory

Before moving paths, capture the current roots and owners:

- former `tests/node-compat/`, now `tests/runtime/node/`
  - durable repo evidence: canary registry, package canaries, classifications,
    expectations, schemas, and README
- `crates/neovex-runtime/src/runtime/tests/node/mod.rs`
  - primary Rust execution harness and many fixture lists
- `crates/neovex-runtime/src/runtime/tests/node/*.rs`
  - concept-owned Rust harness modules, with public test module names preserved
    by path attributes in `crates/neovex-runtime/src/runtime.rs`
- `crates/neovex-runtime/src/runtime/tests/node_compat_manifests/`
  - lane and fixture manifests currently embedded near the Rust harness
- `crates/neovex-runtime/src/runtime/tests/node_compat_fixtures/`
  - vendored upstream fixture corpus currently embedded near the Rust harness
- former `scripts/node_compat/`, now `scripts/runtime/node/`
  - status, inventory, sync, dashboard, trends, oracle, canary, expectations,
    and publish orchestration
- `docs/architecture/runtime/node-compat-evidence/latest/`
  - currently published generated evidence snapshots
- `docs/architecture/runtime/node-compat-surface-matrix.md`
  - architecture-level support matrix
- `docs/adapters/convex/compatibility.md`
  - current user-facing Node runtime configuration and package behavior

Current inventory snapshot:

| Root | Current file count | Notes |
| --- | ---: | --- |
| former `tests/node-compat/`, now `tests/runtime/node/` | 30 tracked files at depth <= 3, plus installed canary dependencies | durable evidence, schemas, classifications, expectations, canaries, README |
| former `scripts/node_compat/`, now `scripts/runtime/node/` | 17 source scripts, plus ignored `__pycache__` outputs | orchestration, report generation, canaries, oracle, schema validation |
| `crates/neovex-runtime/src/runtime/tests/node/*.rs` | 9 Rust files | execution harness, manifest catalog/report/topology, oracle, canary registry, and supplementary batch data |
| `crates/neovex-runtime/src/runtime/tests/node_compat_manifests/` | lane, fixture, prelude, and schema JSON | currently crate-local data that should move toward repo-level evidence ownership |
| `crates/neovex-runtime/src/runtime/tests/node_compat_fixtures/` | vendored upstream and supplementary fixture corpus | currently crate-local fixture corpus used by the Rust harness |

High-risk path references captured before migration:

- Make targets called the old script root before RNC5 and now call
  `scripts/runtime/node/*`.
- Python scripts referenced the evidence root plus
  `node_compat_manifests`, and `node_compat_fixtures`.
- Rust harness files under `tests/node/` use relative `include_str!("../node_compat_manifests/...")`
  and `include_str!("../node_compat_fixtures/...")` references so the fixture
  and manifest inputs stay stable while the Rust source ownership is cleaner.
- Runtime tests and canary registry tests reference
  `tests/runtime/node/{networking-canaries,tooling-canaries,canary-registry.json}`.
- Generated evidence snapshots embed source paths and must be regenerated after
  path migration rather than hand-edited.

## Verification Gates

Run focused verification after each migration slice:

```bash
make node-compat-status
make node-compat-dashboard
make node-compat-trends
make node-compat-publish-evidence
make node-compat-validate-claims
cargo test -p neovex-runtime node_compat -- --nocapture --test-threads=1
```

Use narrower commands when a slice is path-only and does not affect runtime
execution. Do not mark a slice complete unless path references, generated docs,
and the relevant focused verification agree.

## Closeout Criteria

This plan is complete when:

- maintainers have one obvious compatibility data root:
  `tests/runtime/node/`
- maintainers have one obvious script root:
  `scripts/runtime/node/`
- Rust test code is concept-owned under
  `crates/neovex-runtime/src/runtime/tests/node/`
- user-facing Node.js runtime docs live under `docs/runtimes/nodejs/`
- generated evidence pages under `docs/runtimes/nodejs/evidence/` are produced
  from the same JSON evidence used by dashboards
- generated Markdown uses `Passed`, `Expected failure / known gap`,
  `Skipped / excluded`, and `Unclassified` rather than ambiguous red/green
  shorthand
- Node20, Node22, and Node24 remain supported selectable lanes, with Node22 as
  the documented default until an explicit Node24-default migration
- no active docs point users to archived plans for current runtime behavior
- the final audit finds no stale former-path `tests/node-compat/` or
  `scripts/node_compat/` references outside archived history or migration notes

## Progress Log

| Date | Slice | Status | Notes | Verification |
| --- | --- | --- | --- | --- |
| 2026-05-12 | RNC1 | done | Created the active plan, listed it in `docs/plans/README.md`, and captured the then-current evidence/script/harness roots plus high-risk path references before migration. | `find tests/node-compat -maxdepth 3 -type f`; `find scripts/node_compat -maxdepth 2 -type f`; `find crates/neovex-runtime/src/runtime/tests -maxdepth 2 -type f | rg 'node_compat|node/'`; `rg -n 'tests/node-compat|scripts/node_compat|node_compat_manifests|node_compat_fixtures|node-compat-evidence|node-compat-surface-matrix|node-lts-compat' README.md docs Makefile scripts crates packages tests --glob '!docs/plans/archive/**'` |
| 2026-05-12 | RNC2 | done | Added the public `docs/runtimes/` and `docs/runtimes/nodejs/` information architecture with runtime overview, Node.js quickstart, configuration, package/bundling, compatibility, and evidence entry pages. Linked it from `docs/README.md` and the Convex compatibility contract. | `git diff --check -- Makefile docs/README.md docs/adapters/convex/compatibility.md docs/plans/README.md docs/plans/runtime-node-compatibility-canonicalization-plan.md docs/runtimes scripts/runtime/node/publish_docs.py` |
| 2026-05-12 | RNC3 | done | Added `scripts/runtime/node/publish_docs.py`, wired `make node-compat-publish-docs`, and generated `docs/runtimes/nodejs/evidence/latest.md`, `node20.md`, `node22.md`, and `node24.md` from the checked-in status/dashboard/trend evidence. | `make node-compat-publish-docs`; `rg -n '\\| .* \\| .* \\| .* \\| pass \\||agreement_pass|Runtime \\| Oracle' docs/runtimes/nodejs/evidence`; `git diff --check -- Makefile docs/README.md docs/adapters/convex/compatibility.md docs/plans/README.md docs/plans/runtime-node-compatibility-canonicalization-plan.md docs/runtimes scripts/runtime/node/publish_docs.py` |
| 2026-05-12 | RNC4 | done | Moved the evidence root to `tests/runtime/node/`, updated script/Rust/doc references, regenerated architecture and public evidence snapshots from the new paths, and confirmed only intentional migration notes still mention the former root. | `make node-compat-expectations-validate`; `make node-compat-status`; `make node-compat-inventory LANE=node20`; `make node-compat-inventory LANE=node22`; `make node-compat-inventory LANE=node24`; `make node-compat-dashboard`; `make node-compat-trends`; `make node-compat-publish-evidence`; `make node-compat-publish-docs`; `make node-compat-validate-claims`; `rg -n 'tests/node-compat' README.md docs Makefile scripts crates packages tests --glob '!docs/plans/archive/**'`; `test ! -e tests/node-compat`; `git diff --check -- tests/runtime/node tests/node-compat scripts/runtime/node crates/neovex-runtime/src/runtime/tests/basic_invocation.rs crates/neovex-runtime/src/runtime/tests/node_compat_canary_registry.rs docs/architecture/runtime/node-compat-evidence/latest docs/runtimes/nodejs/evidence docs/plans/runtime-node-compatibility-canonicalization-plan.md` |
| 2026-05-12 | RNC5 | done | Moved Node compatibility orchestration scripts to `scripts/runtime/node/`, updated Make targets and refresh/oracle self-references, fixed moved Python and shell repo-root calculations, and verified the old script root is absent. | `make node-compat-expectations-validate`; `make node-compat-status`; `make node-compat-inventory LANE=node20`; `make node-compat-inventory LANE=node22`; `make node-compat-inventory LANE=node24`; `make node-compat-dashboard`; `make node-compat-trends`; `make node-compat-publish-evidence`; `make node-compat-publish-docs`; `make node-compat-validate-claims`; `make node-compat-refresh LANE=node22 DRY_RUN=1`; `rg -n 'scripts/node_compat' README.md docs Makefile scripts crates packages tests --glob '!docs/plans/archive/**'`; `test ! -e scripts/node_compat`; `git diff --check -- Makefile scripts/runtime/node scripts/node_compat docs/architecture/runtime/node-compat-evidence/latest docs/architecture/runtime/node-lts-compat docs/runtimes docs/plans/runtime-node-compatibility-canonicalization-plan.md` |
| 2026-05-12 | RNC6 | done | Moved the Rust Node compatibility harness and support modules under `crates/neovex-runtime/src/runtime/tests/node/`, preserved the existing `runtime::tests::node_compat*` module names with path attributes, updated source-path schemas and watchpoint metadata, and regenerated architecture/public evidence from the new source path. | `cargo test -p neovex-runtime node_compat_manifest_metadata -- --nocapture`; `cargo test -p neovex-runtime node_compat_manifest_topology -- --nocapture`; `cargo test -p neovex-runtime node_compat_canary_registry -- --nocapture`; `make node-compat-expectations-validate`; `make node-compat-status`; `make node-compat-inventory LANE=node20`; `make node-compat-inventory LANE=node22`; `make node-compat-inventory LANE=node24`; `make node-compat-dashboard`; `make node-compat-trends`; `make node-compat-publish-evidence`; `make node-compat-publish-docs`; `make node-compat-validate-claims`; `rg -n 'crates/neovex-runtime/src/runtime/tests/node_compat\\.rs|include_str!\\(\\s*\"node_compat_|include_str!\\(\"node_compat_|scripts/node_compat|tests/node-compat' README.md docs Makefile scripts crates packages tests --glob '!docs/plans/archive/**'`; `test ! -e crates/neovex-runtime/src/runtime/tests/node_compat.rs`; `test ! -e crates/neovex-runtime/src/runtime/tests/node_compat` |
| 2026-05-12 | RNC7 | done | Reworded generated status, dashboard, trend, architecture evidence, and public runtime evidence Markdown from internal `green`/raw status shorthand to user-facing `Passed`, `Expected failure / known gap`, `Skipped / excluded`, and `Unclassified` language while preserving JSON field compatibility. | `make node-compat-status`; `make node-compat-dashboard`; `make node-compat-trends`; `make node-compat-publish-evidence`; `make node-compat-publish-docs`; `make node-compat-validate-claims`; `rg -n '\\bGreen\\b|Known red|red/skip|Green/classified|Documented green|agreement_pass|\\| `pass`|\\| pass \\||expected_failure|expected_gap|expected_skip|\\| `passed`|\\| passed \\||\\| `fail`|\\| fail \\|' docs/architecture/runtime/node-compat-evidence/latest/*.md docs/runtimes/nodejs/evidence/*.md target/node-compat/dashboard/dashboard-summary.md target/node-compat/status/status-summary.md target/node-compat/trends/trend-summary.md` |
| 2026-05-12 | RNC8 | done | Documented the canonical `make node-compat-refresh` workflow, added the developer-facing refresh guide, moved the live slice report wrapper to `scripts/runtime/node/report.sh`, made refresh publish public docs, and loosened refresh/sync report schemas plus status rendering so future checked-in `nodeNN` lanes do not require dashboard code edits. | `make node-compat-refresh LANE=node22 DRY_RUN=1`; `bash scripts/runtime/node/report.sh --help`; `rg -n 'scripts/node-compat-report\\.sh|scripts/runtime/node/report\\.sh|node-compat-refresh|publish_docs' README.md docs Makefile scripts --glob '!docs/plans/archive/**'`; `test ! -e scripts/node-compat-report.sh`; `git diff --check -- Makefile scripts/runtime/node docs/runtimes docs/architecture/runtime/deno-vs-neovex-node-compat.md tests/runtime/node/schemas docs/plans/runtime-node-compatibility-canonicalization-plan.md` |
| 2026-05-12 | RNC9 | done | Completed the final audit, corrected false passed evidence for Node20 `test-assert-checktag.js`, `test-console-clear.js`, and cross-lane `test-fs-realpath.js`, made classification/status generation lane-aware, regenerated architecture and public runtime evidence, and verified only intentional migration notes mention former roots. The authoritative status now uses stricter lane-local passed counts: Node20 `904 / 1308`, Node22 `876 / 1283`, and Node24 `925 / 1495`, with all remaining vendored files explicitly classified. | `cargo fmt --all --check`; `cargo test -p neovex-runtime node20_nlc8_worker_main_thread_batch_fixture -- --nocapture --test-threads=1`; `cargo test -p neovex-runtime node20_supported_lane_executes_official_core_semantics_subset -- --nocapture --test-threads=1`; `cargo test -p neovex-runtime node_compat_manifest_metadata -- --nocapture`; `cargo test -p neovex-runtime node_compat_manifest_topology -- --nocapture`; `make node-compat-classifications`; `make node-compat-classifications CHECK=1`; `make node-compat-expectations-sync`; `make node-compat-expectations-validate`; `make node-compat-status`; `make node-compat-inventory LANE=node20`; `make node-compat-inventory LANE=node22`; `make node-compat-inventory LANE=node24`; `make node-compat-dashboard`; `make node-compat-trends`; `make node-compat-publish-evidence`; `make node-compat-publish-docs`; `make node-compat-validate-claims`; `make node-compat-refresh LANE=node22 DRY_RUN=1`; `git diff --check -- Makefile scripts/runtime/node tests/runtime/node docs/runtimes docs/architecture/runtime/node-compat-evidence/latest docs/architecture/runtime/deno-vs-neovex-node-compat.md docs/plans/runtime-node-compatibility-canonicalization-plan.md crates/neovex-runtime/src/runtime.rs crates/neovex-runtime/src/runtime/tests/node crates/neovex-runtime/src/runtime/tests/node_compat_manifests`; `rg -n 'tests/node-compat|scripts/node_compat|scripts/node-compat-report\\.sh|crates/neovex-runtime/src/runtime/tests/node_compat\\.rs|include_str!\\(\\s*\"node_compat_|include_str!\\(\"node_compat_' README.md docs Makefile scripts crates packages tests --glob '!docs/plans/archive/**'`; `rg -n '\\bGreen\\b|Known red|red/skip|Green/classified|Documented green|agreement_pass|\\| `pass`|\\| pass \\||expected_failure|expected_gap|expected_skip|\\| `passed`|\\| passed \\||\\| `fail`|\\| fail \\|' docs/architecture/runtime/node-compat-evidence/latest/*.md docs/runtimes/nodejs/evidence/*.md target/node-compat/dashboard/dashboard-summary.md target/node-compat/status/status-summary.md target/node-compat/trends/trend-summary.md` |
