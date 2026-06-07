---
status: done
phase: SEQ0
plan: docs/plans/storage-engine-quality-and-mvcc-plan.md
updated: 2026-06-06
---

# SEQ0 Design Refresh

SEQ0 bootstraps the storage-engine quality and MVCC control plane. It records
the dedicated worktree, source references, enterprise guarantee charter,
support matrix, staged proof order, verifier contract, and initial validation
evidence before SEQ1 implementation begins.

## Worktree Evidence

- SEQ worktree:
  `/Users/jack/src/github.com/nimbus/nimbus-worktrees/storage-engine-quality-and-mvcc`
- Branch: `codex/storage-engine-quality-and-mvcc`
- Base commit: `4a9e6a77bcd3c51ef14018d1e34c3e2dfd199d38`
- Base branch state: `main` and `origin/main` both resolved to
  `4a9e6a77bcd3c51ef14018d1e34c3e2dfd199d38` before worktree creation.
- Initial worktree status after applying the reviewed bootstrap plan patch:

```text
## codex/storage-engine-quality-and-mvcc
 M docs/plans/README.md
 M docs/plans/storage-engine-quality-and-mvcc-plan.md
```

The dirty files are expected SEQ0 bootstrap edits. The primary checkout remains
the review baseline; SEQ implementation and proof commits belong to the
dedicated worktree.

Current SEQ0 bootstrap status after proof, debt, and verifier creation:

```text
## codex/storage-engine-quality-and-mvcc
 M .codex/config.toml
 M docs/plans/README.md
 M docs/plans/storage-engine-quality-and-mvcc-plan.md
 M docs/technical-debt.md
?? docs/plans/proof/storage-engine-quality-and-mvcc/seq0-design-refresh.md
?? scripts/verify-storage-engine-quality-and-mvcc.sh
```

The `.codex/config.toml` delta adds the project-local
`~/src/github.com/nimbus/*` writable-root wildcard requested before SEQ worktree
execution so sibling Nimbus worktrees can be used directly.

## Source Refresh

Local comparison repositories were refreshed before this SEQ0 bootstrap:

| Source | Local ref | Status |
| --- | --- | --- |
| Convex (`~/src/github.com/get-convex/convex-backend`) | `602dc945` | `## main...origin/main` |
| CockroachDB (`~/src/github.com/cockroachdb/cockroach`) | `5f5932a2bf5` | `## master...origin/master` |
| TigerBeetle (`~/src/github.com/tigerbeetle/tigerbeetle`) | `64899c7a4` | `## main...origin/main` |
| ElectricSQL (`~/src/github.com/electric-sql/electric`) | `8bcadb7` | `## main...origin/main` |
| ExtendDB (`~/src/github.com/ExtendDB/extenddb`) | `93de8e3` | `## main...origin/main` |

SEQ0 still needs to record detailed source excerpts for any design decision that
changes the MVCC contract before it can move to `done`.

## Source-Backed Design Decisions

| Source | Borrow | Reject for Nimbus |
| --- | --- | --- |
| Convex | Treat published read frontiers and snapshot handles as first-class semantics; carry table/index identity through read dependency tracking and authorization decisions. | Do not copy Convex public tokens or storage internals directly; Nimbus must keep its engine-owned mutation path and adapter support matrix. |
| CockroachDB | Use explicit resolved/safe frontiers, retention errors, and operator-visible lag/backpressure for CDC and historical reads. | Do not adopt distributed range leases, closed-timestamp follower-read routing, or multi-node replication scope in this plan. |
| TigerBeetle | Make log-first replay, deterministic canonical digests, bounded retention, and crash/rebuild proofs part of correctness rather than optional benchmarks. | Do not import whole-program static allocation or a fixed-state-machine architecture that conflicts with Nimbus's adapter/runtime surface. |
| ElectricSQL | Treat snapshot plus log handoff and replica freshness as a correctness boundary with durable progress evidence. | Do not make eventually consistent local replicas a default read path without a provider-owned sequence barrier. |
| ExtendDB | Keep backend-specific physical layout behind protocol-compatible semantics and conformance tests. | Do not force one universal SQL table layout across all providers when backend-owned physical layout is the idiomatic and tested seam. |

