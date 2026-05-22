# Research: Tenant Isolation Enterprise Hardening Prior Art

Durable research note for the follow-on work after
`docs/plans/archive/tenant-isolation-control-plane-plan.md`. Pairs with
`docs/plans/archive/tenant-isolation-enterprise-hardening-plan.md` and the
current posture note at `docs/tenant-isolation.md`.

This document is not progress state. It records what the current Nimbus
tenant-isolation architecture should learn from mature control planes and
which codebases are worth reading before changing the isolation contract.

---

## Why This Research Exists

The completed tenant-isolation control-plane plan made the critical shift from
"a microVM exists" to "tenant authority is admitted before host authority is
materialized." That is the right foundation, but enterprise trust depends on
the next layer:

- policy decisions that are explicit, inspectable, and auditable
- reusable conformance tests instead of one-off proof harnesses
- hard resource enforcement below launch-time quota reservation
- workload identity and secret handling that are first-class capabilities
- signed/provenanced artifacts before production image execution
- correlated traces, metrics, logs, and audit records for every tenant action
- external-review-ready threat model and operational runbooks

The architectural question for this note is: **how do high-quality systems
make multi-tenant workload isolation boring, reviewable, and repeatable?**

## Current Nimbus Baseline

Nimbus already has the right primary seams:

- `TenantIsolationContext` carries server-owned tenant authority through
  native HTTP, adapters, WebSocket, runtime HostBridge, storage-facing engine
  calls, and sandbox service launch.
- `SandboxServiceManager` keys handles by `(tenant_id, service_name)` and
  rejects mismatched tenant/service/backend launch state.
- `RuntimeExecutionAdmission` gates production in-process runtime policies and
  routes unsafe policies away from `in_process_untrusted`.
- krun/container artifacts, logs, named volumes, service handles, and cleanup
  are tenant-scoped.
- Compose production admission rejects host binds, raw secrets, tag-only
  images, local builds without provenance policy, and non-loopback service
  exposure.
- The TIC8 harness proves two-tenant isolation across storage, runtime
  service lookup, `ctx.services`, `_nimbus`, cleanup, and service artifacts.

The main follow-on risk is not a missing primitive. It is that the current
authorization and admission state can become scattered again unless Nimbus
promotes it into a typed, immutable decision record and keeps all enforcement
points consuming that record.

## Canonical Pattern

The strongest pattern across prior art is:

```text
tenant intent
  -> authentication and workload identity
  -> admission policy decision
  -> immutable decision record
  -> enforcement at materialization/runtime/storage/network seams
  -> audit/telemetry/cleanup
```

In Kubernetes terms, this is admission control plus namespace/RBAC/quota/
network-policy enforcement. In Vault terms, this is authenticate, validate,
authorize, issue a leased token/secret, and audit. In Firecracker/gVisor/Kata
terms, this is the control plane deciding what the sandbox may see before the
sandbox process/VM starts.

Nimbus should preserve that structure and avoid letting leaf operations
recompute tenant authority from request strings.

## Prior-Art Survey

### Kubernetes Multi-Tenancy

Sources:

- `https://kubernetes.io/docs/concepts/security/multi-tenancy/`
- `https://kubernetes.io/docs/concepts/policy/resource-quotas/`
- `https://kubernetes.io/docs/concepts/cluster-administration/flow-control/`
- `https://kubernetes.io/docs/reference/access-authn-authz/validating-admission-policy/`

Patterns worth borrowing:

| Kubernetes concept | Nimbus equivalent |
| --- | --- |
| Namespace per tenant/workload | `TenantId` plus service/runtime/storage namespaces |
| RBAC and service accounts | application principal/operator authority in `TenantIsolationContext` |
| ResourceQuota and LimitRange | `SandboxResourceQuotaPolicy`, runtime tenant budgets, future storage/log quotas |
| NetworkPolicy default deny | loopback/proxy service exposure and fail-closed runtime network grants |
| ValidatingAdmissionPolicy | typed Nimbus admission policy before runtime/OCI/storage materialization |
| API Priority and Fairness | per-tenant request/invocation scheduling fairness |

