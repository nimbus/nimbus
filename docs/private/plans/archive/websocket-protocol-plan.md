# Plan: WebSocket Protocol Hardening

Canonical execution plan for versioned WebSocket protocol negotiation and a
unified error schema across all Nimbus client surfaces (SDK, CLI, UI, future
Electron shell). Produces two reference documents and the server-side
implementation.

Reviewed against:

- `crates/nimbus-server/src/ws/mod.rs` — current WebSocket upgrade handler
- `crates/nimbus-server/src/protocol.rs` — current frame types
  (`ClientMessage::Authenticate`, `Subscribe`, `Unsubscribe`;
  `ServerMessage::Authenticated`, `AuthError`, `SubscriptionResult`, `Error`)
- `crates/nimbus-server/src/state.rs` — `AppState` error handling
- `packages/nimbus/src/browser.ts` — `NimbusClient` WebSocket connection,
  `ConnectionState` tracking, reconnection logic

---

## Status

- **Status:** `done`
- **Primary owner:** this plan
- **Activation gate:** prerequisite for `docs/plans/desktop-ui-plan.md`
- **Related plans:**
  - `docs/plans/desktop-ui-plan.md` — consumes the protocol spec and error
    schema; depends on this plan completing before UI implementation
  - `docs/plans/archive/localhost-server-security-plan.md` — middleware ordering
    references the protocol negotiation layer
  - `docs/plans/native-transport-evolution-plan.md` — future follow-on for
    native codec negotiation and WebTransport evaluation; this WebSocket plan
    remains the owner of version negotiation, handshake, and structured error
    schema

## Current Assessed State

- The WebSocket upgrade handler at `ws/mod.rs` accepts connections with
  tenant extraction from `X-Tenant-Id` header or query parameter. No
  subprotocol negotiation, no hello/client_hello exchange.
- Frame types in `protocol.rs` define `ClientMessage` (Authenticate,
  ClearAuth, Subscribe, Unsubscribe) and `ServerMessage` (Authenticated,
  AuthError, SubscriptionResult, Error). These are Convex-compatible but
  unversioned.
- Error responses use `AppError` with HTTP status mapping (404, 409, 429,
  403, 400, 422, 503, 500) but no structured error envelope with codes,
  severity, or remediation.
- The JS SDK has reconnection logic and `ConnectionState` tracking but no
  protocol version awareness.

## Control Plan Rules

1. The protocol spec is written as a reference document **before**
   implementation code.
2. Existing Convex-compatible framing is preserved as `nimbus.v1` — no
   breaking changes to current clients.
3. Error codes are public API — once shipped, they are never renamed, only
   deprecated.
4. Optional native binary codecs and optional WebTransport are out of scope for
   this plan. If future work needs them, it must land through
   `docs/plans/native-transport-evolution-plan.md` rather than expanding this
   plan's ownership.

## Verification Contract

Each roadmap item must satisfy before closing:

- `cargo fmt --all --check` — clean
- `make clippy` — clean
- `make test` — green
- Manual verification described per item

## Architecture

### Subprotocol negotiation

Client sends preferences via `Sec-WebSocket-Protocol` header (RFC 6455):

```
GET /ws HTTP/1.1
Upgrade: websocket
Sec-WebSocket-Protocol: nimbus.v2, nimbus.v1
```

Server picks the highest supported overlap, echoes it in the upgrade
response. If no overlap, reject with HTTP 400 and a structured error body.

### Hello / client_hello handshake

Post-upgrade, server sends `hello` as the first frame:

```json
{
  "type": "hello",
  "protocol": "nimbus.v2",
  "server": { "version": "0.2.3", "build": "git:abc123" },
  "features": ["machine.v1", "runtime.v2", "storage.indexes.v1"],
  "session": { "id": "s_01HX...", "serverNow": 1713571200000 }
}
```

Client replies within 10 seconds:

```json
{
  "type": "client_hello",
  "protocol": "nimbus.v2",
  "client": { "kind": "browser", "version": "0.2.3" },
  "capabilities": ["queries.v1", "mutations.v1", "subscriptions.v1"]
}
```

Features are individually negotiated. Missing features produce per-operation
errors so clients degrade gracefully rather than failing the connection.

### Session loop invariants

- Single-writer on the WebSocket via bounded `mpsc` outbox (256 frames).
- `biased;` select with shutdown first — busy subscriptions cannot starve
  graceful shutdown.
- **Ordering guarantee**: for a mutation M affecting queries Q1..Qn, emit
  `query.result(Q1..Qn)` **before** `mutation.result(M)` on the same socket.
  This makes optimistic UI flicker-free.
