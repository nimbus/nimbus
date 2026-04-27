# Error Schema

This document defines the structured Neovex error contract for HTTP, WebSocket
session failures, and per-operation failures.

Schema source for the examples below:

- [error-envelope.schema.json](schemas/error-envelope.schema.json)

## Goals

- one machine-stable shape across transports
- explicit retryability
- explicit severity so clients can separate session-fatal failures from
  operation failures
- stable public `code` taxonomy that can outlive internal Rust enum reshuffles

## Envelope

HTTP responses use a top-level envelope:

```json
{
  "error": {
    "code": "protocol.no_overlap",
    "message": "Server does not support protocol neovex.v3.",
    "requestId": "req_01HX3PKGZT2S7Z8K4M3NQ3D2QF",
    "timestamp": "2026-04-26T12:34:56.789Z",
    "severity": "fatal",
    "retryable": false,
    "detail": {
      "serverSupports": [
        "neovex.v2"
      ],
      "clientOffered": [
        "neovex.v3"
      ]
    },
    "remediation": {
      "action": "upgrade_server",
      "message": "Update Neovex to a version that supports the requested protocol."
    }
  }
}
```

WebSocket fatal frames embed the same error object:

```json
{
  "type": "fatal_error",
  "error": {
    "code": "protocol.hello_timeout",
    "message": "Client did not send client_hello within 10 seconds.",
    "requestId": "req_01HX3Q4PJ6JVJYVN1T3QWJ4Y5W",
    "timestamp": "2026-04-26T12:35:12.123Z",
    "severity": "fatal",
    "retryable": true,
    "detail": {
      "timeoutMs": 10000
    },
    "remediation": {
      "action": "retry",
      "message": "Reconnect and send client_hello immediately after hello."
    }
  }
}
```

Per-operation WebSocket errors do the same:

```json
{
  "type": "op.error",
  "id": "req_01HX3Q0A2D6Q2V0T0B8Y0C4M8E",
  "error": {
    "code": "op.invalid_input",
    "message": "Query table name is required.",
    "requestId": "req_01HX3Q0A2D6Q2V0T0B8Y0C4M8E",
    "timestamp": "2026-04-26T12:34:56.789Z",
    "severity": "error",
    "retryable": false,
    "detail": {
      "field": "table"
    },
    "remediation": {
      "action": "fix_request",
      "message": "Populate the table field before retrying."
    }
  }
}
```

## Field Contract

| Field | Required | Rule |
| --- | --- | --- |
| `code` | yes | machine-stable dotted namespace such as `protocol.no_overlap` |
| `message` | yes | human-readable, may change, never parse client-side |
| `requestId` | yes | stable per failing request, operation, or session transition |
| `timestamp` | yes | RFC 3339 UTC timestamp with millisecond precision |
| `severity` | yes | one of `fatal`, `error`, `warning` |
| `retryable` | yes | explicit boolean; clients must not infer from `code` |
| `detail` | yes | code-specific structured payload, object or `null` |
| `remediation` | no | UI/action hint for “fix this” flows |

### Severity

| Severity | Meaning | Typical client reaction |
| --- | --- | --- |
| `fatal` | session or request path cannot continue | close view, reconnect, or escalate |
| `error` | one operation failed but session may continue | surface inline error and keep session alive |
| `warning` | operation succeeded with caveat | show non-blocking notice |

### Remediation Actions

Current action vocabulary:

- `retry`
- `reauthenticate`
- `upgrade_client`
- `upgrade_server`
- `fix_request`
- `wait_and_retry`
- `contact_operator`

## Channel Wrapping Rules

### HTTP

- status code remains transport-appropriate
- body is `{ "error": { ... } }`

### WebSocket before upgrade

- the server returns an HTTP error response with the same envelope

### WebSocket after upgrade

- session-fatal failures use `fatal_error`
- request-scoped operation failures use `op.error`
- session-scoped non-fatal failures use `error`

## Code Namespaces

