---
title: Inspect the server
description: Inspect a running Nimbus server — health, debug endpoints, runtime metrics, log configuration, and the access audit log.
sidebar:
  label: Inspect the server
  order: 10
---

This guide shows you how to look inside a running Nimbus server: confirm it
is alive, read its license, encryption, runtime, and per-tenant diagnostics
over HTTP, tune its log output, and find the access audit log on disk.

Every inspection surface is built in — there is nothing to install or
enable. You need a running server (see the
[self-host quickstart](/get-started/self-host/) or
[Deploy to Linux](/operators/deploy-linux/)) and `curl`. `jq` makes the
JSON easier to read.

## Endpoints at a glance

| Method | Path | Returns | Credential |
| --- | --- | --- | --- |
| GET | `/health` | liveness | none |
| GET | `/debug/license/status` | license and entitlement status | admin token |
| GET | `/debug/encryption/status` | encryption-at-rest status | admin token |
| GET | `/debug/runtime/metrics` | runtime limits and live metrics | admin token |
| GET | `/debug/tenants/{tenant_id}/engine/metrics` | per-tenant engine diagnostics | admin token |
| GET | `/debug/tenants/{tenant_id}/consistency` | on-demand consistency verification | admin token |
| GET | `/api/system/version-info` | version and update status | admin token |

## Authenticate first

Everything except `/health` requires the local admin token. The server
creates it on first boot and stores it as a JSON file:

```bash
# Linux
export NIMBUS_TOKEN=$(jq -r .token ~/.local/share/nimbus/auth/token)

# macOS
export NIMBUS_TOKEN=$(jq -r .token "$HOME/Library/Application Support/nimbus/auth/token")
```

On Windows the file is `%LOCALAPPDATA%\nimbus\auth\token.json`.

Send the token either as a bearer or in the dedicated header — both work on
every endpoint in this guide:

```bash
curl -s http://localhost:8080/debug/license/status \
  -H "Authorization: Bearer $NIMBUS_TOKEN"

curl -s http://localhost:8080/debug/license/status \
  -H "X-Nimbus-Admin-Token: $NIMBUS_TOKEN"
```

Missing or wrong credentials return `401` with code `auth.unauthorized` in
the standard [error envelope](/reference/native/errors/). If your client
sends a browser `Origin` header, it must be a loopback HTTP origin
(`localhost`, `127.0.0.1`, or `[::1]`) on the server's port — anything else
returns `403`. Plain `curl` sends no `Origin` header and is unaffected.

## Check liveness

```bash
curl -s http://localhost:8080/health
```

```json
{ "ok": true }
```

`/health` requires no credential, so point load-balancer and systemd-style
health checks at it directly. It answers as soon as the HTTP listener is
up; there is no separate readiness endpoint.

## Read license status

```bash
curl -s http://localhost:8080/debug/license/status \
  -H "Authorization: Bearer $NIMBUS_TOKEN" | jq
```

The response reports where the license came from and what it allows:

- `source` — `kind` is `community_default`, `explicit_file`, or
  `environment_file` (the `NIMBUS_LICENSE_FILE` environment variable), plus
  the file `path` when one was loaded.
- `kind` / `status` — `community`, `trial`
  (`trial_active`/`trial_expired`), or `enterprise`
  (`enterprise_active`/`enterprise_expired`).
- `entitlements` — boolean flags: `hosted_service`, `oem_embedding`,
  `premium_support`, `custom_terms`, `sso`, `audit_logs`, `backup_api`,
  `multi_node`.
- `usage` — live monthly-active-user accounting: `month`,
  `month_start_unix_ms`, `monthly_active_users`, and — when the license
  sets a limit — `limit` and `limit_exceeded`.
- `warnings` — present when the license is expired, expiring soon, or
  usage is at or over a configured limit. Watch this array in monitoring.

## Check encryption status

```bash
curl -s http://localhost:8080/debug/encryption/status \
  -H "Authorization: Bearer $NIMBUS_TOKEN" | jq
```

On a server without encryption at rest configured:

```json
{
  "enabled": false,
  "encrypted_families": [],
  "descriptor": { "status": "disabled" }
}
```

When encryption is on, `encrypted_families` lists the protected storage
families (`embedded_sqlite`, `embedded_redb`, `control_plane_redb`,
`libsql_replica_cache`) and `descriptor` identifies the key provider
(`master_key_file`, `key_directory`, or `aws_kms`) with its path or key ID
— never key material. To set up encryption, see
[Encryption at rest](/operators/encryption/).

## Read runtime metrics

```bash
curl -s http://localhost:8080/debug/runtime/metrics \
  -H "Authorization: Bearer $NIMBUS_TOKEN" | jq
```

This endpoint always returns `200` with a stable shape. On a server with no
deployed app the top-level fields are null:

```json
{ "limits": null, "reset_capabilities": null, "metrics": null, "lanes": [] }
```

Once an app generation is active you get three things:

- `limits` — the effective runtime configuration: backend kind and trust
  tier, execution model, heap sizes (`max_heap_mb`, `initial_heap_mb`),
  `execution_timeout_ms`, concurrency caps
  (`max_concurrent_runtime_instances`, `worker_threads`, per-tenant
  invocation caps), and the per-tenant `tenant_budget` breakdown.
