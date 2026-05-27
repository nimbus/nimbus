# 002 - Auth caching policy

- **Status:** accepted
- **Date:** 2026-05-27
- **Decision owner:** `nimbus/nimbus` maintainers
- **Parent plan:** `docs/plans/multi-backend-adapter-hardening-plan.md` MBA6

---

## Context

Nimbus has multiple adapter auth surfaces:

- Convex-compatible JWT authentication through `convex/auth.config.ts`
- Firebase/Firestore REST, gRPC, and listen surfaces
- Cloud Functions callable and HTTPS invocation surfaces
- MongoDB SCRAM authentication
- Tenant-isolation and operator-policy admission

The tempting optimization is to cache credential metadata, resolved identities,
or policy lookup results. That is high risk: stale or invalidated auth state is
a security bug, not just a performance bug. Nimbus is pre-launch, so the right
default is the simpler and stricter posture.

## Decision

Do not cache security-sensitive auth decisions.

Nimbus must not cache:

- bearer-token verification results
- resolved user identity or `PrincipalContext`
- JWT claims after validation
- authorization or tenant-isolation policy decisions for a request
- MongoDB SCRAM credential verification results
- mutable credential metadata, including OIDC discovery and JWKS, unless a
  later ADR defines TTL, invalidation, failure semantics, and tests

Allowed non-auth caches remain allowed when they do not decide access:

- tenant/runtime/document/schema caches
- update-check stale-while-revalidate cache from ADR 001
- operational metrics or usage aggregation
- immutable encryption-key material after local provider initialization
- per-connection MongoDB SCRAM conversation state needed to complete a single
  handshake
- tenant-isolation operator-policy last-known-good state, because it is the
  authoritative active policy document, not a cache of per-request decisions

## Implementation Contract

Auth verifiers must verify from the current deployment configuration on each
request. If a future auth metadata cache is introduced, the code must name this
ADR, declare the TTL/invalidation rule at the cache site, and add tests for key
rotation, revoked/expired tokens, and policy changes.

Convex JWKS and OIDC metadata are intentionally fetched during verification
today. A bounded JWKS cache is allowed only after a follow-up ADR specifies how
the cache respects provider cache headers, key rotation, fetch failures, and
per-deployment configuration changes.

Firebase emulator mock auth is not a cache. It is an explicit development-mode
parser for emulator-style mock bearer payloads.

MongoDB SCRAM state is per connection and per handshake. It must not become a
cross-connection credential-verification cache.

Tenant-isolation policy reload keeps a last-known-good policy after an invalid
candidate. That state is an operator-policy lifecycle rule, not a cache of
authorization results.

## Rejected Alternatives

- **Global token cache.** Rejected because token revocation, tenant moves, and
  policy changes would depend on invalidation correctness.
- **Unbounded JWKS cache.** Rejected because key rotation failures would become
  authentication outages or stale acceptance windows.
- **Adapter-specific ad hoc caches.** Rejected because operators need one
  security posture across every compatibility surface.
