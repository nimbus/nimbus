# Repo Architecture Quality Audit

Date: 2026-05-26

This note records the architecture and code-organization review that led to
`docs/plans/archive/repo-architecture-quality-hardening-plan.md`. It is intentionally
about ownership seams, testability, and enterprise trust. It is not a mandate to
split files mechanically.

## Sources Reviewed

Nimbus local sources:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `docs/architecture/README.md`
- `docs/architecture/testing/reliability-posture.md`
- `docs/architecture/runtime/adapter-boundary.md`
- `docs/architecture/server/auth-runtime-trust.md`
- `docs/tenant-isolation.md`
- `crates/nimbus-server/src/lib.rs`
- `crates/nimbus-server/src/router.rs`
- `crates/nimbus-server/src/tenant_isolation.rs`
- `crates/nimbus-server/src/system_tenant.rs`
- `crates/nimbus-server/src/service_manager.rs`
- `crates/nimbus-server/src/tenant_isolation/operator_policy.rs`
- `crates/nimbus-runtime/src/limits.rs`
- `crates/nimbus-runtime/src/runtime/bootstrap/ops/runtime_local.rs`
- `crates/nimbus-bin/src/dev.rs`

Comparable local sources:

- `/Users/jack/src/github.com/NVIDIA/OpenShell/README.md`
- `/Users/jack/src/github.com/NVIDIA/OpenShell/architecture/README.md`
- `/Users/jack/src/github.com/NVIDIA/OpenShell/architecture/security-policy.md`
- `/Users/jack/src/github.com/NVIDIA/OpenShell/crates/openshell-policy/src/lib.rs`
- `/Users/jack/src/github.com/NVIDIA/OpenShell/crates/openshell-prover/src/lib.rs`
- `/Users/jack/src/github.com/NVIDIA/OpenShell/crates/openshell-ocsf/src/lib.rs`
- `/Users/jack/src/github.com/kubernetes/kubernetes/staging/src/k8s.io/apiserver/pkg/admission/interfaces.go`
- `/Users/jack/src/github.com/kubernetes/kubernetes/staging/src/k8s.io/apiserver/pkg/admission/chain.go`
- `/Users/jack/src/github.com/kubernetes/kubernetes/staging/src/k8s.io/apiserver/pkg/audit/evaluator.go`
- `/Users/jack/src/github.com/kubernetes/kubernetes/staging/src/k8s.io/apiserver/pkg/authorization/authorizer/interfaces.go`

## Current Nimbus Strengths

Nimbus already has a strong architecture foundation:

- The crate boundaries are clear: `nimbus-core` is validation/types with zero
  I/O, `nimbus-runtime` owns the runtime and HostBridge traits with zero
  workspace dependencies, `nimbus-server` owns integration, and
  `nimbus-sandbox` owns backend-agnostic sandbox lifecycle contracts.
- The product already treats admission, tenant isolation, artifact provenance,
  sandbox egress, runtime grants, and HostBridge authority as explicit safety
  planes rather than incidental checks.
- The active enterprise plans have unusually good evidence discipline:
  reusable verification scripts, phase ledgers, and current docs that explain
  fail-closed behavior.
- The storage implementation is already concept-owned by provider and operation
  class, and the mutation path/transaction atomicity invariants are well
  documented.
- The pre-launch policy lets us make cleaner breaking changes instead of
  carrying compatibility shims.

## Current Tension Points

The main risks are not bad ideas. They are good ideas accumulating in a few
large composition roots and public surfaces that are becoming harder to audit.

