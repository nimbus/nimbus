<!-- convex-ai-start -->
This project implements a [Convex](https://convex.dev)-compatible backend server.

When working on Convex-compatible code (`packages/convex/`, `demos/convex/`, or any Convex API surface), **always read `docs/private/adapters/convex/ai-guidelines.md` first** for important guidelines on how to correctly use Convex APIs and patterns. The file contains rules that override what you may have learned about Convex from training data.
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
  fence, messaging canon, verification gates), gated on
  `bash scripts/verify-nimbus-docs-site.sh` and
  `bash scripts/check-docs.sh`. Promote a new active plan before
  another docs-site wave.
- Launch-readiness gap closure (deploy admin handshake, `X-Nimbus-Api-Key`
  decision, admin-token rotation gate, configurable CORS, CLI wiring for
  Firestore/MongoDB/DynamoDB, `rest.ts` parity, TLS termination, backup
  command, systemd unit, apt channel, hidden node workload executor
  caller): `docs/private/operating/deploy-admin-api.md` for the deploy
  admin handshake and `NIMBUS_DEPLOY_TOKEN` Bearer contract, gated on
  `bash scripts/verify-launch-readiness.sh` (14 conditions). Promote a
  new active plan before another launch-readiness wave.
- Generic maintainability, refactor, modularity, reliability hardening, or
  canonical naming:
  `docs/private/architecture/testing/reliability-posture.md`,
  `docs/private/architecture/testing/ci-failure-investigation.md`
- Adapter/runtime/auth/trust cleanup:
  `docs/private/architecture/server/adapter-expectations.md`,
  `docs/private/architecture/runtime/adapter-boundary.md`,
  `docs/private/architecture/server/auth-runtime-trust.md`
- Runtime capability segregation, exact service grants, adapter context service
  shortcut removal, private Nimbus-managed isolate host-transport gating,
  Bun/JSC service-capability fail-closed parity, principal-class service route
  policy, engine `Service` -> `Engine` naming, or JS SDK authority boundaries:
  `docs/private/architecture/server/auth-runtime-trust.md`,
  `docs/private/architecture/runtime/adapter-boundary.md`,
  `docs/private/architecture/sandbox/service-sandbox-session-model.md`,
  verifier `bash scripts/verify-nimbus-capability-segregation.sh` 10/0.
- Cross-cutting multi-backend / multi-adapter hardening (storage trait
  segregation, adapter/backend registration seam decision,
  `RuntimeHooks` for backend-coupled workers, dual-target tests per
  adapter, auth-caching ADR, per-backend SQL-safety ADRs, per-segment
  latency budgets, trait object-safety audit, stable logical table identity +
  backend-owned physical-layout decision, typed-column key storage,
  read-consistency routing, hybrid event-capture pattern, cross-cutting
  `docs/private/technical-debt.md`):
  `docs/private/operating/multi-backend-adapter-hardening.md` for the current
  contract, gated on
  `bash scripts/verify-multi-backend-adapter-hardening.sh` (fifteen
  conditions). Inspiration source is ExtendDB (Apache-2.0 DynamoDB adapter
  on PostgreSQL) at `~/src/github.com/ExtendDB/extenddb`. Applies across
  every existing backend and adapter, not only the DynamoDB lane.
- Convex-informed storage trust gaps (table lifecycle after stable
  `table_catalog`, table-aware Convex document identity validation,
  `TableId`-based dependency tracking, stable index identity/lifecycle,
  history/repeatable-read posture, table identity diagnostics, and
  cross-backend conformance after comparing against Convex internals):
  `docs/private/architecture/storage/table-identity.md`,
  `docs/private/architecture/storage/consistency-routing.md`, and
  `docs/private/architecture/storage/persistence-engine-baseline.md` for the
  current contract. Use local Convex source at
  `~/src/github.com/get-convex/convex-backend` for comparison. Promote a new
  active plan before another Convex-informed storage trust wave.
- Sandbox, machine lifecycle, or CLI UX:
  `docs/private/architecture/sandbox/service-sandbox-session-model.md`,
  `docs/private/architecture/sandbox/microvm-service-baseline.md`,
  `docs/private/architecture/sandbox/macos-machine-flow.md` when relevant,
  `docs/private/operating/cli.md`, and the active platform plan from
  `docs/private/plans/README.md`
