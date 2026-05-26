# Plan: Repo Architecture Quality Hardening

Canonical execution plan for a repository-wide cleanup wave focused on
architecture clarity, idiomatic Rust organization, stable seams, and enterprise
trust.

## Status

- **Status:** `active`
- **Primary owner:** this plan
- **Research:** `docs/plans/research/repo-architecture-quality-audit.md`
- **Guardrail ledger:** `docs/architecture/repo-architecture-quality-ledger.tsv`
- **Current posture references:** `README.md`, `ARCHITECTURE.md`,
  `docs/README.md`, `docs/plans/README.md`,
  `docs/architecture/testing/reliability-posture.md`,
  `docs/architecture/runtime/adapter-boundary.md`,
  `docs/architecture/server/auth-runtime-trust.md`, and
  `docs/tenant-isolation.md`

## Goal

Make Nimbus easier to inspect, extend, test, and trust by regrouping code
around the product seams that matter:

- admission and tenant isolation
- runtime/backend policy
- sandbox service lifecycle and egress enforcement
- system tenant evidence
- public server construction
- policy/provenance/audit evidence
- CLI/local development orchestration
- SDK/UI compatibility surfaces

This is a refactor and architecture-hardening plan. It should not add product
features unless a small guardrail, test, or documentation hook is necessary to
prove that a refactor preserved behavior.

## Non-Goals

- Do not split files only to reduce line counts.
- Do not move code across crate boundaries unless the ownership boundary gets
  clearer.
- Do not add a mandatory external policy engine such as OPA or Cedar.
- Do not change runtime, storage, sandbox, or policy semantics as a side effect
  of module moves.
- Do not touch generated files, proof screenshots, vendored compatibility
  corpora, or unrelated dirty worktree files.

## Design Principles

- **Concept ownership beats raw size.** A module may stay large when it owns one
  coherent concept and has focused tests. A module above 2,000 lines needs a
  split or an explicit exception.
- **Thin composition roots.** Root modules should name the concepts, re-export
  the intended public surface, and delegate behavior.
- **Typed boundaries first.** Parsing/defaulting, validation, admission,
  materialization, enforcement, and audit/export should have separate types or
  modules when they are separate product phases.
- **Single binary, explicit submodes.** Borrow OpenShell's clarity around
  supervisor and evidence boundaries without copying a multi-component product
  shape.
- **Kubernetes-style pipeline clarity.** Request/admission/evidence phases
  should be obvious and testable, with typed attribute/decision envelopes.
- **Pre-launch cleanup is allowed.** Prefer clean breaking changes over
  compatibility shims when public or internal seams need narrowing.

## Execution Plan

| Phase | Status | Goal | Verification |
| --- | --- | --- | --- |
| RAQ0 | `done` | Add a tracked architecture inventory and guardrail script that records large owned-source files, allowed generated/vendor/test-corpus exclusions, crate dependency invariants, and helper/common naming exceptions. | `./scripts/verify-repo-architecture-quality.sh` reports the current ledger, rejects unapproved owned-source files above the threshold, and confirms `nimbus-core` has no I/O imports while `nimbus-runtime` has no workspace dependencies. Passed on 2026-05-26. |
| RAQ1 | `done` | Split `crates/nimbus-server/src/tenant_isolation.rs` into concept-owned modules while keeping the root as a narrow composition/re-export module. Delivered homes: `authority`, `context`, `decision`, `identity`, `policy_input`, `runtime_admission`, and `tests`. | `cargo test -p nimbus-server tenant_isolation -- --nocapture` passed with 111 tests and the 21-scenario tenant isolation conformance suite. `cargo test -p nimbus-server tenant_isolation_drift -- --nocapture` passed with 2 tests. `cargo fmt --all --check` and `./scripts/verify-repo-architecture-quality.sh` passed. |
| RAQ2 | `done` | Split `crates/nimbus-server/src/system_tenant.rs` by ownership: system identity, schema definitions, projection observation, stable document keys, inventory seeds, and per-resource record writers. | `cargo test -p nimbus-server system_tenant -- --nocapture` passed with 13 tests. `cargo test -p nimbus-server service_manager -- --nocapture` passed with 14 tests. `./scripts/verify-repo-architecture-quality.sh` passed and no longer reports `system_tenant.rs` as a threshold file. |
| RAQ3 | `todo` | Make server construction and public exports boring. Promote one canonical options/builder surface for router/server construction, remove redundant pre-launch overloads, and narrow `crates/nimbus-server/src/lib.rs` re-exports into stable grouped modules. | Server/router tests pass, public docs use the canonical options path, and `rg "build_router_with_" crates docs` shows no new public overload growth. |
| RAQ4 | `todo` | Split `crates/nimbus-runtime/src/limits.rs` and the largest runtime bootstrap ops into stable runtime policy axes. Candidate homes: backend kind, trust tier, execution model, adapter diagnostics, grants, budgets, reset capabilities, routing affinity, and local op families. | Focused runtime tests for `runtime_capabilities`, `limits`, and `backends` pass, the Bun/JSC verification gate still proves backend axis separation, and `nimbus-runtime` still has zero workspace dependencies. |
| RAQ5 | `todo` | Split sandbox service management by lifecycle concept: image verification, activation, launch materialization, handle refresh, runtime service binding, and system-state recording. Keep `ServiceManager` as the public facade. | `cargo test -p nimbus-server service_manager -- --nocapture`, sandbox egress tests, and artifact provenance verification pass with no loss of fail-closed launch behavior. |
| RAQ6 | `todo` | Refine enterprise policy/provenance/audit organization. Keep typed Rust evaluation authoritative, but separate policy document/defaulting, validation, evaluation, explanation/diff, reload, external backend, prover, and export mapping code into findable homes. | Focused server tests for `operator_policy`, `audit_events`, and `image_admission`, focused `nimbus-bin` policy tests, and enterprise policy/provenance scripts pass. |
| RAQ7 | `todo` | Split CLI development and machine orchestration roots by workflow phase: dev plan resolution, env/deployment setup, dependency checks, readiness/browser launch, banner rendering, watch loop, and machine command handlers. | Focused `nimbus-bin` tests for `dev`, `machine`, and `policy` pass, and docs still describe the same `nimbus dev` and machine UX. |
| RAQ8 | `todo` | Clean up JS/SDK/UI compatibility seams. Replace avoidable `as unknown as` adapter casts with typed helpers or documented local exceptions, split very large self-test fixtures by capability, and move oversized UI route logic into concept-owned children. | `npm run typecheck`, `npm run test`, and targeted SDK/UI tests pass. Generated route files and Convex generated demo outputs remain excluded from source-size gates. |
| RAQ9 | `todo` | Add an enterprise evidence taxonomy pass across audit, error reasons, deny reasons, and operator diagnostics so policy/admission/runtime/sandbox/storage/HostBridge events use stable, searchable names without leaking secrets. | Golden audit/export/error fixtures prove stable event names, stable decision correlation fields, and redaction of tokens, bearer claims, secret handles, query strings, and credentials. |
| RAQ10 | `todo` | Final documentation and verification closure. Update architecture docs, active plan status, and verification runbooks to reflect the new module map and public seams. | `cargo fmt --all --check`, `make check`, `make clippy`, `npm run typecheck`, `npm run test`, `npm run build`, `./scripts/verify-repo-architecture-quality.sh`, enterprise policy/provenance/Bun gates, targeted docs reference checks for touched paths, and `git diff --check` pass or have explicitly recorded external/upstream failures. |

