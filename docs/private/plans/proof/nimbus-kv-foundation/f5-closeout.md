# NKV0 F5 Closeout Proof

Date: 2026-06-27
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/nkv-cloudflare-foundation`
Branch: `codex/nkv-cloudflare-foundation`
Implementation commit: `f2382cd1f`
Verifier/branch-CI commit: `29f0bc4ef`
Dependency-audit closeout commit: `ee409877c`
Base commit: `f960fd170`
Status: implementation complete locally; final ledger closeout pending branch CI.

## Delivered Surface

- Added authenticated `NIMBUS.READY`, distinct from `PING`.
- Added authenticated `NIMBUS.METRICS` text output with:
  - `connected_clients`
  - `cache_hits`
  - `cache_misses`
  - `cache_hit_ratio_ppm`
  - `durable_writes_started`
  - `durable_writes_completed`
  - `durable_writes_in_flight`
  - `durable_write_latency_us_total`
  - per-command `command.<NAME>.calls`, `.errors`, and `.latency_us_total`
- Metrics are shared between the listener and the `NimbusKvStore` used by that
  listener, so cache and durable-write counters are recorded at the operation
  boundary.
- Added operator/developer doc at `docs/private/operating/nimbus-kv.md`.

## Verification

Command:

```text
cargo fmt --all --check
```

Output: passed with no diff after rebasing onto `f960fd170`.

Command:

```text
git diff --check
```

Output: passed with no whitespace errors after rebasing onto `f960fd170`.

Command:

```text
make deny
```

Output:

```text
bash scripts/single-flight.sh --key cargo-deny-check -- cargo deny check
advisories ok, bans ok, licenses ok, sources ok
```

Notes:

- Lock-only patch updates: `crypto-bigint 0.7.3 -> 0.7.5`,
  `hybrid-array 0.4.10 -> 0.4.12`, `typenum 1.19.0 -> 1.20.1`, and
  `quinn-proto 0.11.14 -> 0.11.15`.
- `RUSTSEC-2026-0118` / `RUSTSEC-2026-0119` remain explicit audited
  Deno-fork exceptions because the safe hickory path is outside the pinned Deno
  2.9 `hickory-resolver 0.25` lane.

Command:

```text
make check
```

Output:

```text
bash scripts/single-flight.sh --key cargo-check-workspace -- cargo check --workspace
Finished `dev` profile [unoptimized + debuginfo] target(s) in 13.43s
```

Command:

```text
make clippy
```

Output:

```text
bash scripts/single-flight.sh --key cargo-clippy-workspace -- cargo clippy --workspace --all-targets -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 16.33s
```

## Focused Implementation Proof

Focused proof below was captured before the clean rebase onto `f960fd170`; the
tracked implementation is unchanged on the rebased commits listed above.

Command:

```text
cargo check -p nimbus-runtime -p nimbus-storage -p nimbus-kv
```

Output:

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 01s
```

Command:

```text
cargo test -p nimbus-kv
```

Output:

```text
cache_tiering: 5 passed; 0 failed; 0 ignored; finished in 2.60s
resp_server: 6 passed; 0 failed; 0 ignored; finished in 0.17s
spawn_harness: 0 passed; 0 failed; 1 ignored
doc-tests: 0 passed; 0 failed
```

Command:

```text
cargo test -p nimbus-storage kv::tests
```

Output:

```text
running 6 tests
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 304 filtered out; finished in 0.19s
```

Command:

```text
cargo build -p nimbus-bin --bin nimbus
```

Output:

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 23s
```

Command:

```text
REDISRS_SERVER_BIN=target/debug/nimbus cargo test -p nimbus-kv --test spawn_harness -- --ignored --nocapture
```

Output:

```text
running 1 test
test redis_rs_spawned_nimbus_kv_binary_smoke_resp2_and_resp3 ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.09s
```

Command:

```text
REDISRS_SERVER_BIN=/Users/jack/src/github.com/nimbus/nimbus-worktrees/nkv-cloudflare-foundation/target/debug/nimbus bash scripts/nimbus-kv-conformance.sh
```

Output:

```text
RESP2:
Test Summary: 1 passed, 0 failed
RESP2 passing behavioral assertions: 1 (minimum 1)