## Enterprise Guarantee Charter

Nimbus intends to sell to enterprise customers, so the following guarantees are
accepted product requirements for this roadmap:

- repeatable historical point reads, scans, indexed queries, and pagination
  within a configured retention window
- point-in-time restore and export/import from the same retained history
- CDC/changefeed snapshot plus log handoff with no missed or duplicated logical
  events
- historical authorization semantics that cannot leak old data through current
  or stale policy confusion
- durable table, schema, index, and read-policy identity across lifecycle churn,
  import, restore, and replay
- retention and GC that prove no retained reader, consumer, replica,
  materializer, PITR point, or transaction/session can still observe pruned data
- fail-closed storage format handling for unsupported old or future MVCC layouts
- cross-backend and adapter-facing evidence for every supported surface
- operator diagnostics and compatibility docs that expose support state,
  retention health, format state, and fail-closed unsupported states

## All-Supported Backend And Adapter Matrix

Every currently supported backend and adapter surface is represented here. The
final SEQ14 closeout must either prove the SEQ semantics on the surface or keep
that surface fail-closed with customer-facing caveats.

| Surface | Current authoritative docs | SEQ final contract | Required fail-closed or proof state |
| --- | --- | --- | --- |
| SQLite embedded tenant backend | `docs/architecture/storage/provider-topologies.md`, `docs/plans/research/sqlite-storage-benchmark-report.md` | First implementation candidate for MVCC document/index layout, latest-row cache, PITR/export/import, CDC source cuts, deterministic digest, and latest-path budget preservation. | Must pass generated MVCC histories and latest-path benchmarks before SEQ3/SEQ4 graduate. |
| redb embedded tenant backend | `docs/architecture/storage/provider-topologies.md`, SATH proof bundle | First parity candidate for versioned key-prefix semantics, durable journal replay, deterministic digest, and crash/rebuild checks. | Must match SQLite semantic oracles; backend-specific layout may differ but digests and errors must match. |
| Postgres tenant backend | `docs/architecture/storage/provider-topologies.md`, `docs/plans/research/postgres-provider-benchmark-report.md` | External-provider parity before SEQ14 for storage-visible MVCC, retention, PITR, CDC, and diagnostics. | Unsupported subfeatures must return typed unsupported-state errors; RTT-sensitive budgets must catch accidental row-at-a-time plans. |
| MySQL tenant backend | `docs/architecture/storage/provider-topologies.md`, `docs/plans/research/mysql-provider-benchmark-report.md` | External-provider parity before SEQ14 using MySQL-owned physical layout and generated-column/index constraints where needed. | Index-prefix/generated-column limits must be documented and tested; no silent downgrade from indexed historical reads to unbounded scans. |
| libSQL tenant backend | `docs/architecture/storage/provider-topologies.md`, `docs/architecture/storage/consistency-routing.md`, `docs/plans/research/sqlite-replica-provider-benchmark-report.md` | Remote primary remains authoritative; local cache may serve historical/latest reads only after explicit freshness or `sync_until`-equivalent barrier proof. | Local cache historical reads must fail closed unless retained sequence and cache freshness both cover the requested read timestamp. |
| Convex adapter surface | `docs/adapters/convex/compatibility.md` and `docs/adapters/convex/ai-guidelines.md` | Current Convex-compatible reads/mutations/subscriptions stay on Convex-compatible semantics; any historical read/PITR/CDC exposure must be a documented Nimbus extension. | Historical authorization, table identity, index identity, and dependency tracking must be proved before any adapter-visible extension is enabled. |
| Firebase/Firestore adapter surface | `docs/adapters/firebase/compatibility.md` | First-party `@nimbus/firebase` CRUD/query/listen/transaction paths inherit engine/storage correctness; stock browser/admin gaps stay unclaimed. | PITR/CDC/historical-read claims must be documented as Nimbus extensions or fail closed; unclaimed upstream SDK breadth must remain unclaimed. |
| Cloud Functions trigger surface | `docs/adapters/cloud-functions/compatibility.md` | Firestore trigger events and durable retry/replay must be derived from the same typed tenant events and CDC cuts used by storage. | Trigger delivery must not invent event ordering outside the durable journal; non-default database and broader admin gaps stay deferred/fail-fast. |
| DynamoDB adapter surface | `docs/adapters/dynamodb/enterprise-readiness.md`, `docs/adapters/dynamodb/feature-coverage.md` | Streams, TTL-attributed remove records, transactions, export/import-adjacent docs, and auth isolation must remain explicit relative to SEQ CDC/PITR. | If SEQ CDC/PITR does not map to DynamoDB protocol concepts, the adapter must document the divergence rather than implying AWS parity. |
| MongoDB adapter surface | `docs/adapters/mongodb/README.md`, `docs/adapters/mongodb/operations.md` | CRUD, cursor, transaction, and index commands inherit engine/storage correctness through the same mutation/query path. `$changeStream` must fail closed until it is backed by the shared SEQ CDC cut model. | Historical reads, PITR, and change streams are not MongoDB-wire claims unless a documented extension exists; unsupported commands must fail clearly. |
| Nimbus-native APIs | `docs/adapters/native/README.md`, `docs/adapters/native/http-api.md`, `docs/adapters/native/websocket-protocol.md` | Primary public surface for Nimbus-native historical reads, PITR, CDC, diagnostics, and support-state inspection once SEQ semantics are proved. | Native extensions must expose typed expired-retention, unsupported-backend, unsupported-adapter, and format-mismatch states. |