Important lessons:

- Namespaces are necessary but not enough. Kubernetes documents that
  namespace isolation does not cover every global resource, so production
  isolation needs RBAC, quotas, network policies, and sometimes virtual
  control planes.
- Resource quotas are both fairness and safety controls. They prevent noisy
  neighbors but do not cover all shared resources, especially network traffic.
- Default-deny network policy is the recommended starting point when strict
  tenant network isolation is required.
- ValidatingAdmissionPolicy separates policy definition, parameters, and
  binding/scope. Nimbus should mirror that split with policy logic, tenant
  policy parameters, and endpoint/runtime/sandbox scope.

Concrete code/docs to study next:

- Kubernetes validating admission policy docs and CEL examples.
- Kubernetes `resourcequota` admission plugin:
  `staging/src/k8s.io/apiserver/pkg/admission/plugin/resourcequota/admission.go`
- Kubernetes API Priority and Fairness request classification and queueing
  concepts.

### OPA Gatekeeper

Sources:

- `https://www.openpolicyagent.org/docs/kubernetes`
- `https://github.com/open-policy-agent/gatekeeper`
- `https://github.com/open-policy-agent/gatekeeper-library`

Patterns worth borrowing:

- Policy-as-data with explicit constraints.
- Policy templates separate reusable logic from deployment-specific
  parameters.
- Audit is not only request-time admission; Gatekeeper can scan existing state
  for drift.
- The policy library requires allowed/disallowed samples and tests for every
  policy.

Nimbus lesson:

Tenant isolation should grow a small internal policy library with fixture
cases. Even if Nimbus does not embed OPA/Rego, it should have Gatekeeper-style
policy artifacts:

```text
policy schema
policy parameters
decision record
allowed/disallowed fixtures
audit/drift scan
```

Concrete code/docs to study next:

- Gatekeeper Library contribution model:
  `src/<policy-name>/constraint.tmpl`, `src.rego`, `src_test.rego`,
  allowed/disallowed samples, and `suite.yaml`.
- Gatekeeper audit controller shape and violation reporting.

### Firecracker

Sources:

- `https://github.com/firecracker-microvm/firecracker/blob/main/docs/jailer.md`
- `https://github.com/firecracker-microvm/firecracker/blob/main/docs/seccomp.md`
- `https://github.com/firecracker-microvm/firecracker/tree/main/src/jailer`

Patterns worth borrowing:

- Jailer creates process-level isolation before the VM runs.
- Cgroups can be configured before execution, avoiding a privileged process
  mutating resource limits later.
- Chroot/base directories and VM IDs are explicit.
- Resource limits such as file size and file descriptors are part of the
  launch boundary.

Nimbus lesson:

The krun service path already has tenant-scoped artifacts and quota
reservation. Enterprise hardening should add host-enforced limits where the
platform supports them: cgroup v2, project quotas, quota-backed disks, file
size limits, and no-file/proc limits for the VMM/conmon/crun process tree.

Concrete code/docs to study next:

- Firecracker jailer cgroup setup and chroot directory layout.
- Firecracker seccomp filter lifecycle.
- How Firecracker reports jailer/VM startup failures to operators.

### Kata Containers

Sources:

- `https://github.com/kata-containers/kata-containers/blob/main/docs/design/architecture/README.md`
- `https://github.com/kata-containers/kata-containers/blob/main/docs/design/virtualization.md`

Patterns worth borrowing:

- OCI/container UX with a VM-backed data plane.
- Explicit "pod sandbox" architecture where a VM is a second layer of defense,
  not a replacement for control-plane policy.
- Compatibility through OCI/CRI boundaries instead of a bespoke workload
  packaging model.

Nimbus lesson:

Nimbus is taking the same broad path: preserve Compose/OCI semantics while
using microVMs where trust boundaries demand it. The improvement is to make
the VM placement contract explicit: one tenant per guest, one admitted service
identity per sandbox, and no multi-tenant guest until a separate plan proves
that weaker trust tier.

