---
title: WebSocket protocol
description: The nimbus.v2 WebSocket message protocol for live query subscriptions.
sidebar:
  order: 2
---

The native WebSocket endpoint streams live query results: register a query
once and receive a fresh snapshot whenever a mutation changes its result.
This page is the frame-level contract. For a working client walkthrough,
see [Use the native HTTP and WebSocket API](/developers/native/).

## Connection

| Property | Value |
| --- | --- |
| Endpoint | `GET /ws` (HTTP upgrade) |
| Subprotocol | `nimbus.v2`, offered via `Sec-WebSocket-Protocol` |
| Tenant | `X-Tenant-Id` header or `tenant_id` query parameter |
| Frames | JSON text frames only |

The upgrade is rejected before any WebSocket traffic when:

| Condition | Result |
| --- | --- |
| `Sec-WebSocket-Protocol` missing or without `nimbus.v2` | HTTP 400, code `protocol.no_overlap` |
| Tenant missing from header and query | HTTP 400, code `op.invalid_input` |
| Tenant does not exist | HTTP 404, code `session.tenant_not_found` |
| Missing or invalid credential (server with local security) | HTTP 401, code `auth.unauthorized` |

The `protocol.no_overlap` error's `detail` lists `serverSupports`
(`["nimbus.v2"]`) and `clientOffered`.

## Handshake

After the upgrade the server immediately sends a `hello` frame:

```json
{
  "type": "hello",
  "protocol": "nimbus.v2",
  "server": { "version": "<server version>", "build": "<build id>" },
  "features": ["queries.v1", "subscriptions.v1"],
  "session": { "id": "<session id>", "serverNow": 1765386000000 }
}
```

`serverNow` is the server clock in epoch milliseconds. The client must
reply with a `client_hello` text frame within 10 seconds:

```json
{ "type": "client_hello", "protocol": "nimbus.v2" }
```

Handshake violations end the connection with a `fatal_error` frame followed
by a close frame with code `1008` (policy violation); the close reason is
the error code:

| Violation | Error code |
| --- | --- |
| No `client_hello` within 10 seconds | `protocol.hello_timeout` (`detail.timeoutMs`) |
| Frame is not valid JSON | `protocol.invalid_json` |
| First frame's `type` is not `client_hello` | `protocol.unsupported_message_type` (`detail.receivedType`, `detail.expectedType`) |
| `protocol` is not `nimbus.v2` | `protocol.unsupported_version` (`detail.receivedProtocol`) |
| Binary frame during handshake | `protocol.unsupported_binary` |

Ping frames are answered with pongs during the handshake. After the
handshake, binary frames are ignored.

## Client messages

All client frames are JSON objects tagged by `type`.

### `subscribe`

```json
{
  "type": "subscribe",
  "request_id": "messages-1",
  "query": { "table": "messages", "filters": [] }
}
```

- `request_id` — caller-chosen string echoed in the first result and in
  registration errors.
- `query` — the same query object as `POST /query` in the
  [HTTP API reference](/reference/native/http-api/): `table` and `filters`
  required, `order` and `limit` optional.

If registration fails, the server sends an `op.error` frame carrying the
`request_id` and code `op.failed`.

### `unsubscribe`

```json
{ "type": "unsubscribe", "subscription_id": 7 }
```

`subscription_id` is the numeric id from `subscription_result` frames. If
teardown fails the server sends an `error` frame with code
`session.unsubscribe_failed`.

### `clear_auth`

```json
{ "type": "clear_auth" }
```

The server replies with `{"type": "authenticated", "is_authenticated": false}`.

### `authenticate`

Not supported on the native route. The server replies with an `error` frame
whose error code is `auth.unauthorized` and message
`authentication is not supported on the generic websocket route`. The
connection stays open.

## Server messages

| `type` | Fields | Meaning |
| --- | --- | --- |
| `hello` | see handshake | Sent once after upgrade |
| `subscription_result` | `subscription_id`, `data`, `request_id` (first result only) | Full current result set for the subscribed query |
| `authenticated` | `is_authenticated` | Reply to `clear_auth` |
| `error` | `error` | Session-level error; connection stays open |
| `op.error` | `id`, `error` | Error tied to a specific `request_id` |
| `fatal_error` | `error` | Terminal error; followed by close code 1008 |

### `subscription_result`

```json
{
  "type": "subscription_result",
  "subscription_id": 7,
  "request_id": "messages-1",
  "data": [
    { "_id": "...", "_creationTime": 1765386000000, "_updateTime": 1765386000000, "text": "hello" }
  ]
}
```

`data` is always the complete current result set, not a delta. The first
result for a subscription includes the originating `request_id`; later
updates carry only `subscription_id`. Each connection has a single ordered
writer: frames arrive in the order the server emits them.

### Error frames after the handshake

| Code | Frame | Trigger |
| --- | --- | --- |
| `protocol.invalid_json` | `error` | Text frame that is not a valid client message |
| `op.failed` | `op.error` | Subscription registration or evaluation failed for a known `request_id` |
| `session.subscription_error` | `error` | Subscription stream error not tied to a `request_id` |
| `session.unsubscribe_failed` | `error` | `unsubscribe` teardown failed |
| `auth.unauthorized` | `error` | `authenticate` sent on the native route |

The `error` object inside every frame uses the envelope fields documented
in the [error reference](/reference/native/errors/).

## Reconnecting

Subscriptions are connection-scoped. After a disconnect, reconnect,
complete the handshake, and re-send `subscribe` for each query. The first
`subscription_result` after resubscribing is a full snapshot, so no
client-side replay is needed.