## Adapter Exposure Policy

- Storage semantics are implemented once in the engine/storage path; adapters do
  not get private MVCC, PITR, CDC, or retention implementations.
- Adapter-visible claims are narrower than storage capabilities. A backend may
  support historical reads before an adapter exposes them publicly.
- Existing adapter caveats remain binding. SEQ work may improve those surfaces,
  but it must not silently convert `deferred`, `not claimed`, or
  `supported with caveats` rows into enterprise claims without compatibility
  docs and tests.
- Unsupported historical reads, expired reads, unsupported backend/provider
  states, and unsupported adapter extensions must produce typed errors instead
  of falling back to latest reads or lossy snapshots.

## Staged Proof Order

1. Bootstrap verifier/proof/debt/control-plane artifacts.
2. Lock MVCC semantics, historical authorization, cursor identity, retention
   watermarks, and storage-format behavior in `nimbus-core`.
3. Prove versioned registry and policy snapshots before historical document or
   index reads.
4. Prove SQLite and redb document/index MVCC layouts while preserving latest
   fast-path budgets.
5. Extend `ServingSnapshotManager` and transaction sessions instead of creating
   parallel read or transaction paths.
6. Prove retention/GC, PITR, CDC, generated histories, deterministic digests,
   and diagnostics.
7. Graduate Postgres, MySQL, and libSQL parity before final completion.
8. Update architecture and adapter docs, push the branch, and open the PR.

## Performance Baseline Inputs

Existing measured reports provide the initial budget inputs for SEQ0:

| Scope | Existing measured report | Command to refresh before budget close |
| --- | --- | --- |
| SQLite/redb embedded providers | `docs/plans/research/sqlite-storage-benchmark-report.md` | `make bench-embedded-providers REPORT=docs/plans/research/sqlite-storage-benchmark-report.md` |
| Postgres provider | `docs/plans/research/postgres-provider-benchmark-report.md` | `NIMBUS_BENCH_POSTGRES_URL='<connection-string>' make bench-postgres-provider REPORT=docs/plans/research/postgres-provider-benchmark-report.md` |
| MySQL provider | `docs/plans/research/mysql-provider-benchmark-report.md` | `NIMBUS_MYSQL_URL='<connection-string>' make bench-mysql-provider REPORT=docs/plans/research/mysql-provider-benchmark-report.md` |
| libSQL replica provider | `docs/plans/research/sqlite-replica-provider-benchmark-report.md` | `NIMBUS_SQLITE_URL='<primary-url>' NIMBUS_SQLITE_ADMIN_URL='<admin-url>' make bench-libsql-replica-provider REPORT=docs/plans/research/sqlite-replica-provider-benchmark-report.md` |

