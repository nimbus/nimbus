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
| RAQ3 | `done` | Make server construction and public exports boring. Promoted the canonical `RouterOptions::new(service)` + `build_router(options)` router path, moved listener construction into `construction.rs`, and replaced the serve overload family with `ServeOptions::new(service)` + `serve(listener, options)`. | `cargo check -p nimbus-server -p nimbus-bin -p nimbus`, `cargo test -p nimbus-server --lib --no-run`, `cargo test -p nimbus-server --tests --no-run`, `cargo test -p nimbus-bin --bin nimbus --no-run`, focused server/bin behavior tests, `cargo fmt --all --check`, `./scripts/verify-repo-architecture-quality.sh`, legacy overload grep, and `git diff --check` passed. |
| RAQ4 | `done` | Split `crates/nimbus-runtime/src/limits.rs` and the largest runtime bootstrap ops into stable runtime policy axes. Delivered homes: runtime backend adapter diagnostics, backend axes, grants, policy wrapper, resource budgets, tests, and runtime-local op families for bootstrap, env/shared-env, filesystem, require resolution, support, and payload/descriptor types. Kept `node22_runtime_bootstrap.js` as a documented hard-threshold exception because its Deno extension import order is bootstrap-sensitive. | `cargo check -p nimbus-runtime`, `cargo test -p nimbus-runtime limits -- --nocapture` (12 passed), `cargo test -p nimbus-runtime runtime_capabilities -- --nocapture` (14 passed), `cargo test -p nimbus-runtime backends -- --nocapture` (10 passed), `bash scripts/verify-bun-jsc-runtime-contract.sh` outside the local sandbox (11 runtime policy, 10 Bun/JSC backend, 15 registry, 2 diagnostics, 1 tenant-admission, and 5 UI diagnostics tests passed), `cargo fmt --all --check`, and `./scripts/verify-repo-architecture-quality.sh` passed. |
| RAQ5 | `done` | Split sandbox service management by lifecycle concept: image verification, activation, launch materialization, handle refresh, runtime service binding, and system-state recording. Kept `SandboxServiceManager` as the public facade. | `cargo check -p nimbus-server`, `cargo test -p nimbus-server service_manager -- --nocapture` (14 passed), `cargo fmt --all --check`, `./scripts/verify-repo-architecture-quality.sh`, and `git diff --check -- crates/nimbus-server/src/service_manager.rs crates/nimbus-server/src/service_manager` passed. The service manager root is now 1,136 lines and below the review threshold. |
| RAQ6 | `done` | Refined enterprise policy/provenance/audit and Cloud Functions HTTP organization. Kept typed Rust evaluation authoritative, kept policy document/defaulting in the operator policy root, moved validation/evaluation/explanation/diff/formatting into concept-owned modules, preserved the existing reload/external/prover/egress-draft homes, and split HTTP request parsing, execution handoff, tenant resolution, and response mapping into findable homes. | `cargo check -p nimbus-server`, focused `operator_policy` and `cloud_functions_http` tests, `bash scripts/verify-enterprise-policy-egress.sh`, `bash scripts/verify-artifact-provenance.sh`, `cargo fmt --all --check`, `./scripts/verify-repo-architecture-quality.sh`, and focused `git diff --check` passed. |
| RAQ7 | `done` | Split CLI development and machine orchestration roots by workflow phase. Delivered homes for dev adapter detection, plan resolution, `.env.local` deployment binding, readiness/browser launch, banner rendering, watch activation, machine OS lifecycle, and SSH/SCP transfer parsing while preserving the public CLI UX. | `cargo check -p nimbus-bin`, `cargo test -p nimbus-bin dev -- --nocapture` (50 passed), `cargo test -p nimbus-bin machine -- --nocapture` (185 passed), `cargo test -p nimbus-bin policy -- --nocapture` (10 passed), `cargo fmt --all --check`, and `./scripts/verify-repo-architecture-quality.sh` passed. |
| RAQ8 | `done` | Cleaned up JS/SDK/UI compatibility seams. Split the Firebase selftest by compatibility capability, centralized the Convex public-type bridge, replaced route-test reach-through casts with a typed helper, and moved fetch/window mocks onto typed or runtime-checked test helpers. No UI route crossed the current size threshold, so route-component extraction is deferred until a route owns a larger coherent product concept. | `npm run typecheck --workspace @nimbus/firebase`, `npm run test --workspace @nimbus/firebase`, `npm run build --workspace @nimbus/firebase`, `npm run typecheck --workspace convex`, `npm run test --workspace convex`, `npm run build --workspace convex`, `npm run typecheck --workspace nimbus-ui`, `npm run test --workspace nimbus-ui`, `npm run build --workspace nimbus-ui`, and `npm run typecheck --workspace nimbus-html` passed. Generated route files and Convex generated demo outputs remain excluded from source-size gates and were not staged. |
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
  operator_policy.rs (document/defaulting facade)
  operator_policy/
    diff.rs
    draft.rs
    egress.rs
    evaluation.rs
    validation.rs
    explanation.rs
    external.rs
    formatting.rs
    prove.rs
    reload.rs
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