| Namespace | Purpose |
| --- | --- |
| `auth.*` | authentication or authorization failures |
| `protocol.*` | handshake, framing, version negotiation, unsupported message families |
| `rate.*` | bounded queue, resource exhaustion, or throttling behavior |
| `session.*` | tenant/session lifecycle or session-scoped subscription failures |
| `op.*` | request validation and domain-operation failures |
| `machine.*` | machine/service-control errors surfaced to UI clients |
| `service.*` | server/internal/storage availability or corruption failures |

## Canonical Mappings

This table is the required public mapping for the current `neovex_core::Error`
and `neovex_server::AppError` surfaces.

| Internal variant | Code | Severity | Retryable |
| --- | --- | --- | --- |
| `AppError::Unauthorized` | `auth.unauthorized` | `error` | false |
| `AppError::Forbidden` | `auth.forbidden` | `error` | false |
| `AppError::NotFound` | `service.route_not_found` | `error` | false |
| `Error::Cancelled` | `op.cancelled` | `error` | true |
| `Error::TenantNotFound` | `session.tenant_not_found` | `error` | false |
| `Error::DocumentNotFound` | `op.document_not_found` | `error` | false |
| `Error::ScheduledJobNotFound` | `op.scheduled_job_not_found` | `error` | false |
| `Error::AlreadyExists` | `op.already_exists` | `error` | false |
| `Error::ResourceExhausted` | `rate.resource_exhausted` | `error` | true |
| `Error::PermissionDenied` | `auth.permission_denied` | `error` | false |
| `Error::Conflict` | `op.conflict` | `error` | false |
| `Error::InvalidInput` | `op.invalid_input` | `error` | false |
| `Error::SchemaValidation` | `op.schema_validation` | `error` | false |
| `Error::SchemaNotFound` | `op.schema_not_found` | `error` | false |
| `Error::Storage { kind: Busy }` | `service.storage_busy` | `error` | true |
| `Error::Storage { kind: Transient }` | `service.storage_transient` | `error` | true |
| `Error::Storage { kind: Unavailable }` | `service.unavailable` | `error` | true |
| `Error::Storage { kind: Corruption }` | `service.storage_corruption` | `fatal` | false |
| `Error::Storage { kind: Io }` | `service.storage_io` | `error` | true |
| `Error::Storage { kind: Other }` | `service.storage_other` | `error` | false |
| `Error::Serialization` | `service.serialization` | `error` | false |
| `Error::Internal` | `service.internal` | `fatal` | false |

## Protocol-Specific Codes

These codes are owned by the WebSocket protocol plan even before every server
path is implemented:

| Code | Meaning |
| --- | --- |
| `protocol.no_overlap` | client offered subprotocols but none overlap server support |
| `protocol.hello_timeout` | `client_hello` did not arrive within the negotiated deadline |
| `protocol.unsupported_version` | frame references an unknown negotiated protocol |
| `protocol.invalid_json` | text frame is not valid JSON |
| `protocol.unsupported_message_type` | frame `type` is unknown for the negotiated protocol |
| `protocol.unsupported_binary` | binary frame is not supported for this protocol |

## Detail Payload Guidance

`detail` is code-owned. Suggested stable keys for the first hardening slice:

- `protocol.no_overlap`
  - `serverSupports: string[]`
  - `clientOffered: string[]`
- `protocol.hello_timeout`
  - `timeoutMs: number`
- `op.invalid_input`
  - `field?: string`
  - `reason?: string`
- `rate.resource_exhausted`
  - `resource: string`
  - `limit?: number`
- `service.unavailable`
  - `component?: string`
  - `storageKind?: string`

## Client Rendering Contract

- Key product behavior off `severity` and `retryable`, not off HTTP status code
  alone.
- Show `message` directly to operators and developers.
- Use `code` for analytics, filtering, and support links.
- Use `remediation.action` only as a hint. Unknown actions must degrade
  gracefully.

## Example Validation Coverage

The JSON examples in this document are intended to validate against
[error-envelope.schema.json](schemas/error-envelope.schema.json).
That schema is example-oriented and documents the stable public shape rather
than replacing transport-specific server validation.
