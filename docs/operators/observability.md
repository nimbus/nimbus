---
title: Inspect the server
description: Inspect a running Nimbus server — health, debug endpoints, runtime metrics, log configuration, and the access audit log.
sidebar:
  label: Inspect the server
  order: 11
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
  `premium_support`, `custom_terms`, `sso`, `audit_logs`, and `multi_node`.
  Expired trial and enterprise licenses report all flags as `false`.
- `usage` — live monthly-active-user accounting: `month`,
  `month_start_unix_ms`, `monthly_active_users`, and — when the license
  sets a limit — `limit` and `limit_exceeded`.
- `warnings` — present when the license is expired, expiring soon, or
  usage is at or over a configured limit. Watch this array in monitoring.

The license file is unsigned, operator-supplied status metadata. Nimbus does
not cryptographically verify it or gate feature execution from it; the binding
terms remain the repository's `LICENSE`. See the repository's `LICENSING.md`
for the complete operational explanation.

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
  `lane_name`, `default_lane`, `executor_started`,
  `execution_adapter_state`, `execution_adapter_artifact`, and each lane's
  effective limits.

The lane list distinguishes API compatibility from the engine that enforces
it:

| Lane | Default | Runtime backend | Compatibility target | Adapter state | Executor | Memory enforcement |
| --- | --- | --- | --- | --- | --- | --- |
| `default` | yes | `v8` | `web_standard_isolate` | `linked` | lazy until invoked | `v8_isolate_heap_limit` |
| `node20` | no | `v8` | `node20` | `linked` | lazy until invoked | `v8_isolate_heap_limit` |
| `node22` | no | `v8` | `node22` | `linked` | lazy until invoked | `v8_isolate_heap_limit` |
| `node24` | no | `v8` | `node24` | `linked` | lazy until invoked | `v8_isolate_heap_limit` |
| `node26` | no | `v8` | `node26` | `linked` | lazy until invoked | `v8_isolate_heap_limit` |
| `bun_jsc` | no | `bun_jsc` | `bun_jsc` | `not_linked` unless the verified adapter is loaded | lazy until invoked | `outer_quota_required` |

For Bun/JSC, `not_linked` means the lane is visible but the optional
execution adapter has not been loaded. The lane remains fail-closed in that
state. `outer_quota_required` means memory enforcement depends on the
outer runtime quota rather than V8's isolate heap limit.

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
(write admission gate), `mutation_isolate_admission` (the current and peak
concurrent mutation-isolate counts, configured ceiling, bounded-wait depth,
and shed count), `mutation_journal` (durable journal progress),
`subscription_delivery` (reactive delivery queue),
`materialized_read_surface` and `serving_snapshot_manager` (read-path
caches), `query_planning`, and `libsql_replica_freshness` (null unless the
tenant runs on a libSQL replica). Unknown tenants return `404`.

The `mutation_journal` group reports the causal progress of accepted work as
six ordered heads:

```text
assigned_high_water >= active_assigned_head >= durable_head
                    >= storage_applied_head >= published_head >= applied_head
```

`assigned_high_water` preserves assignment history, including a provisional
suffix later proven not to have committed. `active_assigned_head` is that
history with definitively uncommitted work removed. The remaining heads show
what is durable, applied in storage, admitted through the contiguous
publication barrier, and visible to readers. A snapshot reconciles concurrent
observations into this causal order; collecting it never advances production
state.

Use the four adjacent-phase lags to locate a stall:

- `assignment_lag` means accepted work is waiting to become durable.
- `apply_lag` means durable records are waiting for storage application.
- `publication_lag` means storage-applied records are waiting behind the
  contiguous publication barrier.
- `visibility_lag` means published records have not yet advanced the reader
  watermark.

Alert on a sustained nonzero value, not a single point-in-time sample. A head
that stops moving while its preceding lag grows identifies the owning phase;
`assigned_high_water` alone is historical and is not a backlog measure.

