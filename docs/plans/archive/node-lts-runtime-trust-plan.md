# Node LTS Runtime Trust Plan (NLRT)

Status: `done`
Owner: `runtime / tenant / bridge / convex node-compat`
Research baseline:
`docs/plans/research/node-lts-runtime-and-deno-fork-strategy.md`
Proof directory: `docs/plans/proof/node-lts-runtime-trust/`
Verifier: `scripts/verify-node-lts-runtime-trust.sh`

## Goal

Build a Node.js LTS compatibility program that can follow Node's release train
without permanently favoring one major line, while keeping Nimbus' embedded
Deno/V8 runtime, fork provenance, permission model, and evidence claims
enterprise-trustworthy.

The plan succeeds when active non-EOL Node LTS lanes are described by a
data-driven registry, runtime metadata is truthful per lane, generated evidence
is the only source of public support claims, the Deno fork patch closure is
script-verified, and production Node permission profiles no longer look like
local-dev grants that are rescued only by a later admission gate.

## Control Plane

This plan is the authoritative execution state. Chat history is useful context,
but an agent resuming after compaction must be able to continue from this file,
the research baseline, proof files, and the current working tree.

### Resume Protocol

1. Read `AGENTS.md`, then this plan, then the research baseline.
2. Check `git status --short` and treat unrelated dirty files as user or prior
   work. Do not revert them.
3. Read `docs/plans/archive/node-compat-cron-greening-plan.md` before touching Node
   bootstrap, process metadata, or node-compat harness code. If NCG is still
   active and touches the same files, coordinate rather than interleaving
   hidden assumptions.
4. Inspect the ledger and execution log. Continue the single `in_progress` row
   if one exists. If none exists, start the lowest-numbered `pending` row.
5. Load only the files named by that row, the ownership baseline below, the
   acceptance criteria table, and the immediately relevant tests.
6. Before handoff, context loss, or final response, update the row status,
   execution log, and proof artifact for completed work.

### State Rules

- At most one ledger row may be `in_progress`.
- A row may move to `done` only after every listed acceptance criterion passes,
  every required proof artifact exists, and the execution log records the exact
  verification output.
- A row may not be completed by weakening tests, deleting failing fixtures,
  lowering support claims to hide failures, or moving a failure into an ignore
  list without a named watchpoint and written reason.
- Every external-source fact that can drift, such as Node LTS status or fork
  tag identity, must be rechecked during the row that depends on it and written
  into that row's proof artifact with dates.
- Every generated artifact update must name the generator command and the
  observed result count or summary.
- The final closeout is not complete until the verifier exists and passes from
  a fresh shell.

### Proof Artifact Contract

Each NLRT row writes one proof file:

```text
docs/plans/proof/node-lts-runtime-trust/nlrt<N>-<slug>.md
```

Each proof file must include:

- Date, authoring agent, git status summary, and relevant SHAs/tags.
- Files changed.
- Decisions made and alternatives rejected.
- Verification commands run with concrete pass/fail counts or command output
  summaries.
- Remaining risks, if any, tied to a later NLRT row or explicitly resolved.

## Current Baseline

As of 2026-05-27:

- Node20 is EOL and should become legacy/grace only.
- Node22 is Maintenance LTS and must be a supported LTS lane.
- Node24 is Active LTS and must be a supported LTS lane.
- Node26 is Current and should be preview-only until it reaches LTS.
- `docs/plans/archive/node-compat-cron-greening-plan.md` is a nearer-term plan to green
  the currently failing Node Compatibility cron. NLRT does not silence that
  plan; it builds the next support architecture around it.

Re-audited on 2026-05-28 after the crate extraction/refactor:

- The Deno-family dependency shape in root `Cargo.toml` is still the same plan
  risk: `deno_core` and `deno_node` are resolved through the `nimbus/deno` patch,
  and `v8` is resolved through the `nimbus/rusty_v8` patch. NLRT1 still needs to
  verify Cargo source closure rather than relying on hand inspection.
- `nimbus-runtime` owns runtime compatibility targets, runtime limits/policies,
  process metadata/bootstrap behavior, node-compat harness manifests, and the
  eventual data-driven Node LTS registry.
- `nimbus-tenant` now owns tenant-facing runtime admission and operator policy
  mapping for Node20/Node22/Node24 profiles. Production permission acceptance
  and rejection evidence belongs there.
- `nimbus-bridge` now owns execution-time admission from tenant policy decisions
  into runtime invocation. Fail-closed fallback behavior belongs there.
- `nimbus-convex` now owns Convex manifest runtime selection, runtime lane
  diagnostics, and `"use node"` action packaging/routing. Lane registry changes
  must prove this extracted owner stays in sync.