Concrete code/docs to study next:

- Kata shim v2 architecture docs.
- Kata sandbox/pod lifecycle and device/mount policy docs.

### gVisor

Sources:

- `https://gvisor.dev/docs/architecture_guide/security/`
- `https://gvisor.dev/docs/architecture_guide/intro/`
- `https://gvisor.dev/docs/architecture_guide/resources/`

Patterns worth borrowing:

- Clear threat model that distinguishes host kernel exposure, sandbox kernel
  exposure, side channels, and operational tradeoffs.
- A precise statement of what a sandbox can and cannot do.
- Minimal host syscall and host filesystem interaction.
- Separate filesystem broker/Gofer pattern.

Nimbus lesson:

Nimbus needs a customer-facing threat model that says exactly what in-process
V8/Deno, future Bun/JSC, WASM, krun microVMs, containers, storage providers,
and HostBridge do and do not protect against. Avoid impossible claims such as
"V8 isolate makes cross-tenant leakage impossible." Say which layer enforces
which property and what residual risk remains.

Concrete code/docs to study next:

- gVisor security model and resource model wording.
- gVisor sandbox/Gofer split as inspiration for HostBridge and storage/file
  mediation docs.

### SPIFFE / SPIRE

Sources:

- `https://spiffe.io/docs/latest/spire-about/spire-concepts/`

Patterns worth borrowing:

- Workload identity is registered, selected, and attested.
- Node attestation and workload attestation are separate phases.
- Identity is portable and independent from any single runtime platform.

Nimbus lesson:

`TenantIsolationContext` is request authority; it is not yet full workload
identity. Future tenant services and runtime invocations should be able to
derive a workload identity from tenant, deployment, service/function, runtime
tier, node/machine, and sandbox ID. That identity should be usable for secret
provider auth, service-to-service auth, audit logs, and external provider
credentials.

Concrete code/docs to study next:

- SPIRE registration entries and selector model.
- SPIFFE ID naming conventions for multi-tenant services.

### HashiCorp Vault

Sources:

- `https://docs.hashicorp.com/vault/docs/about-vault/how-vault-works`
- `https://developer.hashicorp.com/vault/docs/concepts/lease`
- `https://developer.hashicorp.com/vault/docs/secrets/databases`

Patterns worth borrowing:

- Authenticate, validate, authorize, access.
- Policy is attached to the token/identity.
- Secrets and dynamic credentials are leased and revocable.
- Audit logging is central to trust.
- Secret engines are plugin-like providers behind a core policy layer.

Nimbus lesson:

The existing `docs/plans/secret-management-plan.md` is the right owner for
secret materialization. Tenant isolation should only define the capability
contract: secret access is via handles/references and leased capability, not
ambient environment or raw credentials in deployment descriptors.

Concrete code/docs to study next:

- Vault lease manager and dynamic database secrets.
- Vault audit device failure posture.
- Vault policy path model.

### Sigstore, Cosign, And SLSA

Sources:

- `https://docs.sigstore.dev/`
- `https://docs.sigstore.dev/cosign/verifying/verify/`
- `https://github.com/slsa-framework/slsa`

Patterns worth borrowing:

- Artifacts should be signed and/or accompanied by verifiable provenance.
- Verification should bind image digest, identity, and policy.
- Transparency logs and attestations make supply-chain state inspectable.

Nimbus lesson:

Production Compose admission currently accepts digest-pinned images and fails
closed for tag-only/local-build images. That is a good floor. The next
enterprise step is an image-admission provider that verifies signatures,
attestations, SBOM presence, and allowed builder identities before a digest is
admitted.

Concrete code/docs to study next:

- Cosign verification policy options.
- SLSA provenance predicate shape.
- How policy engines such as Kyverno/Gatekeeper model image verification.

### OpenTelemetry

Sources:

- `https://opentelemetry.io/docs/`
- `https://opentelemetry.io/docs/specs/otel/logs/`
- `https://opentelemetry.io/docs/concepts/semantic-conventions/`

Patterns worth borrowing:

- Vendor-neutral traces, metrics, and logs.
- Correlate logs with traces using trace/span IDs.
- Common semantic attributes make cross-system debugging possible.

Nimbus lesson:

Tenant isolation decisions need observability shape, not just tests. Every
admission/rejection/materialization/cleanup event should carry tenant,
surface, principal class, service/function, runtime tier, sandbox ID, decision
ID, and correlation IDs. Sensitive data must be redacted before export.

Concrete code/docs to study next:

- OTel semantic conventions for attributes.
- Log correlation with trace/span IDs.

## 2026-05-22 Primary And Code-Source Reading Pass

This pass closes the research part of EIH0. The evidence below is intentionally
concrete enough to drive implementation, but it does not copy the external
systems wholesale. Nimbus should borrow the control-plane shape, not the whole
platform.

| Source family | Primary/code source | Pattern observed | Nimbus decision | Rejection or constraint |
| --- | --- | --- | --- | --- |
| Kubernetes multi-tenancy | `https://kubernetes.io/docs/concepts/security/multi-tenancy/` | Tenant isolation is composed from namespaces, RBAC, quotas, network policy, workload identity, and sometimes stronger control-plane isolation. | Keep Nimbus tenant isolation as a control-plane contract that covers compute, network, storage, HostBridge, cleanup, and audit, not as a microVM-only claim. | Reject "OCI bundle exists, therefore tenant isolation exists." OCI remains the runtime envelope, not the authorization model. |
| Kubernetes admission | `https://kubernetes.io/docs/reference/access-authn-authz/admission-controllers/` and `https://kubernetes.io/docs/reference/access-authn-authz/validating-admission-policy/` | Admission observes an incoming request and rejects it before persistence or runtime materialization; policy definition, parameters, and binding/scope are separate concepts. | EIH1/EIH2 should add a typed admission decision plus a clear PDP/PEP split so lower seams consume a decision, not raw user intent. | Do not embed CEL or Kubernetes-shaped policy as the first internal contract; use typed Rust policy first and keep a provider seam possible later. |
| Kubernetes quota | `https://pkg.go.dev/k8s.io/apiserver/pkg/admission/plugin/resourcequota` | The quota admission package defines an evaluator and accessor abstraction around namespace-scoped quota state. | EIH5 should prove quota below Nimbus launch reservation, with an interface that can later support per-tenant aggregate quota. | Do not trust reservation-only accounting as hard isolation evidence. |
| Gatekeeper policy library | `https://github.com/open-policy-agent/gatekeeper-library` | Policies carry templates, parameter schemas, Rego tests, allowed/disallowed samples, and `suite.yaml`; `gator verify ./...` is the policy proof command. | EIH3 should promote TIC8 into named allowed/denied scenario fixtures with one command and counts. | Do not make one-off integration tests the only evidence for tenant isolation. |
| Gatekeeper audit | `https://open-policy-agent.github.io/gatekeeper/website/docs/audit/` | Audit periodically evaluates existing state against constraints and reports violations separately from request-time admission. | EIH4 should add a read-only Nimbus drift scanner for sandbox manifests, handles, port records, volumes, routes, and decision/audit presence. | Do not auto-repair drift in this plan; reporting must be precise and non-mutating first. |
| Firecracker jailer | `https://raw.githubusercontent.com/firecracker-microvm/firecracker/main/docs/jailer.md` | Jailer binds VM IDs, UID/GID, cgroups, chroot root, netns, resource limits, fd cleanup, env cleanup, device nodes, and privilege drop before exec. | EIH5 should prove a Linux hard limit below Nimbus admission, and service launch should keep operator-owned paths, IDs, and resources explicit. | Do not pass tenant-controlled host paths into sandbox launch; the operator/control plane remains in the trusted computing base. |
| Firecracker seccomp | `https://raw.githubusercontent.com/firecracker-microvm/firecracker/main/docs/seccomp.md` | Production seccomp defaults are part of the launch security posture; custom filters are advanced and require integrity care. | Keep OCI/seccomp/capability posture as launch evidence and add it to enterprise readiness docs. | Do not treat debug/no-seccomp paths as production-equivalent. |
| Kata Containers | `https://raw.githubusercontent.com/kata-containers/kata-containers/main/docs/design/architecture/README.md` | Kata preserves OCI/CRI/container UX while using a VM-backed data plane; it separates host, VM root, and container environments. | Nimbus should preserve Compose/OCI compatibility while documenting that one tenant per microVM guest is the strong-isolation tier. | Do not support multi-tenant guest packing until a future weaker-trust-tier plan proves it. |
| gVisor security and resources | `https://gvisor.dev/docs/architecture_guide/security/` and `https://gvisor.dev/docs/architecture_guide/resources/` | gVisor states its threat model, attack surfaces, resource accounting, network stack location, and host fd mediation boundaries. | EIH9 should publish a Nimbus threat model and isolation matrix by runtime tier and enforcement layer. | Do not make impossible claims about V8/JSC isolate security, microVM side channels, or network isolation beyond tested layers. |
| SPIFFE/SPIRE | `https://spiffe.io/docs/latest/spire-about/spire-concepts/` and `https://spiffe.io/docs/latest/deploying/registering/` | Registration entries bind a SPIFFE ID to selectors and a parent ID; node attestation and workload attestation are distinct phases. | EIH6 should define a stable workload identity that can map to `spiffe://<trust-domain>/tenant/<tenant>/deployment/<deployment>/workload/<workload>` later. | Do not expose raw tenant strings as secret-provider auth without a typed identity and authority record. |
| Vault leases and audit | `https://developer.hashicorp.com/vault/docs/concepts/lease` and `https://developer.hashicorp.com/vault/docs/audit` | Dynamic secrets have lease IDs, TTLs, renewal/revocation paths, prefix revocation, and audit devices hash sensitive strings by default. | Tenant isolation owns the capability contract: secrets are referenced by handles, leased to a workload identity, and redacted by schema. | Do not materialize raw secrets in compose descriptors, audit logs, or ambient runtime environment by default. |
| Sigstore/Cosign and SLSA | `https://docs.sigstore.dev/cosign/verifying/verify/` and `https://slsa.dev/spec/v1.0/provenance` | Verification binds signatures to image digests and certificate identity; SLSA provenance uses in-toto statements with `subject`, `builder.id`, and `predicateType`. | EIH7 should add an image-verification provider seam that checks digest floor, signature, issuer/subject, builder identity, and provenance predicate. | Do not hardwire Cosign CLI as the only implementation; keep a provider seam for offline bundles, private roots, and enterprise registries. |
| OpenTelemetry logs and semantics | `https://opentelemetry.io/docs/specs/otel/logs/data-model/` and `https://opentelemetry.io/docs/concepts/semantic-conventions/` | Logs have a stable data model, trace/span correlation, resource attributes, and semantic attribute names for cross-system interpretation. | EIH8 should define structured tenant-isolation events with decision ID, tenant ID, surface, workload, runtime tier, sandbox/invocation ID, result, reason, and correlation IDs. | Do not rely on ad hoc caller-formatted strings for audit or observability evidence. |