| Observed pattern | Interpretation | Check next |
| --- | --- | --- |
| One small lag, moving heads, queues below capacity, and no rising error counters | Healthy transient backlog | Confirm the lag drains over the normal traffic interval. |
| Rising `assignment_lag` with `publisher_queue_depth` approaching `publisher_queue_capacity` or rising `publisher_send_timeout_count` | Publisher admission or persistence is overloaded | Reduce offered write load; inspect provider latency and publisher errors. |
| Rising `apply_lag` with a moving `durable_head` | Durable records are not completing storage application | Inspect provider health and `publisher_transient_error_count`/`publisher_fatal_error_count`. |
| Rising `publication_lag` | Applied provider progress is held behind an earlier pending contiguous-prefix barrier | Inspect the lower assigned batch, publisher errors, and provider catch-up failures; do not treat the later applied suffix as visible. |
| Rising `visibility_lag` | Publication completed but the applied waiter watermark has stalled | Inspect journal worker health and restart/failure counters. |
| Admission `queue_rejection_count`, committer send timeouts, or publisher send timeouts rising | Bounded backpressure is rejecting work | Identify whether admission, committer inbox, or publisher capacity is saturated before raising a bound. |
| `committer_lease_fenced: true`, renewal failures, or a stopped renewal worker on a loaded provider tenant | Lease authority was lost or cannot be maintained | Expect tenant-local fencing and runtime replacement; investigate provider clock/availability and a competing owner. |
| Repeated `scheduler failed for tenant; applying tenant-local bounded retry` events for one tenant | This process observed due scheduler state but could not complete its authority-fenced transition | Check whether another healthy process owns the tenant lease. The log's `retry_after_millis` is a monotonic local delay; other tenants remain runnable. If no healthy owner exists, investigate provider availability or stalled takeover. |
| Rising `publisher_ambiguous_error_count` with a crash-and-replay log | Commit acknowledgement was ambiguous | Expect tenant-local eviction and replay. Do not retry as a definitive non-commit until the rebuilt runtime reports healthy progress. |
| Rising `provider_catch_up_failure_count`, observer catch-up failures, or reconciliation backoff with stalled heads | Restart or takeover recovery is stalled | Inspect provider reads and observer reconciliation; the tenant remains unavailable until the contiguous prefix is recovered. |

The group also reports bounded system-table projection work. Use these fields
together:

- `committer_arm` is the immutable construction-time mutation owner:
  `ordered-publisher` for every production persistence topology. It never
  changes for a live runtime; there are intentionally no transition counters.
  `serial-reference` can appear only in deterministic test-hook runs and is
  not a provider fallback. Rolling back the publisher requires reverting the
  deployed code, not switching a live tenant to an unfenced owner.
- `committer_lease_acquired`, `committer_lease_epoch`,
  `committer_lease_expires_at`, and `committer_lease_fenced` distinguish a
  provider runtime that has not written yet, a healthy holder, and a holder
  that lost authority. Acquisition/renewal counters and
  `committer_lease_renewal_worker_running` separate ordinary backlog from
  lease loss or stalled lifecycle work. Embedded tenants keep these lease
  fields at their neutral values.
  Provider tenants still use storage-backed validation for prepared windows;
  the ordered publisher arm does not make a process-local snapshot
  authoritative.
  Scheduler-state transitions (insert, claim, completion, cancellation,
  result/cron persistence, and startup recovery) do not advance the journal
  heads, but they enter this same per-tenant publisher. On external providers,
  the scheduler row change and current-lease validation share one transaction;
  a stale scheduler writer therefore sets the same fenced/eviction signals
  instead of producing an invisible second-owner write. Runtime load only arms
  one-shot running-job recovery; it executes after the serialized committer
  acquires authority. Lease contention is retried on a bounded, tenant-local
  monotonic deadline and is reported by the event in the decision table rather
  than spinning on an already-due durable row.

