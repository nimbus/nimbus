<!-- convex-ai-start -->
This project implements a [Convex](https://convex.dev)-compatible backend server.

When working on Convex-compatible code (`packages/convex/`, `demos/convex/`, or any Convex API surface), **always read `docs/reference/convex-ai-guidelines.md` first** for important guidelines on how to correctly use Convex APIs and patterns. The file contains rules that override what you may have learned about Convex from training data.
<!-- convex-ai-end -->

# Neovex

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

## Canonical References

### Project docs

Use the repo docs for architecture and behavior details:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- Use `docs/plans/README.md` to identify the current active plan before
  landing roadmap work. Prefer the active owner over archived history.
- For generic maintainability, refactor, modularity, reliability hardening,
  canonical naming, or god-file cleanup work, open
  `docs/reference/reliability-posture.md` and
  `docs/reference/ci-failure-investigation.md` after the three docs above.
  Use `docs/plans/architecture-seam-cleanliness-plan.md` as the latest
  completed repo-wide architecture/modularity/seam-cleanliness baseline, and
  promote a new active plan before another broad architecture cleanup wave.
  Use `docs/plans/deployment-auth-runtime-boundary-plan.md` as the latest
  completed baseline when the work specifically touches repo-wide deploy
  activation, application auth lifecycle, or runtime ABI cleanup. Use
  `docs/plans/repo-architecture-and-seam-hardening-plan.md` as the latest
  completed baseline for the prior repo-wide architecture wave.
- For post-Firebase / post-Cloud-Functions adapter-boundary cleanup,
  compatibility-truth reconciliation, runtime-host seam promotion, auth
  ownership cleanup, provider-family seam cleanup, or runtime trust
  hardening, open `docs/reference/adapter-expectations.md`,
  `docs/reference/runtime-adapter-boundary.md`,
  `docs/reference/server-auth-runtime-trust.md`, and
  `docs/plans/deployment-auth-runtime-boundary-plan.md` after the three docs
  above plus the reliability references when the work touches deploy/auth/live
  runtime seams. Treat it as the latest completed baseline for that
  cross-cutting wave and promote a new active plan before another broad seam
  pass. Use
  `docs/plans/repo-architecture-and-seam-hardening-plan.md` as the latest
  completed repo-wide seam-hardening baseline, not an active owner. Use
  `docs/plans/server-runtime-canonicalization-plan.md` as the completed
  canonicalization baseline and execution record, use
  `docs/plans/adapter-runtime-trust-hardening-plan.md` as the completed trust
  baseline, use `docs/plans/runtime-capability-adapter-boundary-plan.md` as
  the completed adapter/runtime ownership baseline, and use
  `docs/plans/archive/multi-adapter-boundary-hardening-plan.md` only as the earlier
  completed historical hardening wave.
- For the landed krun-backed microVM and service-control architecture, open
  `docs/reference/microvm-service-baseline.md` after the three docs above.
- For current macOS developer-machine behavior, open
  `docs/reference/macos-machine-flow.md` after the microVM baseline.
- For machine/service CLI UX work, help/output/progress consistency, or
  Podman/Docker-style command-surface work, start with `docs/reference/cli.md`
  and `docs/reference/microvm-service-baseline.md`. Promote a new active plan
  before landing another CLI command-surface wave unless one already owns the
  slice.
- For shared machine-lifecycle hardening, enterprise machine-management
  reliability, or Windows-provider groundwork that reuses the existing machine
  manager seams, start with `docs/reference/microvm-service-baseline.md`,
  `docs/reference/macos-machine-flow.md` when relevant, and the active platform
  plan from `docs/plans/README.md`.
- **For localhost/server security work:** start with the completed contract in
  `docs/plans/localhost-server-security-plan.md` for local
  bind/auth/session/origin/CORS, token lifecycle, server discovery,
  route-family gating, audit logging, `/ui/*` bootstrap security, and the
  server-access versus application-auth boundary. Promote a new active plan
  before landing another security wave that materially changes that contract.
- **For install script work (Channel 1):** `docs/plans/install-script-plan.md`
  is the active control plan for the `curl | sh` quick-start bootstrapper. Its
  parent is `docs/plans/distribution-plan.md` (Channel 1 section). Start I1
  immediately — external release inputs already exist. Keep
  `docs/plans/distribution-plan.md` open for the macOS Homebrew contract and
  Linux dependency context, but treat the install-script plan as the execution
  owner.
- **For Firebase/Firestore compatibility work:** start with
  `docs/reference/firebase-compatibility.md` and
  `docs/reference/firebase-migration-guide.md` and
  `docs/reference/firebase-auth-contract.md`, then use
  `docs/reference/runtime-adapter-boundary.md` and
  `docs/reference/server-auth-runtime-trust.md` plus
  `docs/plans/repo-architecture-and-seam-hardening-plan.md` as the latest
  completed repo-wide auth/runtime/modularity baseline. Promote a new active
  plan before another broad Firebase-driven seam-hardening wave. Use
  `docs/plans/server-runtime-canonicalization-plan.md` as the completed
  latest canonicalization baseline for historical context. Use
  `docs/plans/adapter-runtime-trust-hardening-plan.md` as the completed auth,
  trust, and boundary baseline, use
  `docs/plans/runtime-capability-adapter-boundary-plan.md` as the latest
  completed adapter/runtime boundary baseline, and use
  `docs/plans/archive/multi-adapter-boundary-hardening-plan.md` only as the completed
  historical auth, compatibility-truth, and boundary-hardening wave. Use
  `docs/plans/archive/firebase-adapter-plan.md` only as the completed
  historical execution record for the adapter and primitive-hardening wave.
- **For Cloud Functions compute or HTTP handler work:** start with
  `docs/reference/cloud-functions-compatibility.md` and
  `docs/reference/cloud-functions-migration-guide.md`, then use
  `docs/reference/runtime-adapter-boundary.md` and
  `docs/reference/server-auth-runtime-trust.md` plus
  `docs/plans/repo-architecture-and-seam-hardening-plan.md` as the latest
  completed repo-wide runtime/auth/modularity baseline. Promote a new active
  plan before another broad Cloud Functions seam-hardening wave. Use
  `docs/plans/server-runtime-canonicalization-plan.md` as the completed
  latest runtime/auth/modularity canonicalization baseline. Use
  `docs/plans/adapter-runtime-trust-hardening-plan.md` as the completed
  runtime trust and boundary baseline, use
  `docs/plans/runtime-capability-adapter-boundary-plan.md` as the latest
  completed runtime-host and adapter-boundary baseline, and use
  `docs/plans/archive/multi-adapter-boundary-hardening-plan.md` only as the earlier
  completed cross-adapter wave. Use
  `docs/plans/archive/firebase-cloud-functions-plan.md` only as the completed
  historical execution record for Firebase v2 and standalone Functions
  Framework compatibility.
- **For Convex or Neovex CLI/codegen workflow work:** after
  `docs/reference/convex-ai-guidelines.md`, open `docs/reference/cli.md` and
  `docs/convex/compatibility.md` for `packages/codegen/`,
  `packages/convex/`, `demos/convex/`, or the `neovex start --app-dir`
  contract. Promote a new active plan before landing another CLI/codegen
  workflow wave unless one already owns the slice. Use
  `docs/plans/archive/codegen-and-facade-hardening-plan.md` only for the
  completed cleanup wave's execution record.

## Context Window Discipline

- `AGENTS.md` is the agent entrypoint; keep it sparse and principle-first.
- Start with `README.md`, `ARCHITECTURE.md`, and `docs/README.md` before loading deeper implementation docs.
- Use `docs/plans/README.md` to discover the current active plan owner for the
  slice you are touching. Do not rely on archived plans as the default source
  of truth.
- For generic maintainability, refactor, modularity, reliability hardening,
  canonical naming, or readability cleanup work, open
  `docs/reference/reliability-posture.md` and
  `docs/reference/ci-failure-investigation.md` immediately after those three
  docs. Use `docs/plans/architecture-seam-cleanliness-plan.md` as the latest
  completed repo-wide architecture/modularity/seam-cleanliness baseline, and
  promote a new active plan before another broad architecture cleanup wave.
  Use `docs/plans/deployment-auth-runtime-boundary-plan.md` as the latest
  completed baseline when the work specifically touches repo-wide deploy
  activation, application auth lifecycle, or runtime ABI cleanup. Use
  `docs/plans/repo-architecture-and-seam-hardening-plan.md` as the latest
  completed baseline for the prior repo-wide architecture wave.
- For post-Firebase / post-Cloud-Functions adapter-boundary cleanup,
  compatibility-truth reconciliation, runtime-host seam promotion, auth
  ownership cleanup, provider-family seam cleanup, or runtime trust
  hardening, open `docs/reference/adapter-expectations.md`,
  `docs/reference/runtime-adapter-boundary.md`,
  `docs/reference/server-auth-runtime-trust.md`, and
  `docs/plans/deployment-auth-runtime-boundary-plan.md` immediately after
  those three docs plus the reliability references when the work touches
  deploy/auth/live runtime seams. Treat that plan as the latest completed
  baseline for the wave and promote a new active plan before another broad
  seam pass. Use
  `docs/plans/repo-architecture-and-seam-hardening-plan.md` as the latest
  completed repo-wide seam-hardening baseline. Use
  `docs/plans/server-runtime-canonicalization-plan.md` as the completed
  canonicalization baseline, use
  `docs/plans/adapter-runtime-trust-hardening-plan.md` as
  the completed trust/boundary baseline, use
  `docs/plans/runtime-capability-adapter-boundary-plan.md` as the latest
  completed adapter/runtime ownership baseline, and use
  `docs/plans/archive/multi-adapter-boundary-hardening-plan.md` only as the earlier
  completed wave.
- For the current krun-backed microVM and service-control architecture, open
  `docs/reference/microvm-service-baseline.md` immediately after those three
  docs.
- For macOS developer-machine work, open
  `docs/reference/macos-machine-flow.md` after the microVM baseline.
- For historical machine/service CLI alignment work, start with
  `docs/reference/cli.md` and `docs/reference/microvm-service-baseline.md`.
  Promote a new active plan before starting another CLI UX wave unless one
  already owns the slice.
- For shared machine-lifecycle hardening work, open
  `docs/reference/microvm-service-baseline.md` after the microVM baseline and
  then the active platform plan from `docs/plans/README.md`.
- For localhost/server security work, open
  `docs/plans/localhost-server-security-plan.md` after the three top-level
  docs and treat it as the settled contract unless a newer active plan owns
  the slice. Keep local server-access auth separate from tenant/application
  auth unless an active plan explicitly says otherwise.
- For Firebase/Firestore compatibility work, open
  `docs/reference/firebase-compatibility.md` and
  `docs/reference/firebase-migration-guide.md` and
  `docs/reference/firebase-auth-contract.md` after the three top-level docs.
  Use `docs/reference/runtime-adapter-boundary.md`,
  `docs/reference/server-auth-runtime-trust.md`, and
  `docs/plans/repo-architecture-and-seam-hardening-plan.md` as the latest
  completed repo-wide canonicalization baseline. Promote a new active plan
  before another broad Firebase-driven seam-hardening wave. Use
  `docs/plans/server-runtime-canonicalization-plan.md` as the completed
  latest canonicalization baseline for historical context. Use
  `docs/plans/adapter-runtime-trust-hardening-plan.md` as the completed trust
  and boundary baseline, use
  `docs/plans/runtime-capability-adapter-boundary-plan.md` as the latest
  completed adapter/runtime boundary baseline, and use
  `docs/plans/archive/multi-adapter-boundary-hardening-plan.md` only as the completed
  historical auth, compatibility-truth, and boundary-hardening wave. Use
  `docs/plans/archive/firebase-adapter-plan.md` only when you need the
  completed execution record for the historical adapter wave.
- For Cloud Functions compute, HTTP handlers, or trigger work (both Firebase
  and standalone), open `docs/reference/cloud-functions-compatibility.md` and
  `docs/reference/cloud-functions-migration-guide.md` after the three
  top-level docs. Use `docs/reference/runtime-adapter-boundary.md`,
  `docs/reference/server-auth-runtime-trust.md`, and
  `docs/plans/repo-architecture-and-seam-hardening-plan.md` as the latest
  completed repo-wide canonicalization baseline. Promote a new active plan
  before another broad Cloud Functions seam-hardening wave. Use
  `docs/plans/server-runtime-canonicalization-plan.md` as the completed
  latest canonicalization baseline for historical context. Use
  `docs/plans/adapter-runtime-trust-hardening-plan.md` as the completed
  runtime trust and boundary baseline, use
  `docs/plans/runtime-capability-adapter-boundary-plan.md` as the latest
  completed runtime-host and adapter-boundary baseline, and use
  `docs/plans/archive/multi-adapter-boundary-hardening-plan.md` only as the earlier
  completed cross-adapter wave. Use
  `docs/plans/archive/firebase-cloud-functions-plan.md` only when you need the
  completed execution record for the historical compatibility wave.
- For Convex or Neovex CLI/codegen workflow work, open
  `docs/reference/convex-ai-guidelines.md`, `docs/reference/cli.md`, and
  `docs/convex/compatibility.md` after the three top-level docs. Promote a
  new active plan before landing another CLI/codegen workflow wave unless one
  already owns the slice. Use
  `docs/plans/archive/codegen-and-facade-hardening-plan.md` only when you
  need the completed cleanup wave's execution record.
- Treat the current git worktree plus the owning active plan, when there is
  one, as progress state. Do not rely on chat history to remember where work
  stopped.
- If an active roadmap item is already `in_progress` or the worktree is dirty,
  reconcile and resume that work before starting a new roadmap item.
- Checkpoint active roadmap state before stopping, handing off, or any likely
  context loss. Do not assume you will get an explicit compaction warning.
- Load one roadmap item at a time plus only the immediately relevant code, tests, and docs.

### Workspace layout

The repo is a Rust workspace + npm monorepo. Names overlap — know which you mean:

| Name | Path | What it is |
| --- | --- | --- |
| `neovex` (facade crate) | `crates/neovex/` | Re-exports public types for embedders |
| `neovex-bin` | `crates/neovex-bin/` | CLI binary entry point |
| `neovex-core` | `crates/neovex-core/` | Shared types and validation (zero I/O) |
| `neovex-engine` | `crates/neovex-engine/` | Central coordinator (`Service`) |
| `neovex-runtime` | `crates/neovex-runtime/` | V8 execution (zero workspace deps) |
| `neovex-sandbox` | `crates/neovex-sandbox/` | Generic sandbox and isolation seam |
| `neovex-server` | `crates/neovex-server/` | HTTP/WebSocket transport |
| `neovex-storage` | `crates/neovex-storage/` | Persistence layer |
| `neovex-testing` | `crates/neovex-testing/` | Shared test fixtures and deterministic harness helpers |
| `neovex` (JS SDK) | `packages/neovex/` | Neovex-native JavaScript SDK |
| `convex` (JS compat) | `packages/convex/` | Convex compatibility package |
| `@neovex/codegen` | `packages/codegen/` | Code generation tool |

### Rust target layout

- Reserve `examples/` for user-facing example programs.
- Put internal benchmark or evaluation runners under `benches/` with explicit
  custom-harness targets when they are driven through `cargo bench`.
- Keep integration tests in `tests/` and support helpers beside the owning
  crate unless they are shared widely enough to justify `neovex-testing`.

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

## Common Repo Gotchas

### Crate dependency rules

These are architecture invariants — do not violate them:

- **`neovex-core` has zero I/O.** Types and validation only. No file reads, no network calls.
- **`neovex-runtime` has zero workspace dependencies.** It defines the V8 surface and `HostBridge` trait. All Neovex-specific integration lives in the server's bridge implementation.

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

`packages/neovex` is the JS SDK. `crates/neovex` is the Rust facade. When discussing "neovex" clarify which.
- `packages/neovex` is the canonical JS implementation. Keep `packages/convex`
  as a compatibility wrapper via thin adapters, aliases, or re-exports when
  behavior matches instead of copy-forwarding parallel logic.

## Verification Commands

- **Format check:** `cargo fmt --all --check`
- **Workspace check:** `make check`
- **Full test suite:** `make test`
- **Lint:** `make clippy`
- **Dependency audit:** `make deny`
- **Harness focused lanes:** `make verify-harness` or `make verify-harness SURFACE=runtime`
- **Harness nightly lanes:** `make verify-harness-nightly` or `make verify-harness-nightly SURFACE=server`
- **Harness repro:** `make verify-harness-repro SURFACE=runtime MODE=pr CASE=<case-id>`
- **JS typecheck:** `npm run typecheck`
- **JS tests:** `npm run test`
- **JS build:** `npm run build`
- **All at once:** `make ci`

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
those checks plus `make deny`.
