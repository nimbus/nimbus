---
title: Security hardening
description: A checklist for locking down a self-hosted Nimbus server — network exposure, admin token handling, TLS, systemd sandboxing, and encryption at rest.
sidebar:
  label: Hardening
  order: 11
---

Each item below stands alone — work through them in any order. Items marked
**built in** describe behavior the server enforces itself; items marked
**your configuration** are deployment practice you apply around it.

## Keep the listener on loopback

**Built in.** `nimbus start` binds `127.0.0.1:8080` by default and refuses
any non-loopback `--host` unless you explicitly pass `--allow-network`. The
recommended production shape never needs that flag: leave Nimbus on
loopback and let a reverse proxy on the same host be the only public entry
point.

Also built in: browsers only receive CORS approval for `localhost`,
`127.0.0.1`, and `[::1]` origins unless you explicitly allow more with
`--cors-allow-origin` (exact origins only; wildcards are rejected), so
cross-origin browser scripts cannot read responses from the server's
HTTP API by default.

## If you must bind a public interface

**Built in.** Binding a non-loopback host requires two things, enforced at
startup:

1. The `--allow-network` flag.
2. A local admin token that has been explicitly rotated at least once.
   The auto-minted first-boot token never qualifies — the server refuses
   to expose a never-rotated token on a public interface. Once rotated,
   age never blocks a bind: a rotation older than 30 days logs a startup
   warning instead, so a long-running public server always restarts.

To satisfy the rotation gate, rotate as the service user and restart:

```bash
sudo -u nimbus -H nimbus auth rotate-admin
sudo systemctl restart nimbus
```

A running server keeps its in-memory token until restart, so the restart is
what makes the new token live. If the rotation goes stale, the next service
start on a public host fails with an error telling you to rotate again.

## Terminate TLS at a reverse proxy

**Your configuration.** Nimbus serves plain HTTP and does not terminate
TLS. Standard deployment practice: put a TLS-terminating reverse proxy
(nginx, Caddy, HAProxy, or your cloud load balancer) in front, and have it
forward to `http://127.0.0.1:8080`. Because Nimbus stays bound to loopback,
the proxy is the only network path to the server, and the proxy's
certificate handling, request logging, and rate limiting all apply before a
request reaches Nimbus.

WebSocket endpoints (`/ws`, `/convex/{tenant}/ws`) need the proxy's
WebSocket upgrade support enabled.

## Expose only the routes your clients need

**Your configuration**, backed by a built-in gate. The tenant and system
admin routes (`/api/tenants...`, `/api/system/...`, `/api/machines/...`),
the diagnostics endpoints (`/debug/license/status`,
`/debug/encryption/status`, `/debug/runtime/metrics`, and the per-tenant
`/debug/tenants/...` reports), and the native WebSocket endpoint (`/ws`)
all require the local admin token — an unauthenticated request gets a
`401`, and every allow/deny decision on that surface is written to the
audit log. The service, sandbox, and session control routes enforce
per-request authorization of their own, accepting the operator admin token
or verified application credentials. `/health` is the deliberate
exception: it is unauthenticated and returns only `{"ok":true}`.

The token gate is real defense, but these surfaces are operator tooling,
not application API. At the reverse proxy, forward only the paths your
applications actually use (for example the Convex adapter routes under
`/convex/`) and decline to forward `/debug/`, `/ui/`, and `/api/` to the
public internet at all. Layered this way, a token leak alone is not enough
to reach the admin surface from outside.

## Protect the admin token file

**Built in.** Local admin auth is always on — there is no flag to disable
it. The token is a 32-byte random value, stored as JSON at (Linux paths,
relative to the *service user's* home; `XDG_DATA_HOME` is honored when
set):

```text
~/.local/share/nimbus/auth/token
```

The server writes the file with mode `0600` and its parent directory with
mode `0700`, and compares presented tokens in constant time. Requests
authenticate with `Authorization: Bearer <token>` or
`X-Nimbus-Admin-Token: <token>`.

Your part:

- Never copy the token into shell history, unit files, or environment
  blocks that other users can read.
- Rotate on a schedule with `nimbus auth rotate-admin` (run as the service
  user, then restart the service). Rotation bumps the token's `generation`
  and invalidates the old value on restart.
- Treat any token that may have leaked as compromised and rotate
  immediately.

## Watch the audit log

**Built in.** Every authentication decision on the admin surface —
success and failure, with route family, auth method, origin, and reason —
is appended as a JSON line to (Linux, relative to the service user's home;
`XDG_STATE_HOME` honored):

```text
~/.local/state/nimbus/logs/access.jsonl
```

The file is created with mode `0600`. Ship it to your log pipeline and
alert on repeated failures.

## Sandbox the systemd unit

**Your configuration.** These directives describe the unit *you* maintain
(see [Deploy to a Linux server](/operators/deploy-linux/)) — they are
systemd features, not Nimbus behavior. A hardened `[Service]` section for a
server whose state lives under `/var/lib/nimbus`:

```ini
[Service]
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/var/lib/nimbus
ProtectHome=true
PrivateTmp=true
ProtectKernelTunables=true
ProtectControlGroups=true
RestrictSUIDSGID=true
```

`ProtectSystem=strict` mounts the filesystem read-only for the service, so
`ReadWritePaths` must cover everything Nimbus writes: the data directory,
the admin token, state, and logs — all under `/var/lib/nimbus` in the
tutorial layout. If you run hardware-isolated sandbox workloads, the
service additionally needs access to `/dev/kvm`; verify your workloads
still start after every directive you add, and remove the one that breaks
them rather than weakening the rest.

## Encrypt data at rest

**Built in, off by default.** Enable it with
`--encryption-key-provider` (`master-key-file`, `key-dir`, or `aws-kms`)
plus the matching key source flag (`--encryption-master-key-file`,
`--encryption-key-dir`, or `--encryption-aws-kms-key-id`). The same
settings are available as `NIMBUS_ENCRYPTION_*` environment variables. Key
management, migration, and rotation are covered in
[encryption at rest](/operators/encryption/).

## Isolate tenants

**Built in.** `nimbus start` runs in production tenant-isolation mode by
default, every per-tenant route is scoped by tenant ID, and the external
storage backends route each tenant to its own schema (Postgres), database
(MySQL), or namespace (libSQL). For the model and its guarantees, read
[tenant isolation](/concepts/tenant-isolation/); for operating it, see the
[tenant isolation guide](/operators/tenant-isolation/).

## Keep the server current

**Your configuration.** Security fixes ship in regular releases. Follow
[updates](/operators/updates/) for the upgrade procedure, and re-run the
install script (or pull the new [container image](/operators/container-image/))
to pick up new versions — both verify release checksums before installing.