- `observer_spawned_work_depth` is currently executing or queued observer
  work. Compare it with `observer_spawned_work_capacity` and
  `observer_spawned_work_high_watermark` to identify a healthy backlog versus
  saturation.
- `observer_spawned_work_dirty_scope_count` counts coalesced table scopes that
  still require publication. It is bounded by affected table scopes, not the
  number of dropped events.
- `observer_spawned_work_token_lag_scope_count` counts table scopes whose
  highest observed `(tenant incarnation, lease epoch, durable sequence)` token
  has not yet been published. It includes accepted in-flight work, so it can
  be nonzero while the dirty-scope count remains zero. A same-id tenant
  recreation advances the incarnation; stale-no-op activity from the prior
  incarnation is expected briefly while late work drains.
- `observer_spawned_work_stale_no_op_count` is cumulative. It increases when a
  persisted fence already covers an observer attempt; occasional increases are
  expected during replay or provider takeover, while a sustained high rate can
  indicate a lagging observer process.
- `observer_spawned_work_delayed_retry_count` is cumulative. An increase means
  an error or cancellation retained work for a delayed retry.
- `observer_spawned_work_consecutive_failure_count` and
  `observer_spawned_work_current_retry_backoff_millis` describe the current
  failure streak. Both return to zero after the retained scopes land.
- `observer_spawned_work_reconciliation_retry_count` is cumulative, while
  `observer_spawned_work_current_reconciliation_backoff_millis` reports a
  runtime-load reconciliation that is currently retrying before table scopes
  can be enumerated. This distinguishes restart/takeover recovery failure from
  a known dirty projection scope.
- `observer_spawned_work_cap_breach_count` and
  `observer_spawned_work_dropped_event_count` identify overload admission. A
  dropped observer event is retained as a dirty scope; it is not reported as a
  successful projection.

A nonzero token-lag or dirty-scope count with rising retry and backoff fields
indicates stalled recovery, including persistent storage or `_nimbus` lease
contention. A nonzero reconciliation backoff means recovery is stalled earlier,
while discovering the active and previously projected scopes. Alert on a
sustained value rather than one retry: cumulative counters remain as history
after recovery, while lag, dirty, consecutive-failure, and current-backoff
fields clear.

## Verify a tenant's consistency

```bash
curl -s http://localhost:8080/debug/tenants/demo/consistency \
  -H "Authorization: Bearer $NIMBUS_TOKEN" | jq '.ok, .mismatches'
```

This runs the on-demand consistency verifier: it fingerprints the
authoritative snapshot, the shadow materializer, and the embedded replica,
checks the journal bootstrap cut, and compares them. The report contains:

- `ok` — `true` when every position agrees. Alert on `false`.
- `authoritative`, `shadow`, `embedded_replica` — one fingerprint each:
  `position` (`version`, `applied_sequence`, `state_digest`),
  `snapshot_version`, `durable_head`, `schema_table_count`,
  `document_count`, `scheduled_execution_count`. Two scopes agree when their
  `position` agrees: the same `applied_sequence` with a different
  `state_digest` means the state diverged at an unchanged sequence.
- `bootstrap` — the journal bootstrap fingerprint
  (`snapshot_position`, `resume_after_sequence`, `bootstrap_cut_sequence`,
  `cursor_floor_sequence`).
- `mismatches` — empty when healthy; otherwise each entry names the
  violated `invariant`, the two scopes compared, the `path`, and both
  sides' descriptions.

The verifier reads live state, so run it on demand or on a coarse schedule
rather than in a tight loop.

Each request also emits a structured
`action="tenant_consistency_verification"` log event. A clean report uses
`INFO`. A report with mismatches or a verification failure uses `WARN`. The
event includes the tenant, requested full-scrub flag, verification mode,
escalation reason, mismatch count, journal event count, and anchor sequence;
the mismatch event also names the first failed invariant. Alert on the warning
event so a scheduled check cannot return a divergent report without an
operator-visible signal.

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
