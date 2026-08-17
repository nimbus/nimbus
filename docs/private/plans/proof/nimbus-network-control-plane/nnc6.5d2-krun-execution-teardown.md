# NNC6.5d2 Krun Execution Teardown

Status: `complete; K1-K25 green; review cadence exhausted`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

## Scope

NNC6.5d2 adds exact Krun execution drain and stop. It uses the sandbox teardown
contract and provider journal from NNC6.5d1. It adds one real compute adapter
and one Krun-owned state machine.

The item does not detach or release an attachment. It does not stop a PEP or
settle a listener. It does not release a port, IPAM, or a segment. It does not
change a Machine API request or cut over a caller. It keeps the coarse stop
authority. It adds no journal and changes no `nimbus-network` dependency.

## Written Acceptance Contract

| ID | Verifiable success criterion |
| --- | --- |
| K1 | The read-only source audit names the current Krun stop, creator, provision, restart, inspection, manifest, lifecycle-lock, provider-journal, compute, and network cleanup authorities. |
| K2 | NNC6.5d1's shared conmon process identity is sufficient without a shared conmon edit. Runtime ID, creator attempt, provider PID, pidfile PID, and process birth authenticate every signal. |
| K3 | `workload_state_root` plus namespace `krun-runtime` remains the sole Krun provider-command journal. The item adds no second command or effect-result authority. |
| K4 | The existing provider-journal observation durably retains a strict backend failure code for `DefiniteFailure`. Exact replay returns the same code. Missing, invalid, crossed-kind, or unknown failure-code fields fail closed. |
| K5 | Compute exposes one real `KrunTeardownAdapter`. It reuses the shared command lowering, journal phase, result mapping, callback fencing, and exact registry selection without fallback. |
| K6 | One required strict Krun manifest field owns independent drain and stop progress. Missing or corrupt outer, drain, or stop state fails closed. No teardown field uses `serde(default)`. |
| K7 | Execute takes the provider stream lock before the exclusive Krun lifecycle lock. Inspect uses provider synchronization before the shared lifecycle lock. No path takes these locks in reverse order. |
| K8 | Before journal mutation or effect, the adapter authenticates provider role, tenant, sandbox, execution attempt, generation, node, desired digest, source digest, network-plan digest, provider-target digest, operation, attempt, and dispatch epoch. |
| K9 | Drain persists an irreversible admission barrier under the lifecycle lock. It sends no signal and changes no runtime or network authority. |
| K10 | Drain succeeds only after creator, activation, restart, provider-failure cleanup, and lifecycle work is settled. Pending or unknown work returns `Ambiguous` from Execute and `InProgress` from Inspect with exact evidence. Drain does not repair that work. |
| K11 | The barrier blocks new provision preparation, provision attachment, activation, creator spawn/release, restart source quiescence, target switch, retained-network attachment, test-only legacy launch, and coarse stop before their first effect. |
| K12 | Stop requires the matching exact drained command subject. It persists stop intent before runtime inspection and keeps the overall manifest and handle `Stopping`. |
| K13 | Stop persists the exact configured graceful-signal-may-exist state before that signal and KILL-may-exist before KILL. It never signals from a raw PID or from crossed, missing, corrupt, recycled, or unknown process evidence. |
| K14 | The configured graceful signal is not redelivered after an ambiguous response. KILL redelivery requires the exact next epoch, strict stop-only retry lineage, a named delay, and the same authenticated live process incarnation. |
| K15 | Stop succeeds only from an exact exit receipt for the current attempt or creator-authenticated explicit provider/process absence. A path-only stale receipt or absent pidfile is not success. |
| K16 | Inspect is read-only and byte-stable. It reports `NotCompleted` only after exact absence proves that an older effect cannot commit. |
| K17 | Drain, stop, replay, ambiguity, contention, and fresh-process recovery leave the attachment, provider handle, netns, PEP, listener, port, IPAM, segment, launch authority, and launch artifact byte-stable. |
| K18 | Wrong or crossed stable identity returns the frozen exact failure code with zero effect. Missing or corrupt durable state is `Ambiguous`. Stale or skipped epochs preserve newer bytes and return the frozen stale or epoch error. |
| K19 | One exact duplicate replays without effect. Two thread contenders and two process contenders produce one signal sequence and one provider result. A live claim cannot start after retry authority advances. |
| K20 | Fresh-process recovery passes the claim, drain, stop-intent, graceful-signal, KILL, terminal-manifest, provider-result, and compute-result crash cuts without an in-memory snapshot. |
| K21 | Real compute substitution executes and inspects drain and stop through a reopened Krun backend and journal. Both callbacks match the complete confirmed command. |
| K22 | `vm/teardown.rs` and concept-owned children own exact execution teardown. The current coarse stop moves intact to a drain-aware child or the proof records a stronger ownership reason. `vm/lifecycle.rs` does not grow beyond its current mixed threshold. |
| K23 | The item retains coarse stop for NNC6.5g and changes no attachment teardown, machine transport, caller cutover, `nimbus-network` effect, or dependency. |
| K24 | Focused Krun teardown, shared journal, compute substitution, schema, runtime identity, concurrency, and crash tests pass. Full affected crates, strict Clippy, warning-denied rustdoc, format, static dependency/effect scans, NNCV035 arithmetic, docs, site, and proof lint pass with exact counts. |
| K25 | Exactly one candidate-frozen GPT-5.6 Sol/xhigh/fast item review runs after K1-K24 are green. Only an accepted executable finding permits one narrow correction review. |

