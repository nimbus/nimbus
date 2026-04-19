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
- For the landed krun-backed microVM and service-control architecture, go
  directly to `docs/reference/microvm-service-baseline.md` after the three
  docs above. Open the archived plans only when a task needs historical
  execution detail or exact closeout evidence.
- For current macOS developer-machine behavior, open
  `docs/reference/macos-machine-flow.md` after the microVM baseline.
  Open `docs/plans/archive/macos-machine-support-plan.md` only when a task
  needs the historical MAC1-MAC7 execution record or exact proof-bundle paths.
- For historical machine CLI ergonomics work, operator-facing machine UX, or
  machine-command surface closeout evidence, open
  `docs/plans/archive/machine-cli-dx-plan.md` after the microVM baseline.
  That archived plan records the completed `DX1`-`DX11` rollout, proof bundle
  paths, and the final Podman-aligned CLI decisions; use it as historical
  context, then promote a new active plan before starting new machine CLI
  scope.
- For shared machine-lifecycle hardening, enterprise machine-management
  reliability, or Windows-provider groundwork that reuses the existing machine
  manager seams, open
  `docs/plans/archive/machine-lifecycle-hardening-plan.md` after the microVM
  baseline. That archived plan records the landed `MLH1`-`MLH7` robustness
  baseline; use it for historical context and prerequisites, then resume the
  owning active platform plan or promote a new active plan before starting new
  shared machine-hardening scope.

## Context Window Discipline

- `AGENTS.md` is the agent entrypoint; keep it sparse and principle-first.
- Start with `README.md`, `ARCHITECTURE.md`, and `docs/README.md` before loading deeper implementation docs.
- For the current krun-backed microVM and service-control architecture, open
  `docs/reference/microvm-service-baseline.md` immediately after those three
  docs. Only load the archived plans if the task genuinely needs historical
  verification detail or phase-by-phase context.
- For macOS developer-machine work, open
  `docs/reference/macos-machine-flow.md` after the microVM baseline.
  Open `docs/plans/archive/macos-machine-support-plan.md` only when you need
  the historical MAC1-MAC7 execution record, exact real-host proof paths, or
  phase-by-phase closeout context.
- For historical machine CLI DX context, open
  `docs/plans/archive/machine-cli-dx-plan.md` after the microVM baseline and
  reread its `Current Assessed State`, `Current Review Findings`,
  `Verification Contract`, `Roadmap Status Ledger`, and `Execution Log` when
  a task needs the original comparative audit, closeout order, or proof bundle
  paths. Promote a new active plan before resuming new machine-command UX
  scope.
- For shared machine-lifecycle hardening work, open
  `docs/plans/archive/machine-lifecycle-hardening-plan.md` after the microVM
  baseline and reread its `Current Assessed State`, `Current Review Findings`,
  `Podman Alignment Matrix`, `Control Plan Rules`,
  `Verification Contract`, `Roadmap Status Ledger`, and `Execution Log`
  when the task needs the landed shared reliability baseline or exact closeout
  evidence.
- If work spans a platform/reference doc and the archived machine CLI DX plan,
  treat the platform/reference doc as the active architecture owner and the
  archived CLI DX plan as historical user-surface context; promote a new
  active plan before landing new machine-command UX scope.
- If work spans a platform plan and the archived lifecycle hardening plan,
  treat the platform plan as the active owner and the archived lifecycle plan
  as historical reliability context; promote a new active plan before landing
  new shared lifecycle scope.
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

## Common Repo Gotchas

### Crate dependency rules

These are architecture invariants — do not violate them:

- **`neovex-core` has zero I/O.** Types and validation only. No file reads, no network calls.
- **`neovex-runtime` has zero workspace dependencies.** It defines the V8 surface and `HostBridge` trait. All Neovex-specific integration lives in the server's bridge implementation.

### Mutation path

Every mutation — HTTP, WebSocket, scheduler, or V8 runtime — flows through `Service::apply_mutation`. There is no separate code path. Do not create one.

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

## Verification Commands

- **Format check:** `cargo fmt --all --check`
- **Workspace check:** `make check`
- **Full test suite:** `make test`
- **Lint:** `make clippy`
- **Dependency audit:** `make deny`
- **Harness focused lanes:** `make verify-harness` or `make verify-harness SURFACE=runtime`
- **Harness nightly lanes:** `make verify-harness-nightly` or `make verify-harness-nightly SURFACE=server`
- **Harness repro:** `make verify-harness-repro SURFACE=runtime MODE=pr CASE=<case-id>`
- **JS tests:** `npm run test --workspaces --if-present`
- **JS build:** `npm run build --workspaces --if-present`
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