RESP3:
Test Summary: 1 passed, 0 failed
RESP3 passing behavioral assertions: 1 (minimum 1)
```

Command:

```text
bash scripts/verify-nimbus-kv-foundation.sh
```

Output:

```text
[8] F5: smoke, operator doc, ledger, and CI
        note: latest ci.yml for codex/nkv-cloudflare-foundation is status=queued conclusion=none run=28308846727
  FAIL  F5 closeout incomplete
        smoke=1 doc=1 ledger=0 ci=0 ci_branch=codex/nkv-cloudflare-foundation

8 passed, 1 failed
```

## 2026-06-28 Runtime V8 Lane Serialization

CI run `28329115446` still failed after the waitUntil repair because the
main `nimbus_runtime` test binary terminated with `signal: 5, SIGTRAP`
after a sequence of V8-heavy isolate tests. The directly adjacent parent test
passed in isolation, which pointed at process-local V8 state rather than a
single assertion failure. The fix serializes the dedicated runtime lane with
`--test-threads=1` and moves the three snapshot lifecycle stress bodies behind
subprocess-isolated child tests while preserving the parent test names.

Commands:

```text
cargo test -p nimbus-runtime snapshot_seeded_runtime_driver_cycles_survive -- --nocapture --test-threads=1
make test-rust-runtime
cargo fmt --all --check
git diff --check
```

Results:

```text
snapshot lifecycle parents: 3 passed, 0 failed, 1234 filtered out
make test-rust-runtime: lib 440 passed, 0 failed, 117 ignored, 680 filtered; embedded anchor 1 passed; locker smoke 8 passed; doctests 0 passed, 4 ignored
cargo fmt --all --check: passed
git diff --check: passed
```

Committed and pushed as:

```text
85c3ed49f stabilize runtime V8 test lane
```

Manual workflows dispatched after commit
`85c3ed49f304e29bb6375231d30b7e4021027528`:

- CI: `https://github.com/nimbus/nimbus/actions/runs/28330285151`
- Nimbus KV Conformance:
  `https://github.com/nimbus/nimbus/actions/runs/28330285140`

`Nimbus KV Conformance` run `28330285140` succeeded. CI run `28330285151`
fixed the runtime lanes but exposed a real NKV cache-ordering failure in
`Rust Workspace Tests (shard 2/3)`:
`nimbus-kv::cache_tiering concurrent_incr_cache_coherency_disk_backed_and_no_disk`
read cached value `397` while durable state had advanced to `400`.

## 2026-06-28 NKV Cache Mutation Ordering Repair

`TenantKvStore::kv_update` was durable-atomic, but `NimbusKvStore::incr`
updated the cache after the engine write without serializing concurrent cache
publish order. Concurrent writers could commit durable `INCR` operations in
one order and call `cache_put` in another, allowing an older value to overwrite
the newest cached value. The fix serializes mutating NKV operations around the
engine mutation plus cache update.

Commands:

```text
cargo test -p nimbus-kv --test cache_tiering concurrent_incr_cache_coherency_disk_backed_and_no_disk -- --nocapture
for i in 1 2 3 4 5; do cargo test -p nimbus-kv --test cache_tiering concurrent_incr_cache_coherency_disk_backed_and_no_disk -- --nocapture || exit 1; done
cargo test -p nimbus-kv --test cache_tiering -- --nocapture
cargo fmt --all --check
git diff --check
```

Results:

```text
focused cache coherency test: 1 passed, 0 failed, 4 filtered out
focused cache coherency loop: 5/5 passed
cache_tiering test file: 5 passed, 0 failed
cargo fmt --all --check: passed
git diff --check: passed
```

Committed and pushed as:

```text
6d146e838 serialize kv cache mutations
```

Manual workflows dispatched after commit
`6d146e83869dae5e00a32d7bbed8d2c46145661f`:

- CI: `https://github.com/nimbus/nimbus/actions/runs/28330748013`
- Nimbus KV Conformance:
  `https://github.com/nimbus/nimbus/actions/runs/28330748012`

`Nimbus KV Conformance` run `28330748012` succeeded. CI run `28330748013`
fixed workspace shard 2 and the runtime lanes but exposed a seeded Convex
usage test budget failure in `Rust Workspace Tests (shard 3/3)`:
`convex_http_demo_faulted_seeded_usage_scenario_matches_model` timed out after
the journal fault was released.

## 2026-06-28 Seeded Usage Fault Recovery Budget Repair

The exact seeded Convex usage test passed locally in 2.74s, leaving almost no
margin under the seeded helper's 3s request timeout. The analogous non-seeded
faulted overlap already uses a 5s recovery budget. The fix keeps normal seeded
requests on the existing 3s budget and applies a separate 5s budget only after
the faulted apply path is released.

Commands:

```text
cargo test -p nimbus-server convex_http_demo_faulted_seeded_usage_scenario_matches_model -- --nocapture
cargo fmt --all --check
git diff --check
```

Results:

```text
faulted seeded Convex usage test: 1 passed, 0 failed, 482 filtered out
cargo fmt --all --check: passed
git diff --check: passed
```

Committed and pushed as:

```text
18647e3de stabilize seeded usage fault recovery
```

Manual workflows dispatched after commit
`18647e3de5f919e47ef13bb29a1d475ad2cf66c9`:

- CI: `https://github.com/nimbus/nimbus/actions/runs/28331361216`
- Nimbus KV Conformance:
  `https://github.com/nimbus/nimbus/actions/runs/28331361209`

Completed workflow results:

- CI run `28331361216`: completed `success` with 29 jobs.
- Nimbus KV Conformance run `28331361209`: completed `success`.

F5 verifier after marking the plan ledger done:

```text
bash scripts/verify-nimbus-kv-foundation.sh
```

Output:

```text
[8] F5: smoke, operator doc, ledger, and CI
  PASS  smoke proof/test, operator doc, ledger clean, CI green

9 passed, 0 failed
```

## 2026-06-28 Runtime Harness Isolation Repair

The follow-up `Rust Runtime Tests` failure on CI run `28329115446` no longer
reported the stalled waitUntil assertion. It aborted the runtime test binary
with:

```text
Unknown external reference ...
process didn't exit successfully: ... nimbus_runtime ... (signal: 5, SIGTRAP)
```

The log showed every reported `isol_*` pool-reuse parent passing until the
script-affinity parent window, then a raw V8 crash instead of a Rust test
failure. Running that parent alone passed. Serializing the runtime lane exposed
a second order-dependent in-process V8 crash in
`snapshot_seeded_runtime_driver_cycles_survive_on_current_thread_runtime_with_delayed_async_host`.
That test also passed standalone. The repair keeps the runtime lane serial and
moves the snapshot-seeded lifecycle bodies into the same subprocess-isolated
parent/child pattern used by other V8-sensitive tests, preserving the parent
test names in the normal CI bucket.

Commands:

```text
cargo test -p nimbus-runtime isol_warm_context_recycle_preserves_script_affinity_for_distinct_bundle_entries -- --nocapture --test-threads=1
make test-rust-runtime
cargo test -p nimbus-runtime snapshot_seeded_runtime_driver_cycles_survive_on_current_thread_runtime_with_delayed_async_host -- --nocapture --test-threads=1
make test-rust-runtime
cargo fmt --all --check
git diff --check
```

Results:

```text
script-affinity parent standalone: 1 passed, 0 failed, 1233 filtered out
first serialized make test-rust-runtime: reproduced snapshot lifecycle SIGTRAP
current-thread snapshot lifecycle parent wiring: 1 passed, 0 failed, 1 ignored, 1235 filtered out
final make test-rust-runtime: lib 440 passed, 0 failed, 117 ignored, 680 filtered out; embedded anchor 1 passed; locker smoke 8 passed; doctests 0 passed, 4 ignored
cargo fmt --all --check: passed
git diff --check: passed
```