- `nimbus-server` remains a composition and transport owner for NLRT only where
  HTTP/WebSocket wiring or end-to-end service behavior is being verified.

## Principles

- Compatibility target is API shape, not host authority.
- Product default is not evidence priority.
- Generated evidence beats hand-written prose.
- Active LTS lanes are peers unless a lane is explicitly EOL, preview, or
  legacy-grace.
- Forked Deno code is acceptable only with tag/SHA provenance and a clear
  upstream-or-Nimbus-only disposition.
- Production grants should be least-authority at construction time, not only
  after a downstream rejection path.

## Scope

In scope:

- Node LTS lane registry and generated runtime target metadata.
- `RuntimeCompatibilityTarget` and Node runtime metadata derivation.
- `process.version`, `process.versions.node`, `process.release.lts`, and related
  Node metadata tests.
- Deno-family Cargo patch closure verification.
- Generated Node evidence docs and stale prose prevention.
- Node compatibility fixture provenance and per-lane support status.
- Harness timeout/hang behavior for VM, worker, MessagePort, and subprocess
  fixtures.
- Production versus local-development runtime grant profiles.
- Extracted owner-crate contracts across `nimbus-runtime`, `nimbus-tenant`,
  `nimbus-bridge`, and `nimbus-convex`.
- `nimbus/deno` fork bump workflow documentation and verification.

Non-goals:

- Claiming full Node built-in compatibility.
- Native addon execution in the in-process tenant runtime.
- Supporting odd-numbered Node release lines as enterprise LTS targets.
- Replacing the embedded Deno/V8 runtime with an external real-Node process.
- Removing the microVM route for host-heavy Node behavior.

## Ledger

| NLRT | Description | Status |
| --- | --- | --- |
| NLRT0 | Accept this plan and checkpoint the research baseline. Add routing from `docs/plans/README.md`. Capture the current dirty-worktree caveat, the post-refactor owner-crate map, and make sure this plan does not overwrite the active NCG cron-greening work. | done |
| NLRT1 | Add a Deno fork provenance verifier. It must inspect `Cargo.toml`, `Cargo.lock`, and `cargo tree -p nimbus-runtime`, require the expected `nimbus/deno` and `nimbus/rusty_v8` tags/SHAs for patch-sensitive crates, and require an allowlist with reasons for Deno-family crates that intentionally remain on crates.io. | done |
| NLRT2 | Introduce a data-driven Node LTS lane registry. Include major, lane name, support phase, codename, upstream version, upstream tag, fixture corpus path, LTS start, maintenance start, EOL date, product-default flag, and evidence policy. Mark Node20 as EOL legacy/grace, Node22 and Node24 as supported LTS, and Node26 as preview-current. Name every owner crate that consumes the registry. | done |
| NLRT3 | Generate `RuntimeCompatibilityTarget` metadata from the lane registry or make the enum a thin wrapper around registry records. Remove hard-coded synthetic version strings from `axes.rs`; keep compatibility-target parsing stable for public config and synchronized across `nimbus-tenant` operator policy and `nimbus-convex` runtime lane selection. | done |
| NLRT4 | Make runtime Node metadata truthful per lane. `process.version`, `process.versions.node`, `process.release.lts`, ABI/module metadata, and supplementary process-release-shape probes must match the lane registry or document an intentional non-Node component value. Close the active supplementary process-release-shape failure. | done |
| NLRT5 | De-center Node22 in docs and evidence. Keep a product default, but change dashboards, support tables, and prose so active LTS lanes are peers. Rename Node22-shaped internal composition roots only where it improves clarity without destabilizing bootstrap order. Add a stale-prose check for hand-written Node support claims. | done |
| NLRT6 | Add fixture provenance and sync automation. Every vendored Node fixture corpus must record upstream tag, commit, sync date, and selection command. The refresh path must fail if a supported LTS lane has unknown provenance or unclassified generated results. | done |
| NLRT7 | Harden the Node compatibility harness against hangs. Add per-fixture wall-clock timeouts, diagnostic artifacts for stalled event-loop or MessagePort cases, and a rule that ignored watchpoints are counted as watchpoints, not green support. Revisit the worker MessagePort VM hang before worker threads remain in any production in-process profile. | done |
| NLRT8 | Split runtime permission profiles by deployment intent. Add local-dev Node grants, production in-process Node grants, and production service/microVM Node grants as separate constructors or typed profiles. Remove generic loopback/listen/worker/inspector authority from the production in-process profile, virtualize or remove ambient `NODE_TLS_REJECT_UNAUTHORIZED`, keep `nimbus-tenant` production admission as a fail-closed policy backstop, and keep `nimbus-bridge` execution admission fail-closed when no fallback route is available. | done |
| NLRT9 | Define the Deno fork upstream-first policy in operating docs. For each fork bump, record whether the patch is upstream Deno, Nimbus-only host integration, or temporary carry. Require publish/tag/repin proof before release. | done |
| NLRT10 | Expand active-LTS package canaries and oracle comparisons. For every supported LTS lane, run package canaries that exercise ESM/CJS loading, process metadata, fs/path, streams, timers, crypto, fetch/http, and `nimbus-convex` Convex `"use node"` action packaging. No lane may borrow the default lane's canary result. | done |
| NLRT11 | Closeout. Add `scripts/verify-node-lts-runtime-trust.sh`, update public runtime docs, archive or supersede stale Node support prose, run the verifier plus focused runtime tests, and move this plan to `docs/plans/archive/` with proof links. | done |

