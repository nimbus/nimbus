# Codegen And Facade Hardening Plan

Status: completed

This plan owns the remaining architecture-review follow-up for:

- `packages/codegen` compile-time planning and source loading
- the public Rust facade in `crates/neovex`
- workspace-level JavaScript typecheck and build script curation

It is the active owner for the remaining items from the April 2026 review that
were left open after the localhost/server security and runtime/provider
boundary waves landed.

## Reviewed Against

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/reference/convex-ai-guidelines.md`
- `docs/reference/cli.md`
- `docs/convex/compatibility.md`
- `docs/reference/reliability-posture.md`
- `docs/reference/ci-failure-investigation.md`
- `packages/codegen/package.json`
- `packages/codegen/src/planner/evaluate.mjs`
- `packages/codegen/src/parser/server_definition.mjs`
- `packages/codegen/src/schema.mjs`
- `packages/codegen/src/emit/runtime_bundle_preamble.mjs`
- `packages/convex/package.json`
- `packages/neovex/package.json`
- `package.json`
- `crates/neovex/src/lib.rs`

## Purpose

The remaining open architecture-review items are no longer about server
security or runtime/provider boundaries. They are now concentrated in one
tooling and public-surface slice:

- codegen still evaluates supported source text through `new Function` in a
  few compile-time planning paths
- the facade crate still re-exports a very broad mix of high-level embedder
  APIs, low-level server helpers, and rapidly changing implementation-owned
  types
- the JS workspace still lacks one clear, repo-owned typecheck/build contract
  for `packages/codegen`, `packages/convex`, and `packages/neovex`

This plan exists to finish that wave as one deliberate ownership pass instead
of another pile of disconnected cleanup commits.

## Current Verified State

- `packages/codegen` already uses TypeScript parsing for some validation, but
  compile-time planning still executes user-authored source text through
  `new Function` in:
  - `packages/codegen/src/planner/evaluate.mjs`
  - `packages/codegen/src/parser/server_definition.mjs`
  - `packages/codegen/src/schema.mjs`
- `packages/codegen/src/emit/runtime_bundle_preamble.mjs` still uses
  `new Function`, but that is part of emitted runtime-bundle execution glue
  rather than compile-time planning. It is not in scope for this plan unless a
  later item explicitly promotes runtime-bundle loader changes.
- `crates/neovex/src/lib.rs` currently re-exports a broad cross-section of
  engine, runtime, sandbox, server, storage, local-server security, and router
  builder helpers rather than a curated embedder-first facade.
- the root workspace now owns canonical `npm run typecheck`, `npm run test`,
  and `npm run build` entrypoints, while package-owned scripts provide the
  package-specific verification lanes those root commands fan out to
- `docs/plans/archive/codegen-cli-plan.md` and
  `docs/plans/archive/neovex-source-root-plan.md` are historical records only;
  neither owns the remaining architecture follow-up

## Non-Goals

This plan does not cover:

- new Convex compatibility feature expansion beyond what is needed to keep the
  existing supported subset working
- runtime/provider boundary work already closed by
  `docs/plans/runtime-provider-boundary-hardening-plan.md`
- localhost/server security work already closed by
  `docs/plans/localhost-server-security-plan.md`
- package or crate rename work from the pending `nimbus` plans
- runtime-bundle execution-model redesign beyond what is strictly required to
  remove codegen-time `new Function` planning hazards

## Success Criteria

This plan is complete only when:

1. codegen compile-time planning no longer evaluates raw user source through
   `new Function`
2. supported planning and schema/server-definition loading flow through a
   typed AST-based parser or a similarly constrained evaluator with explicit
   ownership and documented limits
3. `crates/neovex/src/lib.rs` is narrowed to an embedder-oriented public
   surface instead of re-exporting low-level server/local-admin/security
   helpers by default
4. low-level callers can still reach owning-crate APIs directly without a
   backwards-compatibility shim wave
5. the JS workspace has a canonical repo-owned script contract for build,
   test, and typecheck lanes, and the docs point contributors at that contract
6. focused verification plus the relevant workspace-level checks are recorded
   in this plan as each item lands

## Roadmap Status Ledger

| Item | Status | Notes |
| --- | --- | --- |
| CF1 | `done` | Planner-time resolver evaluation now lowers through a planner-owned TypeScript AST interpreter instead of `new Function`, while preserving the existing supported compile-time subset and runtime fallback behavior |
| CF2 | `done` | Schema loading and server-definition validator parsing now use the same AST-owned compile-time interpreter rather than `new Function`, keeping validator/schema loading inside an explicit supported subset |
| CF3 | `done` | Curated `crates/neovex/src/lib.rs` into an embedder-first facade by removing low-level localhost-security and router-builder re-exports; the CLI now imports those owning-crate surfaces from `neovex-server` directly |
| CF4 | `done` | Added canonical root `npm run typecheck`, `npm run test`, and `npm run build` entrypoints, gave the typed JS packages dedicated `typecheck` scripts, and updated contributor docs to point at the settled contract |

## Implementation Checkpoints

| Checkpoint | Status |
| --- | --- |
| Active plan promotion and ownership handoff are recorded in docs and `AGENTS.md` | `done` |
| CF1 focused ownership and verification are recorded | `done` |
| CF2 focused ownership and verification are recorded | `done` |
| CF3 focused ownership and verification are recorded | `done` |
| CF4 focused ownership and verification are recorded | `done` |

## Item Guidance

### CF1: Planner Evaluation

Goal: `evaluateResolverPlan` stops executing resolver text through
`new Function` and instead lowers a documented supported subset from the
TypeScript AST into explicit planner-owned operations.

Expected outcome:

- `packages/codegen/src/planner/evaluate.mjs` owns AST-first parsing
- planner evaluation only accepts the supported compile-time subset
- unsupported syntax fails clearly and deterministically
- planner helper ownership remains close to `packages/codegen/src/planner/`

Focused verification:

- `npm run test --workspace @neovex/codegen`
- `npm run test --workspace convex`

### CF2: Schema And Server Definition Loading

Goal: schema loading and server-definition discovery stop evaluating raw source
through `new Function` in compile-time planning paths.

Expected outcome:

- `packages/codegen/src/schema.mjs` uses AST-owned schema extraction or a
  similarly constrained module-loading seam
- `packages/codegen/src/parser/server_definition.mjs` uses AST-owned discovery
  rather than text evaluation
- supported limits are explicit in code and docs

Focused verification:

- `npm run test --workspace @neovex/codegen`
- `npm run test --workspace convex`
- `npm run build --workspace convex`

### CF3: Rust Facade Curation

Goal: `crates/neovex/src/lib.rs` becomes a stable embedder-first facade rather
than a broad re-export bucket.

Expected outcome:

- the facade surface groups around stable embedder needs
- low-level local-server security and router-construction helpers stop being
  re-exported by default unless they are part of the intended public facade
- docs explain the owning crate to use when a surface is intentionally not on
  the facade

Focused verification:

- `cargo check -p neovex`
- `cargo clippy -p neovex --all-targets -- -D warnings`

### CF4: JS Script Contract

Goal: the repo has one canonical JS build/test/typecheck contract and the
package scripts align with it.

Expected outcome:

- package-level build/test/typecheck scripts are explicit where needed
- the workspace root exposes canonical JS verification entrypoints
- `docs/reference/cli.md`, `docs/convex/compatibility.md`, or `README.md`
  point contributors at the settled contract where relevant

Focused verification:

- `npm run typecheck`
- `npm run test`
- `npm run build`
- any new canonical typecheck command added by this item

## Verification Contract

Each roadmap item must record its focused verification before it is marked
`done`. Before this entire plan is closed, run and record:

- `cargo fmt --all --check`
- `make check`
- `make clippy`
- `make test`
- `npm run typecheck`
- `npm run test`
- `npm run build`

## Execution Log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-04-23 | Plan promotion | `done` | Promoted this plan as the active owner for the remaining architecture-review follow-up after localhost/server security and runtime/provider boundary hardening landed. Corrected the plan index so `install-script-plan.md` remains listed as active, and updated `AGENTS.md` plus `docs/plans/README.md` so future CLI/codegen/facade work starts from this plan instead of archived history or floating review notes. |
| 2026-04-23 | CF1 | `done` | Replaced planner-time `new Function` evaluation in `packages/codegen/src/planner/evaluate.mjs` with a planner-owned TypeScript AST interpreter (now shared from `packages/codegen/src/compile_time_interpreter.mjs`). The interpreter now owns resolver parameter binding, block statements, local variables, object/array literals, method calls, query-builder callbacks, and request/response compile helpers for the supported subset, while keeping existing unsafe-identifier rejection and runtime-only fallback behavior. Tightened the compile-time request proxy to return synchronous request markers and added a focused selftest for a compileable block-body server handler. Verification: `npm run test --workspace @neovex/codegen`; `npm run test --workspace convex`. Next: start CF2 and remove `new Function` from `packages/codegen/src/schema.mjs` and `packages/codegen/src/parser/server_definition.mjs`. |
| 2026-04-23 | CF2 | `done` | Reused the same AST-owned compile-time interpreter for schema loading and server-definition validator parsing. `packages/codegen/src/schema.mjs` now evaluates `defineSchema(...)` through interpreter-owned bindings for `defineSchema`, `defineTable`, and `v`, and `packages/codegen/src/parser/server_definition.mjs` now evaluates `args` and `returns` validators through the same constrained expression path instead of `new Function`. The shared interpreter now lives at `packages/codegen/src/compile_time_interpreter.mjs` so later codegen work can reuse one explicit compile-time evaluation surface instead of multiplying evaluators. Verification: `npm run test --workspace @neovex/codegen`; `npm run test --workspace convex`; `npm run build --workspace convex`. Next: start CF3 and audit `crates/neovex/src/lib.rs` re-export consumers before narrowing the facade. |
| 2026-04-23 | CF3 | `done` | Narrowed `crates/neovex/src/lib.rs` to keep the embedder-facing server surface while removing low-level localhost-security records, token and discovery helpers, and router-builder overloads from the top-level facade. `neovex-bin` now depends on `neovex-server` directly for those owning-crate APIs, which keeps the CLI working without keeping implementation-heavy server internals on the public facade. Updated `ARCHITECTURE.md` to document that router-construction and localhost-security helpers stay owned by `neovex-server`. Verification: `cargo check -p neovex`; `cargo check -p neovex-bin`; `cargo clippy -p neovex --all-targets -- -D warnings`; `cargo clippy -p neovex-bin --all-targets -- -D warnings`. Next: start CF4 and settle the canonical JS build/test/typecheck contract. |
| 2026-04-23 | CF4 | `done` | Added canonical root `npm run typecheck`, `npm run test`, and `npm run build` entrypoints in the workspace package, kept the older `convex:*` aliases as thin forwards, and added dedicated `typecheck` scripts for the typed `convex` and `neovex` packages by teaching their selftests a `--typecheck-only` mode. Updated `AGENTS.md`, `docs/reference/cli.md`, and `docs/convex/compatibility.md` so contributors now see the same repo-owned JS verification contract in the docs and the agent entrypoint. Verification: `npm run typecheck`; `npm run test`; `npm run build`. Next: run the plan closeout verification bundle and retire this workstream cleanly. |
| 2026-04-24 | Closeout | `done` | Ran the final closeout verification sweep, updated the repo entrypoints so this plan is no longer treated as active, and archived the control plane as a completed execution record. The first sandboxed `make ci` attempt failed for an environment-only reason because `cargo deny` could not lock `/Users/jack/.cargo/advisory-dbs/db.lock` on a read-only path; the unrestricted retry passed, confirming that the remaining `cargo deny` output is limited to pre-existing duplicate-crate warnings rather than a new verification failure. Verification: `cargo fmt --all --check`; `cargo check --workspace`; `make check`; `make test`; `make clippy`; `npm run typecheck`; `npm run test`; `npm run build`; `make ci` (passed on unrestricted retry after the sandbox-only advisory-db lock failure). Next: promote a new active plan before the next CLI/codegen/facade architecture wave. |
| 2026-04-24 | Follow-up hardening | `done` | Moved compile-time unsafe-reference and prototype-constructor property guards into the shared `packages/codegen/src/compile_time_interpreter.mjs` path so schema and server-definition validator expressions receive the same protection as resolver planning. Added runtime property-read denial for computed keys such as `"con" + "structor"`, adversarial schema/args/returns fixtures, and a `@neovex/codegen` JS parser plus codegen-boundary guardrail lane wired into root `npm run typecheck`. Documented that generated runtime bundles still use `new Function` inside the Neovex V8 runtime boundary, while compile-time planning does not execute user source in Node. Verification: `npm run typecheck --workspace @neovex/codegen`; `npm run test --workspace @neovex/codegen`; `npm run typecheck`; `npm run test`; `npm run build`; direct schema constructor-escape probe rejected. |
