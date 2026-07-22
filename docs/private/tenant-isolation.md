# Tenant Isolation

Nimbus treats tenant isolation as a control-plane contract, not as a side
effect of OCI, JavaScript isolates, or microVMs alone. A tenant workload is
admitted once at the server boundary, then every lower enforcement point
consumes the admitted `TenantIsolationDecision` or a narrow projection of it.

This document is the enterprise readiness reference for the current tenant
isolation posture. It summarizes the threat model, isolation claims, evidence,
residual risks, and review targets. Operational response steps live in the
[tenant isolation runbook](operating/tenant-isolation.md).

## Scope

In scope:

- Tenant identity, deployment generation, workload identity, and authority
  projection.
- In-process runtime admission for V8/Deno-backed JavaScript execution.
- MicroVM and container service sandbox launch admission.
- Storage/API tenant authorization, HostBridge operations, service lookup,
  network exposure, volumes, images, secrets, quotas, cleanup, audit events,
  and drift reporting.

Out of scope:

- A compromised host root account.
- Physical attacks on the operator machine or cloud host.
- Side-channel classes that require hardware-specific mitigation beyond the
  currently selected KVM, V8, Deno, Rust, kernel, and CPU stack.
- Customer application authorization bugs inside a tenant. Nimbus prevents
  cross-tenant widening; it does not replace application-level ACLs.

## Architecture Claim

Tenant-controlled intent must not lower directly into host paths, OCI mounts,
devices, network listeners, database namespaces, runtime grants, or secret
material. Nimbus must first rewrite or reject the intent through admission.

```mermaid
flowchart LR
    Intent["Tenant intent"] --> Context["TenantIsolationContext"]
    Context --> Decision["TenantIsolationDecision"]
    Decision --> Runtime["Runtime PEP"]
    Decision --> Sandbox["Sandbox/OCI PEP"]
    Decision --> Storage["Storage/API PEP"]
    Decision --> Network["Network/service PEP"]
    Decision --> HostBridge["HostBridge PEP"]
    Decision --> Audit["Audit events + drift"]
```

The decision envelope includes tenant ID, authority class, deployment
generation, admitted workload identity, workload subject, workload audit
projection, runtime policy, service grants, network endpoints, storage
namespace, volume/image/secret policy, quota reservation, audit redactions, and
a deterministic decision ID.

The implementation lives in the server's `tenant` domain module:
`crates/nimbus-server/src/tenant.rs` re-exports the concept-owned children under
`crates/nimbus-server/src/tenant/`. The module name is broad by design; the
security concept and admitted artifacts remain explicit as tenant isolation,
`TenantIsolationContext`, and `TenantIsolationDecision`.

Runtime lifecycle composes two narrower authorities without duplicating them.
Engine/storage remain authoritative for tenant existence, durable incarnation,
and the operation/delete fence. `nimbus-compute::RuntimeManager` lowers an
Engine-issued runtime lease into a revocable runtime owner, selects Nimbus-owned
runtime lanes, and owns queue cancellation, worker acknowledgements, retained
state retirement, and deployment-authority generations. Adapters cannot mint a
tenant owner or own an executor.

## Threat Model