## Nimbus Decisions And Rejections

Decisions accepted by this research pass:

- Add an immutable admission decision record before widening more runtime or
  sandbox policy.
- Keep `TenantIsolationContext` as authority input and make the new decision
  the stable enforcement/audit artifact.
- Use internal PDP/PEP language to keep policy decisions out of lower leaf
  operations.
- Promote TIC8 into a reusable conformance suite with Gatekeeper-style
  allowed/denied fixtures.
- Add a read-only drift scanner before any remediation behavior.
- Prove Linux hard quota enforcement in at least one path below launch
  reservation.
- Define workload identity before connecting tenant isolation to secret
  providers.
- Put image signature/provenance behind an adapter seam instead of binding
  core admission to one verifier binary.
- Make structured, redacted audit events a schema contract.

Options rejected or deferred:

- Embedding OPA/Rego or CEL directly in Nimbus's first policy seam. A provider
  can be added after the typed decision artifact is stable.
- Treating OCI, microVM launch, or V8/JSC isolates as sufficient tenant
  isolation without storage, network, HostBridge, quota, and audit controls.
- Passing tenant-supplied host paths, raw secrets, or ambient service tokens
  through admission.
- Running multiple mutually untrusted tenants in one guest VM.
- Auto-repairing tenant-isolation drift before the scanner can report precise
  violations.
