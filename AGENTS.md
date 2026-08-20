<!-- convex-ai-start -->
This project implements a [Convex](https://convex.dev)-compatible backend server.

When working on Convex-compatible code (`packages/convex/`, `examples/convex/`, or any Convex API surface), **always read `docs/private/adapters/convex/ai-guidelines.md` first** for important guidelines on how to correctly use Convex APIs and patterns. The file contains rules that override what you may have learned about Convex from training data.
<!-- convex-ai-end -->

# Nimbus

## What Nimbus Is

Nimbus is a source-available, single-binary backend for apps and AI agents. It
speaks Convex, Firestore/Firebase, Cloud Functions, MongoDB, and DynamoDB
surfaces, but routes them through one engine, storage layer, runtime, and trust
model.

Server and adapter crates provide the protocol front doors. `nimbus-engine` owns
reads, writes, subscriptions, and scheduling. `nimbus-storage` owns durable
state. `nimbus-runtime` and `nimbus-bridge` execute V8 TypeScript. Sandbox and
node crates run agent and service workloads. `packages/*` contains SDKs,
compatibility packages, code generation, and the embedded UI.

The role of this file is to capture common mistakes and recurring confusion points for agents working in this repo.

If you hit a surprise that is likely to trip up another agent, tell the developer. Ask before adding a brief principle-first note here. If the guidance needs more than a few bullets, it probably belongs in `docs/*.md` or beside the code instead of here.

## Keep This File Small

- Put durable repo-wide rules, repeated traps, and verification commands here.
- Add new entries only with developer approval.
- Prefer principle-first notes over historical bug writeups.
- Link to canonical docs for architecture details instead of copying them here.
- Do not use this file as a changelog, ownership map, or deep implementation manual.

## Pre-Launch Status

**This project has NOT launched yet.** It has no production users or data to
migrate.

- **Prefer breaking changes.** Choose clean replacements over compatibility layers.
- **No backwards compatibility code.** Delete old behavior instead of deprecating it.
- **No migration shims.** Change the schema or API directly.
- **No feature flags for legacy behavior.** Remove the old path entirely.

If you find yourself writing compatibility code, stop and make the breaking change instead.

## Working Set

- Start with `README.md`, `ARCHITECTURE.md`, `docs/README.md`, and
  `docs/private/plans/README.md`.
- Use the active plan that owns the current slice. Prefer active
  plans over archived history.
- Treat the current git worktree plus the owning active plan as progress
  state. Resume `in_progress` work before starting a new roadmap item.
- Checkpoint plan state before stopping, handing off, or any likely context
  loss.
- Load one roadmap item at a time plus only the immediately relevant code,
  tests, and docs.

### Routing

Keep routing detail in the local indexes, not in this bootstrap file:

- Private control-plane routing: `docs/private/README.md`
- Active implementation order and plan promotion: `docs/private/plans/README.md`
- Architecture, trust boundaries, runtime, sandbox, and storage seams: `docs/private/architecture/README.md`
- Operating, CI, release, deploy, install, local-dev, and node runbooks: `docs/private/operating/README.md`
- Adapter-family routing: `docs/private/adapters/README.md`
- Public docs site work: `.agents/skills/docs/SKILL.md` and `docs/README.md`

Choose the active plan owner before editing. If no plan owns a concrete
implementation topic, promote exactly one owner plan and update the roadmap map
when the work came from that roadmap. Keep completed-plan evidence in the owning
plan or proof directory, not here.

### Workspace layout

The repo is a Rust workspace and npm monorepo. Names overlap. Identify the
intended item:

| Name | Path | What it is |
| --- | --- | --- |
| `nimbus` (facade crate) | `crates/nimbus/` | Re-exports public types for embedders |
| `nimbus-adapters` | `crates/nimbus-adapters/` | Optional adapter-family aggregation crate |
| `nimbus-auth` | `crates/nimbus-auth/` | Shared auth and identity primitives |
| `nimbus-bin` | `crates/nimbus-bin/` | CLI binary entry point |
| `nimbus-blob` | `crates/nimbus-blob/` | Content-addressed byte plane (`BlobStore`, Seam A) |
| `nimbus-core` | `crates/nimbus-core/` | Shared types and validation (zero I/O) |
| `nimbus-engine` | `crates/nimbus-engine/` | Central coordinator (`Engine`) |
| `nimbus-fs` | `crates/nimbus-fs/` | In-process isolate/WASI filesystem: mount table, `FsCaps`, backends (Seam C) |
| `nimbus-node` | `crates/nimbus-node/` | Host-local workload reconciliation and systemd integration |
| `nimbus-runtime` | `crates/nimbus-runtime/` | V8 execution (zero workspace deps) |
| `nimbus-s3` | `crates/nimbus-s3/` | S3 wire surface over the blob/metadata planes (Seam D) |
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
- Keep integration tests in `tests/`. Keep support helpers beside the owning
  crate unless many crates share them and justify `nimbus-testing`.

## Verification hazards

Read `docs/private/operating/verification.md` before changing verification
workflows or diagnosing skipped, misleading, or hung tests. The runbook owns
command truth, host activity checks, provider feature gates, bounded waits,
and sandboxed GitHub authentication diagnosis.

### Crate dependency rules

Do not violate these architecture invariants:

- **`nimbus-core` has zero I/O.** Types and validation only. No file reads, no network calls.
- **`nimbus-runtime` has zero workspace dependencies.** It defines the V8 surface and `HostBridge` trait. All Nimbus-specific integration lives in the server's bridge implementation.

### Mutation path

Every client document mutation uses one of exactly three engine-owned commit
paths. This rule covers HTTP, WebSocket, scheduled execution, and V8 execution:

1. The **queued journal path** batches client mutations. The per-tenant
   committer applies them in sequence order.
2. The **direct path** uses `apply_mutation_with_mode*`.
3. The **execution-unit path** uses `MutationExecutionUnit`. It commits one
   runtime function invocation as a single transaction.

Do not create a fourth client document mutation path.

That three-route invariant is not an inventory of every storage writer.
Schema, scheduler, trigger, and point-in-time-restore work can use internal
committer jobs. Object manifests use the raw `TenantPointWrite` seam on the
read executor. LibSQL replica refresh reconciles the local replica cache
through storage directly. Changes to SQLite writer ownership or concurrency
must account for these non-client writers. The three client routes are not the
only transactions that can open a writer.

Name all three paths in every audit. An audit of only
`apply_mutation_with_mode*` can miss defects in the other two paths. This
failure has occurred before. Ambiguous-outcome handling silently diverged, and
only the direct route escalated to crash-and-replay.

### Storage atomicity

Document write, supporting index effects, and commit log append must remain a
single storage transaction. Never commit a document without its index entries.
Never append a commit without the document write.

### Runtime bundles

A runtime bundle can carry a recorded SHA-256 provenance hash. Nimbus
recalculates and verifies that hash before every invocation. Nimbus rejects a
tampered or stale bundle.

A path-backed bundle without recorded provenance has no expected hash. Nimbus
admits it based on filesystem trust alone. See `verify_integrity`. Runtime host
operations such as `ctx.db.insert(...)` use the same `Engine` path as direct
HTTP calls. They have no bypass.

### Schema is optional

A table without a schema accepts any document. Setting a schema adds constraints but never removes the ability to write.

### JavaScript package naming

`packages/nimbus` is the JS SDK. `crates/nimbus` is the Rust facade. When
discussing "nimbus," identify the intended item.
- `packages/nimbus` is the canonical JS implementation. Keep `packages/convex`
  as a compatibility wrapper via thin adapters, aliases, or re-exports when
  behavior matches instead of copy-forwarding parallel logic.

## Verification Commands

- **Format check:** `cargo fmt --all --check`
- **Workspace check:** `make check`
- **Rust test suite:** `make test`
- **Rust lint:** `make clippy`
- **Dependency audit:** `make deny`
- **Third-party attribution gate (G4):** `make verify-third-party-attribution` (unit tests: `make verify-third-party-attribution-helper`)
- **Harness focused lanes:** `make verify-harness` or `make verify-harness SURFACE=runtime`
- **Harness nightly lanes:** `make verify-harness-nightly` or `make verify-harness-nightly SURFACE=server`
- **Harness repro:** `make verify-harness-repro SURFACE=runtime MODE=pr CASE=<case-id>`
- **JS typecheck:** `npm run typecheck`
- **JS tests:** `npm run test`
- **JS build:** `npm run build`
- **JS capability-boundary lint:** `npm run lint:capability-boundary`
- **Docs gates:** `bash scripts/check-docs.sh` and `bash scripts/verify-nimbus-docs-site.sh`
- **Required local CI gate:** `make ci`. It aliases `make ci-required`. Hosted
  CI still owns coverage uploads and scheduled or manual Node compatibility
  evidence.

See `docs/private/operating/local-dev.md` for the build contract. Node is a
development dependency for any Rust target that uses `nimbus-server`.

Prefer the `make` entrypoints above for long-running workspace verification.
The repository's single-flight guard wraps them. An accidental duplicate exits
quickly instead of starting another run. Use direct `cargo test ...` or `cargo
clippy ...` commands for intentional crate-level or test-level checks.

For focused Cargo commands, serialize runs against the repository's shared
`target/`. Later commands can then reuse the same artifacts. If Cargo reports
contention or a stale lock, wait for the active Cargo process to finish. Stop
the process only when evidence shows that it is stale or hung. Then rerun the
command on the shared target. Do not use alternate artifact directories as the
default recovery path.

Run `cargo fmt --all --check` and `make clippy` before opening a PR. For
PR-ready code changes, run `make ci` locally when feasible. It covers format,
Clippy, deny, Rust tests, the required verification harness, JavaScript checks,
and proof helpers.

Hosted CI remains the broader merge source of truth. It also gates runtime
pointer compression, the Bun runtime contract, external-provider tests, and
Node or FaaS compatibility. Other gates cover node D-Bus integration,
JavaScript capability boundaries, coverage, and scheduled Node compatibility.