## Read-Only Audit

The audit ran at `79b122bdc49d45a6009c203ee413d454de136c99` with a clean
owner worktree. Three independent packets inspected Krun production source,
Krun tests, and compute substitution. They changed no path.

### Current call graph

```text
SandboxBackend::stop
  -> KrunSandboxBackend::stop_sync
     -> exclusive .nimbus-krun-lifecycle.lock
     -> reconcile creator ownership
     -> execute_stop
        -> persist broad shutdown intent
        -> read raw pidfile PID
        -> TERM / optional KILL
        -> release_network_artifacts(Final)
           -> stop PEP
           -> detach provider and namespace
           -> settle or release listeners, ports, IPAM, segment, attachment
        -> remove launch artifacts
        -> publish Released + Stopped
```

`vm/lifecycle.rs:439-477` combines raw-PID signaling with final network
release. `vm/lifecycle.rs:1179-1228` owns this network cleanup.
`vm/lifecycle.rs:1365-1516` permits terminal publication only after network
release. The same code retires the terminal IPAM witness. Exact stop cannot use
this terminal transition.

The manifest at `vm.rs:669-790` has no exact drain or stop progress. Provision
preparation, attachment, and activation in `vm/provision.rs` have no drain
check. Creator spawn in `vm/creator.rs` has no drain check. Restart source
quiescence, target switch, and retained-network attachment in `vm/restart.rs`
have no drain check.

Krun inspection in `vm/inspection.rs` changes no state. Mutation uses the existing
exclusive lifecycle lock. Inspection uses the existing shared lock. The sole
provider journal already opens under `workload_state_root` with namespace
`krun-runtime` in `vm.rs:282-289`.

### Target call graph

```text
confirmed DrainExecution or StopExecution
  -> KrunTeardownAdapter
  -> existing ProviderCommandAttemptJournal("krun-runtime")
     -> execute_current_claim
        -> exclusive Krun lifecycle lock
        -> authenticate command and strict manifest
        -> drain: persist barrier, prove admitted work settled
        -> stop: persist intent and authenticated signal boundaries
        -> retain every network and launch authority
        -> publish provider result under the same stream lock
  -> compute maps the returned durable observation without a second write
```

The lock order is always provider stream then Krun lifecycle. Exact provider
inspection uses the same order with a shared lifecycle lock.

### Shared seams that remain canonical

- `ProviderCommandAttemptJournal` owns claim, replay, retry lineage, effect
  exclusion, and result publication.