Committed and pushed as:

```text
85c3ed49f stabilize runtime V8 test lane
```

Manual workflows dispatched after commit
`85c3ed49f1a57d7bfebb83f7a7d374865b2b26d9`:

- CI: `https://github.com/nimbus/nimbus/actions/runs/28330285151`
- Nimbus KV Conformance:
  `https://github.com/nimbus/nimbus/actions/runs/28330285140`

## 2026-06-28 NKV Cache Mutation Ordering Repair

The fresh CI run on `85c3ed49f` proved the runtime lane repair
(`Rust Runtime Tests` completed success) and `Nimbus KV Conformance` completed
success, but `Rust Workspace Tests (shard 2/3)` failed in the NKV cache-tiering
test:

```text
nimbus-kv::cache_tiering concurrent_incr_cache_coherency_disk_backed_and_no_disk
left: Some([51, 57, 55])
right: Some([52, 48, 48])
```

That is a lost cache update, not a lost durable update: concurrent writers can
commit `INCR` in one order and then call `cache_put` in a different order,
letting an older value overwrite the newest cached value. The fix serializes
NKV mutating operations around engine mutation plus cache update with a
per-store mutation lock.

Commands:

```text
cargo test -p nimbus-kv --test cache_tiering concurrent_incr_cache_coherency_disk_backed_and_no_disk -- --nocapture
for i in 1 2 3 4 5; do cargo test -p nimbus-kv --test cache_tiering concurrent_incr_cache_coherency_disk_backed_and_no_disk -- --nocapture || exit 1; done
cargo test -p nimbus-kv --test cache_tiering -- --nocapture
cargo fmt --all --check
git diff --check
```

Results:

```text
single focused race run: 1 passed, 0 failed, 4 filtered out
focused race loop: 5/5 runs passed
cache_tiering: 5 passed, 0 failed, 0 ignored
cargo fmt --all --check: passed
git diff --check: passed
```

Committed and pushed as:

```text
6d146e838 serialize kv cache mutations
```

Manual workflows dispatched after commit
`6d146e83869dae5e00a32d7bbed8d2c46145661f`:

- CI: `https://github.com/nimbus/nimbus/actions/runs/28330748013`
- Nimbus KV Conformance:
  `https://github.com/nimbus/nimbus/actions/runs/28330748012`

## Pending Closeout

- Branch `ci.yml` run is queued/running on rebased head
  `ee409877c96fc097491d0245f9a485b73f095f4c`:
  `https://github.com/nimbus/nimbus/actions/runs/28308846727`.
- Branch `Nimbus KV Conformance` run on rebased head
  `ee409877c96fc097491d0245f9a485b73f095f4c`:
  `https://github.com/nimbus/nimbus/actions/runs/28308847178`.
  - Attempt 1 failed while downloading `tower-layer` from crates.io:
    `curl failed: [16] Error in the HTTP2 framing layer (Send failure:
    Connection reset by peer)`.
  - Attempt 2 succeeded; the binary build, redis-rs spawned-binary harness, and
    Valkey external mode completed successfully.
- Fresh branch `ci.yml` is not a closeout signal yet. Current failures observed
  on run `28308846727` are outside NKV:
  - `Rust Runtime Tests`: `isol_node_full_fresh_realm_lease_condemns_stalled_wait_until_before_reuse`
    ended with `Unknown external reference ... SIGTRAP`.
  - `Rust Runtime Ptrcomp Check`: `isol_gate_snapshotted_weblean_crashes` saw
    SIGTRAP instead of the expected SIGABRT/SIGBUS crash-control signal, and
    `isol_node_full_fresh_realm_lease_condemns_stalled_wait_until_before_reuse`
    failed with a pending `waitUntil` error.
  - `Rust Workspace Tests (shard 1/3)` and `(shard 2/3)`: queued Convex runtime
    request-drop cancellation tests timed out waiting for queued work admission.
  - `Node FaaS Compatibility`: `application_node22_host_heavy_diagnostic_canary_batch`
    failed because `prisma-engine.mjs` attempted an fs operation from cwd `/`
    outside the allowed root.