| Threat | Control | Evidence |
| --- | --- | --- |
| Tenant swaps a path/header/bearer tenant ID to read another tenant. | Server-owned tenant admission checks principal claims and route tenant before storage/runtime calls. | `make verify-tenant-isolation-conformance` includes bearer tenant swap and native path denial scenarios. |
| Runtime code for one tenant supplies another tenant ID to HostBridge. | HostBridge uses the admitted invocation tenant and decision-derived storage/service projections. | Conformance scenarios prove tenant-b runtime reads only tenant-b data and application runtime cannot read `_nimbus`. |
| Guest-mutated V8 or Wasmtime state is reused by another tenant or a recreated tenant ID. | Retained state is partitioned by owner class, stable subject, and Engine/storage incarnation; exact reuse authority is checked independently of routing; owner/deployment retirement reaches every worker. | Runtime owner property tests, V8/Wasmtime sentinel tests, executor race tests, and the shared served-adapter delete/recreate harness. |
| Runtime code asks for broad host capabilities in production. | `TenantIsolationMode::Production` rejects unsafe in-process grants or routes to a stronger tier before JavaScript runs. | `cargo test -p nimbus-server 'tenant::' -- --nocapture`. |
| Service launch materializes another tenant's sandbox, port, or volume. | Sandbox launch consumes a decision-derived service access projection and validates tenant/service/backend before backend launch. | Tenant isolation unit tests and conformance service/volume collision scenarios. |
| Tenant-controlled Compose input mounts host paths or exposes public ports. | Production Compose admission rejects host binds, undeclared/unsafe volumes, raw secrets, tag-only images, and non-loopback service exposure unless an explicit policy exists. | Conformance image admission scenarios and sandbox architecture docs. |
| Tenant image content attacks the host during materialization. | Production image admission requires digest-pinned images as the floor; `ServiceManager` runs image admission before service image materialization; concrete Cosign, SLSA, SBOM, offline/private-root, composite-verifier, and command-adapter seams provide stronger policy. | `make verify-artifact-provenance`, image admission tests, service-manager tests, and production Compose admission tests cover digest floor, unsigned, wrong identity, provenance source URI, SBOM, offline trust roots, and local-build rejection. |
| Existing state drifts away from the isolation contract. | Read-only drift scanner reports malformed manifests, handles, ports, routes, volume roots, and missing decision/audit anchors. | `cargo test -p nimbus-server tenant_isolation_drift -- --nocapture`. |
| Audit logs leak bearer claims or secret handles. | `TenantIsolationEvent` redacts sensitive attributes and correlation IDs by schema. | `cargo test -p nimbus-server audit_events -- --nocapture`. |

## Isolation Matrix

