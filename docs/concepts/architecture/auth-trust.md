---
title: Auth and the trust boundary
description: How identity and authority flow through Nimbus — operator credentials, deploy credentials, end-user identity, and the line where adapters stop authenticating and the engine starts authorizing.
sidebar:
  label: "Auth & trust"
  order: 8
---

Three different kinds of caller talk to a Nimbus server, and they carry
three different kinds of credential:

- the **operator** administering this host, holding the local admin token;
- the **deployer** pushing new code, holding the deploy token;
- the **end users** of the application, holding bearer tokens issued by an
  external identity provider.

These never blend. Each credential is verified by a different component,
proves a different thing, and unlocks a different surface. The organizing
principle across all three is the same: **transport and adapters
authenticate and normalize; the engine authorizes.** A protocol front door
may decide *who you are*; only the engine decides *what that identity may
do to data*.

This page walks the three credential paths, then the authorization seam
they all converge on. Task-oriented setup lives in
[the auth guide](/developers/auth/) and
[operator hardening](/operators/hardening/).

## The shared auth vocabulary: `nimbus-auth`

`crates/nimbus-auth` is a small crate that owns the *shape* of application
auth, not any provider's verification logic:

- `ApplicationAuthVerifier` — the trait a deployment's token verifier
  implements: bearer token in, verified `InvocationAuth` out.
- `ApplicationAuthError` — the error taxonomy (unauthorized, forbidden,
  internal), so every surface classifies auth failures identically.
- `normalize_principal_context` — collapses a verified identity into the
  engine-facing `PrincipalContext`.
- Bearer-scheme parsing and subject-alias normalization (`sub`, `uid`,
  `user_id` → one canonical subject).
- A Firebase *emulator* mock-token parser that performs no signature
  verification by design — usable only when a deployment has explicitly
  enabled emulator auth, never on a production bearer path.

Because adapters consume this crate rather than each inventing principal
handling, "who the caller is" has one definition server-wide.

## The local admin token

A freshly started server is administered with a single local credential,
owned by `crates/nimbus-operator`. Its lifecycle is deliberately boring:

- **Minted on first boot** with a `nimbus_at_` prefix from a secure random
  source, and stored in a file written atomically with `0600` permissions
  under an exclusive file lock (`crates/nimbus-operator/src/token.rs`).
- **Compared in constant time.** `LocalAdminTokenRecord::authorize` is a
  constant-time byte comparison, not a string equality.
- **Presented two ways:** `Authorization: Bearer <token>` or the
  `X-Nimbus-Admin-Token` header. The local UI additionally exchanges the
  token for a short-lived session cookie so the browser never retains the
  raw token (`crates/nimbus-operator/src/access.rs`).
- **Audited.** Admin-surface access decisions are appended to a local
  JSONL audit log (`crates/nimbus-operator/src/audit.rs`).

Routes are classified into families (UI, native API, deploy admin, the
per-protocol adapter surfaces) and each family is gated by an access
policy with one of two credential modes
(`crates/nimbus-operator/src/access_policy.rs`):

- `AuthorizationOrAdminHeader` — the standard admin mode: session cookie,
  bearer, or admin header all work.
- `AdminHeaderOnly` — used by the deploy admin surface, where the
  `Authorization` header is already occupied by a *different* credential
  (below), so operator proof must arrive in `X-Nimbus-Admin-Token`.

The server applies these policies as middleware in
`crates/nimbus-server/src/local_server/middleware.rs`, together with an
origin allowlist for browser-reachable route families.

### The 30-day freshness gate

Binding to a non-loopback address is a two-stage opt-in
(`crates/nimbus-bin/src/start/network_bind.rs`). First, a non-loopback
host requires an explicit `--allow-network` flag. Second, the server
refuses the bind unless the admin token has been **explicitly rotated
within the last 30 days** — and the auto-minted first-boot token counts as
never rotated, so it can never be exposed publicly at all. The credential
that grants full control of the server cannot drift onto a public
interface by accident, and cannot stay there indefinitely without
deliberate rotation.

## The deploy token

Deployment is authenticated by a separate credential: a token the operator
sets in the `NIMBUS_DEPLOY_TOKEN` environment variable (or the equivalent
startup flag). If it is unset, the deploy admin API is disabled outright —
there is no default deploy credential.

