# Plan: Localhost Server Security

Canonical execution plan for hardening `neovex start` as a localhost service:
token-based authentication, origin allowlist, session cookie bootstrap,
Content Security Policy, server discovery, and audit logging. These
protections apply regardless of whether a UI exists — any localhost-exposed
server needs them.

Reviewed against:

- `crates/neovex-server/src/router.rs` — current route tree (no auth
  middleware exists today)
- `crates/neovex-server/src/ws/mod.rs` — WebSocket upgrade handler (no
  origin check, no auth gating)
- `crates/neovex-bin/src/main.rs` — CLI subcommands (`start`, `machine`,
  `compose`); no `token` subcommand exists
- `crates/neovex-bin/src/machine/mod.rs:2206-2246` — established XDG path
  convention (`$XDG_CONFIG_HOME/neovex/machine/`, etc.)
- `docs/reference/macos-machine-flow.md:232-237` — settled XDG convention
- `Cargo.toml` — `ring` 0.17 already a workspace dependency

---

## Status

- **Status:** `active`
- **Primary owner:** this plan
- **Activation gate:** prerequisite for `docs/plans/desktop-ui-plan.md`
- **Related plans:**
  - `docs/plans/websocket-protocol-plan.md` — middleware ordering references
    the protocol negotiation layer from that plan
  - `docs/plans/desktop-ui-plan.md` — the UI consumes token-gate, session
    cookie, and CSP; depends on this plan completing first

## Current Assessed State

- The server has historically bound to all interfaces by default with no
  authentication on any endpoint. The hardening baseline is loopback by
  default with an explicit `--host` override for operators who intentionally
  expose it beyond localhost.
- The WebSocket upgrade handler checks tenant existence but not caller
  identity.
- No token file, no origin allowlist, no session cookie, no CSP header, no
  audit log, no server discovery file exist today.
- The machine manager already uses XDG paths correctly — this plan extends
  that convention to auth, run state, and logs.
- `ring` 0.17 is already a workspace dependency (ECDSA/Ed25519 in test auth).

## Control Plan Rules

1. File paths follow the established XDG convention — no `~/.neovex/`.
2. No custom crypto. Session cookies are signed using `ring::hmac` with a
   key derived from the token file. Token comparison uses constant-time byte
   comparison.
3. Middleware ordering is security-critical: origin → auth → protocol →
   accept. This ordering is documented and tested.
4. No tokens in URLs — headers, cookies, or POST bodies only. Short-lived
   browser launch tickets may use URL fragments because fragments are not sent
   to the server, but query strings are forbidden for auth material.

## Verification Contract

Each roadmap item must satisfy before closing:

- `cargo fmt --all --check` — clean
- `make clippy` — clean
- `make test` — green
- Manual verification described per item

## Architecture

### File path convention

| Purpose | Linux / XDG | macOS | Windows |
| --- | --- | --- | --- |
| Auth token | `$XDG_DATA_HOME/neovex/auth/token` (fallback `~/.local/share/neovex/auth/token`) | `~/Library/Application Support/neovex/auth/token` | `%LOCALAPPDATA%\neovex\auth\token.json` |
| Server run state | `$XDG_RUNTIME_DIR/neovex/server.json` (fallback `$XDG_STATE_HOME/neovex/run/server.json`) | `$TMPDIR/neovex/server.json` when `$TMPDIR` is set, otherwise `~/Library/Application Support/neovex/run/server.json` | `%LOCALAPPDATA%\neovex\run\server.json` |
| Audit log | `$XDG_STATE_HOME/neovex/logs/access.jsonl` (fallback `~/.local/state/neovex/logs/access.jsonl`) | `~/Library/Logs/neovex/access.jsonl` | `%LOCALAPPDATA%\neovex\logs\access.jsonl` |

Parent directories that contain auth or run-state files are created user-only
(`0700` on Unix, current-user ACL on Windows). Token files are `0600` on Unix
and current-user only on Windows.

### Token file

`$XDG_DATA_HOME/neovex/auth/token` (`0600`, user-only ACL on Windows):

```json
{
  "version": 1,
  "token": "neovex_at_<base64url-256bit>",
  "generation": 1,
  "issuedAt": "2026-04-18T...",
  "scope": "local-admin"
}
```

