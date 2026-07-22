---
title: Native API errors
description: The structured error envelope, error code catalog, and HTTP status mappings for the native API.
sidebar:
  label: Errors
  order: 3
---

Every native API error — HTTP response or WebSocket frame — carries one
structured error object. Clients should branch on `code`, retry when
`retryable` is true, and treat `message` as human-readable text that may
change between releases.

## Envelope

HTTP error responses wrap the error object in an envelope:

```json
{
  "error": {
    "code": "op.missing_index",
    "message": "no enabled index covers fields [state, rank]",
    "requestId": "req-...",
    "timestamp": "2026-06-10T17:03:21Z",
    "severity": "error",
    "retryable": false,
    "detail": { "fields": ["state", "rank"] },
    "remediation": {
      "action": "create_index",
      "message": "Create an index covering the required fields, then retry."
    }
  }
}
```

WebSocket `error`, `op.error`, and `fatal_error` frames embed the same
error object under their `error` field.

| Field | Type | Meaning |
| --- | --- | --- |
| `code` | string | Stable machine-readable code, dot-namespaced |
| `message` | string | Human-readable description; not stable |
| `requestId` | string | Server-assigned id for correlating logs |
| `timestamp` | string | RFC 3339 time the error was produced |
| `severity` | string | `fatal`, `error`, or `warning` |
| `retryable` | boolean | Whether retrying the same request can succeed |
| `detail` | object or null | Code-specific structured context; commit-path errors include `retryability` |
| `remediation` | object | Optional; `{"action": "<verb>", "message": "..."}` |

Remediation `action` values: `retry`, `wait_and_retry`, `fix_request`, `fix_function`,
`reauthenticate`, `refresh_resource`, `create_index`, `upgrade_client`,
`upgrade_server`, `contact_operator`.

## Code namespaces

| Prefix | Meaning |
| --- | --- |
| `auth.*` | Authentication and authorization |
| `protocol.*` | WebSocket protocol negotiation and handshake |
| `op.*` | A specific operation failed; the request itself is at fault or the target is missing |
| `session.*` | Session- or tenant-level conditions |
| `rate.*` | Capacity and rate limiting |
| `runtime.*` | Function runtime deadlines, stalls, and execution limits |
| `service.*` | Server-side infrastructure conditions |

## HTTP error codes

| Code | Status | Retryable | Detail | Notes |
| --- | --- | --- | --- | --- |
| `auth.unauthorized` | 401 | no | — | Missing or invalid credential |
| `auth.forbidden` | 403 | no | — | Credential valid, access denied |
| `auth.permission_denied` | 403 | no | — | Operation not permitted for principal |
| `op.invalid_input` | 400 | no | — | Malformed request payload or path value |
| `op.cancelled` | 408 | yes | — | Request cancelled before completion |
| `runtime.execution_timeout` | 408 | no | `{"timeoutKind":"execution", "timeoutMs"}` | Function exhausted its execution-time budget; reduce work or increase the configured limit |
| `runtime.system_timeout` | 408 | no | `{"timeoutKind":"system", "timeoutMs"}` | Function exhausted its end-to-end wall-time budget; ensure returned promises settle and background work completes within the configured limit |
| `runtime.promise_stalled` | 422 | no | — | A returned promise cannot settle because the event loop is idle; fix the function so the promise has a reachable resolution or rejection path |
| `op.document_not_found` | 404 | no | `{"documentId"}` | — |
| `op.scheduled_job_not_found` | 404 | no | `{"jobId"}` | — |
| `op.schema_not_found` | 404 | no | `{"table"}` | — |
| `op.not_found` | 404 | no | — | Generic missing resource |
| `op.conflict` | 409 | varies | `{"conflictingSequence"?, "attempts"?, "retryability"}` | Retryable optimistic conflict or terminal generic conflict |
| `op.out_of_retention` | 409 | yes | `{"minimumSequence"?, "retryability": "restart_transaction"}` | Restart from a fresh snapshot |
| `op.cap_exceeded` | 400 | no | `{"cap", "observed", "limit", "retryability": "terminal"}` | Reduce deterministic mutation usage |
| `op.already_exists` | 409 | no | — | Resource already exists |
| `op.precondition_failed` | 412 | no | — | Stale generation or resource version; refresh and retry |
| `op.missing_index` | 412 | no | `{"fields"}` | No enabled index covers the query |
| `op.schema_validation` | 422 | no | — | Document violates the active table schema |
| `op.historical_read` | varies | varies | `{"historicalReadKind"}` | See below |
| `session.tenant_not_found` | 404 | no | `{"tenantId"}` | — |
| `service.route_not_found` | 404 | no | — | No such endpoint |
| `rate.resource_exhausted` | 429 | yes | — | Wait for capacity to recover |
| `rate.overloaded` | 429 | yes | `{"retryability": "retryable_after_backoff"}` | Admission capacity is temporarily exhausted |
| `rate.committer_full` | 429 | yes | `{"capacity", "retryability": "retryable_after_backoff"}` | Committer inbox is full |
| `rate.rejected_before_execution` | 429 | yes | `{"retryability": "retryable"}` | Guaranteed not to have started |
| `rate.limited` | 429 | yes | `{"retryAfterMs", "retryability": "retryable_after_backoff"}` | Tenant write-rate limit |
| `service.storage_busy` | 503 | yes | `{"storageKind"}` | — |
| `service.storage_transient` | 503 | yes | `{"storageKind"}` | — |
| `service.unavailable` | 503 | yes | `{"storageKind"}` | Storage backend unavailable |
| `service.transport` | 503 | yes | — | Connection or transport failure |
| `service.storage_io` | 500 | yes | `{"storageKind"}` | — |
| `service.storage_corruption` | 500 | no | `{"storageKind"}` | Severity `fatal`; operator intervention required |
| `service.storage_other` | 500 | no | `{"storageKind"}` | — |
| `service.serialization` | 500 | no | — | — |
| `service.internal` | 500 | no | — | Severity `fatal`; the fixed public message is correlated to server diagnostics by `requestId` |