- Current-main baseline run `https://github.com/nimbus/nimbus/actions/runs/28308437751`
  at `f960fd170b3d91f0a82d51b48c13d34c4779ec27` already shows the same
  `Rust Runtime Tests`, `Rust Runtime Ptrcomp Check`, workspace shard 1/2, and
  `Node FaaS Compatibility` failures, so those are not introduced by this NKV
  branch.
- Rerun attempt 2 of branch `ci.yml` run `28308846727` repeated the same five
  failed lanes:
  - `Rust Workspace Tests (shard 1/3)` job `83916028283`:
    `dropped_queued_runtime_request_recovers_and_serves_new_work_after_pressure_clears`
    timed out after 20 s waiting for queued runtime mutation admission.
  - `Rust Workspace Tests (shard 2/3)` job `83916028290`:
    `dropped_queued_runtime_request_never_starts_mutation` timed out after 20 s
    waiting for queued runtime mutation admission.
  - `Rust Runtime Tests` job `83916028204`:
    `isol_node_full_fresh_realm_lease_condemns_stalled_wait_until_before_reuse`
    failed before process exit ended in `Unknown external reference ... SIGTRAP`.
  - `Rust Runtime Ptrcomp Check` job `83916028184`:
    `isol_gate_snapshotted_weblean_crashes` saw SIGTRAP instead of expected
    SIGABRT/SIGBUS, and `isol_node_full_fresh_realm_lease_condemns_stalled_wait_until_before_reuse`
    failed with `Promise resolution is still pending but the event loop has
    already resolved`.
  - `Node FaaS Compatibility` job `83916028483`:
    `application_node22_host_heavy_diagnostic_canary_batch` failed because
    `prisma-engine.mjs` attempted a read from cwd `/` outside the allowed
    `/tmp/.../app/.nimbus/convex` root.
- Pre-rebase branch `Nimbus KV Conformance` succeeded:
  `https://github.com/nimbus/nimbus/actions/runs/28308102727`
  (`headSha=c72dfc283fe936a46b6d79c65f1ddda987ca27a9`; binary build,
  redis-rs spawned-binary harness, and Valkey external mode all succeeded).
- Do not flip F5 to `done` or claim `9 passed, 0 failed` until branch CI is
  green and the verifier reports `9 passed, 0 failed`.

## 2026-06-28 Queued Runtime Cancellation Repair

The workspace shard 1/2 failures were traced to stale dispatch-then-hold
assumptions in the queued request-drop path plus a real lifecycle gap: when an
HTTP disconnect dropped the handler future, the queued invocation future could
be dropped before its `select!` cancellation branch withdrew the job from
`RuntimeExecutorAdmission`. The fix adds admission-queue withdrawal by
`invocation_id` and registers a `HostCallCancellation::notify_on_cancel`
listener for queued worker invocations, so request-drop cancellation removes and
accounts the queued job even if the caller future is dropped.

Commands:

```text
cargo fmt --all --check
cargo test -p nimbus-server tests::convex_runtime::cancellation::request_drops::queued -- --nocapture
cargo test -p nimbus-runtime executor::tests::queue_fairness -- --nocapture
cargo test -p nimbus-server tests::convex_runtime::cancellation::request_drops::in_flight::dropped_runtime_http_request_cancels_runtime_invocation -- --exact --nocapture
git diff --check
```

Results:

```text
cargo fmt --all --check: passed
queued request-drop server tests: 2 passed, 0 failed, 481 filtered out
nimbus-runtime queue_fairness tests: 9 passed, 0 failed, 1224 filtered out
in-flight request-drop server test: 1 passed, 0 failed, 482 filtered out
git diff --check: passed
```