Generated on first `neovex start` if absent. Token writes are serialized with
an auth-file lock and committed through an atomic temp-file-and-rename flow.

`neovex token rotate` first discovers a running local server from
`server.json`. If the server is live, the CLI authenticates with the current
token and calls a local rotate endpoint so the process updates its in-memory
HMAC key and generation before the token file is rewritten. If no live server
exists, the CLI may perform an offline atomic rewrite. Rotation bumps
`generation`, invalidating outstanding sessions with `auth.token_revoked`.

### Authentication paths

| Client | How it authenticates | Why |
| --- | --- | --- |
| CLI | Reads token file directly | Same user, same filesystem |
| Native shell | Reads token file, sends `Authorization: Bearer <token>` | Has filesystem access |
| Browser tab | POSTs token to `/ui/auth/session` or redeems a short-lived CLI launch ticket, then receives a session cookie | Cannot read filesystem |

### Auth layer model

This plan protects the local server surface. It must not duplicate or replace
application authentication.

Neovex has two separate auth layers:

1. **Server access auth** proves the caller may use local Neovex control
   surfaces. Sources are the local admin token, signed `neovex_session`
   cookie, one-time browser launch tickets, and the deploy-specific token.
   Server access auth authorizes route families such as native REST,
   diagnostics, deploy admin, and the desktop/system UI. It never populates
   Convex `InvocationAuth`, `ctx.auth`, or the engine `PrincipalContext` for a
   user tenant.
2. **Application auth** proves the end-user identity for a tenant/app
   invocation. Today the Convex adapter verifies JWT/OIDC/custom JWT tokens
   from `convex/auth.config.ts` and normalizes them into `InvocationAuth` plus
   `PrincipalContext`. Future Neovex-native application auth should reuse the
   same provider-neutral verifier and identity normalization path, then map to
   Convex-compatible shapes at the Convex adapter boundary. Do not add a
   second JWT/OIDC verifier for the local server token/session system.

Tenant scope is explicit:

- the tenant id in the route selects the tenant/app registry before
  application auth is verified
- an application identity is scoped to that tenant/app auth configuration
- a local admin token or UI session is server-wide and is not a user-tenant
  application identity
- the reserved `_neovex` system tenant may receive an explicit system
  principal derived from server access auth for management UI functions, but
  that projection is limited to `_neovex` and must not bleed into user tenants

Header ownership is also explicit. `Authorization: Bearer ...` on
Convex-compatible app routes belongs to application auth. Local admin bearer
tokens are accepted on server-control route families; if a future app route
also needs server access auth, use the signed cookie or a distinct
`X-Neovex-Admin-Token` header instead of stealing the application's bearer JWT.

### Session cookie bootstrap

GET navigation to `/ui/` never mints a session by itself. If a request lacks a
valid session cookie and is to any `/ui/*` path other than `/ui/auth`, redirect
to `/ui/auth`.

`/ui/auth` is a minimal server-owned bootstrap route in this plan. It serves an
auth form and, later, the same route can be replaced by the embedded SPA
without changing the server contract. A session is created only after one of
these proofs:

- `POST /ui/auth/session` with the local admin token in the request body
- `POST /ui/auth/session` with a one-time browser launch ticket generated by a
  token-authenticated local CLI call; tickets are single-use, expire within 60
  seconds, and are never accepted from query strings

The server sets a signed `neovex_session` cookie
(`HttpOnly; SameSite=Strict; Path=/`) with fields `{session_id, generation,
issued_at, expires_at}` plus an HMAC. Session TTL is 12 hours by default. A
token-generation mismatch returns `401 auth.token_revoked`.

Use `Sec-Fetch-Mode: navigate` as a hint for UI routing and logging, but not
as an authentication proof. This handles browser prefetch, service workers,
and extensions that may not preserve fetch metadata headers.

### Protected route matrix

