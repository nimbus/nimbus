# NNC7.2 Service Endpoint Generation

Status: `in_progress; read-only substitution audit next`

## Outcome

Preserve service-owned logical naming and readiness while service resolution
carries the stable endpoint identity and generation that `nimbus-network`
already owns. A stale endpoint generation must fail closed without moving name,
readiness, policy, socket, or provider-effect authority.

## Initial recovery checkpoint

| Field | Value |
| --- | --- |
| Dependency | NNC7.1a is complete in the preceding item commit. |
| Current scope | Read-only call-graph and type audit of service publication, resolution, readiness, endpoint generation, and stale-update rejection. |
| Dirty product paths | none |
| Owned paths | This proof and the concise NNC7.2 recovery/ledger rows until the audit freezes exact product paths. |
| Forbidden paths and seams | No `nimbus-network` effect, logical name provider, tenant policy, ingress transport, projection work from NNC7.4-NNC7.5, or machine/sandbox status work from NNC7.3. |
| Acceptance | Service resolution tests remain services-owned. A deterministic stale endpoint-generation test fails before correction and passes after the exact consumer carries and authenticates stable identity plus generation. |
| Last green | NNC7.1a item gates. NNC7.2 has no product change yet. |
| Next action | Trace current service name, readiness, publication, and resolution types and callers. Record the current-versus-target seam and exact fail-before case before product edits. |
| Blocker | none |