| Claim | Enforcing Layer | Test Evidence | Residual Risk |
| --- | --- | --- | --- |
| Tenant identity is fixed before lower seams run. | `TenantIsolationContext` to `TenantIsolationDecision`. | `tenant_isolation_decision_*` tests and conformance tenant-swap scenarios. | Future product tenant membership for native HTTP remains separate from current local-operator auth. |
| In-process runtime cannot widen production host grants. | Runtime admission and `RuntimeExecutionAdmission`. | Runtime policy admission tests. | Unsafe policies need configured fallback executors before they can run outside the in-process tier. |
| Mutable runtime retention cannot cross owner incarnations. | `RuntimeManager`, `RuntimeOwnerLease`, `OwnerPartitionedPool`, and worker retirement control. | `make verify-runtime-tenant-isolation` plus focused V8, Wasmtime, retirement, and served adapter conformance tests. | Same-process isolates are not a hard boundary against V8, native-code, speculative-execution, or process-memory compromise. |
| MicroVM service compute is tenant-scoped. | `ServiceManager`, service registry, sandbox backend validation. | Conformance same-service-name and sandbox handle scenarios. | Host-side krun/libkrun process still carries accepted root VMM lifetime risk. |
| Network exposure is private by default. | Service grants, loopback default, patched krun/libkrun TSI bind address. | Conformance localhost denial and Linux localhost-only proof from the sandbox hardening baseline. | Public exposure policy is intentionally not admitted yet. |
| Sandbox egress has a typed deny-by-default policy contract. | `SandboxEgressPolicy`, compiled canonical policy checks, `SandboxEgressEnforcementPlan`, hidden `nimbus sandbox-supervisor` contract consumer, operator policy compiler, strict Compose `x-nimbus.egress`, service-manager launch checks, OCI bundle env materialization, the shared `EgressProxyRegistry` seam, and the krun microVM netns + `HTTP_PROXY` forwarding + fail-closed execute readiness gate. | `make verify-enterprise-policy-egress`, sandbox egress/proxy unit tests, sandbox-supervisor contract tests, operator reload/prove/draft tests, service-manager policy mismatch tests, Compose lowering tests, minicloud container egress proof, and the krun microVM netns+PEP deny proof (`crates/nimbus-sandbox/tests/krun_linux_egress.rs`, minicloud KVM). | Both process-capable paths are PEP-enforced and live-reloadable: container launches route the bridge through the proxy, and krun execute-mode is the fourth NEG enforcement point — the VMM joins a deny-by-default host netns and the guest forwards HTTP(S) through the PEP, gated by a fail-closed execute readiness check. |
| Storage/API calls cannot cross tenants by caller-supplied tenant IDs. | Server/adapters/runtime HostBridge consume admitted tenant context. | Conformance runtime storage and bearer-swap scenarios. | External storage providers still require correct provider namespace configuration. |
| Named volumes are tenant-owned and host binds are denied by default. | Compose admission and sandbox mount materialization. | Conformance same-named-volume scenario. | Shared read-only artifact policy is future work. |
| Images are immutable at the production floor. | Image admission policy, service-manager launch admission, and verifier backends using maintained OCI reference parsing. | Image admission unit tests, service-manager materialization tests, production Compose admission tests, and `make verify-artifact-provenance`. | Verifier backends are command-adapter first and fixture-proven locally; operators still need to supply real Cosign/SLSA/SBOM tooling and trust roots in production environments. |
| Secrets do not materialize ambiently. | Secret policy records handles/counts, not raw values; raw Compose secrets fail closed. | Tenant audit record and production Compose tests. | Dedicated secret provider integration is tracked separately; provider-auth credentials must consume `docs/plans/service-identity-provider-auth-plan.md`. |
| Per-tenant resource reservation exists before launch. | Runtime budgets, sandbox quota policy, OCI resource quota manager. | EIH5 minicloud cgroup memory proof and sandbox quota tests. | Hard disk write caps require filesystem/project-quota support. |
| Cleanup cannot delete another tenant's artifacts. | Tenant-rooted sandbox state, volumes, and storage deletion path. | Conformance cleanup scenarios. | Manual host edits outside Nimbus remain an operator responsibility. |
| Audit and drift evidence is tenant-safe. | `TenantIsolationEvent` schema and drift scanner. | Audit event and drift scanner tests. | Event transport/export backend is intentionally separate from the schema. |

## Evidence Commands

Run these before changing tenant isolation, runtime admission, sandbox launch,
storage/API tenant authorization, HostBridge operations, image admission, or
drift scanning:

```sh
cargo test -p nimbus-server 'tenant::' -- --nocapture
cargo test -p nimbus-server tenant_isolation -- --nocapture
cargo test -p nimbus-server tenant_isolation_drift -- --nocapture
cargo test -p nimbus-server audit_events -- --nocapture
make verify-tenant-isolation-conformance
make verify-runtime-tenant-isolation
make verify-enterprise-policy-egress
make verify-artifact-provenance
cargo fmt --all --check
cargo clippy -p nimbus-server --all-targets
```

For Linux hard-quota evidence on a cgroup v2 host:

```sh
bash scripts/prove-linux-cgroup-memory-limit.sh
```

## Residual Risks

Accepted for the current baseline:

- The host-side krun/libkrun VMM process can require root for its lifetime on
  the validated stack. Track non-root `/dev/kvm` or post-initialization
  privilege-drop research before treating this as closed.
- Hard disk write caps are admission/reservation-backed today. A future disk
  quota driver should use project quotas, quota-backed volumes, or an
  equivalent platform primitive.
- Cryptographic artifact verification is command-adapter first. Nimbus owns
  policy, admission, evidence normalization, and redaction, while Cosign,
  `slsa-verifier`, and SBOM tooling own signature, certificate, transparency
  log, DSSE/in-toto/SLSA, and SBOM verification. Operators must install and
  configure the concrete tools and trust roots they require.
