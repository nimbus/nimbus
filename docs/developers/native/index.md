---
title: Use the native HTTP and WebSocket API
description: Talk to Nimbus from any language — create tenants, write documents, run queries, and subscribe to live results over WebSocket.
sidebar:
  label: Overview
  order: 1
---

Nimbus speaks plain HTTP and WebSocket, so any language with an HTTP client
can use it — no SDK or codegen required. This guide walks through
authenticating, scoping requests to a tenant, working with documents, and
subscribing to live query results.

If you haven't run a server yet, start with the
[self-host quickstart](/get-started/self-host/).

## 1. Get the local admin token

A server started with `nimbus start` protects its native API with a local
admin token. The token is created on first boot and stored as a JSON file:

| Platform | Token file |
| --- | --- |
| Linux | `~/.local/share/nimbus/auth/token` |
| macOS | `~/Library/Application Support/nimbus/auth/token` |
| Windows | `%LOCALAPPDATA%\nimbus\auth\token.json` |

Read the `token` field and export it for the rest of this guide:

```bash
# Linux
export NIMBUS_TOKEN=$(jq -r .token ~/.local/share/nimbus/auth/token)

# macOS
export NIMBUS_TOKEN=$(jq -r .token "$HOME/Library/Application Support/nimbus/auth/token")
```

Send it on every request, either as a bearer token or in the
`X-Nimbus-Admin-Token` header:

```bash
curl -s http://localhost:8080/api/tenants \
  -H "Authorization: Bearer $NIMBUS_TOKEN"
```

Requests without a valid credential get a `401` with code
`auth.unauthorized`. Browser-based callers are additionally restricted to
loopback origins — see the
[HTTP API reference](/reference/native/http-api/) for the full access rules.

## 2. Scope requests to a tenant

Every data operation is scoped to a tenant through the URL path:
`/api/tenants/{tenant_id}/...`. Create a tenant first:

```bash
curl -s -X POST http://localhost:8080/api/tenants \
  -H "Authorization: Bearer $NIMBUS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"id": "demo"}'
```

The server replies `201 Created` with `{"id": "demo"}`. Tenants are fully
isolated from each other: documents, schemas, scheduled jobs, and
subscriptions never cross tenant boundaries.

## 3. Insert and read documents

Insert a document by naming a table and its fields. Tables are created
implicitly on first write:

```bash
curl -s -X POST http://localhost:8080/api/tenants/demo/documents \
  -H "Authorization: Bearer $NIMBUS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"table": "messages", "fields": {"text": "hello world", "author": "you"}}'
```

The response is `201 Created` with the generated document id:

```json
{"id": "01jx2x9w7d2f0v6q8t3k5r9e1b"}
```

Read it back — single documents and table listings are GET requests:

```bash
# One document
curl -s http://localhost:8080/api/tenants/demo/documents/messages/<id> \
  -H "Authorization: Bearer $NIMBUS_TOKEN"

# Every document in the table
curl -s http://localhost:8080/api/tenants/demo/documents/messages \
  -H "Authorization: Bearer $NIMBUS_TOKEN"
```

Returned documents carry three system fields alongside your own: `_id`,
`_creationTime`, and `_updateTime` (epoch milliseconds). Update with `PATCH`
(a partial patch object) and delete with `DELETE` on the same
`/documents/{table}/{document_id}` path.

## 4. Run queries

POST a query object to filter, order, and limit results. `filters` is
required — pass an empty array to match everything:

```bash
curl -s -X POST http://localhost:8080/api/tenants/demo/query \
  -H "Authorization: Bearer $NIMBUS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "table": "messages",
    "filters": [{"field": "author", "op": "eq", "value": "you"}],
    "order": {"field": "_creationTime", "direction": "desc"},
    "limit": 10
  }'
```

For large result sets, use `/query/paginated` with a `page_size` and follow
the returned cursor. Both endpoints, all filter operators, and the paginated
shapes are listed in the [HTTP API reference](/reference/native/http-api/).

## 5. Subscribe to live results over WebSocket

The same query can be registered as a subscription: the server pushes a
fresh snapshot whenever a mutation changes the result. Connect to `/ws`
with the `nimbus.v2` subprotocol and identify the tenant with an
`X-Tenant-Id` header or a `tenant_id` query parameter.

This example uses the `ws` package, which lets you set headers during the
upgrade:

```javascript
import WebSocket from "ws";

const socket = new WebSocket("ws://localhost:8080/ws?tenant_id=demo", ["nimbus.v2"], {
  headers: { Authorization: `Bearer ${process.env.NIMBUS_TOKEN}` },
});

socket.on("message", (raw) => {
  const frame = JSON.parse(raw.toString());

  switch (frame.type) {
    case "hello":
      // Complete the handshake, then subscribe.
      socket.send(JSON.stringify({ type: "client_hello", protocol: "nimbus.v2" }));
      socket.send(
        JSON.stringify({
          type: "subscribe",
          request_id: "messages-1",
          query: { table: "messages", filters: [] },
        }),
      );
      break;
    case "subscription_result":
      // frame.data is the full current result set for the query.
      console.log(`subscription ${frame.subscription_id}:`, frame.data);
      break;
    case "op.error":
    case "error":
    case "fatal_error":
      console.error(frame.error.code, frame.error.message);
      break;
  }
});
```

Two handshake rules to respect: reply to the server's `hello` frame with
`client_hello` within 10 seconds, and send only JSON text frames. The full
frame catalog, handshake failure modes, and reconnect semantics are in the
[WebSocket protocol reference](/reference/native/websocket-protocol/).

## Handle errors

Every error — HTTP or WebSocket — uses one envelope:

```json
{
  "error": {
    "code": "op.invalid_input",
    "message": "invalid document id `abc`",
    "requestId": "req-...",
    "timestamp": "2026-06-10T17:03:21Z",
    "severity": "error",
    "retryable": false,
    "detail": null,
    "remediation": { "action": "fix_request", "message": "Correct the request payload before retrying." }
  }
}
```

Branch on `code`, retry when `retryable` is true, and surface
`remediation.message` to operators. The complete code catalog with HTTP
status mappings is in the [error reference](/reference/native/errors/).

## Next steps

- [HTTP API reference](/reference/native/http-api/) — every endpoint,
  request shape, and response shape.
- [WebSocket protocol reference](/reference/native/websocket-protocol/) —
  the `nimbus.v2` frame contract.
- [Error reference](/reference/native/errors/) — error codes, severities,
  and status mappings.
- [Manage services, sandboxes, and sessions](/developers/sdk/resource-model/)
  — the same server's resource APIs through the JavaScript SDK.
