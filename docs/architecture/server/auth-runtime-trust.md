# Server Auth And Runtime Trust

This reference captures the landed post-Firebase, post-Cloud-Functions trust
baseline for server-owned auth, provider-family compatibility seams, runtime
bootstrap ownership, and trusted metadata contracts.

It complements:

- [runtime capability and adapter boundary](runtime-adapter-boundary.md)
- [firebase application auth contract](firebase-auth-contract.md)
- [cloud functions compatibility](cloud-functions-compatibility.md)
- [adapter runtime trust hardening plan](../plans/adapter-runtime-trust-hardening-plan.md)
- [server runtime canonicalization plan](../plans/server-runtime-canonicalization-plan.md)

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

Tenant-isolated work now has one canonical workload identity projection:
`TenantWorkloadStableIdentity`, produced from an admitted
`TenantIsolationDecision`.

The stable ID format is:

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

The same projection can render a SPIFFE-style path:

```text
/nimbus/workload/v1/tenant/.../invocation/...
```

and a full ID when a trust domain is configured:

```text
spiffe://<trust-domain>/nimbus/workload/v1/tenant/.../invocation/...
```

Rules:

- The stable workload identity is derived only after tenant admission; lower
  seams do not assemble it from caller-supplied tenant strings.
- Tenant ID, deployment generation, admission surface, service/function name,
  runtime tier/backend, sandbox backend, node/machine, sandbox, and invocation
  IDs are explicit fields. Fields that do not apply render as `none`, rather
  than disappearing from the schema.
- Non-path-safe bytes are percent-escaped in path segments so service names
  such as `messages:send` have deterministic identities.
- Decision fingerprints include node/machine location, so a decision admitted
  for one execution location cannot be silently reused as a different
  provider-auth subject.
- Future secret-management and service-identity providers must use this
  stable workload identity as the provider-auth subject. They should not mint
  credentials from only `tenant_id`, raw bearer claims, or process-local
  runtime context.

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
