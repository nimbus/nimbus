# Plan: Service Identity And Provider Auth

Canonical deferred design and execution plan for workload identity minting and
provider-auth exchange in Nimbus.

The tenant-isolation enterprise hardening work introduced
`TenantWorkloadStableIdentity`, a stable subject derived from an admitted
`TenantIsolationDecision`. This plan owns the next layer: short-lived
credentials and service identity. Secret management owns secret values and
references; this plan owns identity minting and provider authentication.

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** promote when a concrete provider integration needs
  workload identity rather than static secrets: Vault JWT/Kubernetes auth,
  AWS IRSA/OIDC, GCP Workload Identity Federation, Azure federated
  credentials, SPIFFE/SPIRE SVIDs, mTLS client certificates, or signed
  service-account tokens.
- **Canonical subject:** `TenantWorkloadStableIdentity`
- **Current posture reference:** `docs/tenant-isolation.md`

## Goal

Make identity-bearing credentials tenant-scoped, short-lived, auditable, and
derived only from admitted workload identity.

Nimbus must not mint credentials from:

- raw tenant strings
- caller-supplied bearer claims
- local process context
- session IDs alone
- sandbox metadata supplied by tenant code

Nimbus may mint credentials only from a server-owned admission decision and a
provider policy that explicitly allows the workload subject.

## Scope

This plan owns:

- promotion of the `identity` grant from declaration/audit placeholder to an
  enforced capability
- local and enterprise trust-domain configuration
- OIDC/JWT minting for workload identity
- SPIFFE-compatible SVID paths and future SPIRE registration shape
- mTLS client certificates and service-account token exchange
- node/machine identity source, rotation, compromise recovery, and membership
  binding
- provider-auth adapters for Vault, Kubernetes, AWS, GCP, and Azure
- runtime and sandbox propagation of scoped identity projections
- audit events for mint, renew, revoke, deny, and provider exchange

This plan does not own:

- secret value storage, versioning, references, or rotation values
- runtime permission semantics beyond the `identity` grant
- cluster membership transport
- browser policy credentials or WASI agent HTTP allowlists

## Identity Contract

The provider-auth subject is the stable workload identity:

```text
nimbus-workload:v1
  /tenant/<tenant_id>
  /deployment/<generation|none>
  /surface/<admission_surface>
  /kind/<runtime_function|sandbox_service|http_request|system_task>
  /name/<percent-escaped service-or-function>
  /runtime-tier/<tier|none>
  /runtime-backend/<backend|none>
  /sandbox-backend/<backend|none>
  /node/<node_id|none>
  /machine/<machine_id|none>
  /sandbox/<sandbox_id|none>
  /invocation/<invocation_id|none>
```

When a SPIFFE-compatible trust domain is configured, the same subject can be
rendered as:

```text
spiffe://<trust-domain>/nimbus/workload/v1/tenant/.../invocation/...
```

Provider adapters must use this subject or a signed projection of it. They
must not reconstruct identity from caller inputs.

## Phase Ledger

| Phase | Status | Goal | Verification |
| --- | --- | --- | --- |
| SI0 | `todo` | Define provider-auth policy input and audit schema. | Tests prove identity mint requests without an admitted decision fail closed. |
| SI1 | `todo` | Promote `identity` grant to enforced capability. | Runtime and sandbox tests prove identity APIs deny without explicit grants. |
| SI2 | `todo` | Add local trust-domain and signing-key management. | Key rotation and stale-key denial tests pass without exposing private key material. |
| SI3 | `todo` | Implement short-lived OIDC/JWT minting. | Tokens carry tenant/workload subject, audience, expiry, and decision ID; wrong audience/provider denies. |
| SI4 | `todo` | Define SPIFFE/SVID integration path. | SPIFFE IDs render from `TenantWorkloadStableIdentity`; registration entries bind node/machine selectors. |
| SI5 | `todo` | Add provider-auth adapters. | Vault, Kubernetes, AWS, GCP, and Azure adapters are either implemented or gated by precise provider blockers. |
| SI6 | `todo` | Propagate identity projections to runtime and sandbox seams. | V8/Deno, future Bun/JSC, wasmtime, HostBridge, OCI annotations, and microVM metadata consume scoped projections only after admission. |
| SI7 | `todo` | Add lifecycle and revocation. | Mint, renew, revoke, expiry, node compromise, and tenant cleanup paths are tested. |
| SI8 | `todo` | Publish operator runbook and conformance gate. | One command proves mint/deny/exchange/revoke and redaction behavior. |

## Consumer Rules

- Secret management may use provider-auth credentials to fetch secret values,
  but secret values remain owned by the secret-management plan.
- WASI agent HTTP/client credentials require service identity for production
  provider exchange; agent workloads must not invent identity from session IDs.
- Browser session credentials remain secret references or provider-auth
  projections, and browser operations audit against the stable workload
  identity.
- Wasmtime WIT host state carries workload identity internally; guest access to
  identity requires an explicit imported capability.
- Layered admission and metrics may carry decision IDs and low-cardinality
  identity classes, but must not use full workload IDs as metric labels.
- Horizontal scaling must bind node/machine identity to cluster membership
  before provider-auth tokens are minted on a node.

## Acceptance Criteria

- Every minted credential is short-lived and tied to an admitted
  `TenantWorkloadStableIdentity`.
- Every provider exchange records tenant ID, decision ID, workload subject,
  provider, audience, expiry, and result in tenant-safe audit events.
- Runtime code, sandbox guests, and agents cannot request arbitrary audiences,
  subjects, tenants, or token lifetimes.
- Tenant cleanup and node compromise recovery revoke or expire outstanding
  credentials through a documented path.
- No local admin token, deploy token, or raw bearer token is passed into guest
  workloads as service identity.

## References

- `docs/tenant-isolation.md`
- `docs/architecture/server/auth-runtime-trust.md`
- `docs/architecture/runtime/permission-model.md`
- `docs/plans/secret-management-plan.md`
- `docs/plans/wasi-agent-capabilities-plan.md`
- `docs/plans/wasmtime-backend-plan.md`
- `docs/plans/layered-admission-control-plan.md`