## Per-Phase Acceptance Criteria

| NLRT | Required proof artifact | Acceptance criteria |
| --- | --- | --- |
| NLRT0 | `nlrt0-baseline-and-control-plane.md` | Plan status flipped to `active`; proof directory README exists; `docs/plans/README.md` routes to this plan; research baseline exists; current dirty worktree caveat captured; post-refactor owner-crate map captured with relevant Cargo patch tags; NCG overlap note captured; execution log initialized; `npm run docs:validate-refs:strict` passes. |
| NLRT1 | `nlrt1-deno-fork-provenance.md` | A verifier or verifier subcommand prints resolved Deno-family crate sources; patch-sensitive crates resolve to the expected `nimbus/deno` or `nimbus/rusty_v8` tag and SHA; crates.io Deno-family exceptions are allowlisted with reasons; missing or mixed-source crates fail the verifier; proof records `cargo tree -p nimbus-runtime` evidence. |
| NLRT2 | `nlrt2-node-lts-lane-registry.md` | A checked-in lane registry exists; registry schema is validated by a test or script; Node20 is `eol_legacy` or equivalent, Node22 is Maintenance LTS, Node24 is Active LTS, Node26 is Current preview; product default is explicit and separate from evidence policy; `nimbus-runtime`, `nimbus-tenant`, and `nimbus-convex` registry consumers are named; docs cite the registry rather than duplicated hand-maintained lane facts. |
| NLRT3 | `nlrt3-runtime-target-metadata.md` | Runtime compatibility target metadata is derived from the registry or validated against it; public config parsing for `"20"`, `"22"`, and `"24"` still works; unsupported or EOL-active mistakes fail tests; `nimbus-tenant` operator policy mapping and `nimbus-convex` selected runtime lane behavior are tested against the registry; hard-coded synthetic version strings in `axes.rs` are removed or made unreachable for supported LTS metadata. |
| NLRT4 | `nlrt4-truthful-process-metadata.md` | Per-lane tests assert `process.version`, `process.versions.node`, `process.release.name`, `process.release.lts`, and ABI/module metadata; supported LTS lanes pass supplementary process-release-shape probes; intentional differences between Node API contract metadata and actual embedded V8/Deno/Nimbus versions are documented in diagnostics outside the Node API claim. |
| NLRT5 | `nlrt5-equal-lane-evidence-docs.md` | Public docs no longer treat Node22 as evidence-priority language; generated evidence remains the support source of truth; stale prose checks reject hand-written pass-rate or support overclaims; internal Node22-shaped filenames are either justified in proof or renamed with tests; product default language remains explicit. |
| NLRT6 | `nlrt6-fixture-provenance-sync.md` | Every vendored Node fixture corpus records upstream tag, commit, sync date, and selection command; refresh tooling fails on missing provenance; supported LTS lanes fail on unclassified generated results in published support slices; proof includes one dry-run and one checked generated-output comparison. |
| NLRT7 | `nlrt7-harness-timeouts-and-hangs.md` | Node compat harness has per-fixture wall-clock timeout coverage; stalled event-loop, VM, worker, MessagePort, and subprocess diagnostics produce artifacts; ignored watchpoints are reported separately from green support; the known worker MessagePort VM hang is classified with an exit criterion before worker threads remain in any production in-process profile. |
| NLRT8 | `nlrt8-permission-profile-split.md` | Runtime exposes separate local-dev, production in-process, and production service/microVM Node profiles; production in-process profile has no generic loopback, wildcard listen, worker, inspector, run, FFI, or ambient TLS-disable env grants; `nimbus-tenant` production admission tests still reject unsafe custom policies; `nimbus-bridge` execution admission tests fail closed when a fallback route is unavailable; local-dev behavior remains covered by tests. |
| NLRT9 | `nlrt9-deno-fork-upstream-policy.md` | Operating docs describe the fork workflow: unpin to local canonical fork, prove, commit/tag/push fork, repin Nimbus, rerun verification; every carried patch has upstream/Nimbus-only/temporary disposition; release proof requires tag/SHA/changelog mapping. |
| NLRT10 | `nlrt10-active-lts-canaries-and-oracles.md` | Active LTS lanes have lane-local package canaries for ESM/CJS loading, process metadata, fs/path, streams, timers, crypto, fetch/http, and `nimbus-convex` Convex `"use node"` packaging; canary reports cannot borrow the product-default lane; oracle comparisons use version-matched Node binaries or recorded Node output. |
| NLRT11 | `nlrt11-closeout.md` | `scripts/verify-node-lts-runtime-trust.sh` exists and passes; all ledger rows are `done`; public docs and generated evidence are updated; stale Node support prose is archived, regenerated, or made subordinate to generated evidence; final verification commands pass; this plan is archived with proof links. |

