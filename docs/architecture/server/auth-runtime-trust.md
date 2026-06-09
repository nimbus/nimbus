# Server Auth And Runtime Trust

This reference captures the landed post-Firebase, post-Cloud-Functions trust
baseline for server-owned auth, provider-family compatibility seams, runtime
bootstrap ownership, and trusted metadata contracts.

It complements:

- [runtime capability and adapter boundary](../runtime/adapter-boundary.md)
- [firebase application auth contract](../../adapters/firebase/auth-contract.md)
- [cloud functions compatibility](../../adapters/cloud-functions/compatibility.md)
- [adapter runtime trust hardening plan](../../plans/archive/adapter-runtime-trust-hardening-plan.md)
- [server runtime canonicalization plan](../../plans/archive/server-runtime-canonicalization-plan.md)

## Landed Conclusions

The current landed architecture now reflects the following settled rules:

1. Live server activation is deployment-scoped.
   Auth verifier, adapter registries, Firebase config, and generation now move
   together inside one active deployment snapshot instead of several live cells.
2. Shared application auth is server-owned; adapters consume it rather than
   owning principal normalization or bearer verification semantics.
3. Cloud Functions callable auth fails closed when a bearer token is presented
   but cannot be verified.
4. Firestore-family compatibility logic shared by Firebase and Cloud Functions
   meets on a provider-family seam rather than through adapter-to-adapter
   imports.
5. Covered Firestore-admin metadata is truthful or omitted.
6. Shared runtime bootstrap has one authoritative implementation.
7. Shared runtime capability execution is explicitly separated from runtime
   ABI payload dispatch.
8. The shared runtime document ABI is provider-neutral; Convex naming remains
   adapter-owned at the contract edge.

## Direction

- Server auth should be server-owned.
- Live deployment state should be activation-scoped and swapped atomically.
- Adapters may depend on shared auth and provider-family seams.
- Adapters should not depend on each other for compatibility translation.
- Shared runtime capability code should be provider-neutral and runtime-ABI
  aware only where explicitly named as such.
- Pre-launch direct corrections are preferred over compatibility shims.

## Tenant Workload Identity

Tenant-isolated work now uses two explicit workload identity shapes:

- `WorkloadAttributes` is the pre-admission description of the requested work:
  kind, name, runtime tier, sandbox backend, sandbox ID, and invocation ID.
- `WorkloadIdentity` is the admitted identity projection produced only from a
  `TenantIsolationDecision`. It is tenant-scoped by construction and is the
  source for provider subjects, SPIFFE paths, credential claims, and audit
  evidence.

`WorkloadIdentity.subject()` is the low-cardinality provider-policy subject. It
intentionally excludes placement and per-invocation fields:

```text
nimbus-workload:v1
  /tenant/<tenant_id>
  /deployment/<generation|none>
  /surface/<admission_surface>
  /kind/<runtime_function|service|sandbox|http_request|system_task>
  /name/<percent-escaped workload name>
  /runtime-tier/<tier|none>
  /runtime-backend/<backend|none>
  /sandbox-backend/<backend|none>
```

`WorkloadIdentity.audit_projection()` is the full evidence projection. It keeps
the same prefix and appends placement/invocation fields:

```text
nimbus-workload-audit:v1
  /tenant/<tenant_id>
  /deployment/<generation|none>
  /surface/<admission_surface>
  /kind/<runtime_function|service|sandbox|http_request|system_task>
  /name/<percent-escaped workload name>
  /runtime-tier/<tier|none>
  /runtime-backend/<backend|none>
  /sandbox-backend/<backend|none>
  /node/<node_id|none>
  /machine/<machine_id|none>
  /sandbox/<sandbox_id|none>
  /invocation/<invocation_id|none>
```

The subject can render a SPIFFE-style workload path:

```text
/nimbus/workload/v1/tenant/.../sandbox-backend/...
```

and a full SPIFFE ID when a trust domain is configured:

```text
spiffe://<trust-domain>/nimbus/workload/v1/tenant/.../sandbox-backend/...
```

Rules:

- The admitted workload identity is derived only after tenant admission; lower
  seams do not assemble it from caller-supplied tenant strings.
- Tenant ID, deployment generation, admission surface, service/function name,
  runtime tier/backend, sandbox backend, node/machine, sandbox, and invocation
  IDs are explicit fields. Fields that do not apply render as `none`, rather
  than disappearing from the schema.
- Non-path-safe bytes are percent-escaped in path segments so service names
  such as `messages:send` have deterministic identities.
- Decision fingerprints include node/machine location, so a decision admitted
  for one execution location cannot be silently reused as another admitted
  execution context.
- Future service-identity providers must derive provider-auth subjects from
  `WorkloadIdentity.subject()`. Placement and per-invocation fields
  (`node_id`, `machine_id`, `sandbox_id`, `invocation_id`, decision ID, and
  `WorkloadIdentity.audit_projection()`) belong in signed credential/audit
  claims unless a provider explicitly requires a stronger placement-bound
  subject.
- Secret-management and service-identity providers must not mint credentials
  from only `tenant_id`, raw bearer claims, or process-local runtime context.

## Tenant Isolation Audit Events

Tenant isolation telemetry uses `TenantIsolationEvent`, a structured event
projection from either an admitted `TenantIsolationDecision` or a narrow
no-decision context for pre-admission rejection, cleanup, and drift findings.
The schema version is `nimbus.tenant_isolation.event.v1`.

Event kinds are:

- `admission`
- `rejection`
- `materialization`
- `runtime_invocation`
- `sandbox_launch`
- `storage_access`
- `host_bridge_operation`
- `cleanup`
- `drift_violation`
- `lifecycle_status`

