# Metadata Retention Runbook

Nimbus bounds document history, index history, CDC history, and point-in-time
restore history by durable sequence count. The policy is not a time promise.
The same window can represent minutes on a high-write tenant and months on a
low-write tenant.

## Shipped Profile

| Resource | Retained sequences |
| --- | ---: |
| Document versions | 100,000 |
| Index versions | 100,000 |
| CDC cursor validity | 50,000 |
| PITR target validity | 100,000 |
| Maintenance eligibility step | 10,000 |

The physical journal keeps the most conservative active dependency. A CDC
cursor can expire before a PITR target at the same sequence because the two
logical windows are separate.

## Configure The Profile

The server accepts two profiles:

```text
nimbus start --metadata-retention bounded
nimbus start --metadata-retention retain-all
```

`bounded` is the default. `retain-all` disables automatic checkpoint advance
and physical history deletion. It makes unbounded storage growth an explicit
operator choice.

The environment variable is `NIMBUS_METADATA_RETENTION`. A configuration file
uses the `metadata_retention` key. Accepted values are `bounded` and
`retain-all`. Precedence is command line, environment, configuration file,
then the bounded default.

The server presets do not accept custom window numbers. An embedder can use
`MetadataRetentionProfile::bounded(...)` and must supply five nonzero values:
the four windows and the maintenance step. Keep PITR at least as long as the
recovery objective and keep CDC at least as long as the maximum supported
consumer outage.

## Read Diagnostics

The local-admin metrics route contains `diagnostics.metadata_retention`:

```text
GET /debug/tenants/{tenant_id}/engine/metrics
```

Use the local admin token as `Authorization: Bearer` or
`X-Nimbus-Admin-Token`. Do not copy the token into a ticket, log, or retained
shell transcript.

```bash
nimbus_admin_token="$(nimbus auth token)"
nimbus_base_url="http://127.0.0.1:8080"
curl --fail --silent --show-error \
  --header "Authorization: Bearer ${nimbus_admin_token}" \
  "${nimbus_base_url}/debug/tenants/TENANT_ID/engine/metrics"
unset nimbus_admin_token
```

Interpret the fields as follows:

| Field | Meaning |
| --- | --- |
| `profile` | Effective bounded or retain-all policy and its windows. |
| `controller_running` | The tenant controller is accepting lifecycle work. |
| `maintenance_running` | One checkpoint prepare or finalize is active. |
| `run_count`, `success_count`, `retention_failure_count` | Cumulative controller outcomes for this process. |
| `desired_floor` | The policy and active pins permit deletion through this sequence. |
| `confirmed_floor` | A durable materialized checkpoint covers this sequence. |
| `physical_floor` | Journal rows through this sequence are confirmed deleted. |
| `retention_floor_lag_sequences` | `desired_floor - confirmed_floor`. |
| `retention_*_pruned` | Cumulative journal, document-version, and index-version rows removed by this process. |
| `retention_last_duration_millis` | Duration of the last completed maintenance attempt. |
| `last_failure` | Last preparation or finalization failure. Success clears it. |
| `next_eligible_floor` | Sequence hint for the next automatic bounded run. |
| `next_retry_in_millis` | Delay before retry after a failure. |

The safe ordering is:

```text
physical_floor <= confirmed_floor <= desired_floor <= latest_sequence
```

Report a violation as a storage-integrity incident. Do not edit metadata keys
or delete journal rows to repair it.

## Run One Maintenance Cycle

The controller runs automatically. Use the manual route after a configuration
change, provider recovery, or diagnosis when you need one ordered result:

```text
POST /debug/tenants/{tenant_id}/engine/retention
```

```bash
nimbus_admin_token="$(nimbus auth token)"
nimbus_base_url="http://127.0.0.1:8080"
curl --fail --silent --show-error --request POST \
  --header "Authorization: Bearer ${nimbus_admin_token}" \
  "${nimbus_base_url}/debug/tenants/TENANT_ID/engine/retention"
unset nimbus_admin_token
```

The result names whether compaction ran, all three floors, each pruned count,
and duration. A successful no-op is valid when the policy has not advanced by
one maintenance step or an active pin keeps the safe floor behind the desired
window.

Do not run parallel manual requests as a throughput tool. The tenant
controller is deliberately single-flight, and provider finalization must use
the current committer lease.

## Alerts

Use bounded labels. Do not put tenant IDs, document IDs, table names, state
bytes, or SQL text in metric labels.

- Page on `physical_floor > confirmed_floor` or a floor that moves backward.
  These states violate the durable checkpoint contract.
- Warn when `retention_failure_count` increases or `last_failure` is non-null.
  Capture the provider error and the three floors before retry.
- Warn when `retention_floor_lag_sequences` stays above the configured
  maintenance step while `maintenance_running` is false for two 30-second
  recheck periods. A short lag is normal while a checkpoint is prepared.
- Alert on storage capacity separately for `retain_all`. A zero lag in that
  mode does not mean storage is bounded.
- Treat `RetentionExpired` for a cursor or PITR target below its published
  floor as an expected client contract. Alert only on an unexpected rate or a
  target that should still be inside the configured window.

## Failure And Recovery

Preparation is read-only. Finalization publishes the checkpoint, floors, and
deletes in one transaction. A failed attempt retains history and retries after
one second; low-write tenants are also checked every 30 seconds.

1. Capture the metrics response and `last_failure`.
2. Confirm that the tenant provider is reachable and that the current Engine
   still owns the provider committer lease.
3. Confirm free space for an embedded database and its parent directory.
4. Let the automatic retry run. If the cause is repaired, use one manual cycle
   and confirm that `confirmed_floor` and `physical_floor` advance together.
5. If restart is required, stop Nimbus cleanly. Startup reloads the durable
   checkpoint and floors before the controller resumes.

Never remove the checkpoint, lower a durable floor, delete a provider journal
prefix directly, or copy only the main database file without its normal
snapshot sidecars. Such actions can make retained history appear available
when its rebuild base is absent.

## Horizontal Scaling Handoff

Today, one process fence protects an embedded root and one provider committer
lease protects each provider tenant. Do not start a second retention leader or
add a separate lease system. The horizontal-scaling owner can replace the
authority source, but it must preserve the same atomic finalization contract,
typed expired-history behavior, and single active tenant finalizer.

## Verification

Run the semantic and benchmark gates from the repository root:

```bash
cargo test -p nimbus-storage generated_retained_checkpoint -- --nocapture
cargo test -p nimbus-storage retention_checkpoint -- --nocapture
cargo bench -p nimbus-storage --bench metadata-retention-baseline
bash scripts/verify-storage-metadata-retention.sh
```

External provider qualification must name PostgreSQL, MySQL, and libSQL as
configured, passed, failed, or skipped. A missing fixture is `UNVERIFIED`, not
a pass. Follow [`verification.md`](verification.md) for fixture and exit-status
rules.
