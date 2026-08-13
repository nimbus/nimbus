# NNC7.3 Portable Provider Status Handles

Status: `in_progress; read-only audit next`

## Outcome

Return portable endpoint and attachment handles from sandbox and machine
status. An address change must not change resource identity. Provider handles
must remain opaque and redacted outside their effect owner.

## Recovery checkpoint

| Field | Value |
| --- | --- |
| Dependency | NNC7.2 is complete in the commit that starts this item. |
| Current scope | Read-only trace of sandbox and machine status carriers, provider evidence, address-derived identity, redaction, and current consumers. |
| Owned paths | This proof plus the concise plan and routing transition. No product path is owned yet. |
| Forbidden paths and seams | No provider effect, transport, tenant policy, logical naming, NNC7.4 projection schema, or NNC8 recovery change. |
| Acceptance | Address change does not change resource identity. Provider handles remain opaque and redacted. |
| Last green | NNC7.2 item gates are green and its review cadence is exhausted. |
| Next action | Trace current status types and consumers. Freeze the current and target seam plus one deterministic fail-before case before product edits. |
| Blocker | none |

The audit must first distinguish portable identity from provider evidence and
observed location. It must preserve concrete Netavark, gvproxy, WSL2, sandbox,
and machine effects in their current owners. It must not move logical service
naming or observed system projection authority into `nimbus-network`.
