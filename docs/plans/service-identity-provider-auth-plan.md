# Plan: Service Identity And Provider Auth

Canonical deferred design and execution plan for workload identity minting and
provider-auth exchange in Nimbus.

The tenant-isolation enterprise hardening work introduced
`TenantWorkloadStableIdentity`, an admitted workload/audit projection derived
from a `TenantIsolationDecision`. This plan owns the next layer:
low-cardinality provider subjects, short-lived credentials, and service identity.
Secret management owns secret values and references; this plan owns identity
minting and provider authentication.

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** promote when a concrete provider integration needs
  workload identity rather than static secrets: Vault JWT/Kubernetes auth,
  AWS IRSA/OIDC, GCP Workload Identity Federation, Azure federated
  credentials, SPIFFE/SPIRE SVIDs, mTLS client certificates, or signed
  service-account tokens.
- **Canonical source:** `TenantWorkloadStableIdentity`
- **Provider subject:** stable workload projection defined by SI0;
  per-invocation and placement fields become credential claims, not the cloud
  provider allow-policy subject.
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
- local and enterprise trust-domain configuration, including a local-dev
  fallback that does not require SPIRE
- OIDC/JWT minting for workload identity
- SPIFFE-compatible SVID paths and future SPIRE registration shape
- workload selectors, node attestation inputs, SVID rotation, mTLS client
  certificates, and service-account token exchange
- node/machine identity source, rotation, compromise recovery, and membership
  binding
- provider-auth adapters for Vault, Kubernetes, AWS, GCP, and Azure, including
  provider-specific subject/audience claim mapping
- runtime and sandbox propagation of scoped identity projections
- audit events for mint, renew, revoke, deny, provider exchange, and
  downstream secret-read correlation

This plan does not own:

- secret value storage, versioning, references, or rotation values
- runtime permission semantics beyond the `identity` grant
- cluster membership transport
- browser policy credentials or WASI agent HTTP allowlists

## Dependency Posture

Use the dependency audit at
`docs/plans/research/service-identity-provenance-dependency-audit.md` before
promoting implementation. The intended dependency posture is:

- Reuse iroh endpoint identity as a transport-peer input only. It can help bind
  a connection to a cluster node, but it is not a tenant workload identity and
  must not authorize provider credentials by itself.
- Use SPIFFE/SPIRE through a Workload API adapter for production SVIDs when
  available. Local development may use a Nimbus local issuer, but that fallback
  must stay explicitly non-production.
- Prefer `jsonwebtoken`/`openidconnect` for new JWT/OIDC minting or verification
  code, and avoid expanding the existing Convex compatibility verifier's
  hand-coded `ring` path into provider-auth infrastructure.
- Keep AWS, GCP, Azure, Vault, and Kubernetes client libraries adapter-local and
  feature-gated. Runtime crates receive identity projections only; they do not
  link cloud/provider SDKs.
- Wrap token-bearing values in zeroizing or secret wrapper types and keep raw
  token strings out of debug output, audit payloads, metrics, and guest-visible
  context.

## Identity Contract