- `backends/conmon/runtime_process.rs` owns exact runtime process identity and
  pidfd signaling. This item needs no shared conmon change.
- `.nimbus-krun-lifecycle.lock` serializes Krun effects and synchronizes
  read-only inspection.
- The Krun manifest owns provider-local effect progress.
- Compute owns command authentication, capability selection, and saga result
  translation.

## Finding And Owner Decision

The audit found that `SandboxExecutionTeardownObservation::DefiniteFailure`
carries a stable code, but `ProviderCommandObservation` persists only the kind
and evidence digest. A backend-local crossed manifest can therefore return
`sandbox_teardown_command_crossed` once and replay later as the generic
`sandbox_teardown_provider_failure`.

NNC6.5d2 extends the existing provider observation envelope with a strict
optional failure code. Teardown `DefiniteFailure` requires a valid code. Other
outcome kinds forbid one. The complete envelope participates in equality,
durable validation, idempotent replay, and result mapping. This correction
keeps one journal and repairs its result contract. It does not add an effect or
coordinator.

This correction owns `crates/nimbus-sandbox/src/provider_command.rs` and its
tests. It also owns the narrow Container consumer and its regression tests.
The shared compute result mapping and tests are in scope. No other provision
or restart producer changes in this item.

## Fail-Before Baseline

Captured at `79b122bdc49d45a6009c203ee413d454de136c99` before a
product-source edit. Each command exited `1`.

| Check | Expected-red result |
| --- | --- |
| `test -f crates/nimbus-sandbox/src/backends/krun/vm/teardown.rs` | No concept-owned Krun exact execution teardown exists. |
| `test -f crates/nimbus-compute/src/workload_saga/teardown_sandbox/krun.rs` | No real Krun compute teardown adapter exists. |
| `rg -q execution_teardown crates/nimbus-sandbox/src/backends/krun/vm.rs` | The Krun manifest has no durable exact teardown state. |
| `rg -q KrunTeardownAdapter crates/nimbus-compute/src/workload_saga/teardown_sandbox.rs crates/nimbus-compute/src/workload_saga/teardown_sandbox` | Compute has no Krun execution teardown capability implementation. |
| `rg -q require_execution_admission_open crates/nimbus-sandbox/src/backends/krun` | Krun provider entry points do not enforce a durable drain barrier. |
| `rg -q capture_runtime_process_identity crates/nimbus-sandbox/src/backends/krun` | Krun stop does not use the shared authenticated process seam. |

Source inspection also proves that current `execute_stop` calls raw
`read_pid` and `signal_process`, then `release_network_artifacts(Final)`.
Existing explicit-stop tests prove the combined coarse behavior. They cannot
serve as exact execution-only stop proofs.

### Accepted full-review fail-before proofs

The one candidate-frozen full item review found two executable defects. Both
were accepted and received deterministic regressions before product-source
correction:

| Finding | Expected-red proof |
| --- | --- |
| P1, confidence `0.99`: provision used a pre-lock manifest snapshot for drain admission. | `krun_execution_drain_fences_creator_activation_restart_and_launch_admission` exited `101`. A preparation worker read `Open`, contended on the real lifecycle lock, then crossed a durable drain barrier and failed at a later stale-plan check instead of the drain fence. The same bounded harness owns preparation, attachment, and activation. |
| P2, confidence `0.94`: an adjacent graceful-stop retry returned before rebasing durable progress to its current fence. | `krun_execution_stop_replay_never_duplicates_a_signal` exited `101`. After an authorized epoch-2 adoption under a backward clock step, durable progress still named epoch 1 and could not authorize epoch 3. |

The corrected tests use the real lifecycle-lock contention probe, bounded
channels, the real provider journal, exact durable progress, and byte-stable
state checks. They add no sleep-based ordering and no new test-roster entry.

## Frozen Implementation Boundaries

### Compute

