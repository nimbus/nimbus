---
title: WebSocket Listen
description: The browser-oriented Firestore Listen transport — framing, subprotocols, auth tunneling, and close codes.
sidebar:
  order: 2
---

Nimbus exposes a browser-oriented Firestore `Listen` transport at the same
route family as native gRPC:

```text
GET /google.firestore.v1.Firestore/Listen
```

This endpoint exists because gRPC-Web does not support bidirectional
streams. Unary Firestore browser traffic uses REST or gRPC-Web; live queries
(`onSnapshot`) use this WebSocket path. Nimbus's provisioned `firebase`
package selects it automatically.

## Framing

- One binary protobuf message per WebSocket frame.
- Client frames are serialized `google.firestore.v1.ListenRequest`.
- Server frames are serialized `google.firestore.v1.ListenResponse`.
- Text frames are not part of the protocol and are rejected by the server.

The server-side target/session implementation is shared with native gRPC
`Listen`, so target IDs, resume tokens, resets, existence filters, and
subscription cleanup follow the same semantics across both transports.

## Handshake and subprotocols

- Clients offer the fixed subprotocol `nimbus.firebase.listen.v1`; the
  server selects it to confirm the binary Firestore Listen session contract.
- Auth tokens never travel in URL query strings. When the client has a
  bearer token, it offers an additional
  `nimbus.firebase.auth.<base64url-token>` entry in
  `Sec-WebSocket-Protocol`.
- A conventional `Authorization: Bearer <token>` header may be sent as well.
  When both the header and the auth subprotocol are present, the server
  requires the two bearer values to match.
- The server only ever echoes `nimbus.firebase.listen.v1` as the selected
  protocol — the auth token is never reflected back.
- The bearer value routes through the same application-auth resolution used
  by REST, gRPC, and gRPC-Web; active `Listen` targets execute with the
  resolved principal. See [Firebase auth](/reference/firebase/auth/).

## Origin policy

- Loopback browser origins — `localhost`, `127.0.0.1`, and `[::1]`, with or
  without a port — are allowed under the local origin policy.
- Non-loopback origins are rejected before any auth resolution runs.

## Failure model

Per-target query failures stay in-band as Firestore `targetChange`
messages. Whole-stream failures close the socket with stable close codes:

| Condition | Close code | Notes |
| --- | --- | --- |
| Text frame instead of binary protobuf | `1003` (unsupported data) | Clients must send binary protobuf frames only. |
| Invalid protobuf or unsupported request shape | `1008` (policy violation) | Malformed `ListenRequest` frames and request-level contract failures. |
| Internal server failure or backpressure exhaustion | `1011` (internal error) | The shared `Listen` session aborted the whole stream. |

## Reconnect semantics

Reconnect by reusing the Firestore `resume_token` or `read_time` from the
last server response, exactly as a native gRPC `Listen` client would.