- SDK services/sandboxes/sessions resource model, built-in/external/sandbox-backed
  service implementations, dynamic services, sandbox APIs, runtime-isolate
  non-resource semantics, future `profile: "isolate"` sandbox semantics, or
  session target semantics:
  `docs/private/architecture/sandbox/service-sandbox-session-model.md`
- In-process filesystem (NimbusFS mount table + V8/WASI binders + tier-gated
  backends + `FsCaps`, replacing `deno_fs::RealFs`), object storage (two-plane
  content-addressed chunk core + `s3s` S3 surface + Local/Mirror/Tier/
  Cloud-primary placement over `object_store` + "an S3 becomes a mount" FS
  binder), or tier-neutral egress (extracting `EgressPolicy`/`EgressGateway`
  from the container-only `SandboxEgressProxy`, binding isolate/wasm `fetch`):
  the three-plan portfolio from the `/tmp/nimbus-isolate-architecture.html`
  deep-dive — `docs/private/plans/nimbus-isolate-filesystem-plan.md` (NFS0..NFS7,
  foundational; gated on `bash scripts/verify-nimbus-isolate-filesystem.sh`,
  10 conditions), `docs/private/plans/nimbus-s3-object-storage-plan.md`
  (NOS0..NOS7, depends on NFS6; gated on
  `bash scripts/verify-nimbus-s3-object-storage.sh`, 12 conditions; Garage is
  AGPL-3.0 and excluded), and
  `docs/private/plans/nimbus-egress-gateway-extraction-plan.md` (NEG0..NEG7,
  independent; gated on
  `bash scripts/verify-nimbus-egress-gateway-extraction.sh`, 10 conditions).
  Honor the `nimbus-runtime` zero-workspace-dep and `nimbus-core` zero-I/O
  invariants via injected traits (`NimbusFsBackend`, `EgressGateway` — both the
  HostBridge pattern). Coordinate the NimbusFS WASI binder and the egress wasm
  binding with `docs/private/plans/wasi-agent-capabilities-plan.md` /
  `docs/private/plans/wasmtime-backend-plan.md`.
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
  (macOS outer-VM flow). Firecracker-as-separate-VMM was dropped per D2.
  Macros: Linux production
  runs direct libkrun-on-KVM microVMs per service; macOS dev runs ONE
  outer machine-os Linux VM and per-workload sandboxes inside it are
  standard Linux containers (crun), NOT nested microVMs (D11/D12).
  Snapshot/fork S0..S5 is Linux-KVM-only by construction.
- Node-side systemd D-Bus binding (`SystemdDbusClient` /
  `SystemdTransientUnitBackend` / `NodeWorkloadReconciler`):
  `docs/private/operating/node-dbus-binding.md` for the canonical contract,
  with decision rationale and option matrix in
  `docs/private/plans/research/systemd-dbus-binding-rust-2026.md`. The binding
  attaches `lucab/zbus_systemd` (pin `=0.26000.0`, features `systemd1` +
  `zbus-async-tokio`) and direct `zbus` to the trait at
  `crates/nimbus-node/src/systemd_transient.rs:15-32`. Signal-correlated job
  completion (call systemd `Manager.Subscribe`, establish the `JobRemoved`
  stream *before* calling `StartTransientUnit`/`StopUnit`, complete only on
  matching signal `result`) is the trust-critical path, not polling. Linux
  builds default to `ZbusSystemdClient` instead of
  `UnavailableSystemdDbusClient`; the `node-dbus-integration` CI lane on
  `ubuntu-24.04` runs the `systemctl --user` integration tests. Gated on
  `bash scripts/verify-node-dbus-binding.sh` (10 conditions). No production
  caller for `NodeWorkloadReconciler` is wired yet — that needs its own
  follow-up plan.
- CLI daemon canonicalization, walk-up boundaries, or banner shape:
  `docs/private/operating/cli.md`. Promote a new active plan before another
  CLI-canonicalization wave.
