# Profile-Aware Isolate Runtime Findings

Measurement status: complete with blocked synthetic-await lane.

Runtime-owner correction (2026-07-21): the PIR exact-authority work did not by
itself structurally prove tenant ownership for `None` routing, unscoped
`Script` routing, same-ID tenant recreation, or retained Wasmtime Stores. The
Nimbus runtime tenant-isolation follow-on added mandatory Engine-derived owner
incarnations, owner-partitioned retained pools, compute-owned runtime lanes, and
acknowledged owner/deployment retirement. Routing affinity is now only a
locality dimension; it is never ownership.

## Status

- Active band: PIR5
- Current result: PIR0 measurement gate and PIR1 classification are complete;
  PIR6 is complete. PIR6 wired the shared bootstrap extension registry, web-lean
  snapshot shape proof, node startup-snapshot consumption, code-cache key safety,
  code-cache impact measurement, code-cache v1 closeout decision, and the full
  post-node-snapshot profile matrix rerun. Blocked cooperative synthetic-await
  rows remain implementation prerequisites for PIR2/PIR4. PIR5 has started with
  a measured-RSS-driven density planning Module, V8 boundary memory maintenance
  on warm-pool return, typed warm-runtime condemnation, high/critical idle
  warm-entry pressure eviction, near-heap-limit checked-out runtime
  condemnation, retained-density current-RSS methodology, cgroup v2 pressure
  sampling, pressure-aware prewarm admission, V8-reported external-memory
  accounting, and an explicit IsolateGroup/shared-RO FFI gate. PIR5 remains
  active only on the pointer-compression artifact blocker: the current
  `nimbus/rusty_v8` release lacks the ptrcomp+simdutf assets needed to measure
  the Deno-family build graph.
- Completion gate:
  `bash scripts/verify-profile-aware-isolate-runtime.sh`
- Benchmark target:
  `cargo bench -p nimbus-runtime --bench runtime_pool_modes`

PIR0 did not prove that adaptive profile-aware pooling is ready to ship. It
proved the current defaults are mispriced for Node cold start, fixed one
warm-pool retained-entry metric bug, and exposed a cooperative synthetic-await
stall that must be resolved before warm-reuse or multiplexing is promoted.

## Live Default Inventory

Current inventory after PIR6 closeout:

| Surface | Current fact | Evidence |
| --- | --- | --- |
| Default runtime limits | `RuntimeLimits::default()` uses `RuntimeExecutionModel::CooperativeLocker` and `RuntimePoolKind::WarmPool`. | `crates/nimbus-runtime/src/limits/resources.rs` |
| Web application preset | `application_web_standard()` keeps the default WebStandard isolate target and web-standard grants. | `crates/nimbus-runtime/src/limits/resources.rs` |
| Node application presets | `application_node20/22/24/26()` select Node compatibility targets over the same runtime-limit shape. | `crates/nimbus-runtime/src/limits/resources.rs` |
| Warm-pool partition | Warm entries first live in explicit partitions keyed by runtime-owner class, stable subject, and Engine/storage incarnation. Checkout then requires the exact deployment authority, bundle provenance, runtime lane/shape, permissions, capabilities/services, construction mode, and optional locality discriminator. | `crates/nimbus-runtime/src/retained_state.rs`, `crates/nimbus-runtime/src/backends/v8/warm_pool.rs` |
| Warm-pool lifecycle | Returned warm runtimes run V8 moderate memory-pressure and low-memory notifications before retention; max-reuse and heap-carryover cases are typed condemnation reasons; near-heap-limit callbacks condemn checked-out warm-pool runtimes before retention; the internal high/critical pressure response evicts idle retained entries from injected cgroup/host pressure samples. | `crates/nimbus-runtime/src/backends/v8/lifecycle.rs`, `crates/nimbus-runtime/src/runtime/driver/invocation.rs` |
| Density planner | `RuntimeDensityPlan` consumes measured RSS evidence plus a host/operator runtime-memory budget, reserves active slots, and derives an effective retained warm-pool cap per worker. | `crates/nimbus-runtime/src/limits/density.rs` |
| Node snapshot state | `V8WorkerRuntimePool` now derives `V8RuntimeConstructionMode::StartupSnapshot` for V8 compatibility targets, including Node20/22/24/26. PIR6 reran the matrix and reduced Node startup-snapshot-cache medians to roughly 10-11 ms. | `crates/nimbus-runtime/src/backends/v8/{startup,warm_pool}.rs` |
| Existing benchmark | `runtime_pool_modes` measures profile, workload, pool kind, execution model, synthetic await, owner-partition lookup under tenant/function skew, and acknowledged retirement fanout. | `crates/nimbus-runtime/benches/runtime_pool_modes.rs` |

## Exemplar Cross-Check

| Exemplar | Pattern | PIR0 disposition | Evidence |
| --- | --- | --- | --- |
| OpenWorkers | Tenant/type keyed V8 pools with pinned execution and async waiter behavior. | adopted | Local source under `~/src/github.com/openworkers/*` |
| Supabase Edge Runtime | Worker pool, supervisor separation, explicit service/user runtime split. | adopted | `https://github.com/supabase/edge-runtime` |
| workerd | Context lifecycle and side-channel/security posture shape the safety gates. | adopted | Local source under `~/src/github.com/cloudflare/workerd` |
| Convex backend | Context cache, isolate cleanliness, timeout split, and runtime reset discipline. | adopted | Local source under `~/src/github.com/get-convex/convex-backend` |
| Deno / deno_core | Snapshot, realm, module-loader, and code-cache substrate semantics. | adopted | Local source under `~/src/github.com/denoland/*` |
| Node | Snapshot builder and Environment restore semantics, but not tenant pool sizing. | adopted for snapshot semantics; rejected as a pool-sizing model | Local source under `~/src/github.com/nodejs/node` |
| Blueboat | Bootstrap context lifecycle, cgroup/system memory watermarks, idle GC, and process hardening before user code. | adopted | `https://github.com/losfair/blueboat` |
| isolated-vm | Snapshot provenance warnings, advisory memory limit posture, scheduler separation, and catastrophic-error condemnation. | adopted | `https://github.com/laverdet/isolated-vm` |
| Isolator | CPU time charged around V8 wakeups separately from wall-clock timeout. | adopted | `https://github.com/merlinfuchs/isolator` |

