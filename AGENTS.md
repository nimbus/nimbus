<!-- convex-ai-start -->
This project implements a [Convex](https://convex.dev)-compatible backend server.

When working on Convex-compatible code (`packages/convex/`, `demos/convex/`, or any Convex API surface), **always read `docs/adapters/convex/ai-guidelines.md` first** for important guidelines on how to correctly use Convex APIs and patterns. The file contains rules that override what you may have learned about Convex from training data.
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

## Working Set

- Start with `README.md`, `ARCHITECTURE.md`, `docs/README.md`, and
  `docs/plans/README.md`.
- Use the active plan owner for the slice you are touching. Prefer active
  plans over archived history.
- Treat the current git worktree plus the owning active plan as progress
  state. Resume `in_progress` work before starting a new roadmap item.
- Checkpoint plan state before stopping, handing off, or any likely context
  loss.
- Load one roadmap item at a time plus only the immediately relevant code,
  tests, and docs.

### Routing By Work Type

- Generic maintainability, refactor, modularity, reliability hardening, or
  canonical naming:
  `docs/architecture/testing/reliability-posture.md`,
  `docs/architecture/testing/ci-failure-investigation.md`,
  `docs/plans/archive/architecture-seam-cleanliness-plan.md`,
  `docs/plans/archive/deployment-auth-runtime-boundary-plan.md`,
  `docs/plans/archive/repo-architecture-and-seam-hardening-plan.md`
- Adapter/runtime/auth/trust cleanup:
  `docs/architecture/server/adapter-expectations.md`,
  `docs/architecture/runtime/adapter-boundary.md`,
  `docs/architecture/server/auth-runtime-trust.md`,
  `docs/plans/archive/deployment-auth-runtime-boundary-plan.md`
  Use the completed baselines in `docs/plans/archive/server-runtime-canonicalization-plan.md`,
  `docs/plans/archive/adapter-runtime-trust-hardening-plan.md`,
  `docs/plans/archive/runtime-capability-adapter-boundary-plan.md`, and
  `docs/plans/archive/multi-adapter-boundary-hardening-plan.md` only as prior
  wave references.
- Sandbox, machine lifecycle, or CLI UX:
  `docs/architecture/sandbox/microvm-service-baseline.md`,
  `docs/architecture/sandbox/macos-machine-flow.md` when relevant,
  `docs/operating/cli.md`, and the active platform plan from
  `docs/plans/README.md`
- Localhost/server security:
  `docs/plans/archive/localhost-server-security-plan.md`
- Install script work:
  `docs/plans/install-script-plan.md` as the active owner and
  `docs/plans/distribution-plan.md` as parent context
- Firebase/Firestore compatibility:
  `docs/adapters/firebase/compatibility.md`,
  `docs/adapters/firebase/migration.md`,
  `docs/adapters/firebase/auth-contract.md`,
  `docs/architecture/runtime/adapter-boundary.md`,
  `docs/architecture/server/auth-runtime-trust.md`
- Cloud Functions compatibility:
  `docs/adapters/cloud-functions/compatibility.md`,
  `docs/adapters/cloud-functions/migration.md`,
  `docs/architecture/runtime/adapter-boundary.md`,
  `docs/architecture/server/auth-runtime-trust.md`
- Convex or Neovex CLI/codegen workflow:
  `docs/adapters/convex/ai-guidelines.md`,
  `docs/operating/cli.md`,
  `docs/adapters/convex/compatibility.md`,
  `docs/plans/archive/neovex-init-plan.md`
- Node-compatible runtime / `deno_core` / `rusty_v8` / embedded-codegen:
  `docs/architecture/runtime/adapter-boundary.md` and
  `docs/architecture/server/auth-runtime-trust.md` after the top-level docs.
  Use `docs/plans/archive/node-compatible-runtime-plan.md`,
  `docs/plans/archive/node-lts-compatibility-plan.md`,
  `docs/plans/archive/node-compat-test-infrastructure-plan.md`, and
  `docs/plans/archive/node-compat-future-lanes-and-correctness-plan.md` as completed
  baselines. If new Node-compat roadmap work is needed beyond those completed
  plans, create or adopt a fresh active plan before starting a new wave.
  `~/src/github.com/agentstation/deno` as the canonical Deno-family fork,
  `~/src/github.com/agentstation/rusty_v8` as the matching V8 fork,
  `~/src/github.com/agentstation/deno_core` only as historical delta context,
  `~/src/github.com/denoland/deno` for upstream comparison, and
  `~/src/github.com/nodejs/node` for upstream Node source/tests.
  Prefer working and verifying against those canonical worktrees with normal
  sandbox approval when needed. Do not make `/private/tmp` checkout copies or
  alternate Cargo-source workspaces the default workflow.
  For Deno-owner changes, temporarily unpin Neovex from the published
  `agentstation/deno` tag and point the Deno-family dependencies at the
  canonical `~/src/github.com/agentstation/deno` worktree while proving the
  fix. Do not create shadow checkout copies to mimic the pin.
  Once the fork change is verified, commit/tag/push it in
  `~/src/github.com/agentstation/deno`, then repin `Cargo.toml` and
  `Cargo.lock` back to the published tag/revision and rerun Neovex
  verification on that repinned baseline before updating the control plane.
  Keep Neovex-specific bootstrap/profile/capability fixes local. Promote a fix
  to `agentstation/deno` when the local alternative would duplicate Deno/Node
  builtin semantics, shadow internal behavior long-term, or add avoidable
  hot-path overhead. For one-off macOS fork verification that must bypass the
  checked-in `-fuse-ld=lld` target flag, prefer `CARGO_ENCODED_RUSTFLAGS`.
  Use `/private/tmp` Cargo overrides only as short-lived last-resort proof
  paths, never as progress state or the main source of truth.

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