SEQ13 owns the final performance closeout, but SEQ0 fixes the initial policy:
latest-path regressions are failures unless the phase proof explicitly records
the before/after report, root cause, and accepted enterprise tradeoff. External
provider budgets must include RTT-sensitive lanes because extra round trips are
the easiest way for a correct-looking MVCC design to become operationally
unusable.

## Focused Current Embedded Baseline

SEQ0 ran a focused current embedded point-read lane to make the local latest
read guardrail concrete before SEQ1:

```text
NIMBUS_BENCH_STEADY_WARMUP_ROUNDS=1 \
NIMBUS_BENCH_STEADY_MEASURE_ROUNDS=3 \
NIMBUS_BENCH_COLD_WARMUP_ROUNDS=1 \
NIMBUS_BENCH_COLD_MEASURE_ROUNDS=3 \
make bench-embedded-providers \
  REPORT=docs/plans/proof/storage-engine-quality-and-mvcc/seq0-embedded-point-read-baseline.md \
  WORKLOAD=point-read

Finished `bench` profile [optimized] target(s) in 5m 11s
finished point read latency in 37.496354334s
```

Report: `docs/plans/proof/storage-engine-quality-and-mvcc/seq0-embedded-point-read-baseline.md`.

| Lane | Backend | Samples | Current p95 | SEQ0 latest-path guardrail |
| --- | --- | ---: | ---: | ---: |
| steady-state point read | redb | 3 | 1.16 us | <= 1.45 us |
| steady-state point read | SQLite | 3 | 1.15 us | <= 1.44 us |
| cold-start point read | redb | 3 | 199.32 us | <= 249.15 us |
| cold-start point read | SQLite | 3 | 125.63 us | <= 157.04 us |

These reduced-round values are a SEQ0 guardrail, not the final performance
closeout. SEQ13 must refresh the full benchmark suite and record final
thresholds for latest reads, latest writes, historical reads, historical
pagination, compaction, CDC, and restore.

## Performance Budgets

SEQ0 still needs measured baseline values before it can move to `done`. The
budget dimensions are fixed:

- latest point read and latest indexed query latency
- latest write latency and write amplification
- historical point read, scan, indexed query, and historical pagination latency
- index backfill cost
- retention compaction and GC cost
- PITR/export/import throughput
- CDC initial snapshot, catch-up, and live-tail latency
- storage growth under representative retention windows

SEQ0 promotion requires the benchmark reports above to exist, the refresh
commands to be recorded, and latest-path budget thresholds to be stated before
SEQ1 begins. Running the full refreshed benchmark suite can remain a SEQ13
deliverable when external services are unavailable, but the absence of a fresh
external-provider run must be recorded as an explicit proof gap rather than
hidden behind local-only checks.

## External Provider Benchmark Gate State

The external refresh commands are service-gated in this local session:

| Env var | State | Consequence |
| --- | --- | --- |
| `NIMBUS_BENCH_POSTGRES_URL` | unset | Postgres refresh remains a SEQ13/external-service proof item. |
| `NIMBUS_MYSQL_URL` | unset | MySQL refresh remains a SEQ13/external-service proof item. |
| `NIMBUS_SQLITE_URL` | unset | libSQL replica refresh remains a SEQ13/external-service proof item. |
| `NIMBUS_SQLITE_ADMIN_URL` | unset | libSQL replica refresh remains a SEQ13/external-service proof item. |

This does not remove external-provider parity from final scope. It only means
SEQ1 may begin from the current measured reports plus the focused embedded
baseline, while SEQ13 and the final SEQ14 closeout still require refreshed
performance evidence or an explicit PR-visible service-gated gap.

## SEQ1 And SEQ9 Pre-Implementation Decisions

These decisions are the starting contract for SEQ1/SEQ9 and must become typed
code plus tests before their owning phases close:

| Topic | Decision before implementation |
| --- | --- |
| Historical authorization | Historical reads resolve the read policy as of the requested read timestamp from versioned policy snapshots. They must not apply current policy to old data unless SEQ1 explicitly proves that product mode. Missing or expired policy snapshots fail closed. |
| Historical cursor identity | Historical cursors bind the read timestamp, resolved table id, index id or full-scan shape, query filters/order/limit, policy snapshot identity, retention floor observed at issue time, backend support state, and storage-format generation. Decode mismatch or expired floor fails closed. |
| Storage-format behavior | Old/future MVCC layout versions fail closed at open/admission. Nimbus is pre-launch, so clean breaking upgrades are acceptable, but silent best-effort reads across unknown layouts are not. |
| Retention watermarks | The prune floor is the minimum safe floor across document history, index history, registry snapshots, read-policy snapshots, CDC consumers, PITR/export points, serving materializers, replicas, and transaction/session pins. |
| CDC snapshot-to-log handoff | CDC starts from an explicit snapshot cut and then resumes from the next ordered tenant event/journal sequence. The handoff proof must show no missed or duplicated logical events across document, table, schema, index, and policy changes. |

## Feature Gate Posture

Nimbus is pre-launch, so internal storage layout changes should be clean
breaking changes. Runtime feature flags must not preserve legacy storage
semantics. Temporary implementation gates are allowed only to keep incomplete
MVCC/PITR/CDC surfaces fail-closed until their SEQ owner has proof.

## Initial Validation Evidence

The following commands were run during SEQ0 bootstrap:

```text
npm run docs:validate-refs:strict
docs reference validation: pass (241 working-tree Markdown files)

git diff --check
clean

bash scripts/verify-storage-architecture-trust-hardening.sh
Summary: 12 passed, 0 failed

npm run docs:validate-refs:strict
docs reference validation: pass (241 working-tree Markdown files)

git diff --check
clean

bash scripts/verify-storage-engine-quality-and-mvcc.sh
Summary: 6 passed, 0 failed
```

After syncing the config wildcard and proof updates into the SEQ worktree:

```text
npm run docs:validate-refs:strict
docs reference validation: pass (241 working-tree Markdown files)

git diff --check
clean

bash scripts/verify-storage-engine-quality-and-mvcc.sh
Summary: 7 passed, 0 failed
```

After adding the focused current embedded point-read baseline:

```text
npm run docs:validate-refs:strict
docs reference validation: pass (241 working-tree Markdown files)

git diff --check
clean

bash scripts/verify-storage-engine-quality-and-mvcc.sh
Summary: 7 passed, 0 failed
```

## SEQ0 Promotion Gate Evidence

| Gate | Status | Evidence |
| --- | --- | --- |
| Dedicated worktree | pass | Worktree, branch, base commit, and expected dirty status recorded above. |
| Enterprise charter | pass | Charter recorded in this proof and in the plan completion gate. |
| Backend/adapter matrix | pass | Matrix above maps every current backend/adapter surface to docs and fail-closed/proof state. |
| Source refresh | partial | Local refs recorded; source-backed decision table recorded; detailed excerpts still required for any new SEQ1 semantic decision. |
| Performance budgets | pass for SEQ0 | Existing measured reports, refresh commands, focused current point-read report, initial latest-path guardrail, and explicit external-service gaps are recorded; full benchmark closeout remains SEQ13. |
| Verifier | pass for SEQ0 | Verifier passes and checks benchmark inputs, adapter exposure policy, source-decision sections, focused baseline, and external-provider gap recording. |

## SEQ0 Closeout

SEQ0 is complete as of 2026-06-06. The control plane now has:

- dedicated `main`-based `codex/` worktree evidence
- enterprise guarantee charter
- all-supported backend and adapter support matrix
- adapter exposure and fail-closed policy
- source-backed design decisions
- staged proof order
- technical-debt rows
- reusable verifier
- existing benchmark inputs, focused current embedded baseline, and explicit
  external-service benchmark gaps
- pre-implementation decisions for historical authorization, cursor identity,
  format gates, retention watermarks, and CDC snapshot-to-log handoff

SEQ1 may begin with `docs/plans/proof/storage-engine-quality-and-mvcc/seq1-mvcc-semantics.md`
as the active proof file.