## Refactor Targets

### Server And Tenant Isolation

Target files:

- `crates/nimbus-server/src/tenant_isolation.rs`
- `crates/nimbus-server/src/tenant_isolation/operator_policy.rs`
- `crates/nimbus-server/src/tenant_isolation/artifact_provenance.rs`
- `crates/nimbus-server/src/tenant_isolation/audit_events.rs`
- `crates/nimbus-server/src/tenant_isolation_drift.rs`

Desired shape:

```text
tenant_isolation.rs
  composition root and public re-export surface
tenant_isolation/
  authority.rs
  context.rs
  decision.rs
  identity.rs
  policy_input.rs
  runtime_admission.rs
  tests.rs
  audit_events.rs
  artifact_provenance.rs
  image_admission.rs
  operator_policy/
    mod.rs
    document.rs
    defaults.rs
    validation.rs
    evaluation.rs
    explanation.rs
    reload.rs
    external.rs
    prove.rs
    egress.rs
```

The exact file names may change during implementation. The rule is that each
file owns a product concept and its tests, not merely a slice of old code.

### Runtime Policy

Target files:

- `crates/nimbus-runtime/src/limits.rs`
- `crates/nimbus-runtime/src/runtime_capabilities.rs`
- `crates/nimbus-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js`
- `crates/nimbus-runtime/src/runtime/bootstrap/ops/runtime_local.rs`
- `crates/nimbus-runtime/src/backends/bun_jsc/manifest.rs`

Desired shape:

- backend axes cannot cross-contaminate V8/Deno/Node and Bun/JSC policy
- adapter diagnostics are a stable API contract
- memory policy distinguishes V8 heap limits from Bun/JSC outer quota
- resolver/package policy lives near backend selection, not in server shims
- runtime bootstrap op families are readable and testable by capability

### Adapter Surfaces

Target files:

- `crates/nimbus-server/src/adapters/cloud_functions/http.rs`
- `crates/nimbus-server/src/adapters/cloud_functions/execution.rs`

Desired shape:

- HTTP request parsing, signature binding, execution handoff, and response
  mapping have separate homes
- adapter code remains thin at the request boundary and calls concept-owned
  modules for compatibility behavior
- Cloud Functions tests keep proving externally visible compatibility rather
  than internal file layout

### Sandbox And Service Lifecycle

Target files:

- `crates/nimbus-server/src/service_manager.rs`
- `crates/nimbus-sandbox/src/egress.rs`
- `crates/nimbus-sandbox/src/egress_proxy.rs`
- `crates/nimbus-sandbox/src/backends/oci/network.rs`
- `crates/nimbus-sandbox/src/backends/oci/builder.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime.rs`