Keep `teardown_sandbox.rs` as the shared lowering and result owner. Rename the
private phase adapter only if the new provider child needs a precise shared
name. Add `teardown_sandbox/krun.rs`, its attributed tests, and the narrow
public export. `KrunTeardownAdapter::new` gets the journal from
`KrunSandboxBackend::attempt_idempotency_journal`. It does not open a journal.

Do not change the registry, dispatcher, confirmed command, or callback
protocol unless a written K criterion first proves the shared seam
insufficient. Do not add a generic backend trait, macro state machine, god
provider, fallback, or no-op capability.

### Krun provider

Use this concept-owned module structure:

```text
vm/teardown.rs          command authentication and lifecycle composition
vm/teardown/state.rs    strict drain and stop progress
vm/teardown/effects.rs  runtime observation and authenticated signal adapter
vm/teardown/tests.rs    deterministic provider behavior
vm/teardown/tests/*     process, crash, retry, and recovery proofs
```

Move the intact coarse stop composition to a drain-aware concept child if that
keeps `vm/lifecycle.rs` below 1,500 lines without changing coarse behavior.
The later NNC6.5g deletion gate remains its owner.

The exact stop result keeps `shutdown_requested=true`, both status fields at
`Stopping`, and `launch_authority=ProviderOwned`. It retains the network
config, attachment association, provider handle, netns evidence, PEP, listener
and port leases, IPAM witness, segment hold, and launch artifact.

### Drain admission points

Check the same required manifest state under the exclusive lifecycle lock
before each first effect:

1. provision preparation.
2. provision attachment.
3. provision activation.
4. creator spawn and release.
5. restart source quiescence.
6. restart target switch.
7. restart retained-network attachment.
8. test-only legacy launch.
9. coarse stop.

The backend treats a retained restart record as settled only at `NetworkAttached`.
Its fence must name the current target attempt. Launch authority must be
`ProviderOwned`, and creator handoff is runtime-observed. A pending creator,
provider-failure cleanup, partial restart, or incomplete activation prevents
drain success. Drain observes these owners. It does not repair them.

## Frozen Outcome Rules

| Durable result | Execute | Inspect |
| --- | --- | --- |
| `Succeeded` | Step-specific success | `Satisfied` |
| `DefiniteFailure(code)` | Exact `DefiniteFailure(code)` | Exact `DefiniteFailure(code)` |
| `Absent` | `Ambiguous` | `NotCompleted` |
| `RetryAuthorized` | `Ambiguous` | `NotCompleted` |
| `Claimed` or `InProgress` | `Ambiguous` | `InProgress` |
| `Ambiguous` | `Ambiguous` | `Ambiguous` |

Missing or corrupt manifest or journal state is `Ambiguous`. A crossed stable
identity is `sandbox_teardown_command_crossed`. A stale generation or epoch is
`sandbox_teardown_command_stale`. A skipped epoch or crossed transition is
`sandbox_teardown_epoch_invalid`. Every failure preserves the requested and
current durable bytes and makes zero effect.

## Required Behavior Proofs

The item must add named tests for these contracts:

- exact provider identity, complete pre-journal fence authentication, two-real-
  provider registry selection, and reopened-journal compute substitution.
- strict journal failure-code persistence, validation, and replay.
- durable drain without signal or network mutation.
- pending creator, partial restart, provider cleanup, and activation evidence.
- all nine post-barrier admission rejections.
- exact drain prerequisite for stop.
- stop intent, configured-signal-may-exist, KILL-may-exist, and terminal progress ordering.
- default TERM and a custom named signal, plus crossed creator, pidfile, provider PID, recycled PID,
  stale receipt, and unknown-process rejection.
- exact duplicate, stale live claim, thread contention, process contention,
  and unrelated-stream progress.
- byte-stable populated network and launch authority for every stop state.
- strict manifest schema for the outer field and both nested fields.
- read-only inspection and missing/corrupt durable-state handling.

The fresh-process matrix uses no handed-over state. It cuts after the claim,
drain barrier, stop intent, and graceful-signal boundary. It also cuts after
the graceful-signal effect, KILL boundary, KILL effect, terminal manifest, and
provider result. The final cut occurs before compute result CAS.