- Localhost/server security:
  `docs/private/architecture/server/auth-runtime-trust.md` (see "Localhost
  Server Hardening"), implemented in `crates/nimbus-server/src/local_server/`
  (`middleware.rs`, `mod.rs`, `discovery.rs`) and wired through
  `crates/nimbus-server/src/router.rs`.
- Install script work:
  `docs/private/plans/distribution-plan.md` as parent context; promote a new
  active plan before another install-script wave
- Local-dev / build-contract / Make-vs-Cargo orchestration:
  `docs/private/operating/local-dev.md` for the user-facing contract. The Makefile UI
  dependency graph at the top of `Makefile` (`UI_PKG`, `UI_DIST_INDEX`,
  etc.) is the source of truth for cross-toolchain prerequisites;
  `crates/nimbus-server/build.rs` only asserts that those inputs exist
  and errors actionably otherwise. Promote a new active plan before
  another local-dev / build-graph wave.
- CI caching / sccache / Swatinem orchestration:
  `docs/private/operating/ci-caching.md` for the canonical caching contract
  (sccache rollout across every Rust job in `.github/workflows/*.yml`,
  Swatinem `shared-key` rotation, `save-if: refs/heads/main` for
  PR-cannot-poison-main saves, the `ui-artifacts` and `warm-sccache` leader
  jobs, and the `mozilla-actions/sccache-action@v0.0.6` GHA-cache-v2 pin
  floor). Promote a new active plan before another CI caching /
  sccache / Swatinem wave.
- CI infrastructure modernization (composite actions, SHA pinning,
  runner determinism, job summaries, SAST):
  `docs/private/operating/ci-modernization.md` for the canonical contract
  (the cross-workflow Rust + sccache + Swatinem composite action at
  `.github/actions/setup-rust-cached/`, SHA-pinning every non-`actions/*`
  `uses:` reference with a `# vX.Y.Z` version-name comment, pinning
  `runs-on:` to `ubuntu-24.04`, `$GITHUB_STEP_SUMMARY` markdown emission,
  and `.github/workflows/codeql.yml`), gated on
  `bash scripts/verify-ci-modernization.sh` (12 conditions). Promote
  a new active plan before another CI infrastructure / SAST /
  composite-action wave.
- Coverage / release-pipeline acceleration (mold linker, coverage
  parallelism + sharding, release.yml composite adoption, Windows
  release-build investigation): `docs/private/operating/ci-modernization.md`
  for the canonical contract (see "Coverage and release acceleration"
  section). `mold` is installed in the `setup-rust-cached` composite via
  `CARGO_TARGET_*_RUSTFLAGS=-C link-arg=-fuse-ld=mold` (NOT
  `LINKER=mold` — that invocation fails with `mold: fatal: unknown
  -m argument: 64`); Coverage runs `-j 4` sharded into 3 lanes
  (`server` / `engine` / `rest`) feeding a `cargo llvm-cov report --lcov`
  reducer via `target/llvm-cov-target/profraw/`. The Windows release pole
  (vendored OpenSSL, V8 link, cold-target build) is deferred to a follow-on
  release-acceleration plan. Promote a new active plan before another
  coverage / release acceleration wave.
- PR CI wall acceleration (verification-harness sharding, workspace-
  tests sharding via cargo-nextest --partition, external-provider
  matrix split by provider, warm-sccache shrink):
  `docs/private/operating/ci-modernization.md` for the canonical contract
  ("PR critical-path acceleration" section). The sharding env-vars are
  `NIMBUS_HARNESS_SHARD=N/M` (in-test corpus filter for the server
  verification harness), `NIMBUS_NEXTEST_PARTITION=N/M` (Makefile forwarding
  to nextest `--partition hash:N/M` for workspace tests), and
  `NIMBUS_PROVIDER_FILTER=postgres|mysql|libsql` (per-provider
  external-provider test-script filter). Promote a new active plan before
  another PR-wall acceleration wave.
- CI PR-wall sub-15 (post-CW pole attack — libsql image pin +
  docker-image cache, coverage extraction to its own workflow,
  branch-conditional concurrency cap, warm-sccache retain-or-retire
  decision): `docs/private/operating/ci-pr-wall.md` for the canonical
  contract. `ghcr.io/tursodatabase/libsql-server` is pinned to a `vX.Y.Z`
  tag chosen by probing GHCR directly (the upstream GitHub release list can
  contain tags that 404 on GHCR), wrapped in a three-step `actions/cache@v5`
  lane keyed on `libsql-image-vX.Y.Z`; the Coverage shards + reducer live in
  `.github/workflows/coverage.yml` on `push.main + schedule +
  workflow_dispatch` (off `rust-gate-summary.needs:`, so never gating merge);
  ci.yml's `cancel-in-progress` is `${{ github.ref != 'refs/heads/main' }}`
  so cancelled main runs no longer abandon cache saves mid-flight;
  warm-sccache is retained (libsql shard hits Swatinem 0% while harness lanes
  hit 76%+). Promote a new active plan before another PR-wall
  acceleration wave.
- Firebase/Firestore compatibility:
  `docs/private/adapters/firebase/compatibility.md`,
  `docs/private/adapters/firebase/migration.md`,
  `docs/private/adapters/firebase/auth-contract.md`,
  `docs/private/architecture/runtime/adapter-boundary.md`,
  `docs/private/architecture/server/auth-runtime-trust.md`
- Cloud Functions compatibility:
  `docs/private/adapters/cloud-functions/compatibility.md`,
  `docs/private/adapters/cloud-functions/migration.md`,
  `docs/private/architecture/runtime/adapter-boundary.md`,
  `docs/private/architecture/server/auth-runtime-trust.md`
- Cloudflare adapters (inbound Workers / Workers KV / D1 / R2 / Durable Objects
  compatibility): `docs/private/plans/cloudflare-adapters-plan.md` is the active
  control plane (CFA0..CFA7), gated on
  `bash scripts/verify-cloudflare-adapters.sh` (10 conditions). Contract source
  of truth is `docs/private/plans/research/cloudflare-adapters-2026.md` (do not
  re-derive Cloudflare API contracts from memory). Direction is inbound-compat-
  first; build wedge is Workers KV then Durable Objects; D1, R2, and full
  Worker-code execution are named follow-on bands. `workerd`/`miniflare`/
  `workers-types` are Apache-2.0/MIT and freely incorporable. Also read
  `docs/private/architecture/runtime/adapter-boundary.md` and
  `docs/private/architecture/server/auth-runtime-trust.md`.
- Convex or Nimbus CLI/codegen workflow:
  `docs/private/adapters/convex/ai-guidelines.md`,
  `docs/private/operating/cli.md`,
  `docs/private/adapters/convex/compatibility.md`
- Node-compatible runtime / `deno_core` / `rusty_v8` / embedded-codegen:
  `docs/private/architecture/runtime/adapter-boundary.md` and
  `docs/private/architecture/server/auth-runtime-trust.md` after the top-level docs.
  Node default-quality hardening is complete (merged PR #10, Cycle 37,
  2026-06-16): Node24 is a well-supported default, Node22/Node24 carry
  expanded official fixture + package evidence, Node26 has real Current-line
  fixture evidence, and `v8_isolate_required` is at 100% across
  node22/node24/node26. If new Node-compat roadmap work is needed, create or
  adopt a fresh active plan before starting a new wave.
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
| `nimbus-adapters` | `crates/nimbus-adapters/` | Optional adapter-family aggregation crate |
| `nimbus-auth` | `crates/nimbus-auth/` | Shared auth and identity primitives |
| `nimbus-bin` | `crates/nimbus-bin/` | CLI binary entry point |
| `nimbus-core` | `crates/nimbus-core/` | Shared types and validation (zero I/O) |
| `nimbus-engine` | `crates/nimbus-engine/` | Central coordinator (`Engine`) |
| `nimbus-node` | `crates/nimbus-node/` | Host-local workload reconciliation and systemd integration |
| `nimbus-runtime` | `crates/nimbus-runtime/` | V8 execution (zero workspace deps) |
| `nimbus-sandbox` | `crates/nimbus-sandbox/` | Generic sandbox and isolation seam |
| `nimbus-server` | `crates/nimbus-server/` | HTTP/WebSocket transport |
| `nimbus-services` | `crates/nimbus-services/` | Service, sandbox, and session resource manager |
| `nimbus-storage` | `crates/nimbus-storage/` | Persistence layer |
| `nimbus-tenant` | `crates/nimbus-tenant/` | Tenant policy and workload admission decisions |
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

### GitHub CLI auth under sandbox

If `gh` reports an invalid token or auth failure inside the sandbox, retry the
same GitHub CLI operation with elevated permissions before treating credentials
as broken. Record a real credential blocker only after the elevated command
fails too.

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

A runtime bundle that carries a recorded SHA-256 (its provenance hash) is re-hashed and verified against that hash before every invocation; a tampered or stale bundle is rejected. A path-backed bundle loaded without recorded provenance has no expected hash and is admitted on filesystem trust alone (see `verify_integrity`). Runtime host operations (`ctx.db.insert(...)` etc.) go through the same `Engine` path as direct HTTP calls — no bypass.

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

See `docs/private/operating/local-dev.md` for the build contract; Node is a dev
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