The provider-auth subject is a stable workload projection derived from the
admitted `TenantWorkloadStableIdentity`, not the full decision/audit string.
It intentionally excludes placement and per-credential fields such as
`node_id`, `machine_id`, `sandbox_id`, and `invocation_id` so cloud provider
policies do not need to change on every invocation or reschedule:

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
```

When a SPIFFE-compatible trust domain is configured, the same subject can be
rendered as:

```text
spiffe://<trust-domain>/nimbus/workload/v1/tenant/.../sandbox-backend/...
```

Credential instances then carry signed, short-lived claims for correlation and
placement binding:

```text
sub=<stable workload subject>
aud=<provider audience>
exp=<short ttl>
jti=<credential instance id>
nimbus_decision_id=<tenant isolation decision id>
nimbus_workload_stable_id=<full admitted audit projection>
nimbus_node_id=<node_id|none>
nimbus_machine_id=<machine_id|none>
nimbus_sandbox_id=<sandbox_id|none>
nimbus_invocation_id=<invocation_id|none>
```

Provider adapters must use the stable subject or a signed projection of it for
provider allow policies. They may use placement and invocation claims as
additional proof or audit inputs, but must not reconstruct identity from caller
inputs.

## Topic Coverage

This plan is expected to cover the follow-up topics below when promoted:

| Topic | Required coverage |
| --- | --- |
| Service identity | Stable workload subject issuance; short-lived credential forms; OIDC/JWT, SPIFFE SVID, mTLS certificate, and service-account token outputs; TTL, renewal, revocation, and redaction behavior. |
| Identity grants vs secret grants | `identity` grants authorize identity projection or credential minting; `secret` grants authorize secret handle reads. A workload with one grant does not implicitly receive the other. |
| SPIFFE/SPIRE | Trust-domain shape, SPIFFE ID path convention, workload selectors, node attestation, SVID rotation, and local-dev fallback without SPIRE. |
| Secret-provider auth | Vault Kubernetes/JWT/OIDC auth; AWS/GCP/Azure workload identity mapping; provider-specific subject/audience claims; per-tenant secret-store routing from the secret-management plan; audit correlation between credential mint and secret read. |
| Node and machine identity | Canonical `node_id` and `machine_id`; identity source for local machines, Linux hosts, microVM guests, and future cluster nodes; key rotation and compromised-node recovery. |
| Runtime identity propagation | V8/Deno/Node, future Bun/JSC, and Wasm receive only scoped identity context; `HostBridge` rechecks the admitted identity projection on every host operation; raw service tokens do not enter guests or runtimes unless explicitly minted by this plan. |
| Sandbox identity propagation | Server-owned OCI annotations/labels, microVM metadata, cgroup or sandbox audit labels, and denial of tenant-controlled identity fields. |
| Audit and observability | Stable workload identity and decision ID in admission, runtime, sandbox, storage, HostBridge, mint, exchange, and secret-read events; redaction rules and enterprise evidence trail. |
| Policy admission | Matrix of which identities can request secret, service, network, and storage grants; deny-by-default behavior; conformance tests for forged tenant/workload/node identities. |

## Phase Ledger

| Phase | Status | Goal | Verification |
| --- | --- | --- | --- |
| SI0 | `todo` | Define provider-auth policy input, stable workload subject projection, per-credential claim set, and audit schema. | Tests prove identity mint requests without an admitted decision fail closed, forged tenant/workload/node identities are denied, and provider policy subjects do not include invocation IDs. |
| SI1 | `todo` | Promote `identity` grant to enforced capability. | Runtime and sandbox tests prove identity APIs deny without explicit grants. |
| SI2 | `todo` | Add canonical node/machine identity, local-dev trust-domain fallback, and signing-key management. | Key rotation, stale-key denial, canonical `node_id`/`machine_id`, and local-dev fallback tests pass without exposing private key material. |
| SI3 | `todo` | Implement short-lived OIDC/JWT minting. | Tokens carry stable workload subject, audience, expiry, decision ID, credential instance ID, and optional placement/invocation claims; wrong audience/provider denies. |
| SI4 | `todo` | Define SPIFFE/SVID integration path. | SPIFFE IDs render from the stable workload subject projection; registration entries bind workload selectors, node attestation inputs, and node/machine selectors; SVID rotation is tested. |
| SI5 | `todo` | Add provider-auth adapters and per-tenant secret-store routing hooks. | Vault, Kubernetes, AWS, GCP, and Azure adapters are either implemented or gated by precise provider blockers; subject/audience mapping and mint-to-secret-read audit correlation are tested. |
| SI6 | `todo` | Propagate identity projections to runtime and sandbox seams. | V8/Deno/Node, future Bun/JSC, wasmtime, HostBridge, OCI annotations/labels, microVM metadata, and cgroup/sandbox audit labels consume server-owned scoped projections only after admission. |
| SI7 | `todo` | Add lifecycle and revocation. | Mint, renew, revoke, expiry, node compromise, and tenant cleanup paths are tested. |
| SI8 | `todo` | Publish operator runbook and conformance gate. | One command proves mint/deny/exchange/revoke, forged-identity denial, secret-read correlation, and redaction behavior. |

## Consumer Rules

- Secret management may use provider-auth credentials to fetch secret values,
  but secret values and per-tenant secret-store routing remain owned by the
  secret-management plan.
- WASI agent HTTP/client credentials require service identity for production
  provider exchange; agent workloads must not invent identity from session IDs.
- Browser session credentials remain secret references or provider-auth
  projections, and browser operations audit against the stable workload
  identity.
- Wasmtime WIT host state carries workload identity internally; guest access to
  identity requires an explicit imported capability.
- HostBridge implementations must recheck tenant, workload, grant, and
  provider policy on every identity-sensitive host operation instead of trusting
  identity context echoed back by guest/runtime code.
- Layered admission and metrics may carry decision IDs and low-cardinality
  identity classes, but must not use full workload IDs as metric labels.
- Horizontal scaling must bind node/machine identity to cluster membership
  before provider-auth tokens are minted on a node.

## Acceptance Criteria

- Every minted credential is short-lived and tied to an admitted
  `TenantWorkloadStableIdentity` through a stable workload subject plus signed
  decision, placement, and invocation claims.
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