Two additional process cuts repeat claim-before-manifest failure and prove one
strict next-epoch retry lineage. The graceful signal never redelivers. KILL can
redeliver only for the same authenticated process and exact authorized epoch.

### Exact Krun provider test roster

Add `27` runnable Krun provider tests and one child-only ignored dispatcher.
Keep semantic checkpoint waits bounded. Do not use sleeps as contention proof.

In-process lifecycle and fencing:

- `krun_execution_drain_persists_barrier_and_keeps_exact_runtime_running`.
- `krun_execution_drain_fences_creator_activation_restart_and_launch_admission`.
- `krun_execution_drain_pending_owner_is_ambiguous_and_byte_stable`.
- `krun_execution_drain_inspection_is_read_only_and_creates_no_lock`.
- `krun_execution_teardown_crossed_locator_matrix_is_zero_effect`.
- `krun_execution_teardown_missing_or_corrupt_manifest_is_ambiguous_and_zero_effect`.
- `krun_execution_stop_requires_the_exact_drain_fence`.
- `krun_execution_stop_persists_intent_before_runtime_inspection`.
- `krun_execution_stop_persists_configured_graceful_signal_before_effect`.
- `krun_execution_stop_persists_kill_may_exist_before_effect`.
- `krun_execution_stop_authenticates_runtime_creator_and_process_birth_before_signal`.
- `krun_execution_stop_rejects_raw_recycled_or_crossed_pid_before_signal`.
- `krun_execution_stop_adopts_exact_exit_receipt_or_explicit_absence`.
- `krun_execution_stop_keeps_present_unknown_and_missing_evidence_nonterminal`.
- `stale_krun_exit_receipt_cannot_satisfy_successor_execution_stop`.
- `krun_execution_stop_replay_never_duplicates_a_signal`.
- `delayed_krun_stop_claim_fails_before_manifest_or_effect_after_epoch_advances`.
- `krun_live_claim_publishes_result_before_releasing_provider_journal_lock`.
- `krun_execution_teardown_retains_populated_network_authority_byte_stable`.

Thread contention:

- `two_krun_drain_contenders_publish_one_barrier`.
- `two_krun_stop_contenders_dispatch_one_signal_for_one_epoch`.

Strict manifest schema:

- `manifest_deserialization_requires_explicit_execution_teardown`.
- `manifest_deserialization_requires_explicit_execution_drain`.
- `manifest_deserialization_requires_explicit_execution_stop`.
- `manifest_deserialization_rejects_unknown_execution_teardown_phase`.

Real-process recovery:

- `fresh_process_krun_execution_teardown_contenders_share_one_claim_and_signal`.
- `fresh_process_krun_execution_teardown_recovers_all_provider_crash_cuts`.
- `krun_execution_teardown_process_child`, marked ignored because only the two
  parent tests invoke it.

The Krun process matrix does not repeat the generic five-process journal
lineage proof. It keeps the existing shared journal and compute-CAS process
proofs green. Real signal tests use owned child processes. A fixture-created
receipt, shell state text, or direct exit-file write can prove translation,
but it cannot prove signal authority.

The final Krun inventory contains `177` test attributes and `4` child-only
ignores. The exact provider roster is `27` runnable tests plus one child-only
dispatcher. Existing coarse-stop tests remain regression tests. They do not
count as exact execution-only teardown proof because they release network
authority.

## Implemented Result

- Provider observation envelope v4 retains and validates the exact teardown
  `DefiniteFailure` code. Replay compares the code as part of the durable
  result, and other outcomes reject one.
- `KrunTeardownAdapter` reuses the shared compute lowering, result mapping,
  callback fencing, registry, and the backend-opened `krun-runtime` journal.
  The registry substitution proof installs real Container and Krun providers.
- One strict manifest field owns independent drain and stop state. Its outer
  field and both nested fields reject absence, corruption, and unknown phases.