Desired shape:

- `ServiceManager` remains the facade
- admission, verification, launch materialization, runtime binding, reload, and
  system recording have separate ownership
- sandbox supervisor/proxy contract naming remains consistent with enterprise
  policy docs
- backend-specific files stay backend-specific instead of absorbing shared
  policy logic

### Public API And CLI

Target files:

- `crates/nimbus-server/src/lib.rs`
- `crates/nimbus-server/src/router.rs`
- `crates/nimbus-bin/src/dev.rs`
- `crates/nimbus-bin/src/machine/handlers.rs`

Desired shape:

- one documented server/router options path
- grouped public enterprise types instead of broad root-level re-exports
- `nimbus dev` internals follow workflow phases
- command handlers stay thin and call owned workflow modules

### JavaScript, SDK, And UI

Target files:

- `packages/convex/src/server.ts`
- `packages/firebase/src/selftest.mjs`
- `packages/firebase/src/firestore.ts`
- `packages/codegen/src/cloud_functions/runtime_sources.mjs`
- `packages/nimbus-ui/src/routes/developer/storage_.$table.tsx`

Desired shape:

- compatibility wrappers use typed adapters where possible
- unavoidable casts are local, documented, and covered by type tests
- self-tests are grouped by capability
- UI route files delegate dense table/storage behavior to owned components or
  hooks without changing visible UX

## Completion Gate

The plan is complete when:

- all owned-source files above 2,000 lines are split or have an explicit
  documented exception in this plan
- files between 1,500 and 1,999 lines have either a split or a documented
  ownership justification
- the architecture guardrail script is tracked and used by the final gate
- public server construction has one canonical documented path
- runtime backend axes remain separated by tests
- policy/admission/audit/provenance seams are grouped by product concept
- generated/vendor/proof/test-corpus exclusions are explicit
- docs and active plan status match the code
- final verification commands in RAQ10 pass or record a concrete external
  failure that is not caused by Nimbus changes

## Progress Ledger

| Date | Phase | Status | Evidence |
| --- | --- | --- | --- |
| 2026-05-26 | RAQ0 | `done` | Added `docs/architecture/repo-architecture-quality-ledger.tsv` and `scripts/verify-repo-architecture-quality.sh`. The baseline ledger recorded 10 owned-source files at or above AGENTS.md thresholds, generated/vendor/test-corpus/proof exclusions, and helper/common naming exceptions. `./scripts/verify-repo-architecture-quality.sh` passed: large-file ledger matched source, helper/common naming exceptions were tracked, `nimbus-core` zero-I/O scan passed, and `nimbus-runtime` zero-workspace-dependency scan passed. |
| 2026-05-26 | RAQ1 | `done` | Split `crates/nimbus-server/src/tenant_isolation.rs` from a 2,596-line root into a 79-line composition root plus concept-owned `authority`, `context`, `decision`, `identity`, `policy_input`, `runtime_admission`, and `tests` modules. Removed the tenant-isolation root from the large-file ledger. `cargo test -p nimbus-server tenant_isolation -- --nocapture` passed with 111 tests and the 21-scenario conformance suite. `cargo test -p nimbus-server tenant_isolation_drift -- --nocapture` passed with 2 tests. `./scripts/verify-repo-architecture-quality.sh` passed and now reports 9 threshold files remaining for later phases. |
| 2026-05-26 | RAQ2 | `done` | Split `crates/nimbus-server/src/system_tenant.rs` from a 2,116-line root into a 36-line composition root plus concept-owned `identity`, `schema`, `projection`, `keys`, `inventory`, `records`, and `tests` modules. Removed the system-tenant root from the large-file ledger and hardened the guardrail script to preload ledger values instead of repeatedly spawning ledger readers. `cargo test -p nimbus-server system_tenant -- --nocapture` passed with 13 tests, `cargo test -p nimbus-server service_manager -- --nocapture` passed with 14 tests, and `./scripts/verify-repo-architecture-quality.sh` passed with 8 threshold files remaining. |

## Suggested Goal Prompt

```text
/goal Complete docs/plans/repo-architecture-quality-hardening-plan.md from RAQ0 through RAQ10 autonomously. Use docs/plans/research/repo-architecture-quality-audit.md as the research baseline. Preserve Nimbus crate invariants: nimbus-core remains zero I/O and nimbus-runtime remains zero workspace dependencies. Do not split code mechanically; regroup code only by coherent product ownership. Do not touch unrelated dirty worktree files, generated outputs, proof screenshots, vendored compatibility corpora, or user changes. Update the phase ledger as each RAQ phase completes, commit clean checkpoints, and stop only when the completion gate is satisfied. Verifiable success criteria: the architecture guardrail script exists and passes; all owned-source files above AGENTS.md thresholds are split or explicitly justified; public server construction uses one canonical documented options path; runtime backend axes remain separated by tests; policy/admission/audit/provenance seams are grouped by concept; npm and Rust focused tests for touched areas pass; final RAQ10 commands pass or record concrete external failures not caused by Nimbus changes; docs/plans/README.md and architecture docs match the implemented state.
```