| Area | Observation | Enterprise trust risk |
| --- | --- | --- |
| Tenant isolation | `crates/nimbus-server/src/tenant_isolation.rs` is over 2,500 lines while also re-exporting child modules. | Identity, decision evidence, runtime grants, storage grants, network endpoints, quotas, and redaction are harder to review independently. |
| System tenant | `crates/nimbus-server/src/system_tenant.rs` is over 2,100 lines and mixes reserved tenant helpers, schemas, projection observation, and record writers. | Operators need system-state evidence to be predictable. Mixed ownership makes drift and audit changes harder to localize. |
| Server construction | `crates/nimbus-server/src/lib.rs` re-exports a broad enterprise surface, and router construction has many public convenience shapes. | Public seams become difficult to stabilize, document, and test. |
| Runtime policy | `crates/nimbus-runtime/src/limits.rs` mixes backend taxonomy, adapter diagnostics, execution models, trust tiers, grants, budgets, reset capabilities, and routing affinity. | Runtime backend expansion, especially optional Bun/JSC, needs small stable axes that cannot accidentally bleed into V8/Deno/Node policy. |
| Runtime bootstrap | `crates/nimbus-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js` is product-owned JavaScript above the hard threshold. | Node compatibility bootstrap behavior is safety-critical and should be split by capability or documented as a strong exception. |
| Sandbox services | `crates/nimbus-server/src/service_manager.rs` mixes image verification, activation, launch, handle refresh, runtime service lookup, and system-state recording. | Service admission and launch evidence should be traceable through a narrow lifecycle facade. |
| Operator policy | `tenant_isolation/operator_policy.rs` is over 1,600 lines even with child modules. | Typed policy documents, defaulting, validation, evaluation, explanation, reload, external backend, and prover behavior need clearer homes. |
| Cloud Functions adapter | `crates/nimbus-server/src/adapters/cloud_functions/http.rs` is over 1,500 lines. | HTTP request parsing, signature binding, execution handoff, and response mapping should be easier to test independently. |
| CLI dev | `crates/nimbus-bin/src/dev.rs` is near 2,000 lines and owns args, app-dir setup, env/deployment, dependency checks, readiness, browser launch, banner, and watch loops. | Local DX behavior is central to trust, but failures are harder to reason about when all phases live together. |
| JS/package surfaces | Several SDK/UI files rely on broad casts or large self-test files. | Adapter wrappers and generated/test fixtures need clearer typed seams so compatibility behavior is easier to audit. |
| Generated/vendor/test corpus | Node compatibility fixtures and generated route files dominate some line-count reports. | Architecture guardrails need explicit exclusions so refactor plans focus on owned source, not imported evidence corpora. |

## OpenShell Lessons

OpenShell is useful as a competitor and reference because it makes the security
story legible:

- The gateway/supervisor split keeps control-plane intent separate from
  sandbox-local enforcement.
- `openshell-policy` makes the policy file a single canonical serde model with
  unknown-field denial and conversion at the boundary.
- `openshell-ocsf` isolates event builders, objects, enums, formatters, and
  tracing layers from the rest of the product.
- `openshell-prover` keeps formal policy analysis in a separate lane from core
  policy parsing and enforcement.
- Capability tests are named around enterprise behaviors such as egress bypass,
  live policy update, provider setup, and sandbox labels.

Nimbus should adopt the shape, not the product sprawl. The single binary can
still expose clear submodes and internal modules for policy, evidence,
supervisor enforcement, and proof.

## Kubernetes Lessons

Kubernetes is useful because it shows how long-lived enterprise control planes
make critical seams boring and findable:

- API types, validation/defaulting, admission, authorization, authentication,
  and audit live in distinct packages.
- Admission handlers receive a typed request attribute envelope and are chained
  through narrow `Admit` and `Validate` interfaces.
- Authorization uses a small decision interface over typed attributes.
- Audit policy evaluation returns a compact evaluated audit config rather than
  spreading audit decisions through handlers.
- Conformance and integration tests are organized by behavior, not by the file
  that happened to contain the implementation.

The Nimbus translation is not to copy Kubernetes package structure. It is to
make Nimbus request/admission/evidence phases similarly explicit inside the
existing Rust crate map.

## Architecture Direction

The cleanup should preserve these boundaries:

```text
tenant/operator intent
  -> typed document parsing and defaulting
  -> validation
  -> admission/evaluation
  -> materialization inputs
  -> PEP enforcement in runtime, HostBridge, storage, sandbox, egress
  -> audit/export/proof evidence
```

Recommended rules:

- Keep typed Rust policy evaluation as the built-in authority. OPA, Cedar, and
  provers remain optional adapters that cannot override hard Nimbus denies.
- Keep composition roots thin. Move behavior into concept-owned modules such as
  `identity`, `decision`, `records`, `validation`, `evaluation`, `launch`, or
  `diagnostics` instead of vague `helpers` or `common`.
- Use canonical options/builders for public construction surfaces rather than
  proliferating overloads.
- Preserve `nimbus-core` zero-I/O and `nimbus-runtime` zero-workspace-dependency
  invariants.
- Treat generated files, vendored compatibility fixtures, proof artifacts, and
  upstream test corpora as explicit guardrail exclusions.
- Split only when the target module has a coherent ownership story and focused
  tests can prove behavior did not drift.

## Recommended Buckets

1. Architecture inventory and guardrails.
2. Tenant isolation concept split.
3. System tenant and projection split.
4. Server construction/public API seam cleanup.
5. Runtime limits and bootstrap ops split.
6. Sandbox service manager and supervisor lifecycle split.
7. Operator policy, provenance, audit, and evidence organization.
8. CLI dev and machine command organization.
9. JS SDK, Convex/Firebase compatibility, and UI typed seam cleanup.
10. Final verification and documentation synchronization.