The queued tests now assert the intended queue-at-admission behavior:
`worker_dispatched_invocations` stays at 1 while the second request is queued,
dropping that request records one queued cancellation before worker dispatch,
and dropping the blocker records one in-flight cancellation.

Manual workflows dispatched after commit
`cd470b2bc327eb142fe55d8f4bcbb42cc7707387`:

- CI: `https://github.com/nimbus/nimbus/actions/runs/28327347356`
- Nimbus KV Conformance:
  `https://github.com/nimbus/nimbus/actions/runs/28327347860`

Verifier after dispatch:

```text
bash scripts/verify-nimbus-kv-foundation.sh
```

Output:

```text
[8] F5: smoke, operator doc, ledger, and CI
        note: latest ci.yml for codex/nkv-cloudflare-foundation is status=queued conclusion=none run=28327347356
  FAIL  F5 closeout incomplete
        smoke=1 doc=1 ledger=0 ci=0 ci_branch=codex/nkv-cloudflare-foundation

8 passed, 1 failed
```

Completed workflow results:

- `Nimbus KV Conformance` run `28327347860` succeeded on
  `cd470b2bc327eb142fe55d8f4bcbb42cc7707387`.
- CI run `28327347356` completed with conclusion `failure`, but the
  queued-cancellation blocker is fixed in CI:
  - `Rust Workspace Tests (shard 1/3)`: success.
  - `Rust Workspace Tests (shard 2/3)`: success.
  - `Rust Workspace Tests (shard 3/3)`: success.
  - `Rust Clippy`, `Rust Format`, `Rust Dependency Audit`,
    `JavaScript Build and Test`, all external-provider lanes, `Node D-Bus
    Integration`, and all manual verification-harness shards: success.
- Remaining failed CI lanes match the pre-existing current-main runtime/Node
  failure class:
  - `Rust Runtime Tests` job `83919515179`:
    `isol_node_full_fresh_realm_lease_condemns_stalled_wait_until_before_reuse`
    failed and the test process ended with `Unknown external reference ...`
    followed by `signal: 5, SIGTRAP`.
  - `Rust Runtime Ptrcomp Check` job `83919515173`:
    `isol_gate_snapshotted_weblean_crashes` saw SIGTRAP instead of the
    SIGABRT/SIGBUS crash-control signal, and
    `isol_node_full_fresh_realm_lease_condemns_stalled_wait_until_before_reuse`
    failed with `Promise resolution is still pending but the event loop has
    already resolved`.
  - `Node FaaS Compatibility` job `83920438220`:
    `application_node22_host_heavy_diagnostic_canary_batch` failed because
    `prisma-engine.mjs` attempted to read from `/`, outside the allowed
    `/tmp/.../app/.nimbus/convex` root.

Verifier after completed workflows:

```text
bash scripts/verify-nimbus-kv-foundation.sh
```

Output:

```text
[8] F5: smoke, operator doc, ledger, and CI
        note: latest ci.yml for codex/nkv-cloudflare-foundation is status=completed conclusion=failure run=28327347356
  FAIL  F5 closeout incomplete
        smoke=1 doc=1 ledger=0 ci=0 ci_branch=codex/nkv-cloudflare-foundation

8 passed, 1 failed
```

## 2026-06-28 Node FaaS Prisma CWD Repair

The `Node FaaS Compatibility` failure was traced to `node:fs` sync helpers
resolving relative paths through a `node:process` object whose cwd was seeded
as `/` during Node bootstrap/snapshot construction. The runtime capability
policy was later installed for the application bundle, but the patched
process object kept the stale cwd. The fix adds a per-invocation Node process
cwd refresh hook in the runtime reset path and covers sync relative filesystem
operations with a regression in the Node capabilities test.

Commands:

```text
npm ci --prefix tests/runtime/node/host-heavy-canaries
cargo test -p nimbus-runtime application_node22_reads_local_files_hides_non_allowlisted_env_and_denies_escape_writes -- --nocapture
cargo test -p nimbus-runtime application_node22_host_heavy_diagnostic_canary_batch -- --nocapture --test-threads=1 --ignored
cargo test -p nimbus-runtime host_heavy_diagnostic_canary_batch -- --nocapture --test-threads=1 --ignored
cargo fmt --all --check
git diff --check
```