- `metrics` — live counters: `started_invocations`,
  `completed_invocations`, `timed_out_invocations`, `canceled_invocations`,
  `rejected_invocations`, queue depth (`queued_invocations`), pool
  efficiency (`runtime_pool_hits`/`runtime_pool_misses`,
  `warm_pool_hits`/`warm_pool_misses`), bundle load counts and timings,
  cumulative `queue_wait_nanos_total` and `execution_nanos_total`
  (nanoseconds), plus per-host-operation and per-tenant counter maps and a
  ring of `recent_request_correlations` for tracing recent invocations.
- `lanes` — the same limits/metrics breakdown per runtime lane, with
  `lane_name`, `default_lane`, and `executor_started` so you can confirm
  each configured lane actually started.

Track the ratio of `rejected_invocations` and `timed_out_invocations` to
`started_invocations`, and watch `queued_invocations` for sustained
backlog.

## Inspect a tenant's engine

```bash
curl -s http://localhost:8080/debug/tenants/demo/engine/metrics \
  -H "Authorization: Bearer $NIMBUS_TOKEN" | jq
```

Returns `{ "tenant_id": "demo", "diagnostics": { ... } }` where
`diagnostics` groups per-tenant engine state: `mutation_admission`
(write admission gate), `mutation_journal` (durable journal progress),
`subscription_delivery` (reactive delivery queue),
`materialized_read_surface` and `serving_snapshot_manager` (read-path
caches), `query_planning`, and `libsql_replica_freshness` (null unless the
tenant runs on a libSQL replica). Unknown tenants return `404`.

## Verify a tenant's consistency

```bash
curl -s http://localhost:8080/debug/tenants/demo/consistency \
  -H "Authorization: Bearer $NIMBUS_TOKEN" | jq '.ok, .mismatches'
```

This runs the on-demand consistency verifier: it fingerprints the
authoritative snapshot, the shadow materializer, and the embedded replica,
checks the journal bootstrap cut, and compares them. The report contains:

- `ok` — `true` when every fingerprint agrees. Alert on `false`.
- `authoritative`, `shadow`, `embedded_replica` — one fingerprint each:
  `digest`, `applied_sequence`, `durable_head`, `schema_table_count`,
  `document_count`, `scheduled_execution_count`.
- `bootstrap` — the journal bootstrap fingerprint
  (`snapshot_digest`, `resume_after_sequence`, `bootstrap_cut_sequence`,
  `cursor_floor_sequence`).
- `mismatches` — empty when healthy; otherwise each entry names the
  violated `invariant`, the two scopes compared, the `path`, and both
  sides' descriptions.

The verifier reads live state, so run it on demand or on a coarse schedule
rather than in a tight loop.

## Check version and update status

```bash
curl -s http://localhost:8080/api/system/version-info \
  -H "Authorization: Bearer $NIMBUS_TOKEN" | jq
```

Returns the running version (`current`), the newest known release
(`latest`, `url`, `publishedAt`), whether an upgrade is `available`, and a
suggested `upgrade` action for the detected install method. `checkStatus`
tells you how fresh that answer is: `never`, `fresh`, `stale`, `error`, or
`disabled`. The server checks the GitHub releases feed at most once per 24
hours; set `NIMBUS_DISABLE_UPDATE_CHECK=1` before starting the server to
disable the outbound check entirely (then `checkStatus` reports
`disabled`).

## Configure logging

The server logs human-readable lines to stdout. Filtering is controlled by
the standard `RUST_LOG` environment variable; the default level is `info`
when it is unset.

```bash
# Everything at debug
RUST_LOG=debug nimbus start --port 8080

# Quiet overall, verbose for the server crate only
RUST_LOG=warn,nimbus_server=debug nimbus start --port 8080
```

`RUST_LOG` accepts the usual `target=level` directive list
(`error`, `warn`, `info`, `debug`, `trace`). There is no `NIMBUS_LOG`
variable. Under systemd the log stream lands in the journal — see
[Deploy to Linux](/operators/deploy-linux/).

### Latency warnings

At the default `info` level you will still see `WARN` events when a
budgeted hot-path segment runs slow. Each event carries structured fields:
`latency_segment` (a stable name such as `engine.query_execute` or
`server.auth`), `elapsed_ms`, and `budget_ms`. These are emitted only when
a segment exceeds its internal budget, so they are low-volume and safe to
alert on as an early latency signal.

## Read the access audit log

Every request to an admin-gated route family is appended as one JSON line
to an audit log — successes and failures, with the credential method used
and the reason:

| Platform | Path |
| --- | --- |
| Linux | `~/.local/state/nimbus/logs/access.jsonl` (honors `XDG_STATE_HOME`) |
| macOS | `~/Library/Logs/nimbus/access.jsonl` |
| Windows | `%LOCALAPPDATA%\nimbus\logs\access.jsonl` |

Each record has `ts`, `routeFamily` (for example `debug`, `native_api`,
`deploy_admin`), `tenantId`, `authScope`, `authMethod`
(`local_admin_bearer`, `local_admin_header`, or `local_session_cookie`),
`success`, `origin`, and `reason`. Tail recent failures with:

```bash
tail -n 50 ~/.local/state/nimbus/logs/access.jsonl | jq 'select(.success == false)'
```

A burst of `success: false` records from an unexpected origin is your
signal to rotate the admin token (`nimbus token rotate`, or
`POST /api/system/token/rotate`).