- **Backpressure**: per-query "latest value wins" dedup in the outbox. Event
  streams use sequence numbers so clients detect drops.

### Error schema

One shape everywhere — HTTP bodies, WebSocket close payloads, per-op errors:

```json
{
  "error": {
    "code": "protocol.no_overlap",
    "message": "Server does not support protocol nimbus.v3.",
    "requestId": "req_01HX3PKGZT...",
    "timestamp": "2026-04-18T12:34:56.789Z",
    "severity": "fatal",
    "retryable": false,
    "detail": { "serverSupports": ["nimbus.v1", "nimbus.v2"], "clientOffered": ["nimbus.v3"] },
    "remediation": { "action": "upgrade_server", "message": "Update Nimbus to match this client." }
  }
}
```

| Field | Rule |
| --- | --- |
| `code` | Machine-stable, snake_case, dotted namespace. Public API — never rename. |
| `message` | Human-readable. May change between versions. Never parse client-side. |
| `requestId` | Always present. Correlates with server logs for bug reports. |
| `severity` | `fatal` (session done — reconnect), `error` (this op failed), `warning` (succeeded with caveat). Application-level extension not from RFC 9457/gRPC/GraphQL; justified by client need to distinguish connection-level from operation-level failures. |
| `retryable` | Explicit boolean. Client must not infer from code. |
| `detail` | Per-code typed payload. Schema documented alongside the code. |
| `remediation` | Optional. `action` is an enum for client "Fix this" buttons. |

Error code namespaces: `auth.*`, `protocol.*`, `rate.*`, `session.*`, `op.*`,
`machine.*`, `service.*`.

## Roadmap

### WP1 — Spec: WebSocket protocol reference document

Write `docs/adapters/native/websocket-protocol.md`. Covers: subprotocol
negotiation, hello/client_hello frames, op types (query.subscribe, mutation,
action, stream.subscribe, ping), frame envelope schema, ordering guarantee,
backpressure rules, reconnection semantics.

**Verification:** spec reviewed, JSON examples validate against a JSON Schema.

**Status:** `done`

### WP2 — Spec: error schema reference document

Write `docs/adapters/native/errors.md`. Covers: error code taxonomy covering all
existing `AppError` variants, error field contracts, per-channel wrapping
(HTTP vs WebSocket fatal vs in-session op error), client rendering contract
(how to map severity to UI behavior).

**Verification:** every `AppError` variant has a corresponding error code,
JSON examples validate.

**Status:** `done`

### WP3 — Server: protocol version negotiation

Implement `Sec-WebSocket-Protocol` negotiation in the WebSocket upgrade
handler at `ws/mod.rs`. Implement `hello` / `client_hello` frame exchange
with 10-second timeout. Preserve backward compatibility by treating the
current Convex-compatible framing as `nimbus.v1`.

**Verification:** integration test proving: (a) no subprotocol overlap → 400
with structured body, (b) `hello` sent immediately after upgrade,
(c) `client_hello` timeout → close with `protocol.hello_timeout`,
(d) negotiated subprotocol echoed in upgrade response, (e) existing
Convex clients continue to work as `nimbus.v1`.

**Status:** `done`

### WP4 — Server: structured error types

Implement the error schema as Rust types in `nimbus-server`. Replace ad-hoc
`AppError` → HTTP status mapping with structured error envelope serialization
on all response paths (HTTP bodies, WebSocket close frames, per-op errors).

**Verification:** all error responses conform to the schema, snapshot tests
per error code asserting shape, `make test` green.

**Status:** `done`

## Execution Log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-04-18 | Plan authored | — | Extracted from desktop-ui-plan.md as prerequisite |
| 2026-04-26 | WP1 | `done` | Added `docs/adapters/native/websocket-protocol.md` plus example-oriented frame schema at `docs/adapters/native/schemas/websocket-protocol.schema.json` |
| 2026-04-26 | WP2 | `done` | Added `docs/adapters/native/errors.md` plus example-oriented error schema at `docs/adapters/native/schemas/error-envelope.schema.json` |
| 2026-04-26 | WP3 | `done` | Implemented `Sec-WebSocket-Protocol` negotiation, `nimbus.v2` `hello` / `client_hello` handshake with 10s timeout, structured `protocol.no_overlap` HTTP rejection, and legacy `v1` compatibility on `/ws` and `/convex/{tenant_id}/ws` |
| 2026-04-26 | WP4 | `done` | Completed the structured error rollout across HTTP bodies, WebSocket fatal frames, and negotiated `v2` request-scoped `op.error` frames; boxed `AppError::Structured` to keep the shared error path clippy-safe, updated protocol/docs/schema references to the shipped frame shape, and verified the full `nimbus-server` lib lane plus schema/example parsing |