Deploy requests carry this token as `Authorization: Bearer`, verified with
a constant-time HMAC-based comparison
(`crates/nimbus-operator/src/access_policy.rs`, enforced from
`crates/nimbus-server/src/http/deploy.rs`). On a locally-secured server
the deploy route family additionally demands the local admin token in
`X-Nimbus-Admin-Token` — pushing code requires both the deploy credential
and operator proof. Keeping the two tokens separate means a CI system that
deploys holds a credential that cannot reconfigure the server, and
rotating one never invalidates the other. The wire contract is documented
in [the deploy admin API reference](/reference/deploy-admin-api/).

## End-user identity: adapters authenticate, then normalize

Application users never authenticate *to Nimbus* — they authenticate to an
external identity provider, and Nimbus verifies the resulting token.
Verification is **deployment-scoped**: each adapter contributes a verifier
built from the deployed app's auth configuration, and the server resolves
bearers against the active deployment
(`crates/nimbus-server/src/application_auth.rs`).

The Convex adapter is the worked example. Its verifier
(`crates/nimbus-convex/src/auth/`) implements the providers declared in
the app's `auth.config.ts`:

- **OIDC providers** — fetch the issuer's discovery metadata, cross-check
  the token issuer against it, fetch the JWKS, and verify the JWT
  signature, algorithm, audience, and temporal claims.
- **Custom JWT providers** — verify against the provider's directly
  configured JWKS with the same claim discipline.

A successful verification produces an `InvocationAuth` carrying both the
claimed identity and a `VerifiedUserIdentity`. Failure is fail-closed: a
bearer that is present but unverifiable is rejected, including when no
provider is configured at all — it is never downgraded to anonymous. A
request with *no* bearer is anonymous by construction.

The verified identity is then normalized into `PrincipalContext`
(`crates/nimbus-core/src/auth/`): an `authenticated` flag plus two claim
bags, the identity claims and the separately-tracked verified claims. This
is the only representation of "who is calling" that crosses into the
engine. Adapter-specific token formats, header conventions, and provider
quirks all stop at the adapter boundary — by design, since each protocol
front door speaks a different auth dialect
(see [the adapter boundary](/concepts/adapter-boundary/)).

## The engine authorizes

Authorization lives with the data, in the engine. A table's schema may
carry a declarative `TableAccessPolicy` — rules for read, create, update,
and delete, each combining a `require_authenticated` flag with predicates
over the principal's claims and the document
(`crates/nimbus-core/src/auth/access.rs`). A table without a policy
accepts whatever the transport admitted; setting a policy adds constraints.

Enforcement is wired into the single mutation path described in
[the engine and mutation path](/concepts/architecture/engine-mutation-path/):

- **Writes.** `enforce_mutation_authorization`
  (`crates/nimbus-engine/src/engine/mutations/authorization.rs`) evaluates
  the table's rule against the principal, the candidate document, and the
  existing document. It runs on both the direct mutation path and the
  queued journal path — there is no write route that skips it, including
  writes originating inside the runtime, because those re-enter the engine
  through the host bridge.
- **Reads.** Read rules are compiled per-principal into planner filters
  pushed into the query, plus a residual per-document check
  (`crates/nimbus-engine/src/engine/queries/authorization.rs`), so an
  unauthorized document is filtered, not fetched-then-hidden.

Because the engine sees only `PrincipalContext`, authorization decisions
are identical no matter which protocol, SDK, scheduler, or isolate
produced the operation.

## Auth is not grants

It is worth separating two systems that both say "no":

- **Auth** decides *who the caller is* and *what data operations that
  principal may perform*. It travels with the request.
- **Runtime grants** decide *what executing code may touch* — filesystem,
  network, environment, subprocesses. They travel with the deployed
  function's policy, regardless of who invoked it.

A function invoked by a fully-authenticated admin user still has no
network access unless its policy grants a named host. Conversely, a
function with broad grants still cannot write a protected table on behalf
of an anonymous caller. Identity enters the isolate as *data* — the
verified claims behind `ctx.auth` — never as capability. The grant model
is covered in [Runtime permissions](/concepts/runtime-permissions/), and
the isolation machinery in
[Runtime and isolates](/concepts/architecture/runtime-isolates/).

## The boundary, summarized

```text
operator ── local admin token ──► operator surfaces (nimbus-operator policy)
deployer ── deploy token ────────► deploy admin API (separate credential)
end user ── provider JWT ────────► adapter verifier ──► PrincipalContext
                                                            │
                                                            ▼
                                                  Engine authorization
                                                  (table access policies,
                                                   single mutation path)
```

Adapters authenticate and normalize. The engine authorizes. The runtime
executes under explicit grants. Each layer can refuse, and no layer can
mint authority the layer above it did not establish — which is the same
posture that scales up to full
[tenant isolation](/concepts/tenant-isolation/).
