# Firebase WebSocket Listen

Neovex exposes a browser-oriented Firestore `Listen` transport at the same
route family as native gRPC:

```text
GET /google.firestore.v1.Firestore/Listen
```

This endpoint exists because gRPC-Web does not support bidirectional streams.
Unary and server-streaming Firestore browser traffic should keep using gRPC-Web;
browser `Listen` uses this WebSocket path instead.

For the broader Firebase-route auth and principal-resolution truth behind this
transport, see [Firebase application auth contract](firebase-auth-contract.md).

## Framing

- One binary protobuf message per WebSocket frame.
- Client frames are serialized `google.firestore.v1.ListenRequest`.
- Server frames are serialized `google.firestore.v1.ListenResponse`.
- Text frames are not part of the production protocol. They are reserved for
  test tooling and are rejected by the server.

The server-side target/session implementation is shared with native gRPC
`Listen`, so target IDs, resume tokens, resets, existence filters, and
subscription cleanup follow the same semantics across both transports.

## Handshake And Security

- The WebSocket upgrade uses the same Firebase route-family middleware as REST,
  gRPC, and gRPC-Web.
- Browser SDK clients offer the fixed subprotocol
  `neovex.firebase.listen.v1` so the server can explicitly select the binary
  Firestore Listen session contract without taking ownership of the native
  WebSocket protocol plan's general version-negotiation work.
- Browser auth tokens must not move into URL query strings. When the SDK has a
  bearer token, it offers an additional
  `neovex.firebase.auth.<base64url-token>` entry in `Sec-WebSocket-Protocol`.
  The server validates that offer against any conventional `Authorization`
  header but only echoes the fixed Listen protocol, so the auth token never
  comes back in the selected protocol string.
- Loopback browser origins such as `http://localhost:5173` are allowed under
  the local origin policy.
- Non-loopback origins are rejected before any Firebase principal-resolution
  work would run.
- This transport remains an application-surface Firebase route. It must not
  require local admin server-access auth.

Current auth truth for this transport:

- the browser SDK can tunnel a bearer token through the `Authorization` header,
  the auth subprotocol offer, or both
- when both are present, the server requires them to match
- the resulting bearer value routes through the same shared Firebase
  application-auth resolver used by REST, gRPC, and gRPC-Web
- active `Listen` targets execute with the resolved `PrincipalContext` on the
  covered Firebase data paths
- opaque string bearers still require configured auth providers if they are
  expected to authenticate; JSON-object emulator `mockUserToken` values resolve
  into unverified principal claims only when the server explicitly enables the
  emulator-only mock-user-token auth contract

## Failure Model

Per-target query failures stay in-band as Firestore `targetChange` messages.
Whole-stream failures close the socket with stable close codes:

| Condition | Close code | Notes |
| --- | --- | --- |
| Text frame instead of binary protobuf | `1003` (`Unsupported`) | Production clients must send binary protobuf frames only. |
| Invalid protobuf or unsupported request/policy shape | `1008` (`Policy`) | Used for malformed `ListenRequest` frames and request-level contract failures. |
| Internal server failure or bounded backpressure exhaustion | `1011` (`Error`) | Used when the shared `Listen` session aborts the whole stream. |

Client reconnect behavior should reuse Firestore `resume_token` or `read_time`
exactly as it would over native gRPC `Listen`.
