# Firebase Application Auth Contract

This reference defines the current Neovex application-auth contract for the
Firebase / Firestore-compatible route family.

It intentionally separates two different questions:

1. Which auth-shaped inputs can reach a Firebase transport today?
2. Which of those inputs currently resolve into a shared
   `PrincipalContext` inside Neovex?

As of 2026-04-26, those answers are now aligned for the covered Firebase data
paths. The first-party `@neovex/firebase` SDK can emit bearer tokens across its
covered transports, and the current Firebase adapter resolves those inputs
through the shared Neovex principal path on covered reads, writes,
transactions, and listeners.

For the broader support matrix, see
[Firebase compatibility](firebase-compatibility.md). For the migration path,
see [Firebase migration guide](firebase-migration-guide.md). For the browser
watch transport details, see
[Firebase WebSocket Listen](firebase-websocket-listen.md).

## Canonical Principal Shape

When a transport boundary resolves application identity successfully, it must
produce the shared engine-facing shape:

- `PrincipalContext.authenticated`
- `PrincipalContext.claims`
- `PrincipalContext.verified_claims`

That is the same principal contract used by the rest of Neovex. Firebase must
not invent a parallel adapter-local auth shape.

## Current Baseline

Today, the Firebase route family has one honest auth baseline:

- absence of auth input resolves to `PrincipalContext::anonymous()`
- covered bearer inputs resolve once at the server edge through one shared
  helper
- covered Firebase reads, writes, transactions, and listeners pass the
  resolved principal into the same engine APIs used elsewhere in Neovex

That applies to:

- REST unary document/query/write endpoints
- gRPC unary methods
- gRPC-Web unary methods
- native gRPC `Write`
- native gRPC `Listen`
- browser WebSocket `Listen`

This means Firebase transport auth **is** a usable application-auth and
authorization surface for the covered Firebase data paths, but only within the
explicit boundaries documented below.

## Accepted Inputs By Transport

| Surface | Client input today | Server handling today | Principal outcome today | Notes |
| --- | --- | --- | --- | --- |
| `@neovex/firebase` REST unary | `Authorization: Bearer <token>` when `experimentalAuthToken` or emulator `mockUserToken` is set | The server extracts the bearer once and resolves it through the shared Firebase application-auth helper | `authenticated` for covered verified JWT bearers; JSON-object emulator tokens authenticate only when the server explicitly enables mock-user-token auth; otherwise requests fail closed | `x-goog-api-key` and `x-firebase-gmpid` may also be sent for compatibility shape, but they are not auth. |
| `@neovex/firebase` gRPC-Web unary | `Authorization: Bearer <token>` through the shared gRPC-Web fetch path | Same shared bearer extraction and resolution path as REST unary | Same as REST unary | Same bearer emission and retry shape as REST unary. |
| Browser WebSocket `Listen` | fixed `neovex.firebase.listen.v1` plus optional `neovex.firebase.auth.<base64url-token>` subprotocol; optional matching `Authorization` header | The server validates that subprotocol-carried and header-carried bearer values match when both are present, then routes the bearer through the same shared resolver | Same as REST unary | The selected protocol stays `neovex.firebase.listen.v1`; the auth token is never echoed back in the accepted protocol string. |
| Native gRPC unary / `Write` / `Listen` | upstream clients may send auth metadata such as `authorization: Bearer <token>` | The server extracts native gRPC auth metadata and resolves it through the same shared helper used by REST and browser `Listen` | Same as REST unary | No separate gRPC-only auth shape is allowed. |

## Emulator And Mock Token Boundary

`@neovex/firebase` can source auth material from:

- `experimentalAuthToken`
- `experimentalAuthToken({ forceRefresh })`
- `connectFirestoreEmulator(..., { mockUserToken })`

Those values now split into two explicit paths:

- string values are forwarded as bearer tokens and require configured auth
  providers if they are expected to authenticate
- object `mockUserToken` values are JSON-stringified before forwarding and are
  accepted only when the server explicitly enables emulator mock-user-token
  auth for the Firebase route family

So emulator auth-shape compatibility is no longer implicit. The documented
JSON-object `mockUserToken` path only reaches the server principal layer when
the server has opted into that emulator-only contract.

## Explicit Non-Goals In The Current Contract

The Firebase application-auth contract does **not** currently include:

- query-string auth tokens
- App Check headers or `enforceAppCheck` semantics
- Firebase Security Rules evaluation
- local admin / localhost server-access tokens on Firebase application routes
- stock upstream SDK parity for auth behavior outside the first-party
  `@neovex/firebase` path

If a doc or test implies any of those are live on Firebase-compatible routes,
that claim is ahead of the current implementation.

## Covered Principal-Entry Contract

The landed Firebase principal-entry contract is:

1. Firebase application auth must enter once at the server edge.
2. Absence of auth input resolves to `PrincipalContext::anonymous()`.
3. Presence of a bearer token must route through one server-owned Firebase
   principal-extraction helper, not transport-specific ad hoc parsing.
4. JSON-object emulator bearers are emulator-only compatibility inputs and
   must require explicit server-side opt-in instead of authenticating by
   default.
5. Browser WebSocket `Listen` auth subprotocols are only a transport tunnel to
   that same bearer-extraction helper; they are not a second auth scheme.
6. Unary, `Write`, and `Listen` flows all pass the resolved principal into the
   same shared engine APIs used elsewhere.

The remaining follow-on work is no longer principal propagation itself; that
proof and compatibility-truth hardening landed in the completed
`docs/plans/archive/multi-adapter-boundary-hardening-plan.md` (`MAB3`). The later
runtime-host ownership cleanup built on that settled contract in the completed
`docs/plans/runtime-capability-adapter-boundary-plan.md`.