## Measurement Matrix

PIR0 extended `runtime_pool_modes` with:

- WebStandard plus Node20/Node22/Node24/Node26 profile labels.
- `hostless_trivial`, `compute_bound_jit_hot`, and
  `setup_heavy_large_module` pure-JS workloads.
- StartupSnapshotCache versus WarmPool where the execution model permits it.
- RunToCompletion versus CooperativeLocker execution.
- Synthetic host-call await delays of `0`, `1`, `5`, and `50` ms.
- Optional JSONL trace emission through `NIMBUS_PIR0_TRACE_PATH`.

Runnability status:

- Profile matrix: 45 Criterion rows completed.
- Default synthetic-await matrix: 12 Criterion rows completed.
- Blocked synthetic lane: WebStandard cooperative synthetic-await rows and
  synthetic warm-pool await rows are opt-in through
  `NIMBUS_PIR0_INCLUDE_BLOCKED_AWAIT_ROWS=1` because they can stall.

## Findings

Measured on Apple M2 Max, macOS 24.6.0, 32 GiB RAM:

| Profile | Cold avg | Warm avg | Max trace RSS |
| --- | ---: | ---: | ---: |
| WebStandard | 2.03 ms | 0.042 ms | 83.6 MiB |
| Node20 | 209.76 ms | 0.288 ms | 133.1 MiB |
| Node22 | 212.20 ms | 0.299 ms | 153.2 MiB |
| Node24 | 217.80 ms | 0.303 ms | 180.0 MiB |
| Node26 | 220.06 ms | 0.357 ms | 189.2 MiB |

Interpretation:

- The PIR0 baseline measured Node profiles at roughly 210-220 ms cold
  construction because Node targets were unsnapshotted at that checkpoint. PIR6
  wired node startup-snapshot consumption and reran the full profile matrix.
  Node20/22/24/26 startup-snapshot-cache medians now cluster around 10-11 ms;
  WebStandard cold medians remain around 1.8-2.2 ms; warm-pool medians remain
  tens of microseconds.
- PIR6's dedicated setup-heavy code-cache impact rows show a small end-to-end
  win from the existing in-memory per-bundle module code cache. WebStandard
  fresh-bundle invocation measured `[2.2724 ms 2.2957 ms 2.3220 ms]` versus
  primed bundle cache `[2.0905 ms 2.1007 ms 2.1186 ms]`; Node22 fresh-bundle
  invocation measured `[10.586 ms 10.624 ms 10.668 ms]` versus primed bundle
  cache `[10.364 ms 10.415 ms 10.520 ms]`. These rows measure the product
  invocation surface, not isolated V8 parse/compile time.
- Warm reuse removes most of that cost in the pure-JS profile matrix, but the
  synthetic await lane exposed a scheduler/reuse blocker. Warm reuse cannot be
  promoted for async host-call traffic until that blocker has a regression test.
- WebStandard cold start is around 2 ms on this host, so web profile work should
  prioritize correctness, density, side-channel posture, and scheduler safety
  before complicated autoscaling.
- The warm-pool retained-entry gauge was wrong before PIR0 because taking a
  retained entry did not decrement the gauge. PIR0 fixed and tested that metric.

## ROI Ranking

ROI ranking:

| Rank | Band | Reason |
| ---: | --- | --- |
| 1 | PIR1 | Mandatory classifier/admission foundation; low-risk, flag-off, and required before PIR2/PIR6 can safely use profile labels. |
| 2 | PIR6 | Complete. Node startup-snapshot-cache medians now cluster around 10-11 ms instead of the PIR0 210-220 ms unsnapshotted baseline. |
| 3 | PIR2 | Warm reuse is a major throughput win in pure-JS rows, but PIR2 must first fix the blocked cooperative synthetic-await lane and prove cleanliness. |
| 4 | PIR7 | Telemetry/static defaults need the PIR0 data and must encode blocked-lane/host-budget guardrails before adaptivity, but PIR7 depends on PIR4. |
| 5 | PIR5 | Active. Node RSS is materially higher than Web RSS; density work matters after cold-start and scheduler safety are addressed. |
| 6 | PIR3 | Required before untrusted multiplexing; not the top immediate ROI because it gates PIR4 rather than reducing current cold cost. |
| 7 | PIR4 | Multiplexing remains blocked on PIR2 and PIR3; synthetic startup rows do not justify shipping it first. |
| 8 | PIR8 | Still deferred to the sandbox S-band; PIR0 found no reason to pull it forward. |

## Recommended Next Band

Recommended next band: PIR5.

PIR6 has landed and closed the high-ROI cold-start band. PIR2/PIR4 must not ship
until the cooperative synthetic-await warm/reuse blocker has a focused fix and
regression test, and PIR7 depends on PIR4. PIR5 is therefore the next active
eligible band by the current ROI/dependency ledger.