- Drain persists its admission barrier before it inspects admitted creator,
  activation, restart, provider-cleanup, or lifecycle work. All nine producer
  entry points reject the durable barrier before their first effect.
- Provision preparation, attachment, and activation re-read and reauthenticate
  the manifest after acquiring the lifecycle lock. A drain winner between the
  initial identity read and lock acquisition therefore fences the stale entry
  before its first provider effect.
- Stop requires the exact drained subject, persists intent and both
  may-exist signal boundaries, and signals only an authenticated creator
  attempt, provider PID, pidfile PID, and process birth. Adjacent journal
  retry lineage is the only older-progress authority.
- An adjacent graceful-stop retry that observes the same live process before
  its stored deadline persists the same may-exist state with the current
  fence. A backward wall-clock step cannot strand progress two epochs behind.
- The host runtime does not treat the legacy path-only integer exit receipt as
  attempt-qualified evidence. It accepts current creator-authenticated
  explicit absence and leaves `last_exit_code` unset. The substitution seam
  proves the exact-receipt branch separately.
- Exact stop leaves the populated attachment, provider handle, namespace,
  PEP, listener, port, IPAM, segment, launch authority, and launch artifact
  byte-stable. Coarse stop remains intact in `vm/coarse_stop.rs` for NNC6.5g.

### Modularity decisions

`vm/lifecycle.rs` is `1,377` lines after moving the intact coarse-stop owner.
All production files added for exact teardown are below `1,000` lines except
the existing shared provider journal at `1,510` lines. That file is an
explicit concept-owned exception: it keeps claim validation, retry lineage,
authenticated persistence, store locking, and result publication in one
journal state-machine authority. Splitting eleven lines of the same invariant
would add a switchboard boundary without creating a substitutable concept.

`vm/teardown/tests.rs` is `1,786` handwritten test lines and is also an
explicit concept-owned exception. It owns one fixture and the deterministic
in-process state-machine contract; the distinct process/crash concept is
already isolated in `tests/fresh_process.rs`. A closeout-only split would
duplicate fixture authority without reducing production complexity.

## Verification Contract

Run focused gates during implementation. Run the full item review only after
all other gates pass and the candidate is frozen.

```sh
cargo test -p nimbus-sandbox --lib provider_command -- --test-threads=1
cargo test -p nimbus-sandbox --lib backends::container::runtime::teardown::tests -- --test-threads=1
cargo test -p nimbus-sandbox --lib backends::krun::vm::teardown::tests -- --test-threads=1
cargo test -p nimbus-sandbox --lib backends::conmon::runtime_process::tests -- --test-threads=1
cargo test -p nimbus-sandbox --lib backends::krun::vm::tests::manifest_schema -- --test-threads=1
cargo test -p nimbus-compute teardown_sandbox -- --test-threads=1
cargo test -p nimbus-server teardown_driver_process -- --test-threads=1
cargo test -p nimbus-sandbox --all-features -- --test-threads=1
cargo test -p nimbus-compute --all-features
cargo clippy -p nimbus-sandbox -p nimbus-compute --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p nimbus-sandbox -p nimbus-compute --no-deps
cargo fmt --all --check
git diff --check
bash scripts/nimbus-network-control-plane/workload-teardown-contract.sh --self-test
bash scripts/nimbus-network-control-plane/workload-teardown-contract.sh --check
bash scripts/verify-nimbus-network-control-plane.sh
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
```

Also record the exact `nimbus-network -> nimbus-core` workspace edge and the
forbidden effect scan. Record the changed-file line census and NNCV035
arithmetic. Record the proof lint, exact test counts, skipped tests, candidate
tree identity, and all review-finding dispositions.

## Evidence Ledger