## Completion Gate

Create `scripts/verify-node-lts-runtime-trust.sh` during NLRT11. The verifier
must pass these conditions:

1. Plan exists in active or archived location and ledger has no `pending` rows.
2. Research baseline exists and is linked from this plan.
3. Node LTS lane registry exists and includes Node22, Node24, and Node26 with
   current support phases.
4. Node20 is not advertised as active enterprise LTS after its 2026-04-30 EOL.
5. Runtime metadata tests pass for every active LTS lane.
6. Supplementary process-release-shape failure inventory has no active failure
   for supported LTS lanes.
7. Deno patch closure and upstream-policy verifiers pass and print the resolved
   fork tags/SHAs plus the carried-patch disposition contract.
8. Public Node support docs are generated from evidence and do not contain stale
   unsupported pass-rate prose.
9. Production in-process Node profile does not include generic loopback,
   wildcard listen, worker, inspector, run, FFI, or ambient TLS-disable env
   grants, and the `nimbus-tenant` plus `nimbus-bridge` admission tests prove
   unsafe policies fail closed.
10. Harness timeout/hang diagnostics exist for worker, MessagePort, VM, and
    subprocess fixture families.
11. Active LTS package canaries have lane-local results.
12. `cargo fmt --all --check`, the NLRT verifier, and the focused runtime Node
    metadata tests pass.

## Verification Commands

Expected final verification:

```bash
cargo fmt --all --check
bash scripts/verify-node-lts-runtime-trust.sh
cargo test -p nimbus-runtime node_compat_supplementary_process_shape -- --nocapture --test-threads=1
cargo test -p nimbus-tenant production_untrusted_runtime_admission -- --nocapture
cargo test -p nimbus-bridge runtime_execution_admission -- --nocapture
cargo test -p nimbus-convex runtime_access -- --nocapture
```

When NLRT touches generated evidence, also run the documented Node evidence
refresh path for the affected lanes before closeout.

## Execution Log

