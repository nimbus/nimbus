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
- `docs/plans/README.md` to find the owning active or deferred execution plan
- The SQLite storage migration plan is complete and archived at
  `docs/plans/archive/pluggable-storage-backend-plan.md`; use it only for
  historical review, not as live progress state.
- The Postgres-first provider implementation plan is complete and archived at
  `docs/plans/archive/postgres-storage-provider-plan.md`; use it only for
  historical review, not as live progress state.
- The umbrella external-provider plan at
  `docs/plans/external-sql-storage-backends-plan.md` is complete historical
  design context. For future provider-topology implementation work, use
  `docs/plans/README.md` to promote or author a new active plan from that
  baseline rather than treating it as live progress state.
- The MySQL provider implementation plan is complete and archived at
  `docs/plans/archive/mysql-storage-provider-plan.md`; use it only for
  historical review, not as live progress state.
- The replica-connected SQLite provider plan is complete and archived at
  `docs/plans/archive/sqlite-replica-provider-plan.md`; use it only for
  historical review, not as live progress state.
- For cleanup or refactor work, go from `docs/plans/README.md` to the owning
  active plan instead of assuming an archived cleanup pass is still live.
- For future admission-control work, go from `docs/plans/README.md` to
  `docs/plans/layered-admission-control-plan.md`.

## Context Window Discipline

- `AGENTS.md` is the agent entrypoint; keep it sparse and principle-first.
- Start with `README.md`, `ARCHITECTURE.md`, and `docs/README.md` before loading deeper implementation docs.
- For active roadmap work, start with `docs/plans/README.md`, then use the owning active plan as the durable control plane.
- Do not resume plans from `docs/plans/archive/` unless the user explicitly
  asks for historical review or follow-up on a completed workstream.
- If archived work needs a new execution pass, create or promote a new active
  plan instead of treating the archived plan as live progress state.
- The SQLite storage migration plan in
  `docs/plans/archive/pluggable-storage-backend-plan.md` is historical context
  only. The Postgres-first provider implementation plan in
  `docs/plans/archive/postgres-storage-provider-plan.md` is also historical
  context only. The MySQL provider implementation plan in
  `docs/plans/archive/mysql-storage-provider-plan.md` is also historical
  context only. The umbrella external-provider plan in
  `docs/plans/external-sql-storage-backends-plan.md` is also historical design
  context only. The replica-connected SQLite provider plan in
  `docs/plans/archive/sqlite-replica-provider-plan.md` is also historical
  context only. If later provider-topology work starts beyond those completed
  records, promote or author a new active plan instead of reopening archived
  progress state.
- For future layered admission work, use
  `docs/plans/layered-admission-control-plan.md`.
- For any active control plan, reread the plan's invariants section
  (`Cleanup Invariants`, `Migration Invariants`, or equivalent),
  `Current Assessed State`, `Current Review Findings`,
  `Feature Preservation Matrix`, `Control Plane Rules`,
  `Verification Contract`, `Roadmap Status Ledger`,
  `Implementation Checkpoints`, `Dependency Graph`,
  `Recommended Delivery Order`, and `Execution Log` before changing code.
- Treat the roadmap plus the current git worktree as progress state. Do not rely on chat history to remember where work stopped.
- If an item is already `in_progress` or the worktree is dirty, reconcile and resume that work before starting a new roadmap item.
- Checkpoint roadmap state before stopping, handing off, or any likely context loss. Do not assume you will get an explicit compaction warning.
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
| `neovex-server` | `crates/neovex-server/` | HTTP/WebSocket transport |
| `neovex-storage` | `crates/neovex-storage/` | Persistence layer |
| `neovex-testing` | `crates/neovex-testing/` | Shared test fixtures and deterministic harness helpers |
| `neovex` (JS SDK) | `packages/neovex/` | Neovex-native JavaScript SDK |
| `convex` (JS compat) | `packages/convex/` | Convex compatibility package |
| `@neovex/codegen` | `packages/codegen/` | Code generation tool |

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