| Checkpoint | Evidence |
| --- | --- |
| Read-only audit | Three independent audits covered Krun production, Krun tests, and compute substitution at clean `79b122bdc49d`; zero paths changed. |
| Fail-before | Six source conditions exited `1`; current coarse stop still uses raw PID signaling and final network release. |
| Shared-seam decision | The existing provider observation gains strict durable failure-code retention. No second journal is permitted. |
| Shared conmon | NNC6.5d1 process identity and pidfd signaling are sufficient; no shared conmon edit is authorized. |
| Focused behavior | Exact Krun teardown passes `27` plus one child-only ignore; full Krun VM passes `173` plus four ignores; provider journal passes `25` plus one ignore; runtime identity passes `6`; Krun manifest schema passes `5`; Container regression passes `19` plus one ignore; compute substitution passes `11`; server compute-CAS process recovery passes `2` plus one child-only ignore. |
| Full affected behavior | Serialized sandbox passes `1,078` with `45` declared ignores from an exact `1,123`-test inventory. Compute passes `357` with one child-only ignore. |
| Quality gates | Strict all-target/all-feature Clippy and warning-denied rustdoc pass for sandbox and compute. Format and diff checks pass. |
| Static gates | NNCV035 self-test passes `55/55`. The direct gate is expected `0/7`. The NNCV015 line census was advanced by three lines for four unchanged Krun constructor occurrences and passes. Aggregate verification is `35/36`, with only NNCV035 red. NNCV000-NNCV034 pass. |
| Dependency/effect boundary | No Cargo file changed. NNCV004 proves the sole `nimbus-network -> nimbus-core` workspace edge; NNCV012 proves no forbidden network dependency or provider effect. Exact teardown contains no detach/release effect. |
| Modularity | `vm/lifecycle.rs` is `1,377` lines. The `1,510`-line shared journal and `1,786`-line concept test owner have explicit single-authority reasons above; all other new production concept files are below `1,000` lines. |
| Documentation gates | Proof/ledger text and diff checks pass. Docs pass `108` pages. Site verification passes `17/17`. |
| Accepted correction behavior | Both deterministic regressions failed before correction and pass after it. Corrected exact Krun teardown passes `27 + 1`; full Krun VM passes `173 + 4`; serialized full sandbox remains `1,078 + 45` from `1,123`. |
| Accepted correction quality | Strict all-target/all-feature sandbox Clippy, warning-denied sandbox rustdoc, format, and diff checks pass after the executable correction. |
| Accepted correction static/docs | NNCV035 self-test passes `55/55`; direct remains expected `0/7`; aggregate is `35/36` with only NNCV035 red. Docs pass `108` pages and site verification passes `17/17`. |
| Review cadence | The one full Sol/xhigh/fast item review ran on thread `019fe916-d4c5-79b0-91bf-38cac9c5abf4` over a `266,439`-byte bundle. It reported the accepted P1 and P2 defects above and classified the candidate incorrect at confidence `0.97`. The one narrow Sol/xhigh/fast correction review ran on thread `019fe935-23dd-73c2-8170-c5a08e5f77a3` over a `280,464`-byte bundle and reported zero findings, correct at confidence `0.99`. Review cadence is exhausted; no further review is warranted. |
| Reviewed correction identity | Candidate tree `4dd82b219dfb8f8e46194da7e12da2d8fe757f3c`; `31` paths; raw staged binary patch `276,429` bytes; SHA-256 `9af49413c28fe982a6a41cc8d032c31a96ebbbc9221cd73242c1ade4b5356b8d`. |

## Acceptance Matrix

| Criteria | Current result |
| --- | --- |
| K1-K3 | `pass`; the read-only source and seam audit is durable at `04bf5f7c39a51ddad61d31b1d0934c19950dc1d4`. |
| K4-K23 | `pass`; focused behavior, owner inspection, line census, dependency scan, and effect scan satisfy every written criterion. |
| K24 | `pass`; affected behavior, quality, static, arithmetic, proof, docs, and site gates are green after correction. Unchanged pre-review focused and full-compute evidence remains green. |
| K25 | `pass`; the one full review's two accepted executable findings have deterministic fail-before proofs and corrected gates. The one narrow correction review is clean at `0.99`. No further review is warranted. |