| Date | NLRT | Status | Files touched | Verification | Notes |
| --- | --- | --- | --- | --- | --- |
| 2026-05-28 | NLRT0 | done | `docs/plans/node-lts-runtime-trust-plan.md`, `docs/plans/proof/node-lts-runtime-trust/README.md`, `docs/plans/proof/node-lts-runtime-trust/nlrt0-baseline-and-control-plane.md` | `npm run docs:validate-refs:strict`: pass, 218 working-tree Markdown files | Baseline commit `9995f65d`; plan activated after the crate-extraction refactor; NCG overlap remains a named coordination hazard. |
| 2026-05-28 | NLRT1 | done | `scripts/verify-deno-fork-provenance.sh`, `docs/architecture/runtime/deno-fork-provenance-allowlist.tsv`, `docs/plans/proof/node-lts-runtime-trust/nlrt1-deno-fork-provenance.md` | `bash scripts/verify-deno-fork-provenance.sh`: 5 passed, 0 failed; `npm run docs:validate-refs:strict`: pass, 218 working-tree Markdown files | Verifier classifies 55 runtime Deno-family crates: 40 on expected Nimbus fork revisions and 15 crates.io exceptions with reasons. |
| 2026-05-28 | NLRT2 | done | `docs/architecture/runtime/node-lts-compat/node-lts-lanes.json`, `docs/architecture/runtime/node-lts-compat/node-lts-lanes.md`, `docs/architecture/runtime/node-compat-surface-matrix.md`, `tests/runtime/node/schemas/node-lts-lanes.schema.json`, `scripts/runtime/node/lane_registry.py`, `scripts/verify-node-lts-lanes.sh`, `docs/plans/proof/node-lts-runtime-trust/nlrt2-node-lts-lane-registry.md` | `bash scripts/verify-node-lts-lanes.sh`: pass, 4 lanes with product default `node22`; `npm run docs:validate-refs:strict`: pass, 219 working-tree Markdown files | Registry marks Node20 `eol_legacy`, Node22 `maintenance_lts`, Node24 `active_lts`, and Node26 `preview_current`; fixture tags are cross-checked against current node-compat lane manifests. |
| 2026-05-28 | NLRT3 | done | `crates/nimbus-runtime/src/limits/axes.rs`, `crates/nimbus-runtime/src/limits/tests.rs`, `crates/nimbus-runtime/src/limits.rs`, `crates/nimbus-runtime/src/lib.rs`, `crates/nimbus-tenant/src/operator_policy.rs`, `crates/nimbus-tenant/src/operator_policy/tests.rs`, `crates/nimbus-convex/src/manifest.rs`, `crates/nimbus-convex/src/registry/resolution/runtime_access.rs`, `docs/plans/proof/node-lts-runtime-trust/nlrt3-runtime-target-metadata.md` | `cargo test -p nimbus-runtime node_lts -- --nocapture`: 3 passed; `cargo test -p nimbus-tenant node_runtime_profiles_follow_lts_registry_targets -- --nocapture`: 1 passed; `cargo test -p nimbus-convex convex_node_runtime_lanes_follow_lts_registry_targets -- --nocapture`: 1 passed; `cargo fmt --all --check`: pass; `npm run docs:validate-refs:strict`: pass, 219 working-tree Markdown files | `RuntimeCompatibilityTarget` remains the public selector but Node metadata now comes from the lane registry; Node26 remains preview-only and unparseable as a runtime target. |
| 2026-05-28 | NLRT4 | done | `docs/architecture/runtime/node-lts-compat/node-lts-lanes.json`, `tests/runtime/node/schemas/node-lts-lanes.schema.json`, `scripts/runtime/node/lane_registry.py`, `crates/nimbus-runtime/src/limits/axes.rs`, `crates/nimbus-runtime/src/runtime/bootstrap/ops/shared.rs`, `crates/nimbus-runtime/src/runtime/bootstrap/source.rs`, `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`, `crates/nimbus-runtime/src/runtime/tests/node/cases/watchpoints_extended.rs`, `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/supplementary/process-release-shape.node20.js`, `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/supplementary/process-release-shape.node22.js`, `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/supplementary/process-release-shape.node24.js`, `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/fixtures/process-and-timing-supplementary.json`, `docs/architecture/runtime/node-compat-supplementary.md`, `docs/architecture/runtime/node-compat-supplementary-failures.md`, `docs/plans/proof/node-lts-runtime-trust/nlrt4-truthful-process-metadata.md` | `cargo test -p nimbus-runtime node_compat_supplementary_process_shape -- --nocapture --test-threads=1`: 3 passed; `cargo test -p nimbus-runtime node22_target_exposes_minimal_node_globals -- --nocapture`: 1 passed; `cargo test -p nimbus-runtime node_lts -- --nocapture`: 3 passed; `cargo test -p nimbus-runtime manifest_topology -- --nocapture`: 17 passed; `bash scripts/verify-node-lts-lanes.sh`: pass; `cargo fmt --all --check`: pass; `npm run docs:validate-refs:strict`: pass, 219 files; `git diff --check`: pass | Runtime process metadata now comes from the lane registry, including exact Node version, release, LTS codename, and ABI/module version; node-compat seeded fixture execution now selects the requested lane instead of always using Node22. |
| 2026-05-28 | NLRT5 | done | `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/node20.json`, `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/schema.json`, `crates/nimbus-runtime/src/runtime/tests/node/manifest_catalog.rs`, `crates/nimbus-runtime/src/runtime/tests/node/manifest_metadata.rs`, `crates/nimbus-runtime/src/runtime/tests/node/manifest_report.rs`, `crates/nimbus-runtime/src/runtime/tests/node/manifest_report_tests.rs`, `crates/nimbus-runtime/src/runtime/tests/node/oracle.rs`, `docs/runtimes/nodejs/README.md`, `docs/runtimes/nodejs/compatibility.md`, `docs/runtimes/nodejs/configuration.md`, `docs/runtimes/nodejs/evidence/latest.md`, `docs/runtimes/nodejs/evidence/node20.md`, `docs/runtimes/nodejs/evidence/node22.md`, `docs/runtimes/nodejs/evidence/node24.md`, `docs/architecture/runtime/deno-vs-nimbus-node-compat.md`, `scripts/runtime/node/publish_docs.py`, `scripts/runtime/node/docs_guard.py`, `scripts/verify-node-lts-docs.sh`, `docs/plans/proof/node-lts-runtime-trust/nlrt5-equal-lane-evidence-docs.md` | `bash scripts/verify-node-lts-docs.sh`: pass; `cargo test -p nimbus-runtime manifest_metadata -- --nocapture`: 3 passed; `cargo test -p nimbus-runtime manifest_report -- --nocapture`: 11 passed, 1 ignored; `bash scripts/verify-node-lts-lanes.sh`: pass; `cargo fmt --all --check`: pass; `npm run docs:validate-refs:strict`: pass, 219 files; `git diff --check`: pass | Public docs and generated public evidence now distinguish product default, supported LTS, and Node20 legacy-grace roles; stale hand-written pass-rate and Node22-priority prose is guarded by `scripts/verify-node-lts-docs.sh`. |
| 2026-05-28 | NLRT6 | done | `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/node20.json`, `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/node22.json`, `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/node24.json`, `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/schema.json`, `crates/nimbus-runtime/src/runtime/tests/node/manifest_catalog.rs`, `crates/nimbus-runtime/src/runtime/tests/node/manifest_metadata.rs`, `scripts/runtime/node/fixture_provenance.py`, `scripts/runtime/node/refresh.py`, `scripts/runtime/node/sync.py`, `scripts/verify-node-fixture-provenance.sh`, `tests/runtime/node/schemas/fixture-sync-report.schema.json`, `docs/plans/proof/node-lts-runtime-trust/nlrt6-fixture-provenance-sync.md` | `python3 scripts/runtime/node/fixture_provenance.py validate`: pass, 3 vendored corpora and 2 supported LTS lanes with zero unclassified published results; `python3 scripts/runtime/node/sync.py --lane node22 --dry-run --output-root target/node-compat/nlrt6-sync-dry-run`: pass, 1283 local test files; `python3 scripts/runtime/node/publish_docs.py --check`: pass; negative unproven tag override and synthetic unclassified-status probes failed as expected; `cargo test -p nimbus-runtime manifest_metadata -- --nocapture`: 3 passed; `bash scripts/verify-node-lts-lanes.sh`: pass; `bash scripts/verify-node-lts-docs.sh`: pass; `cargo fmt --all --check`: pass; `npm run docs:validate-refs:strict`: pass, 219 files; `git diff --check`: pass | Vendored Node fixture corpora now carry tag, commit, tag object, sync date, and selection command provenance; refresh gates provenance before sync and after evidence publication. |
| 2026-05-28 | NLRT7 | done | `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`, `scripts/runtime/node/report.sh`, `scripts/verify-node-compat-harness-hardening.sh`, `docs/architecture/runtime/node-lts-compat/harness-timeouts-and-hangs.md`, `docs/architecture/runtime/node-lts-compat/node-lts-compat-summary.md`, `tests/runtime/node/expectations/rust-watchpoints.json`, `tests/runtime/node/classifications/node20.json`, `tests/runtime/node/classifications/node22.json`, `tests/runtime/node/classifications/node24.json`, `docs/architecture/runtime/node-compat-evidence/latest/*`, `docs/runtimes/nodejs/evidence/*.md`, `docs/plans/proof/node-lts-runtime-trust/nlrt7-harness-timeouts-and-hangs.md` | `bash scripts/verify-node-compat-harness-hardening.sh`: pass, 3 harness tests plus watchpoint/classification/status checks; full evidence rebuild produced 8 slice reports, 2 canary bundles, 12 canary checks, 1 oracle report, and 3 inventories; `python3 scripts/runtime/node/fixture_provenance.py validate`: pass; `python3 scripts/runtime/node/publish_docs.py --check`: pass; `bash scripts/verify-node-lts-docs.sh`: pass; `bash scripts/verify-node-fixture-provenance.sh`: pass; `cargo fmt --all --check`: pass; `npm run docs:validate-refs:strict`: pass; `git diff --check`: pass | Harness failures now produce family-classified diagnostics under `target/node-compat/diagnostics`; ignored Rust tests are explicit watchpoints, moving the catalog from 61 to 67 entries; MessagePort worker promotion remains blocked on NLRT8 production profile proof. |
| 2026-05-28 | NLRT8 | done | `crates/nimbus-runtime/src/limits/grants.rs`, `crates/nimbus-runtime/src/limits/resources.rs`, `crates/nimbus-runtime/src/limits/tests.rs`, `crates/nimbus-runtime/src/runtime_capabilities.rs`, `crates/nimbus-runtime/src/runtime/tests/basic_invocation/node_capabilities.rs`, `crates/nimbus-runtime/src/runtime/tests/basic_invocation/support.rs`, `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`, `crates/nimbus-tenant/src/runtime_admission.rs`, `crates/nimbus-tenant/src/tests.rs`, `crates/nimbus-tenant/src/operator_policy.rs`, `crates/nimbus-tenant/src/operator_policy/evaluation.rs`, `crates/nimbus-tenant/src/operator_policy/validation.rs`, `crates/nimbus-tenant/src/operator_policy/tests.rs`, `crates/nimbus-convex/src/registry/resolution/runtime_access.rs`, `docs/architecture/runtime/permission-model.md`, `docs/plans/proof/node-lts-runtime-trust/nlrt8-permission-profile-split.md` | `cargo test -p nimbus-runtime node_permission_profiles -- --nocapture`: 1 passed; `cargo test -p nimbus-runtime node_capabilities -- --nocapture`: 7 passed; `cargo test -p nimbus-runtime package_resolution -- --nocapture`: 6 passed, 3 ignored; `cargo test -p nimbus-tenant production_untrusted_runtime_admission -- --nocapture`: 8 passed; `cargo test -p nimbus-tenant node_profile -- --nocapture`: 4 passed; `cargo test -p nimbus-bridge runtime_execution_admission -- --nocapture`: 2 passed; `cargo test -p nimbus-convex runtime_access -- --nocapture`: 2 passed; `cargo fmt --all --check`: pass; `npm run docs:validate-refs:strict`: pass; `git diff --check`: pass | `application_node*()` is now the safe production in-process profile; local-dev and service/microVM Node profiles are explicit constructors; tenant admission still rejects unsafe custom Node grants and bridge execution still fails closed when fallback routing is unavailable. |
| 2026-05-28 | NLRT9 | done | `docs/operating/deno-fork-workflow.md`, `docs/architecture/runtime/deno-fork-bump-ledger.md`, `docs/architecture/runtime/deno-vs-nimbus-node-compat.md`, `docs/README.md`, `scripts/verify-deno-fork-upstream-policy.sh`, `docs/plans/proof/node-lts-runtime-trust/nlrt9-deno-fork-upstream-policy.md` | `bash scripts/verify-deno-fork-upstream-policy.sh`: 27 passed, 0 failed; `bash scripts/verify-deno-fork-provenance.sh`: 5 passed, 0 failed, 40 forked and 15 allowlisted runtime Deno-family crates | Fork workflow now requires classify, unpin to canonical local fork, prove, commit/tag/push, repin, verify, and record release proof; current Deno and `rusty_v8` carried patches have upstream/Nimbus-only/temporary dispositions and removal/upstream triggers. |
| 2026-05-28 | NLRT10 | done | `crates/nimbus-runtime/src/limits/resources.rs`, `crates/nimbus-runtime/src/limits/tests.rs`, `crates/nimbus-runtime/src/runtime/tests/basic_invocation/package_resolution.rs`, `crates/nimbus-runtime/src/runtime/tests/basic_invocation/support.rs`, `crates/nimbus-convex/src/registry/resolution/runtime_access.rs`, `tests/runtime/node/canary-registry.json`, `tests/runtime/node/networking-canaries/bundles/platform.mjs`, `scripts/runtime/node/canary_registry.py`, `scripts/runtime/node/dashboard.py`, `scripts/verify-node-lts-canaries-and-oracles.sh`, `docs/architecture/runtime/node-compat-surface-matrix.md`, `docs/architecture/runtime/node-compat-evidence/latest/*`, `docs/runtimes/nodejs/evidence/*.md`, `docs/plans/proof/node-lts-runtime-trust/nlrt10-active-lts-canaries-and-oracles.md` | `make node-compat-canaries PRESET=application`: pass, 16 canary checks passed, 0 failed; `make node-compat-canaries PRESET=tooling`: pass, 10 checks passed, 0 failed; `make node-compat-oracle LANE=node24 SAMPLE=test/parallel/test-buffer-alloc.js NODE_BIN=/Users/jack/.local/share/mise/installs/node/24.16.0/bin/node`: pass, 1 oracle test; `bash scripts/verify-node-lts-canaries-and-oracles.sh`: 12 passed, 0 failed; `bash scripts/runtime/node/validate-claims.sh`: 12 claims; `cargo test -p nimbus-runtime package_resolution -- --nocapture`: 6 passed, 5 ignored; `cargo test -p nimbus-convex runtime_access -- --nocapture`: 2 passed, 2 ignored; `bash scripts/verify-node-lts-docs.sh`: pass; `python3 scripts/runtime/node/publish_docs.py --check`: pass; `cargo fmt --all --check`: pass; `npm run docs:validate-refs:strict`: pass; `git diff --check`: pass | Active public canary claims are scoped to Node22 and Node24; Node20 remains only legacy-grace extra canary coverage; published dashboard now has 12 canary claims, 26 canary checks, 2 oracle reports, and zero required canary gaps. |
| 2026-05-28 | NLRT11 | done | `scripts/verify-node-lts-runtime-trust.sh`, `docs/plans/archive/node-lts-runtime-trust-plan.md`, `docs/plans/README.md`, `docs/plans/proof/node-lts-runtime-trust/README.md`, `docs/plans/proof/node-lts-runtime-trust/nlrt11-closeout.md` | `bash scripts/verify-node-lts-runtime-trust.sh`: pass, 16 checks; focused final tests pass as listed in `nlrt11-closeout.md` | Plan archived with all ledger rows done; final verifier composes lane, fork, docs, fixture provenance, harness hardening, canary/oracle, metadata, permission, and admission gates. |