| Route family | Server access auth | Application auth | Origin / CORS | Notes |
| --- | --- | --- | --- | --- |
| `GET /health` | none | none | no credentials, no CORS credentials | Liveness only; must not expose tenant, runtime, license, machine, or path state |
| `GET /ui/*` | signed session cookie, redirect to `/ui/auth` when missing | `_neovex` system principal only when a system-tenant function is invoked | same-origin only | LS4 owns minimal bootstrap routes; DU1 later replaces static assets without weakening middleware |
| `POST /ui/auth/session` | local admin token in POST body or one-time CLI launch ticket | none | same-origin or no-origin localhost form POST only | Sets `neovex_session`; never accepts query-string credentials |
| `/api/tenants/*`, `/api/tenants`, `/api/*/documents`, `/api/*/query`, scheduler, cron, journal | local admin bearer token or signed session cookie | none unless route explicitly delegates into a tenant app | localhost allowlist only; credentialed CORS disabled unless explicitly configured | Native admin/data surface; local admin token is not a tenant principal |
| `/debug/*` | local admin bearer token or signed session cookie | none | localhost allowlist only | Diagnostics can leak local state and provider topology |
| `POST /api/admin/deploy` | existing deploy token plus local admin auth when bound to loopback; deploy token remains required | none | localhost allowlist only | `NEOVEX_DEPLOY_TOKEN` remains the deploy-specific capability |
| `/convex/{tenant}/query`, `/mutation`, `/action`, `/schedule/*`, `/http/*` | none by default for Convex-compatible app API | tenant/app `Authorization: Bearer <JWT>` verified by the selected Convex registry when configured; otherwise anonymous | localhost allowlist only | Preserves Convex semantics. Local server auth must not consume the app bearer token or populate `ctx.auth`. |
| `/ws` | local admin bearer token or signed session cookie before protocol selection/upgrade | none | WebSocket `Origin` must be absent or in allowlist | Native server WebSocket surface |
| `/convex/{tenant}/ws` | none by default for Convex-compatible app API | tenant/app auth follows the Convex WebSocket protocol and selected registry | WebSocket `Origin` must be absent or in allowlist | Bad origin must return `403` before any local or app token validation |

Default allowlist entries are `http://localhost:<port>`,
`http://127.0.0.1:<port>`, and `http://[::1]:<port>`. Explicit extra origins
must be provided through a future server option; wildcard origins are not a
localhost-security closeout shape.

### Middleware ordering

Request flows through layers in this order (outermost first):

```
trace → request_id → origin_allowlist → rate_limit → server_access_extract → route_family_gate → tenant_select → application_auth_extract → protocol_select → ws_upgrade
```

- Origin before auth prevents leaking token-validity timing to hostile origins.
- Rate limit before auth bounds unauthenticated brute force. Baseline limits:
  60 failed auth attempts per minute per remote IP, 120 `/ui/auth/session`
  attempts per minute per process, and a global cap of 512 concurrent
  WebSocket upgrades.
- Server access extraction parses local token/session credentials but only
  enforces them for route families that require server access auth.
- Application auth runs after tenant selection so the selected tenant/app
  registry owns JWT/OIDC verification. It produces `InvocationAuth` and
  `PrincipalContext`; server access auth does not.
- Protocol after the relevant auth gate avoids wasting parser work on
  unauthenticated requests.
- `/health` is unauthenticated and outside this stack (liveness probe).

### Content Security Policy

CSP header on all `/ui/*` responses:

```
default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline';
img-src 'self' data:; font-src 'self' data:;
connect-src 'self' ws://127.0.0.1:* ws://localhost:*;
```

No `'unsafe-eval'` in production. Gate dev-mode relaxation behind
`#[cfg(debug_assertions)]`.

### Server discovery file

`$XDG_RUNTIME_DIR/neovex/server.json`:

```json
{
  "pid": 12345,
  "address": "127.0.0.1:6789",
  "startedAt": "2026-04-18T12:34:56Z",
  "version": "0.2.3",
  "protocolVersions": ["neovex.v1", "neovex.v2"]
}
```

Written on bind with a `RemoveOnDrop` guard. On startup, validate PID
liveness to handle stale files from `SIGKILL`.

### Defense-in-depth layers

All active simultaneously:
- Bind loopback only (`127.0.0.1` / `::1`)
- Origin allowlist on WebSocket upgrade
- Private Network Access preflight denial for non-loopback origins
- Audit log at `$XDG_STATE_HOME/neovex/logs/access.jsonl`
- No tokens in URLs
- `ring::hmac` for session cookie signing

