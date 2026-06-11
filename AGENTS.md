<!-- convex-ai-start -->
This project implements a [Convex](https://convex.dev)-compatible backend server.

When working on Convex-compatible code (`packages/convex/`, `demos/convex/`, or any Convex API surface), **always read `docs/private/staging/adapters/convex/ai-guidelines.md` first** for important guidelines on how to correctly use Convex APIs and patterns. The file contains rules that override what you may have learned about Convex from training data.
<!-- convex-ai-end -->

# Nimbus

The role of this file is to capture common mistakes and recurring confusion points for agents working in this repo.

If you hit a surprise that is likely to trip up another agent, tell the developer. Ask before adding a brief principle-first note here. If the guidance needs more than a few bullets, it probably belongs in `docs/*.md` or beside the code instead of here.

## Keep This File Small

- Put durable repo-wide rules, repeated traps, and verification commands here.
- Add new entries only with developer approval.
- Prefer principle-first notes over historical bug writeups.
- Link to canonical docs for architecture details instead of copying them here.
- Do not use this file as a changelog, ownership map, or deep implementation manual.

## Pre-Launch Status

**This project has NOT launched yet.** There are no production users or data to migrate.

- **Breaking changes are preferred.** Choose clean replacements over compatibility layers.
- **No backwards compatibility code.** Delete old behavior instead of deprecating it.
- **No migration shims.** Change the schema or API directly.
- **No feature flags for legacy behavior.** Remove the old path entirely.

If you find yourself writing compatibility code, stop and make the breaking change instead.

## Working Set

- Start with `README.md`, `ARCHITECTURE.md`, `docs/README.md`, and
  `docs/private/plans/README.md`.
- Use the active plan owner for the slice you are touching. Prefer active
  plans over archived history.
- Treat the current git worktree plus the owning active plan as progress
  state. Resume `in_progress` work before starting a new roadmap item.
- Checkpoint plan state before stopping, handing off, or any likely context
  loss.
- Load one roadmap item at a time plus only the immediately relevant code,
  tests, and docs.

### Routing By Work Type

- Public docs site (nimbusdocs.com), the five public `docs/` groups,
  `website/`, llms.txt artifacts, or README/repo front-door messaging:
  `.agents/skills/docs/SKILL.md` (IA, Diátaxis rules, `docs/private/`
  fence, messaging canon, verification gates) and
  `docs/private/plans/nimbus-docs-site-plan.md` (the `nimbus-docs-site`
  plan), gated on `bash scripts/verify-nimbus-docs-site.sh` and
  `bash scripts/check-docs.sh`.
- Generic maintainability, refactor, modularity, reliability hardening, or
  canonical naming:
  `docs/private/architecture/testing/reliability-posture.md`,
  `docs/private/architecture/testing/ci-failure-investigation.md`,
  `docs/private/plans/archive/architecture-seam-cleanliness-plan.md`,
  `docs/private/plans/archive/deployment-auth-runtime-boundary-plan.md`,
  `docs/private/plans/archive/repo-architecture-and-seam-hardening-plan.md`
- Adapter/runtime/auth/trust cleanup:
  `docs/private/architecture/server/adapter-expectations.md`,
  `docs/private/architecture/runtime/adapter-boundary.md`,
  `docs/private/architecture/server/auth-runtime-trust.md`,
  `docs/private/plans/archive/deployment-auth-runtime-boundary-plan.md`
  Use the completed baselines in `docs/private/plans/archive/server-runtime-canonicalization-plan.md`,
  `docs/private/plans/archive/adapter-runtime-trust-hardening-plan.md`,
  `docs/private/plans/archive/runtime-capability-adapter-boundary-plan.md`, and
  `docs/private/plans/archive/multi-adapter-boundary-hardening-plan.md` only as prior
  wave references.
- Runtime capability segregation, exact service grants, adapter context service
  shortcut removal, private Nimbus-managed isolate host-transport gating,
  Bun/JSC service-capability fail-closed parity, principal-class service route
  policy, engine `Service` -> `Engine` naming, or JS SDK authority boundaries:
  `docs/private/architecture/server/auth-runtime-trust.md`,
  `docs/private/architecture/runtime/adapter-boundary.md`,
  `docs/private/architecture/sandbox/service-sandbox-session-model.md`,
  `docs/private/plans/nimbus-capability-segregation-plan.md`, gated on
  `bash scripts/verify-nimbus-capability-segregation.sh` once CB0 creates it.
- Cross-cutting multi-backend / multi-adapter hardening (storage trait
  segregation, adapter/backend registration seam decision,
  `RuntimeHooks` for backend-coupled workers, dual-target tests per
  adapter, auth-caching ADR, per-backend SQL-safety ADRs, per-segment
  latency budgets, trait object-safety audit, stable logical table identity +
  backend-owned physical-layout decision, typed-column key storage,
  read-consistency routing, hybrid event-capture pattern, cross-cutting
  `docs/private/technical-debt.md`):
  `docs/private/plans/archive/multi-backend-adapter-hardening-plan.md` (MBA0..MBA14,
  completed baseline, closed 2026-05-27). Inspiration source is ExtendDB
  (Apache-2.0 DynamoDB adapter on PostgreSQL) at
  `~/src/github.com/ExtendDB/extenddb`. Independent
  of `docs/private/plans/archive/dynamodb-adapter-plan.md`; applies across every
  existing backend and adapter, not only the DynamoDB lane. `/goal`
  control plane gated on
  `bash scripts/verify-multi-backend-adapter-hardening.sh` (fifteen
  conditions). Use `docs/private/staging/operating/multi-backend-adapter-hardening.md`
  for the current contract.
- Convex-informed storage trust gaps (table lifecycle after stable
  `table_catalog`, table-aware Convex document identity validation,
  `TableId`-based dependency tracking, stable index identity/lifecycle,
  history/repeatable-read posture, table identity diagnostics, and
  cross-backend conformance after comparing against Convex internals):
  `docs/private/plans/archive/convex-storage-trust-hardening-plan.md` is the
  completed baseline, closed 2026-05-27. Use local Convex source at
  `~/src/github.com/get-convex/convex-backend` and the baseline proof at
  `docs/private/plans/proof/convex-storage-trust-hardening/cst0-convex-storage-comparison.md`
  for historical context. Promote a new active plan before another
  Convex-informed storage trust wave.
- Sandbox, machine lifecycle, or CLI UX:
  `docs/private/architecture/sandbox/service-sandbox-session-model.md`,
  `docs/private/architecture/sandbox/microvm-service-baseline.md`,
  `docs/private/architecture/sandbox/macos-machine-flow.md` when relevant,
  `docs/private/staging/operating/cli.md`, and the active platform plan from
  `docs/private/plans/README.md`
- SDK services/sandboxes/sessions resource model, built-in/external/sandbox-backed
  service implementations, dynamic services, sandbox APIs, runtime-isolate
  non-resource semantics, future `profile: "isolate"` sandbox semantics, or
  session target semantics:
  `docs/private/architecture/sandbox/service-sandbox-session-model.md` and
  `docs/private/plans/nimbus-sdk-resource-model-plan.md`
- Sandbox backend / snapshot / desktop / GPU (unified-lift roadmap on
  `nimbus-libkrun`): `docs/private/plans/nimbus-sandbox-plan.md` is the single
  active execution plan. Bands route as **B** (backend / capability
  profiles / nimbus-guest), **S** (Linux-KVM snapshot + fork, S0..S5),
  **D** (desktop profile / computer use, D1..D10), **G** (GPU profile,
  G1..G13). Decision baseline D1-D12 in
  `docs/private/plans/research/vmm-landscape-2026.md`. Subject research:
  `docs/private/plans/research/libkrun-session-sandbox.md` (backend),
  `docs/private/plans/research/gpu-sandbox-backends.md` (GPU mediation),
  `docs/private/plans/research/computer-use-capabilities-audit.md` (desktop
  capability gaps), `docs/private/plans/research/nimbus-libkrun-fork-inventory.md`
  (fork delta + muvm lift map),
  `docs/private/plans/research/macos-host-vs-guest-control-plane-rationale.md`
  (per-host topology), `docs/private/architecture/sandbox/macos-machine-flow.md`
  (macOS outer-VM flow). Archived predecessors:
  `docs/private/plans/archive/computer-use-sandbox-plan.md` (→ Band D),
  `docs/private/plans/archive/gpu-accelerated-sandbox-plan.md` (→ Band G),
  `docs/private/plans/archive/nimbus-libkrun-snapshot-port-plan.md` (→ Band S),
  `docs/private/plans/archive/firecracker-snapshot-invocation-backend-plan.md`
  (Firecracker-as-separate-VMM dropped per D2). Macros: Linux production
  runs direct libkrun-on-KVM microVMs per service; macOS dev runs ONE
  outer machine-os Linux VM and per-workload sandboxes inside it are
  standard Linux containers (crun), NOT nested microVMs (D11/D12).
  Snapshot/fork S0..S5 is Linux-KVM-only by construction.
- Node-side systemd D-Bus binding (`SystemdDbusClient` /
  `SystemdTransientUnitBackend` / `NodeWorkloadReconciler`):
  `docs/private/plans/archive/node-dbus-client-binding-plan.md` is the
  completed baseline (NDB0..NDB7, closed 2026-05-29 via PR #3). Lifted
  the TSB7 deferral recorded in
  `docs/private/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb7-systemd-transient.md`
  by attaching `lucab/zbus_systemd` (pin `=0.26000.0`, features
  `systemd1` + `zbus-async-tokio`) and direct `zbus` to the existing
  trait at `crates/nimbus-node/src/systemd_transient.rs:15-32`.
  Decision rationale and option matrix in
  `docs/private/plans/research/systemd-dbus-binding-rust-2026.md`.
  Signal-correlated job completion (call systemd `Manager.Subscribe`,
  establish the `JobRemoved` stream *before* calling
  `StartTransientUnit`/`StopUnit`, complete only on matching signal
  `result`) is the trust-critical NDB3
  deliverable, not polling. Linux-gated integration tests against
  `systemctl --user` land in NDB5; CI lane `node-dbus-integration`
  on `ubuntu-24.04` lands in NDB6; default activation (Linux
  builds default to `ZbusSystemdClient` instead of
  `UnavailableSystemdDbusClient`) lands in NDB7. `/goal` control
  plane gated on `bash scripts/verify-node-dbus-binding.sh` (10
  conditions). Plan does NOT wire a production caller for
  `NodeWorkloadReconciler` — TSB14's deferral of that work
  (`docs/private/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb14-node-extraction-decision.md:28-37`)
  needs its own follow-up plan.
- CLI daemon canonicalization, walk-up boundaries, or banner shape:
  `docs/private/plans/archive/cli-daemon-canonicalization-plan.md` (completed
  baseline, closed 2026-05-19), `docs/private/staging/operating/cli.md`,
  `docs/private/plans/archive/cli-command-surface-plan.md` (prior wave),
  `docs/private/plans/archive/compose-discovery-plan.md` (compose precedent).
  Promote a new active plan before another CLI-canonicalization wave.
- Localhost/server security:
  `docs/private/plans/archive/localhost-server-security-plan.md`
- Install script work:
  `docs/private/plans/archive/install-script-plan.md` as the completed baseline and
  `docs/private/plans/distribution-plan.md` as parent context; promote a new active
  plan before another install-script wave
- Local-dev / build-contract / Make-vs-Cargo orchestration:
  `docs/private/staging/operating/local-dev.md` for the user-facing contract, then
  `docs/private/plans/archive/local-dev-canonicalization-plan.md` as the
  completed baseline (LD0-LD7, closed 2026-05-21). The Makefile UI
  dependency graph at the top of `Makefile` (`UI_PKG`, `UI_DIST_INDEX`,
  etc.) is the source of truth for cross-toolchain prerequisites;
  `crates/nimbus-server/build.rs` only asserts that those inputs exist
  and errors actionably otherwise. Promote a new active plan before
  another local-dev / build-graph wave.
- CI caching / sccache / Swatinem orchestration:
  `docs/private/staging/operating/ci-caching.md` for the canonical caching contract,
  then `docs/private/plans/archive/ci-caching-canonicalization-plan.md` as the
  completed baseline (CC0-CC9, closed 2026-05-22). The baseline
  covers sccache rollout across every Rust job in
  `.github/workflows/*.yml`, Swatinem `shared-key` rotation v1→v2,
  `save-if: refs/heads/main` for PR-cannot-poison-main saves,
  the `ui-artifacts` leader job that deduplicates the UI build for
  harness + coverage, the `warm-sccache` leader job that converts
  parallel-cold-start to serial-cold-then-parallel-warm, and the
  CC9 pin-floor + save-always retraction sweep that fixed the GHA
  cache v1 → v2 migration breakage on `mozilla-actions/sccache-action
  @v0.0.6`. Promote a new active plan before another CI caching /
  sccache / Swatinem wave.
- CI infrastructure modernization (composite actions, SHA pinning,
  runner determinism, job summaries, SAST):
  `docs/private/staging/operating/ci-modernization.md` for the canonical contract,
  then `docs/private/plans/archive/ci-modernization-plan.md` as the completed
  baseline (CM0..CM8, closed 2026-05-22). The baseline covers the
  cross-workflow Rust + sccache + Swatinem composite action
  extraction at `.github/actions/setup-rust-cached/`, SHA-pinning
  every non-`actions/*` `uses:` reference (including codeql-action
  sub-paths) with a `# vX.Y.Z` version-name comment, pinning
  `runs-on: ubuntu-latest` → `ubuntu-24.04`, dropping
  `create-github-app-token@v3.2.0` to `@v3`, emitting
  `$GITHUB_STEP_SUMMARY` markdown from deny / coverage /
  rust-gate-summary / desktop-ui, and shipping
  `.github/workflows/codeql.yml`. `/goal` control plane gated on
  `bash scripts/verify-ci-modernization.sh` (12 conditions). Promote
  a new active plan before another CI infrastructure / SAST /
  composite-action wave.
- Coverage / release-pipeline acceleration (mold linker, coverage
  parallelism + sharding, release.yml composite adoption, Windows
  release-build investigation): `docs/private/staging/operating/ci-modernization.md`
  for the canonical contract (see "Coverage and release acceleration"
  section), then `docs/private/plans/archive/coverage-acceleration-plan.md`
  as the completed baseline (CA0..CA5, closed 2026-05-22). The
  baseline installs `mold` in the `setup-rust-cached` composite via
  `CARGO_TARGET_*_RUSTFLAGS=-C link-arg=-fuse-ld=mold` (NOT
  `LINKER=mold` — that invocation fails with `mold: fatal: unknown
  -m argument: 64`), flips Coverage from `-j 1` to `-j 4`, shards
  Coverage into 3 lanes (`server` / `engine` / `rest`) that fan into
  a `cargo llvm-cov report --lcov` reducer with profraw artifact
  transfer through `target/llvm-cov-target/profraw/`, migrates the
  5 inline `dtolnay/rust-toolchain` + `Swatinem/rust-cache` sites in
  `release.yml` into the composite with `save-cache: always` on
  release tags, and identifies the Windows release pole (vendored
  OpenSSL, V8 link, cold-target build) as deferred scope for a
  follow-on release-acceleration plan. `/goal` control plane gated
  on `bash scripts/verify-coverage-acceleration.sh` (10 conditions).
  Promote a new active plan before another coverage / release
  acceleration wave.
- PR CI wall acceleration (verification-harness sharding, workspace-
  tests sharding via cargo-nextest --partition, external-provider
  matrix split by provider, warm-sccache shrink):
  `docs/private/staging/operating/ci-modernization.md` for the canonical contract
  ("PR critical-path acceleration" section), then
  `docs/private/plans/archive/ci-wall-acceleration-plan.md` as the completed
  baseline (CW0..CW5, closed 2026-05-23). The baseline attacks the
  post-CA wall poles on `main` (23.6m on `32951ee7`): Server
  Verification Harness (12.7m → ~3.5m via 4-shard server harness),
  Rust Workspace Tests (15.7m → ~6m via 3-way nextest `--partition
  hash:N/M`), External Provider Integration Tests (14.6m → ~7m via
  per-provider matrix), warm-sccache itself (10.2m, `--tests`
  dropped). CW1 introduces `NIMBUS_HARNESS_SHARD=N/M` as the
  in-test corpus filter env-var. CW2 introduces
  `NIMBUS_NEXTEST_PARTITION=N/M` as the Makefile forwarding shape.
  CW3 introduces `NIMBUS_PROVIDER_FILTER=postgres|mysql|libsql` as
  the per-provider test-script filter. CW4 retires `--tests` from
  warm-sccache and documents the deferred per-target cache lane
  (Swatinem v2 already caches `target/`). `/goal` control plane
  gated on `bash scripts/verify-ci-wall-acceleration.sh` (10
  conditions). Promote a new active plan before another PR-wall
  acceleration wave.
- CI PR-wall sub-15 (post-CW pole attack — libsql image pin +
  docker-image cache, coverage extraction to its own workflow,
  branch-conditional concurrency cap, warm-sccache retain-or-retire
  decision): `docs/private/staging/operating/ci-pr-wall.md` for the canonical
  contract, then `docs/private/plans/archive/ci-pr-wall-sub-15-plan.md` as
  the completed baseline (PW0..PW6, closed 2026-05-23). The
  baseline pins
  `ghcr.io/tursodatabase/libsql-server` to a `vX.Y.Z` tag chosen
  by probing GHCR directly (the upstream GitHub release list can
  contain tags that 404 on GHCR), wraps every libsql usage with a
  three-step `actions/cache@v5` lane keyed on
  `libsql-image-vX.Y.Z`, extracts the Coverage shards + reducer
  into `.github/workflows/coverage.yml` on
  `push.main + schedule + workflow_dispatch` (Coverage is not on
  `rust-gate-summary.needs:`, so it never gated merge),
  flips ci.yml's `cancel-in-progress: true` to
  `${{ github.ref != 'refs/heads/main' }}` so cancelled main runs
  no longer abandon Swatinem / sccache / libsql-image cache saves
  mid-flight, and takes the PW4c warm-sccache-retained path with
  a measurement bundle (libsql shard hits Swatinem 0% on CW5
  while harness lanes hit 76%+, so retiring warm-sccache would
  expose the consistently-cold lanes to full cold-compile cost on
  every PR). `/goal` control plane gated on
  `bash scripts/verify-ci-pr-wall-sub-15.sh` (10 conditions;
  condition 8 wall threshold flips between 15m (PW4b retired) and
  18m (PW4c retained) based on `warm-sccache:` presence in
  ci.yml). Promote a new active plan before another PR-wall
  acceleration wave.
- Firebase/Firestore compatibility:
  `docs/private/staging/adapters/firebase/compatibility.md`,
  `docs/private/staging/adapters/firebase/migration.md`,
  `docs/private/staging/adapters/firebase/auth-contract.md`,
  `docs/private/architecture/runtime/adapter-boundary.md`,
  `docs/private/architecture/server/auth-runtime-trust.md`
- Cloud Functions compatibility:
  `docs/private/staging/adapters/cloud-functions/compatibility.md`,
  `docs/private/staging/adapters/cloud-functions/migration.md`,
  `docs/private/architecture/runtime/adapter-boundary.md`,
  `docs/private/architecture/server/auth-runtime-trust.md`
- Convex or Nimbus CLI/codegen workflow:
  `docs/private/staging/adapters/convex/ai-guidelines.md`,
  `docs/private/staging/operating/cli.md`,
  `docs/private/staging/adapters/convex/compatibility.md`,
  `docs/private/plans/archive/nimbus-init-plan.md`
- Node-compatible runtime / `deno_core` / `rusty_v8` / embedded-codegen:
  `docs/private/architecture/runtime/adapter-boundary.md` and
  `docs/private/architecture/server/auth-runtime-trust.md` after the top-level docs.
  Current default-quality work lives in
  `docs/private/plans/node-default-runtime-support-hardening-plan.md` (NDS0..NDS10):
  raise Node24 from bounded FaaS-compatible default to a well-supported default,
  expand Node22/Node24 official fixture and package evidence, give Node26 real
  Current-line fixture evidence, and subsume
  `docs/private/plans/archive/node-compat-cron-greening-plan.md`.
  Use `docs/private/plans/archive/node-compatible-runtime-plan.md`,
  `docs/private/plans/archive/node-lts-compatibility-plan.md`,
  `docs/private/plans/archive/node-compat-test-infrastructure-plan.md`, and
  `docs/private/plans/archive/node-compat-future-lanes-and-correctness-plan.md` as completed
  baselines. If new Node-compat roadmap work is needed beyond those completed
  plans, create or adopt a fresh active plan before starting a new wave.
  `~/src/github.com/nimbus/deno` as the canonical Deno-family fork,
  `~/src/github.com/nimbus/rusty_v8` as the matching V8 fork,
  `~/src/github.com/nimbus/deno` only as historical delta context,
  `~/src/github.com/denoland/deno` for upstream comparison, and
  `~/src/github.com/nodejs/node` for upstream Node source/tests.
  Prefer working and verifying against those canonical worktrees with normal
  sandbox approval when needed. Do not make `/private/tmp` checkout copies or
  alternate Cargo-source workspaces the default workflow.
  For Deno-owner changes, temporarily unpin Nimbus from the published
  `nimbus/deno` tag and point the Deno-family dependencies at the
  canonical `~/src/github.com/nimbus/deno` worktree while proving the
  fix. Do not create shadow checkout copies to mimic the pin.
  Once the fork change is verified, commit/tag/push it in
  `~/src/github.com/nimbus/deno`, then repin `Cargo.toml` and
  `Cargo.lock` back to the published tag/revision and rerun Nimbus
  verification on that repinned baseline before updating the control plane.
  Keep Nimbus-specific bootstrap/profile/capability fixes local. Promote a fix
  to `nimbus/deno` when the local alternative would duplicate Deno/Node
  builtin semantics, shadow internal behavior long-term, or add avoidable
  hot-path overhead. For one-off macOS fork verification that must bypass the
  checked-in `-fuse-ld=lld` target flag, prefer `CARGO_ENCODED_RUSTFLAGS`.
  Use `/private/tmp` Cargo overrides only as short-lived last-resort proof
  paths, never as progress state or the main source of truth.

### Workspace layout

The repo is a Rust workspace + npm monorepo. Names overlap — know which you mean:

| Name | Path | What it is |
| --- | --- | --- |
| `nimbus` (facade crate) | `crates/nimbus/` | Re-exports public types for embedders |
| `nimbus-bin` | `crates/nimbus-bin/` | CLI binary entry point |
| `nimbus-core` | `crates/nimbus-core/` | Shared types and validation (zero I/O) |
| `nimbus-engine` | `crates/nimbus-engine/` | Central coordinator (`Service`) |
| `nimbus-runtime` | `crates/nimbus-runtime/` | V8 execution (zero workspace deps) |
| `nimbus-sandbox` | `crates/nimbus-sandbox/` | Generic sandbox and isolation seam |
| `nimbus-server` | `crates/nimbus-server/` | HTTP/WebSocket transport |
| `nimbus-storage` | `crates/nimbus-storage/` | Persistence layer |
| `nimbus-testing` | `crates/nimbus-testing/` | Shared test fixtures and deterministic harness helpers |
| `nimbus` (JS SDK) | `packages/nimbus/` | Nimbus-native JavaScript SDK |
| `convex` (JS compat) | `packages/convex/` | Convex compatibility package |
| `@nimbus/codegen` | `packages/codegen/` | Code generation tool |

### Rust target layout

- Reserve `examples/` for user-facing example programs.
- Put internal benchmark or evaluation runners under `benches/` with explicit
  custom-harness targets when they are driven through `cargo bench`.
- Keep integration tests in `tests/` and support helpers beside the owning
  crate unless they are shared widely enough to justify `nimbus-testing`.

### Modularity thresholds

- Files under 1,500 lines are usually acceptable when they keep one coherent
  ownership story.
- Files from 1,500 through 1,999 lines need an explicit justification in the
  owning active plan if they remain unsplit.
- Files at 2,000 lines or above must be decomposed or documented as a strong
  ownership-based exception.
- Do not split files or lines mechanically. Group like concepts together,
  keep composition roots thin, and prefer clearer boundaries over smaller raw
  numbers.
- Once a file becomes a composition root, keep new logic in concept-owned
  children instead of rebuilding inline switchboards there.
- Prefer concept-owned names such as `bootstrap.rs`, `provider.rs`, `read.rs`,
  `write.rs`, or `state.rs` over `helpers.rs`, `common.rs`, `misc.rs`, or
  `utils.rs` unless ownership is truly shared and obvious.

## Execution Quality

This project targets enterprise-grade code. Every agent working here must
meet this bar — not "good enough," not "as a first pass," not "can be
improved later."

- **Read before edit.** Read the file, its tests, and its callers before
  changing it. Do not edit files you have not read in this session.
- **Fix root causes.** When a test fails or a warning appears, fix the
  underlying issue. Do not delete tests, weaken assertions, suppress
  warnings, or change expected values to match wrong output.
- **No deferred work inside completion gates.** If a plan's completion gate
  says to handle N cases, handle all N. Do not implement a subset and leave
  TODOs for the rest.
- **Tests verify behavior, not compilation.** Every test must assert a
  specific outcome. A test that only checks "it didn't panic" is not a
  test. Cover happy path, edge cases, and error cases.
- **Verification is evidence.** "Tests pass" without naming the test count
  or showing the output is not verification. Record what you ran and what it
  produced.
- **No lazy-exit phrases.** Do not use "good enough for now," "left as an
  exercise," "out of scope" (for in-scope work), "as a first pass," or
  "can be improved later" to justify incomplete work.

## Common Repo Gotchas

### Crate dependency rules

These are architecture invariants — do not violate them:

- **`nimbus-core` has zero I/O.** Types and validation only. No file reads, no network calls.
- **`nimbus-runtime` has zero workspace dependencies.** It defines the V8 surface and `HostBridge` trait. All Nimbus-specific integration lives in the server's bridge implementation.

### Mutation path

Every mutation — HTTP, WebSocket, scheduler, or V8 runtime — flows through the
engine-owned mutation path (`apply_mutation_with_mode*` plus the queued journal
path). There is no separate code path. Do not create one.

### Storage atomicity

Document write, supporting index effects, and commit log append must remain a
single storage transaction. Never commit a document without its index entries.
Never append a commit without the document write.

### Runtime bundles

Runtime bundles are SHA-256 integrity-checked before every invocation. Runtime host operations (`ctx.db.insert(...)` etc.) go through the same `Service` path as direct HTTP calls — no bypass.

### Schema is optional

A table without a schema accepts any document. Setting a schema adds constraints but never removes the ability to write.

### JavaScript package naming

`packages/nimbus` is the JS SDK. `crates/nimbus` is the Rust facade. When discussing "nimbus" clarify which.
- `packages/nimbus` is the canonical JS implementation. Keep `packages/convex`
  as a compatibility wrapper via thin adapters, aliases, or re-exports when
  behavior matches instead of copy-forwarding parallel logic.

## Verification Commands

- **Format check:** `cargo fmt --all --check`
- **Workspace check:** `make check`
- **Full test suite:** `make test`
- **Lint:** `make clippy`
- **Dependency audit:** `make deny`
- **Third-party attribution gate (G4):** `make verify-third-party-attribution` (unit tests: `make verify-third-party-attribution-helper`)
- **Harness focused lanes:** `make verify-harness` or `make verify-harness SURFACE=runtime`
- **Harness nightly lanes:** `make verify-harness-nightly` or `make verify-harness-nightly SURFACE=server`
- **Harness repro:** `make verify-harness-repro SURFACE=runtime MODE=pr CASE=<case-id>`
- **JS typecheck:** `npm run typecheck`
- **JS tests:** `npm run test`
- **JS build:** `npm run build`
- **All at once:** `make ci`

See `docs/private/staging/operating/local-dev.md` for the build contract; Node is a dev
build dependency for any Rust target that touches `nimbus-server`.

Prefer the `make` entrypoints above for long-running workspace-wide verification:
they are wrapped with the repo's single-flight guard so an accidental duplicate
invocation exits quickly instead of starting another overlapping run. Use
direct `cargo test ...` or `cargo clippy ...` when you intentionally want a
focused crate-level or test-level command.

For focused ad hoc cargo commands, prefer serialized runs against the repo's
shared `target/` so later commands reuse the same artifacts. If Cargo
contention or a stale lock shows up, heal by waiting for the active Cargo
process to finish, or by stopping the genuinely stale/hung process and rerunning
on the shared target. Do not treat alternate artifact directories as the
default recovery path.

Run `cargo fmt --all --check` and `make clippy` before opening a PR. CI enforces
those checks plus `make deny` and `make verify-third-party-attribution`
(the latter is currently a pre-existence pass — it begins enforcing
provenance headers once `crates/nimbus-guest/` or
`crates/nimbus-libkrun-*/` lands).
