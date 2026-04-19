# Plan: Localhost Server Security

Canonical execution plan for hardening `neovex serve` as a localhost service:
token-based authentication, origin allowlist, session cookie bootstrap,
Content Security Policy, server discovery, and audit logging. These
protections apply regardless of whether a UI exists — any localhost-exposed
server needs them.

Reviewed against:

- `crates/neovex-server/src/router.rs` — current route tree (no auth
  middleware exists today)
- `crates/neovex-server/src/ws/mod.rs` — WebSocket upgrade handler (no
  origin check, no auth gating)
- `crates/neovex-bin/src/main.rs` — CLI subcommands (`serve`, `machine`,
  `service`); no `token` subcommand exists
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

- The server binds to `0.0.0.0` by default with no authentication on any
  endpoint. The WebSocket upgrade handler checks tenant existence but not
  caller identity.
- No token file, no origin allowlist, no session cookie, no CSP header, no
  audit log, no server discovery file exist today.
- The machine manager already uses XDG paths correctly — this plan extends
  that convention to auth, run state, and logs.
- `ring` 0.17 is already a workspace dependency (ECDSA/Ed25519 in test auth).

## Control Plan Rules

1. File paths follow the established XDG convention — no `~/.neovex/`.
2. No custom crypto. Session cookies are signed using `ring::hmac` with a
   key derived from the token file.
3. Middleware ordering is security-critical: origin → auth → protocol →
   accept. This ordering is documented and tested.
4. No tokens in URLs — headers or cookies only.

## Verification Contract

Each roadmap item must satisfy before closing:

- `cargo fmt --all --check` — clean
- `make clippy` — clean
- `make test` — green
- Manual verification described per item

## Architecture

### File path convention

| Purpose | Path |
| --- | --- |
| Auth token | `$XDG_DATA_HOME/neovex/auth/token` (fallback `~/.local/share/neovex/auth/token`) |
| Server run state | `$XDG_RUNTIME_DIR/neovex/server.json` (fallback `$XDG_STATE_HOME/neovex/run/server.json`) |
| Audit log | `$XDG_STATE_HOME/neovex/logs/access.jsonl` (fallback `~/.local/state/neovex/logs/access.jsonl`) |

On macOS: `~/Library/Application Support/neovex/` (data).
On Windows: `%LOCALAPPDATA%\neovex\`.

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

Generated on first `neovex serve` if absent. `neovex token rotate` bumps
`generation`, invalidating outstanding sessions with `auth.token_revoked`.

### Authentication paths

| Client | How it authenticates | Why |
| --- | --- | --- |
| CLI | Reads token file directly | Same user, same filesystem |
| Native shell | Reads token file, sends `Authorization: Bearer <token>` | Has filesystem access |
| Browser tab | Session cookie set on `/ui/` navigation | Cannot read filesystem |

### Session cookie bootstrap

On navigation to `/ui/`, set a signed `neovex_session` cookie
(`HttpOnly; SameSite=Strict; Path=/`) derived from the token file. Use
`Sec-Fetch-Mode: navigate` as a hint but not the sole gate — if the
request lacks a valid session cookie and is to any `/ui/*` path, redirect
to `/ui/auth` which always sets the cookie on GET. This handles browser
prefetch, service workers, and extensions that may not preserve fetch
metadata headers.

### Middleware ordering

Request flows through layers in this order (outermost first):

```
trace → request_id → origin_allowlist → rate_limit → auth_extract → protocol_select → ws_upgrade
```

- Origin before auth prevents leaking token-validity timing to hostile origins.
- Protocol after auth avoids wasting parser work on unauthenticated requests.
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
`server.json` on `neovex serve` bind with `RemoveOnDrop` guard. PID
liveness check on startup for stale file recovery.

**Verification:** (a) `server.json` written on bind with correct PID/address,
(b) file removed on clean shutdown, (c) stale file from SIGKILL cleaned on
next startup, (d) paths respect `$XDG_DATA_HOME`, `$XDG_RUNTIME_DIR`,
`$XDG_STATE_HOME` when set.

**Status:** `pending`

### LS2 — Token file lifecycle and CLI subcommand

Generate 256-bit token on first `neovex serve`, write to
`$XDG_DATA_HOME/neovex/auth/token` with `0600` permissions (ACL on
Windows). Add `Command::Token(TokenCommand)` to the CLI with
`TokenSubcommand::Rotate` that bumps `generation`.

**Verification:** (a) token file created on first serve, (b) permissions
are 0600, (c) `neovex token rotate` increments generation, (d) token
file is reused across restarts, (e) Windows ACL restricts to current user.

**Status:** `pending`

### LS3 — Origin allowlist and middleware stack

Implement origin allowlist middleware on the WebSocket upgrade path.
Implement the full middleware ordering: trace → request_id →
origin_allowlist → rate_limit → auth_extract → protocol_select →
ws_upgrade. Bind to loopback only by default.

**Verification:** (a) non-allowlisted origin → 403, (b) allowlisted origin
with invalid token → 401, (c) `/health` bypasses auth, (d) ordering
confirmed via integration test that checks 403 before 401 for bad origin +
bad token.

**Status:** `pending`

### LS4 — Session cookie bootstrap and CSP

Implement signed session cookie on `/ui/` navigation. Implement `/ui/auth`
redirect fallback for requests lacking a valid cookie. Set CSP header on
all `/ui/*` responses.

**Verification:** (a) first `/ui/` navigation sets cookie, (b) subsequent
WebSocket upgrade succeeds with cookie, (c) `/ui/auth` redirect sets cookie
without `Sec-Fetch-Mode`, (d) CSP header present on `/ui/*` responses,
(e) `'unsafe-eval'` absent in release builds.

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
