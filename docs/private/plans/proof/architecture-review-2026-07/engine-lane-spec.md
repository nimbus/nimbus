# Engine Lane Spec — CO1, GR4, GR6, TI3, TI4, TI5, DE11

Design authority: `architecture-review-2026-07-plan.md` rows + the
2026-07-07 engine inventory. Crate scope: `nimbus-engine` (+
`nimbus-testing` for TI3). Pre-launch: breaking changes preferred, no
compat shims. Implement in the order below — later items build on
earlier ones.

## Facts this rests on (inventory, all paths under `crates/nimbus-engine/src/`)

- Three hand-rolled worker scaffolds share a byte-identical skeleton
  (Mutex<VecDeque>+Condvar queue with shutdown-aware `pop`, start guard
  on `Mutex<Option<JoinHandle>>` + `AtomicBool`, named OS thread on a
  `Weak<TenantRuntime>`, self-join-guarded shutdown, `Drop → shutdown`,
  lock-poison `.expect`s): `tenant/subscription_delivery/{worker,queue}.rs`,
  `tenant/trigger_candidates.rs`, `tenant/trigger_execution.rs`.
- Per-worker variance: item type; bounded queue (capacity 256 +
  overflow `Err(work)` + `worker_start_count` + drain-batch 8) only for
  subscription delivery; dedup+sort-by-(ready_at,key) on enqueue +
  `wait_timeout` readiness only for trigger execution; loop bodies and
  retry policies differ; test sidecars differ.
- GR4 divergence: `trigger_candidates.rs:481-497` REQUEUES unprocessed
  commits on store `Err` (split_off + requeue_front + 10ms backoff);
  `trigger_execution.rs:251-254` only `warn!`s and DROPS the key — a
  transient store failure strands the persisted record (possibly
  `Running`) with nothing re-enqueuing it. Business retries
  (`RetryableFailure`, :233-240) are unaffected — only `Result::Err`
  store/I/O failures drop.
- GR6 census: the ONLY non-test `.unwrap()` in the crate is
  `persistence_config.rs:533` (`key_provider().unwrap()` inside
  `validate_encryption`, guarded by `is_enabled` at :528; the coupling
  is implicit). `EncryptionValidationError` (:560-582) has no variant
  for it. `background_executor.rs:32` `.expect("background runtime
  should build")` in a `new() -> Self`. Everything else is test code or
  lock-poison guards. The plan row's "~115 sites" was wrong.
- TI4: prod scheduler is async (`tick_at_async` :82 with
  for_each_concurrent). The `#[cfg(test)]` twins (`tick_at` :73-80,
  `process_tenant` :107-111, `process_due_jobs` :124-183,
  `process_cron_jobs` :259-282, `dispatch_mutation` :318-334) are
  synchronous copies; `engine/scheduler/tests.rs` drives `tick_at` at
  8 call sites (364,373,408,463,501,557,605,804); the async path has
  thin direct coverage (:183,:942 + postgres_provider/scheduler.rs:164).
- TI5: three armed/entered/released condvar pause barriers —
  `subscription_delivery/pause.rs:9-92`,
  `trigger_candidates.rs:52-65,338-416` (adds `release_for_shutdown`,
  called from shutdown :286), `subscriptions/registry.rs:82-168`
  (sequence-gated: `entered_sequence: Option<SequenceNumber>`).
  subscription_delivery's handle STRUCT is prod-visible (methods
  cfg(test)) — the shared type must be constructible from prod code.
- DE11: metrics triple-hop — commit_processing →
  `TenantRuntime::record_subscription_*`
  (subscription_delivery_facade.rs:19-42) →
  `SubscriptionDeliveryQueue::record_*` (subscription_delivery.rs:62-89)
  → `SubscriptionDeliveryMetrics` fetch_adds (stats.rs:51-80). The
  subscribe ladder: 5 public entries collapsing onto
  `subscribe_async_cancellable_with_principal` (subscriptions.rs:251)
  + sync `subscribe_with_principal` (:136). Cron pure-rename aliases
  `list_cron_jobs{,_async}` → `load_cron_jobs{,_async}` (cron.rs:82-92).

## Target design (normative)

### 1. CO1 — `WorkQueue<T>` + `BackgroundWorker` (do first)

New `tenant/background/{queue.rs,worker.rs}` (concept-owned; NOT
`util.rs`):

- `WorkQueue<T>`: Mutex<VecDeque<T>>+Condvar; `new_unbounded()` /
  `new_bounded(capacity)` (capacity adjustable for tests, preserving
  `set_capacity_for_testing`); `enqueue(item) -> Result<(), T>` (full ⇒
  Err only when bounded); `enqueue_with(merge: impl FnOnce(&mut VecDeque<T>))`
  for trigger-execution's dedup+sort (the caller supplies the merge —
  the queue does not know about ready_at); `requeue_front(items)`;
  `pop(shutdown) -> Option<T>` with the existing
  shutdown-check/clear/wait loop; `pop_with_deadline(shutdown, deadline_of:
  impl Fn(&T) -> Option<Timestamp>, clock)` for the ready_at timed wait;
  `notify_all`, `len`, drain-batch helper.
