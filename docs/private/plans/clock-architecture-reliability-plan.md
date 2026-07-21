# Clock Architecture, Reliability, and Testability Plan

Status: `proposed; reviewed against current main; implementation blocked on
explicit PPSC ownership handoff and promotion`

Owner: this plan after promotion. Until then,
`parallel-prepare-serial-commit-plan.md` owns its remaining committer and
provider-publisher work, `horizontal-scaling-plan.md` owns distributed Durable
Object placement/leasing, and `architecture-review-2026-07-plan.md` remains the
active owner for unrelated workspace architecture cleanup.

Created: 2026-07-20. Revalidated: 2026-07-21.

Verified baseline:

- original architecture audit target:
  `9a40b60a429a1d77bea729f45ace6fb97f11293d` (`main`, 2026-07-20);
- current review target:
  `93124d87e22d6b128964ba4cbd4b9e84da5a1910` (`origin/main`, 2026-07-21);
- reconciled dependency: PR
  [#222](https://github.com/nimbus/nimbus/pull/222) squash-merged as
  `1ed97076f770b37450521276c9333e2e10976c87` with 48 successful and 3
  intentionally skipped checks; the PostgreSQL, MySQL, and libSQL provider
  fixture jobs all executed successfully;
- PPSC remains active after PR #224. No clock implementation begins until its
  owner confirms that the first promoted CLK row will not collide with the
  remaining committer/provider-publisher work.

## Objective

Make every Nimbus time decision use the clock domain that matches its
invariant:

1. **wall time** for externally meaningful timestamps and absolute schedules;
2. **monotonic elapsed time** for process-local deadlines, TTLs, rate windows,
   retry cadence, and latency;
3. **shared lease authority plus an epoch fence** for distributed authority:
   provider transaction time for current SQL leases, or an explicit
   consensus/skew contract for future cluster leases;
4. **logical sequence numbers** for durable ordering and publication.

Completion means:

- wall-clock movement cannot freeze or prematurely reset process-local
  duration policies;
- an absolute scheduled target is accepted and persisted by the Engine, not
  reconstructed independently by adapters;
- scheduler waiting remains responsive to forward and backward wall-clock
  movement with a documented maximum resampling delay;
- distributed leases never use a process-local monotonic instant as shared
  authority; every multi-process lease has an explicit shared-time model and
  atomic fencing, or the product is structurally prevented from running in
  that deployment shape;
- clock Interfaces are named by semantics, have real system/manual Adapters,
  and are tested through the owning Module's production Interface;
- ambient clock reads are explicit, classified, and guarded in
  correctness-sensitive code;
- provider-specific time mechanics remain inside provider Adapters while one
  conformance contract verifies the shared semantics;
- no new generic time crate or universal time abstraction is introduced
  without evidence that the standard library and existing `time`/Tokio
  dependencies cannot meet a measured need.

## Scope and ownership

This plan owns:

- `nimbus-core` wall and monotonic clock vocabulary;
- Engine absolute scheduling and scheduler wait behavior;
- Engine process-local transaction-session and write-rate duration policies;
- Firebase process-local write/listen retention;
- temporal validation test seams for JWT and SigV4;
- committer-renewal policy deepening after PPSC lands its monotonic fix;
- ambient-time architecture guards and the exception ledger;
- clock-related diagnostics, documentation, and cleanup;
- proof gates and owner handoffs for other persisted leases whose authority is
  currently local or underspecified.

This plan does not own:

- PPSC durable-outcome classification, publication, or provider-publisher
  enablement;
- a new distributed coordinator;
- horizontal-scaling implementation before that plan is promoted;
- civil calendar or timezone product work;
- guest-language compatibility semantics except where an existing host clock
  is being wired consistently;
- replacing logical `SequenceNumber` ordering with a Hybrid Logical Clock;
- broad removal of every `Instant::now()` used only for latency metrics or
  bounded test timeouts.

## Architecture vocabulary

The repository has no `CONTEXT.md` or `docs/adr/` tree. This plan therefore
records the clock-domain vocabulary that implementations and later architecture
documents must use.

### Wall timestamp

A Unix-epoch fact that can be serialized, displayed, compared with protocol
fields, or persisted. It may move forward or backward. Nimbus currently uses
`Timestamp`, `SystemTime`, seconds-since-epoch fields, and database timestamp
columns for this role.

### Local instant

An opaque, non-serializable point used only to measure elapsed duration inside
one process. It has no cross-process or civil-time meaning. Rust's
`std::time::Instant` or Tokio's runtime-local `Instant` are the concrete
representations.

### Provider time

The database Adapter's evaluation of current time inside an atomic lease state
transition. It is authoritative for provider lease expiry even when it diverges
from Nimbus's local wall clock. PostgreSQL, MySQL, and libSQL have different
evaluation rules; conformance is semantic, not statement-text parity.

### Shared lease authority

The authority that all participants use to decide a distributed lease. Current
SQL committer leases use provider time evaluated inside the fencing
transaction. Future consensus-issued leases may instead use a committed wall
deadline plus a proved clock-skew/drift and reassignment-grace contract. A local
monotonic instant may schedule work but is never itself shared authority.

### Logical order

An Engine or storage ordering fact such as `SequenceNumber`, assigned head,
durable head, applied head, published head, or lease epoch. It is not wall or
elapsed time and must not be derived from either.

### Durable-deadline waiter

A scheduler-owned Module that translates a persisted wall target into bounded
local waits while resampling wall time and listening for earlier work or
shutdown. The wall target remains authoritative; the local timer is only an
efficient wait mechanism.

### Temporal policy

A pure or in-process Module that decides behavior at an explicitly supplied
observation time. JWT validity, SigV4 skew, rate-window admission, and
process-local expiration are temporal policies. Sampling ambient time is done
by a thin caller, not hidden inside the policy.

## Required invariants

| ID | Invariant |
| --- | --- |
| TIME-I1 | A wall timestamp may be persisted; a local instant must never be serialized, logged as an epoch time, compared across processes, or reconstructed after restart. |
| TIME-I2 | Duration-based process-local behavior is insensitive to local wall-clock steps. |
| TIME-I3 | An absolute schedule retains its requested wall target exactly; adapters do not convert it to a relative delay using ambient time. |
| TIME-I4 | Scheduler wake latency after a wall-clock correction is bounded and documented. |
| TIME-I5 | Provider lease validity is decided inside the provider transaction using provider time, owner identity, epoch, and expected durable head. |
| TIME-I6 | Local monotonic time may schedule a renewal attempt but can never prove that a distributed lease remains valid. |
| TIME-I7 | Identical time policy is sampled once per operation and passed through; a single operation cannot observe several accidental versions of "now." |
| TIME-I8 | Backward movement fails conservative where durable retention or security depends on wall time; it must not cause unsafe reclamation or acceptance. |
| TIME-I9 | Every injected clock seam has at least a system Adapter and a deterministic manual Adapter, and the owning Module's Interface—not the Adapter implementation—is the primary test surface. |
| TIME-I10 | Provider Adapter differences are verified through one conformance contract; SQL text is not artificially unified across dialects. |

## Architecture decisions

### AD1 — Keep four clock domains separate

Do not create a `TimeProvider` that combines wall time, monotonic time, sleeps,
provider time, and sequence ordering. Such a Module would have a large,
semantically contradictory Interface and would let callers choose the wrong
authority.

### AD2 — Rename the canonical wall seam

`nimbus_core::Clock` becomes `WallClock`; its production and test Adapters
become `SystemWallClock` and `ManualWallClock`. The Interface continues to
provide `Timestamp`, epoch milliseconds/seconds, and `SystemTime` conversion.
The rename is deliberately breaking because Nimbus is pre-launch.

The shallow `nimbus-storage::simulation::clocks` re-export is removed. Engine,
server, storage, and tests import the canonical owner directly from
`nimbus-core`. Deleting the re-export makes complexity disappear rather than
redistributing it, so it fails the deletion test and does not earn continued
existence.

### AD3 — Add one observation-only monotonic seam when the second consumer lands

The shared Interface is intentionally small: observe a local `Instant`.
Production and manual Adapters provide the real seam. It does **not** own
`sleep`, `sleep_until`, Condvars, Tokio runtime handles, provider time, or
wall-time conversion.

The first production users are transaction-session expiration and tenant
write-rate windows. The PPSC lease-specific Adapter may migrate to the shared
seam only after PPSC's ownership handoff and only if doing so removes duplicated
clock Adapters without leaking lease wake/shutdown behavior into `nimbus-core`.

Async timer behavior remains internal to the scheduler Module and uses
Tokio. Blocking wait behavior remains internal to the committer-lease Module
and uses its existing worker/Condvar ownership.

### AD4 — Prefer explicit observation parameters inside temporal policies

Once a caller samples wall or monotonic time, pure/in-process policy code takes
that observation explicitly. Do not thread `Arc<dyn WallClock>` into every
validator. This is the functional-core/imperative-shell pattern:

```text
system/manual Adapter samples once
              -> temporal policy evaluates explicitly at that observation
              -> caller maps the result to its protocol/error vocabulary
```

### AD5 — The Engine owns absolute scheduling

Adapters may request `run_after` or `run_at`, but they never calculate one from
the other. The Engine scheduler Module owns the wall observation, target
validation, persistence, notification, and result. This removes three current
ambient `Timestamp::now()` conversions from Convex adapter paths.

### AD6 — Persisted leases require shared authority and atomic fencing

A local monotonic clock is appropriate for when to attempt renewal. It is never
appropriate for a persisted expiry that another process must interpret.
Current SQL committer leases use provider transaction time as their shared
authority. A future consensus-issued lease may instead use a committed deadline
only when its owner proves the permitted skew/drift, observation-delay, and
reassignment-grace contract. Persisted activation/network leases must either:

- use a shared authority and atomically validate expiry plus epoch with the
  protected write; or
- be structurally limited to one process until the owning distributed plan
  supplies that implementation.

This plan owns the production-construction guard that keeps the current Durable
Object substrate unserved. `horizontal-scaling-plan.md` HS5 owns the eventual
distributed placement and lease implementation; the clock plan does not
duplicate it.

### AD7 — Keep the current Rust dependency posture

- `std::time::Instant` remains the default elapsed-time primitive.
- Tokio time remains the async timer and virtual-time test Adapter.
- the existing `time` crate remains for civil parsing/formatting.
- do not add `quanta`, `coarsetime`, `mock_instant`, `web-time`, `chrono`, or a
  custom clock crate without benchmark, platform, or target evidence.

Rust documents `Instant` as monotonic but not steady and leaves suspend
accounting platform-dependent. Provider fencing and generous renewal margins
must therefore remain the safety mechanism:
<https://doc.rust-lang.org/std/time/struct.Instant.html>.

Tokio paused time affects Tokio's `Instant`, not `std::time::Instant`, which is
why it belongs to async scheduler tests rather than the blocking renewal
worker: <https://docs.rs/tokio/latest/tokio/time/fn.pause.html>.

## Verified findings ledger

All classifications below were rechecked against current review commit
`93124d87e22d6b128964ba4cbd4b9e84da5a1910`; prose claims are not counted as
proof.

| ID | Classification | Evidence and consequence | Owner item |
| --- | --- | --- | --- |
| CLK-F1 | `CONFIRMED P1 liveness defect` | `scheduler.rs:25-51` computes a wall delta once and sleeps for that local duration. `_interval` is unused. A forward wall jump can leave due work asleep until the stale delay expires. | CLK2 |
| CLK-F2 | `CONFIRMED P1 ownership/correctness defect` | Convex HTTP, sync host, and async host `runAt` fallbacks independently subtract ambient `Timestamp::now()` before calling an Engine interface that only accepts a relative delay. The fourth path, `MutationExecutionUnit::schedule_mutation_at`, samples the Engine clock but clamps a requested past target to that observation before persistence. All four violate exact absolute-target ownership. | CLK2 |
| CLK-F3 | `CONFIRMED P1 tenant-local liveness defect` | `tenant/write_rate.rs:167-170` clamps a backward wall observation to the last event. The sliding window can then remain full until wall time catches up; a forward step can clear it prematurely. Existing tests cover only forward manual time. | CLK3A |
| CLK-F4 | `CONFIRMED P2 reliability defect` | `engine/transactions.rs:81,106,128-147` enforces a process-local 60-second lifetime with wall timestamps. Backward movement extends resource/auth lifetime; forward movement expires it early. The existing expiry test encodes wall advancement. | CLK3B |
| CLK-F5 | `CONFIRMED P2 reliability/test gap` | Firebase write-stream and retained-listen registries use ambient wall timestamps for process-local retention and have no deterministic clock-movement tests. | CLK3C |
| CLK-F6 | `FIXED on reviewed main` | PR #222 moved committer-renewal cadence to `Instant`, kept provider time authoritative, and added system/manual Adapters, bounded wake/shutdown behavior, and local/provider divergence tests. The merge and hosted evidence are recorded in CLK0. | CLK0, CLK5 |
| CLK-F7 | `STRONG POST-PPSC CLOSEOUT REFACTOR` | The merged manual renewal clock controls observation while tests separately wake and observe worker decisions. The seam is useful but time advancement and waiting are only partially encapsulated. | CLK5 |
| CLK-F8 | `CONFIRMED shallow Module/DRY cleanup` | `nimbus-storage/src/simulation/clocks.rs` is a seven-line compatibility re-export of `nimbus-core`; engine production and tests import time through storage. | CLK1A |
| CLK-F9 | `CONFIRMED naming/interface ambiguity` | `Clock` returns Unix epoch values and `SystemTime` but is named generically. The source comment claims it is the single source of now while `Timestamp::now()` and other ambient paths remain public. | CLK1A, CLK4A, CLK4B |
| CLK-F10 | `CONFIRMED deterministic-test gap` | JWT and SigV4 validators sample ambient epoch seconds inside policy functions instead of validating at an explicit observation. Exact skew/expiry edges require ambient time. | CLK4A |
| CLK-F11 | `CONFIRMED guard coverage gap` | `mutation_committer_source_tree_has_no_ambient_time_or_id_mints` covers only mutation/execution-unit directories. The current census finds 38 `Timestamp::now`, 28 `SystemTime::now`, 17 `system_now_millis`, and 12 `system_now_secs` lexical occurrences across Rust source, mixing policy, adapters, tests, and uniqueness-only uses. | CLK4B |
| CLK-F12 | `CONFIRMED representation leak/DRY opportunity` | `Timestamp(pub u64)` leaves millisecond conversion, saturating addition/subtraction, and duration calculation distributed across callers. This is not itself a correctness defect while units remain correct. | CLK7A |
| CLK-F13 | `EXISTING STRONG SEAM; targeted proof gap` | `CommitterLeaseStore` has PostgreSQL, MySQL, and libSQL Adapters plus a shared transition/concurrency contract. Missing coverage is delayed transaction/lock-wait behavior across nominal expiry and provider-clock discontinuity diagnostics. | CLK5 |
| CLK-F14 | `BLOCKER before served Durable Objects` | Durable Object activation computes and checks a persisted lease with local wall time; claim and protected operations do not use the provider committer-lease CAS as their authority. Current scope is a checked no-production-construction gate while the substrate remains unserved; `horizontal-scaling-plan.md` HS5 owns distributed placement/leasing. | CLK6A |
| CLK-F15 | `DEFERRED PROOF GAP in future-only seam` | The dead-code-gated `ClusterLeaseProvider` combines the committed super-net lease with an injected local wall clock. The future leader/node clock-skew assumption and atomic fencing relationship are not yet specified, so cluster enablement remains blocked rather than current production behavior being changed here. | CLK6B |
| CLK-F16 | `NICE TO HAVE` | JS control-plane `wait()` measures elapsed timeout with `Date.now()`, so a wall step affects client wait duration. | CLK7C |
| CLK-F17 | `CONFIRMED dead cleanup candidate` | `nimbus-sandbox::backends::conmon::lifecycle::now_millis()` has no reachable caller in the source census. | CLK7B |
| CLK-F18 | `DELIBERATE DIRECT SYSTEM TIME` | TLS leaf-cache freshness, deploy nonce entropy, temp-file uniqueness, guest-host time, filesystem mtime fallback, and test fixture timestamps do not all benefit from Engine clock injection. They require explicit classification, not indiscriminate replacement. | CLK4B |

## Existing patterns to preserve

- PPSC's provider lease uses provider time, owner, monotonic epoch, and durable
  sequence inside the provider transaction. Local time schedules attempts only.
- `CommitterLeaseStore` is a real seam with three production Adapters and one
  shared conformance contract.
- trigger execution bounds wall-derived waits to a short polling interval and
  resamples; this is a useful blocking-worker reference for clock correction.
- blob GC treats clock regression conservatively and uses a monotonic storage
  position to cover concurrent puts; do not replace it with an `Instant` that
  cannot survive restart.
- mutation admission, latency, queueing, cancellation, and most timeout paths
  already use `Instant` or Tokio timers appropriately.
- `nimbus-runtime` retains local clock Interfaces where its zero-workspace-
  dependency invariant prevents reuse of `nimbus-core`.

## Execution map

Each implementation item is one PR unless its row explicitly permits a
mechanical split. No later item may silently absorb an earlier item's behavior
change.

### CLK0 — Reconcile and consume PPSC monotonic renewal

Status: `complete on 2026-07-21`

Problem/invariant:

- The original audit found wall-time renewal cadence and was blocked on the
  PPSC-owned repair.
- Current main contains that repair; this row records the dependency evidence
  so later clock work does not reimplement or conflict with it.

Owning Modules and paths:

- `crates/nimbus-engine/src/tenant/committer_lease.rs`
- Engine bootstrap/runtime construction paths touched by PR #222
- `docs/concepts/architecture/engine-mutation-path.md`
- `docs/private/plans/parallel-prepare-serial-commit-plan.md` remains the status
  owner until the PR merges

Acceptance:

- [x] PR #222 is merged to `main` with confirmed-green required CI.
- [x] Main contains system/manual monotonic renewal Adapters and the five named
  behavior cases from the PPSC plan.
- [x] Provider validity remains provider-clock-owned.
- [x] This plan records the merge commit before CLK1A begins.

Evidence:

- PR [#222](https://github.com/nimbus/nimbus/pull/222), squash commit
  `1ed97076f770b37450521276c9333e2e10976c87`.
- Hosted checks: 48 successful and 3 intentionally skipped (51 total).
- Named renewal cases present on main:
  `lease_renewal_ignores_backward_wall_clock_step`,
  `lease_renewal_ignores_forward_wall_clock_step`,
  `lease_renewal_shutdown_interrupts_monotonic_wait`,
  `provider_expiry_remains_authoritative_after_local_clock_divergence`, and
  `postgres_lease_renewal_survives_local_clock_divergence`.
- PPSC ledger live-provider counts: libSQL 37/37, PostgreSQL 51/51, and MySQL
  33/33. The corresponding hosted provider jobs all executed successfully.

### CLK1A — Canonical wall-clock naming and shallow-shim removal

Status: `planned`

Problem/invariant:

- A generic `Clock` name hides that its values are wall/epoch time.
- The storage re-export obscures ownership and creates stale import paths.
- Ambient time remains easy to mint through `Timestamp::now()`.

Owning Module and production paths:

- `nimbus-core/src/clock.rs` owns the wall-clock Interface and system/manual
  Adapters.
- `nimbus-core/src/types.rs` owns `Timestamp` construction.
- `nimbus-storage/src/simulation/{mod.rs,clocks.rs}` loses the pass-through.
- all callers import from `nimbus-core` directly.

Interface contract:

- wall observations remain Unix milliseconds represented by `Timestamp`;
- seconds conversion floors;
- `SystemTime` conversion remains millisecond-precision and documented;
- observation is cheap, side-effect free, and may move backward;
- no waiting or monotonic guarantees are added.

Seam/Adapters:

- `SystemWallClock` and `ManualWallClock` are the two Adapters.
- the manual Adapter supports explicit set/advance because wall movement,
  including backward movement, is behavior under test.

Dependencies/order:

- after plan promotion; CLK0 is satisfied, but the PPSC owner must hand off the
  remaining overlapping committer/provider-publisher surface first;
- before CLK2 through CLK4B so new code uses final vocabulary.

Acceptance and named tests:

- `wall_clock_seconds_floor_milliseconds`
- `manual_wall_clock_moves_forward_and_backward`
- `wall_clock_systemtime_conversion_preserves_milliseconds`
- `clock_types_are_imported_from_nimbus_core`
- no production import of `nimbus_storage::{Clock, ManualClock, SystemClock}`
  remains;
- the storage shim is deleted;
- the canonical clock comment describes this as the wall-clock seam without
  claiming that all ambient time has already been centralized;
- `Timestamp::now()` visibility and caller migration remain explicitly owned
  by CLK4B after classification, rather than expanding this rename/shim PR.

Fail-before:

- the import-ownership guard must identify current storage re-export consumers;
- a compile-only rename without deleting the shim does not pass.

Verification:

```bash
cargo fmt --all --check
cargo nextest run -p nimbus-core -p nimbus-storage -p nimbus-engine
cargo check --workspace --all-targets
make clippy
rg -n 'nimbus_storage(?:::simulation)?::.*(Clock|ManualClock|SystemClock)' crates
```

Docs/observability:

- create `docs/private/architecture/time-and-ordering.md` and route it from
  `docs/private/architecture/README.md`;
- update clock terminology in public architecture docs only where users need
  the operational distinction.

### CLK1B — Observation-only monotonic seam and deterministic harness wiring

Status: `planned`

Problem/invariant:

- multiple process-local policies need deterministic elapsed time;
- duplicating manual monotonic clocks per Module weakens leverage, while a
  timer mega-interface would weaken locality.

Owning Module and paths:

- canonical observation seam beside the wall seam in `nimbus-core`;
- Engine bootstrap simulation seams carry the Adapter at construction;
- `nimbus-testing` exposes it through the deterministic harness where an
  Engine-wide test needs coordinated wall and monotonic divergence.

Interface contract:

- returns opaque `std::time::Instant` values;
- observations never move backward;
- no serialization, epoch conversion, cross-process comparison, or sleep;
- manual Adapter advances only forward and reports overflow loudly;
- callers use `checked_*`/`saturating_duration_since` according to their
  documented error policy.

Seam/Adapters:

- system and manual monotonic Adapters;
- Tokio timer and blocking Condvar remain internal Adapters of their owning
  Modules, not methods on this Interface.

Dependencies/order:

- after CLK1A naming;
- lands with the first two non-lease production consumers or immediately
  before CLK3A/CLK3B so the seam is never hypothetical.

Acceptance and named tests:

- `manual_monotonic_clock_advances_without_wall_clock_movement`
- `manual_wall_clock_moves_without_monotonic_clock_movement`
- `engine_simulation_constructs_all_runtimes_with_the_supplied_monotonic_clock`
- compile/architecture guard prevents serialization traits on monotonic clock
  or instant wrappers.

Fail-before:

- a divergence test using the current single `ManualClock` must demonstrate
  that wall and elapsed time cannot be controlled independently.

Verification:

```bash
cargo nextest run -p nimbus-core -p nimbus-testing -p nimbus-engine
cargo check --workspace --all-targets
cargo fmt --all --check
make clippy
```

### CLK2 — Engine-owned absolute scheduling and bounded durable-deadline waits

Status: `planned; highest-priority new behavior after CLK0`

Problem/invariant:

- absolute scheduling is converted to delay by three adapter fallbacks using
  ambient wall time, while the execution-unit path clamps a requested past
  target to its sampled Engine wall time;
- the scheduler converts a wall target to one unbounded local sleep;
- forward wall correction can make due work late.

Owning Modules and paths:

- Engine scheduler interfaces under `engine/scheduler/` own relative/absolute
  target construction and persistence;
- `engine/scheduler/scheduled_jobs.rs` owns direct scheduled-job persistence;
- `engine/execution_units/staging.rs` owns transactional staging of schedules
  alongside the parent mutation;
- `scheduler.rs` owns the global loop and delegates waiting to a concept-owned
  child Module;
- Convex HTTP, sync host, and async host scheduling paths become thin adapters.

Interface, ordering, errors, and performance:

- relative requests sample Engine wall time once and use saturating duration
  arithmetic;
- direct Engine `schedule_mutation_at*` methods persist the requested wall
  timestamp exactly rather than round-tripping through a relative DTO;
- execution-unit `schedule_mutation_at` stages that exact timestamp in the
  parent transaction so parent rollback also removes the schedule;
- a past absolute target is immediately due, not an error;
- notification is armed before inspecting/sleeping so earlier work cannot lose
  a wakeup;
- shutdown always wins promptly;
- wait duration is `min(wall_delta, resample_interval)` and wall time is
  resampled after every wake;
- default maximum clock-correction latency is one second unless benchmarks and
  operator requirements justify a different bound;
- no busy loop at zero/past deadlines.

Seam/Adapters:

- external test surface is the Engine scheduling Interface;
- internal async timer Adapter is Tokio sleep/paused time;
- wall observation uses `WallClock`;
- scheduler Notify and shutdown watch remain production wake Adapters.

Dependencies/order:

- after CLK1A; may run before the shared monotonic seam because Tokio owns its
  own monotonic timer;
- must land before process-local TTL cleanup so scheduler semantics are the
  reference pattern.

Acceptance and named tests:

- `engine_absolute_schedule_persists_requested_past_target`
- `mutation_execution_unit_run_at_preserves_requested_past_target`
- `mutation_execution_unit_run_at_rolls_back_with_parent`
- `convex_http_run_at_uses_engine_wall_clock`
- `convex_sync_run_at_uses_engine_wall_clock`
- `convex_async_run_at_uses_engine_wall_clock`
- `scheduler_forward_wall_jump_executes_within_resample_bound`
- `scheduler_backward_wall_jump_does_not_execute_early`
- `scheduler_earlier_work_interrupts_far_future_wait`
- `scheduler_shutdown_interrupts_far_future_wait`
- `scheduler_clock_resampling_does_not_busy_loop`
- restart test proves the persisted target remains authoritative after process
  loss.

Fail-before:

- run the real scheduler with a far-future target, advance only the injected
  wall clock past the target, and show that current code remains asleep beyond
  the configured resample bound;
- construct an Engine whose manual wall time differs materially from ambient
  system time and show current adapter `runAt` persists the wrong target;
- request a past execution-unit target and show current staging replaces it
  with the sampled Engine time.

Verification:

```bash
cargo nextest run -p nimbus-engine -E 'test(scheduler)'
cargo nextest run -p nimbus-server
cargo fmt --all --check
make clippy
```

Docs/observability:

- document maximum scheduler correction latency;
- add bounded-cardinality counters for timer wake, notifier wake, shutdown
  wake, and immediate-due loops only if existing scheduler diagnostics cannot
  distinguish them without cardinality growth.

### CLK3A — Monotonic tenant write-rate windows

Status: `planned`

Problem/invariant:

- a local wall step can freeze or erase a tenant's sliding window;
- rate limiting is elapsed-duration policy, not a civil-time fact.

Owning Module and path:

- `nimbus-engine/src/tenant/write_rate.rs` owns event age, byte accounting,
  retry-after calculation, diagnostics, and configuration.

Interface/performance:

- observations are local instants;
- retry-after remains a `Duration` and never exposes an instant;
- window operations remain amortized O(events expired) and do not add a clock
  trait call per queued byte fragment beyond the existing per-mutation check;
- tenant-local lock and counters remain tenant-local;
- a single mutation larger than the limit retains the existing stable whole-
  window backoff behavior.

Seam/Adapters:

- system/manual monotonic Adapters through the Engine/TenantRuntime construction
  seam;
- tests exercise the limiter Interface with explicit observations and an
  Engine integration case.

Dependencies/order:

- after CLK1B supplies the monotonic observation seam and CLK2 establishes the
  reference wall-target/monotonic-wait separation.

Acceptance and named tests:

- `write_rate_window_expires_on_monotonic_time_when_wall_moves_backward`
- `write_rate_window_does_not_reset_when_wall_moves_forward`
- `write_rate_retry_after_uses_remaining_monotonic_duration`
- `write_rate_exact_window_edge_releases_bytes`
- `write_rate_concurrent_checks_preserve_limit_and_byte_accounting`
- existing happy/shadow/oversized cases remain behaviorally equivalent.

Fail-before:

- add an event, move wall time backward, advance elapsed time by a full window,
  and show current code continues rejecting;
- move wall time forward without elapsed time and show current code clears the
  window.

Verification:

```bash
cargo nextest run -p nimbus-engine -E 'test(write_rate)'
cargo fmt --all --check
make clippy
```

Observability:

- retain existing shadow/rejection counters;
- do not add timestamps or per-tenant labels beyond existing tenant-local
  diagnostic records.

### CLK3B — Monotonic transaction-session lifetime with wall metadata

Status: `planned`

Problem/invariant:

- process-local sessions should live for the configured elapsed TTL regardless
  of wall movement;
- clients may still need wall `started_at` and `expires_at` metadata.

Owning Module and path:

- `nimbus-engine/src/engine/transactions.rs` owns registry admission, access,
  expiry, pruning, and metadata.

Interface/ordering/errors:

- begin samples wall and monotonic time once each;
- public session metadata remains wall time;
- internal stored session has a non-serializable monotonic deadline;
- access first checks expiration, then tenant, then principal, preserving the
  current invalidation/error posture unless a security review chooses a more
  conservative order;
- restart still loses all sessions by construction;
- pruning remains O(active sessions) under the existing 256-session bound.

Dependencies/order:

- after CLK1B supplies the monotonic observation seam and CLK2 establishes the
  reference wall-target/monotonic-wait separation.

Acceptance and named tests:

- `transaction_session_expires_after_elapsed_ttl_when_wall_moves_backward`
- `transaction_session_survives_forward_wall_step_before_elapsed_ttl`
- `transaction_session_wall_metadata_remains_stable`
- `transaction_session_exact_deadline_is_expired`
- `transaction_session_pruning_respects_tenant_and_principal_invalidation`
- `transaction_session_concurrent_commit_or_expire_has_one_terminal_outcome`
- cancellation/crash are not separate cases because sessions are in-memory and
  disappear on process loss; the restart behavior must be documented.

Fail-before:

- preserve the current wall-only expiry implementation and show the backward
  and forward divergence tests fail for the intended reason.

Verification:

```bash
cargo nextest run -p nimbus-engine -E 'test(transaction_session)'
cargo fmt --all --check
make clippy
```

### CLK3C — Firebase process-local stream/listen retention

Status: `planned`

Problem/invariant:

- write-stream replay state and retained listen targets are process-local
  caches, but wall steps currently extend or truncate their lifetime;
- there is no deterministic time seam for either registry.

Owning Modules and paths:

- `nimbus-firebase/src/grpc/write_stream.rs`
- `nimbus-firebase/src/grpc/listen_stream.rs`

Interface/performance:

- registries store local deadlines and accept explicit observation at touch,
  prune, and resume operations;
- no protocol response exposes a local instant;
- eviction ordering uses deadline then a monotonic insertion sequence (or an
  equivalently stable owned key) so equal-deadline eviction is deterministic;
- locked/in-use write streams retain the current conservative behavior;
- no background polling thread is added.

Dependencies/order:

- after CLK1B supplies the monotonic observation seam and CLK2 establishes the
  reference wall-target/monotonic-wait separation.

Acceptance and named tests:

- `write_stream_retention_uses_elapsed_time_across_wall_steps`
- `write_stream_touch_extends_from_current_monotonic_observation`
- `listen_target_retention_uses_elapsed_time_across_wall_steps`
- `listen_target_eviction_is_deterministic_at_equal_deadlines`
- `busy_write_stream_is_not_pruned`
- replay/resume behavior remains unchanged until elapsed expiry.

Fail-before:

- demonstrate early/late eviction by moving only wall time under current code.

Verification:

```bash
cargo nextest run -p nimbus-firebase
cargo fmt --all --check
make clippy
```

### CLK4A — Pure temporal validators with explicit observation

Status: `planned`

Problem/invariant:

- JWT and SigV4 policy functions hide ambient sampling, preventing exact and
  deterministic edge proofs.

Owning Modules and paths:

- `nimbus-convex/src/auth/jwt/claims.rs`
- `nimbus-dynamodb/src/auth/sigv4/verify.rs`

Interface/errors:

- thin production wrappers sample `SystemWallClock` once;
- pure policy functions validate at explicit epoch seconds;
- existing wire error classes/messages remain stable;
- seconds flooring and configured skew remain explicit;
- no trait object is threaded through parsing and signature verification.

Acceptance and named tests:

- `jwt_not_before_accepts_exact_positive_skew_edge`
- `jwt_expiry_rejects_exact_negative_skew_edge`
- `jwt_temporal_validation_samples_now_once`
- `sigv4_accepts_exact_clock_skew_edge`
- `sigv4_rejects_one_second_past_clock_skew_edge`
- `sigv4_rejects_future_and_past_symmetrically`
- malformed timestamp and server-clock conversion errors preserve their
  existing classifications.

Fail-before:

- tests calling only the current public policy function cannot select an exact
  observation and must fail or rely on a race-prone ambient timestamp.

Verification:

```bash
cargo nextest run -p nimbus-convex
cargo nextest run -p nimbus-dynamodb
cargo fmt --all --check
make clippy
```

### CLK4B — Ambient-time classification and architecture guard

Status: `planned`

Problem/invariant:

- the canonical clock comment overstates current centralization;
- correctness-sensitive and benign ambient reads are indistinguishable in
  review;
- a global blind replacement would add shallow seams to uniqueness/test code.

Owning Modules and paths:

- a checked-in allowlist under `docs/private/architecture/` or `scripts/data/`;
- architecture verification script/test;
- targeted production call sites across adapters, auth, scheduling, storage,
  runtime, and sandbox.

Classification vocabulary:

- `INJECT`: behavior changes under clock movement and deterministic testing is
  valuable;
- `SAMPLE_AT_SHELL`: pure policy should receive an explicit observation;
- `SYSTEM_ADAPTER`: an explicit production Adapter may read ambient time;
- `UNIQUENESS_ONLY`: wall time contributes entropy/temporary naming but not
  temporal correctness;
- `TEST_ONLY`: fixture/benchmark use;
- `EXTERNAL_TYPE`: third-party Interface requires `SystemTime`;
- `REMOVE`: dead or redundant source.

Acceptance:

- every production `Timestamp::now`, `SystemTime::now`, and canonical free
  helper occurrence is classified;
- `Timestamp::now()` is made non-public after its callers are classified;
  temporal callers use `WallClock` or explicit observations, while benign
  uniqueness/external-type/test sources follow their recorded classification;
- correctness-sensitive paths fail the architecture check on a new ambient
  read;
- allowlist entries include owner, rationale, and removal trigger;
- tests and benches are excluded structurally rather than by a growing list of
  individual files;
- direct Engine mutation paths retain their existing stricter guard.

Named tests/checks:

- `clock_source_allowlist_matches_reachable_production_sources`
- `correctness_sensitive_source_tree_rejects_ambient_wall_time`
- `ambient_clock_allowlist_has_no_stale_entries`

Fail-before:

- seed one disallowed `SystemTime::now()` in a guarded fixture and prove the
  checker names file, line, and semantic reason.

Verification:

```bash
bash scripts/verify-repo-architecture-quality.sh
cargo nextest run -p nimbus-engine -E 'test(ambient_sources)'
rg -n 'Timestamp::now\(|SystemTime::now\(|system_now_(millis|secs)\(' crates --glob '*.rs'
git diff --check
```

### CLK5 — Committer-renewal policy depth, diagnostics, and provider conformance

Status: `planned after PPSC FINAL; do not modify PR #222 from this plan`

Problem/invariant:

- PR #222 fixes wall/monotonic separation, but renewal cadence, transient
  failure retry, safety margin, wake behavior, and diagnostics remain one
  lifecycle implementation concern;
- fixed ten-second retry after a transient error does not express the remaining
  local safety budget;
- provider expiry is exposed without an elapsed-since-success diagnostic;
- provider conformance does not cover lock-wait/long-transaction timing.

Owning Modules and paths:

- `nimbus-engine/src/tenant/committer_lease/` after ownership-based extraction
  if the file crosses the repo threshold or the deletion test justifies a
  `renewal.rs` child;
- Engine diagnostics projection;
- `nimbus-storage` shared committer-lease conformance tests;
- provider Adapter SQL remains provider-owned.

Interface, ordering, errors, performance:

- acquisition still occurs at last responsible moment before sequence
  assignment;
- normal renewal cadence and transient retry cadence are distinct policy;
- retry scheduling is bounded by a conservative local elapsed safety budget
  derived from the requested duration, never by subtracting provider expiry
  from local wall time;
- deterministic owner/tenant-keyed jitter may spread load, but maximum jitter
  must preserve the documented safety margin;
- provider `Fenced` is terminal and hands off to the existing eviction path;
- transient storage error is counted and retried without asserting lease
  validity;
- shutdown wakes and joins promptly;
- no per-tenant unbounded task/thread or high-cardinality metric is added.

Seam/Adapters:

- production/manual monotonic observation;
- blocking wait/wake remains internal to the lifecycle Module;
- PostgreSQL, MySQL, and libSQL remain real provider Adapters at
  `CommitterLeaseStore`.

Acceptance and named tests:

- preserve all PR #222 wall-divergence and shutdown tests;
- `lease_transient_failure_retries_before_local_safety_budget`
- `lease_retry_jitter_is_deterministic_and_bounded`
- `lease_renewal_success_resets_failure_streak`
- `lease_stats_report_monotonic_age_since_last_success`
- `lease_stats_never_compare_provider_expiry_to_local_wall_clock`
- `lease_shutdown_during_provider_error_drains_worker`
- `lease_takeover_after_expiry_fences_stale_epoch_under_concurrency`
- provider contract cases for acquire/renew delayed across nominal expiry and
  lock wait, executed on every available live provider lane.

Fail-before:

- inject consecutive transient renewal failures and show current fixed retry
  schedule can consume the intended safety margin;
- provider timing cases must fail against a deliberately weakened validity
  predicate, not by merely changing expected SQL strings.

Backend reporting:

- PostgreSQL, MySQL, and remote libSQL lanes are individually reported;
- local absence or skip is `UNVERIFIED`;
- hosted CI service-container evidence may close MySQL only when the named test
  actually executes.

Verification:

```bash
cargo nextest run -p nimbus-storage -E 'test(committer_lease)'
cargo nextest run -p nimbus-engine -E 'test(lease)'
cargo fmt --all --check
make clippy
```

Docs/observability:

- document normal cadence, failure retry, maximum correction/safety margin,
  provider authority, and platform suspend caveat;
- expose last-success age and failure streak through bounded-cardinality
  diagnostics; absolute provider expiry remains labeled provider time.

### CLK6A — Durable Object production-construction gate and HS5 handoff

Status: `required before any Durable Object data plane is served; distributed implementation deferred to horizontal-scaling HS5`

Problem/invariant:

- a persisted activation lease is minted and checked with local wall time;
- the in-process lane mutex cannot provide cross-process exclusion;
- moving this to local monotonic time would make it less transferable, not
  safe.

Owning paths:

- `nimbus-server/src/adapters/cloudflare/durable_objects/mod.rs`
- architecture-quality verification script/test;
- Cloudflare adapter documentation and capability diagnostics.

Binding decision:

- the existing substrate remains unserved: no production front door may
  construct it before HS5 supplies distributed placement and fencing;
- a checked architecture guard preserves that boundary by rejecting production
  construction/call sites outside the concept-owned substrate Module;
- public compatibility and capability docs continue to report Durable Objects
  as not served, not as a single-node production feature;
- `horizontal-scaling-plan.md` HS5 owns moving activation claim/takeover and
  every protected write behind shared authority with epoch validation atomic
  with the write.

Acceptance:

- `durable_objects_have_no_production_front_door_before_hs5` fails when a
  production path constructs or invokes `DurableObjectSubstrate`;
- `docs/reference/cloudflare/compatibility.md` and the current-capability source
  map continue to report no served Durable Object data plane;
- the current local-wall activation deadline is documented as a test-substrate
  implementation detail, never shared authority or a production exclusivity
  guarantee;
- manually constructing two substrate instances over shared storage remains an
  unsupported shape whose demonstrated exclusivity gap is open until HS5;
- the HS5 acceptance contract retains deterministic stale/expired epoch,
  acknowledgement-loss, retry/replay, cancellation, crash, takeover, and
  two-process fencing tests before distributed enablement.

Fail-before:

- two independent substrate instances over one shared tenant demonstrate the
  current non-atomic claim/write gap;
- seed a production front-door construction call and prove the architecture
  guard rejects it with the HS5 ownership explanation.

Coordination:

- this row supplies the current production-construction guard, diagnostic, and
  explicit handoff; it does not re-own the Cloudflare adapter, change Engine KV
  fencing, or implement HS5.

### CLK6B — Cluster super-net lease clock-skew contract

Status: `deferred future-only proof gate with horizontal scaling`

Problem/invariant:

- the current `cluster.rs` surface is dead-code-gated and is not an enabled
  production lease path;
- the future Raft-committed lease contains wall expiry while a node
  self-fences using local wall time;
- leader/node skew, observation delay, partition behavior, and reassignment
  grace are not specified.

Owning documents/paths:

- `crates/nimbus-sandbox/src/backends/oci/network/cluster.rs`
- `docs/private/architecture/horizontal-scaling.md`
- `docs/private/plans/horizontal-scaling-plan.md`

Acceptance before promotion:

- the leader authority and node observation model are explicit;
- the maximum clock-skew/drift assumption or clock-free alternative is stated;
- self-fencing and reassignment cannot overlap under the permitted model;
- tests cover forward/backward node time, delayed committed lease observation,
  partition, restart, stale epoch, and concurrent reassignment;
- if the invariant cannot be proven with wall expiry, the design changes before
  cluster mode is enabled.

### CLK7A — Deepen `Timestamp` arithmetic

Status: `nice to have after behavioral bands`

Problem/invariant:

- callers repeatedly know that the tuple field is milliseconds and hand-roll
  saturating arithmetic.

Scope:

- give the `Timestamp` Module owned duration arithmetic and explicit epoch
  conversion;
- migrate correctness-sensitive callers first;
- consider making the tuple field private only after a usage census shows the
  migration is bounded and serialization remains stable.

Acceptance:

- exact edge, overflow, pre-epoch, and flooring tests;
- no new panic on untrusted duration input;
- serialized representation remains unchanged;
- avoid type proliferation such as separate wrappers for every protocol field
  unless misuse evidence justifies them.

### CLK7B — Remove dead/redundant clock helpers

Status: `cleanup`

Scope and acceptance:

- delete the unused conmon `now_millis()` helper;
- consolidate one-line wrappers only when they add no domain semantics or
  crate-dependency value;
- retain explicit `SYSTEM_ADAPTER`, `EXTERNAL_TYPE`, and `UNIQUENESS_ONLY`
  helpers from CLK4B;
- deletion must reduce interface surface without moving conversion/error
  complexity into several callers.

Verification:

```bash
cargo check -p nimbus-sandbox --all-targets
cargo nextest run -p nimbus-sandbox
make clippy
```

### CLK7C — Monotonic JavaScript SDK polling timeout

Status: `nice to have`

Problem/invariant:

- `packages/nimbus/src/control-plane/client.ts::wait()` uses `Date.now()` for
  elapsed timeout.

Acceptance:

- elapsed timeout uses `performance.now()` or an injected monotonic observation
  accepted by every supported Node/browser target;
- request/interval behavior and error text remain stable;
- tests inject observation and sleep so no real 30-second wait is required;
- forward/backward wall movement does not affect timeout.

Verification:

```bash
npm run typecheck -w @nimbus/nimbus
npm run test -w @nimbus/nimbus
npm run build -w @nimbus/nimbus
```

### CLK8 — Final documentation, verification, and archival

Status: `planned`

Required closeout:

- repeat the clock-source census and classify all deltas from the baseline;
- verify the architecture decisions with the deletion test and interface test
  surface;
- confirm no generic timer/time-provider Module or unused seam was introduced;
- run every focused lane named above, then the repository gates below;
- record exact test counts and live-provider execution rather than "tests
  pass";
- run the required independent `autoreview` for each code-changing PR without
  nesting reviews;
- update `docs/private/architecture/time-and-ordering.md`, routing docs,
  affected public architecture/concept docs, and operator diagnostics;
- archive this plan only when every required item is complete and deferred
  multi-process gates are routed to an explicit owner plan.

Final commands:

```bash
cargo fmt --all --check
bash scripts/verify-repo-architecture-quality.sh
bash scripts/check-docs.sh
make ci
git diff --check
```

Unavailable live-provider lanes are named `UNVERIFIED`; they cannot be counted
as green. Hosted CI is the merge source of truth for MySQL, Windows, and other
lanes unavailable locally.

## Status ledger

| Item | Status | Dependency | Completion evidence |
| --- | --- | --- | --- |
| CLK0 PPSC monotonic renewal | `complete` | PR #222 merged | `1ed97076f`; 48 success/3 skipped; named tests and live lanes recorded |
| CLK1A wall naming/shim removal | `planned` | plan promotion + PPSC handoff | import guard, core/storage/engine tests |
| CLK1B monotonic observation seam | `planned` | CLK1A | divergence tests, construction proof |
| CLK2 scheduler ownership/waiting | `planned` | CLK1A | fail-before clock jump + four-path parity |
| CLK3A write-rate elapsed window | `planned` | CLK1B + CLK2 | backward/forward wall divergence tests |
| CLK3B transaction elapsed TTL | `planned` | CLK1B + CLK2 | metadata + elapsed expiry tests |
| CLK3C Firebase local retention | `planned` | CLK1B + CLK2 | stream/listen divergence tests |
| CLK4A pure temporal validators | `planned` | CLK1A | exact JWT/SigV4 edge tests |
| CLK4B ambient source guard | `planned` | CLK1A | complete classified census |
| CLK5 lease policy/diagnostics | `planned` | PPSC FINAL | retry budget, stats, provider conformance |
| CLK6A Durable Object construction gate | `required before serving` | HS5 owns distributed lease | no-production-construction guard now; HS5 two-process fence later |
| CLK6B cluster skew contract | `deferred` | horizontal scaling promotion | model + deterministic partition/skew tests |
| CLK7A Timestamp arithmetic | `nice to have` | CLK2-CLK4 | edge/serialization tests |
| CLK7B dead helper cleanup | `cleanup` | CLK4B | deletion census + affected tests |
| CLK7C SDK timeout | `nice to have` | none | wall-divergence JS tests |
| CLK8 closeout | `planned` | all required rows | final commands, review, archive |

## Promotion gate

Promote this plan from `proposed` to `active` only when:

1. [x] PR #222 is merged and its dependency evidence is recorded in CLK0.
2. [ ] The PPSC owner confirms that CLK1/CLK2 will not collide with remaining
   PPSC mutation-path files.
3. [x] `architecture-review-2026-07-plan.md` records that CO7 follow-on clock
   work routes here rather than creating a second cleanup ledger.
4. [x] The baseline commit and clock-source census are refreshed from current
   `main`.
5. [ ] The promoting change selects one PR-sized first implementation target,
   normally CLK1A; CLK2 remains next in order because it depends on CLK1A's
   final wall-clock vocabulary.

## Rejected designs

- **Universal time provider:** shallow semantic switchboard; rejected.
- **Clock plus sleep trait:** couples async Tokio and blocking worker behavior;
  rejected.
- **Persist local instants:** meaningless across restart/processes; rejected.
- **Use local monotonic time for distributed lease expiry:** unsafe/undefined
  across processes; rejected.
- **Replace sequence numbers with HLCs:** does not solve the identified
  duration/scheduling problems and weakens current ordering clarity; rejected.
- **Unify provider SQL text:** dialect time semantics differ; share tests, not
  queries; rejected.
- **Adopt `quanta` for mockability:** standard `Instant` plus owned manual
  Adapters meet current correctness needs without TSC calibration and suspend
  caveats; rejected absent measurement.
- **Global fake clock:** introduces cross-test state and hides ownership;
  rejected.
- **Blindly inject every direct `SystemTime::now()`:** uniqueness, external
  type, and test-only uses do not all justify a seam; classify first.

## Source references

Repository evidence:

- `crates/nimbus-core/src/clock.rs`
- `crates/nimbus-core/src/types.rs`
- `crates/nimbus-engine/src/scheduler.rs`
- `crates/nimbus-engine/src/engine/scheduler/scheduled_jobs.rs`
- `crates/nimbus-engine/src/engine/execution_units/staging.rs`
- `crates/nimbus-engine/src/engine/transactions.rs`
- `crates/nimbus-engine/src/tenant/write_rate.rs`
- `crates/nimbus-engine/src/tenant/committer_lease.rs`
- `crates/nimbus-storage/src/traits/committer_lease.rs`
- `crates/nimbus-storage/src/tests.rs`
- `crates/nimbus-firebase/src/grpc/{write_stream,listen_stream}.rs`
- `crates/nimbus-convex/src/auth/jwt/claims.rs`
- `crates/nimbus-dynamodb/src/auth/sigv4/verify.rs`
- `crates/nimbus-server/src/adapters/cloudflare/durable_objects/mod.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/cluster.rs`
- `docs/private/plans/parallel-prepare-serial-commit-plan.md`
- `docs/private/plans/horizontal-scaling-plan.md`

Primary external references:

- Rust `Instant`: <https://doc.rust-lang.org/std/time/struct.Instant.html>
- Tokio time: <https://docs.rs/tokio/latest/tokio/time/>
- Tokio paused time: <https://docs.rs/tokio/latest/tokio/time/fn.pause.html>
- PostgreSQL current time:
  <https://www.postgresql.org/docs/current/functions-datetime.html>
- MySQL date/time functions:
  <https://dev.mysql.com/doc/refman/9.1/en/date-and-time-functions.html>
- SQLite date/time functions: <https://www.sqlite.org/lang_datefunc.html>
- `time` crate: <https://docs.rs/time/latest/time/>
- `quanta` evaluation reference: <https://docs.rs/quanta/latest/quanta/>

## Paste-ready implementation goal

```text
/goal Execute docs/private/plans/clock-architecture-reliability-plan.md one
PR-sized item at a time from current origin/main. Preserve the four-domain
contract: wall timestamps for external facts, monotonic instants for local
elapsed policy, shared lease authority plus epoch fencing for distributed
authority, and SequenceNumber/frontiers for durable order. Current SQL leases
use provider transaction time; future consensus leases require a proved
skew/drift and reassignment contract. Do not start until the PPSC owner hands
off overlapping paths and the plan is promoted. Begin with the first
dependency-ready required row, produce deterministic fail-before evidence, test through the
owning Module's production Interface, report actual provider lanes, run focused
gates then repo-required gates, invoke autoreview before shipping, record exact
commands/counts/PR evidence in the ledger, and never replace the design with a
universal TimeProvider, persisted Instant, global fake clock, or shared provider
SQL text.
```