- Making public network exposure or generic localhost grants implicit.
- Requiring Sigstore/SLSA for local developer flows before production policy
  can express explicit exceptions.

## Design Recommendations For Nimbus

### 1. Promote `TenantIsolationContext` Into A Decision Envelope

Keep `TenantIsolationContext` as the authority input, but add an immutable
decision record produced by admission:

```text
TenantIsolationDecision
  id
  tenant_id
  surface
  authority
  deployment_generation
  workload_identity
  runtime_policy_decision
  service_grants
  network_endpoints
  storage_namespace
  volume_policy
  image_policy
  secret_policy
  quota_reservations
  audit_redactions
```

The decision should be passed to materialization seams instead of passing
independent tenant/policy/quota arguments.

### 2. Use PDP/PEP Language Internally

Nimbus does not need to expose this naming to users, but the code should have
the shape:

- PDP: policy decision point, builds `TenantIsolationDecision`
- PEP: policy enforcement point, consumes a decision at runtime, sandbox,
  storage, network, and HostBridge boundaries

This keeps policy logic out of leaf operations and makes audits easier.

### 3. Turn TIC8 Into A Conformance Suite

The two-tenant proof harness should become a reusable conformance suite with
fixtures for:

- swapped path/header tenant
- swapped bearer tenant claim
- mismatched service handle
- same service name across tenants
- same named volume across tenants
- `_nimbus` access from application runtime
- generic localhost grant
- production image admission failure
- tenant cleanup preserving the other tenant

### 4. Add A Drift/Audit Scanner

Gatekeeper's audit loop is the right inspiration. Nimbus should be able to
scan existing state and report tenant-isolation drift:

- sandbox manifest under wrong tenant root
- service handle tenant mismatch
- system port record missing tenant/service identity
- volume root not under tenant root
- route metadata without tenant/surface
- admission decision without audit record

### 5. Finish Hard Quotas Below The Control Plane

Launch-time reservation is necessary but not enough. The next hardening wave
should prove platform enforcement for at least one Linux path:

- cgroup v2 CPU/memory/pids/no-file for the sandbox process tree
- project quotas or quota-backed filesystem for writable volumes/rootfs/logs
- conmon/crun/libkrun failure behavior when limits are exceeded

### 6. Make Enterprise Readiness A Documented Product Surface

Enterprise trust is helped by code, but won by evidence:

- threat model
- isolation matrix
- conformance test report
- supply-chain verification story
- audit/telemetry schema
- residual risk register
- incident/debug runbooks
- external security review queue

## Follow-Up Implementation Reading Queue

EIH0 is complete for plan direction. During implementation, keep reading the
specific code around the seam being changed and record concrete examples when
they influence code shape:

1. Kubernetes ResourceQuota admission plugin and APF fair-queuing behavior.
2. Kubernetes ValidatingAdmissionPolicy binding/parameter model.
3. Gatekeeper audit controller and policy library fixture conventions.
4. Firecracker jailer resource-limit and cgroup setup.
5. gVisor threat model and filesystem mediation wording.
6. SPIRE workload registration and selector model.
7. Vault audit failure and lease revocation implementation.
8. Cosign verification API and SLSA provenance predicates.
9. OTel semantic attributes for multi-tenant admission/audit events.