## Roadmap

### LS1 — XDG file paths and server discovery

Implement XDG-compliant path resolution for auth, run state, and logs —
consistent with the existing machine manager convention. Write
`server.json` on `neovex start` bind with `RemoveOnDrop` guard. PID
liveness check on startup for stale file recovery.

**Verification:** (a) `server.json` written on bind with correct PID/address,
(b) file removed on clean shutdown, (c) stale file from SIGKILL cleaned on
next startup, (d) paths respect `$XDG_DATA_HOME`, `$XDG_RUNTIME_DIR`,
`$XDG_STATE_HOME` when set.

**Status:** `pending`

### LS2 — Token file lifecycle and CLI subcommand

Generate 256-bit token on first `neovex start`, write to
`$XDG_DATA_HOME/neovex/auth/token` with `0600` permissions (ACL on
Windows). Add `Command::Token(TokenCommand)` to the CLI with
`TokenSubcommand::Rotate` that bumps `generation`.

When a live server is present, rotation goes through an authenticated local
rotate endpoint so the running process updates its in-memory token generation
and HMAC key before writing the new token file. Offline rotation is allowed
only when `server.json` is absent or stale.

**Verification:** (a) token file created on first start, (b) permissions
are 0600, (c) `neovex token rotate` increments generation, (d) token
file is reused across restarts, (e) Windows ACL restricts to current user,
(f) live-server rotation invalidates an existing cookie without restart,
(g) offline rotation refuses to race a live server.

**Status:** `pending`

### LS3 — Origin allowlist and middleware stack

Implement route-family server-access policy, application-auth extraction, and
origin policy from the protected route matrix. Implement the full middleware
ordering: trace → request_id → origin_allowlist → rate_limit →
server_access_extract → route_family_gate → tenant_select →
application_auth_extract → protocol_select → ws_upgrade. Bind to loopback only
by default; non-loopback binding requires an explicit `--host` value and must
print a startup warning that local admin auth is now reachable from that
interface.

**Verification:** (a) non-allowlisted origin → 403, (b) allowlisted origin
with invalid server-access token on a protected native route → 401,
(c) `/health` bypasses auth, (d) ordering confirmed via integration test that
checks 403 before 401 for bad origin + bad token, (e) representative native
CRUD, debug, deploy, and native WebSocket routes reject missing server access
auth, (f) Convex-compatible HTTP and WebSocket app routes preserve Convex
application-auth semantics and do not require or interpret the local admin
token by default, (g) a local admin token presented to a Convex app route does
not populate `ctx.auth`, and (h) a tenant-scoped Convex JWT is verified only
against the selected tenant/app registry.

**Status:** `pending`

### LS4 — Session cookie bootstrap and CSP

Implement minimal server-owned `/ui/`, `/ui/auth`, and
`POST /ui/auth/session` routes for bootstrap before the desktop UI exists.
`/ui/` redirects to `/ui/auth` when the signed session cookie is missing.
`/ui/auth` GET never mints a cookie. Set CSP header on all `/ui/*` responses.
DU1 may replace the static body later, but must keep the same middleware and
session contract.

**Verification:** (a) first `/ui/` navigation without a cookie redirects to
`/ui/auth`, (b) GET `/ui/auth` does not set a cookie, (c) POST
`/ui/auth/session` with a valid token sets a cookie, (d) subsequent WebSocket
upgrade succeeds with cookie, (e) invalid or revoked generation returns 401,
(f) CSP header present on `/ui/*` responses, (g) `'unsafe-eval'` absent in
release builds.

**Status:** `pending`

### LS5 — Audit log

Write `{ts, origin, client_kind, user_agent, session_id}` to
`$XDG_STATE_HOME/neovex/logs/access.jsonl` on every WebSocket connection.

**Verification:** (a) log entry written on connection, (b) file created
with correct permissions, (c) log is append-only JSONL.

**Status:** `pending`

## Execution Log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-04-18 | Plan authored | — | Extracted from desktop-ui-plan.md as prerequisite |