Every event carries tenant ID, surface, principal class, result, reason code,
correlation IDs, audit redaction fields, and any available decision ID,
workload subject, workload audit projection, workload kind/name, runtime tier,
sandbox ID, invocation ID, and service name. Decision-backed events derive those
fields from the admitted decision, not from caller-supplied strings.

Sensitive attributes are redacted by the event schema. Attribute and
correlation-ID keys that name bearer claims, authorization headers, cookies,
credentials, passwords, private keys, query parameters, raw bearer claims, raw
credentials, secrets, secret handles, or tokens are not serialized with
caller-provided values. The event records the redacted field path and
serializes the value as `redacted` instead. Callers may add more redacted
attributes, but they must not bypass the schema by attaching raw secrets to
another telemetry channel.

Lifecycle status events are observed evidence from node-local enforcement.
Unit names, systemd job paths, process IDs, cgroup paths, journal selectors,
node lease IDs, heartbeat IDs, and evidence correlation IDs belong in the
event payload and `_nimbus` evidence records. Metrics derived from workload
status must use low-cardinality labels such as lifecycle backend, phase, and
patch target.

`TenantIsolationEvent` is the internal canonical schema. Enterprise export
formats such as OCSF JSONL or OpenTelemetry log records are mappings from this
schema, not replacements for it. Nimbus maps events to conservative OCSF Base
Event records and OpenTelemetry log-record shaped events with low-cardinality
event names, trace/span correlation when present, and namespaced `nimbus.*`
attributes for decision ID, tenant ID, workload identity, runtime tier,
sandbox ID, invocation ID, service name, reason code, and redaction evidence.
The export, retention, and conformance owner is
`docs/plans/archive/enterprise-policy-and-sandbox-egress-plan.md`.

## Agent Auth Contract

This is a forward-looking contract for the `nimbus agent` workload class.
It locks in the auth shape **before** the implementation lands so neither
the runtime layer nor the operator console grows incidental coupling to
the local admin token. Re-read this section at the start of the
`nimbus agent` implementation plan.

### Principles

- Agents do not authenticate with the local admin token. The local admin
  token is single-tenant root authority for the operator. Handing it to
  an agent erases the boundary the operator console enforces.
- Agents authenticate with **scoped agent sessions** minted by the
  server through the same `LocalServerSecurityState` access path that
  mints operator console sessions. The mint flow is server-owned, the
  scope vocabulary is server-owned, and the storage path is the
  existing local-server audit log + session registry.
- Scoped agent sessions are revocable both individually and through a
  blanket admin-token rotation. Rotation is the kill-switch of last
  resort; targeted revocation is the normal path.

### Scoped session shape

A scoped agent session record must carry, at minimum:

1. `session_id` — opaque, prefixed (`nimbus_agent_sess_`), generated by
   the same `SecureRandom` path the operator session uses.
2. `tenant_id` — the tenant under which the agent may act. Cross-tenant
   reach must be a separate session per tenant, never a wildcard.
3. `capabilities` — a closed-set list naming the host operations the
   agent may perform (e.g. `browser.session`, `kv.read`, `kv.write`).
   No "all capabilities" sentinel.
4. `issued_at`, `expires_at`, `last_used_at` — RFC 3339 timestamps.
   Default TTL is short (≤ 12 hours) and is refreshable only by an
   explicit re-mint, never by inline extension.
5. `issued_by` — provenance of the mint (operator console, deploy
   bearer, or in-process service) so audit-log entries are traceable.
6. `parent_generation` — the local admin token generation the session
   was minted under. Rotation invalidates every session whose
   `parent_generation` is below the current generation.

The session record is stored next to the existing local-server session
table; it is not a separate trust domain. A scoped agent session that
references a stale `parent_generation` must be rejected at the
middleware layer, not just at the access layer.

### Revocation requirements

- **Targeted revocation** — `DELETE /local/agent/sessions/{id}` (or the
  equivalent host op) revokes one session. The session must be removed
  from the in-memory registry AND must surface a revocation event in
  the audit log before the response returns.
- **Blanket revocation** — `nimbus auth rotate-admin` (and the live
  `rotate_and_persist_token_with_outcome` path) must clear scoped
  agent sessions alongside operator sessions and launch tickets. This
  is already the documented behavior for operator sessions; the agent
  surface inherits it.
- **No silent renewal** — expired sessions never auto-renew. A request
  presenting an expired session id must return a structured error the
  agent runtime can surface as a deliberate re-auth signal.

### Audit-log requirements

Every scoped-session lifecycle event must produce a single audit-log
entry through the existing `LocalServerAuditLog` channel:

- `agent_session_minted` — records issuer, tenant, capability set,
  and parent generation.
- `agent_session_used` — records the host operation requested and
  whether the access layer accepted it. Use is the high-volume event;
  prefer sampled or aggregate emission if the per-event volume becomes
  a hot path, but keep at least one entry per session per operation
  class.
- `agent_session_revoked` — records the cause (`targeted`, `rotation`,
  `expiry`), the revoking principal, and the session id.

These three event names are part of the contract. Future agent
implementations may add fields but must not rename or split these
buckets without bumping the audit-log schema version.

### References

- Operator session shape and rotation:
  `crates/nimbus-server/src/local_server/access.rs`
- Audit log channel and event envelope:
  `crates/nimbus-server/src/local_server/audit.rs`
- Local admin token rotation + freshness gate:
  `crates/nimbus-server/src/local_server/token.rs` and
  `crates/nimbus-bin/src/start/network_bind.rs`
- Forward plan that consumes this contract (when promoted):
  `docs/plans/agent-browser-service-plan.md`