## Commit-path taxonomy

`detail.retryability` is the stable instruction for commit-path failures. The
top-level `retryable` boolean is convenient for generic clients; the detailed
value tells a transaction-aware client how to retry.

| Class | Native code | Meaning | Retryability | Client action |
| --- | --- | --- | --- | --- |
| Conflict | `op.conflict` | A newer commit invalidated an observed or derived dependency. | `retryable` | Retry the complete mutation after the bounded server retry budget is exhausted. |
| Overloaded / committer full | `rate.overloaded`, `rate.committer_full` | A bounded server queue has no capacity. | `retryable_after_backoff` | Back off, then retry. |
| Rejected before execution | `rate.rejected_before_execution` | Nimbus guarantees the invocation never started. | `retryable` | It is idempotent-safe to retry immediately. |
| Rate limited | `rate.limited` | The tenant write-rate window is full. | `retryable_after_backoff` | Wait at least `retryAfterMs`, then retry. |
| Out of retention | `op.out_of_retention` | The transaction snapshot is older than retained validation history. | `restart_transaction` | Discard the transaction and restart from a fresh snapshot. |
| Cap exceeded | `op.cap_exceeded` | The mutation deterministically exceeded a prepare-time resource cap. | `terminal` | Reduce the mutation's reads or writes; repeating the same request will fail. |

Transport adapters translate these classes into their documented protocol
vocabulary, but preserve the same distinction between retryable environmental
conditions and terminal request errors.

### `op.historical_read`

The status and retryability depend on `detail.historicalReadKind`:

| `historicalReadKind` | Status | Retryable |
| --- | --- | --- |
| `unsupported_backend`, `unsupported_adapter` | 501 | yes |
| `policy_snapshot_missing` | 403 | no |
| `snapshot_unavailable` | 503 | no |
| `cursor_mismatch`, `format_mismatch`, `retention_expired`, `timestamp_out_of_range` | 400 | no |

## WebSocket error codes

Handshake violations are fatal: the server sends a `fatal_error` frame and
closes the connection with close code `1008`, using the error code as the
close reason.

| Code | Fatal | Detail | Trigger |
| --- | --- | --- | --- |
| `protocol.no_overlap` | yes (HTTP 400, pre-upgrade) | `{"serverSupports", "clientOffered"}` | No supported subprotocol offered |
| `protocol.hello_timeout` | yes | `{"timeoutMs"}` | No `client_hello` within the timeout |
| `protocol.invalid_json` | during handshake | — | Invalid JSON; after the handshake it is a non-fatal `error` frame |
| `protocol.unsupported_message_type` | yes | `{"receivedType", "expectedType"}` | First frame was not `client_hello` |
| `protocol.unsupported_version` | yes | `{"receivedProtocol"}` | `client_hello` named another protocol |
| `protocol.unsupported_binary` | yes | — | Binary frame during handshake |
| `op.failed` | no | — | Subscription registration or evaluation failed (`op.error` frame) |
| `session.subscription_error` | no | — | Subscription stream error without a `request_id` |
| `session.unsubscribe_failed` | no | — | Unsubscribe teardown failed |
| `auth.unauthorized` | no | — | `authenticate` sent on the native route |

Frame shapes are defined in the
[WebSocket protocol reference](/reference/native/websocket-protocol/);
endpoint behavior is in the
[HTTP API reference](/reference/native/http-api/).