## Risk Register

| Risk | Mitigation |
| --- | --- |
| Active NCG work and NLRT both touch Node bootstrap. | NLRT starts from registry/provenance/docs and defers bootstrap edits until NCG is either closed or explicitly coordinated. |
| Exact Node patch metadata is mistaken for actual embedded native component parity. | Expose Node API contract metadata separately from actual V8/Deno/Nimbus diagnostic metadata. |
| Node26 reaches LTS before the registry exists. | Treat Node26 as preview-current now and make promotion a registry data change plus evidence gate, not an enum/code archaeology task. |
| Production admission remains correct but constructors stay confusing. | Split constructors/profiles so the first selected type communicates local-dev versus production intent. |
| Extracted crates let one owner pass tests while another consumer drifts. | Completion criteria and focused verification cover `nimbus-runtime`, `nimbus-tenant`, `nimbus-bridge`, and `nimbus-convex` explicitly. |
| Deno fork carries grow silently. | Verifier requires tag/SHA and patch disposition; release proof must cite the fork changelog. |

## Readiness Audit

Audited on 2026-05-27.

Findings:

- Ready as an execution control plane after this audit: the plan has a resume
  protocol, state rules, proof artifact contract, per-phase acceptance
  criteria, completion gate, verification commands, and execution log.