Results:

```text
npm ci --prefix tests/runtime/node/host-heavy-canaries: added 8 packages, audited 9 packages
Node capabilities cwd/fs regression: 1 passed, 0 failed, 1232 filtered out
Node22 host-heavy batch: 1 passed, 0 failed, 1232 filtered out
Node22/Node24/Node26 host-heavy batches: 3 passed, 0 failed, 1230 filtered out
cargo fmt --all --check: passed
git diff --check: passed
```

Committed and pushed as:

```text
091124aa5 fix node runtime cwd refresh
```

## 2026-06-28 Stalled WaitUntil Timeout Repair

The `Rust Runtime Tests` and `Rust Runtime Ptrcomp Check` failures were
reproduced locally with `CI=1`: the fresh-realm waitUntil test received
`Promise resolution is still pending but the event loop has already resolved`
before the 300ms CI system watchdog fired. That happens when waitUntil holds
a no-ref never-resolving promise: deno_core has no event-loop work left to
poll, but Nimbus still must bound the unfinished background phase by the
system timeout and condemn the retained realm lease as `TimedOut`. The fix
classifies only the waitUntil drain pending/event-loop-resolved messages as
`SystemTimeout` and sets the driver timeout flag so lease condemnation remains
`TimedOut`. Ordinary response-promise pending errors are unchanged.

Commands:

```text
cargo test -p nimbus-runtime pir4_wait_until_system_timeout_bounds_unreferenced_pending_background_work -- --nocapture
CI=1 cargo test -p nimbus-runtime isol_node_full_fresh_realm_lease_condemns_stalled_wait_until_before_reuse -- --nocapture --test-threads=1
cargo test -p nimbus-runtime pir4_wait_until -- --nocapture --test-threads=1
CARGO_TARGET_DIR=target/ptrcomp cargo test -p nimbus-runtime isol_gate_snapshotted_weblean_crashes --features v8-pointer-compression -- --nocapture --test-threads=1
CI=1 CARGO_TARGET_DIR=target/ptrcomp cargo test -p nimbus-runtime isol_node_full_fresh_realm_lease_condemns_stalled_wait_until_before_reuse --features v8-pointer-compression -- --nocapture --test-threads=1
cargo fmt --all --check
git diff --check
```

Results:

```text
unreferenced pending waitUntil regression: 1 passed, 0 failed, 1233 filtered out
CI-mode stalled waitUntil parent: 1 passed, 0 failed, 1233 filtered out
serialized pir4_wait_until tests: 5 passed, 0 failed, 1229 filtered out
ptrcomp crash-control parent: 1 passed, 0 failed, 1239 filtered out
ptrcomp CI-mode stalled waitUntil parent: 1 passed, 0 failed, 1239 filtered out
cargo fmt --all --check: passed
git diff --check: passed
```

Committed and pushed as:

```text
084b5760 classify stalled waitUntil drains as timeouts
```

Manual workflows dispatched after commit
`084b5760d2a308538c199ab6bf855f3bc8a128d6`:

- CI: `https://github.com/nimbus/nimbus/actions/runs/28329115446`
- Nimbus KV Conformance:
  `https://github.com/nimbus/nimbus/actions/runs/28329115459`

Verifier immediately after dispatch:

```text
bash scripts/verify-nimbus-kv-foundation.sh
```

Output:

```text
[8] F5: smoke, operator doc, ledger, and CI
        note: latest ci.yml for codex/nkv-cloudflare-foundation is for head=cd470b2bc327eb142fe55d8f4bcbb42cc7707387, local HEAD=084b5760d2a308538c199ab6bf855f3bc8a128d6
  FAIL  F5 closeout incomplete
        smoke=1 doc=1 ledger=0 ci=0 ci_branch=codex/nkv-cloudflare-foundation

8 passed, 1 failed
```
