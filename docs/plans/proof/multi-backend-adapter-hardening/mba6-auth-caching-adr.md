# MBA6 Auth Caching ADR Proof

adr: docs/decisions/002-auth-caching-policy.md
posture: no_security_sensitive_auth_decision_cache

## Audit Scope

Audited roots:

- `crates/nimbus-server/src/application_auth.rs`
- `crates/nimbus-server/src/adapters/convex/auth/`
- `crates/nimbus-server/src/adapters/firebase/`
- `crates/nimbus-server/src/adapters/cloud_functions/`
- `crates/nimbus-server/src/adapters/mongodb/auth.rs`
- `crates/nimbus-server/src/tenant_isolation/`

## Findings

| Surface | Current behavior | MBA6 decision |
| --- | --- | --- |
| Shared application auth | Extracts bearer token and delegates to the active deployment verifier per request. | No auth result cache. |
| Convex auth | Parses JWT, fetches OIDC/JWKS metadata during verification, validates signature and claims. | Keep uncached until a later JWKS-cache ADR defines TTL and invalidation. |
| Firebase/Firestore | Resolves application auth through the shared verifier for REST, gRPC, and listen paths. Emulator mock auth is explicit dev-mode parsing. | No auth result cache. |
| Cloud Functions | Callable auth resolves through shared optional application auth; HTTP auth payload is invocation-local. | No auth result cache. |
| MongoDB | SCRAM state lives on the connection while a handshake completes. | Per-handshake state is allowed; no cross-connection credential cache. |
| Tenant isolation | Operator policy reload keeps the active last-known-good policy after invalid candidates. | Authoritative active policy state is allowed; no per-request policy decision cache. |

## Grep Evidence

The broad verifier grep finds several non-auth uses of the word `cache`
under tenant-isolation tests and fixture values: named volumes, secret-handle
fixtures, and `$cache_root` runtime grants. They are annotated with
`002-auth-caching-policy` so future audits can distinguish fixture names from
auth-cache behavior without weakening the gate.