- Arbitrary guest egress from process-capable sandboxes — container and krun
  microVM alike — is enforced through the host-side egress PEP and live-reloads
  policy through the sandbox backend seam. The krun microVM is the fourth NEG
  enforcement point: the VMM joins a deny-by-default host network namespace
  (libkrun TSI makes the guest's `connect()` a host-side syscall the netns
  confines) and the guest forwards HTTP(S) through the PEP via injected
  `HTTP_PROXY`, gated by a fail-closed execute readiness check. The direct-egress
  deny is proven on minicloud KVM
  (`crates/nimbus-sandbox/tests/krun_linux_egress.rs`); the two-tenant parity and
  bypass-vector proofs currently time out waiting on the in-guest result file
  (test-harness plumbing, not the enforcement path) and are tracked as a
  follow-up. Browser and agent services must consume the same operator policy
  and sandbox egress contract rather than inventing a separate network policy
  dialect.
- `TenantIsolationEvent` is the canonical internal event schema. OCSF and
  OpenTelemetry mappings exist as schema projections, but export routing,
  retention, and SIEM transport remain operator/product choices.
- Secret provider authentication is not complete until
  `docs/plans/service-identity-provider-auth-plan.md` can mint short-lived,
  tenant-scoped credentials from admitted `WorkloadIdentity`
  projections, using stable provider subjects plus signed decision and
  invocation claims.
- Native HTTP tenant membership is not a general customer auth model yet. The
  current native API is a local-operator surface guarded by local admin auth.
- Audit event schema is stable, but export routing and retention policy are
  still operator/product choices.

## External Review Targets

Prioritize external security review in this order:

1. Tenant admission and PDP/PEP split:
   `crates/nimbus-server/src/tenant.rs` and
   `crates/nimbus-server/src/tenant/audit_events.rs`.
2. Runtime HostBridge and capability execution:
   `crates/nimbus-server/src/runtime_host/` and adapter HostBridge code.
3. Sandbox launch and OCI materialization:
   `crates/nimbus-sandbox/`, `nimbus-crun`, and `nimbus-libkrun`.
4. Artifact provenance verification:
   `TenantImageVerificationProvider` plus concrete Cosign/SLSA/SBOM backends
   from `docs/plans/archive/artifact-provenance-verification-plan.md`.
5. Service identity and provider auth:
   `WorkloadIdentity` projections, stable provider subjects, and
   short-lived credential minting from
   `docs/plans/service-identity-provider-auth-plan.md`.
6. Storage provider namespace isolation:
   `crates/nimbus-storage/` plus provider topology docs.
7. Conformance and drift evidence:
   `scripts/verify-tenant-isolation-conformance.sh`,
   `crates/nimbus-server/src/tenant_isolation_drift.rs`, and the minicloud
   cgroup proof.

## References

- [Tenant isolation runbook](operating/tenant-isolation.md)
- [Server auth and runtime trust](architecture/server/auth-runtime-trust.md)
- [MicroVM service baseline](architecture/sandbox/microvm-service-baseline.md)
- [Artifact provenance verification plan](plans/archive/artifact-provenance-verification-plan.md)
- [Service identity and provider auth plan](plans/service-identity-provider-auth-plan.md)
- [Verification architecture](architecture/testing/verification-architecture.md)
- [Completed tenant-isolation control-plane plan](plans/archive/tenant-isolation-control-plane-plan.md)
- [Enterprise hardening prior-art research](plans/research/tenant-isolation-enterprise-hardening-prior-art.md)
- [OpenShell competitor analysis](plans/research/openshell-competitor-analysis.md)
- [Enterprise policy and sandbox egress plan](plans/archive/enterprise-policy-and-sandbox-egress-plan.md)
