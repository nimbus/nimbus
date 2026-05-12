# WebSocket Protocol

This document is the canonical public contract for Nimbus WebSocket
negotiation, handshake, framing, ordering, and reconnect behavior.

It defines the single prelaunch WebSocket contract Nimbus now supports:
explicit `nimbus.v2` negotiation plus the structured handshake/error behavior
that `docs/plans/archive/websocket-protocol-plan.md` established.

Use [errors.md](errors.md)
for the structured error contract that this protocol embeds.

Schema source for the examples below:

- [websocket-protocol.schema.json](schemas/websocket-protocol.schema.json)

## Scope

This protocol currently covers:

- native live-query subscriptions on `GET /ws`
- Convex-compatible live-query subscriptions on `GET /convex/{tenant_id}/ws`

It does not own:

- Firebase `Listen` WebSocket framing
- native binary codecs
- WebTransport

Those remain owned by their separate reference or follow-on plans.

## Route And Tenant Rules

### Native route

- Path: `GET /ws`
- Tenant selection:
  - non-browser clients may send `X-Tenant-Id`
  - browser clients may send `?tenant_id=...`

### Convex route

- Path: `GET /convex/{tenant_id}/ws`
- Tenant selection:
  - tenant id is path-owned

## Protocol Versions

| Identifier | Status | Meaning |
| --- | --- | --- |
| `nimbus.v2` | required | `hello` / `client_hello`, structured errors, and the current JSON subscription/auth framing |

Rules:

- Clients must offer `Sec-WebSocket-Protocol: nimbus.v2`.
- If the client omits the header or offers only unsupported protocols, the
  server rejects the upgrade with an HTTP `400` structured error body.

Example request:

```http
GET /convex/demo/ws HTTP/1.1
Upgrade: websocket
Connection: Upgrade
Sec-WebSocket-Protocol: nimbus.v2
```

Example successful response:

```http
HTTP/1.1 101 Switching Protocols
Upgrade: websocket
Connection: Upgrade
Sec-WebSocket-Protocol: nimbus.v2
```

## Handshake

### `nimbus.v2`

Immediately after upgrade, the server sends `hello`:

```json
{
  "type": "hello",
  "protocol": "nimbus.v2",
  "server": {
    "version": "0.2.3",
    "build": "git:abc123"
  },
  "features": [
    "queries.v1",
    "mutations.v1",
    "subscriptions.v1"
  ],
  "session": {
    "id": "s_01HX3PKGZT2S7Z8K4M3NQ3D2QF",
    "serverNow": 1777228800000
  }
}
```

The client must answer within 10 seconds:

```json
{
  "type": "client_hello",
  "protocol": "nimbus.v2",
  "client": {
    "kind": "browser",
    "version": "0.2.3"
  },
  "capabilities": [
    "queries.v1",
    "subscriptions.v1"
  ]
}
```

Rules:

- The server closes the session if `client_hello` is missing after 10 seconds.
- `session.id` is diagnostic only in the first `v2` slice. Reconnecting creates
  a new logical session.
- Feature negotiation is additive. Unsupported features fail per operation
  rather than invalidating the whole connection.

## Message Shapes

After the `nimbus.v2` hello exchange completes, the current public client
frames remain the JSON `authenticate`, `clear_auth`, `subscribe`, and
`unsubscribe` messages shown below.

### Client frames

#### Authenticate

```json
{
  "type": "authenticate",
  "token": "eyJhbGciOi..."
}
```

#### Clear auth

```json
{
  "type": "clear_auth"
}
```

#### Subscribe

```json
{
  "type": "subscribe",
  "request_id": "req_1",
  "query": {
    "table": "tasks",
    "filters": [],
    "order": null,
    "limit": null
  }
}
```

#### Unsubscribe

```json
{
  "type": "unsubscribe",
  "subscription_id": 7
}
```

### Server frames

#### Authenticated

```json
{
  "type": "authenticated",
  "is_authenticated": true
}
```

#### Session error

```json
{
  "type": "error",
  "error": {
    "code": "auth.unauthorized",
    "message": "invalid bearer token"
  }
}
```

#### Subscription result

```json
{
  "type": "subscription_result",
  "subscription_id": 7,
  "request_id": "req_1",
  "data": []
}
```

#### Request-scoped error

```json
{
  "type": "op.error",
  "id": "req_1",
  "error": {
    "code": "op.invalid_input",
    "message": "invalid websocket message: missing field `query`"
  }
}
```

## Ordering Rules

These rules are normative for `nimbus.v2`:

- Each socket has exactly one application writer.
- For a mutation `M` that changes live queries `Q1..Qn`, the server must emit
  the resulting query updates before the success result for `M` on that same
  socket.
- Subscription bootstrap and later updates for the same subscription are
  ordered.
- Sequence numbers are not part of the current public frame family.

## Backpressure And Delivery

- The per-socket application outbox is bounded.
- Query subscriptions are latest-value-wins. If a client is slow, the server
  may collapse multiple pending updates into the newest snapshot for that
  subscription.
- If the server can no longer make progress for a session, it emits a fatal
  structured error when the negotiated protocol supports it and then closes.
- Browsers still rely on transport-level ping/pong owned by the WebSocket
  implementation. `v2` `ping` is an application-level health primitive, not a
  replacement for wire ping/pong.

## Reconnect Semantics

Current guaranteed baseline:

- reconnecting creates a new socket and a new logical session
- clients must reauthenticate if they use application auth
- clients must resubscribe after reconnect
- `nimbus.v2` has no server-managed resume token
- `session.id` is currently diagnostic only and is used for correlation and
  logging

Any future resumable transport or durable resume token work must land through
the native transport evolution plan rather than changing this baseline
implicitly.

## Close Behavior

- Before upgrade completes, failures use an HTTP response with the structured
  error body.
- After upgrade completes, session-fatal failures use a structured fatal frame
  when the negotiated protocol supports it, then close the socket.
- Transport close codes are advisory. Semantic clients must key off the JSON
  error `code`, `severity`, and `retryable` fields instead of parsing close
  reasons.

## Example Validation Coverage

The examples in this document are intended to validate against
[websocket-protocol.schema.json](schemas/websocket-protocol.schema.json).
That schema is example-oriented: it covers the public frame families shown
here and is not a substitute for server-side validation logic.