- HTTP request parsing, tenant binding, execution handoff, callable dispatch,
  and response mapping have separate homes
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
- `packages/firebase/src/selftest/*.mjs`
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
| 2026-05-26 | RAQ3 | `done` | Replaced the public router overload family with `RouterOptions::new(service)` plus `build_router(options)`, moved listener-owned serving into `construction.rs` behind `ServeOptions::new(service)` plus `serve(listener, options)`, and updated `nimbus-server`, the `nimbus` facade, CLI tests, server unit tests, and reactive-loop integration tests to the canonical construction path. Verification passed: `cargo check -p nimbus-server -p nimbus-bin -p nimbus`; `cargo test -p nimbus-server --lib --no-run`; `cargo test -p nimbus-server --tests --no-run`; `cargo test -p nimbus-bin --bin nimbus --no-run`; `cargo test -p nimbus-server serve_loads_embedded_system_convex_registry_by_default --lib -- --nocapture` (1 passed); `cargo test -p nimbus-server local_admin --lib -- --nocapture` (12 passed); `cargo test -p nimbus-server --test reactive_loop -- --nocapture` (32 passed); `cargo test -p nimbus-bin --bin nimbus ui_command_resolves_live_discovery_record -- --nocapture` (1 passed); `cargo test -p nimbus-server firebase_rest_and_cors --lib -- --nocapture` (3 passed); `cargo test -p nimbus-server services --lib -- --nocapture` (7 passed, 1 ignored Linux/KVM smoke); `cargo fmt --all --check`; `./scripts/verify-repo-architecture-quality.sh`; active legacy-overload grep; and `git diff --check`. |
| 2026-05-26 | RAQ4 | `done` | Split `crates/nimbus-runtime/src/limits.rs` from a 1,713-line policy root into a narrow composition root plus concept-owned `adapter`, `axes`, `grants`, `policy`, `resources`, and `tests` modules. Split `crates/nimbus-runtime/src/runtime/bootstrap/ops/runtime_local.rs` from a 1,636-line op root into a narrow composition root plus `bootstrap`, `env`, `fs`, `require`, `support`, and `types` modules. Removed those two roots from the large-file ledger. Kept `node22_runtime_bootstrap.js` as a justified hard-threshold exception after a live split experiment showed the bootstrap is too Deno extension-order-sensitive for child extension modules in this wave. Verification passed: `cargo check -p nimbus-runtime`; `cargo test -p nimbus-runtime limits -- --nocapture` (12 passed); `cargo test -p nimbus-runtime runtime_capabilities -- --nocapture` (14 passed); `cargo test -p nimbus-runtime backends -- --nocapture` (10 passed); `bash scripts/verify-bun-jsc-runtime-contract.sh` outside the local sandbox (11 runtime policy, 10 Bun/JSC backend, 15 registry, 2 diagnostics, 1 tenant-admission, and 5 UI diagnostics tests passed); `cargo fmt --all --check`; and `./scripts/verify-repo-architecture-quality.sh`. Additional live Node22 bootstrap probe `cargo test -p nimbus-runtime node22_target_exposes_minimal_node_globals -- --nocapture` still fails with pre-existing Deno extension-script error `ext:deno_crypto/00_crypto.js` while `node22_runtime_bootstrap.js` and `node22_runtime.rs` are unchanged from `HEAD`; track that under Node-compat greening, not this structural split. |
| 2026-05-26 | RAQ5 | `done` | Split `crates/nimbus-server/src/service_manager.rs` from a 1,676-line lifecycle root into a 1,136-line public facade plus concept-owned child modules for `activation`, `catalog`, `handles`, `launch`, `registry`, `system_state`, `types`, and `verification`. The public `SandboxServiceManager` type and caller API stayed stable while service activation, egress reload, launch admission, handle refresh, runtime binding, tenant teardown, image verification, and system-state recording moved to their owning modules. Verification passed: `cargo check -p nimbus-server`; `cargo test -p nimbus-server service_manager -- --nocapture` (14 passed, plus integration binaries filtered to 0); `cargo fmt --all --check`; `git diff --check -- crates/nimbus-server/src/service_manager.rs crates/nimbus-server/src/service_manager`; and `./scripts/verify-repo-architecture-quality.sh`, which no longer reports `service_manager.rs` as an owned-source threshold file. |
| 2026-05-26 | RAQ6 | `done` | Split `crates/nimbus-server/src/tenant_isolation/operator_policy.rs` from a 1,629-line policy root into a 510-line document/defaulting facade plus concept-owned child modules for `diff`, `evaluation`, `explanation`, `formatting`, and `validation`, while preserving the existing `draft`, `egress`, `external`, `prove`, and `reload` homes. Split `crates/nimbus-server/src/adapters/cloud_functions/http.rs` from a 1,551-line HTTP adapter root into a 1,270-line route/test facade plus `request`, `invocation`, `response`, and `tenant` modules beside the existing `callable` module. Artifact provenance and tenant-isolation audit code were already below the review threshold and stayed behavior-owned; their gates remained part of verification. Verification passed: `cargo check -p nimbus-server`; `cargo test -p nimbus-server operator_policy -- --nocapture` (24 passed); `cargo test -p nimbus-server cloud_functions_http -- --nocapture` (4 passed); `bash scripts/verify-enterprise-policy-egress.sh` outside the local sandbox after a sandbox-only socket bind denial (8 gates passed, including 24 operator-policy, 10 policy CLI, 3 Compose egress, 14 service-manager, 41 sandbox egress, 16 egress-proxy, 4 audit export, and 2 drift tests); `bash scripts/verify-artifact-provenance.sh` (5 gates passed, including 41 artifact-provenance, 1 runtime invocation provenance, 14 image admission, 1 SBOM hook, and 6 production Compose admission tests); `cargo fmt --all --check`; and `./scripts/verify-repo-architecture-quality.sh`, which no longer reports the operator policy or Cloud Functions HTTP roots as owned-source threshold files. |
| 2026-05-26 | RAQ7 | `done` | Split `crates/nimbus-bin/src/dev.rs` from a 1,957-line dev command root into a 1,228-line CLI/test facade plus `dev/adapter.rs`, `dev/banner.rs`, `dev/env_file.rs`, `dev/launch.rs`, `dev/plan.rs`, and `dev/watch.rs`. Split `crates/nimbus-bin/src/machine/handlers.rs` from a 1,378-line command handler root into a 661-line dispatcher plus `machine/handlers/os.rs` for machine OS lifecycle and `machine/handlers/transfer.rs` for SSH/SCP target parsing. The machine manager helper naming exception was not touched and remains queued for the owning manager phase. Verification passed: `cargo check -p nimbus-bin`; `cargo test -p nimbus-bin dev -- --nocapture` (50 passed); `cargo test -p nimbus-bin machine -- --nocapture` (185 passed); `cargo test -p nimbus-bin policy -- --nocapture` (10 passed); `cargo fmt --all --check`; and `./scripts/verify-repo-architecture-quality.sh`, which no longer reports `crates/nimbus-bin/src/dev.rs` as an owned-source threshold file. |
| 2026-05-26 | RAQ8 | `done` | Split `packages/firebase/src/selftest.mjs` from a 3,925-line compatibility selftest into a 47-line command root plus capability-owned modules for package exports/build/typecheck, runtime lifecycle, REST/query/converter behavior, gRPC-Web/protobuf behavior, watch/listen behavior, smoke flow, and shared support. Replaced avoidable `as unknown as` casts across the touched JS/SDK/UI surface: Firebase query-source converter bridging is now type-discriminated, Convex's compatibility registration bridge is centralized in one documented helper, route tests use `src/test/route-internals.ts`, fetch mocks use `vi.stubGlobal`, and desktop/perf/test toast mocks use typed runtime objects. `rg -n "as unknown as|unknown as" packages/firebase packages/nimbus-ui packages/convex demos/nimbus/html/src/main.ts` now reports only the intentional `query-entry.spec.ts` comment saying there is no escape hatch. Verification passed: `npm run typecheck --workspace @nimbus/firebase`; `npm run test --workspace @nimbus/firebase`; `npm run build --workspace @nimbus/firebase`; `npm run typecheck --workspace convex`; `npm run test --workspace convex`; `npm run build --workspace convex`; `npm run typecheck --workspace nimbus-ui`; `npm run test --workspace nimbus-ui` (42 files, 278 tests); `npm run build --workspace nimbus-ui`; `npm run typecheck --workspace nimbus-html`; and `./scripts/verify-repo-architecture-quality.sh`, which no longer reports the Firebase selftest as an owned-source threshold file. |

## Suggested Goal Prompt

```text
/goal Complete docs/plans/repo-architecture-quality-hardening-plan.md from RAQ0 through RAQ10 autonomously. Use docs/plans/research/repo-architecture-quality-audit.md as the research baseline. Preserve Nimbus crate invariants: nimbus-core remains zero I/O and nimbus-runtime remains zero workspace dependencies. Do not split code mechanically; regroup code only by coherent product ownership. Do not touch unrelated dirty worktree files, generated outputs, proof screenshots, vendored compatibility corpora, or user changes. Update the phase ledger as each RAQ phase completes, commit clean checkpoints, and stop only when the completion gate is satisfied. Verifiable success criteria: the architecture guardrail script exists and passes; all owned-source files above AGENTS.md thresholds are split or explicitly justified; public server construction uses one canonical documented options path; runtime backend axes remain separated by tests; policy/admission/audit/provenance seams are grouped by concept; npm and Rust focused tests for touched areas pass; final RAQ10 commands pass or record concrete external failures not caused by Nimbus changes; docs/plans/README.md and architecture docs match the implemented state.
```
