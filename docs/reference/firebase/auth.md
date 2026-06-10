---
title: Firebase auth
description: How bearer tokens and emulator mock user tokens authenticate on Nimbus's Firestore-compatible routes.
sidebar:
  order: 3
---

This reference defines application auth on the Firestore-compatible route
family: which auth inputs each transport accepts, and what identity they
resolve to.

## Baseline behavior

- A request with no auth input runs as an **anonymous** principal.
- A bearer token on a covered transport resolves once at the server edge
  into the same application principal used everywhere else in Nimbus —
  there is no Firebase-specific identity model.
- Covered Firebase reads, writes, transactions, and listeners enforce the
  resolved principal.

The covered transports are:

- REST unary document/query/write endpoints
- gRPC unary methods
- gRPC-Web unary methods
- native gRPC `Write`
- native gRPC `Listen`
- browser [WebSocket Listen](/reference/firebase/websocket-listen/)

## Accepted inputs by transport

| Surface | Client input | Outcome |
| --- | --- | --- |
| REST unary | `Authorization: Bearer <token>` | Authenticated for verified JWT bearers; JSON-object emulator tokens authenticate only when the server enables mock-user-token auth; otherwise the request fails closed. |
| gRPC-Web unary | `Authorization: Bearer <token>` | Same as REST unary. |
| Browser WebSocket `Listen` | Fixed `nimbus.firebase.listen.v1` subprotocol plus optional `nimbus.firebase.auth.<base64url-token>` subprotocol and/or a matching `Authorization` header | Same as REST unary. When both the subprotocol and the header carry a bearer, the values must match. The auth token is never echoed back in the selected protocol. |
| Native gRPC unary / `Write` / `Listen` | `authorization: Bearer <token>` metadata | Same as REST unary. There is no separate gRPC-only auth scheme. |

Compatibility-shape headers such as `x-goog-api-key` and `x-firebase-gmpid`
are accepted on requests but are not auth inputs.

## Token sources in `@nimbus/firebase`

The SDK can source auth material from:

- the `experimentalAuthToken` Firestore setting — either a fixed string or
  a fetcher function called with `{ forceRefresh }`
- `connectFirestoreEmulator(db, host, port, { mockUserToken })`

These split into two explicit paths:

- **String values** are forwarded as bearer tokens. They authenticate only
  if the server has auth providers configured to verify them.
- **Object `mockUserToken` values** are JSON-stringified before forwarding
  and are accepted only when the server explicitly enables emulator
  mock-user-token auth for the Firebase route family. Without that
  server-side opt-in, object mock tokens do not authenticate.

## Not in the contract

The Firebase application-auth contract does **not** include:

- query-string auth tokens
- App Check headers or `enforceAppCheck` semantics
- Firebase Security Rules evaluation — port rules intent into application
  auth checks, see [Migrate from Firebase](/developers/firebase/migrate/)
- local admin / server-access tokens on Firebase application routes
- stock upstream SDK auth-behavior parity outside the first-party
  `@nimbus/firebase` path