- `BackgroundWorker`: owns `Mutex<Option<JoinHandle>>` +
  `Arc<AtomicBool>`; `start(name, runtime: &Arc<TenantRuntime>, loop_fn:
  impl FnMut(&TenantRuntime) -> LoopStep + Send + 'static)` — generic
  enough that each worker's body stays a closure in its own module; the
  Weak-upgrade dance, start guard, and self-join-guarded `shutdown()`
  live HERE once. `Drop → shutdown`.
- The three workers become adapters: item types, loop bodies, metrics,
  and test sidecars stay in their modules; every line of duplicated
  scaffold is deleted. Preserve observable semantics EXACTLY: bounded
  overflow behavior + `worker_start_count`, drain batch of 8,
  candidates' requeue_front backoff, execution's dedup/sort + timed
  wait, all existing pause/test hooks, thread names.

### 2. TI5 — one `PauseBarrier` (rides the CO1 refactor)

`tenant/pause_barrier.rs`: one armed/entered/released condvar barrier,
prod-visible struct, `#[cfg(test)]` control methods (arm,
wait_until_entered, release) + `release_for_shutdown` available to all
three users (it is correct everywhere, not just candidates). The
sequence-gated registry variant: `PauseBarrier` gains an optional
generic payload `PauseBarrier<S = ()>` where `entered` carries `S`
(registry uses `SequenceNumber`; the other two use `()`). Three
existing barrier implementations deleted; all existing tests keep
passing with only import/constructor updates.

### 3. GR4 — trigger-execution store-failure requeue

On loop-body `Result::Err` in trigger_execution: re-enqueue the key
with `ready_at = clock.now() + TRIGGER_EXECUTION_STORE_RETRY_BACKOFF`
(new const, mirror candidates' 10ms order of magnitude; document why
unbounded retry is correct: the persisted record is the durable truth
and a permanently failing store keeps the whole engine degraded anyway,
matching candidates' posture). Do NOT count store retries against
`MAX_ATTEMPTS` (that budget is for business failures). Required test
uses TI3's counted injector: fail the Nth store call, assert the key is
retried and the invocation ultimately completes with exactly one
terminal record.

### 4. TI3 — counted fault injector (nimbus-testing)

Beside `BlockingFaultInjector` in `nimbus-testing/src/faults.rs`: a
`CountedFaultInjector` (or extend the existing type) supporting
"fail the Nth call" and "fail N times then succeed", with the same
integration surface the engine store fixtures already use. Unit tests
in nimbus-testing assert the count semantics.

### 5. TI4 — delete the sync scheduler twins

Delete `tick_at`/`process_tenant`/`process_due_jobs`/
`process_cron_jobs`/`dispatch_mutation` sync twins; migrate the 8
`engine/scheduler/tests.rs` call sites to `tick_at_async` (tokio
current-thread test runtime — `#[tokio::test]` or a small block_on
helper, match the file's existing async tests at :183/:942). If sync
engine methods lose their last caller, delete them too (pre-launch) —
but ONLY if truly dead workspace-wide (grep first).

### 6. DE11 — facade/ladder/alias cleanup

- Metrics: expose the metrics handle once
  (`TenantRuntime::subscription_metrics() -> &SubscriptionDeliveryMetrics`
  or equivalent); commit_processing records directly; delete the hop
  methods on TenantRuntime and SubscriptionDeliveryQueue.
- Subscribe ladder: one options struct
  (`SubscribeOptions { principal: Option<...>, cancellation: Option<...> }`
  — shape to what callers need) with `subscribe(query, opts)` sync and
  `subscribe_async(query, opts)`; delete the five-entry ladder; update
  all callers.
- Cron: delete the `list_cron_jobs{,_async}` rename aliases; keep one
  name (`load_cron_jobs*` — it matches the storage verb); update
  callers.

### 7. GR6 — the honest unwrap fix + census truth-up

- `persistence_config.rs:533`: add
  `EncryptionValidationError::MissingKeyProvider` (or restructure so
  the enabled-state match yields the provider directly, making the
  invalid state unrepresentable — preferred if it stays small) and
  return it instead of unwrapping. Test: enabled-encryption config with
  a Disabled provider state (construct via whatever path allows it; if
  the type system now prevents it, the compile_fail or the match
  exhaustiveness IS the evidence — record that).
- `background_executor.rs:32`: change `new() -> io::Result<Self>` and
  propagate IF the construction chain can carry it without introducing
  an unwrap at any caller; otherwise keep the `.expect` with a comment
  stating why boot-time panic is the design. Report which.
- Ledger note: record the corrected census (1 real unwrap, not ~115).

## Hard constraints

- Engine mutation-path invariant untouched (all mutations still flow
  through `apply_mutation_with_mode*`).
- No observable scheduling/delivery behavior change except GR4's
  requeue. The pinned subscription/trigger test suites pass unmodified
  except type/import updates.
- Workers stay OS threads with the existing names; `Weak` non-retention
  preserved (a queued worker must never keep a tenant alive).
- nimbus-testing changes are additive (other crates consume it).

## Verification gates (worktree root, report real counts)

```
cargo fmt --all --check
cargo clippy -p nimbus-engine -p nimbus-testing --all-targets -- -D warnings
cargo test -p nimbus-engine
cargo test -p nimbus-testing
cargo test -p nimbus-system      # blast radius: records/scheduler consumers
cargo check -p nimbus-server
```

GR4 is a fail-closed-adjacent change: run the full engine suite, not a
filtered subset. Update ledger rows CO1/GR4/GR6/TI3/TI4/TI5/DE11 with
evidence on completion.