- The plan intentionally remains `ready for execution, not yet started` until
  NLRT0 is accepted and marked `in_progress`.
- No requirement is left without a measurable artifact or verifier condition.
- The only live coordination hazard is overlap with
  `docs/plans/archive/node-compat-cron-greening-plan.md`; the resume protocol and risk
  register now make that coordination explicit.

Re-audited on 2026-05-28 after the maintainability/refactor extraction.

Findings:

- The plan still needs to run; the refactor does not change the Node LTS, Deno
  fork, permission, or evidence objectives.
- The plan did need updates so an executing agent does not implement against an
  old `nimbus-server`-centric ownership model. Runtime metadata belongs in
  `nimbus-runtime`; tenant policy in `nimbus-tenant`; runtime invocation
  admission in `nimbus-bridge`; Convex lane selection and `"use node"` packaging
  in `nimbus-convex`.
- Acceptance criteria and final verification now include the extracted owners,
  so completion cannot be claimed by updating only one crate.

Closeout audit on 2026-05-28:

- NLRT0 through NLRT11 are complete and proof-backed under
  `docs/plans/proof/node-lts-runtime-trust/`.
- The final control plane is
  `scripts/verify-node-lts-runtime-trust.sh`, which composes the focused
  lane-registry, Deno fork provenance, Deno upstream-policy, fixture
  provenance, harness hardening, canary/oracle, generated-docs, metadata,
  permission-profile, tenant-admission, bridge-admission, Convex lane, format,
  and markdown-reference gates.
- Public Node.js runtime support prose is subordinate to generated evidence in
  `docs/runtimes/nodejs/evidence/latest.md`; Node22 remains a product default,
  not an evidence priority, and Node20 remains legacy-grace after its
  2026-04-30 EOL.

## References

- `docs/plans/research/node-lts-runtime-and-deno-fork-strategy.md`
- `docs/plans/archive/node-compat-cron-greening-plan.md`
- `docs/runtimes/nodejs/compatibility.md`
- `docs/runtimes/nodejs/evidence/refreshing.md`
- `docs/architecture/runtime/permission-model.md`
- `docs/architecture/runtime/node-compat-supplementary-failures.md`
- `docs/architecture/runtime/node-compat-surface-matrix.md`
- `crates/nimbus-runtime/src/`
- `crates/nimbus-tenant/src/runtime_admission.rs`
- `crates/nimbus-tenant/src/operator_policy.rs`
- `crates/nimbus-bridge/src/admission.rs`
- `crates/nimbus-convex/src/registry/resolution/runtime_access.rs`
